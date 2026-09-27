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

### M1 — Delete duplicated passes

Each is a local change with no semantic content. All are verified in the tree.

1. `row_expressions.validate()` runs at least twice per compile —
   `crates/boon_compiler/src/machine_plan_backend.rs:7545` and
   `crates/boon_plan/src/lib.rs:10978`.
2. Per-root reachability walk; the multi-root variant already exists at
   `crates/boon_plan/src/lib.rs:9025` — `machine_plan_backend.rs:7546`.
3. `plan.clone()` of the whole plan in `refresh_typed_list_view_fingerprints` —
   `machine_plan_backend.rs:7341`.
4. Arena intern hashes (CBOR + SHA-256) before comparing nodes —
   `crates/boon_plan/src/lib.rs:8969-8987`. Compare first, hash on miss.
5. Every semantic execution row is CBOR+SHA'd twice —
   `crates/boon_semantic/src/semantic_image.rs:2643` (payload) and `:2658` (row
   fingerprint). One preimage, one hash.
6. `verify_plan` recomputed on an unchanged plan — `crates/boon_plan/src/lib.rs:10977`.
   It is a pure function of the sealed plan; memoize on the plan digest, which is
   already computed. Its 41 checks are independent and may run in parallel.
7. `structural_widen_cache` is unsymmetric and uses std SipHash although
   `structural_widen_uncached` is commutative — `crates/boon_compiler_kernel/src/term.rs:593`.
   Order the pair and use the arena's own open-addressed table.
8. `intern_object` re-derives lexical rank inside a binary search per field —
   `term.rs:2314`. One sort of `(rank, index)` pairs.

Items 5 and 6 change hash *computation*, so they require the controlled oracle
migration the performance plan already contemplates: re-establish the affected
digests from a fresh producer and record the migration in evidence. They do not
weaken a budget or a gate.

Expected: 300-450 ms off the verified path, low risk, independently gated.

### M2 — Freeze static facts (no source change, no semantic change)

**Problem.** For every request the compiler re-derives facts that are static
properties of source text: which names a body reads through `PASSED`
(`signature.context_formal`, `crates/boon_typecheck/src/lib.rs:5344`), and which
callables are direct wrappers (`owner.result_expression == Some(call.expression)`,
`crates/boon_compiler_kernel/src/owner.rs:4687`). Because a callee's needs can be
discovered only after its callers are examined, the derivation is a global fixed
point seeded with every owner (`:4597`, `:5294-5312`). It is correct, and it is
recomputed from nothing on every edit.

**Key observation.** Which *names* a function reads is static. What *value* flows
into them is not. `lights()` reads the name `mode`; the value is `store.mode`,
threaded from a record field three modules away. The dependency is a permanent
property of the text; the binding is a per-revision fact. Today the first is
re-derived by search every time.

**Design.** A generated, per-module, authoritative file:

```boon
owner material:
    reads_passed: [ mode ]
    wraps:        [ get ]
    basis:        <stable_fingerprint>

owner lights:
    reads_passed: [ mode ]
    basis:        <stable_fingerprint>
```

- The invalidation key is the **existing** term-id-free basis fingerprint
  (`crates/boon_compiler/src/receipt.rs:255-272`, over `KernelOwnerProgramInput` /
  `KernelDefinitionFactsInput`). No new epoch scheme is invented.
- On each request: demand-load records, recompute each basis fingerprint. Match
  admits the facts with no search. Mismatch drops the record, re-discovers it,
  rewrites it, and re-discovers everything upstream through the frozen wrapper
  edges.
- First build after a change costs what it costs today. Every later build skips
  the fixed point.

**Hard requirement — fail closed.** If the basis fingerprint does not cover
everything the frozen facts depend on, a stale record is admitted and this trades
a performance problem for a correctness one. A cache that can be silently wrong is
worse than no cache. Therefore: the fingerprint must be over the complete
structural input for that owner; no record is written when a complete key cannot
be computed; the file is build output and is never hand-edited. A corruption test
is part of the gate — mutate one record, assert the next build reproduces the
identical diagnostics fingerprint and plan hash.

**Attestation.** The producer's build-input scope is
`workspace-cargo-toolchain-and-crates-excluding-xtask-plus-mimalloc-v1`;
`examples/` is not covered. Before this lands, the attested scope must cover the
freeze inputs, or a stale freeze could be scored without appearing in the
producer identity. This is owner question 1 and it is not optional.

**Explicitly not in scope.** `PASSED` keeps its meaning and its spelling. No
declaration is added to any Boon program. The hand-written `mode:` argument in
`examples/novywave/View/NovyView.bn:139` is redundant plumbing and should be
removed as a separate cleanup, not preserved as a convention. Nothing about this
item changes how a user interacts with Boon.

**Expected.** 1.10-1.35x cold typecheck. ~1.0x warm on its own, because the
solve still runs. Its real payoff is that one dimension of the dirty cone becomes
a static fact instead of a search result.

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
