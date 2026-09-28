# Boon Compiler Performance Plan — Internal

Status: **active compiler execution plan.** It supersedes the work sequencing in
`BOON_COMPILER_REQUIREMENT_AGGREGATION_GOAL_PROMPT.md` (Goals B and C) while
leaving that document's measurement discipline, evidence format and correctness
constraints in force. It changes no language surface, no user-facing behavior and
no semantic meaning; the only observable difference is latency.

## Decision Boundary

This plan is engineering-owned. It is written so the repository owner does not
have to adjudicate it.

Only two questions are reserved for the owner:

1. **The oracle lane.** `budgets/compiler.toml` is the sole executable budget and
   oracle authority. A compiler that ships a generated artifact needs the
   producer's attested build-input scope to cover that artifact, which forces an
   oracle re-baseline. Recommendation: approve a separate, explicitly attested
   lane for it, and never fold it into a performance candidate.
2. **Anything that would change what a Boon program looks like.** Nothing in this
   plan does. If an item turns out to need one, it stops and is escalated rather
   than reclassified.

Everything else — attribution, duplicated-pass removal, the freeze, shape-keyed
specialization, retention mechanics, delta semantic construction, harness
repair — is decided here.

## Measured Position at the Starting Point

Release producer at `e7065292`, `compiler-sample --mode fresh-process --samples 1`.
These are single interleaved samples, not acceptance numbers; the acceptance
position is in the evidence files.

| phase | TodoMVC (3,576 lines) | NovyWave (11,926) |
| --- | --- | --- |
| parse | 52 ms | 93 ms |
| typecheck | 1042 ms | 681 ms |
| semantic (Manifest) | 212 ms | 1083 ms |
| contract_verify | 0.1 ms | 0.1 ms |
| ir_lower + ir_validation | 10 ms | 36 ms |
| backend | 130 ms | 291 ms |
| plan_validation | 117 ms | 81 ms |
| harness-only pretty-JSON export | 122 ms / 20.4 MB | 82 ms / 13.3 MB |
| warm edit-to-ready | 863 ms p50 / 883 ms p95 (budget 16.7) | — |
| warm verified preview | 2145 ms p50 / 2186 ms p95 (budget 100) | — |

Three facts set the ordering:

- The requirement-aggregation refresh is now **142 ms / 87 ms**, i.e. 11-13% of
  typecheck. It is no longer the dominant cost and is not where this plan starts.
- **~725 ms of TodoMVC typecheck and ~580 ms of NovyWave typecheck are
  unattributed.** The `*_us` sub-phase timers live in the Oracle-only bridge;
  production discards `owner_projection_us` and `dependency_pruning_us`
  (`crates/boon_compiler/src/kernel_oracle.rs:2388`). No target for the largest
  cost in the compiler has been measured. This is item M0 and it blocks the rest.
- Routing call sites through the pre-compiled residual-module path instead of the
  interpreted summary path is **52.7% slower on TodoMVC and 433% slower on
  NovyWave**, with byte-identical diagnostics fingerprints. The conditional
  linked-transfer-code successor is therefore closed as unnecessary-and-harmful.
  Do not reopen it.

The warm numbers are measured on `CompileIntent::Diagnostics`, which writes a
different session slot than `EditorDiagnostics`/`VerifiedPreview`
(`crates/boon_compiler/src/session.rs:731-780`) and therefore solves the document
twice per edit. The product playground uses `EditorDiagnostics` then
`VerifiedPreview` (`crates/boon_native_playground/src/compile.rs:540,598`) and does
not. M6 re-baselines before any warm optimization targets a number.

## Ordering Principle

Attribute, then delete duplicated passes, then make static facts static, then
make work demand-driven, then make the back half incremental. Each item must be
landable and measurable on its own; no item may depend on a later one to be
correct.

## Work Items

### M0 — Attribution (blocks everything) — **COMPLETE**

