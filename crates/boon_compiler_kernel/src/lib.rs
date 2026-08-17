//! Dense, dependency-isolated compiler kernel.
//!
//! The kernel consumes compact, owned programs rather than parser arenas or
//! the legacy owner checker. Hot inference uses immutable hash-consed type
//! terms, dense variables, and deterministic mutation-driven work queues.
//! `boon_checked` is only the public projection boundary.

mod abi;
mod artifact;
mod artifact_terms;
mod link;
mod owner;
mod program;
mod receipt;
mod session;
mod solver;
mod term;

pub use abi::*;
pub use artifact::*;
pub use artifact_terms::*;
pub use link::*;
pub use owner::*;
pub use program::*;
pub use receipt::*;
pub use session::*;
pub use solver::*;
pub use term::*;

/// Whether the non-normative two-worker kernel projection experiment is
/// enabled for this process.
///
/// Cold compiler acceptance is deliberately single-threaded. Keeping the
/// graph-proven split behind an explicit experiment lets us measure it later
/// without silently making the production compiler or its reports parallel.
#[cfg(not(target_family = "wasm"))]
pub(crate) fn experimental_parallel_projection_enabled() -> bool {
    std::env::var_os("BOON_KERNEL_EXPERIMENTAL_PARALLEL").is_some_and(|value| value == "1")
}

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
