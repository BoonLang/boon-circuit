use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Git inputs to the producer's HEAD/diff/untracked v1 identity. Resolve paths
/// through Git so linked worktrees use their own HEAD/index and shared refs.
pub fn identity_watch_paths(workspace: &Path) -> Vec<PathBuf> {
    let mut paths = vec![git_path(workspace, "HEAD")];
    for name in ["index", "packed-refs"] {
        let path = git_path(workspace, name);
        // An absent optional dependency causes perpetual Cargo rebuilds.
        if path.exists() {
            paths.push(path);
        }
    }
    let symbolic = git(workspace, &["symbolic-ref", "--quiet", "HEAD"]);
    if symbolic.status.success() {
        let reference = String::from_utf8(symbolic.stdout).expect("Git ref UTF-8");
        let mut path = git_path(workspace, reference.trim());
        // Packed branches have no loose ref yet. Watch its existing parent so
        // a later commit creating the loose ref is observed, without watching
        // the entire Git directory (objects, locks and unrelated activity).
        while !path.exists() {
            assert!(path.pop(), "Git reference has no existing ancestor");
        }
        paths.push(path);
    } else {
        assert_eq!(symbolic.status.code(), Some(1), "read symbolic Git HEAD");
    }
    paths.sort();
    paths.dedup();
    paths
}

fn git_path(workspace: &Path, name: &str) -> PathBuf {
    let output = git(workspace, &["rev-parse", "--git-path", name]);
    assert!(output.status.success(), "resolve Git path {name}");
    let path = String::from_utf8(output.stdout).expect("Git path UTF-8");
    workspace.join(path.trim())
}

fn git(workspace: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .args(args)
        .current_dir(workspace)
        .output()
        .expect("run Git for producer watches")
}
