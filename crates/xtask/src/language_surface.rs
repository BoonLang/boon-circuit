use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

pub const MANIFEST_RELATIVE_PATH: &str = "examples/language_feature_coverage.toml";

const FORMAT_VERSION: u32 = 1;
const REGISTRY_OWNER: &str = "boon_parser";
const REGISTRY_PROTOCOL: &str = "boon-language-feature-registry-v1";

#[derive(Clone, Debug, Eq, PartialEq)]
struct Manifest {
    format_version: u32,
    registry_owner: String,
    features: Vec<ManifestFeature>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ManifestFeature {
    id: String,
    stage: String,
    parser_expectation: String,
    fixture: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RegistryFeature {
    id: String,
    stage: String,
    parser_expectation: String,
}

#[derive(Default)]
struct FeatureBuilder {
    id: Option<String>,
    stage: Option<String>,
    parser_expectation: Option<String>,
    fixture: Option<String>,
}

impl FeatureBuilder {
    fn set(&mut self, key: &str, value: String, line: usize) -> Result<(), String> {
        let slot = match key {
            "id" => &mut self.id,
            "stage" => &mut self.stage,
            "parser_expectation" => &mut self.parser_expectation,
            "fixture" => &mut self.fixture,
            _ => return Err(format!("line {line}: unknown feature key `{key}`")),
        };
        if slot.replace(value).is_some() {
            return Err(format!("line {line}: duplicate feature key `{key}`"));
        }
        Ok(())
    }

    fn finish(self, ordinal: usize) -> Result<ManifestFeature, String> {
        Ok(ManifestFeature {
            id: self
                .id
                .ok_or_else(|| format!("feature {ordinal} has no `id`"))?,
            stage: self
                .stage
                .ok_or_else(|| format!("feature {ordinal} has no `stage`"))?,
            parser_expectation: self
                .parser_expectation
                .ok_or_else(|| format!("feature {ordinal} has no `parser_expectation`"))?,
            fixture: self
                .fixture
                .ok_or_else(|| format!("feature {ordinal} has no `fixture`"))?,
        })
    }
}

/// Verify the committed language-surface manifest against the parser-owned
/// registry and the parser's current behavior.
///
/// This module is intentionally independent of the xtask command dispatcher so
/// it can land before command wiring without creating a second source of truth.
pub fn run(workspace: &Path) -> Result<(), String> {
    let manifest_path = workspace.join(MANIFEST_RELATIVE_PATH);
    let source = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("failed to read {}: {error}", manifest_path.display()))?;
    let manifest = parse_manifest(&source)?;
    let registry = load_parser_registry(workspace)?;
    validate_contract(workspace, &manifest, &registry)?;
    verify_fixture_parsing(workspace, &manifest)?;
    let pattern_corpus_count = verify_match_pattern_corpus(workspace)?;
    println!(
        "verified {} parser-owned language feature(s) from {} and {} Boon match-pattern source(s)",
        manifest.features.len(),
        MANIFEST_RELATIVE_PATH,
        pattern_corpus_count,
    );
    Ok(())
}

fn parse_manifest(source: &str) -> Result<Manifest, String> {
    let mut format_version = None;
    let mut registry_owner = None;
    let mut features = Vec::new();
    let mut current = None::<FeatureBuilder>;

    for (index, raw_line) in source.lines().enumerate() {
        let line_number = index + 1;
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[[feature]]" {
            if let Some(builder) = current.take() {
                features.push(builder.finish(features.len() + 1)?);
            }
            current = Some(FeatureBuilder::default());
            continue;
        }
        let (key, raw_value) = line
            .split_once('=')
            .ok_or_else(|| format!("line {line_number}: expected `key = value`"))?;
        let key = key.trim();
        let raw_value = raw_value.trim();
        if let Some(builder) = current.as_mut() {
            builder.set(
                key,
                parse_basic_string(raw_value, line_number)?,
                line_number,
            )?;
            continue;
        }
        match key {
            "format_version" => {
                if format_version.is_some() {
                    return Err(format!("line {line_number}: duplicate `format_version`"));
                }
                format_version = Some(
                    raw_value
                        .parse::<u32>()
                        .map_err(|_| format!("line {line_number}: invalid format version"))?,
                );
            }
            "registry_owner" => {
                if registry_owner.is_some() {
                    return Err(format!("line {line_number}: duplicate `registry_owner`"));
                }
                registry_owner = Some(parse_basic_string(raw_value, line_number)?);
            }
            _ => return Err(format!("line {line_number}: unknown manifest key `{key}`")),
        }
    }
    if let Some(builder) = current {
        features.push(builder.finish(features.len() + 1)?);
    }
    let manifest = Manifest {
        format_version: format_version.ok_or("manifest has no `format_version`")?,
        registry_owner: registry_owner.ok_or("manifest has no `registry_owner`")?,
        features,
    };
    validate_manifest_shape(&manifest)?;
    Ok(manifest)
}

