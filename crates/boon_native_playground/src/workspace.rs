use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::protocol::{ApplicationIdentity, SourceUnit};

const CUSTOM_ROOT: &str = "playground/custom_examples";
const CUSTOM_PACKAGE_ID: &str = "dev.boon.playground.custom";
const CUSTOM_DEPLOYMENT_DOMAIN: &str = "local";
const CUSTOM_NAMESPACE_PREFIX: &str = "custom:";
static CUSTOM_NAMESPACE_NONCE: AtomicU64 = AtomicU64::new(0);

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
    pub application: ApplicationIdentity,
    pub units: Vec<SourceUnit>,
}

impl StoredProject {
    pub fn add_file(&mut self, requested: &str) -> io::Result<usize> {
        self.require_custom()?;
        let name = normalized_custom_file_name(requested)?;
        self.require_unique_file_name(&name, None)?;
        self.units.push(SourceUnit {
            path: format!("{CUSTOM_ROOT}/{}/{name}", self.id),
            source: format!("-- {name}\n"),
        });
        Ok(self.units.len() - 1)
    }

    pub fn rename_file(&mut self, index: usize, requested: &str) -> io::Result<()> {
        self.require_custom()?;
        if index >= self.units.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "source file index is out of bounds",
            ));
        }
        let name = normalized_custom_file_name(requested)?;
        self.require_unique_file_name(&name, Some(index))?;
        self.units[index].path = format!("{CUSTOM_ROOT}/{}/{name}", self.id);
        Ok(())
    }

    pub fn remove_file(&mut self, index: usize) -> io::Result<()> {
        self.require_custom()?;
        if self.units.len() <= 1 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a custom example must keep at least one source file",
            ));
        }
        if index >= self.units.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "source file index is out of bounds",
            ));
        }
        self.units.remove(index);
        Ok(())
    }

    fn require_custom(&self) -> io::Result<()> {
        if self.origin == ProjectOrigin::Custom {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "versioned built-in project structure cannot be changed",
            ))
        }
    }

    fn require_unique_file_name(&self, name: &str, except: Option<usize>) -> io::Result<()> {
        if self.units.iter().enumerate().any(|(index, unit)| {
            Some(index) != except
                && Path::new(&unit.path)
                    .file_name()
                    .and_then(|value| value.to_str())
                    .is_some_and(|value| value.eq_ignore_ascii_case(name))
        }) {
            Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("source file `{name}` already exists"),
            ))
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CustomManifest {
    id: String,
    label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    state_namespace: Option<String>,
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
            application: custom_application_identity(generate_state_namespace(
                &self.repo_root,
                &id,
            )),
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
        validate_custom_application(&project.application)?;
        let directory = self.custom_root.join(&project.id);
        fs::create_dir_all(&directory)?;
        let manifest_path = directory.join("example.toml");
        let previous_manifest = match fs::read_to_string(&manifest_path) {
            Ok(manifest) => Some(
                toml::from_str::<CustomManifest>(&manifest).map_err(|error| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("invalid custom example manifest: {error}"),
                    )
                })?,
            ),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        if let Some(previous_namespace) = previous_manifest
            .as_ref()
            .and_then(|manifest| manifest.state_namespace.as_deref())
            && previous_namespace != project.application.state_namespace
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "custom example state namespace is immutable",
            ));
        }
        let previous_files = previous_manifest
            .map(|manifest| manifest.files)
            .unwrap_or_default();
        let mut files = Vec::with_capacity(project.units.len());
        for unit in &project.units {
            let name = safe_file_name(&unit.path)?;
            atomic_write(&directory.join(&name), unit.source.as_bytes())?;
            files.push(name);
        }
        let manifest = toml::to_string_pretty(&CustomManifest {
            id: project.id.clone(),
            label: project.label.clone(),
            state_namespace: Some(project.application.state_namespace.clone()),
            files: files.clone(),
        })
        .map_err(io::Error::other)?;
        atomic_write(&manifest_path, manifest.as_bytes())?;
        for stale in previous_files
            .into_iter()
            .filter(|previous| !files.iter().any(|file| file == previous))
        {
            let stale = normalized_custom_file_name(&stale)?;
            let path = directory.join(stale);
            if path.exists() {
                fs::remove_file(path)?;
            }
        }
        Ok(())
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
        let manifest_path = directory.join("example.toml");
        let mut manifest: CustomManifest =
            toml::from_str(&fs::read_to_string(&manifest_path)?).map_err(io::Error::other)?;
        validate_custom_id(&manifest.id)?;
        if directory.file_name().and_then(|name| name.to_str()) != Some(&manifest.id) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "custom example directory does not match manifest id",
            ));
        }
        let state_namespace = match manifest.state_namespace.as_deref() {
            Some(namespace) => {
                validate_state_namespace(namespace)?;
                namespace.to_owned()
            }
            None => {
                let namespace = generate_state_namespace(&self.repo_root, &manifest.id);
                manifest.state_namespace = Some(namespace.clone());
                let upgraded = toml::to_string_pretty(&manifest).map_err(io::Error::other)?;
                atomic_write(&manifest_path, upgraded.as_bytes())?;
                namespace
            }
        };
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
            application: custom_application_identity(state_namespace),
            units,
        })
    }
}

fn custom_application_identity(state_namespace: String) -> ApplicationIdentity {
    ApplicationIdentity::new(CUSTOM_PACKAGE_ID, state_namespace, CUSTOM_DEPLOYMENT_DOMAIN)
}

fn validate_custom_application(application: &ApplicationIdentity) -> io::Result<()> {
    if application.package_id != CUSTOM_PACKAGE_ID
        || application.deployment_domain != CUSTOM_DEPLOYMENT_DOMAIN
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "custom example application identity has invalid host-owned components",
        ));
    }
    validate_state_namespace(&application.state_namespace)
}

fn validate_state_namespace(namespace: &str) -> io::Result<()> {
    let digest = namespace
        .strip_prefix(CUSTOM_NAMESPACE_PREFIX)
        .filter(|digest| digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()));
    if digest.is_some() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "custom example state namespace is invalid",
        ))
    }
}

fn generate_state_namespace(repo_root: &Path, project_id: &str) -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let nonce = CUSTOM_NAMESPACE_NONCE.fetch_add(1, Ordering::Relaxed);
    let seed = format!(
        "{}\0{project_id}\0{}\0{timestamp}\0{}\0{nonce}",
        repo_root.display(),
        std::process::id(),
        format_args!("{:?}", thread::current().id()),
    );
    format!(
        "{CUSTOM_NAMESPACE_PREFIX}{}",
        boon_runtime::sha256_bytes(seed.as_bytes())
    )
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

pub fn normalized_custom_file_name(requested: &str) -> io::Result<String> {
    let mut name = requested.trim().to_owned();
    if !name.to_ascii_lowercase().ends_with(".bn") {
        name.push_str(".bn");
    }
    if name.is_empty()
        || name.len() > 64
        || name == "example.toml"
        || name.starts_with('.')
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "file name must use 1-64 letters, numbers, dots, dashes, or underscores and end in .bn",
        ));
    }
    Ok(name)
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
