use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use serde::{Deserialize, Serialize};

use crate::protocol::SourceUnit;

const CUSTOM_ROOT: &str = "playground/custom_examples";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectOrigin {
    BuiltIn,
    Custom,
}

impl ProjectOrigin {
    pub fn badge(self) -> &'static str {
        match self {
            Self::BuiltIn => "BUILT-IN  VERSIONED",
            Self::Custom => "CUSTOM  LOCAL",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredProject {
    pub id: String,
    pub label: String,
    pub origin: ProjectOrigin,
    pub units: Vec<SourceUnit>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CustomManifest {
    id: String,
    label: String,
    files: Vec<String>,
}

pub struct ProjectStore {
    repo_root: PathBuf,
    custom_root: PathBuf,
}

impl ProjectStore {
    pub fn discover() -> io::Result<Self> {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()?;
        Self::at(repo_root)
    }

    fn at(repo_root: PathBuf) -> io::Result<Self> {
        let custom_root = repo_root.join(CUSTOM_ROOT);
        fs::create_dir_all(&custom_root)?;
        Ok(Self {
            repo_root,
            custom_root,
        })
    }

    pub fn load_custom(&self) -> io::Result<Vec<StoredProject>> {
        let mut directories = fs::read_dir(&self.custom_root)?
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        directories.sort();
        directories
            .into_iter()
            .map(|directory| self.load_one(&directory))
            .collect()
    }

    pub fn create_custom(&self, existing_ids: impl Iterator<Item = String>) -> StoredProject {
        let ids = existing_ids.collect::<std::collections::BTreeSet<_>>();
        let ordinal = (1_u64..)
            .find(|ordinal| !ids.contains(&format!("custom-{ordinal}")))
            .expect("custom project id space exhausted");
        let id = format!("custom-{ordinal}");
        StoredProject {
            id: id.clone(),
            label: format!("Untitled {ordinal}"),
            origin: ProjectOrigin::Custom,
            units: vec![SourceUnit {
                path: format!("{CUSTOM_ROOT}/{id}/RUN.bn"),
                source: concat!(
                    "document: Document/new(\n",
                    "    root: Element/label(\n",
                    "        element: []\n",
                    "        style: [width: Fill, height: Fill, align: [row: Center]]\n",
                    "        label: TEXT { New Boon example }\n",
                    "    )\n",
                    ")\n",
                )
                .to_owned(),
            }],
        }
    }

    pub fn save_custom(&self, project: &StoredProject) -> io::Result<()> {
        validate_custom_id(&project.id)?;
        if project.origin != ProjectOrigin::Custom {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "built-in project cannot be written to custom storage",
            ));
        }
        let directory = self.custom_root.join(&project.id);
        fs::create_dir_all(&directory)?;
        let mut files = Vec::with_capacity(project.units.len());
        for unit in &project.units {
            let name = safe_file_name(&unit.path)?;
            atomic_write(&directory.join(&name), unit.source.as_bytes())?;
            files.push(name);
        }
        let manifest = toml::to_string_pretty(&CustomManifest {
            id: project.id.clone(),
            label: project.label.clone(),
            files,
        })
        .map_err(io::Error::other)?;
        atomic_write(&directory.join("example.toml"), manifest.as_bytes())
    }

    pub fn save_built_in(&self, project: &StoredProject) -> io::Result<()> {
        if project.origin != ProjectOrigin::BuiltIn {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "custom project must be written to custom storage",
            ));
        }
        let examples_root = self.repo_root.join("examples").canonicalize()?;
        for unit in &project.units {
            let relative = Path::new(&unit.path);
            if relative.is_absolute()
                || relative
                    .components()
                    .any(|component| matches!(component, Component::ParentDir))
            {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("refusing to write outside repository: {}", unit.path),
                ));
            }
            let path = self.repo_root.join(relative);
            let parent = path.parent().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "source path has no parent")
            })?;
            if !parent.canonicalize()?.starts_with(&examples_root) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("built-in source is not under examples/: {}", unit.path),
                ));
            }
            atomic_write(&path, unit.source.as_bytes())?;
        }
        Ok(())
    }

    pub fn remove_custom(&self, id: &str) -> io::Result<()> {
        validate_custom_id(id)?;
        let directory = self.custom_root.join(id);
        if directory.exists() {
            fs::remove_dir_all(directory)?;
        }
        Ok(())
    }

    fn load_one(&self, directory: &Path) -> io::Result<StoredProject> {
        let manifest: CustomManifest =
            toml::from_str(&fs::read_to_string(directory.join("example.toml"))?)
                .map_err(io::Error::other)?;
        validate_custom_id(&manifest.id)?;
        if directory.file_name().and_then(|name| name.to_str()) != Some(&manifest.id) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "custom example directory does not match manifest id",
            ));
        }
        let units = manifest
            .files
            .iter()
            .map(|file| {
                let name = safe_file_name(file)?;
                Ok(SourceUnit {
                    path: format!("{CUSTOM_ROOT}/{}/{name}", manifest.id),
                    source: fs::read_to_string(directory.join(name))?,
                })
            })
            .collect::<io::Result<Vec<_>>>()?;
        if units.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "custom example has no source files",
            ));
        }
        Ok(StoredProject {
            id: manifest.id,
            label: manifest.label,
            origin: ProjectOrigin::Custom,
            units,
        })
    }
}