fn parse_basic_string(value: &str, line: usize) -> Result<String, String> {
    let Some(inner) = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
    else {
        return Err(format!("line {line}: expected a quoted basic string"));
    };
    if inner.is_empty()
        || inner
            .chars()
            .any(|ch| ch == '\\' || ch == '\t' || ch == '\n' || ch == '\r')
    {
        return Err(format!(
            "line {line}: strings must be non-empty and contain no escapes or control characters"
        ));
    }
    Ok(inner.to_owned())
}

fn validate_manifest_shape(manifest: &Manifest) -> Result<(), String> {
    if manifest.format_version != FORMAT_VERSION {
        return Err(format!(
            "language feature manifest format must be {FORMAT_VERSION}, found {}",
            manifest.format_version
        ));
    }
    if manifest.registry_owner != REGISTRY_OWNER {
        return Err(format!(
            "language feature registry owner must be `{REGISTRY_OWNER}`, found `{}`",
            manifest.registry_owner
        ));
    }
    if manifest.features.is_empty() {
        return Err("language feature manifest is empty".to_owned());
    }
    for pair in manifest.features.windows(2) {
        if pair[0].id >= pair[1].id {
            return Err(format!(
                "manifest feature `{}` must sort before unique feature `{}`",
                pair[0].id, pair[1].id
            ));
        }
    }
    for feature in &manifest.features {
        if feature.id.is_empty()
            || !feature
                .id
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
        {
            return Err(format!("invalid language feature id `{}`", feature.id));
        }
        if !matches!(feature.stage.as_str(), "current" | "planned") {
            return Err(format!(
                "feature `{}` has invalid stage `{}`",
                feature.id, feature.stage
            ));
        }
        if !matches!(feature.parser_expectation.as_str(), "accept" | "reject") {
            return Err(format!(
                "feature `{}` has invalid parser expectation `{}`",
                feature.id, feature.parser_expectation
            ));
        }
        if feature.stage == "current" && feature.parser_expectation != "accept" {
            return Err(format!(
                "current feature `{}` cannot claim parser rejection",
                feature.id
            ));
        }
    }
    Ok(())
}

fn load_parser_registry(workspace: &Path) -> Result<Vec<RegistryFeature>, String> {
    let output = parser_probe_command(workspace)
        .arg("registry")
        .output()
        .map_err(|error| format!("failed to run parser language-surface probe: {error}"))?;
    if !output.status.success() {
        return Err(command_failure("parser registry probe", &output));
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|_| "parser registry probe emitted non-UTF-8 output".to_owned())?;
    parse_registry_output(&stdout)
}

fn parse_registry_output(source: &str) -> Result<Vec<RegistryFeature>, String> {
    let mut lines = source.lines();
    if lines.next() != Some(REGISTRY_PROTOCOL) {
        return Err(format!(
            "parser registry probe did not emit `{REGISTRY_PROTOCOL}`"
        ));
    }
    let mut features = Vec::new();
    for (index, line) in lines.enumerate() {
        let fields = line.split('\t').collect::<Vec<_>>();
        let [id, stage, parser_expectation] = fields.as_slice() else {
            return Err(format!(
                "parser registry row {} is not id, stage, expectation",
                index + 1
            ));
        };
        features.push(RegistryFeature {
            id: (*id).to_owned(),
            stage: (*stage).to_owned(),
            parser_expectation: (*parser_expectation).to_owned(),
        });
    }
    if features.is_empty() {
        return Err("parser language feature registry is empty".to_owned());
    }
    for pair in features.windows(2) {
        if pair[0].id >= pair[1].id {
            return Err(format!(
                "parser registry feature `{}` must sort before unique feature `{}`",
                pair[0].id, pair[1].id
            ));
        }
    }
    Ok(features)
}

