#[path = "../build_git.rs"]
mod build_git;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_FIXTURE: AtomicUsize = AtomicUsize::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "boon-build-git-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        let fixture = Self(path);
        git(&fixture.0, &["init", "--initial-branch=main"]);
        git(&fixture.0, &["config", "user.name", "Boon build test"]);
        git(
            &fixture.0,
            &["config", "user.email", "build-test@example.invalid"],
        );
        fixture.commit("initial");
        fixture
    }

    fn commit(&self, message: &str) {
        git(
            &self.0,
            &[
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--allow-empty",
                "-m",
                message,
            ],
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn git(workspace: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(workspace)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {:?}", output);
}

#[test]
fn branch_commit_changes_a_watched_ref_even_when_head_file_is_unchanged() {
    let fixture = Fixture::new();
    let paths = build_git::identity_watch_paths(&fixture.0);
    assert!(paths.iter().all(|path| path.exists()));
    assert!(!paths.contains(&fixture.0.join(".git")));
    let head = fixture.0.join(".git/HEAD");
    let reference = fixture.0.join(".git/refs/heads/main");
    assert!(paths.contains(&head));
    assert!(paths.contains(&reference));
    let old_head = std::fs::read(&head).unwrap();
    let old_ref = std::fs::read(&reference).unwrap();
    fixture.commit("documentation-only checkpoint");
    assert_eq!(std::fs::read(head).unwrap(), old_head);
    assert_ne!(std::fs::read(reference).unwrap(), old_ref);
    assert_eq!(build_git::identity_watch_paths(&fixture.0), paths);
}

#[test]
fn detached_head_and_staging_keep_existing_identity_inputs_watched() {
    let fixture = Fixture::new();
    git(&fixture.0, &["checkout", "--detach"]);
    std::fs::write(fixture.0.join("input.rs"), "// input\n").unwrap();
    git(&fixture.0, &["add", "input.rs"]);
    let paths = build_git::identity_watch_paths(&fixture.0);
    assert!(paths.contains(&fixture.0.join(".git/HEAD")));
    assert!(paths.contains(&fixture.0.join(".git/index")));
    assert!(!paths.contains(&fixture.0.join(".git/refs/heads/main")));
    assert!(paths.iter().all(|path| path.exists()));
}

#[test]
fn packed_ref_creation_is_covered_without_a_missing_file_watch() {
    let fixture = Fixture::new();
    git(&fixture.0, &["pack-refs", "--all", "--prune"]);
    let reference = fixture.0.join(".git/refs/heads/main");
    assert!(!reference.exists());
    let paths = build_git::identity_watch_paths(&fixture.0);
    assert!(paths.contains(&fixture.0.join(".git/packed-refs")));
    assert!(paths.contains(&fixture.0.join(".git/refs/heads")));
    assert!(paths.iter().all(|path| path.exists()));
    fixture.commit("create loose ref");
    assert!(build_git::identity_watch_paths(&fixture.0).contains(&reference));
}

#[test]
fn linked_worktree_watches_private_head_and_shared_branch_ref() {
    let fixture = Fixture::new();
    let linked = fixture.0.join("linked");
    git(
        &fixture.0,
        &["worktree", "add", "-b", "linked", linked.to_str().unwrap()],
    );
    let paths = build_git::identity_watch_paths(&linked);
    assert!(paths.contains(&fixture.0.join(".git/worktrees/linked/HEAD")));
    assert!(paths.contains(&fixture.0.join(".git/worktrees/linked/index")));
    assert!(paths.contains(&fixture.0.join(".git/refs/heads/linked")));
    assert!(!paths.contains(&fixture.0.join(".git/HEAD")));
    assert!(paths.iter().all(|path| path.exists()));
}
