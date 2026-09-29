# Boon Compiler: Profile-Driven Speed Plan

Written: 2026-09-28. Status: **active compiler execution plan.** It supersedes
the work ordering in `BOON_COMPILER_PERFORMANCE_PLAN_INTERNAL.md` (M-items) and
`BOON_COMPILER_REQUIREMENT_AGGREGATION_GOAL_PROMPT.md` (Goals B/C). The
measurement contract in `BOON_COMPILER_PERFORMANCE_PLAN.md` and the budgets in
`budgets/compiler.toml` stay in force. Nothing here changes Boon syntax or
semantics; the two places where a language decision *could* help are listed at
the end as questions for the owner, not as work.

## 1. Position

Single fresh-process samples on the release producer, i7-9700K, before and
after the one change landed with this plan (section 3, T0). Budgets in
parentheses.

| fixture / intent | before | after T0 | budget |
| --- | ---: | ---: | ---: |
| TodoMVC diagnostics (3,576 lines) | 851 ms | **466 ms** | 75 |
| TodoMVC verified | 1238 ms | **852 ms** | 300 |
| NovyWave diagnostics (11,926 lines) | ~780 ms | ~760 ms | 250 |
| NovyWave verified | 1890 ms | 1875 ms | 1000 |
| counter verified | 15.7 ms | 16.0 ms | 50 |
| warm edit-to-ready (TodoMVC, type-preserving edit) | ~880 ms | not re-measured | 16.7 |
| warm verified preview | ~2100 ms | not re-measured | 100 |

NovyWave verified phase split after T0: parse 65, typecheck 731, semantic 783,
IR 24, backend 200, plan validation 66.

## 2. Diagnosis: why three months of compiler work did not move the number

1. **Attribution was wrong, because no sampling profiler was ever used.** Every
   prior plan derived its target from hand-placed `Instant` timers at phase
   boundaries and from work counters. The first `samply` profile of the release
   binary (see the memory note `compiler-profiling-setup` and
   `docs/plans/evidence/compiler-profile-driven-t0-2026-09-28.json`) showed that
   36% of TodoMVC diagnostics time was one predicate,
   `direct_result_summary_supported` in `crates/boon_compiler_kernel/src/owner.rs`,
   which re-walked every callee's whole expression graph once per call path and
   visited the `THEN` arm twice per level (2^depth). The K1/K1′/K1″ sequence
   spent thirteen landed slices and five rejected experiments on the
   requirement-aggregation fold (28-30% of *solve*, i.e. ~13% of the run) while
   this sat unattributed inside "kernel compile". A 40-line memo removed it
   with byte-identical output.
2. **The product path proves its own output, repeatedly.** On NovyWave roughly
   45% of the semantic phase (~350 of 783 ms) is receipts, CBOR+SHA-256
   digests, the dependency manifest, the construction image, and validation of
   structures the same process just built. `verify_plan` (41 checks plus a
   full plan hash) runs on the plan the backend just sealed, then again in
   `boon_program_runtime::artifact_from_compiled`. The IR is verified twice.
   The playground's EditorDiagnostics path runs a parity oracle
   (`derive_checked_order_chains`) in production, and an error in a
   RuntimePacked check throws the solve away and re-runs the whole typecheck
   as EditorRich. None of this reaches the runtime plan.
3. **There is no cross-edit reuse at all**, although the solved state was
   measured 100% identical for the benchmark edit (M0/M4 evidence). Every edit
   is a cold solve. The warm budget (16.7 ms) is ~50x away and is the product
   metric.
4. **Dead weight.** ~58k lines of `boon_typecheck` are compiled out by a
   feature gate, ~23k lines of `boon_compiler` are tests/oracles, four empty
   crate directories sit untracked under `crates/`, and `boon_cli/build.rs`
   re-runs on any tracked-file change, shelling out to git eight times and
   recompiling mimalloc. This costs build time and attention, not runtime.
5. **Plans outnumber changes.** `docs/plans` holds ~13k lines of compiler plan
   text with four documents each claiming to be the authority. The
   measurement protocol (3+30 interleaved observations in two lanes and two
   cold modes per candidate) is heavier than the changes it gates and pushed
   work toward what could be counted rather than what was slow.

## 3. Work, in order

