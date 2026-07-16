# Persons.pro Local-First Implementation Plan

Date: 2026-07-15

Status: local-first release verifier passed at the implementation checkpoint;
current-HEAD native handoff awaits the installed compositor restart

Persons.pro is a Boon-hosted personal publishing workspace. A visitor receives
an immediately usable anonymous workspace, edits a small Boon program, sees the
result through the normal Boon document and WGPU pipeline, and can later protect
and publish the workspace with passkeys.

The first production-shaped implementation runs locally as a versioned
`boon-circuit` playground example. The browser and hosted service are later host
adapters around the same Boon application, not rewrites.

## Product Decision

The initial product has one authentication mechanism: passkeys.

- There are no passwords, email login links, SSH login, OAuth providers, or
  recovery codes.
- An account may own multiple passkeys. Multiple credentials are still one
  authentication mechanism and are required for practical account durability.
- Losing every registered passkey means losing the account unless the product
  deliberately adds another recovery mechanism in a later design revision.
- SSH may eventually be a developer publishing client, but it is not an account
  authentication path.
- Persons.pro does not require a specific password manager. It uses WebAuthn and
  lets the browser, operating system, password manager, or hardware security key
  provide the credential.

Supported browser policy is capability-based rather than user-agent based:

- Chrome or Chromium on Linux is the recommended low-friction path.
- Firefox on Linux is supported with a working passkey provider such as the
  1Password extension or a FIDO2 security key.
- Safari is supported on Apple platforms.
- An unsupported WebAuthn capability is blocked with concrete setup guidance;
  Firefox itself is not blocked merely because its default Linux credential
  experience differs from Chrome or Safari.

The account-protection flow performs a real registration attempt while the
anonymous workspace grant remains usable. It does not destroy the anonymous
grant until registration and a subsequent authentication are proven. After the
first registration, the UI strongly asks the owner to add a second passkey.

## First-Visit Contract

1. A visitor opens `persons.pro`.
2. The service creates a cryptographically random workspace identifier and
   redirects to `persons.pro/p/<workspace-id>`.
3. A separate random workspace grant is stored in browser-local durable storage.
   It is never embedded in the public URL.
4. The workspace opens immediately with short starter Boon source and a live
   preview. No registration wall appears first.
5. The visitor can edit, compile, preview, and publish an unlisted revision on
   the current device.
6. `Protect workspace` creates a passkey account and binds the workspace to it.
7. A later product phase can associate one or more human-readable handles with
   the immutable workspace identity.

The anonymous grant is a capability for one workspace, not a second account
authentication mechanism. Passkey authentication creates ordinary bounded
sessions after the credential ceremony succeeds.

## Boon Owns The Product

The implementation boundary is:

> Rust supplies generic mechanisms. Boon defines product policy and presentation.

Persons.pro Boon code owns:

- the complete application UI and theme;
- editor, preview, workspace, route, and dialog state;
- the anonymous-to-protected account workflow;
- passkey registration and authentication state machines;
- publish, revision, and restore workflows;
- diagnostics presentation;
- responsive layout decisions;
- the starter profile template and public profile presentation;
- server-side application policy where the server Boon runtime is suitable.

Generic Rust, native, browser, or server hosts own:

- parser, compiler, typechecker, plan executor, and runtime;
- WGPU or WebGPU rendering and platform input translation;
- HTTP, WebSocket, TLS, storage, clocks, and random-number generation;
- WebAuthn browser calls and server-side credential verification;
- redb on native/server and IndexedDB in the browser;
- process isolation, execution budgets, quotas, and resource handles;
- loading the trusted precompiled Persons.pro root artifact.

Private passkey material never becomes a Boon value. Boon receives typed
credential metadata and success, cancellation, or failure results.

No parser, compiler, runtime, document, renderer, playground, or verifier code
may branch on the Persons.pro example name, source paths, labels, geometry, or
fixture text.

## Trusted Parent And Restricted Child

Persons.pro is a trusted parent Boon application that hosts a separately
compiled user program:

```text
Rust/Wasm capability host
    -> trusted precompiled Persons.pro Boon session
        -> editor and workspace state
        -> generic compiler capability
        -> restricted child Boon session
            -> public-page document
        -> nested preview surface
```

The parent and child have different authority:

- The parent may request workspace, authentication, publishing, and compiler
  capabilities.
- The child receives only the capabilities permitted for a public page.
- The child cannot read account credentials, other workspaces, persistence
  internals, filesystem paths, or arbitrary network resources.
- User source can produce a document and use an explicitly bounded public-page
  module API. It cannot inject JavaScript, HTML, CSS, shaders, or host handles.