fn validate_contract(
    workspace: &Path,
    manifest: &Manifest,
    registry: &[RegistryFeature],
) -> Result<(), String> {
    if manifest.features.len() != registry.len() {
        return Err(format!(
            "manifest has {} feature(s), parser registry has {}; coverage must be one-to-one",
            manifest.features.len(),
            registry.len()
        ));
    }
    let mut fixtures = BTreeSet::new();
    for (manifest_feature, registry_feature) in manifest.features.iter().zip(registry) {
        if (
            manifest_feature.id.as_str(),
            manifest_feature.stage.as_str(),
            manifest_feature.parser_expectation.as_str(),
        ) != (
            registry_feature.id.as_str(),
            registry_feature.stage.as_str(),
            registry_feature.parser_expectation.as_str(),
        ) {
            return Err(format!(
                "manifest feature ({}, {}, {}) differs from parser registry feature ({}, {}, {})",
                manifest_feature.id,
                manifest_feature.stage,
                manifest_feature.parser_expectation,
                registry_feature.id,
                registry_feature.stage,
                registry_feature.parser_expectation
            ));
        }
        validate_fixture_path(&manifest_feature.fixture)?;
        if !fixtures.insert(manifest_feature.fixture.as_str()) {
            return Err(format!(
                "fixture `{}` is assigned to more than one feature",
                manifest_feature.fixture
            ));
        }
        let fixture_path = workspace.join(&manifest_feature.fixture);
        if !fixture_path.is_file() {
            return Err(format!(
                "feature `{}` fixture `{}` does not exist as a file",
                manifest_feature.id, manifest_feature.fixture
            ));
        }
        if fs::metadata(&fixture_path)
            .map_err(|error| format!("failed to inspect {}: {error}", fixture_path.display()))?
            .len()
            == 0
        {
            return Err(format!(
                "feature `{}` fixture `{}` is empty",
                manifest_feature.id, manifest_feature.fixture
            ));
        }
    }
    Ok(())
}

fn validate_fixture_path(path: &str) -> Result<(), String> {
    let fixture = Path::new(path);
    if fixture.is_absolute()
        || fixture
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        || !path.starts_with("examples/language_surface/")
        || fixture.extension().and_then(|extension| extension.to_str()) != Some("bn")
        || path.contains('\t')
    {
        return Err(format!(
            "fixture `{path}` must be a normalized workspace-relative .bn path below examples/language_surface"
        ));
    }
    Ok(())
}

fn verify_fixture_parsing(workspace: &Path, manifest: &Manifest) -> Result<(), String> {
    let mut child = parser_probe_command(workspace)
        .arg("verify-fixtures")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to start parser fixture probe: {error}"))?;
    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or("parser fixture probe has no stdin")?;
        for feature in &manifest.features {
            writeln!(
                stdin,
                "{}\t{}\t{}",
                feature.id, feature.parser_expectation, feature.fixture
            )
            .map_err(|error| format!("failed to send fixtures to parser probe: {error}"))?;
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|error| format!("failed to wait for parser fixture probe: {error}"))?;
    if !output.status.success() {
        return Err(command_failure("parser fixture probe", &output));
    }
    Ok(())
}

fn verify_match_pattern_corpus(workspace: &Path) -> Result<usize, String> {
    let mut paths = Vec::new();
    collect_boon_source_paths(workspace, workspace, &mut paths)?;
    paths.sort();
    if paths.is_empty() {
        return Err("workspace contains no Boon source files".to_owned());
    }

    let mut child = parser_probe_command(workspace)
        .arg("verify-pattern-corpus")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to start parser match-pattern corpus probe: {error}"))?;
    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or("parser match-pattern corpus probe has no stdin")?;
        for path in &paths {
            let relative = path
                .strip_prefix(workspace)
                .map_err(|_| format!("{} escaped the workspace", path.display()))?;
            let relative = relative
                .to_str()
                .ok_or_else(|| format!("{} is not valid UTF-8", relative.display()))?;
            if relative.chars().any(|ch| matches!(ch, '\n' | '\r' | '\t')) {
                return Err(format!(
                    "Boon source path `{relative}` contains protocol control characters"
                ));
            }
            writeln!(stdin, "{relative}")
                .map_err(|error| format!("failed to send corpus paths to parser probe: {error}"))?;
        }
    }
    let output = child.wait_with_output().map_err(|error| {
        format!("failed to wait for parser match-pattern corpus probe: {error}")
    })?;
    if !output.status.success() {
        return Err(command_failure(
            "parser match-pattern corpus probe",
            &output,
        ));
    }
    Ok(paths.len())
}

