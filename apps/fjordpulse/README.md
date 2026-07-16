# FjordPulse Boon Package

`app.toml` is the declarative paired Client/Server package contract. Rust hosts
must not branch on this package ID, routes, data fields, or visual content.

The generic package build selects an explicit mode and namespace profile,
compiles both Boon programs into immutable `ContentArtifact` values, runs
wasm-bindgen over the generic browser host Wasm, copies the closed asset,
fixture, migration, scenario, and budget inventory, and records every digest in
`bundle.json`.

The current package/deployment slice does not claim the browser P7 gate, Live
Entur behavior, persistence restore/migration completion, public deployment, or
FjordPulse parity. Those remain governed by
`docs/plans/FJORDPULSE_FULL_STACK_BOON_REWRITE_PLAN.md`.