The compiler produces an immutable child artifact identified by source digest,
compiler version, target profile, and capability profile. A successful artifact
replaces the prior child session atomically. Invalid source leaves the last valid
preview alive while displaying diagnostics for the current draft.

## Generic Nested Program Session

The local implementation requires a reusable runtime primitive, conceptually:

```text
Program/compile(project, target, capability_profile)
Program/start(artifact, initial_inputs)
Program/replace(session, artifact, migration_policy)
Program/document(session)
Program/diagnostics(project_revision)
Program/stop(session)
```

The concrete Rust API should follow existing compiler, plan, Session, retained
document, and source-replacement ownership rather than these placeholder names.
The semantic requirements are fixed:

- one authoritative child Session at a time;
- latest-wins compilation and stale-result rejection;
- bounded source size, compile work, runtime work, memory, and document size;
- explicit parent-to-child input capabilities;
- explicit child-to-parent document and diagnostic outputs;
- retained document identity across child updates where stable IDs permit it;
- cancellation or deterministic budget failure instead of freezing the parent;
- no mutable Session cache shared across revisions;
- no example-specific lowering or rendering.

The first implementation may keep parent and child execution on one thread if
fresh measurements remain comfortably inside the frame budget. The API remains
asynchronous so execution can move behind a worker or native job without
changing Persons.pro Boon code.

## Threading And Worker Policy

A Web Worker is not mandatory merely to hide compiler latency.

- Short Persons.pro source should compile and execute fast enough on one thread.
- Browser builds first measure synchronous compile and currentness behavior.
- The preferred initial target is edit-to-compiled p95 at or below 4 ms and a
  bounded maximum at or below 8 ms for the supported source envelope.
- A task may not block the interactive thread for 16.7 ms or longer.
- Transparent parallel execution is introduced only for independent pure graph
  regions where measurements justify synchronization overhead.

A worker remains a valid isolation boundary if deterministic execution budgets
cannot guarantee that malformed or adversarial child programs yield promptly.
It is an isolation and cancellation mechanism, not an excuse for a slow engine.

## Local Playground Product Shape

The first implementation is a built-in, versioned example:

```text
examples/persons_pro/
  RUN.bn
  App.bn
  Model/Workspace.bn
  Model/Account.bn
  View/Shell.bn
  View/Editor.bn
  View/Preview.bn
  View/Auth.bn
  View/Publish.bn
  Templates/Profile.bn
  Theme/PersonsTheme.bn
  assets/

examples/persons_pro.scn
examples/persons_pro.budget.toml
```

The outer native playground dev window edits the trusted Persons.pro example.
The Persons.pro preview window contains the product editor for the user's child
Boon page. These editors are distinct and must remain visually understandable.

The built-in source remains versioned under `examples/`. Runtime workspace data
uses the existing gitignored `playground/state/` application namespace. User
drafts are product state, not modifications to the built-in example files.

## Local Host Adapters

The native playground provides generic development adapters for the same typed
ports used by future browser and server hosts.

### Workspace

- Generate a deterministic test UUID in scenarios and secure random UUIDs in
  interactive runs.
- Persist workspace metadata, current draft, last valid artifact identity,
  published revisions, and account state through normal Boon persistence.
- Restore the application before publishing the first visible frame.
- Keep large assets content-addressed rather than duplicating them in every
  state checkpoint.

### Authentication

Native WGPU cannot prove browser WebAuthn behavior. The local authentication
adapter is therefore explicitly marked `Development passkey simulator`.

- It exercises the full Boon-owned registration, cancellation, success,
  duplicate-credential, second-passkey, sign-out, and sign-in workflow.
- It stores no fake private key in Boon state.
- Deterministic scenarios can select success, cancellation, and failure.
- It must never be reported as WebAuthn interoperability evidence.
- Browser acceptance later uses real HTTPS WebAuthn registration and assertion
  ceremonies against the same typed Boon workflow contract.

### Publishing

- Publishing compiles the exact immutable source revision again.
- A failed build cannot replace the current published revision.
- A successful local publish records an immutable revision and atomically moves
  the local public pointer.
- The embedded public view renders the published child artifact, not a manually
  duplicated approximation.
- Production later performs the same operation on the server with a pinned
  compiler and capability profile.

## Initial User Page API

The first public-page API should be ordinary Boon modules and records, not new
language syntax:

```boon
profile: [
    name: TEXT { Your name }
    introduction: TEXT { What are you making? }
    location: TEXT { Trondheim, Norway }
    projects: LIST {
        [title: TEXT { First project }, summary: TEXT { Describe it here. }]
    }
]

document: ProfilePage/render(profile: profile)
```