Each tier is independently landable and measurable. Gate for every item: same
`plan_sha256` and `diagnostics_fingerprint_v1` on counter, TodoMVC and NovyWave
unless the item is an explicitly declared digest migration; kernel lib tests
(198) and `boon_compiler` suites green; interleaved A/B of at least 8
observations per cell for development, the full 3+30 protocol only at
checkpoints.

### T0 — landed: memoize `direct_result_summary_supported`

`crates/boon_compiler_kernel/src/owner.rs`: the predicate is a conjunction over
the reachable expression graph and a cycle anywhere makes every node above it
unsupported, so the answer is path-independent. A per-`(owner, expression)`
state table (unvisited / active / no / yes) replaces the path set. TodoMVC
diagnostics −45%, verified −31%, NovyWave and counter neutral, outputs
byte-identical, 198 kernel tests green.

### T1 — kernel solve: the transitive reschedule walk

`ComponentSolver::schedule_variable` (`solver.rs:4057`) is now 22.7% self time
of TodoMVC diagnostics. On every bind of a requirement-managed variable it
walks the equivalence class, then the transitive `binding_dependents` closure
(every variable whose binding term contains this one, recursively), queueing
consumers and marking requirement destinations dirty. Nested record types
make that closure the whole ancestor chain, and it is re-walked from scratch on
every bind because the visited stamp is per call.

Measured on TodoMVC diagnostics (counters in the profiling build, not the
product): 154,287 calls, 11.83M variable visits, 16.66M consumer edges examined,
44,665 executed operations. **10.95M of the 11.83M visits (92.5%) are
re-visits of a variable already walked since the previous executed
operation**, i.e. inside the same activation, where nothing can have been
dequeued. `binding_dependents` edges account for only 95k of the visits; the
cost is enumerating large equivalence classes and their consumers again and
again. NovyWave: 83,553 calls, 317k visits, 55% re-visits.

**Landed (T1a):** a per-variable *reschedule epoch* stamp. The epoch ends
whenever an operation is dequeued (`solve loop`, `solver.rs`) or a requirement
dirty flag is consumed (`refresh_requirements`, `pop_dirty` loop), because only
those events can undo an earlier walk; inside one epoch a variable already
walked is skipped. Interleaved A/B against the T0 binary, outputs
byte-identical, 198 kernel tests green:

| cell | T0 | T1a | delta |
| --- | ---: | ---: | ---: |
| TodoMVC diagnostics p50 | 461 ms | 390 ms | −15.4% |
| TodoMVC verified p50 | 974 ms | 887 ms | −8.9% |
| NovyWave verified p50 | 2042 ms | 1956 ms | −4.2% |

(The T0 column is higher than section 1's numbers because the machine was
busier during this run; A/B interleaving makes the deltas comparable, the
absolute values are not.)

The three small cleanups below plus the T2 field lookup were built together
and A/B'd against the T1a binary: TodoMVC diagnostics +0.6% (noise), NovyWave
verified −3.1% in a noisy run with test suites executing concurrently.
Kept as correct simplifications with no latency claim; outputs byte-identical,
198 kernel tests and all `boon_compiler` suites green.

Remaining options if the walk is still visible in the next profile:

1. Batch changed variables per activation and walk once before the next
   `refresh_requirements`.
2. Structural: CSR reverse index built once per program and a bitset walk
   instead of nested `Vec<Vec<_>>` with `Vec::insert` maintenance.

Related, same file, each small and byte-identical:

- `requirement_closed_pairs.insert` ran on every closed merge in release even
  with the probe off (`solver.rs:3276`); an unbounded SipHash set. **Landed:**
  gated on the probe.
- `occurs`/`term_occurs` (`solver.rs:3855, 4132`) has no DAG memo; shared
  subterms are re-walked per bind.
- `term_syntax_selected` (`solver.rs:4046`) walks the full term DAG for every
  input of every publish/record/select.
- `resolve_term` re-interns whole terms with a cache that lives one call.

### T2 — term arena and object handling

`crates/boon_compiler_kernel/src/term.rs`: `lookup_object_field` (`:1290`)
binary-searched by string compare and each compare re-validated UTF-8 through
`ProjectTextSnapshot::symbol` → `from_utf8` (1.3% self time on its own);
**landed:** it now compares lexical ranks, which order identically. `intern_object`
(`:2343`) copies, sorts and re-derives the semantic order on every call
including the 84% that are hits. `object()` (`:1405`) de-duplicates with
`position` (O(n²)). `union` sorts with a recursive string-comparing
`compare_terms`. Expected: a few percent each; do them together and measure once.

