# Boon Circuit Simplification And Native Recovery

Status: active implementation contract for the destructive cleanup.

## Objective

Reduce the current 411,982 tracked Rust lines to at most 240,000 while
restoring a responsive native playground. The final repository has one
execution engine, one typed document/render update path, one native input path,
compact verification tooling, and no executable 3D/manufacturing island.

The numerical caps are forcing targets, not permission to delete unique
product behavior or its only effective tests. Every reduction checkpoint must
name the surviving behavior owner and independent verification path. If an
ownership and representation audit establishes that the unique implementation
floor is above a cap, reconcile the plan and cap explicitly instead of deleting
essential logic or weakening a gate.

The checkpoint at `6935352` is intentionally not a completed native-input fix:
the automated Counter TEST route passed while physical COSMIC dev-window input
remained unresponsive.

## Current Implementation State

Mandatory slices 1 through 5 are implemented. At commit `9b4ed71`, the fresh
architecture report passes every check with 183,763 tracked Rust lines, 31,703
test Rust lines, 31,361 playground production lines, 5,117 xtask production
lines, and 20,544 runtime-plus-executor production lines. The counter now
partitions trailing inline test modules from production instead of hiding them
or double-counting them. Duplicate private playground behavior oracles were
deleted, reducing its focused suite from 118 tests taking roughly 34 seconds to
62 tests taking roughly 1.5 seconds. `app_window` is published at
`BoonLang/app_window`, pinned to immutable revision
`6aec9831f281df355736df28a4c3aacdef7cf8a1`, and measures 1,192 net code lines
over v0.3.3 against the 1,200-line cap.

The recovered execution path now preserves generic scoped row sources from the
typed compiler plan through `Session`, document bindings, retained hit targets,
and scenario dispatch. Source-event transforms evaluate against the event row,
helper parameter names no longer define source ownership, and unscoped visual
target text is not mistaken for a row target. Document evaluation treats equal
text and enum values semantically, preserves hidden/semantic labels, and checks
that patch-applied retained frames equal the authoritative runtime frame after
every test dispatch. NovyWave now uses canonical row-owned scope and signal
events instead of parallel scenario-only controls.

Fresh semantic runs pass through `MachinePlan`, `Session`, and typed document
patches. The current vertical-matrix consolidation keeps one native/Wasm
foundation trace for universal values, typed list views, and scoped reactive
rows while deleting duplicated happy-path assertions from the typechecker,
compiler, and executor suites. Mutually exclusive scoped `SOURCE` occurrences
at one structural row path now remain explicit alternatives through semantic
storage, erased IR, `MachinePlan`, `Session`, and retained document bindings;
ordinary HOLD state storage stays on the existing singular path.

The focused matrix and the typechecker, semantic, IR, compiler, plan, and
executor suites pass under serial execution. A fresh architecture report passes
the verified compiler spine, dependency-classifier schema, and every non-budget
structural check. The first consolidation removed 8,461 tracked Rust lines and
8,800 test Rust lines relative to its preceding measurement.

The next ownership cut deletes 5,613 lines of private `boon_runtime` wrapper and
distributed behavior oracles now owned by public executor, server-runtime,
persistence, web-host, and native integration paths. Genuine transport,
document, persistence, Wasm, and migration algorithms retain focused tests. It
also deletes the stale in-process `resume_persistent` compatibility API: a cold
Server restart restores durable Server authority while creating a fresh
process-local Session, whereas reconnect inside the live resume window remains
the Session registry's one resumability path. The public four-case in-process
Client/Session/Server integration suite passes with that ownership.

A fresh architecture report now measures 456,248 tracked Rust lines, 92,304
test Rust lines, and 64,505 runtime-plus-executor production lines. Those remain
over the respective 240,000, 32,000, and 42,000 limits. The playground and
xtask production caps pass at 31,298 and 17,725 lines. Every non-budget
architecture check passes. Native product reports remain stale until those
budgets close and the compositor restart described below occurs.

The first production ownership cut keeps the narrow `DistributedServerMachine`
authority trait and opaque `SessionOrigin` in `boon_runtime`, but moves the sole
2,287-line Server router into its only production owner,
`boon_server_runtime`. No execution logic, migration runner, migration fixture,
or state-draining path is removed. The runtime-plus-executor count is now
62,224, leaving 20,224 lines above its cap; tracked Rust is 456,266 and test
Rust remains 92,304. The packed-site inventory now explicitly scans Server
runtime instead of letting the ownership move hide its hot container sites; it
passes with 19,892 rows across 85 files. The focused four-case public
Client/Session/Server integration suite passes, as does an all-target compile.
A broader serial library run is not claimed green: 39 tests passed while two
existing scheduling expectations failed. The moved router body is
diff-identical to its prior `boon_runtime` body, so the failures are recorded
rather than hidden or misreported as a passing broad suite.

