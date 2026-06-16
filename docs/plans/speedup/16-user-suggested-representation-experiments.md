# User-Suggested Representation Experiments

This file records the current binary/BYTES/container/LIST/constant ideas as
implementation experiments. The rule is unchanged: no Boon syntax changes, no
NovyWave-specific shortcuts, and no speed claim without the native gates or a
focused phase profile proving it.

## Current Evidence

Initial NovyWave source replacement measurement showed a compile/runtime build
around 68.5s in debug:

- `total_ms`: 68482.8
- `live_runtime_ms`: 61037.5
- `lower_ms`: 46968.1
- `verify_combinational_field_cycles_ms`: 21252.4
- `typecheck_ms`: 7253.7
- `update_branches_ms`: 4732.3
- `possible_causes_ms`: 4801.8
- `initialize_generic_derived_first_ms`: 10373.8
- `layout_ms`: 7376.8

This means the first useful experiment is not a blind container swap. The
engine has algorithmic compiler/runtime work that must be made bounded before
binary transport or BYTES can affect the end-to-end source replacement gate.

After the first two compiler slices:

- `total_ms`: 32784.0
- `live_runtime_ms`: 25351.2
- `lower_ms`: 11295.6
- `verify_combinational_field_cycles_ms`: 25.5
- `dependency_edges_ms`: 36.4
- `possible_causes_ms`: 0.09
- `update_branches_ms`: 145.5
- `typecheck_ms`: 6959.7
- `initialize_generic_derived_first_ms`: 10410.3
- `layout_ms`: 7368.5

The remaining large targets are typecheck/source-shape analysis, generic
derived initialization, layout proof, and cancellation/latest-wins below the
current coarse source-replacement phases.

After the root dirty-set scheduler experiment:

- `total_ms`: 24062.3
- `live_runtime_ms`: 16140.9
- `lower_ms`: 11940.0
- `typecheck_ms`: 7437.3
- `initialize_generic_derived_first_ms`: 434.6
- `initialize_generic_derived_first_profile.root_profile.changed_recompute_ms`:
  about 335ms after incremental dirty read-key counts
- `initialize_generic_derived_second_ms`: 88.7
- `runtime_total_ms`: 536.5
- `layout_ms`: 7855.1

The first generic-derived pass was not slow because of indexed key evaluation:
indexed recompute was about 7ms. It was slow because root dirty ordering rebuilt
root read-key/path checks while proving that 540 dependent roots did not need a
materialized change. Maintaining dirty root read-key counts makes the same
ordering rule much cheaper without changing Boon syntax or NovyWave source.

After the typecheck call-site index experiment:

- `total_ms`: 22999.1
- `live_runtime_ms`: 15418.0
- `lower_ms`: 11112.5
- `typecheck_ms`: 6641.7
- `typecheck_profile.check_statements_ms`: 5874.4
- `typecheck_profile.function_index_ms`: 4.5
- `runtime_total_ms`: 557.7
- `layout_ms`: 7514.8

The compiler now indexes function statements, function-body call graph, and
function argument call-site expression IDs once during checker initialization.
This keeps recursive-function diagnostics based on function-body calls and does
not cache inferred return types. It removes repeated all-expression scans when
rendering/checking function argument types.

After the source-payload lookup index experiment:

- `total_ms`: 20573.1
- `live_runtime_ms`: 13241.9
- `lower_ms`: 8987.2
- `typecheck_ms`: 4768.8
- `typecheck_profile.check_statements_ms`: 4551.0
- `typecheck_profile.source_payload_shape_table_ms`: 12.6
- `runtime_total_ms`: 498.7
- `layout_ms`: 7268.2

The typechecker now builds a source-payload path lookup once from source ports
and reuses it for expression path typing and payload shape extraction. The
lookup indexes full source aliases, store-relative aliases, scoped aliases, and
source suffixes while preserving the existing overlap order. This removes
repeated scans over all source ports without changing Boon syntax or requiring
manual payload annotations.

After the runtime parsed-program sharing experiment:

- `total_ms`: 17151.8
- `live_runtime_ms`: 13021.4
- `lower_ms`: 8994.6
- `typecheck_ms`: 4752.0
- `runtime_total_ms`: 516.6
- `layout_ms`: 4041.9
- `layout_profile.parse_cache_ms`: about 0.0, down from about 3296.7
- `layout_profile.document_eval_lower_ms`: 3631.6
- `layout_profile.text_measure_and_layout_ms`: 73.8
- `layout_profile.typecheck_ms`: 70.1