Recorded in
[M0 attribution and solved-state cone](evidence/compiler-m0-attribution-2026-09-27.json).
Instrumentation is stderr-only and environment-gated under
`BOON_COMPILER_PHASE_TRACE`, `BOON_COMPILER_OWNER_STATE_TRACE` and two added
`BOON_PARSER_TRACE` phases, none of it `debug_assertions`-gated. Behavior
neutrality is proved by building the release producer twice from source differing
only by the instrumentation: all three `machine_plan_sha256` values are identical.
Focused gate: `boon_compiler_kernel` lib 198 passed.

The findings changed the plan:

| | TodoMVC (3,576 lines) | NovyWave (11,926 lines) |
| --- | --- | --- |
| parse | 27.3 ms | 98.6 ms |
| typecheck | 1099.4 ms | 625.0 ms |
| — project prepare | 23.2 | 99.9 |
| — kernel input | 16.7 | 57.7 |
| — **kernel compile (prepare)** | **583.0** | **136.3** |
| — **kernel solve** | **474.1** | **316.0** |
| — present diagnostics + receipt | 0.9 | 3.0 |
| requirement refresh, share of solve | 142 ms (30%) | 87 ms (28%) |

1. **The two halves are different problems and the ratio inverts.** TodoMVC is
   compile bound (583 vs 474) with 10,537 call sites for 155 definitions. NovyWave
   is solve bound (136 vs 316) with 1,389 definitions and 2,699 call sites. The
   plan treated "the solve" as one target; it is two, and per-call-site work would
   barely touch NovyWave. This raises M3's priority for call-site-heavy code and
   lowers its value for definition-heavy code.
2. **Parse is not the cost the plan assumed.** The `validation_visits` counter
   suggested the index build plus four validators were ~70% of parse. They are
   2.7 ms of TodoMVC's 27.3 ms and 15.3 ms of NovyWave's 98.6 ms. The untraced
   remainder is project-level module resolution and link assembly. Parse is
   de-prioritized.
3. **A stale oracle, not a regression.** `budgets/compiler.toml`'s
   `machine_plan_sha256` for `todo-mvc-physical` and `novywave` do not match what
   HEAD produces; only `counter` is current. The unmodified HEAD source reproduces
   the same two hashes, and the K1'' checkpoint already discloses a TodoMVC plan
   change. The budget file was **not** edited. Until the oracle lane
   re-establishes it, a plan-hash gate must compare against the pre-change HEAD
   hash.
4. **The dirty cone, measured on solved state, is 100% reusable.** A per-definition
   digest of the solved result and formal flows is unchanged for **155/155**
   TodoMVC definitions and **1389/1389** NovyWave definitions, for a
   type-preserving edit *and* for a real type change. This closes the
   counters-are-not-results caveat in the cone probe. The digest covers published
   interfaces only, so it is not a sound reuse key alone; a sound key pairs it
   with the existing term-id-free `definition_basis_fingerprint` for the inputs.