### T3 — kernel compile: per-call bookkeeping

`owner.rs`: `specialize_owner` runs, hashes and clones a boxed
`OwnerSpecialization` for all 10,537 compiled calls before the three early
exits at `:20624/:20638/:20690`; move it after them. The env var
`BOON_KERNEL_DISABLE_DIRECT_SUMMARIES` was read per summary call site
(`:20744`); **landed:** read once through a `OnceLock`. `infer_static_variants` allocates per merge node per
round. `ComponentProgramBuilder::finish` (`program.rs:1132`) makes ~5 full
passes and runs `visit_program_topology` twice. After T0 the whole compile
phase is ~4% of TodoMVC verified, so this tier is small; do it only after T1/T2.

### T4 — back half: stop proving our own output on the preview path

`boon_semantic`, `boon_plan`, `boon_compiler`, `boon_program_runtime`. This is
the NovyWave lever (typecheck is only 39% there).

1. A preview/product mode of `elaborate_with_representation` with no receipts
   (`ExecutionReceiptPublisherV5`, `seal_execution_expression_proof_v2`), no
   dependency manifest V7, no construction image V5/routes, no six per-graph
   CBOR+SHA digests, and `execution.validate` / `validate_freshly_constructed`
   / `resource graph.validate` under `debug_assertions`. `SemanticProgramDigest`
   becomes a cheap composite (source bundle digest + checked image seal). The
   only runtime-visible digest, the pulse `slice_digest`, must stay non-zero;
   make it a structural hash. Estimated NovyWave semantic 783 → ~420 ms.
2. `boon_plan::seal_machine_plan`: a crate-private trusted seal for in-process
   plans that computes `plan_sha256` and skips the 41-check `verify_plan`;
   full verification stays for `decode_program_artifact` and
   `MachineTemplate::new` (untrusted/loaded plans). Remove the second
   `verify_plan` in `artifact_from_compiled` (`program_core.rs:698`). ~66-90 ms
   per fixture.
3. `boon_ir::erase_and_lower` already runs `verify_static_schedule` and
   `verify_hidden_identity`; `finish_checked_program_to_machine_plan`
   (`boon_compiler/src/lib.rs:1409`) runs them again. Drop the second pair.
4. `PlanRowExpressionArena::intern` hashes every node with SHA-256 including
   dedup hits, and nodes are re-interned three times (`intern`,
   `refresh_typed_list_view_fingerprints` after a whole-plan `plan.clone()`,
   `compact_machine_plan_row_expressions`). Intern by FxHash + structural
   equality, compute fingerprints during lowering, drop the clone.
5. `erased_field_is_runtime_row_storage` (`machine_plan_backend.rs:8456`) scans
   all scope bindings per field from ten call sites (4.2% self on NovyWave);
   build the excluded-field set once per program.
6. Harness: `compiler-sample` exports a 20 MB pretty-JSON plan and hashes it
   (~100 ms); hash canonical bytes instead. Product-path only; it is not in
   the scored `elapsed_ms` but it is in every developer's loop.

Items 1 and 4 change what is hashed. They are explicit digest migrations under
the oracle lane the internal plan already reserves; they do not touch
`machine_plan_sha256` unless the plan content changes, and every A/B must show
that it does not.

### T5 — warm retention (the product metric)

`crates/boon_compiler/src/session.rs` clears `state.checked` on every update;
`KernelSession::new` is constructed per request; `replace_project` has no
production caller. Land in this order, each with the interactions collector:

1. Retain the previous revision's `KernelSession` and
   `PreparedKernelProjectProjection` in `ProjectState`. Key on the existing
   `basis_fingerprints_v14` plus a per-owner content fingerprint
   (`owner_program_fingerprint`, already release-visible under
   `BOON_COMPILER_OWNER_FINGERPRINT_TRACE`). If every owner's content
   fingerprint is unchanged, reuse the solved revision and re-run only span
   rebasing and diagnostics presentation. M4's measurement says this is 155/155
   owners for the budget's type-preserving edit; it should take warm
   diagnostics from ~880 ms to parse + presentation (~10-25 ms).