The runtime cached plan now retains the parsed project, and preview source
replacement passes that parsed program into the layout proof builder. This
removes the duplicate multi-file parse between runtime build and layout proof.
The remaining layout cost is document evaluation/lowering, not text measurement,
artifact writing, or layout cache lookup.

A later verification run was noisier overall (`total_ms`: 19271.9,
`layout_ms`: 4642.4) but kept the important shape: layout parse time stayed
near zero and `document_eval_lower_ms` was the remaining layout bottleneck at
about 4189.5ms.

After the document eval cache-key and scoped-context experiments, the best
focused run was:

- `total_ms`: 14273.2
- `live_runtime_ms`: 12830.1
- `layout_ms`: 1355.7
- `layout_profile.parse_cache_ms`: about 0.0
- `layout_profile.document_eval_lower_ms`: 909.2
- `layout_profile.text_measure_and_layout_ms`: 74.4
- `layout_profile.typecheck_ms`: 69.3

The final verification run stayed in the same band:

- `total_ms`: 14519.7
- `live_runtime_ms`: 13041.5
- `layout_ms`: 1391.1
- `layout_profile.parse_cache_ms`: about 0.0
- `layout_profile.document_eval_lower_ms`: 945.3
- `layout_profile.text_measure_and_layout_ms`: 73.8
- `layout_profile.typecheck_ms`: 69.8

The document eval cache no longer converts inputs, arguments, and `PASSED`
through JSON bytes plus SHA-256 to find an in-process cache entry. It now uses
a structured key over canonicalized `serde_json::Value` shapes. This keeps the
cache private and deterministic while directly applying the "replace JSON where
it makes sense" idea to a measured hot path.

`DocumentEvalContext` now stores locals, local origins, and render arguments in
parent-backed scoped maps. Row/function/block scopes add only local bindings and
fall back through parent scopes, instead of cloning full `BTreeMap`s for every
visible row and nested helper. This is an internal runtime/lowering
representation change only; it adds no Boon syntax and no NovyWave-specific
path.

Diagnostic:

- The broader physical TodoMVC native playground filter still has current
  operator/hover/toggle failures. The operator row-scope and footer hover
  failures reproduced even when `DocumentScopedMap::child()` was temporarily
  flattened back to clone semantics, so they are not attributed to the scoped
  map speed slice.
- Keep physical probe repair separate from the NovyWave layout speed work:
  row-scoped scenario events need address-safe probes, and hover tests need a
  reliable app-owned frame update path.

## EXP-REP-001 Private Binary Encoding

Hypothesis:
Private Rust-to-Rust transport, cache keys, and render metadata should avoid
JSON when the value does not leave the process.

Current state:
- Source-project IPC already has a binary source-frame path.
- Public reports and scenarios must remain JSON.
- The example-switch gate now has app-owned readback enabled for the preview
  child.

First implementation slice:
- Enable app-owned frame readback for the example-switch preview process.
- Keep the binary source-project frame and JSON fallback.
- Do not remove report JSON.

Acceptance:
- `verify-native-example-switch-speed` reports real SHA-256 readback hashes
  instead of `missing`.
- `source_project_binary_frame=true` remains true.
- ACK payloads stay within budget.

Status:
- Implemented for the verifier/evidence slice. The refreshed debug report has
  real top-level readback hashes and the readback-hashes check passes.
- The full gate still fails because NovyWave replacement stays pending after
  the 10s ready wait and later rapid-switch requests hit IPC refusal.

## EXP-REP-002 BYTES Runtime Boundary

Hypothesis:
Waveform file chunks, decoded pages, blob payloads, and renderer upload buffers
should use engine-native bytes, not `TEXT`, JSON arrays, or nested generic
records.

Current state:
- `boon_bridge::BridgeValue::Bytes` exists and is backed by `bytes::Bytes`.
- Runtime/typechecker values are still missing first-class bytes.
- NovyWave labels, file paths, visible values, statuses, formulas, and UI text
  must stay `TEXT`.

Queued implementation shape:
- Add internal runtime bytes only behind inferred bridge/file contracts.
- Add byte/page/blob refs for large or streamed payloads.
- Keep deterministic replay and useful type diagnostics.

