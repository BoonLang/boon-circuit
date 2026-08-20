# `/goal` Prompt — Boon Tens-of-Milliseconds Compiler

Paste the complete fenced block as a new goal. Retire the older paused unified
product goal; this is a bounded compiler goal and does not continue into later
repository plans.

```text
/goal Implement
docs/plans/BOON_COMPILER_TENS_OF_MILLISECONDS_ARCHITECTURE_PLAN.md completely
from the actual current HEAD and working tree as the only active implementation
objective.

This is a new bounded compiler goal. It replaces the older paused unified
product/performance goal. Do not resume that goal, follow one of its stale
resumption points, or automatically continue into later steps, native/game,
language-foundation, runtime, console, Wasm, hardware, or example-portfolio
work after this compiler goal finishes.

Before editing:

1. Read AGENTS.md and
   docs/plans/BOON_COMPILER_TENS_OF_MILLISECONDS_ARCHITECTURE_PLAN.md
   completely.
2. Read the authority, non-negotiable outcome, benchmark protocol, current
   budgets, and Clear End Condition in
   docs/plans/BOON_COMPILER_PERFORMANCE_PLAN.md. It remains authoritative for
   correctness, cold/no-cache acceptance, determinism, memory reporting,
   cancellation, and verification meaning.
3. Read the current status/target-shape/deletion sections relevant to the next
   milestone in docs/plans/BOON_COMPILER_ARCHITECTURE_REFACTOR_PLAN.md and
   docs/plans/BOON_COMPILER_MACRO_ARCHITECTURE_RESEARCH.md. The
   tens-of-milliseconds plan refines their implementation priority; it does not
   weaken their semantic gates.
4. Resolve and record the actual HEAD, branch, worktree state, Rust toolchains,
   benchmark producer identity, and current baseline. Treat dated line numbers
   and measurements in the plans as evidence anchors that must be revalidated
   after source changes, not immutable truth.

Reconcile the current HEAD and working tree against every M0--M6 exit, preserve
already-landed and still-passing work, and resume at the first incomplete exit.
Do not redo, revert, or route around completed slices merely to restart at M0.
Within the remaining work, keep the milestone order defined by the plan. M0 is
a bounded measurement/allocator-enabling slice. M1 through M5 are the primary
architectural work and must remain biggest-first. M6 is finishing work after
the representation and ownership cuts; do not substitute compiler flags,
allocator tuning, local maps, small vectors, branch hints, SIMD, caches, or
threads for an unfinished whole-owner deletion.

The required production architecture is:

- one SymbolId/PathId/packed TypeId authority across compiler phases;
- direct packed syntax into the kernel, deleting the production compatibility
  adapter and old SOURCE ABI checker dependency;
- one persistent revision database for cold revision zero and warm edits, with
  exact dirty owner/SCC cones, backdating, cancellation, and diagnostics-to-
  preview promotion;
- normalized definition code and compact parent-linked invocation frames,
  without reconstructing cumulative rich substitution maps;
- one physical tagged edge store with distinct evaluation/currentness and
  proof/link-relocation planes and separate CSR views;
- one plan-code body per compatible definition variant, keyed by definition,
  execution domain, resolved layout, overlay/control shape, and capability
  contract, with occurrences referencing it through frames;
- one consuming SealedExecutableImage builder and runtime handoff without rich
  checked, semantic, IR, legacy MachinePlan, whole-plan clone, retrospective
  compaction, or pretty-JSON reconstruction on the normal preview path.

Preserve these identity and correctness invariants:

- dense IDs and SourceSpanId values are revision-local coordinates;
- stable external definition and structural occurrence identities survive
  unrelated edits and own request/currentness keys;
- TypeRef includes its module/definition scope; raw type-variable ordinals
  never globalize;
- object field order remains part of semantic equality, hashing, and receipts,
  not merely presentation;
- evaluation/currentness edges and proof/link relocations retain distinct
  meanings and cycle rules even when sharing physical storage;
- NoElement is a UI-library convention, never a Boon language/compiler special
  case;
- WHERE, exact proof meaning, diagnostics, source locations, determinism,
  migration/restart behavior, and all existing language semantics remain exact;
- compiler-built packed images receive one independent dense cross-table
  verifier; untrusted/deserialized artifacts retain exhaustive verification.

Linux responsiveness is the first target. Integrate and A/B exact Microsoft
mimalloc 3.5 against a retained System-allocator lane with identical semantic
artifacts. Enable it by default only after an uninstrumented product win plus
correctness, invalid-input, long-session, cancellation, concurrency, and leak/
RSS soak gates. Record upstream tag/commit, library hash, build mode/options,
preload/static identity, environment, and THP policy. The Rust mimalloc wrapper
must not be called 3.5 while it vendors an older C release. Keep an
uninstrumented product producer separate from the allocation/work evidence
producer. Current thread-local Rust global-allocation counters are not
process-wide native-allocation evidence and must aggregate every compiler
worker before parallel compilation.

Do not make nightly the default because it builds the Rust workspace faster.
Measure Boon runtime independently. Test stable ThinLTO, codegen-units=1,
target-cpu=native, instrumentation PGO, and conditional BOLT/sample PGO as
isolated M6 candidates. Retain only holdout-proven wins. A nightly dependency
requires at least the plan's measured improvement and parity gate; allocator_api
is optional ergonomics for a proven lifetime boundary, not the architecture.

Measure and report the four products independently:

- warm complete diagnostics: target 5--15 ms p50 and at most 16.7 ms p95 for
  the reference ordinary edit;
- warm verified in-memory preview: target 15--40 ms p50 and at most 50 ms p95
  for an unchanged-public-interface edit;
- cold in-process complete diagnostics from an empty database and resident
  source bytes: target 20--45 ms p50 and 30--70 ms p95;
- cold in-process verified runnable image: target 40--85 ms p50 and 60--120 ms
  p95, with sub-100 ms p95 as the stretch exit.

Also retain both normative cache-disabled cold modes from the compiler-
performance contract: a fresh invocation of the prebuilt compiler and an in-
process request against a newly created empty database. The fresh-process CLI
result includes startup and required I/O; decompose those spans for diagnosis
without omitting them from its total. Explicit export serialization remains a
separate product only when the request does not consume it.

Use randomized/alternating variants and 20--30 samples for directional
architecture and toolchain tournaments. A numbered phase exit and final
acceptance must use the manifest's exact ladder: three setup observations plus
30 scored observations across every required fixture and both cold modes,
followed by the interaction/scaling gates and manifest-backed
`--check-existing` closure. Record p50/p95 wall and CPU time, peak RSS, Rust
global-allocation calls/bytes, page faults, work counts, dirty-cone size,
allocator/toolchain/profile identity, source/worktree fingerprint, binary hash,
and output digest. Use disjoint PGO/BOLT training and holdout workloads.

Use one Cargo process at a time, normally with --jobs 2. Build a fresh release
producer explicitly and invoke the prebuilt binary for repeated samples. Do not
claim a win with stale binaries/reports, extra compiler concurrency, caches,
relaxed verification, increased timeouts, omitted consumed work, hidden export
work, or NovyWave/example-specific shortcuts. Warm reuse cannot satisfy a cold
gate. Pretty export work that the in-memory preview does not consume is a
separate explicit product, not mandatory preview latency.

Do not enable optional compiler parallelism until every normative single-
threaded cold gate passes. Afterward, disabling every worker pool must keep
those gates green; parallelism may improve only a separately reported product.

Existing warm example-switch and final native-presentation timing gates remain
required end-to-end evidence. Compiler/session/artifact integration changes
needed to pass them are in scope. Unrelated native renderer, input, compositor,
or product implementation is out of scope; if a gate is proven to fail solely
for such an unrelated native defect, report the exact boundary and request
scope rather than silently skipping the gate or expanding this goal.

Preserve differential parity for canonical diagnostics, declaration and
expression flow types, callable substitutions, captures, projections, calls,
effects, states, lists, sources, lexical bindings, currentness, exact dependency
cones, verified artifacts, runtime behavior, and deterministic fresh-process
output. Include late provider epochs, nested/empty/disappearing projections,
capture backflow, generic HOLD, recursive calls, PASSED, long WHEN chains,
heterogeneous collections, ABI calls, cyclic SCCs, invalid sources, Counter,
TodoMVC, and NovyWave.

At each milestone:

1. implement a coherent vertical ownership cut;
2. delete the superseded production owner after differential parity rather than
   leaving a fallback or permanent compatibility path;
3. run focused correctness and architecture-boundary gates;
4. regenerate fresh directional performance/allocation evidence;
5. if a latency hypothesis misses by more than 25 percent or the same blocker
   class repeats, stop micro-optimizing and reprofile/review ownership;
6. obtain a fresh read-only adversarial review before accepting the milestone;
7. commit the coherent local checkpoint with exact staging, then continue to
   the next failing milestone. Do not push.

Use subagents for independent source, measurement-integrity, allocator/
toolchain, and adversarial reviews when they can work in parallel, while the
main agent remains responsible for reading the controlling contracts and
integrating corrections. Do not fabricate human observation or performance
evidence.

Before final completion, obtain three independent fresh-context reviews with
distinct implementation-completeness, measurement-integrity, and semantic/
architectural-soundness charters against the same unchanged revision. Close
every finding, regenerate current evidence and sidecars, and rerun the manifest
closure; milestone reviews do not substitute for this final three-review gate.

The goal is not complete after documentation, mimalloc integration,
instrumentation, a packed type sidecar, a kernel slice, one favorable timing,
or one checkpoint. Complete it only when every applicable M0--M6 implementation,
deletion, correctness, cold/warm latency, allocation, RSS, scaling,
determinism, invalidation, cancellation, verification, and adversarial-review
exit in the tens-of-milliseconds and compiler-performance plans passes with
fresh current-binary evidence.

If a target remains red, continue with the largest measured owning
architectural cut. Mark the goal blocked only under the product's repeated-
impasse rule, after safe in-scope architectural and measurement alternatives
are exhausted and the exact external change required is identified. Do not
mark the goal complete because the remaining work is difficult, expensive, or
would exceed an estimate.
```