The exact syntax must compile under the current language before it becomes the
starter template. The template stays intentionally short and exposes a narrow,
typed page vocabulary. More general visual composition can be enabled as the
sandbox and renderer mature.

## Local MVP Interface

The first preview should be the actual tool, not a marketing landing page.

- A compact top bar shows the Persons.pro identity, workspace state, preview
  width control, and protect or account action.
- The main area uses a stable two-pane editor and live preview on desktop.
- Narrow layouts use tabs for Code, Preview, and Publish rather than squeezing
  both panes into unusable widths.
- Diagnostics are attached to source locations and summarized without covering
  the preview.
- Publish status distinguishes Draft, Building, Published, and Failed.
- Account state distinguishes Anonymous device, Passkey protected, and Signed
  out. The native simulator remains visibly identified.
- Invalid edits keep the last valid preview with a clear stale-revision marker.
- There are no decorative cards around whole page sections and no nested cards.

The visual direction should use the existing CryptoKick or Persons.pro design
material as reference, adapted to the current Boon document model and native
design system. The first viewport must clearly identify Persons.pro while still
showing the editor and actual page output.

## Persistence Model

Persist only authoritative product memory:

- workspace identity and ownership state;
- current source draft and source revision;
- last successful artifact identity;
- immutable published revision metadata;
- selected editor, preview surface, and preview-width mode as workspace UI
  preferences;
- registered credential descriptors, never private key material;
- bounded account and publish workflow state that cannot be reconstructed.

Do not persist:

- compiler diagnostics;
- derived document trees;
- layout frames, render scenes, GPU resources, or readback artifacts;
- compiler caches or mutable runtime Sessions;
- derived profile fields that can be recomputed from source;
- instrumentation and verifier reports.

The existing semantic-memory persistence and migration rules remain the only
state mechanism. Persons.pro does not introduce a second storage model.

## Accessibility And Search

The public page remains WGPU/WebGPU-rendered. Accessibility and search are
semantic projections of the same Boon document, not an independently authored
HTML/CSS renderer.

- Stable document nodes expose roles, names, values, actions, focus, ordering,
  headings, links, and text through the platform accessibility bridge.
- Browser hosting maintains a minimal semantic DOM or accessibility projection
  where browser APIs require it, while WGPU remains the visual renderer.
- Publishing emits canonical metadata, title, description, structured profile
  data, social previews, and a text-content snapshot derived from the compiled
  Boon document.
- The text snapshot is not an interactive fallback renderer and may not diverge
  semantically from the published artifact.
- Public links and headings remain real semantic nodes even when their pixels
  are drawn by WGPU.

## Verification Contract

### Semantic scenarios

The local scenario must cover:

1. fresh anonymous workspace creation;
2. starter source and initial child preview;
3. valid edit updates the preview;
4. invalid edit displays diagnostics and retains the last valid preview;
5. correction clears diagnostics and advances the child revision;
6. local publish records an immutable revision;
7. failed publish preserves the prior public pointer;
8. passkey-simulator cancellation preserves anonymous access;
9. first simulated passkey protects the workspace;
10. second simulated passkey is registered without creating a second account;
11. sign-out and simulated passkey sign-in restore access;
12. application restart restores authoritative workspace state;
13. desktop and narrow layouts expose the same product actions;
14. stale compile results cannot replace a newer draft or child session.

### Native visual and input evidence

- Use app-owned host events and WGPU readback through the native verifier path.
- Exercise the visible code editor, diagnostics, preview, protect, and publish
  controls using the launch-scoped verifier seat.
- Prove desktop and mobile-sized layouts without whole-desktop screenshots.
- Show the native virtual cursor during TEST playback.
- Never report the native passkey simulator as real WebAuthn evidence.

### Performance

- Keystroke-to-editor-visible p95: at or below 16.7 ms.
- Valid-edit-to-preview-visible p95 for the bounded starter project: at or below
  16.7 ms after warmup, with p99 at or below 25 ms.
- Passive preview scrolling p95: at or below 16.7 ms.
- No persistence transaction, report serialization, proof readback, or compile
  cache write blocks an interaction frame.
- Compilation is latest-wins with at most one pending child artifact.
- Normal edits do not rebuild the trusted parent runtime, full dev document, or
  unrelated retained render state.

## Implementation Slices

### Slice 1: Runnable product shell

