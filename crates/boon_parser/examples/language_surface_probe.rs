use boon_parser::{parse_ast, parse_source};
use boon_syntax::LANGUAGE_FEATURE_REGISTRY;
use std::fs;
use std::io::{self, BufRead};

const REGISTRY_PROTOCOL: &str = "boon-language-feature-registry-v1";

fn main() {
    if let Err(error) = run() {
        eprintln!("language_surface_probe: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    match args.as_slice() {
        [command] if command == "registry" => emit_registry(),
        [command] if command == "verify-fixtures" => verify_fixtures(),
        [command] if command == "verify-pattern-corpus" => verify_pattern_corpus(),
        _ => Err(
            "usage: language_surface_probe <registry|verify-fixtures|verify-pattern-corpus>; \
             verify-fixtures reads tab-separated feature-id, expectation, and path rows from stdin; \
             verify-pattern-corpus reads one workspace-relative Boon source path per line"
                .to_owned(),
        ),
    }
}

fn emit_registry() -> Result<(), String> {
    println!("{REGISTRY_PROTOCOL}");
    for feature in LANGUAGE_FEATURE_REGISTRY {
        reject_protocol_text(feature.id)?;
        println!(
            "{}\t{}\t{}",
            feature.id,
            feature.stage.as_str(),
            feature.parse_expectation.as_str()
        );
    }
    Ok(())
}

fn verify_fixtures() -> Result<(), String> {
    let stdin = io::stdin();
    let mut verified = 0usize;
    for (index, line) in stdin.lock().lines().enumerate() {
        let line = line.map_err(|error| format!("stdin row {}: {error}", index + 1))?;
        if line.is_empty() {
            continue;
        }
        let mut fields = line.split('\t');
        let feature_id = fields
            .next()
            .ok_or_else(|| format!("stdin row {} has no feature id", index + 1))?;
        let expectation = fields
            .next()
            .ok_or_else(|| format!("stdin row {} has no expectation", index + 1))?;
        let fixture = fields
            .next()
            .ok_or_else(|| format!("stdin row {} has no fixture path", index + 1))?;
        if fields.next().is_some() {
            return Err(format!("stdin row {} has too many fields", index + 1));
        }
        let source = fs::read_to_string(fixture)
            .map_err(|error| format!("failed to read fixture `{fixture}`: {error}"))?;
        let parsed = parse_source(fixture, source);
        match (expectation, parsed) {
            ("accept", Ok(_)) => {}
            ("accept", Err(error)) => {
                return Err(format!(
                    "current parser unexpectedly rejected `{fixture}` for `{feature_id}`: {error}"
                ));
            }
            ("reject", Err(error)) if error.message.contains(feature_id) => {}
            ("reject", Err(error)) => {
                return Err(format!(
                    "`{fixture}` failed for an unrelated reason instead of planned feature \
                     `{feature_id}`: {error}"
                ));
            }
            ("reject", Ok(_)) => {
                return Err(format!(
                    "current parser unexpectedly accepted planned rejected fixture `{fixture}` \
                     for `{feature_id}`"
                ));
            }
            (other, _) => {
                return Err(format!(
                    "fixture `{fixture}` has unsupported parse expectation `{other}`"
                ));
            }
        }
        verified += 1;
    }
    if verified == 0 {
        return Err("no language-surface fixtures were provided".to_owned());
    }
    println!("verified {verified} language-surface parser fixture(s)");
    Ok(())
}

fn verify_pattern_corpus() -> Result<(), String> {
    let stdin = io::stdin();
    let mut verified = 0usize;
    for (index, line) in stdin.lock().lines().enumerate() {
        let path = line.map_err(|error| format!("stdin row {}: {error}", index + 1))?;
        if path.is_empty() {
            continue;
        }
        reject_protocol_text(&path)?;
        let source = fs::read_to_string(&path)
            .map_err(|error| format!("failed to read Boon source `{path}`: {error}"))?;
        parse_ast(&path, &source)
            .map_err(|error| format!("match-pattern corpus rejected `{path}`: {error}"))?;
        verified += 1;
    }
    if verified == 0 {
        return Err("no Boon source paths were provided".to_owned());
    }
    println!("verified {verified} Boon source match-pattern surfaces");
    Ok(())
}

fn reject_protocol_text(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value
            .chars()
            .any(|ch| ch == '\t' || ch == '\n' || ch == '\r')
    {
        return Err(format!("invalid registry protocol text `{value}`"));
    }
    Ok(())
}