#[derive(Clone, Debug)]
pub enum PersistRequest {
    Save(StoredProject),
    Remove(String),
}

#[derive(Clone, Debug)]
pub struct PersistResult {
    pub project_id: String,
    pub result: Result<(), String>,
}

pub struct PersistenceWorker {
    input: Option<mpsc::Sender<PersistRequest>>,
    output: mpsc::Receiver<PersistResult>,
    thread: Option<JoinHandle<()>>,
}

impl PersistenceWorker {
    pub fn new() -> io::Result<Self> {
        let store = ProjectStore::discover()?;
        let (input_tx, input) = mpsc::channel();
        let (output_tx, output) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("boon-dev-persistence".to_owned())
            .spawn(move || {
                while let Ok(request) = input.recv() {
                    let (project_id, result) = match request {
                        PersistRequest::Save(project) => {
                            let id = project.id.clone();
                            let result = match project.origin {
                                ProjectOrigin::BuiltIn => store.save_built_in(&project),
                                ProjectOrigin::Custom => store.save_custom(&project),
                            };
                            (id, result)
                        }
                        PersistRequest::Remove(id) => {
                            let result = store.remove_custom(&id);
                            (id, result)
                        }
                    };
                    if output_tx
                        .send(PersistResult {
                            project_id,
                            result: result.map_err(|error| error.to_string()),
                        })
                        .is_err()
                    {
                        return;
                    }
                }
            })?;
        Ok(Self {
            input: Some(input_tx),
            output,
            thread: Some(thread),
        })
    }

    pub fn submit(&self, request: PersistRequest) -> Result<(), String> {
        self.input
            .as_ref()
            .ok_or_else(|| "persistence worker stopped".to_owned())?
            .send(request)
            .map_err(|_| "persistence worker stopped".to_owned())
    }

    pub fn try_result(&self) -> Option<PersistResult> {
        self.output.try_recv().ok()
    }
}

impl Drop for PersistenceWorker {
    fn drop(&mut self) {
        self.input.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn validate_custom_id(id: &str) -> io::Result<()> {
    if id.starts_with("custom-")
        && id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid custom example id `{id}`"),
        ))
    }
}

fn safe_file_name(path: &str) -> io::Result<String> {
    let path = Path::new(path);
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty() && *name != "example.toml")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid source file name"))?;
    if name.contains('/') || name.contains('\\') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "source file name contains a separator",
        ));
    }
    Ok(name.to_owned())
}

fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temporary = path.with_extension(format!(
        "{}.tmp",
        path.extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("file")
    ));
    fs::write(&temporary, bytes)?;
    fs::rename(temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_projects_round_trip_under_the_repo_local_root_shape() {
        let root = std::env::temp_dir().join(format!(
            "boon-custom-store-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        fs::create_dir_all(root.join("examples")).unwrap();
        let store = ProjectStore::at(root.clone()).unwrap();
        let project = store.create_custom(std::iter::empty());
        store.save_custom(&project).unwrap();
        assert_eq!(store.load_custom().unwrap(), vec![project]);
        fs::remove_dir_all(root).unwrap();
    }
}