5. **Half the typechecker's reported counters are structurally zero.** All 18
   `context_scheme_*`, `wrapper_scheme_*` and `checked_flow_*` counters read 0 on
   both fixtures, because `boon_typecheck`'s inference engine has no production
   caller and the kernel path passes `TypeCheckWorkCounters::default()`. This
   retires M2 and invalidates the premise of the external review that prompted
   it. See [M2](#m2--freeze-static-facts--retired-as-unnecessary) and the
   [evidence record](evidence/compiler-legacy-typechecker-not-in-production-2026-09-27.json).

### M1 — Delete duplicated passes

Each is a local change with no semantic content. All are verified in the tree.

1. `row_expressions.validate()` runs at least twice per compile —
   `crates/boon_compiler/src/machine_plan_backend.rs:7545` and
   `crates/boon_plan/src/lib.rs:10978`. **Parked, not waste:** the two calls sit
   in different phases (`finalize_machine_plan_row_expressions` and
   `seal_shared_machine_plan`), and the seal-time one is the authoritative gate.
   Removing either weakens an error path rather than removing duplication.
2. **DONE** — per-root reachability walk replaced by one shared-visit walk.
   `walk_postorder_many` is now public (`crates/boon_plan/src/lib.rs`) and
   `validate_machine_plan_row_expression_reachability` calls it once with all
   roots. Each previous single-root call allocated its own visited set, so a
   child shared by two roots was walked, ordered and inserted twice.
3. `plan.clone()` of the whole plan in `refresh_typed_list_view_fingerprints` —
   `machine_plan_backend.rs:7341`. **Parked:** the clone is a borrow-checker
   workaround, because `TypedListViewFingerprintContext` borrows
   `row_expressions` while the rewrite closure mutates it. A partial fix
   restructures that borrow, which needs its own measurement.
4. **DONE** — `PlanRowExpressionArena::intern` computed `canonical_sha256(&node)`
   for the index lookup, then called `push`, which hashed *the same node* again
   and re-ran `validate_new_node`. A private `push_with_key` now takes the key
   `intern` already computed. Note the rejected alternative first: comparing nodes
   before hashing turns the index lookup into a linear scan over every node and
   is far worse. The key is exactly what `push` would have recomputed, so index
   contents and every digest are unchanged.
5. **FALSIFIED — do not implement.** This item claimed every semantic execution
   row is CBOR+SHA'd twice (`semantic_image.rs:2643` payload and `:2658` row
   fingerprint) and that one preimage would serve both. It is false. The two
   hashes have different domains and different inputs:
   `payload_digest = H(ROW_PAYLOAD_DOMAIN, CBOR(payload))` and
   `row_digest = H(ROW_DOMAIN, CBOR({stable_key_digest, domain, payload_digest,
   relocations}))`. The second is not a re-hash of the first, and 12 of the 73
   `push`/`push_presealed` call sites never hash a payload at all. The measured
   127 ms on NovyWave (`payload_hash_ms=75.0`, `row_hash_ms=52.1`) is necessary
   domain-separated hashing, not duplicated work. Removing either hash would
   break the artifact digest contract for no gain. The entry is retained so the
   claim is not re-derived from the plan text later.
6. `verify_plan` recomputed on an unchanged plan — `crates/boon_plan/src/lib.rs:10977`.
   It is a pure function of the sealed plan; memoize on the plan digest, which is
   already computed. Its 41 checks are independent and may run in parallel.
7. `structural_widen_cache` is unsymmetric and uses std SipHash although
   `structural_widen_uncached` is commutative — `crates/boon_compiler_kernel/src/term.rs:593`.
   Order the pair and use the arena's own open-addressed table. *(Not yet tested;
   see the rejected canonical-set candidate below for what "not yet tested" must
   mean here.)*
8. `intern_object` re-derives lexical rank inside a binary search per field —
   `term.rs:2314`. One sort of `(rank, index)` pairs.

**Items 7 and 8 are now partly resolved by measurement — read this before
revisiting them.** An external research review identified three further
quadratic algorithms in the term arena, all verified in source and then measured
with new `BOON_TERM_SIZE_TRACE` probes *before* any restructuring:
[`canonical set operations rejected`](evidence/compiler-canonical-sets-rejected-2026-09-27.json).

| observed maximum | TodoMVC | NovyWave |
| --- | --- | --- |
| union members before sort | 46 | 42 |
| object fields widened (left / right) | 43 / 36 | 32 / 32 |
| variant set merged length | 16 | 8 |

Removing the `union()` membership prefilter and replacing the O(L*R) object-widen
field scan with a linear merge join was implemented, gated (198 tests, all three
plan hashes byte-identical) and measured at **+1.4%, +3.7%, −0.8%, +1.3%** across
the four fixture/intent cells. Rejected and reverted with no logic change. The
quadratics are real in the source and unreachable at this scale, and the merge
join pays a lexical-rank lookup per step that costs more than the bounded scan it
replaces. The probes are retained so any future restructuring decision is made
from measured sizes rather than from asymptotic reasoning.

Items 5 and 6 change hash *computation*, so they require the controlled oracle
migration the performance plan already contemplates: re-establish the affected
digests from a fresh producer and record the migration in evidence. They do not
weaken a budget or a gate.

