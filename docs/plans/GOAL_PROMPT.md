# `/goal` Prompt

```text
/goal Complete the unified Boon compiler, distributed runtime, streaming,
NovyWave, Cells, and FjordPulse objective from the current HEAD.

Read AGENTS.md and these contracts before editing:

- docs/plans/BOON_OUT_PARAMETERS_AND_ORDER_INDEPENDENT_BINDINGS_PLAN.md
- docs/architecture/LANGUAGE_SEMANTICS.md
- docs/architecture/BYTES_SEMANTICS.md
- docs/architecture/NATIVE_GPU_PIPELINE.md
- docs/plans/NOVYWAVE_BOON_REWRITE_PLAN.md
- docs/plans/FJORDPULSE_FULL_STACK_BOON_REWRITE_PLAN.md
- docs/architecture/native_gpu_handoff_manifest.json

Authority and conflict rules:

- The OUT plan is authoritative for function calls, OUT, final-position PASS,
  order-independent lexical bindings, contextual functions, List/find,
  List/chunk, checked/elaborated programs, keyed ownership, migration, and its
  Clear End Condition.
- The Client/Session/Server contract below supersedes conflicting older
  Client/Server pairing, ProgramRole::Document, internal HTTP/JSON,
  SessionInfo, and transport details.
- The NovyWave plan remains authoritative for product behavior and acceptance,
  except its old hardcoded contextual-operator paragraph is replaced by the OUT
  plan.
- The FjordPulse plan remains authoritative for pinned revision
  dd6e750c2ca9dec3041f66ceda31d30379d4027a, 108 stories, 340 scenarios,
  product behavior, budgets, security, persistence, Live mode, deployment, and
  Clear End Condition. Reconcile its package language with
  Client/Session/Server; do not weaken its product requirements.
- NATIVE_GPU_PIPELINE.md and its manifest are authoritative for native input,
  WGPU proof, performance evidence, and final report inventory.

Current checkpoint to preserve and verify rather than redo:

- commit 44c011a added substantial Client/Session/Server compiler/runtime/host
  infrastructure, positional CBOR framing, SessionInfo intrinsics, immutable
  bytes::Bytes values, bounded file/content effects, Wellen integration, real
  VCD/FST/GHW fixtures, and real NovyWave effect/data paths;
- docs/plans/BOON_OUT_PARAMETERS_AND_ORDER_INDEPENDENT_BINDINGS_PLAN.md is an
  implementation-ready design, but its compiler/runtime cutover is not yet
  implemented;
- native reports predating the OUT and source migrations are stale evidence and
  must not be refreshed until the architecture and source stabilize;
- Cells previously met its interaction budgets, but the new generic collection
  APIs and ownership model must preserve and freshly prove that performance;
- FjordPulse parity still classifies 338 scenarios as not implemented and two
  backup/restore automation scenarios as explicitly deferred.

Execution strategy:

- Work in large coherent ownership slices. Temporary compile breakage is
  acceptable inside a slice; do not preserve two execution worlds to keep an
  intermediate tree green.
- Run targeted parser/typecheck/IR/executor/runtime tests at slice boundaries.
  Run broad workspace and native report gates only at major milestones, and run
  final report generation only after the final tracked edit.
- After the same blocker class appears twice, stop tactical patching and change
  the owning parser, compiler, runtime, currentness, document, renderer, host,
  or verifier architecture.
- Use subagents for disjoint compiler, ownership/runtime, distributed/session,
  streaming, NovyWave, Cells/native proof, FjordPulse, security, and final
  adversarial review. Give each one a distinct write or review boundary.
- Delete superseded syntax, aliases, codecs, plans, tests, runtime paths, and
  compatibility fallbacks once replacements compile. Do not rename,
  quarantine, feature-gate, or retain them as a second path.
- Never hide an engine limitation in example Boon source. Reduce it to an
  unrelated fixture, fix the generic owner, then remove the diagnostic
  workaround.
- Keep command/report output bounded. Use focused filters and jq summaries; do
  not dump large JSON reports into the conversation.
- Add no Python source, scripts, invocations, or generated Python artifacts.
- Do not commit or push unless the user explicitly requests it.

Phase 0: reconcile contracts and freeze executable fixtures

- Update LANGUAGE_SEMANTICS, NovyWave, FjordPulse, and related active contracts
  so they reference one OUT/PASS-last/call/list API and one
  Client/Session/Server model. Delete conflicting active guidance rather than
  documenting both forms.
- Add small unrelated contract fixtures for direct, one-wrapper, and
  multi-wrapper contextual functions; final-position PASS; typed List/find;
  canonical List/chunk; nested keyed state; effect cancellation; stale event
  routing; distributed ownership; and visible-window materialization.
- Compare normalized executable sections and bounded work, not only parser AST
  or final output values.

Phase 1: implement the OUT compiler and runtime cutover completely

- Implement every item and every Clear End Condition in
  BOON_OUT_PARAMETERS_AND_ORDER_INDEPENDENT_BINDINGS_PLAN.md.
- Parse structured Value/Out parameters, BareBinding/Named call entries, and a
  separately represented PASS clause. PASS may occur once only as the final
  clause; it is not a parameter, value, pipe receiver, OUT, persisted field, or
  wire value.
- Enforce parentheses, exact named ordinary arguments in declaration order,
  canonical bare fresh OUT bindings, typed named OUT forwarding, and a pipe
  supplying only the first ordinary parameter. Remove positional calls,
  argument renaming, unknown-name recovery, and first-unused binding.
- Predeclare lexical declarations with stable IDs, resolve references
  independently of textual order, infer scope effects, and diagnose labeled
  type/alias/value/temporal/distributed cycles correctly.
- Make boon_typecheck produce the authoritative CheckedProgram and boon_ir
  elaborate/unify OutNet, expand contextual functions, erase transparent
  wrappers/OUT/PASS, and produce the authoritative ErasedProgram before
  executable IDs and hashes.
- Make machine, document, distributed, persistence, native host, and verifier
  backends consume only ErasedProgram. Delete ListMapBinding, parser row-scope
  heuristics, template rediscovery, backend positional binders, contextual
  string matching, AST execution fallbacks, and runtime OUT representations.
- Intern OwnerInstanceId from static ownership plus every ancestor
  (list, hidden key, generation). Use it for state, sources, effects,
  dependencies, currentness, persistence, retained rows, and event routing.
  Reject incomplete/stale routes; delete payload/index/default-generation
  recovery.
- Replace List/find_value and reflective List/find(field:/target:/fallback:)
  with typed `List/find(item, if:) -> Found[value] | NotFound`. Derive field
  index use from typed equality and preserve zero scans for Cells address
  lookup without an example branch.
- Replace List/filter_field_equal and List/filter_field_not_equal with typed
  `List/filter(item, if:)` predicates. Infer any field index from typed IR;
  never expose quoted field metadata as a user-facing query API.
- Replace caller-named List/chunk fields with `List/chunk(size:)` and canonical
  `.items`/`.label`. Make map/filter/chunk/find lazy keyed views with logical
  length, demanded ranges, precise current reads, and incremental dirty work.
- Make document/layout request visible ranges plus bounded overscan before row
  value materialization. Remove full-list snapshots, positional
  list_row_at(index) ownership recovery, full document lower, full host
  reconciliation, and full render-scene rebuild from normal interaction paths.
- Perform one parser-aware Rust codemod across examples, tests, embedded Boon,
  migration fixtures, and docs. Move every PASS clause last. Delete the codemod
  after migration; do not use regex-only rewriting or Python.
- Complete structured diagnostics and semantic editor data for OUT bindings,
  forwarding, references, hover, navigation, PASS, and cycles.
- Run the OUT plan's deletion scans and direct/wrapper executable-equivalence,
  ownership, distribution, persistence, editor, and 2,600-row window tests.

Phase 2: preserve and finish distributed/session architecture

- Audit the existing implementation after ErasedProgram migration. Keep
  correct code; do not rebuild already-proven functionality merely because the
  old prompt described it as scaffolding.
- Implement one browser Client, one resumable Session island per tab, and one
  global Server. Permit only Client <-> Session <-> Server; reject direct
  Client <-> Server references and calls.
- Derive cross-island edges from qualified values and calls such as
  Client/store.submit and Server/search(...). Boon source declares no internal
  routes, subscriptions, RPC, HTTP, JSON, synthetic result SOURCE values, or
  invented effects blocks.
- Keep SOURCE for ordinary host/UI inputs. Compile Session once as an indexed
  template with isolated state slabs, hidden ownership, generations, bounded
  fair scheduling, deterministic row-scoped event merging, scoped replies, and
  shared demand subscriptions for independent Server values.
- Expand calls in their declaring island. Stateful nodes and effects use
  call-site plus complete Session-generation ownership. Reject combinational
  distributed cycles unless SOURCE, HOLD, or an asynchronous effect is a real
  temporal boundary.
- Finish SessionInfo/status() and SessionInfo/principal() role restrictions,
  one canonical standard-root registry, and application-shadowing rejection.
  Never expose session IDs, resume tokens, credentials, correlation IDs,
  hidden keys, generations, or binding identities to Boon, reports, logs, or
  durable state.
- Resume each tab for 60 seconds using a host-managed session-storage token.
  Expiry/disconnect cancels work, rejects stale generations, and never replays
  stale events.
- Retain bounded positional CBOR Client/Session frames with protocol version,
  graph/schema hash, edge ID, revision, sequence, and generation. Pass immutable
  values directly between in-process Session and Server; keep JSON only at
  genuine external boundaries.
- Prove two tabs, cross-user isolation, scoped replies, shared values, fairness,
  reconnect/expiry, schema mismatch, current SessionInfo, stale-route rejection,
  and complete secret absence.

Phase 3: preserve and finish numbers, bytes, content, and streaming

- Keep one observable finite IEEE-754 binary64 NUMBER with exact bounded
  conversions and no semantic integer/f128 promotion.
- Keep immutable shared bytes::Bytes storage and BYTES[1] get/set semantics.
  Ensure final PlanExecutor paths use typed IDs and operations rather than
  parser AST or string paths.
- Run one differential internal numeric-specialization experiment. Keep it only
  for at least 10% gain with exact behavior and no regression above 2%;
  otherwise delete it and record the rejection.
- Finish multishot File/read_stream() with default 64 KiB chunks, four-chunk
  credit, strict sequencing, bounded queues, stale-owner rejection, and RAII
  cleanup. Cancel on owner replacement, inactive WHILE, Session expiry,
  disconnect, timeout, failure, and completion.
- Finish bounded File/read_bytes(), atomic File/write_bytes(), Content/import(),
  and Content/save(); concurrent target use returns Busy. Durable content is an
  ordinary digest/size/media record, never a Boon-visible process handle or new
  nominal capability type.
- Prove bounded memory, credit/backpressure, sequencing, replacement, branch
  removal, disconnect, corrupt input, atomic writes, Busy, and terminal cleanup
  through executor, native, server, and browser/Wasm ownership paths.

Phase 4: finish NovyWave

- Preserve the real Wellen and streaming work already present. Remove any
  remaining bootstrap waveform data from active behavior so committed real VCD,
  FST, and GHW files drive hierarchy, signal rows, traces, cursor values,
  comparison, analog display, and paging.
- Complete navigation, comparison, analog/physical rendering, cancellation,
  bounded materialization, and 60 FPS interaction.
- Add app-owned native scenarios and reports proving real waveform contents,
  bounded backpressure, replacement/cancellation, and terminal resource cleanup
  through exact input and frame-linked WGPU proof.
- Pass every functional, visual, bridge, resource, and performance condition in
  NOVYWAVE_BOON_REWRITE_PLAN.md without fixture substitution or
  example-specific host/runtime/renderer/verifier paths.

Phase 5: preserve and freshly prove Cells

- Migrate Cells naturally to typed List/find and canonical List/chunk. Keep
  sparse demand-current values/errors, indexed lookup, dependency/range fanout,
  cycle safety, retained layout/render state, and generic virtual windows.
- Verify real cell click and formula-bar visibility, text/formula editing,
  dependency/range updates, cycles, repeated selection, vertical/horizontal
  scrolling, hover/focus, and selected-input currentness through real host
  events and frame-linked app-owned WGPU proof.
- Require product input-to-visible and scroll p95 <= 16.7 ms and max <= 33.4 ms,
  with proof/readback latency separately accounted. Require zero normal-path
  full-grid recompute, list scan for indexed address find, relower, layout/host
  rebuild, or scene rebuild.
- If the same blocker recurs twice, change scheduling, ownership, currentness,
  virtualization, or retained-render architecture rather than accumulating
  local timing patches.

Phase 6: finish FjordPulse

- Convert the package and plans to Client/Session/Server islands while
  preserving the pinned source revision and exact parity inventory.
- Complete every non-deferred phase 0 through 13 in the FjordPulse plan:
  generic external HTTP/WS capabilities, bounded external JSON, indexed
  collections, the 60k generic query fixture, 58,500-station queries, retained
  MapViewport, browser WebGPU, accessibility projection, deterministic and Live
  modes, Entur/raster providers, public/Admin workflows, security, redb
  persistence, migrations, restart/redeploy, and Coolify deployment.
- Product logic and Session policy remain Boon. Rust contains only generic
  platform mechanisms. No request-time catalog scan, fixture-response
  substitution, direct browser Entur/redb access, handwritten application
  JS/TS, HTML/CSS product renderer, or FjordPulse-specific generic-code branch
  is allowed.
- Convert all 338 not_implemented parity scenarios to fresh passing evidence.
  Only the two explicitly deferred backup/restore automation scenarios may
  remain deferred.

Final verification and absolute stop condition:

- Run independent adversarial reviews for OUT/compiler genericity, executable
  erasure and wrapper equivalence, nested ownership/event safety,
  Session isolation/security, streaming cleanup, native proof integrity,
  NovyWave real-data ownership, Cells performance, and FjordPulse parity.
- Final scans must find no Python, semantic BYTE, SOURCE { ... }, invented
  effects blocks, internal JSON role transport, ProgramRole::Document or paired
  fallback, positional/renamed calls, non-final PASS, ListMapBinding,
  List/find_value, reflective find, List/filter_field_equal,
  List/filter_field_not_equal, caller-renamed chunk fields, runtime OUT,
  parser/backend contextual rediscovery, exposed hidden session identity,
  positional owner/event fallback, example-specific shortcut, compatibility
  runtime, temporary codemod, or stale report.
- All OUT-plan language, plan-equivalence, keyed ownership, virtualization,
  editor, persistence, distributed, and deletion conditions must pass.
- All distributed/session/security and streaming lifecycle tests must pass.
- Every NovyWave acceptance scenario and all Cells functional/performance
  budgets must pass.
- All 108 FjordPulse stories and 340 classified scenarios must have the exact
  permitted final status; indexed 58,500-station evidence, browser 60 FPS,
  persistence/restart/migration, Live Entur operation, HTTPS/WSS, and production
  deployment at https://fjordpulse-boon.kavik.cz must pass.
- From one final unchanged source revision, run every command in
  docs/architecture/native_gpu_handoff_manifest.json and then:
  cargo xtask verify-all --check-existing --report
  target/reports/report-v2/verify-all.json
- Fresh evidence must bind source, executable graph, schema/index, dataset,
  adapter, binary/artifact, image/deployment, surface, input, presented frame,
  and proof identities as applicable. Proof work remains separate from product
  latency but linked to the exact measured frame.
- Documentation, scaffolding, passing deterministic fixtures, stale reports,
  partial parity, or successful deployment without persistence/restart proof is
  not completion.
- Mark the goal complete only when every condition above and every referenced
  plan's compatible Clear End Condition passes and the independent final review
  finds no unresolved issue. If unavailable credentials, infrastructure, or
  hardware genuinely prevent an external gate after all independent work is
  complete, record the exact evidence and mark the goal blocked, never complete.
```