fn collect_boon_source_paths(
    workspace: &Path,
    directory: &Path,
    output: &mut Vec<PathBuf>,
) -> Result<(), String> {
    let mut entries = fs::read_dir(directory)
        .map_err(|error| format!("failed to read {}: {error}", directory.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to enumerate {}: {error}", directory.display()))?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| format!("failed to inspect {}: {error}", path.display()))?;
        if file_type.is_dir() {
            if directory == workspace
                && matches!(entry.file_name().to_str(), Some(".git" | "target"))
            {
                continue;
            }
            collect_boon_source_paths(workspace, &path, output)?;
        } else if file_type.is_file()
            && path.extension().and_then(|extension| extension.to_str()) == Some("bn")
        {
            output.push(path);
        }
    }
    Ok(())
}

fn parser_probe_command(workspace: &Path) -> Command {
    let mut command = Command::new("cargo");
    command.current_dir(workspace).args([
        "run",
        "--quiet",
        "--package",
        "boon_parser",
        "--example",
        "language_surface_probe",
        "--",
    ]);
    command
}

fn command_failure(context: &str, output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    format!(
        "{context} failed with {}: stderr: {}; stdout: {}",
        output.status,
        stderr.trim(),
        stdout.trim()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_TEMP: AtomicUsize = AtomicUsize::new(0);

    fn manifest_source(features: &str) -> String {
        format!("format_version = 1\nregistry_owner = \"boon_parser\"\n\n{features}")
    }

    fn temp_workspace() -> std::path::PathBuf {
        let suffix = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "boon-language-surface-{}-{suffix}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("examples/language_surface/current")).unwrap();
        root
    }

    #[test]
    fn strict_manifest_parser_preserves_delivery_and_parse_status() {
        let source = manifest_source(
            "[[feature]]\n\
             id = \"current_feature\"\n\
             stage = \"current\"\n\
             parser_expectation = \"accept\"\n\
             fixture = \"examples/language_surface/current/current.bn\"\n\n\
             [[feature]]\n\
             id = \"planned_feature\"\n\
             stage = \"planned\"\n\
             parser_expectation = \"reject\"\n\
             fixture = \"examples/language_surface/future/planned.bn\"\n",
        );
        let manifest = parse_manifest(&source).unwrap();
        assert_eq!(manifest.features.len(), 2);
        assert_eq!(manifest.features[0].stage, "current");
        assert_eq!(manifest.features[1].parser_expectation, "reject");
    }

    #[test]
    fn manifest_rejects_duplicates_and_current_rejection_claims() {
        let duplicate = manifest_source(
            "[[feature]]\n\
             id = \"same\"\n\
             stage = \"current\"\n\
             parser_expectation = \"accept\"\n\
             fixture = \"examples/language_surface/current/one.bn\"\n\n\
             [[feature]]\n\
             id = \"same\"\n\
             stage = \"current\"\n\
             parser_expectation = \"accept\"\n\
             fixture = \"examples/language_surface/current/two.bn\"\n",
        );
        assert!(
            parse_manifest(&duplicate)
                .unwrap_err()
                .contains("sort before unique")
        );

        let dishonest = manifest_source(
            "[[feature]]\n\
             id = \"current_feature\"\n\
             stage = \"current\"\n\
             parser_expectation = \"reject\"\n\
             fixture = \"examples/language_surface/current/current.bn\"\n",
        );
        assert!(
            parse_manifest(&dishonest)
                .unwrap_err()
                .contains("cannot claim parser rejection")
        );
    }

    #[test]
    fn registry_protocol_and_manifest_must_match_one_to_one() {
        let registry = parse_registry_output(
            "boon-language-feature-registry-v1\ncurrent_feature\tcurrent\taccept\n",
        )
        .unwrap();
        let source = manifest_source(
            "[[feature]]\n\
             id = \"current_feature\"\n\
             stage = \"current\"\n\
             parser_expectation = \"accept\"\n\
             fixture = \"examples/language_surface/current/current.bn\"\n",
        );
        let manifest = parse_manifest(&source).unwrap();
        let workspace = temp_workspace();
        fs::write(
            workspace.join("examples/language_surface/current/current.bn"),
            "value: 1\n",
        )
        .unwrap();
        validate_contract(&workspace, &manifest, &registry).unwrap();

        let missing = Vec::<RegistryFeature>::new();
        assert!(
            validate_contract(&workspace, &manifest, &missing)
                .unwrap_err()
                .contains("one-to-one")
        );
        fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn contract_rejects_missing_or_escaping_fixture_paths() {
        let registry = vec![RegistryFeature {
            id: "current_feature".to_owned(),
            stage: "current".to_owned(),
            parser_expectation: "accept".to_owned(),
        }];
        let workspace = temp_workspace();
        let source = manifest_source(
            "[[feature]]\n\
             id = \"current_feature\"\n\
             stage = \"current\"\n\
             parser_expectation = \"accept\"\n\
             fixture = \"examples/language_surface/current/missing.bn\"\n",
        );
        let manifest = parse_manifest(&source).unwrap();
        assert!(
            validate_contract(&workspace, &manifest, &registry)
                .unwrap_err()
                .contains("does not exist")
        );
        assert!(validate_fixture_path("../outside.bn").is_err());
        fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn committed_manifest_matches_the_live_parser_registry_and_behavior() {
        let current = std::env::current_dir().unwrap();
        let workspace = if current.join(MANIFEST_RELATIVE_PATH).is_file() {
            current
        } else {
            let xtask = option_env!("CARGO_MANIFEST_DIR")
                .map(std::path::PathBuf::from)
                .expect("test must run from the workspace or through Cargo");
            xtask
                .parent()
                .and_then(Path::parent)
                .expect("xtask lives below the workspace")
                .to_path_buf()
        };
        run(&workspace).unwrap();
    }
}
