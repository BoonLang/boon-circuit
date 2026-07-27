# FjordPulse Boon Package

`app.toml` is the declarative Client/Session/Server package contract. Rust
hosts must not branch on this package ID, routes, data fields, or visual
content.

The generic package build selects an explicit mode and namespace profile,
compiles the three Boon programs into immutable `ContentArtifact` values, runs
wasm-bindgen over the generic browser host Wasm, copies the closed asset,
fixture, migration, scenario, and budget inventory, and records every digest in
`bundle.cbor`.

This Phase 0 reconciliation establishes the package shape only. The current
Session source is a minimal deterministic lifecycle skeleton; it does not yet
claim complete isolation, resumability, expiry, demand mediation, scoped
replies, or Server policy. The current package/deployment slice also does not
claim the browser P7 gate, Live Entur behavior, persistence restore/migration
completion, public deployment, or FjordPulse parity. Those remain governed by
`docs/plans/FJORDPULSE_FULL_STACK_BOON_REWRITE_PLAN.md`.