The next production ownership cut moves the existing target-neutral Client and
Session transport orchestration into `boon_distributed_runtime`. The crate
wraps, but does not duplicate, the one `boon_runtime` machine path; Server and
browser hosts now consume it directly. `boon_runtime` retains the shared
distributed error, narrow Server machine authority trait, opaque Session
origin, and data-boundary helpers. No migration runner, migration fixture,
draining state, persistence path, or transport behavior was removed. The
runtime-plus-executor count is now 58,055, a 4,169-line reduction that leaves
16,055 lines above its cap; tracked Rust is 456,321 and test Rust is 92,306.
The packed-site and container inventories explicitly classify the new crate and
pass with 19,892 packed candidates across 86 source files and 5,037 exact
container occurrences across 154 files. The new crate's nine algorithm tests,
the four-case in-process Client/Session/Server suite, the browser transport's
17 reconnect/journal/backpressure tests, formatting, and an affected all-target
compile pass under serial execution. The broader Server scheduling failures
recorded above have not been reclassified or hidden.

The following ownership cut moves the existing MachinePlan document evaluator
intact into `boon_document`, which is now the sole document-evaluation owner;
`boon_runtime` retains only the transaction facade that invokes it. The
single-executor gate permits exactly the document evaluator and runtime as
direct `boon_plan_executor` consumers and still rejects any additional
dependent or executor definition. No document algorithm, migration path,
draining state, or retained-window behavior was removed. Runtime plus executor
now measures 51,812 lines, a further 6,243-line reduction that leaves 9,812
lines above its cap; tracked Rust is 456,333 and test Rust remains 92,306. The
18 moved document-evaluator tests and an affected all-target compile pass under
serial execution. Packed and container inventories retain the same 19,892 and
5,037 exact occurrences under the new owner, and all non-budget architecture
checks pass.

The next target-ownership cut moves the generic native migration scenario
runner intact into `boon_host_runtime` and the IndexedDB persistence
coordinator intact into `boon_web_host`. The native playground still invokes
the same runner, and its three deterministic Counter, Persons.pro, and TodoMVC
migration scenarios pass, including incremental and skipped version steps and
cross-path authority preservation. The browser coordinator passes a real
`wasm32-unknown-unknown` compile in its new owner. That target check also
exposed and fixed a missing exhaustive queue-weight classification for the
current `CollectionAuthority`, MAP, SET, and BITS runtime values in the generic
web effect host. No migration fixture, draining state, persistence protocol,
or migration demo path was removed. Runtime plus executor now measures 48,870
lines, a further 2,942-line reduction that leaves 6,870 lines above its cap;
tracked Rust is 456,350 and test Rust remains 92,306. Playground and xtask
production remain within their caps at 31,299 and 17,753 lines. Packed and
container inventories retain 19,892 candidate sites across 86 source files and
5,037 exact container occurrences across 154 files under the new owners. All
non-budget architecture checks pass.

The following runtime-ownership cut creates target-neutral
`boon_program_runtime` as the owner of program compilation, `ProgramArtifact`,
`ProgramSession`, and embedded `ProgramDocumentHost` orchestration. Native
effect execution, durable persistence, and `PersistentProgramSession` move to
`boon_host_runtime`; `boon_server_runtime` consumes that host-owned persistent
session instead of forcing native persistence back into core runtime.
`boon_runtime` retains the single `LiveRuntime`/executor facade and the narrow
target-neutral authority needed by those owners. No compiler, document,
persistence, migration, fixture, or demo algorithm was deleted. The three
checked-in Counter, Persons.pro, and TodoMVC migration scenarios pass in their
new owner, as do the wrong-artifact rejection and exact equal-revision durable
binding tests and the four-case in-process Server integration suite. A native
affected all-target compile and a real `wasm32-unknown-unknown` all-target
compile pass under serial execution.

Runtime plus executor now measures 40,520 production lines and passes its
42,000-line cap, an 8,350-line reduction from the preceding ownership cut.
Tracked Rust is 456,444 lines and test Rust is 92,313 lines; those two global
budgets remain open. Playground and xtask production remain within their caps
at 31,305 and 17,773 lines. The packed and container inventories explicitly
follow every moved implementation and pass with 19,896 candidate sites across
88 source files and 5,039 exact container occurrences across 156 files. Phase
0 versions, deletion, fixture, inventory, and stale-report checks pass; the
Cells packed baseline and its dependent current-worktree budget report remain
honestly stale.