- Add the versioned manifest entry, Boon modules, scenario, and budget.
- Implement anonymous workspace, starter source display, responsive editor and
  preview shell, account status, and local publish presentation in Boon.
- Use only currently supported language and document capabilities.
- Verify compile, scenario loading, retained document construction, and native
  launch before expanding the capability surface.

### Slice 2: Generic child program host

- Add typed parent-to-child compiler/session ownership independent of examples.
- Compile the actual starter source into a restricted child artifact.
- Embed the child document in the parent preview with stable retained identity.
- Preserve the last valid child on diagnostics and reject stale results.

Implementation checkpoint (2026-07-14): the generic Slice 2 code path is in
place. `Element/program` and `Scene/Element/program` lower to a typed private
descriptor; `ProgramArtifact`, `ProgramSession`, and `ProgramDocumentHost` own a
bounded `SoftwareBounded` child runtime; compilation runs through a latest-wins
per-host worker; child nodes, source routes, scroll roots, and materialization
IDs are namespaced into the retained parent document; invalid and stale results
cannot replace the active child. Persons.pro now edits the actual multiline Boon
source and no longer duplicates its public page in trusted parent code. Generic
compiler fixes cover function-wrapped HOLD constants, quoted-string newline
escapes, and function-body SOURCE continuations rather than hiding those engine
limitations in the example.

This checkpoint did not by itself complete the local-first milestone. The
2026-07-15 checkpoint below records the subsequent durable workspace,
publishing, authentication, and migration implementation. Native app-owned
visual evidence and release performance budgets remain governed by the stop
conditions below.

### Slice 3: Durable local workspace

- Bind the example to a stable application identity and redb namespace.
- Restore draft, artifact identity, published pointer, and account workflow.
- Add restart, clear, export, import, corruption, and migration coverage through
  existing persistence contracts.

### Slice 4: Local authentication and publishing workflows

- Implement the generic deterministic passkey development adapter.
- Complete protect, second-passkey, sign-out, sign-in, and failure flows.
- Publish immutable local revisions from the exact source digest.

Implementation checkpoint (2026-07-15): Slices 3 and 4 are implemented in
commits `442e7c4` through `4ca8a3d`. The generic redb-backed runtime restores
acknowledged authority and immutable child artifacts before the first product
frame, keeps persistence/artifact workers bounded, and supports restart,
selected clear, start-over, canonical export/import preview and activation,
and fail-closed corruption handling. Persons.pro has an exact semantic-memory
allowlist; compiler diagnostics, documents, layouts, render scenes, GPU data,
proof data, private passkey material, and other reconstructable machinery are
not stored.

The typed `DevelopmentPasskey` adapter covers registration and authentication
success, cancellation, failure, duplicate credentials, a second credential,
sign-out, and sign-in through the durable effect outbox. It binds the anonymous
workspace grant explicitly and remains labeled as a simulator. Publishing
recompiles the exact candidate source, stores the immutable artifact closure,
and advances the public pointer only after the durable success outcome; failed
or stale completions preserve the previous pointer. The published view mounts
that child artifact rather than a parent-owned duplicate.

The versioned manifest now carries a source-controlled Persons.pro migration
sequence from V1 through V3. V2 uses `DRAINING` and `DRAIN` to rename the
published source digest, V3 removes obsolete authority, and deterministic tests
cover incremental and skipped activation, restart, exact deletion, and stable
canonical module identity. The native verifier code also carries generic
frame-bound workflow checkpoints for restart, asynchronous artifact load,
responsive desktop/narrow layouts, migration activation, scrolling, and
visible TEST cursor playback.

Current implementation evidence (2026-07-16, through commit `be6ff21`):

- Asynchronous program-artifact store/load work no longer acts as a persistence
  checkpoint boundary. A 150 ms slow-I/O regression proves that an interleaved
  artifact does not split a collected authority batch. A second deterministic
  regression holds artifact execution with a condition-variable gate, queues
  another turn, and proves both the current and peak pending-checkpoint counts
  include the sealed batch without relying on scheduler timing.
- Restart verification requires the applied program-artifact load observation
  before the first product presentation and an exact metadata-identical event
  linked to the mounted `FrameEvidenceKey` afterward. It no longer infers
  startup ordering from a post-presentation event.
- Responsive evidence is armed only after the declared final native workflow
  baseline. It preserves source path, route intent, and control multiplicity,
  uses viewport plus propagated retained clip intersections, and binds the
  desktop baseline key/action digest to the narrow-layout proof.
- Migration verification proves the isolated migrated frame and a later
  app-owned WGPU frame where authority and the original mounted content,
  layout, and render revisions are all restored unchanged.