Expected: 300-450 ms off the verified path, low risk, independently gated.

**Items 2 and 4 are DONE and measured NEUTRAL** — see
[`M1 items 2 and 4`](evidence/compiler-m1-items-2-4-neutral-2026-09-28.json). All
three plan hashes byte-identical, `boon_plan` 59 and `boon_compiler_kernel` 198
green, one fingerprint per lane. Interleaved 3+30 A/B: min −1.21% / +0.24% /
−0.44% / +0.26% across the four cells, no consistent sign. Kept as correct
simplifications with **no latency claim**. The 300-450 ms estimate was never
attributed to specific items, and items 2 and 4 demonstrably do not deliver it;
the unattributed share is in items 5 and 6, which remain unbuilt.

### M2 — Freeze static facts — **RETIRED as unnecessary**

Recorded in
[legacy typechecker not in production](evidence/compiler-legacy-typechecker-not-in-production-2026-09-27.json).

This item existed to eliminate a global context/wrapper fixed point that the
compiler re-derived on every request. **That fixed point is not in the
production path.** All 18 `context_scheme_*`, `wrapper_scheme_*` and
`checked_flow_*` counters read 0 on both fixtures, and the cause is structural:
`boon_typecheck::checked_program` has no production caller (every external call
site is `#[cfg(test)]` code in `crates/boon_semantic/src/out_net.rs`), and the
kernel path constructs `TypeCheckWorkCounters::default()` and never mutates it
(`crates/boon_compiler/src/kernel_oracle.rs:3254`).

Do not implement. The underlying observation — that the compiler re-derives
source-static facts per request — is still true of the *kernel*, but there is no
measured cost to remove, and M4 already addresses the kernel's per-revision
rebuild directly.

**Corollary that changes how every other item must be argued:** the compiler has
two typecheckers, and only the dense kernel is in production. Any proposal
justified by a `boon_typecheck` counter is describing the test path until it
shows a non-zero production reading. This also means the earlier crate-size
figures overstated production surface: `boon_typecheck` is 109,295 lines, most of
which does not execute in a compile.

**What is still true, and what is retired.** The general observation holds for
the kernel: it re-derives source-static facts per request, and M4 addresses that
directly. The specific mechanism this item targeted — a global context/wrapper
fixed point in `boon_typecheck` — is not in production, so there is nothing here
to remove. The design text that followed (the generated per-module record, the
fail-closed fingerprint rule, the attestation scope change) is preserved in the
git history of this file at commit `5c282b27` should the item ever be revived,
which would require evidence that the kernel does pay for the re-derivation.

**Withdrawn language proposals.** An external research review recommended
converting `PASSED` into a per-path effect requirement, and an earlier pass of
this plan proposed declaring context formals (A), lexical-only `PASSED` (K), a
`WHERE DEPENDS ON` clause (E) and declared module exports (I). All four are
withdrawn. An independent verification pass found the review's central premise
refuted (no context type reaches either specialization key; requirements are
already per-path with lazy projection, which is the design the review proposed)
and its fixed-point claim refuted empirically (all scheme counters read 0). There
is no measured cost behind any of these proposals.

### M3 — Shape-keyed specialization

`InvocationKey` keys on the **caller's** variable ids
(`crates/boon_compiler_kernel/src/owner.rs:15364`), so two call sites passing the
identical tag instantiate the body twice. `SpecializationKey` (`:15430`) carries
no argument types at all. TodoMVC: 155 definitions, 414 specialization plans,
10,123 plan reuses, 10,537 call sites.

Boon's polymorphism here is not parametric — it is finite tag dispatch over a
closed set the compiler already knows (exhaustive `WHEN` is checked). The one
genuinely parametric part is the collection-operator and `OUT`-forwarding core,
which must keep the interpreted summary.