The compiler-entrypoint cut removes the public matrix of source-path,
source-text, source-unit, parsed-program, runtime/full, default-identity, and
persistence wrapper variants. One explicit `CompileRequest` now carries source,
target profile, program role, and application identity; its optional
persistence catalog carries schema version and exact predecessors. One
`compile_machine_plan` route owns source compilation and one explicit
`compile_erased_program` route owns verified backend lowering. Runtime
compilation always uses the runtime checker; editor hint production remains a
direct typechecker concern instead of a second executable compiler mode. The
affected all-target check, all 143 compiler unit/integration tests, and the
three generic Counter, Persons.pro, and TodoMVC migration scenarios pass under
serial execution. No migration runner, fixture, persistence path, or draining
state was removed.

The fresh architecture report has only the two global budget failures: 456,396
tracked Rust lines and 92,561 test Rust lines remain above their 240,000 and
32,000 caps. Runtime plus executor, playground, and xtask production remain
within their caps at 40,518, 31,313, and 17,797 lines. The canonical compiler
entrypoint is now part of the verified-spine architecture check. The packed
inventory follows the changed compiler ownership with 19,894 candidate sites
across 88 files, and the exact container inventory retains 5,039 occurrences
across 156 files.

The next test-ownership cut deletes the 7,941-line private compiler behavior
oracle module and its two now-unused dev dependencies. Public compiler
integration tests retain transient collection, nested match, pulse/fusion, and
worklist behavior; the cross-layer foundations vertical retains universal
values, typed views, and scoped rows; the executable host migration runner
retains Counter, Persons.pro, and incremental/skipped TodoMVC state migration.
Six focused private tests remain for genuine document/backend mapping and
resource-reference algorithms. The retained 18 compiler tests, three
foundations verticals, and all three migration scenarios pass under serial
execution.

This cut removes 7,944 tracked Rust lines, including 7,941 test lines. The
current architecture report now measures 448,452 tracked Rust lines and 84,620
test Rust lines; only those two global caps fail. Runtime plus executor,
playground, and xtask production remain within their caps at 40,518, 31,313,
and 17,797 lines. The packed inventory now records 19,667 candidate sites
across 87 files, and the exact container inventory records 5,008 occurrences
across 155 files. Phase 0 deletion, fixture, and inventory checks pass; only
the already-declared stale Cells packed baseline and its dependent budget
report remain open.

The erased-IR test-ownership cut removes 4,055 lines of private source-behavior
verticals plus 50 lines of obsolete test-only runtime/full lowering helpers and
unused producer aliases. The 31 retained private IR tests are genuine
semantic-to-executable identity, allocation, storage-join, typed-list-storage,
and fail-closed mapping algorithms. Public compiler integrations and the
cross-layer foundations vertical remain the source-behavior owners. Those 31
IR tests, all 18 retained compiler tests, all three foundations verticals, and
the three executable migration scenarios pass under serial execution.

The current architecture report now measures 444,347 tracked Rust lines and
80,565 test Rust lines; only those two global caps fail. Runtime plus executor,
playground, and xtask production remain within their caps at 40,518, 31,313,
and 17,797 lines. The packed inventory now records 19,514 candidate sites
across 83 files, and the exact container inventory records 4,968 occurrences
across 151 files. Phase 0 deletion, fixture, and inventory checks pass; only
the already-declared stale Cells packed baseline and its dependent budget
report remain open.

The typechecker test-ownership cut removes 7,139 lines of private call,
distributed/session, and host-port source-behavior verticals plus 50 lines of
obsolete test-only checked-flow instrumentation. A small shared payload helper
remains with the compact 36-test typechecker suite, which owns exact number and
BITS rules, map/set authority, FLUSH, pulse, reactive-collection, structural
assignability, and fail-closed negative algorithms. Public compiler and
cross-layer integrations remain the success-behavior owners. Those 36 retained
typechecker tests, all 18 retained compiler tests, all three foundations
verticals, and the three executable migration scenarios pass under serial
execution.

This cut removes a net 7,182 tracked Rust lines and 7,132 test Rust lines. The
current architecture report measures 437,165 tracked Rust lines and 73,433 test
Rust lines; only those two global caps fail. Runtime plus executor, playground,
and xtask production remain within their caps at 40,518, 31,313, and 17,797
lines. The packed inventory now records 19,317 candidate sites across 80 files,
and the exact container inventory records 4,963 occurrences across 148 files.
Phase 0 deletion and both regenerated inventories pass; only the
already-declared stale Cells packed baseline and its dependent budget report
remain open.