- A compact compiled-plan test pins the exact Persons.pro semantic-memory
  allowlist, its two authoritative lists and row fields, two durable effect
  contracts, and the exclusion of diagnostics and derived workflow status.
  Host-only scenario ownership is assigned to the manifest-backed native
  verifier.
- `cargo test -p boon_persistence --lib` passes all 54 tests.
- `cargo test -p boon_native_playground` passes all 63 tests and `cargo test -p
  xtask` passes all 19 tooling tests.
- `cargo check --workspace --all-targets`, `cargo test --workspace
  --all-targets --quiet`, and `cargo build --release
  -p boon_native_playground` pass from the current implementation.
- The architecture gate passes with 184,571 tracked Rust lines, 31,926 test
  lines, 31,876 playground production lines, 5,117 xtask production lines, and
  20,544 runtime-plus-executor production lines, all within their caps.
- No tracked Python source exists. Production engine/playground scans contain
  no Persons.pro branch; remaining Persons literals are source-controlled
  fixtures and tests.
- Independent persistence and native-verifier re-audits confirmed the corrected
  production batching, startup ordering, responsive identity/clip, and
  migration-revision contracts. Their timing-based test concern was replaced
  by the deterministic gate described above.

The older release Persons report remains useful only as a historical timing
sample: editor-visible p95 8.257 ms, child-preview p95 10.648 ms and p99
11.152 ms, starter compile p95 2.852 ms and max 2.952 ms, passive scroll p95
0.841 ms, and maximum interaction-frame blocking 9.803 ms. It predates the
current clean commits and cannot satisfy final handoff evidence.

The installed `/usr/bin/cosmic-comp` and rebuilt compositor have SHA-256
`f1efface3d67ac6b011712a812b406f8775e4ab23835ea1bb54a098310422e38`,
while the running compositor process still maps
`5fd348ebd1e142e70b357aae2ec5577d3a6a56034a791886d5597c0c76a70782`.
A user/session restart is therefore mandatory before regenerating every native
handoff report from current HEAD. Until those reports and the manifest-backed
aggregate pass, this local-first milestone remains incomplete.

### Slice 5: Browser host

- Build the Rust/Wasm browser shell and IndexedDB adapter.
- Add real WebAuthn registration and authentication over HTTPS.
- Verify Chrome/Chromium Linux, Firefox Linux with 1Password, and Safari on
  Apple platforms.
- Keep product UI and workflow policy in the same Boon modules.

### Slice 6: Hosted service

- Add Rust TLS/HTTP/storage mechanisms and a trusted server Boon application.
- Allocate workspaces and grants, verify credentials, store immutable builds,
  and serve public artifacts and semantic metadata.
- Recompile publishes with a pinned compiler and sandbox profile.
- Add quotas, abuse controls, audit records, backup, and restore.

## Stop Conditions

The local-first milestone is complete only when all of these are true:

- Persons.pro is selectable as a built-in versioned playground example.
- Its preview is an actual usable editor and live public-page preview.
- User source is compiled by the generic compiler into a restricted child
  Session; the preview is not duplicated manually in parent Boon code.
- Valid, invalid, stale, publish, authentication-simulator, restart, and
  responsive-layout scenarios pass deterministically.
- Authoritative workspace state restores from redb without persisting derived
  compiler, document, layout, or render data.
- Native app-owned visual/input evidence proves the complete local workflow.
- The measured interaction budgets pass in release mode.
- Audits find no Persons.pro-specific branches in engine or verifier crates.
- The native simulator is clearly distinguished from browser WebAuthn.
- The worktree is clean and all implementation commits identify any remaining
  browser or hosted-service work without claiming it is complete.

The full hosted milestone is complete only after the same Boon application runs
through the browser host, real WebAuthn interoperability passes on the supported
matrix, published pages survive fresh-device navigation, and production backup,
restore, quota, accessibility, and semantic search evidence passes.

## References

- `docs/architecture/NATIVE_GPU_PIPELINE.md`
- `docs/architecture/RUNTIME_MODEL.md`
- `docs/architecture/DELTA_PROTOCOL.md`
- `docs/plans/BOON_PERSISTENCE_ARCHITECTURE_PLAN.md`
- `examples/novywave/`
- `examples/kavik_cz/`
- [1Password passkeys](https://support.1password.com/save-use-passkeys/)
- [1Password browser integration on Linux](https://support.1password.com/connect-1password-browser-app/)
- [Chrome passkeys](https://support.google.com/chrome/answer/13168025)
- [Passkey device support](https://passkeys.dev/device-support/)