**Step 1 is measurement, not code.** Instrument the instantiation cache to report
distinct `(target, resolved argument shape)` pairs against distinct
`(target, caller variables)` pairs. The ratio is the payoff and the ceiling. If
the ratio is small, record the idea as closed with that evidence, exactly as M2's
predecessor was closed.

**Step 2, only if the ratio is large:** monomorphize the finite-shape majority
per distinct resolved shape, keep the interpreted summary for the parametric core,
and add `fn` as an opt-in escape hatch for functions whose shape domain is
explosive. Determinism is preserved by reassembling in canonical index order, not
completion order.

### M4 — Retain the solved revision

Contract unchanged; see `BOON_COMPILER_REQUIREMENT_AGGREGATION_GOAL_PROMPT.md`
Goal B. `crates/boon_compiler/src/session.rs:480` clears `state.checked` on every
update, forcing a full 155-owner solve per edit. Retain the packed solved
revision, dirty-cone it, backdate, and publish last-good. Reuse keys are exact
semantic input tuples plus dependency epochs — never result-type equality.

**Gate before implementation — MET, with one caveat.** The plan required the
affected-owner count for the benchmark edit before any retention work. It is
measured, in
[the cone probe](evidence/compiler-cone-probe-2026-09-27.json), without any
compiler change:

| edit | TodoMVC (3,576 lines) | NovyWave (11,926 lines) |
| --- | --- | --- |
| type-preserving (`Walk the dog` → `Walk the dogs`) | **every typecheck counter byte-identical**; cost 946 ms | **every typecheck counter byte-identical**; cost 602 ms |
| type-changing (`+1` field on one row / tag payload) | `variables` +1, `operations` +1 of 82,451; cost 874 ms | `variables` +1, `operations` +1 of 39,252 |

`summary_node_evaluations` (416,698 / 1,337,136), `aggregate_evaluations` and
every invalidation counter are unchanged in both cases, and the type store
genuinely grew in the type-changing case, so those were real type changes.

**What this changes about the plan.** The typecheck workload is a function of the
program's *shape*, not of the edit. The 53x warm gap is therefore **not** a
conservative-invalidation problem — it is a missing cache over a delta that is
already almost zero. M4 moves from "the biggest unknown" to "the best-supported
item in the plan".

**Caveat, and it is load-bearing.** Counters are not results. The probe compares
work counters, not per-owner solved type tables; two programs can do identical
work and differ. M0's per-owner solved-state digest exists to close this, and it
should land before M4's correctness argument is written, not after. The
type-changing edits probed are the mildest available and are lower bounds on the
delta, not upper bounds.

### M5 — Delta semantic construction

After M4 this is the entire remaining preview cost (212 ms TodoMVC, 1083 ms
NovyWave). `dependency_manifest.rs` `finish_compact_v7` (`:5084`) is a whole-program
fixed point; `validate_integrity_handoff` (`crates/boon_semantic/src/lib.rs:980`)
re-streams the entire checked program on the warm path. Build per-owner and
per-domain, and skip the full-program integrity re-stream when the retained
revision already validated it. M2's frozen file doubles as the module surface
description this needs.

### M6 — Harness repair

- Re-baseline the warm metric on the product intent (`EditorDiagnostics`), not
  `CompileIntent::Diagnostics`, which solves twice per edit.
- Fix the sidecar budget rejection: `cargo xtask verify-compiler-interactions`
  refuses its 23.5 MB report against a 16 MiB budget
  (`crates/xtask/src/compiler_interactions.rs:23`). Nearly every per-edit counter
  is byte-identical across edits. Emit counters only where they vary, or as
  deltas against the previous revision.
- Drop the 20.4 MB pretty-JSON plan export from the product path. The product
  consumes `Arc<MachinePlan>` in-process
  (`crates/boon_native_playground/src/compile.rs:45`) and never needs JSON; only
  the report's `plan_sha256` does, and it can hash canonical bytes.

### M7 — Developer loop (outside the oracle)