The executor test-ownership cut removes a net 5,062 lines of private
source-compiled success verticals for typed-list paging/index construction,
FjordPulse and scalar conversions, public FLUSH behavior, source-level effect
guards, stateful calls, BITS, and pulse fusion. Their public compiler,
foundations, typed-list, map/set, FjordPulse, host/runtime, and pulse integration
owners remain. The compact private executor suite retains the hand-built
`MachinePlan` algorithms for rollback, currentness, dependency cycles, indexes,
bounded work, transient and durable effects, distributed leases, and detached
captures. Ten lines of test-only ordered-index access that became unused were
also deleted. The 105 retained executor unit tests, 18 public executor
integration tests, eight public pulse compiler tests, and all three executable
migration scenarios pass under serial execution.

This cut removes a net 5,010 tracked Rust lines and 5,000 test Rust lines. The
current architecture report measures 432,155 tracked Rust lines and 68,433 test
Rust lines; only those two global caps fail. Runtime plus executor, playground,
and xtask production remain within their caps at 40,508, 31,313, and 17,797
lines. The packed inventory now records 18,899 candidate sites across 80 files,
and the exact container inventory records 4,922 occurrences across 148 files.
Phase 0 deletion and both regenerated inventories pass; only the
already-declared stale Cells packed baseline and its dependent budget report
remain open.

The host/session test-ownership cut removes the private
`DistributedSessionRegistry`, Server transaction/transient-host, and host
persistent-runtime behavior modules. Public server auth, hostile/resumable
transport, HTTP contract, in-process restart/effect, and loopback integrations
own Server behavior; public host content, file effect, stream, service, and
migration integrations own host persistence and effect behavior. The cut also
deletes the queue-pressure injection, mutable queue-limit hook, settled-slab
inspection, direct-delivery helper, and transient-effect count accessor that
existed only for those private modules. It removes no production migration
runner, persistence backend, session registry path, state-draining path, or
checked-in migration fixture.

All 33 host-runtime tests pass, including the three executable migration
scenarios. The affected Server all-target check passes without warnings. Its
public test run passes 15 tests and keeps one explicit fixture refresh ignored;
one websocket loopback remains red because `server_websocket_echo.bn` is
rejected during compiler expansion with a checked-value cycle before the
removed host/session test code can run. That broader failure is recorded rather
than hidden or reported as green.

This cut removes 5,743 tracked Rust lines, including 5,455 test Rust lines. The
current architecture report measures 426,412 tracked Rust lines and 62,978 test
Rust lines; only those two global caps fail. Runtime plus executor, playground,
and xtask production remain within their caps at 40,508, 31,313, and 17,797
lines. The packed inventory now records 18,642 candidate sites across 80 files,
and the exact container inventory records 4,909 occurrences across 148 files.
Phase 0 deletion and both regenerated inventories pass; only the
already-declared stale Cells packed baseline and its dependent budget report
remain open.

The second executor test-ownership cut removes the remaining private
compiler-fixture behavior block for host-bound values, transient and durable
effects, SessionInfo, distributed context replacement, remote function leases,
and row-owned distributed calls. Public host/server integrations own those
behaviors. The retained private executor module now contains only direct
`MachinePlan` algorithms for rollback, currentness, dependency cycles, indexes,
bounded work, FLUSH, and detached captures; cursor, effect-stream, and ownership
algorithm modules remain separate. The deleted block's compiler helpers and
template-metadata inspection are also gone, while the Phase 0-only delta helper
is compiled only when its instrumentation feature is active.

All 66 retained executor unit tests, all 18 public executor integrations, and
the three executable migration scenarios pass under serial execution. This cut
removes 4,119 tracked Rust lines, including 4,114 test Rust lines. The current
architecture report measures 422,293 tracked Rust lines and 58,864 test Rust
lines; only those two global caps fail. Runtime plus executor, playground, and
xtask production remain within their caps at 40,503, 31,313, and 17,797 lines.
The packed inventory now records 18,193 candidate sites across 80 files, and
the exact container inventory records 4,849 occurrences across 148 files.
Phase 0 deletion and both regenerated inventories pass; only the
already-declared stale Cells packed baseline and its dependent budget report
remain open.