2. Same-revision reuse: `CompileIntent::Diagnostics` followed by a verified
   request on the same revision solves twice; `state.checked.take()` after
   verified forces a third solve for the next EditorDiagnostics. Share one
   retained solved revision across intents.
3. Dirty cones for type-changing edits: solve only owners whose content
   fingerprint changed plus their dependents (the receipt dependency graph
   exists in `receipt.rs:391`), backdate unchanged dependents, publish
   last-good. The arena is append-only (`term.rs:2485`) so retained term ids
   stay valid.
4. Remove the EditorRich parity oracle (`kernel_oracle.rs:3091-3102`) and the
   error-path full re-run (`:3105-3119`, `:2931`) from the product path;
   diagnose from the retained solve.

Warm preview then reduces to T4's back half on a reused checked image; M5
(delta semantic construction) is only needed if that is still over 100 ms.

### T6 — structural: the solver design (research, not scheduled)

Both kernel audits reached the same conclusion independently: frames are call
*paths*, not call sites (10,537 compiled calls from ~2,700 syntactic calls on
TodoMVC), because the kernel compiles every definition into one global graph
before solving, and the solver is an incremental Datalog-with-retraction engine
applied to a batch problem (eager cone invalidation, per-destination retained
fold state, order receipts). A per-definition scheme solved bottom-up over
call-graph SCCs, with retraction confined to selector-dependent arms, would
remove most of that work. It is also the riskiest change in this document
(byte-exact field order, `syntax_selected` provenance, hole erasure) and is
worth opening only if T1-T5 leave cold typecheck over budget. First step if
opened: make authored field order a side table so type identity becomes
order-free; that alone deletes the order-receipt machinery.

### T7 — dead code and build loop

- Delete the four untracked empty crate directories `crates/boon_bridge`,
  `boon_driver`, `boon_ply_playground`, `boon_transport_json`.
- `boon_typecheck`: 58k lines already behind `cfg(any(test, feature =
  "legacy-owner-oracle"))`. Keep `CheckedProgramDatabase` (used by distributed
  packages); move the rest to a separate legacy crate or delete once no test
  needs it. `kernel_oracle.rs` (24k lines, `allow(dead_code)`) should split its
  12k-line test module out.
- `boon_cli/build.rs`: stop emitting `rerun-if-changed` for every tracked
  file; watch HEAD, the index and the mimalloc source only.
- Drop the `[profile.test.package.*] opt-level = 0` overrides for the three
  large compiler crates once `cargo nextest` is adopted, so test and dev
  fingerprints match.
- Profiling: keep `target/profiling` as a second target dir with
  `CARGO_PROFILE_RELEASE_DEBUG=line-tables-only`; samply needs
  `perf_event_paranoid <= 1`.

## 4. Expected position

Estimates from the profiles, not measurements; non-additive.

| | TodoMVC diag | TodoMVC verified | NovyWave verified | warm diag |
| --- | ---: | ---: | ---: | ---: |
| now (after T0) | 466 | 852 | 1875 | ~880 |
| T1 + T2 | ~300 | ~680 | ~1700 | ~700 |
| T4 | — | ~450 | ~1000-1100 | — |
| T5 | — | — | — | **10-30** |

Cold budgets (75 / 300 / 1000) are still not reachable without T6 or a
persistent artifact cache; the warm budget is reachable with T5 alone, and warm
is what the playground user feels.

## 5. Questions for the owner (language surface)

Nothing above needs an answer to proceed. Two facts would change T6's shape:

1. **Is authored record field order semantically observable in Boon** (does
   `[a: 1, b: 2]` ever behave differently from `[b: 2, a: 1]` at runtime or in
   rendering)? If not, field order can leave type identity and the solver's
   order-receipt machinery goes away.
2. **Would optional parameter annotations on `FUNCTION` be acceptable in the
   future** as inference firewalls? Today every function is fully
   polymorphic, which is what forces per-call-path instantiation. This is not
   proposed; it is the one place where syntax and compiler cost are linked.

## 6. Measurement discipline, simplified

- Attribute with a sampling profile of the release binary first; timers second.
- Develop against interleaved A/B (8-10 observations, prebuilt binaries),
  fingerprint equality on all three fixtures, and the kernel/compiler suites.
- Run the 3+30 two-lane protocol once per tier checkpoint, and the warm
  interactions collector once per T5 slice.
- Do not open a new plan document; append dated status to this one.