Adopt `cargo-nextest`. Drop the `[profile.test.package.*] opt-level = 0` override
for the three large compiler crates (`Cargo.toml:240-254`) that gives them a
different fingerprint from their dev build. Narrow the `crates/boon_cli/build.rs`
git fan-out (four subprocesses plus a mimalloc relink on every commit). No
`RUSTC_WRAPPER`/sccache is configured. `lto`, `codegen-units` and `target-cpu`
are frozen in both `budgets/compiler.toml` and `build.rs:105-115`; changing them
is an attested lane, not a build tweak.

## Expected Position

Estimates, not measurements. Not additive; most items are enablers.

| | cold verified TodoMVC | warm edit |
| --- | --- | --- |
| starting point | 1905 ms | 883 ms |
| M0 + M1 | ~1400-1600 ms | ~880 ms |
| M2 | included above | ~880 ms |
| M4 | — | **25-45 ms** |
| M5 | — | preview 120-200 ms |
| M3 | if the ratio is large | — |

M4's projection is better supported than the others. M0 measured the per-definition
solved state across revisions and found **100% of definitions unchanged** on both
fixtures for both a type-preserving edit and a real type change. The warm figure
is a cache with a measured near-100% hit rate, which is why 883 ms is not
conservative invalidation.

M3's value is now fixture-dependent rather than general: TodoMVC spends 583 of
1057 ms of typecheck in kernel compile against 10,537 call sites, while NovyWave
spends 136 of 452 ms with 2,699 call sites. Any shape-key result must be reported
per fixture, and a win on TodoMVC is not evidence about NovyWave.

Cold is structurally capped: with caches disabled a fresh process is always a full
solve, and the cold budgets (75 / 250 / 1000 ms) are not reachable for genuinely
cold work. The only structural answer is a content-addressed persistent artifact
cache, which is a separate protocol decision and is not in this plan.

## Measurement Protocol

Unchanged from the standing contract: one Cargo process at a time with
`--jobs 2`; fresh release `boon_cli` and `boon_cli_evidence`; prebuilt binaries
for repeated observations; interleaved A/B/A with 3 setup and 30 scored
observations per fixture, intent and cold mode, in both product and evidence
lanes; store the raw report JSON. Record latency, RSS, allocations and every
kernel work counter including the `KernelRequirementWork` block. Run the warm
collector on each checkpoint. Inherited red limits are reported, never weakened.
At least one independent fresh-context read-only review audits each final
decision before acceptance. Commit coherent local milestones after focused gates
pass; do not push.

## Non-Goals

- Any change to the Boon language surface, `PASSED`/`PASS` semantics, `WHERE`, or
  module visibility. If an item needs one, it stops and escalates.
- Editing `budgets/compiler.toml` to make a candidate pass.
- The residual-module or linked-transfer-code direction (measured 4.3x worse).
- A second solver, a result-type-equality cache, a fixture-name shortcut, or
  batching the requirement refresh (breaks
  `projection_does_not_reimport_a_withdrawn_requirement_as_base`).
- Re-running K1′'s rejected memoization, residual fallback, scaffold caching or
  raw-input dedup.
- Weakening native GPU schemas, reports, budgets or negative checks.

## Stopping Rules

Land each item only with a byte-identical machine plan and diagnostics
fingerprint, except where the item is explicitly a controlled hash migration (M1
items 5 and 6) and records the re-established oracle. If an item cannot meet its
gate, stop, record the measurement that explains why, and hand off. Do not iterate
micro-optimizations against a failing gate.

### M3 gate measurement — invocation-frame fragmentation (2026-09-28)

The M3 gate was open pending the distinct-shape ratio. That ratio cannot be
measured cheaply, because shape-keying needs *resolved* argument types and those
do not exist at compile time — the kernel is compiling call sites into
pre-solve frames. What can be measured is the fragmentation those frames imply.

| counter | TodoMVC | NovyWave |
| --- | --- | --- |
| `compiled_call_sites` | 10,537 | 2,699 |
| `invocation_frames` | 5,789 | 703 |
| `reused_invocation_frames` | 302 | 32 |
| frames that serve exactly one call site | **5,487 (94.8%)** | **671 (95.4%)** |
| call sites that reuse a frame | 2.9% | 1.2% |