Kill criteria:
- Boon examples need manual byte annotations before inference is ready.
- Binary payloads are immediately converted back to JSON/text.
- Large byte payloads appear inline in state summaries.

## EXP-REP-003 Container Replacement Only At Hot Boundaries

Hypothesis:
Some `BTreeSet`/`BTreeMap` sites are accidental, but replacing them one by one
is likely noise. The useful target is a whole hot boundary: a dependency graph,
dirty-set representation, lookup index, or LIST materialization path.

First implementation slice:
- Replace the combinational-cycle verifier's repeated sibling scans with a
  precomputed same-parent dependency graph and DFS completion state.
- Share a memoized candidate-source index across dependency edges, possible
  causes, and update-branch derivation.

Why this first:
- The measured NovyWave lower profile shows `verify_combinational_field_cycles`
  at about 21.3s.
- The current pass repeatedly scans fields and tokens during recursive DFS.
- The graph keeps the same semantic dependency rule but avoids rebuilding it
  for every traversal.

Acceptance:
- Existing cycle diagnostics still report the same HOLD guidance.
- NovyWave lowering profile shows the cycle verifier phase materially lower.
- No parser/runtime syntax or Boon example workaround is introduced.

Status:
- Implemented and kept. The cycle verifier dropped from about 21.3s to about
  25.5ms in the measured NovyWave source-replacement profile.
- Shared candidate-source indexing reduced `dependency_edges`,
  `possible_causes`, and `update_branches` from multi-second phases to
  millisecond-scale phases in the same profile.
- Implemented and kept a runtime dirty-root scheduler improvement. The engine
  now keeps read-key counts for the dirty root set while popping roots in
  dependency order, instead of rebuilding path-read overlap checks for every
  candidate pair. NovyWave debug `initialize_generic_derived_first_ms` dropped
  from about 10.4s to about 435ms in the focused source-replacement test.

## EXP-REP-004 LIST Storage Modes

Hypothesis:
User-facing `LIST` should remain generic, while compiler/runtime storage can be
constant array, dense `Vec`, selection view, indexed list, incremental
projection, or virtual list when provable.

Queued implementation shape:
- Add a generic list-shape classifier before changing execution.
- Promote one storage mode only when a hot shape is measured.
- Preserve full generic LIST execution as the oracle.

Likely later targets:
- Direct root `List/map` materialization.
- Selection complement without tree sets.
- Incremental text/equality lookup indexes.
- Generic virtual list windows for large collections.

## EXP-REP-005 Constants And Hoisted Templates

Hypothesis:
NovyWave and bundled examples contain many literal labels, separators, field
names, enum tags, source names, static row fragments, and bridge-contract
constants. These should be interned or hoisted only when dependency analysis
proves they cannot depend on `SOURCE`, `HOLD`, row fields, bridge payloads, or
file data.

Queued implementation shape:
- Start with compiler/runtime diagnostics that classify constants, row
  invariants, and dynamic expressions.
- Hoist row templates only after LIST/root materialization has a correctness
  oracle.
- Emit compiler errors only for real semantic ambiguity; do not ask users for
  manual annotations to satisfy an optimization.

Kill criteria:
- A dynamic value freezes.
- Error messages mention lowered internals instead of source concepts.
- The speed win comes from hiding semantic work rather than removing it.

## Immediate Execution Order

1. Done: enable example-switch preview frame readback so the binary source
   transport gate has real app-owned evidence.
2. Done: optimize the combinational-cycle verifier with a precomputed graph.
3. Done: re-measure NovyWave source replacement and update the lower profile.
4. Done: share candidate-source indexing across dependency edges, possible
   causes, and update branches.
5. Done: profile generic-derived initialization and replace repeated dirty-root
   path scans with maintained dirty read-key counts.
6. Done: add a typecheck function/call-site index for argument type inference
   and recursive diagnostics.
7. Done: add a source-payload lookup index for source-shape and expression path
   typing.
8. Done: share the runtime parsed project with preview layout proof so
   source-replacement does not parse the same project twice.
9. Next: target document evaluation/lowering cost or cancellable source
   replacement.
10. Only move BYTES into runtime/NovyWave payloads when the value is truly
   binary waveform/blob/page data and the change avoids real copies or JSON/text
   conversions.
