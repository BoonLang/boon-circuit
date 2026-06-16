# User Representation Ledger

This file records the current user-suggested speed ideas as a standalone
execution ledger. It does not replace the master checklist. It gives each idea
a safe first boundary, an experiment order, and a kill rule.

## Rules

- Do not add Boon syntax in these slices.
- Keep visible labels, paths, statuses, scenario text, diagnostics, and public
  reports as `TEXT`/JSON unless a later versioned public schema migration is
  explicitly accepted.
- Replace JSON only at private Rust-to-Rust boundaries or behind compatibility
  serializers that preserve the public shape.
- Replace `BTreeMap`/`BTreeSet` only where the whole boundary is hot and there
  is a correctness oracle.
- Keep generic `LIST` behavior as the oracle; storage mode is an engine choice.
- Treat unsafe or ambiguous inference as a compiler/runtime error, not as a
  request for manual Boon annotations.

## REP-19-001 Binary Instead Of Accidental JSON

Idea:
Internal source frames, cache keys, bridge staging, and render metadata should
not serialize to JSON when both producer and consumer are Rust.

Safe first boundaries:

- Keep typed dev render metadata as the existing proven pattern.
- Extend structured cache keys where profiles show JSON/string allocation.
- Keep bridge canonical hashes stable until a versioned ABI migration exists.

Do not target:

- Scenario files, public reports, human diagnostics, or visible app state.

Kill criteria:

- Public schema shape changes without migration.
- Binary encoding hides evidence needed for debugging.
- The official NovyWave gate regresses.

## REP-19-002 BYTES And Streamed Payloads

Idea:
Raw waveform file bytes, decoded waveform pages, blob/page payloads, and upload
buffers should move as shared `Bytes`, page refs, blob refs, or future
`Stream<Bytes>` values instead of text, JSON arrays, or generic nested records.

Current repo state:

- `boon_bridge::BridgeValue::Bytes` exists.
- Bridge completion payload sidecars exist for blob/page bytes.
- Runtime/typechecker values do not yet have first-class bytes.
- `LiveSourceEvent` is still text/json shaped.

Safe first boundary:

- Add bridge-private deterministic producer tests that call
  `BridgeEffectScheduler::complete_with_payloads`.
- Prove real blob/page bytes validate against refs without changing
  `wellen.v1` schemas, NovyWave Boon source, scenario text, labels, paths, or
  public report JSON.

Later boundary:

- Add deterministic replay bundles for completion plus payload sidecars.
- Integrate real NovyWave waveform/page/blob producers only after the bridge
  replay contract is deterministic.
- Add runtime/typechecker `Bytes` only where bridge/file contracts infer bytes.

Kill criteria:

- Boon programmers must manually annotate common examples as bytes.
- Bytes cross the bridge and are immediately converted back to JSON/text.
- State summaries inline large byte arrays.
- Replay depends on nondeterministic host state instead of ref digest/length.

## REP-19-003 Tree Containers At Hot Boundaries

Idea:
Some `BTreeSet` and `BTreeMap` uses are deterministic-order conveniences, not
the best runtime representation. Replacements should happen at whole hot
boundaries, not one-off swaps.

Safe boundaries:

- Already sorted list-index hits should remain sorted `Vec<usize>` and avoid
  temporary `BTreeSet` membership structures.
- Dirty/read-key sets can move to small-vector or dense-index forms when
  profiling shows duplicate pressure.
- Use `IndexSet` only where deterministic order and stable index access are
  both semantically useful.

Current implementation target:

- NovyWave click p95 currently points at duplicated root-derived candidates,
  not at a standalone container type. The safe runtime experiment is pruning
  duplicate structured child-root materialization before changing containers.

Kill criteria:

- Order changes for list filters, joins, retains, or rendered rows.
- A set replacement reduces determinism in reports or diagnostics.
- Candidate counts stay flat while code complexity grows.

## REP-19-004 LIST Storage Modes

Idea:
`LIST` should remain generic to users, while compiler/runtime choose a physical
mode when usage proves it safe.

Internal modes to test:

- constant array for literal static lists;
- dense `Vec` for ordinary materialized rows;
- selection view for indexed filters;
- incremental projection for root `List/map`;
- virtual list for large visible windows;
- stream/page-backed list for large binary/file payloads.

Current best candidates:

- Direct root `List/map` materialization for NovyWave selected lanes through a
  generic root-list-view path.
- Incremental exact-text lookup-index maintenance for equality filters.
- Virtual collections only as a reusable component shared across examples.

Kill criteria:

- A dynamic dependency freezes.
- Row identity or event routing changes.
- User-facing code must select a storage strategy manually.

## REP-19-005 Constant Classification And Hoisting

Idea:
The examples contain many labels, separators, enum tags, field names, source
names, static rows, and style fragments. They should be classified and then
hoisted only when dependency analysis proves they are constant or row-invariant.

Safe order:

1. Keep the existing diagnostic classifier as the source-linked oracle.
2. Intern names and tags in hot IR/runtime plans before folding values.
3. Hoist static renderer/style/cache fragments after invalidation is proven.
4. Fold literal/static row templates only after LIST storage-mode tests pass.

Kill criteria:

- `SOURCE`, `HOLD`, row fields, bridge payloads, or file data are frozen.
- Diagnostics cannot explain why a value was hoisted or blocked.

## Immediate Experiment Order

1. Runtime: prune duplicated structured child-root materialization in the
   NovyWave click path while preserving same-event root scalar correctness.
2. Bridge: add deterministic real blob/page producer proof for
   `complete_with_payloads`.
3. Runtime/LIST: use current profiles to pick either direct root `List/map`
   materialization or exact-text lookup-index maintenance.
4. Compiler/runtime: promote constant classification into interned IDs and
   static fragments only after candidate reports prove safety.
