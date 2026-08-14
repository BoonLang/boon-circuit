//! Dense, dependency-isolated compiler kernel.
//!
//! The kernel consumes compact, owned programs rather than parser arenas or
//! the legacy owner checker. Hot inference uses immutable hash-consed type
//! terms, dense variables, and deterministic mutation-driven work queues.
//! `boon_checked` is only the public projection boundary.

mod artifact;
mod owner;
mod program;
mod receipt;
mod session;
mod solver;
mod term;

pub use artifact::*;
pub use owner::*;
pub use program::*;
pub use receipt::*;
pub use session::*;
pub use solver::*;
pub use term::*;

#[cfg(test)]
mod architecture_tests {
    #[test]
    fn kernel_manifest_has_no_upper_compiler_dependencies() {
        let manifest = include_str!("../Cargo.toml");
        for forbidden in [
            "boon_typecheck",
            "boon_semantic",
            "boon_verify",
            "boon_ir",
            "boon_plan",
            "boon_compiler",
            "boon_parser",
        ] {
            assert!(
                !manifest.lines().any(|line| {
                    line.trim_start()
                        .strip_prefix(forbidden)
                        .is_some_and(|suffix| suffix.trim_start().starts_with(['=', '.']))
                }),
                "dense kernel must not depend on upper compiler crate `{forbidden}`"
            );
        }
    }
}