The product-host test-ownership cut removes private browser persistence and
effect-host behavior modules, the app-server wrapper/transient-host behavior
modules, and native runtime-view product scenarios. Public web-host
integrations remain the browser document, storage, transport, capability, map,
and startup behavior owners; public host/server integrations and manifest
native gates remain the product-host owners. Test-only routing, queue-count,
content-import, and native inspection hooks that had no remaining owner are
also deleted. Production persistence coordinators, content/file effect
algorithms, app-server configuration and routing, native document runtime,
migration runners, fixtures, and state-draining paths remain intact.

The affected web-host, web-effect-host, app-server, and native-playground
all-target compile passes without warnings. All 47 retained native web-host
unit/integration tests pass, as do the three executable Counter, Persons.pro,
and TodoMVC migration scenarios. This cut removes 4,161 tracked Rust lines,
including 3,751 test Rust lines. The current architecture report measures
418,132 tracked Rust lines and 55,113 test Rust lines; only those two global
caps fail. Runtime plus executor, playground, and xtask production remain
within their caps at 40,503, 31,125, and 17,797 lines. The packed inventory now
records 18,150 candidate sites across 80 files, and the exact container
inventory records 4,814 occurrences across 147 files. Phase 0 deletion and
both regenerated inventories pass; only the already-declared stale Cells
packed baseline and its dependent budget report remain open.

The executable migration demos currently prove the catalog/`MachinePlan`
version-migration path, including incremental, skipped-version, restart,
activation, namespace, and cross-path authority behavior. They do not claim
that the planned Boon `DRAIN`/`DRAINING` source surface is already implemented.
Its parser, typechecker, lowering, one-owner transfer, interruption/retry, and
release-finalization tests remain explicit future work in
`BOON_PERSISTENCE_ARCHITECTURE_PLAN.md`; this ownership cut removes none of
those planned paths or their current migration fixtures.

The verified-erasure ownership cut removes post-verification mirrors from the
opaque `ErasedProgram`: the full semantic index is reduced to the source-unit
and field records consumed by compiler debug maps, row-scope validity is
derived from canonical list ownership, and duplicated expression-coverage,
possible-cause, and verification-flag records are gone. The authoritative
semantic possible-cause closure and its validation remain in `boon_semantic`;
executable state updates, list mutations, semantic memory, migration edges,
persistence owners, and migration runners are unchanged. All 31 IR tests, 6
compiler unit tests, 18 dependency-classifier tests, and the three executable
Counter, Persons.pro, and TodoMVC migration scenarios pass under serial
execution. The affected all-target compile passes without warnings.

This cut removes 764 tracked Rust lines, including 26 test Rust lines. The
current architecture report measures 417,368 tracked Rust lines and 55,087
test Rust lines; only those two global caps fail. Runtime plus executor,
playground, and xtask production remain within their caps at 40,503, 31,125,
and 17,797 lines. The packed inventory now records 18,103 candidate sites
across 80 files, while the exact container inventory remains at 4,814
occurrences across 147 files. Phase 0 deletion and both regenerated inventories
pass; only the already-declared stale Cells packed baseline and its dependent
budget report remain open.

The semantic identity-map ownership cut removes allocated `0..N` vector
mirrors for expressions, values, statements, lexical scopes, sources, states,
callables, materializations, lists, row scopes, and value-list authorities.
Canonical dense semantic validation plus bounded typed-ID conversion now owns
those identity relationships; the genuinely non-identity maps remain explicit
and retain their allocation-bijection validation. All 31 IR tests, the focused
architecture-contract test, all 18 dependency-classifier tests, and the three
executable Counter, Persons.pro, and TodoMVC migration scenarios pass under
serial execution. The affected all-target compile passes without warnings.

This cut removes 87 tracked Rust lines, including 4 test Rust lines. The
current architecture report measures 417,281 tracked Rust lines and 55,083
test Rust lines; only those two global caps fail. Runtime plus executor,
playground, and xtask production remain within their caps at 40,503, 31,125,
and 17,830 lines. The packed inventory now records 18,082 candidate sites
across 80 files, and the exact container inventory records 4,812 occurrences
across 147 files. Phase 0 deletion and both regenerated inventories pass; only
the already-declared stale Cells packed baseline and its dependent budget
report remain open.

The verified-erasure proof-shadow cut removes private mapped named-value,
projection, and storage-representation copies after the semantic storage
contract has already checked them. Final named-value type metadata continues
to come from the verified lowering contract, while the reactive-to-mapped and
storage-to-erased ID joins now retain dense-domain counts instead of allocated
identity vectors. The one genuine non-identity reactive-field-to-storage-field
map remains explicit and bijection-checked. An architecture contract fixes the
exact two stage-map schemas and rejects restoration of the deleted proof-shadow
record family. Executable state updates, list mutations, semantic memory,
migration edges, persistence owners, draining behavior, and migration runners
are unchanged.