`InvocationKey` (`crates/boon_compiler_kernel/src/owner.rs:15364`) keys on the
caller's `TypeVariableId`s, so two call sites that will resolve to the same
argument shapes still get separate frames. About 95% of frames are singletons on
both fixtures.

This is the fragmentation that survived the M2 retirement, and unlike every
retired proposal it is backed by non-zero **production** counters on both
fixtures. It is the largest measured structural inefficiency left in the compile
path, and it sits squarely in the kernel-compile half that M0 identified as
TodoMVC's dominant cost (583 ms of 1057 ms of typecheck).

What it is not: proof that shape-keying will pay. The ceiling is bounded by how
many of those 5,487 singleton frames would collapse to a shared shape, and that
is exactly the ratio that cannot be measured without running a solve. A cheap
next step is to instrument the *solve* side, where resolved types do exist, and
count distinct `(target, resolved formal shape)` tuples against the 5,487
frames they came from. That is a probe, not a redesign, and it is the correct
next measurement rather than an implementation.

### M3 — Shape-keyed specialization: **the answer was already measured, and it is no**

The frame-fragmentation measurement above (5,487 of 5,789 TodoMVC frames are
singletons) looks like a large opportunity, and it is the reason M3 was ranked
above the retired items. It is not. The ceiling was measured during K1′ and
recorded in
[`compiler-k1p-reuse-ceiling-2026-09-11.json`](evidence/compiler-k1p-reuse-ceiling-2026-09-11.json),
on a debug build, and the release producer can now reproduce it because the reuse
probe's `#[cfg(debug_assertions)]` gate has been removed in favour of the
environment variable alone.

| | TodoMVC | NovyWave |
| --- | --- | --- |
| `closed_share` of summary calls | 0.348 | 0.348 |
| `closed_classes` | 168 | 143 |
| `closed_calls_in_shared_classes` | 3,743 of 3,911 (95.7%) | 1,793 of 1,936 |
| `value_mismatches` on repeats | 0 | 0 |
| `requirement_mismatches` on repeats | **84** | **4** |

Read together with the frame count this closes M3:

1. **Only 34.8% of summary calls ever close their inputs.** Shape-keying needs a
   resolved argument shape; two thirds of calls never have one, so they cannot be
   keyed by shape at all regardless of how the key is built.
2. **Where calls do close, they already collapse.** 95.7% of TodoMVC's closed
   calls share a `(definition, input terms)` class, forming only 168 classes
   against 3,911 calls. The reuse the shape key would enable is largely already
   present in the closed path.
3. **The value half is a pure function; the requirement half is not.** Zero value
   mismatches across 3,743 repeats, but 84 TodoMVC and 4 NovyWave repeats produce
   *different* closed requirement terms for identical inputs. So a
   `(definition, input terms)` key is sound for the computed value and unsound
   for the requirement state, and a sound key must additionally cover the
   dependency epochs behind the requirement side. No such key exists.

The 94.8% singleton-frame figure is therefore **not** recoverable headroom. It is
the correct cost of calls whose argument types are genuinely not known until the
solve completes, plus a small unsound-to-key remainder. This is the same wall
K1′ hit, reached from a different direction, and it is why the earlier estimate of
"1.3-2.0x on typecheck" for this item was never defensible.

**Disposition: M3 is closed as measured-unavailable.** The remaining shape-keyed
payoff is bounded above by the 34.8% closed share and further reduced by the
requirement-side instability, which is the same invariance the requirement
aggregation work already failed to key around five times. Do not re-open without
a sound requirement-side key, which is a research problem rather than a
performance task.

### M4 — Retain the solved revision: structural findings (2026-09-28)

The plan described M4 as "retain the packed solved revision, dirty-cone it,
backdate, and publish last-good". Reading the code before writing it changes the
shape of the work in three ways.