All 28 remaining IR tests, 6 compiler unit tests, 18 dependency-classifier
tests, the focused architecture-contract test, and the three executable
Counter, Persons.pro, and TodoMVC migration scenarios pass under serial
execution. The affected all-target compile passes without warnings. This cut
removes 1,377 tracked Rust lines, including 369 test Rust lines. The current
architecture report measures 415,904 tracked Rust lines and 54,714 test Rust
lines; only those two global caps fail. Runtime plus executor, playground, and
xtask production remain within their caps at 40,503, 31,125, and 17,959 lines.
The packed inventory now records 17,960 candidate sites across 80 files, and
the exact container inventory records 4,808 occurrences across 147 files.
Phase 0 deletion and both regenerated inventories pass; only the
already-declared stale Cells packed baseline and its dependent budget report
remain open.

The canonical-core ownership prerequisite removes the erasure allocation map
from bundle crossing and moves the complete target-neutral executable schema,
including semantic-memory and migration records, from `boon_ir` into
`boon_semantic::program_core::CanonicalProgramCoreV1`. `ErasedProgram` remains
opaque and privately wraps that core plus the source, semantic, and
verification digests; compiler backends still accept only `ErasedProgram`.
Compiler consumers now import the semantic-owned schema directly, with no
`boon_ir` compatibility aliases or duplicate DTO copy. The temporary
`semantic_mapping` stage still constructs the core and is the next explicit
deletion boundary. Migration recipes, predecessor handling, persistence,
semantic memory, migration edges, state/list schedules, and draining paths are
unchanged.

All 28 IR tests, 18 dependency-classifier tests, 8 architecture-contract
tests, the affected compiler all-target compile, and the three executable
Counter, Persons.pro, and TodoMVC migration scenarios pass under serial
execution. The architecture report fails only the two global line caps, at
415,948 tracked Rust lines and 54,730 test Rust lines. Runtime plus executor,
playground, and xtask production remain within their caps at 40,503, 31,125,
and 17,986 lines. The packed inventory records 17,961 candidate sites across
81 files, and the exact container inventory records 4,808 occurrences across
147 files. Phase 0 again fails only the pre-existing stale Cells packed
baseline and its dependent budget report. This preparatory ownership move adds
a net 44 tracked Rust lines, including 16 test lines, to establish the final
owner before deleting the roughly 10,400-line mapping island.

The semantic-core construction checkpoint deletes the old
`boon_ir::semantic_mapping` owner and makes semantic elaboration construct and
privately retain `CanonicalProgramCoreV1` after all semantic graphs and
manifests join. `boon_ir` now consumes that retained core, binds only the
post-verification pulse-fusion decisions, and wraps the result in opaque
`ErasedProgram`; both IR schedule validation and the `MachinePlan` backend fail
closed if pending fusion reaches them. The canonical semantic digest includes
the retained core. Restoring that digest coverage exposed an impossible Serde
shape in six internally tagged executable literal variants; those variants now
use named payload fields, so the documented canonical core is actually
serializable instead of relying on a digest omission.

This checkpoint removes the old mapping file and its 18 private proof-shadow
tests, but it does not pretend that the mapping implementation has disappeared:
the private semantic-owned `core_lowering.rs` is still 10,357 lines. Collapsing
that builder into the semantic producers, then deleting the resulting
superseded joins, is the next production-reduction target. No migration recipe,
predecessor handling, persistence owner, semantic memory, state/list schedule,
or draining behavior is removed, and this checkpoint does not claim the future
Boon `DRAIN`/`DRAINING` source surface is complete.

The affected compiler all-target check, all 10 retained IR tests, the IR
opacity compile-fail doctest, all 8 public pulse/fusion tests, all 18 active
dependency-classifier tests, all 4 architecture contract tests, and the three
executable Counter, Persons.pro, and TodoMVC migration scenarios pass under
serial execution. The current architecture report fails only the two global
line caps, at 413,120 tracked Rust lines and 53,065 test Rust lines. This is a
net reduction of 2,828 tracked lines and 1,665 test lines from the preceding
canonical-core prerequisite. Runtime plus executor, playground, and xtask
production remain within their caps at 40,503, 31,125, and 16,724 lines. The
packed inventory records 17,921 exact candidate sites across 81 occurrence
files, and the container inventory records 4,770 exact occurrences across 147
Rust files. Phase 0 deletion and both regenerated inventories pass; only the
already-declared stale Cells packed baseline and its dependent budget report
remain open.

The semantic-core self-audit checkpoint deletes 1,174 lines from the private
`core_lowering.rs`, reducing it from 10,357 to 9,183 lines. The deleted code
consists of five post-construction totality or closure rescans, one producer-ID
sequence retained only for those rescans, and two `#[cfg(test)]` helpers with no
remaining callers; it removes no test case or assertion. A 36-line net addition
to the independent canonical-core handoff validator fixes the distributed
event-source inventory bug exposed by the retained imported-event/HOLD test,
for a net reduction of 1,138 tracked production lines. No constructor, mapped
payload, migration recipe, predecessor rule, persistence owner,
semantic-memory record, state/list schedule, draining path, runtime behavior,
fixture, or test is removed. Allocation bijections remain checked while IDs
are assigned, `SemanticReactiveGraphV1::validate` deterministically re-derives
the reactive graph, semantic shape validation checks dense domains and
references, the semantic-program handoff validates the canonical core and its
exact external-event paths, and IR plus compiler backends still reject an
invalid or pending schedule. The architecture contract now rejects restoration
of the private `validate_totality` proof-shadow family.

The affected compiler all-target check, all 10 retained IR tests, all 8 public
pulse/fusion tests, all 18 active dependency-classifier tests, all 4
architecture contract tests, and the three executable Counter, Persons.pro,
and TodoMVC migration scenarios pass under serial execution. The architecture
report again fails only the global caps, now at 411,982 tracked Rust lines and
53,065 test Rust lines. Runtime plus executor, playground, and xtask production
remain within their caps at 40,503, 31,125, and 16,724 lines. The packed
inventory records 17,815 exact candidate sites across 81 occurrence files, and
the container inventory records 4,761 exact occurrences across 147 Rust files.
Phase 0 deletion and both regenerated inventories pass; only the declared
stale Cells packed baseline and its dependent budget report remain open.

The full semantic library run passed 99 tests and exposed one imported-event
inventory failure; after the engine validator fix, that exact retained test
passes on its focused rerun. This leaves 171,982 tracked lines and 21,065 test
lines above the global caps.

The historical `9b4ed71` checkpoint proves that an earlier, smaller feature set
fit below both caps, but it does not prove that the current feature set can do
so without loss. Subsequent cuts therefore require an explicit duplicate-owner
or duplicate-representation proof; line count alone is not a deletion reason.

The nested-compositor diagnostic path has been deleted. The replacement uses
ordinary COSMIC preview/dev windows, kernel uinput mouse and keyboard devices,
the normal app_window callback route, and app-owned exact-frame WGPU readback.
The compositor fork now has a generic launch-scoped reconciliation operation so
all descendant windows of one background launch are gathered and tiled without
fixture, role, title, app-ID, or geometry matching. The matching compositor and
launcher release binaries are installed, but the running compositor predates
that operation. No refreshed native report is accepted until the COSMIC session
loads the installed binary and the real Counter, Cells, wheel, keyboard, TEST,
proof, and aggregate gates pass.

The remaining completion work is explicit and ordered:

1. remove duplicated and superseded implementation/test ownership until the
   remaining tracked-Rust and test-Rust line budgets pass, without deleting
   current generic compiler, runtime, persistence, or product behavior; the
   runtime-plus-executor budget now passes;
2. restart the COSMIC session so the installed compositor exposes launch-scoped
   window reconciliation, then refresh all seven manifest reports and the
   aggregate from one unchanged revision;
3. launch the release playground with demand pacing and obtain the required
   physical human confirmation.

## Non-Negotiable Rules

- Delete obsolete code. Do not rename, quarantine, alias, or preserve it behind
  compatibility switches.
- Make changes in large ownership slices. A slice may be temporarily broken in
  the worktree, but every slice commit must compile.
- Run targeted checks only at slice boundaries. Regenerate expensive native
  reports only after the architecture has stabilized.
- Keep Cells and all runtime/compiler/renderer behavior generic.
- Keep readback out of normal frames. Explicit proof requests use asynchronous
  app-owned WGPU readback tied to exact frame identity.
- Add no Python and no Boon-specific behavior to windowing, runtime, compiler,
  document, renderer, or verifier infrastructure.

## Required Architecture

### Execution And Documents

- `MachinePlan` is the only executable artifact. Its format may break; no old
  decoder remains.