**1. The kernel session is not reused at all, so `replace_project` is the wrong
retention point.** `KernelSession::replace_project` looks like the place where
retention should happen — it discards `prepared`, `solved` and `checks` on every
revision. But it has **no production caller**: the only call site is a unit test
at `crates/boon_compiler_kernel/src/session.rs:1793`. Every production path does
`KernelSession::new(input)` at `crates/boon_compiler/src/kernel_oracle.rs:1279`,
`:2443` and `:2877`, constructing a brand-new session per request. Retention
therefore belongs one layer up, in `boon_compiler`'s `ProjectState`
(`crates/boon_compiler/src/session.rs:189`), which already retains six frontend
request tables — including `parse_requests`, which is why a warm edit re-parses
only the changed unit and costs 6.9 ms — but retains no kernel request family at
all. `state.checked` and `state.diagnostics` are cleared on every update at
`:485-486`.

**2. The enabling property holds: the term arena is append-only.**
`TypeTermArena::append_term` (`crates/boon_compiler_kernel/src/term.rs:2485`)
only pushes; no existing row is mutated or reordered, and `ensure_term_slot_capacity`
rebuilds only the open-addressed slot table, not the rows. This matches the
measured type-store growth, which was pure appends with untouched existing rows.
So an arena retained across revisions stays valid: old `TypeTermId`s keep meaning
whatever they meant, and new terms append. This is what makes reuse sound rather
than a use-after-free.

**3. The blocker is that the arena is deliberately one-shot.**
`TypeTermArena` is a field of `KernelProjectConstruction` (`session.rs:186`),
moved out by `take_construction` and consumed by `compile_with_construction` in
`ensure_prepared` (`:927-951`); its doc comment states the design
"deliberately establishes that ownership before persistent red/green reuse is
implemented". So the groundwork was laid and the reuse never built. The
`KernelProjectSolveSession` retains the derived terms, and it is that session's
lifetime, not the arena's, that must be extended to cover a revision boundary.

**Consequence for ordering.** M4 is a two-layer change, not one: a new retained
request family in `ProjectState` for the kernel solve, plus letting the retained
solve session outlive one revision. It cannot be started as a local edit to
`replace_project`, which the earlier draft of this plan implied. The first
landable slice is the cheapest sound one: **retain the previous revision's
`CompiledSealedMachinePlanFromSource` and short-circuit a request whose kernel
input is unchanged**, which needs a kernel-input digest that does not exist yet.

### M4 reuse key — measured (2026-09-28)

[`M4 reuse key`](evidence/compiler-m4-reuse-key-2026-09-28.json). A per-owner
compiled-program fingerprint is now available and release-measurable. It hashes
one owner's node kinds and modes in order, and deliberately does not cover the
separate flat edge column, so it over-reports change rather than under-reporting
it — the safe direction for a reuse hint, and explicitly not a soundness key.

| edit | owners unchanged by index | distinct fingerprints shared |
| --- | --- | --- |
| type-preserving (`Walk the dog` → `Walk the dogs`) | **155/155 (100%)** | identical as a multiset |
| type-changing (`+pinned` field on one row) | 38/155 (24.5%) | 23 of 109 |

The type-changing result is not a dense-renumbering artefact. Its by-index
changed set is a near-contiguous tail from owner 25, which looks like a shift,
but a pure renumbering would leave the 109-element multiset of fingerprints
unchanged, and 86 of 109 differ. The owners' content really changed.

This is consistent with, and sharper than, the solved-state result. Published
interface (what a definition promises callers) stayed identical across the
type-changing edit, while the compiled program was rewritten — because the
shared structural term store renumbers when any term is appended. The same
append-only property that makes cross-revision term reuse sound is what moves
dense positions. **Consequence: a reuse key must be owner content, never the
dense owner id.**

So M4's ceiling is edit-class dependent, and the budgets' own warm benchmark edit
is the type-preserving one, where the entire project is reusable and a retained
revision could skip the whole kernel compile and solve. The remaining work is a
retained request family in `ProjectState` plus letting a solve session outlive one
revision; neither is started.