- `boon_plan_executor::Session` exclusively owns values, lists, indexes, source
  routing, currentness, formula dependencies, cycles, dirty keys, and deltas.
- `boon_runtime` is a thin compile/cache/scenario facade returning typed
  `RuntimeTurn` values.
- `boon_document` alone turns typed `DocumentPatch` values into retained layout
  and render changes. The playground does not interpret parser AST or rebuild
  bindings from JSON.
- Product crates do not depend on `serde_json`. JSON is limited to final CLI and
  verifier report serialization.

### Native Windowing And Frames

- Generic window-event improvements live in `BoonLang/app_window`, not in a
  copied workspace dependency. Boon Circuit pins an immutable fork revision.
- `Surface::take_events()` returns one ordered asynchronous receiver covering
  pointer, button, wheel, physical/logical key, text/IME, focus, resize, scale,
  close, and accessibility actions.
- The event queue uses one `AtomicWaker`, coalesces only adjacent motion/wheel
  events, preserves discrete order, and reports overflow as fatal. It contains
  no Boon names, event histories, public test injection, polling timer, or
  second platform dispatcher.
- Desktop only supervises preview and dev. Preview and dev use the same native
  role runner and the same typed event-to-frame transaction.
- Every transaction drains input, applies runtime changes, patches retained
  document/render state, submits/presents if dirty, then schedules optional
  proof work. Proof never blocks product presentation.
- Hidden COSMIC workspaces use explicit demand pacing: requested callbacks are
  coalesced to output refresh cadence and stop when clients stop requesting
  frames. Standard inactive-workspace throttling is not valid performance
  evidence.
- Source replacement uses one typed depth-one latest-wins mailbox. Product IPC
  is binary and contains no JSON.

### Verification

- Delete `boon_report_schema`; report-v2 types and validation are tooling-only.
- Reduce xtask to at most nine public commands and a seven-gate native manifest:
  architecture, Counter/dev, TodoMVC/physical, Cells, NovyWave, Persons.pro,
  and negative.
- Every proof names its frame, input, content, layout, render, surface epoch, and
  present revisions. PNG proof is an asynchronous sidecar.
- A private Wayland server drives the actual app-window callback path. TEST is
  clicked through that path; its scenario input enters at the public HostEvent
  boundary and displays a retained cursor overlay.
- Public behavior tests are integration tests. Private unit tests remain only
  for genuine algorithms; wrapper-parity, report-field, duplicate-oracle, and
  private implementation tests are deleted.

## Mandatory Slices

1. Delete the four 3D/manufacturing crates, examples, fixtures, runtime outputs,
   native branches, commands, schemas, tests, and plans.
2. Break the plan format and migrate all execution to PlanExecutor; delete the
   duplicate runtime representation and execution oracles.
3. Move document bindings into typed plan/runtime output and delete playground
   AST/JSON lowering and state-summary synchronization.
4. Create the external app_window fork event API, delete `vendor/app_window`,
   and rewrite native host/playground roles around it.
5. Replace report/verifier v1 and duplicated tests with compact report v2 and
   the manifest-owned gate inventory.
6. Run final structural, semantic, native, visual, performance, idle, and manual
   verification once.

## Completion Gates

- Tracked Rust lines: at most 240,000; test Rust: at most 32,000.
- Playground: at most 32,000; xtask: at most 25,000; runtime plus executor: at
  most 42,000; app_window fork additions: at most 1,200 net lines.
- No vendored app_window, report-schema crate, executable 3D/manufacturing code,
  duplicate executable artifact, product JSON path, input resampling/history,
  verifier test injection, or compatibility fallback remains.
- Counter, TodoMVC, physical TodoMVC, Cells, and NovyWave scenarios pass through
  the same runtime and document APIs.
- Window callback to HostEvent p99 is at most 1 ms. Warm visible interaction and
  scroll p95 are at most 16.7 ms and max at most 33.4 ms. Warm example-switch
  acknowledgement p95 is at most 16.7 ms; final preview p95 is at most 250 ms
  and max at most 500 ms.
- Settled release preview plus dev CPU is below 1% of one core with zero
  unsolicited frames.
- Formatting, workspace check/test, scenarios, all fresh manifest gates, report
  validation, and the aggregate pass.
- The release playground is launched in the COSMIC background workspace. The
  goal is complete only after the automated gates pass and the user confirms
  physical dev hover/click/wheel/keyboard, TEST, Counter, and Cells behavior.

Rust/Zig code generation is deliberately after this recovery. It must not grow
the repository before these gates pass.
