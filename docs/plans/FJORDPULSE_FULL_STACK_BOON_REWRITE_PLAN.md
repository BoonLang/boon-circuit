# FjordPulse Full-Stack Boon Rewrite Plan

Date: 2026-07-16

Status: implementation plan. No implementation or production-readiness claim is
made by this document.

Reference FjordPulse revision:

```text
repository: /home/martinkavik/repos/FjordPulse
commit:     dd6e750c2ca9dec3041f66ceda31d30379d4027a
```

That commit is the immutable product and behavior reference for this rewrite.
Later FjordPulse commits do not silently expand or change this plan. A deliberate
rebase of the reference requires a plan edit, a new parity diff, and explicit
approval.

## Goal

Build a complete, production-shaped FjordPulse implementation whose client and
server application logic are Boon programs:

```text
FjordPulse Client Boon program
    -> generic Rust/Wasm browser host
    -> retained Boon document and scene
    -> browser WebGPU

FjordPulse Server Boon program
    -> generic Rust server host
    -> HTTP, WebSocket, timers, outbound HTTP, secrets, and redb capabilities
    -> Entur services
```

The deployed result runs at:

```text
https://fjordpulse-boon.kavik.cz
wss://fjordpulse-boon.kavik.cz/live
```

The rewrite must preserve the pinned product behavior, not its implementation
stack. PHP, CakePHP, AMPHP, SolidJS, TypeScript application code, MapLibre, and
SurrealDB are not runtime dependencies of the Boon deployment. The pinned
repository remains a read-only behavioral, contract, fixture, design, and test
oracle.

## Scope Decisions

### Complete Means Full Product Parity

The parity baseline is the pinned revision's:

- 108 `FP-*` stories and their 340 black-box scenarios;
- public HTTP and realtime contracts and valid/invalid fixtures;
- deterministic desktop, mobile, Admin, and bilingual visual-state inventory;
- public map, search, station, departures, vehicle, Focus, recovery, and
  localization workflows;
- demand-driven Entur collection and truthful freshness behavior;
- Admin authentication, read-only diagnostics, security, observability, and
  production smoke behavior;
- restart and deployment persistence behavior.

Technology-specific wording is translated honestly. For example, the new Admin
database view reports Boon semantic collections, indexes, migrations, and redb
health; it must not pretend that SurrealDB still exists. The new deployment uses
one Boon server process rather than separate FrankenPHP and AMPHP processes.

### Compatibility Policy

Product and failure semantics are the parity target. Unchanged public DTOs,
routes, envelopes, commands, events, validation failures, and status codes stay
wire-compatible with the pinned contracts. Phase 0 must maintain an explicit
contract-delta ledger for fields that would otherwise describe removed
technology falsely. The initially approved delta classes are only:

- `MapConfig` describes allowlisted raster tile sources instead of MapLibre
  style documents;
- Admin database/schema/migration DTOs describe Boon collections, indexes,
  migrations, and redb rather than SurrealDB INFO/SurrealQL objects;
- Admin service topology describes one Boon Server and its durable publication
  path rather than FrankenPHP, AMPHP, and a SurrealDB live-query bridge;
- deployment identity uses `fjordpulse-boon.kavik.cz` and the single-container
  topology;
- backup/restore automation checks are deferred exactly as stated below.

Each approved delta needs old-field-to-new-field rationale, updated schema and
valid/invalid fixtures, Client/Server version compatibility, and black-box
evidence for the same user/operator intent. No other wire delta is accepted
implicitly because the implementation is a rewrite.

### Explicitly Deferred

The following are not part of this implementation milestone:

- backup automation;
- restore automation;
- backup retention or off-host backup storage;
- disaster-recovery drills based on backups;
- high availability;
- clustering;
- multi-node execution;
- multi-replica WebSocket fan-out;
- zero-downtime deployment that overlaps two redb writers.

This is a deliberate override of the backup/restore portions of pinned stories
FP-089 and FP-092. It is not an override of persistence. A process restart,
container restart, host-service restart, and normal Coolify redeploy must all
preserve acknowledged application state in the mounted redb volume.

The deployment remains one process and one redb writer. A second process trying
to open the same production namespace must fail closed. Coolify replacement is
stop-old/start-new, with bounded downtime, rather than concurrent rolling
replacement.

## Hard Invariants

1. **Both product halves are Boon.** Client UI/workflow logic and server
   routing/domain/scheduling logic are authored in Boon. Rust supplies generic
   platform mechanisms only.
2. **One generic engine remains.** Parser, typechecker, IR, compiler, plan,
   executor, runtime, persistence, document, renderer, browser host, server
   host, and verifiers may not branch on `fjordpulse`, source paths, labels,
   fixture text, route names, geometry, or package identity.
3. **No Boon workaround may hide an engine defect.** A real language/runtime
   limitation is reduced to an application-independent reproducer, fixed in the
   owning generic layer, and protected by a generic regression before product
   work continues.
4. **No Python.** Do not add Python source, Python build/test/deploy scripts,
   Python code generation, Python fixture servers, or commands that invoke
   Python. Rust, Cargo/xtask, shell, and app-independent browser automation are
   sufficient.
5. **No second renderer.** WebGPU renders the product. HTML/CSS may provide the
   minimal canvas host and semantic accessibility projection, but may not become
   a separately authored FjordPulse visual implementation.
6. **No direct browser-to-Entur or browser-to-redb path.** The Client talks to
   the Server over the declared same-origin HTTP/WS contracts. Only raster tile
   provider traffic may leave the browser directly, through an allowlisted
   public tile-source capability.
7. **No implicit data mode.** `Deterministic` and `Live` are explicit immutable
   launch modes. Missing Live configuration is a startup failure, never a
   fallback to fixtures.
8. **Persistence is semantic.** Authoritative server state flows through the
   existing Boon prepare/commit/settle and redb persistence architecture.
   Documents, layouts, GPU resources, sockets, caches, and verifier evidence are
   reconstructed.
9. **The pinned product is the oracle.** The rewrite does not copy custom PHP,
   TypeScript, SQL/SurrealQL, MapLibre mutation code, or framework structure.
   Contracts, fixture data, product copy, visual references, and the brand mark
   may be carried forward with provenance.
10. **No readiness by documentation.** Completion requires fresh, source-bound
    executable evidence from the final implementation and the deployed domain.

## Target Topology

```text
Browser
  generic Rust/Wasm web host
    compiled FjordPulse Client Boon artifact (ProgramRole::Document)
    retained document + semantic accessibility projection
    retained raster MapViewport + geographic overlays
    WebGPU renderer
    generic HTTP/WS client capabilities
          |
          | HTTPS / WSS, same origin
          v
Coolify-managed Traefik
          |
          | private container port 8080
          v
generic Boon server host
  compiled FjordPulse Server Boon artifact (ProgramRole::Server)
  deterministic turn executor and bounded task/effect scheduler
  redb persistence worker -> /var/lib/boon/fjordpulse/state.redb
  outbound HTTP capability -> Entur allowlist
  static browser artifact service

Browser MapViewport -> allowlisted MapTiler raster HTTPS tiles
```

TLS is terminated by Coolify/Traefik. The container serves the static browser
bundle, `/api/*`, and `/live` from one port. There is no public database port,
no embedded second reverse proxy, and no separate realtime replica.

## Ownership Boundary

### Boon Owns

- all public and Admin routes and response policy;
- request and WebSocket message validation beyond generic framing limits;
- all domain records, mapping, normalization, classification, and freshness;
- station import state, search policy, clustering policy, paging, and cursors;
- Entur request planning, shared budgets, retries, backoff, caching policy, and
  demand priorities;
- station, vehicle, journey, watch, timetable, event, and diagnostic state;
- semantic event/version creation and room/watch publication policy;
- client routing, localization, camera intent, selection, tabs, sheets, search,
  reconnect, periodic fallback, stale-result rejection, and Focus behavior;
- all layout, styling, visible states, accessible labels, and public/Admin copy;
- the exact distinction between passenger state and position freshness;
- explicit `Deterministic` and `Live` mode behavior;
- deterministic fixture data and scenario scripts.

### Generic Hosts Own

- loading and validating trusted precompiled artifacts;
- process lifecycle, bounded input queues, cancellation, and shutdown;
- TCP/HTTP/WebSocket framing and connection resources;
- outbound HTTP transport, DNS, TLS validation, pooling, timeouts, and body
  byte limits;
- request/response correlation and socket identity below the Boon data model;
- clocks, timers, secure randomness, environment/configuration, and host-vault
  secret references;
- cryptographic primitives, constant-time secret verification, signed cookies,
  and secure cookie serialization exposed through typed generic contracts;
- redb transactions, durability, repair, compaction, and single-writer locking;
- browser URL/history, canvas, input translation, accessibility projection,
  fetch/WebSocket mechanics, and IndexedDB;
- raster tile fetch/decode/cache/upload mechanics and Web Mercator projection;
- WGPU/WebGPU device, surface, retained render resources, and readback;
- bounded host CPU, memory, filesystem, build, and persistence diagnostics.

The host may enforce capability, resource, framing, origin, and size boundaries.
It may not choose a FjordPulse route, station, refresh priority, search result,
room, map marker, layout, warning, or visible state.

## Production Package Shape

Create one application package with two independently compiled programs and
shared pure modules. Exact filenames may follow the package-manifest work, but
the ownership must remain recognizable:

```text
apps/fjordpulse/
  app.toml
  Shared/
    Contracts.bn
    Domain.bn
    Realtime.bn
    Locale.bn
    Time.bn
    Geo.bn
    Validation.bn
  Client/
    RUN.bn
    App.bn
    Model/
    Network/
    Theme/
    View/
  Server/
    RUN.bn
    Config.bn
    Http.bn
    Realtime.bn
    Entur/
    Repository/
    Scheduler/
    Admin/
  Fixtures/
    ...
  assets/
    ...
  scenarios/
    ...
  budgets/
    ...
```

The package manifest declares:

- stable package identity;
- one browser/client entry compiled as `ProgramRole::Document`;
- one nonvisual server entry compiled as `ProgramRole::Server`;
- source, generated fixture, asset, migration, scenario, and budget files;
- capability profiles for each program;
- compatible client/server protocol versions;
- stable state namespaces for deterministic, staging, and production runs.

The pair declaration is build metadata, not app-specific host code. The generic
bundle loader must also work for an unrelated paired example.

Shared modules are compiled into each artifact. Client and Server do not share
mutable memory or runtime handles. Their only production communication is the
versioned HTTP/WS protocol.

## Generic Platform Prerequisites

Application implementation may use a prerequisite only after its independent
generic fixture, unit/integration suite, and plan verification pass. Concurrent
or partial worktree changes do not count as completed capability.

### P0: ProgramRole Server And Paired Bundles

`ProgramRole::Server` must become a complete host boundary, not only an enum or
an inference based on the absence of `document`.

Required semantics:

- the manifest explicitly declares the expected role for every entry artifact;
- compile fails when a Server artifact contains a document root or when a
  Document artifact lacks its visual output contract;
- a Server artifact exposes typed host output roots and typed source/effect
  contracts without a document plan;
- the plan verifier checks role, output contracts, capabilities, persistence,
  and target profile together;
- Server startup restores/migrates/settles before binding listeners, timers, or
  outbound effect workers and before readiness becomes true;
- one shared Server Session processes source turns in deterministic sequence;
- request/socket correlation remains host-owned and cannot be persisted or
  compared as ordinary Boon data;
- asynchronous completions carry generic effect invocation identity and are
  rejected when stale, cancelled, or from a prior launch epoch;
- bounded admission rejects overload before mutating authority;
- graceful shutdown stops admission, settles the bounded in-flight set, applies
  the required Immediate persistence barrier, then exits;
- forced termination restores only the last acknowledged redb epoch.

Add an unrelated server fixture that serves a small JSON API, accepts a WS
message, schedules a timer, persists a counter, restarts, and proves no document
or example-specific host path exists. Add an unrelated paired bundle fixture
whose Document and Server programs share a pure contract module.

Exit gate: the generic fixtures run through real loopback HTTP and WebSocket,
survive restart, and pass the no-package-branch audit before FjordPulse Server
routes are added.

### P1: Real Number

Boon `Number` must represent finite non-integer values consistently across all
targets. FjordPulse must not encode coordinates, zoom, bearing, distance,
percentages, durations, or interpolation factors as scaled integers or text.

Canonical contract:

- `Number` is one finite IEEE-754 binary64 semantic type;
- integer and decimal literals produce the same Number type and runtime value
  representation;
- NaN and positive/negative infinity cannot enter a Boon value;
- negative zero is normalized to positive zero for equality, ordering, hashing,
  persistence, and protocol identity;
- parser, typechecker, IR, plan constants, executor slots, indexed fields,
  lists, source payloads, effects, document styles, scenario values, debug
  output, and host outputs all support the same representation;
- arithmetic, comparisons, min/max, interpolation, rounding, text conversion,
  and JSON conversion have specified checked behavior;
- division by zero, overflow to non-finite, and invalid text conversion produce
  typed errors rather than non-finite values;
- list/byte indices and bounded counts require an explicit checked whole-number
  conversion and reject fractional, negative, or out-of-range values;
- the canonical CBOR codec stores float64 Number values and hashes canonical
  bits;
- an old integer-only persisted Number is upgraded only by a generic exact
  storage-format migration; values that cannot be represented exactly fail with
  a diagnostic rather than silently round;
- native and Wasm golden tests compare canonical bits and encoded bytes.

Do not leave a permanent split in which some arithmetic uses an integer value
variant and decimal arithmetic takes a second fallback path. Transitional
variants are allowed only while the migration is incomplete and must be removed
before P1 passes.

Exit gate: decimal camera/coordinate, persistence, arithmetic, JSON, indexed
range, and native/Wasm round-trip fixtures all pass; every existing example and
migration gate remains green.

### P2: JSON

Add a generic bounded JSON data and codec boundary. Do not parse JSON with Boon
text splitting and do not expose `serde_json::Value` or Rust object identity to
Boon.

Required semantics:

- JSON null, booleans/tags, finite Number, Text, arrays, and objects map to
  ordinary structurally inspectable Boon data;
- UTF-8, escapes, surrogate handling, exponents, and nested arrays/objects are
  standards-compliant;
- duplicate object keys are rejected by the strict server/client profile;
- decoder limits cover input bytes, nesting depth, object fields, array items,
  total nodes, and string bytes;
- decode returns a typed success or bounded diagnostic with byte location;
- encode is deterministic, emits valid UTF-8, rejects non-finite Number at the
  boundary, and uses canonical object-key ordering for fixtures/hashes;
- streaming HTTP bodies and WebSocket messages are bounded before full decode;
- domain validation and domain-to-wire mapping remain Boon code;
- JSON schema/OpenAPI checks are test tooling and contract evidence, not a
  second production validation implementation.

Golden coverage must include every pinned HTTP and realtime fixture, invalid
UTF-8, duplicate keys, deep nesting, oversized payloads, decimal coordinates,
unknown fields, missing fields, and native/Wasm equivalence.

Exit gate: an unrelated Boon JSON echo/validation fixture passes fuzz/property
tests and both host targets before Entur or FjordPulse contracts use the codec.

### P3: Generic HTTP And WebSocket Capabilities

Implement both server-side and client-side transport contracts. Prefer mature
Rust protocol/TLS libraries; do not hand-roll HTTP or WebSocket framing.

#### Server HTTP

The generic host supplies bounded source events for:

- startup configuration ready/failed;
- HTTP method, normalized path segments, query multimap, allowlisted headers,
  cookie metadata, body bytes, peer address result, and deadline;
- disconnect/cancellation;
- shutdown.

The Server Boon program returns typed response intent containing status,
headers, cookies, and bytes/JSON body. The host owns response correlation and
may emit a generic timeout/overload response only when the program cannot be
admitted or complete within its declared budget.

The host also provides generic static-asset service, gzip/brotli negotiation,
ETag/cache headers, and SPA fallback configured by the package manifest. It
must not know FjordPulse route names.

#### Server WebSocket

The generic host owns upgrade/framing, ping timeouts, connection resources, and
bounded write queues. It emits open, text/binary message, close, and transport
error sources. Boon emits generic actions:

- accept/reject upgrade;
- reply to the current message;
- join/leave a textual room;
- broadcast a frame to a room;
- send/close the current connection;
- request a resync frame.

Room names and all membership policy originate in Boon. Socket handles do not
become Boon values. A slow client receives bounded coalescing or a policy close;
it cannot grow memory without limit or block another connection.

#### Outbound HTTP/WS

Both program roles receive a generic effect contract for allowlisted outbound
HTTP and, for platform completeness, outbound WebSocket. It supports method,
headers, bounded body, connect/overall timeout, cancellation, response status,
headers, and bounded bytes. The host owns pooling, DNS, TLS, redirects, and
connection replacement. The capability profile restricts destinations by
named endpoint, scheme, host, port, and path prefix so Boon cannot turn user
input into arbitrary SSRF.

#### Browser Client

The browser host exposes same-origin fetch and WebSocket effects, lifecycle and
online/offline sources, reconnect-safe cancellation, and response/frame byte
limits. Product retry, subscription, stale-result, and fallback policy stays in
the Client Boon program.

#### Supporting Host Primitives

Finish generic typed contracts for monotonic/wall clocks, timers, secure random
bytes, URL/history, trusted proxy resolution, cookies, secret references,
HMAC/signature verification, constant-time configured-secret checks, and
bounded host-resource diagnostics. Secret bytes never enter Boon state, logs,
reports, or JSON.

Exit gate: generic loopback server/client fixtures prove HTTP request/response,
outbound HTTP, WS lifecycle, rooms, malformed messages, cancellation, timeout,
backpressure, reconnect, cookies, origin rejection, and clean/forced restart.

### P4: Generic Indexed Queries Over Persistent Collections

FjordPulse's national station catalog and observation history must not be
implemented with request-time `List/filter` scans. Extend the generic semantic
memory plan rather than adding raw redb calls to Boon.

Required plan concepts:

```text
CollectionPlan
  stable collection identity
  stable row identity and recursive row schema
  authoritative fields
  retention/migration metadata

IndexPlan
  stable index identity
  ordered key projection
  unique or multi-value policy
  ascending/descending order
  collation/normalization contract

QueryPlan
  exact, prefix, lower/upper range
  ordered union/intersection
  limit and deterministic tie-break
  opaque continuation cursor
  optional bounded pure residual
```

The language-facing API should be generic standard-library declarations and
queries over keyed collections. Final syntax is decided in the platform slice;
the semantic constraints are fixed:

- key projections are closed, pure, typed, deterministic, and compiler-known;
- supported keys include Text, Number, tagged values, and compound tuples;
- multi-value projections support token/prefix indexes without duplicating
  application rows;
- every result has a deterministic total order ending in stable row identity;
- limits are mandatory on externally triggered queries;
- cursors bind index identity, query fingerprint, schema, epoch/version, and
  last ordered key; stale/incompatible cursors fail explicitly;
- a bounded residual predicate may examine only index-selected candidates;
- query metrics report selected index, ranges, keys visited, rows examined,
  residual count, returned count, and elapsed time;
- a declared indexed request path may not silently fall back to a full scan;
- inserts, updates, removals, migrations, and retention cleanup update index
  entries atomically with row authority;
- redb index entries are derived acceleration state with schema hashes. They
  may be rebuilt from canonical authority, but are never allowed to disagree
  silently;
- restore validates or rebuilds indexes before readiness; it does not publish
  an unindexed server;
- in-memory and redb drivers execute the same golden query plans and ordering;
- current Session queries see current committed authority, including a bounded
  not-yet-checkpointed tail, without waiting for request-time disk I/O;
- persistence remains off the interactive browser/render path.

Add generic pure text normalization/tokenization, exact one-edit Damerau
distance, Web Mercator helpers, and WGS84 distance/bounds helpers where the
language lacks them. These belong in standard modules with independent tests,
not in the executor or a FjordPulse host branch.

Exit gate: a generic 60,000-row catalog fixture proves exact, prefix,
multi-value, compound, spatial-cell-plus-residual, time-range, union,
intersection, pagination, mutation, migration, restart, and no-scan evidence.

### P5: redb Server Persistence

Use the existing Boon persistence architecture and the generic query extension
for the Server artifact.

Required production behavior:

- stable application identity separates Client, Server, test, staging, and
  production namespaces;
- one redb database file lives under the mounted production data directory;
- startup obtains an exclusive single-writer lock before restore;
- restore, migration, index validation/rebuild, and output settling finish
  before listener admission and readiness;
- server mutations that are acknowledged to a remote client use Immediate
  durability at the required barrier;
- high-frequency replaceable telemetry may use bounded Buffered coalescing when
  the UI and Admin report its pending tail honestly;
- canonical state change and semantic realtime event/outbox intent commit in
  one authoritative turn;
- WebSocket publication starts only after the required durable acknowledgement;
  duplicate publication is harmless because event IDs/versions are stable and
  clients reject old versions;
- redb failure produces backpressure or failed readiness, not dropped authority;
- clean shutdown flushes the acknowledged tail;
- crash recovery restores the last acknowledged epoch and reconciles pending
  idempotent outbox items;
- schema changes use the ordinary source-controlled DRAIN/DRAINING migration
  sequence and atomic activation;
- incompatible/corrupt state fails closed and never resets production data;
- compaction, retention, and index rebuild run outside request/render hot paths.

Do not persist:

- active socket handles or room membership;
- HTTP request correlation;
- current timer handles or connection pools;
- decoded JSON trees that can be remapped from canonical state;
- HTTP response caches unless explicitly authoritative product cache records;
- Client documents, layout, map tiles, GPU resources, or accessibility trees;
- query planner caches, currentness bits, reports, or proof images.

Exit gate: forced-crash matrices, clean restart, schema migration, corrupt-copy
rejection, and a container-like process replacement all preserve exactly the
last acknowledged authority and never expose a pre-restore response.

### P6: Retained Raster Tile MapViewport

Add one renderer-neutral retained document/scene primitive for interactive
raster maps. It must be useful to any Boon application and must not contain
FjordPulse marker, station, vehicle, MapTiler-style, or route logic.

Conceptual Boon-facing data:

```text
MapViewport[
  camera: [longitude, latitude, zoom, bearing]
  bounds: [width, height, scale]
  tile_source: TileSourceRef
  overlays: List<MapOverlay>
  interaction: [pan, wheel_zoom, pinch_zoom, keyboard_zoom]
]
```

Generic overlay contracts cover:

- geographic point/symbol anchors with z-order and hit identity;
- count circles/clusters;
- polylines and filled polygons;
- text labels with collision priority;
- selected/focused overlay priority;
- optional screen-space controls composed as normal Boon document elements.

The app supplies geographic data, visual style data, labels, selection, and
behavior. The generic renderer supplies projection, clipping, batching,
retention, and hit testing.

Tile requirements:

- XYZ/Web Mercator raster sources with explicit min/max zoom, tile size,
  attribution, allowed origins, and URL-template capability reference;
- no raw provider secret embedded in Boon source or report artifacts;
- visible tile selection plus bounded overscan;
- generation-aware request cancellation and stale-response rejection;
- retained decoded and GPU-resident caches keyed by source/z/x/y/scale;
- parent/child tile retention while replacement tiles load, with no blank flash;
- async fetch, image decode, texture upload, and eviction outside input turns;
- bounded concurrent fetch/decode/upload and deterministic LRU accounting;
- device-loss reconstruction from retained CPU descriptors, not Boon state;
- camera updates patch one retained node and overlay deltas rather than rebuild
  the document or all map resources;
- app-visible load/error/retry sources with no hidden fallback source;
- renderer-owned pixel readback and frame evidence for generic verification.

FjordPulse uses two allowlisted precomposed raster basemaps: labelled satellite
as default and labelled streets as the alternate. Provider labels are part of
the raster tiles; Boon overlays selected transport above them. The deterministic
mode uses declared local raster fixtures through the same `TileSourceRef`
contract, not a renderer shortcut.

Exit gate: an unrelated map example proves pan/zoom, resize, cache reuse,
cancellation, tile failure/retry, overlays, hit testing, device loss, and frame
budgets on native WGPU and browser WebGPU with the same descriptor semantics.

### P7: Browser WebGPU Host

Build a generic Rust/Wasm host for `ProgramRole::Document`. It must run the
normal compiler artifact, Session, retained document, and renderer architecture
rather than a browser-only FjordPulse implementation.

Required capabilities:

- WebGPU adapter/device/surface acquisition and explicit unsupported state;
- no WebGL visual fallback for the production product;
- `requestAnimationFrame` presentation with demand-driven idle behavior;
- retained document/layout/render state and the MapViewport path;
- pointer, wheel, touch/pinch, keyboard, focus, IME, clipboard, resize, scale,
  visibility, and reduced-motion inputs;
- asynchronous fetch, WebSocket, timer, URL/history, and IndexedDB effects;
- minimal semantic DOM/accessibility projection derived from the same Boon
  document roles, names, values, actions, focus, order, links, and text;
- no independently authored HTML/CSS FjordPulse layout;
- device-lost reporting and bounded reconstruction;
- app-owned canvas/readback evidence, adapter information, frame timestamps,
  allocation counts, and retained patch counts;
- generated Wasm bootstrap glue may exist, but no FjordPulse JS/TS application
  logic may be added.

Correctness CI may use a declared software WebGPU adapter. Release performance
evidence must identify a hardware-backed adapter and fail if the run silently
falls back to software. Browser support is capability-based: lack of required
WebGPU or semantic APIs shows a small host-owned unsupported message before a
program session starts.

Exit gate: Counter, TodoMVC, the generic map example, and a generic HTTP/WS
client run in the browser host with semantic, visual, input, persistence, and
performance evidence before FjordPulse Client is enabled.

## Paired Program Contract

### Shared Pure Domain

The Client and Server compile the same pure structural definitions for:

- `Station`, `StationCluster`, `Coordinate`, and `BoundingBox`;
- `Departure`, `DepartureBoard`, `StationTimetablePage`, and cursor metadata;
- `StationSnapshot`, `StationVehicle`, serving coverage, call role, and
  progress;
- `VehicleState`, `VehicleObservation`, `VehicleTransportMode`, passenger
  service state, freshness, monitored call, and route progress;
- `JourneySnapshot`, geometry, calls, and upcoming stops;
- `Watch`, watch type/state/priority, source state, and data state;
- `MapConfig`, basemap descriptors, health, diagnostics, and Admin DTOs;
- HTTP success/error envelopes;
- realtime client commands, server frames, event versions, and protocol errors;
- locale keys and canonical Norwegian/English copy data;
- normalization, time-zone-facing labels, and validation helpers that are truly
  pure and target-independent.

Wire DTOs and internal records are separate where ownership differs. Public
Entur identifiers remain Text. RFC3339 timestamps remain Text at the wire
boundary and are parsed into checked time values inside the programs.

### Client Boon Program

The Client owns one coherent application state machine covering:

- default Norwegian Bokmål locale and explicit `NO`/`EN` switching;
- public and Admin routes, history, canonical map hash, and invalid-route state;
- intro visibility, basemap choice, responsive width, and mobile sheet snap;
- map camera, viewport request generation, overlay ordering, and selected pins;
- search opening, text, keyboard selection, pending generation, results, empty,
  and errors;
- selected station, tab, compact board, daily timetable pages, serving and
  nearby vehicles, details, loading, stale, empty, retry, and cursor expiry;
- selected vehicle, trail, journey, previous/next/upcoming stops, passenger
  classification, stale/lost state, refresh, Focus/follow/pause/resume/unfocus;
- HTTP requests with structural generation fingerprints and stale completion
  rejection;
- WS connect/authenticate/subscribe/resubscribe/message-version/reconnect state;
- periodic HTTP refresh only while realtime is degraded, with authoritative
  snapshot replacement and no duplicate watch policy;
- contextual public health notices and source provenance separate from health;
- Admin login/session/logout, status, infrastructure, watches, Entur log,
  realtime, persisted events, redb schema/index, and migration views;
- responsive desktop/mobile layout and every deterministic visual state.

Only explicit user preferences are durable in browser IndexedDB: locale,
basemap, intro choice, and a valid canonical camera if approved by the product
contract. Active sockets, pending requests, transient hover/focus, timers,
diagnostics, map tiles, and server snapshots are reconstructed. A browser
refresh resubscribes and asks the Server for current authoritative snapshots.

#### Physical Styling Contract

The Client uses Boon's physical scene styling API as the product styling
system, not a flat compatibility subset. `FjordPulseTheme` owns named
`material`, `elevation`, and `lights` functions; views compose those tokens
through `Scene/new` and `Scene/Element/*`. Panels, controls, selected/focused
states, sheets, dialogs, map controls, pins, warnings, and Admin surfaces use
named physical materials and restrained depth appropriate for a professional
transit application. Raw color values belong in the theme implementation, not
scattered through product views.

Physical styling must remain retained and renderer-neutral: state changes patch
material/elevation inputs on stable scene identities and do not rebuild the
document. Native WGPU and browser WebGPU consume the same material/light scene
contract. Accessibility semantics remain explicit scene metadata and never
depend on lighting, depth, color, or GPU pixel inference. Visual gates cover
light/dark contrast where supported, hover/focus/pressed/selected states,
disabled/error states, depth ordering, shadow clipping, responsive layouts,
and deterministic WGPU/WebGPU evidence. A flat per-control style workaround is
not an accepted final implementation.

### Server Boon Program

The Server owns:

- startup configuration validation and explicit mode selection;
- all public/Admin HTTP dispatch and typed response mapping;
- Admin session policy, operator/demo separation, allowlisted demo reads, and
  rate limits;
- map configuration allowlist and public provider attribution;
- station catalog import/resume/provenance and local map/search queries;
- search normalization, prefix ranking, bounded one-edit correction, vehicle
  search, and cross-entity merge;
- station snapshot, compact board, complete-day timetable cache/paging, station
  call matching, nearby radial results, and coverage truthfulness;
- current vehicles, observations, freshness transitions, passenger-service
  classification, journey cache, polyline/geometry mapping, and retention;
- connection-independent durable watch demand and in-memory host room actions;
- demand scheduler priority: Focus, selected vehicle, selected station,
  operator/pinned, then maintenance;
- shared Entur request budget reservation, cache, timeout, backoff, retry, and
  request evidence;
- independent Journey Planner and Vehicle Positions refresh outcomes so one
  failure does not erase the other's authoritative cache;
- semantic content hashes/versions, durable events, event retention, and one
  publication path;
- health/readiness and bounded structured diagnostics.

The browser never sends a database query, index name, redb path, Entur URL, or
arbitrary outbound destination.

## Explicit Execution Modes

Use one closed startup value:

```text
RunMode = Deterministic[scenario, seed, initial_time] | Live
```

The spelling may follow normal Boon tag syntax. Its semantics may not be
inferred from hostname, missing secrets, build profile, request path, or a
failed Live dependency.

### Deterministic

- Uses a virtual wall/monotonic clock controlled by scenario turns.
- Uses seeded deterministic IDs and random outcomes where randomness is part of
  product behavior.
- Uses declared fixture HTTP responses and local raster tiles through the same
  generic transport/tile capabilities as Live.
- Uses an isolated in-memory or temporary redb namespace selected by the test
  manifest.
- Makes every Entur outcome, delay, 429, malformed payload, disconnect, tile
  failure, and recovery an explicit scripted event.
- Never reaches public Entur or MapTiler hosts.
- Advertises deterministic/demo provenance in API and UI.
- Enables development-only scenario endpoints only when the launch manifest
  also grants that capability.

### Live

- Uses real clocks, secure randomness, production redb, Entur endpoints, and
  the configured allowlisted raster provider.
- Requires public origin, Entur client name, tile source/key, signing secrets,
  admin identity, trusted proxy policy, data directory, and resource limits.
- Refuses fixture/scenario endpoints and refuses a deterministic transport
  adapter.
- Never falls back to fixture data when Entur, tiles, HTTP, WS, or redb fails.
- Retains acknowledged cached data and reports stale/rate-limited/unavailable
  state honestly.
- Advertises `Transport data: Entur` without claiming an individual request is
  healthy.

The same Client and Server Boon source modules run in both modes. Mode-specific
data is supplied by capability/configuration adapters, not `if fjordpulse` host
branches or duplicated product programs.

## Server Persistence And Query Model

### Stable Identities

Planned production identities:

```text
client package:     cz.kavik.fjordpulse.client
server package:     cz.kavik.fjordpulse.server
deployment domain: fjordpulse-boon.kavik.cz
production state:  production-v1
```

Tests and staging use distinct namespaces. The state namespace is never derived
from a container ID, image hash, source path, or launch order.

### Authoritative Collections

The Server's semantic collections replace the pinned SurrealDB tables:

| Collection | Authoritative purpose |
| --- | --- |
| `stations` | imported station identity, name/type, coordinates, normalized search fields, import provenance |
| `station_snapshots` | bounded current board, station-serving matches/coverage, nearby summaries, source state, semantic version |
| `station_timetables` | immutable versioned Oslo-local day cache and bounded page metadata |
| `current_vehicles` | current position, mode, passenger classification, freshness, compact journey progress, semantic version |
| `journey_snapshots` | cached complete route geometry and ordered calls |
| `vehicle_observations` | bounded ordered trails with explicit expiry |
| `watches` | TTL demand for station, vehicle, Focus, and operator scopes independent of socket membership |
| `realtime_events` | bounded semantic notification/audit rows emitted from canonical changes |
| `entur_request_log` | bounded outbound outcome, latency, cache, backoff, and scope evidence |
| `entur_budget_state` | shared rolling reservation ledger and idempotent request reservations |
| `system_status` | bounded observed dependency and scheduler state |
| persistence metadata | schema, migration edges/attempts, epochs, outbox, index status, and build compatibility |

Nested snapshot and timetable data remain bounded document-shaped values so an
HTTP resync is atomic. Stable links are Text IDs resolved by indexed queries;
redb internal keys never cross the Boon or HTTP boundary.

### Required Indexes And Bounds

| Query | Required generic index plan | Bound/evidence |
| --- | --- | --- |
| station by ID | unique Text exact | one row |
| map stations/clusters | spatial cell plus longitude/latitude ranges and exact bounds residual | complete matched total; at most 2,000 response items; direct markers only at zoom 9+ and at most 300 rows |
| station search | normalized whole-field and multi-token prefix ranges | bounded candidates before rank; no catalog scan |
| one-edit station correction | token length/first/last compound candidate index plus exact Damerau residual | correction lane only for one token of at least four characters; bounded candidate set |
| vehicle search | normalized multi-token prefix range plus exact ID | lost rows excluded except exact ID recovery |
| station snapshot/timetable | station ID plus date/version/offset | preview at most 20; page at most 50; cursor version-bound |
| serving vehicle match | dated journey identity compound exact | at most 200 unique candidate journey/date pairs |
| nearby vehicles | spatial cell/bounds plus exact 5,000 m residual | deterministic distance order and reported radius |
| vehicle trail | vehicle ID plus observation time descending | retention- and response-bounded |
| watch scheduling | state/priority/next-at compound ordered | due-page bound; shared scope deduplication |
| request budget | source plus reserved-at range | rolling 60-second bound and idempotent request ID |
| events/logs/retention | created/expiry time range | bounded Admin page and cleanup batch |

Every query used by a request or scheduler emits planner evidence. A report that
only says `fast` without proving the selected index and examined-row bound does
not pass.

### One Realtime Publication Path

The replacement for the pinned database-live-query loop is:

```text
Entur or deterministic source completion
  -> Server Boon maps and validates domain data
  -> one atomic authority turn updates canonical record
  -> same turn creates semantic event and durable publication intent
  -> redb Immediate acknowledgement where externally promised
  -> generic WS host broadcasts the Boon-selected room frame
  -> Client applies only a newer semantic version
```

There is no second direct broadcast after a canonical write. Full snapshots are
authoritative; events are notifications. Reconnect joins the room, obtains a
fresh snapshot, then accepts newer versions. The daily timetable does not emit
realtime events and never enters station snapshot frames.

### Restart And Redeploy Contract

Before a restart/redeploy test, record at least:

- persistence epoch and schema hash;
- station catalog count and import digest;
- one station snapshot/version;
- one current vehicle and trail digest;
- one timetable cache version;
- one durable watch/expiry;
- event/log high-water marks.

After a process kill, normal restart, and Coolify redeploy:

- the same application identity opens the same mounted redb file;
- restore/migration/index validation completes before readiness;
- all acknowledged values above are identical or have advanced only through an
  explicit post-restart source turn;
- sockets reconnect and resubscribe; socket/room handles are new;
- due watches resume without duplicate immediate Entur effects;
- pending idempotent intents reconcile by stable invocation key;
- no default/empty frame or empty API response is published before restore;
- a missing volume, wrong namespace, lock conflict, corrupt store, or unsupported
  schema keeps readiness false and never starts with empty production state.

These checks are blocking even though backup/restore automation is deferred.

## HTTP Contract Plan

Preserve the pinned public routes, envelopes, and product behavior through the
Server Boon program, subject only to the explicit compatibility-delta ledger:

```text
GET    /api/health
GET    /api/readiness
GET    /api/map/config
GET    /api/stations
GET    /api/search
GET    /api/stations/{stationId}
GET    /api/stations/{stationId}/departures
GET    /api/stations/{stationId}/nearby-vehicles
GET    /api/vehicles/{vehicleId}
POST   /api/realtime-token
GET    /api/admin/demo-credentials
GET    /api/admin/session
POST   /api/admin/session
DELETE /api/admin/session
GET    /api/admin/status
GET    /api/admin/watches
GET    /api/admin/entur-log
GET    /api/admin/realtime
GET    /api/admin/events
GET    /api/admin/database/schema
GET    /api/admin/database/migrations
GET    /api/admin/migrations
```

The compatibility alias remains read-only. Development scenario routes exist
only in Deterministic mode with an explicit capability grant.

Contract requirements:

- common success/error envelopes, request IDs, timestamps, status codes, and
  content types remain compatible;
- `/api/map/config` returns versioned allowlisted raster source descriptors,
  attribution, zoom/tile bounds, and opaque capability references rather than a
  MapLibre style URL;
- query/path/body validation returns bounded structured errors;
- daily timetable date, limit, cursor, refresh, completion, and expiry semantics
  remain pinned;
- map box/zoom queries return complete counts with bounded projection;
- no endpoint returns HTML on an application error;
- protected Admin routes preserve operator/demo role policy;
- Database endpoints map only an allowlisted typed Boon/redb structure and
  migration report. They expose no raw redb keys, file paths, arbitrary query,
  mutation, secret, or host handle;
- health is liveness/degraded diagnostics; readiness requires restored redb,
  valid indexes/migrations, listener admission, and required Live config;
- OpenAPI and fixture compatibility are generated/checked by Rust tooling from
  committed contracts. Unchanged operations must accept the pinned fixtures
  exactly; approved delta operations use paired old/new fixtures and an explicit
  mapping report. The production router is still Boon-owned.

## WebSocket Contract Plan

Keep the pinned command/event semantics. Keep protocol version 1 only if every
wire field remains truthful and compatible. If removing a technology-specific
field requires a wire change, make one explicit protocol-version bump, update
both paired artifacts and fixtures, and reject mismatched versions; never change
version 1 in place.

Client commands:

```text
watch_station       unwatch_station
watch_vehicle       unwatch_vehicle
focus_vehicle       unfocus_vehicle
resume_focus        pause_focus
ping
```

Server frames include acknowledgements plus:

```text
station_snapshot
station_snapshot_changed
vehicle_snapshot
vehicle_moved
vehicle_stale
vehicle_lost
source_backoff
rate_limited
telemetry_tick
realtime_degraded
resync_required
pong
error
```

Rules:

- validate protocol version, JSON, message ID, type, and payload in Boon;
- reject malformed/unknown messages without killing the connection;
- issue signed short-lived realtime authorization through the HTTP route;
- enforce exact same-origin/allowed-origin policy at upgrade;
- join textual station/vehicle/focus/Admin rooms only after Boon policy accepts;
- deduplicate shared watches independently of connection rooms;
- cap commands, rooms, frame bytes, outbound queue bytes, and message rate per
  connection;
- resubscribe and send authoritative snapshots after reconnect/restart;
- compact movement events never duplicate full journey geometry/calls;
- metadata-only refresh does not create a semantic-change event;
- stale/duplicate event versions cannot regress Client state.

## Entur And Freshness Plan

The Live Server uses only generic outbound HTTP plus JSON. All request creation,
GraphQL bodies, response mapping, and domain rules are Boon modules.

Required sources:

```text
Stop Place Register  station catalog import
Geocoder v3          place search lane
Journey Planner v3  departures, station calls, journey geometry/calls
Vehicle Positions   current vehicles and movement
```

Preserve pinned behavior:

- send the configured `ET-Client-Name` on every request;
- reserve shared and per-source rolling budget before transport;
- cache the nationwide Vehicle Positions response for the pinned bounded window;
- refresh Focus every three seconds while respecting the 30/minute source
  budget;
- refresh Journey Planner geometry/calls no more than every 30 seconds per
  canonical service journey;
- query station calls six hours before/after, deduplicate journey/date pairs,
  prioritize upcoming journeys, and cap at 200;
- fetch the compact board through Oslo midnight with at most 20 rows;
- build full-day caches by bounded subdivision and boundary deduplication;
- retain successful source authority when an independent source fails;
- use 30 seconds live, through five minutes stale, then lost/unavailable for
  vehicle position freshness;
- never invent movement, journey, transport mode, passenger state, previous
  stop, or source success;
- explicit dead runs and the pinned bounded provider rule remain
  non-passenger; unknown remains unknown;
- 429/timeouts/errors schedule bounded backoff and do not issue an immediate
  hidden duplicate request.

Live external latency is reported separately from Boon/server processing. A
slow provider must not be hidden by changing deterministic performance gates.

## Client Visual And Interaction Plan

The first browser frame is the actual map application, not a landing page.

Required surfaces:

- compact FjordPulse top bar, public/Admin navigation, locale switch, search,
  basemap control, and source attribution;
- full available map viewport with labelled satellite default and labelled
  streets alternate;
- station clusters and direct markers, selected station/vehicle pins, route,
  trail, stops, and focus camera behavior as geographic overlays;
- desktop station/vehicle side panels and mobile peek/half/full bottom sheets;
- station Departures, Vehicles, and Details tabs with independent resource
  states and no misleading aggregate badges;
- vehicle passenger, non-passenger, live, stale, lost, selected, following, and
  paused states;
- deterministic Norwegian Bokmål default and immediate bilingual switching;
- contextual update health without a permanent healthy service matrix;
- read-only Admin status, infrastructure, watches, Entur log, realtime, events,
  database/index, and migration views;
- explicit loading, empty, stale, error, retry, degraded, and device-loss
  states; lack of WebGPU is the generic host's pre-session unsupported state.

Map behavior remains station-first. Initial load must not fetch/display all live
vehicles. Selected transport remains visible above clusters and raster labels.
An overview selection moves to at least zoom 11; a local visible selection keeps
the camera; an off-screen selection pans without zooming out; updates to the
same selection never recenter. Canonical `#map=zoom/latitude/longitude` state is
restored before the first viewport query and replaced, not pushed, during
settled movement.

Responsive gates cover at least 1440x900, 390x844, 320px width, short landscape,
and intermediate widths in both locales. Text may not clip, overlap, or create
unintended horizontal scrolling. Touch targets, keyboard operation, focus,
reduced motion, semantic roles/names/states, and document language remain
testable through the semantic projection.

## Security And Operational Truthfulness

- Live secrets are host-vault/environment references, not Boon values or source.
- Only the public origin-restricted raster provider key may reach the browser.
- Admin operator credentials never appear in client assets, discovery responses,
  logs, state, reports, or redb ordinary authority.
- Public demo Admin credentials are a separate explicitly enabled identity,
  default off, with an exact diagnostic-GET allowlist plus logout.
- Sessions use Secure, HttpOnly, same-site cookies with bounded lifetime and
  signed/verified host primitives.
- CORS and WS origins are exact allowlists. Production is same-origin.
- Forwarded client addresses are accepted only from configured Coolify proxy
  hops; arbitrary forwarding headers are ignored.
- HTTP and WS limits cover bytes, rates, concurrent work, query bounds, rooms,
  and output queues before expensive processing.
- Outbound HTTP is restricted to named Entur capabilities; raster tiles are
  restricted to named browser origins/templates.
- Logs are structured and bounded, with request/event/build correlation and no
  credentials, raw cookies, provider key, secret length/hash, or arbitrary
  payload dump.
- Unknown measurements render as unavailable, never zero. Idle demand is not a
  successful Entur probe. Pending durability is not reported as durable.
- Admin reports the actual single-process/single-redb topology and the explicit
  absence of configured backup, HA, and multi-node capability.

## Implementation Phases

Each phase ends with fresh evidence. Later phases may not compensate for a
failed prerequisite with product-specific code.

### Phase 0: Freeze Reference And Traceability

- Record the exact pinned commit and verify it exists.
- Build a Rust/xtask importer that reads the pinned story manifest, black-box
  scenarios, HTTP OpenAPI/fixtures, realtime schemas/fixtures, design inventory,
  and visual inventory without modifying the reference repository.
- Emit a source-controlled parity manifest keyed by `FP-001` through `FP-108`,
  with each scenario classified as automated semantic, browser visual/input,
  server integration, Live external, deployment, explicitly deferred, or human
  follow-up.
- Emit the compatibility-delta ledger and prove every unchanged fixture remains
  exact while each approved technology-bound delta has paired old/new schemas,
  fixtures, and rationale.
- Mark only backup/restore automation evidence deferred. Do not broadly waive
  FP-089 or FP-092.
- Record reference file digests and reject drift from the pinned commit.

Gate: 108 stories and 340 scenarios are accounted for exactly, with no
unclassified or silently dropped item.

### Phase 1: Complete ProgramRole Server And Bundle Model

- Finish P0's generic role, output, lifecycle, and paired-package contracts.
- Add the generic loopback Server and paired bundle fixtures.
- Add release artifact serialization/loading and role/capability validation.
- Prove restore-before-listen and bounded graceful/forced shutdown.

Gate: all generic role/server fixtures pass with no FjordPulse source present.

### Phase 2: Complete Number And JSON

- Finish one real Number representation end to end.
- Version plan/persistence protocols deliberately; remove transition paths.
- Add bounded JSON data/codec and native/Wasm golden/fuzz coverage.
- Re-run every existing compiler, runtime, persistence, migration, document,
  scenario, and native gate.

Gate: P1 and P2 exit conditions pass; no coordinate/text/scaled-number
workaround is permitted in later phases.

### Phase 3: Complete Generic HTTP/WS And Host Services

- Implement server listener, browser client, outbound transport, rooms,
  correlation, timers, clocks, secrets, cookies, crypto, trusted proxy, and
  diagnostics contracts.
- Add real loopback integration, fault injection, backpressure, and restart.
- Bind host sources/effects through typed plan metadata rather than output-name
  string conventions.

Gate: P3 passes with unrelated programs and no package-specific routing.

### Phase 4: Complete Generic Indexed Query And redb Collection Support

- Add compiler-known Collection/Index/Query plans and pure key projections.
- Extend Session currentness/deltas and redb transactions atomically.
- Add deterministic cursors, planner metrics, retention, rebuild, migration,
  and no-scan rejection.
- Prove the 60,000-row generic fixture and restart behavior.

Gate: P4 and P5 generic data gates pass before importing a FjordPulse catalog.

### Phase 5: Complete Browser WebGPU And MapViewport

- Bring the retained document/scene renderer to Wasm WebGPU.
- Add semantic projection and complete browser input/network/storage adapters.
- Implement the generic retained raster MapViewport and unrelated map example.
- Verify native/browser descriptor parity, tile retention, and hardware release
  performance.

Gate: P6 and P7 pass; a browser WebGPU map is visibly usable without any
FjordPulse code.

### Phase 6: Create Paired FjordPulse Skeleton In Deterministic Mode

- Add package manifest, shared contracts, Client and Server entrypoints,
  fixtures, scenarios, and budgets.
- Serve the WebGPU client from the generic Server host.
- Implement mode/config bootstrap, health/readiness, map config, success/error
  envelopes, WS connect/ping, Norwegian default, responsive shell, and a local
  deterministic raster map.
- Persist one generic Server authority value and prove browser/server restart.

Gate: one end-to-end deterministic browser session reaches Server Boon over
real loopback HTTP/WS, renders through WebGPU, and survives Server restart.

### Phase 7: Catalog, Map, And Search

- Implement resumable station import through Boon-owned Entur mapping.
- Add station collections and exact/prefix/token/typo/spatial indexes.
- Implement complete bounded viewport query and deterministic clustering.
- Add camera URL, basemap, overlay, selected-station, place-label, search,
  keyboard, empty/error, and locale behavior.
- Run the 58,500-row map/search regression and prove query plans.

Gate: FP-001 through FP-014 and the corresponding contract/visual/performance
scenarios pass in Deterministic mode.

### Phase 8: Station And Daily Timetable Workflows

- Implement station watch, independent Journey Planner/Vehicle Positions
  outcomes, compact board, station-serving matches, radial nearby vehicles,
  truthful coverage, and scoped resource states.
- Implement immutable full-day timetable cache, subdivision, deduplication,
  stable relevance anchor, opaque cursor, expiry recovery, and DST cases.
- Implement desktop/mobile tabs and sheets without losing map context.

Gate: FP-015 through FP-025 and station portions of FP-049 through FP-056 pass.

### Phase 9: Vehicle, Journey, Realtime, And Focus

- Implement vehicle mapping, mode, passenger classification, freshness,
  observations, trail, journey/calls/geometry, previous/next/upcoming stops.
- Implement watch/focus scheduling and full WS protocol publication/resync.
- Implement selected/following/paused/stale/lost/non-passenger client behavior.
- Prove one publication path, stale-version rejection, reconnection, and
  periodic HTTP fallback.

Gate: FP-026 through FP-056 pass, including controlled outage and recovery.

### Phase 10: Admin, Security, Migrations, And Maintenance

- Implement operator/demo sessions, exact allowlists, rate limits, CORS/origin,
  trusted proxy, security headers, and structured logs.
- Implement Admin status, infrastructure, watches, Entur log, realtime, events,
  redb collection/index schema, and migration compatibility views.
- Implement redb schema sequence, DRAIN/DRAINING migrations, retention cleanup,
  station import maintenance, and bounded compaction outside hot paths.
- Keep all Admin database/migration controls read-only.

Gate: adapted FP-057 through FP-084 and non-backup portions of FP-092 pass.

### Phase 11: Full Visual, Accessibility, And Browser Parity

- Complete all 27 deterministic scenario routes in Norwegian and English.
- Capture the 74 pinned-equivalent base/expanded responsive states through
  app-owned WebGPU frames and semantic assertions.
- Complete 320px/intermediate/short-landscape overflow checks.
- Verify keyboard-only public and Admin workflows, focus order, labels, state,
  contrast, reduced motion, and semantic document language.
- Compare against pinned design/production references for information hierarchy,
  state visibility, and domain truthfulness. Pixel identity with MapLibre is not
  required; missing behavior or content is not acceptable.

Gate: FP-071 through FP-077 and FP-098 through FP-100 pass with no HTML/CSS
product renderer.

### Phase 12: Coolify Production Deployment

- Build one multi-stage image containing the generic Server binary, trusted
  precompiled Server artifact, browser Wasm/WebGPU bundle, Client artifact, and
  static assets.
- Run as a non-root user with read-only root filesystem and one writable mounted
  data path.
- Create a dedicated Coolify resource and persistent volume for
  `/var/lib/boon/fjordpulse`.
- Expose only container port 8080 to Coolify's managed network; do not add a
  custom public network or database port.
- Configure exact host `fjordpulse-boon.kavik.cz`, HTTPS redirect, TLS, WSS,
  health, readiness, graceful-stop timeout, one replica, and no overlapping
  rollout.
- Configure required Live values and secrets in Coolify, not the image/repo.
- Add Netlify DNS for the new host without replacing the pinned production host.
- Gate deployment on all deterministic, contract, browser, persistence,
  security, architecture, and image smoke reports for the exact source SHA.
- Label image and Admin build identity with source SHA, artifact digests,
  compiler version, schema hash, and protocol version.

Gate: HTTPS/WSS and production smoke pass on the new domain, and Live mode is
visibly using Entur and the configured raster provider.

### Phase 13: Restart, Redeploy, Live, And Final Parity

- Seed/observe the production persistence sentinel set.
- Restart the process and container; verify exact restored state.
- Redeploy a schema-compatible new image through Coolify; verify the same
  volume/namespace, migration, indexes, state, and resumed watches.
- Exercise a source-controlled schema migration in staging and then production
  only after fault/restart evidence passes.
- Run opt-in Live Entur smoke for all four source groups.
- Run public app, map, search `førde`, station, timetable, vehicle, Focus,
  reconnect, Admin, health, readiness, HTTPS, WSS, and build-version smoke.
- Run final parity accounting and genericity/no-Python audits.

Gate: every non-deferred scenario is passing from fresh artifacts tied to the
deployed SHA, with no tracked edit after report generation.

## Parity Gates

| Pinned range | Rewrite acceptance |
| --- | --- |
| FP-001..008 | public shell, Norwegian default, locale persistence, retained raster Norway map, station-first behavior, clusters, smooth camera, selected marker, contextual health, contained errors |
| FP-009..014 | search open/close, Norwegian normalization, prefix/one-edit results, empty state, keyboard navigation, station/line/vehicle selection behavior |
| FP-015..025 | station panel/watch, loading/fresh/empty/stale/error/retry, compact departures, daily timetable, serving/nearby grouping, coverage, scoped tabs, refresh |
| FP-026..039 | vehicle details/trail, Focus start/follow/pause/resume/stop, stale/lost/retry, transport mode, passenger/non-passenger truthfulness |
| FP-040..048 | browser WS, backend-only Entur, typed protocol, watches/rooms, reconnect/resync, degraded periodic refresh |
| FP-049..056 | Entur identity, departures/positions/journeys, focused refresh, shared limits, 429/backoff, freshness distinctions, no fabricated data |
| FP-057..063 | redb production authority, atomic migrations, typed collections/indexes, station/vehicle/observation/event persistence and retention |
| FP-064..070 | Admin auth, status/infrastructure, watches, Entur log, realtime, health/readiness, structured logs, read-only redb diagnostics |
| FP-071..077 | all desktop/mobile states and design-system components rendered by Client Boon through WebGPU |
| FP-078..084 | secrets, request/frame validation, rate limits, Admin protection, data minimization, CORS/origins/trusted proxy |
| FP-085..092 | Coolify/new domain/single Boon server/redb/env/compatible rollback/cleanup-import maintenance; backup/restore automation explicitly deferred |
| FP-093..102 | static/genericity checks, unit/contract/integration/resilience/visual/accessibility/performance/production smoke |
| FP-103..108 | architecture, development, deployment, Entur, UI state, and readiness documentation updated to the actual Boon stack |

Every row must link to executable report IDs. A range cannot pass because one
representative happy path passed.

## Verification Strategy

### Generic Platform Evidence

- role/bundle plan verification and unrelated Server/Client fixtures;
- Number native/Wasm/persistence/JSON/index golden tests;
- bounded JSON fuzz/property and fixture tests;
- loopback HTTP/WS/client/outbound transport tests;
- 60,000-row indexed query planner/no-scan/restart fixture;
- generic MapViewport native/browser visual/input/cache/failure tests;
- generic browser WebGPU document/input/accessibility/device-loss tests;
- no package-identity branches in production generic crates.

### FjordPulse Semantic And Contract Evidence

- deterministic pure tests for mapping, normalization, rank, typo residual,
  clustering, distance, freshness, passenger classification, progress,
  departure grouping, cursor, rate budget, scheduler, versions, and locale;
- exact validation of every unchanged pinned HTTP/realtime fixture, plus paired
  old/new mapping evidence for every approved compatibility delta;
- black-box HTTP/WS process tests against the actual Server artifact;
- fault injection for Entur source independence, 429, timeout, malformed JSON,
  redb backpressure, WS interruption, tile failure, and recovery;
- restart and migration fault matrix at every durable boundary;
- source-controlled parity trace from all 340 scenarios to evidence.

### Browser Evidence

- Playwright or an equivalent browser driver may orchestrate the browser; it is
  not a product renderer and contains no application policy;
- assertions use public HTTP/WS behavior, semantic accessibility projection,
  app-owned frame/readback, canvas pixels, and generic runtime reports;
- scenario behavior is manifest/data-driven. The verifier may not contain
  `if fjordpulse`, route-specific drawing, expected station coordinates, or
  fixture-text inference;
- raster fixture tiles are served by a generic declared test asset server;
- hardware WebGPU release runs record adapter/backend and reject software;
- public production smoke never accesses Entur or redb directly.

### Architecture Audits

Scan parser, typechecker, IR, plan, compiler, executor, runtime, persistence,
document, renderer, native/browser/server hosts, and verifiers for:

- `fjordpulse`, package IDs, source paths, station/vehicle IDs, route names, and
  example labels in production control flow;
- app-specific HTTP routes, JSON fields, query plans, map layers, geometry,
  pixels, or verifier expectations;
- duplicate Number, JSON, runtime, renderer, or persistence paths;
- request-time full scans for declared indexed operations;
- direct redb calls outside generic persistence/query drivers;
- direct Entur/MapTiler code outside Boon app modules and named host capability
  configuration;
- PHP, CakePHP, AMPHP, SolidJS, MapLibre, SurrealDB, Python, LocalStorage, or
  handwritten app JavaScript in the new production stack;
- secrets or sensitive payloads in source, state, logs, fixtures, reports, or
  browser assets.

Test fixtures may contain FjordPulse names where the test is explicitly testing
the app. Generic production code may not.

## Performance Gates

Budgets are measured in release mode after warmup. Reports identify hardware,
OS/browser, WebGPU adapter, source/artifact hashes, dataset digest, sample count,
and percentile method. External provider/network time is recorded separately
from host and Boon processing.

### Browser Interaction

| Metric | Required budget |
| --- | --- |
| pointer/wheel/keyboard input to presented frame | p95 <= 16.7 ms, p99 <= 25 ms |
| cached-tile pan/zoom frame interval during scripted interaction | p95 <= 16.7 ms, p99 <= 25 ms |
| search keystroke to visible local pending/result state | p95 <= 16.7 ms, p99 <= 25 ms |
| WS frame receipt to visible retained update on deterministic loopback | p95 <= 33.4 ms, p99 <= 50 ms |
| mobile sheet snap/focus/layout frame | p95 <= 16.7 ms, p99 <= 25 ms |
| decoded raster tile ready to presented GPU tile | p95 <= 33.4 ms, p99 <= 50 ms |
| deterministic first usable shell on reference release host | <= 1,000 ms |
| deterministic first complete fixture map on reference release host | <= 2,000 ms |

Additional browser requirements:

- no task blocks the interactive thread for 50 ms or longer in the five-minute
  map/search/station/Focus scenario;
- no normal update rebuilds the Client Session, full document, full overlay
  list, or all tile textures;
- resident decoded plus GPU tile caches remain within declared budget (initial
  target: at most 256 tiles and 128 MiB) and evict deterministically;
- dense map responses remain at most 2,000 items and direct station markers at
  most 300;
- a 30-minute Focus run has bounded sockets/tasks/tiles and no sustained heap or
  GPU-memory growth after warmup beyond 10% of the stabilized baseline.

Cold public raster/Entur latency cannot be made deterministic. The Client must
remain responsive and retain prior tiles/data while those requests are pending.

### Server And Persistence

| Metric | Required budget on the deterministic reference dataset |
| --- | --- |
| exact/prefix indexed query | p95 <= 25 ms, p99 <= 50 ms |
| 58,500-station bbox plus complete bounded clustering | p95 <= 50 ms, p99 <= 100 ms |
| deterministic cached public HTTP endpoint end to end | p95 <= 75 ms, p99 <= 150 ms |
| valid WS command to acknowledgement | p95 <= 50 ms, p99 <= 100 ms |
| committed canonical update to queued WS publication | p95 <= 100 ms, p99 <= 250 ms |
| redb Immediate checkpoint for bounded ordinary turn | p95 <= 25 ms, p99 <= 75 ms |
| production-shaped restore plus index validation for 58,500 stations | <= 10 s before readiness |

Additional server requirements:

- query reports prove index selection and bounded examined rows; elapsed time
  alone is insufficient;
- no request, WS turn, or scheduler tick loads/encodes a full catalog or full
  application snapshot;
- no external network wait runs on the deterministic Session turn or persistence
  worker thread;
- source requests and results are bounded; station match candidates remain at
  most 200, compact departures 20, timetable pages 50, journey calls 1,000, and
  map response items 2,000;
- persistence/query/report work uses bounded queues and visible backpressure;
- one production-shaped 30-minute Focus soak keeps memory, task count, redb file
  growth, event/log retention, and socket queues within declared bounds;
- cleanup, compaction, import, and index rebuild do not violate browser/server
  interaction budgets.

Budget values may be tightened after a measured platform baseline. They may not
be loosened merely to accommodate an app workaround or a repeated architectural
failure. A repeated failure in the same class triggers architecture review of
query ownership, scheduling, retained state, tile lifecycle, currentness, and
effect/persistence boundaries.

## Coolify Deployment Contract

### Build Artifact

The image build is reproducible from one source SHA and pinned Rust/Cargo lock
state. It performs no network-time code generation in the runtime stage. The
runtime image contains:

- generic Boon server host binary;
- trusted compiled Server plan/artifact;
- static generic browser host Wasm/loader;
- trusted compiled Client plan/artifact;
- fonts, brand assets, locale assets, and package manifest;
- source-controlled migration catalog;
- no compiler toolchain, package manager, shell requirement, Python, PHP, Node
  runtime, or database server in the final image.

### Required Configuration

Document and validate at least:

```text
BOON_RUN_MODE=live
BOON_PUBLIC_ORIGIN=https://fjordpulse-boon.kavik.cz
BOON_DATA_DIR=/var/lib/boon/fjordpulse
BOON_STATE_NAMESPACE=production-v1
ENTUR_CLIENT_NAME=...
MAP_TILE_PROVIDER=maptiler
MAP_TILE_SATELLITE_TEMPLATE=...
MAP_TILE_STREETS_TEMPLATE=...
MAP_TILE_PUBLIC_KEY=...
SESSION_SIGNING_SECRET_REF=...
ADMIN_OPERATOR_ID=...
ADMIN_OPERATOR_SECRET_REF=...
ADMIN_DEMO_ACCESS=false
TRUSTED_PROXY_CIDRS=...
RUST_LOG=...
```

Names may be normalized by the generic host configuration design, but every
value has a typed description, required/optional status, redaction policy, and
startup validation. Production fails if `Deterministic` mode or scenario
capabilities are requested.

### Health And Lifecycle

- `/api/health` proves process/event-loop liveness and reports degraded
  dependencies without blocking on slow external probes.
- `/api/readiness` is false until config, redb lock/restore/migration/indexes,
  Server artifact, and listener admission are ready.
- Coolify health checks readiness for rollout and health for ongoing liveness.
- SIGTERM stops admission, closes/announces WS shutdown, flushes bounded durable
  work, and exits within the declared grace period.
- single-writer lock conflict is a startup failure and readiness never passes.
- container restart policy may restart the single process; it may not launch a
  second replica against the same volume.

### Deployment Proof

1. Deploy to a staging hostname/volume and run all production-image smokes.
2. Create authoritative sentinel data through normal Boon workflows.
3. Restart the process and container; compare sentinel digests.
4. Redeploy an exact new image with the same volume; compare digests and schema.
5. Exercise one compatible migration and forced failure before activation.
6. Configure DNS/TLS for `fjordpulse-boon.kavik.cz`.
7. Deploy the exact gated SHA and verify public HTTPS, HTTP redirect, WSS,
   headers, map, search, station, vehicle, Focus, Admin, health, and readiness.
8. Confirm the original `fjordpulse.kavik.cz` deployment remains independent.

The runbook must say plainly that the persistent volume protects ordinary
restart/redeploy continuity but is not a backup and does not protect against
host/disk loss or operator deletion.

## Defect Handling Rule

When FjordPulse source exposes a missing or broken Boon capability:

1. Stop adding product code around the failure.
2. Reduce it to the smallest app-independent Boon source and host input.
3. Identify the owning parser, typechecker, IR, plan, executor, runtime,
   persistence, document, renderer, browser/server host, or verifier contract.
4. Fix that generic layer with positive, negative, deterministic, and where
   relevant native/Wasm/restart coverage.
5. Re-run affected existing examples and architecture gates.
6. Remove any diagnostic workaround before resuming FjordPulse.

Forbidden final patterns include:

- scaled-integer or Text coordinates because Number is incomplete;
- string-splitting JSON;
- precomputed fixture responses substituted for runtime mapping;
- `List/filter` catalog scans hidden behind small test data;
- hardcoded station/vehicle pixels in renderer or verifier;
- duplicated Client state because currentness is wrong;
- Server routes implemented in Rust because output/source plumbing is missing;
- DOM/CSS panels layered over a deficient WebGPU document renderer;
- app-name branches, source-text detection, route-name dispatch, or geometry
  inference in generic code;
- weakening budgets, schemas, negative tests, or evidence requirements to make
  a report pass.

## Clear End Condition

The full-stack rewrite is complete only when all of the following are true on
one final source revision:

- the exact pinned parity manifest accounts for all 108 stories and 340
  scenarios, with only the explicitly named backup/restore automation items
  deferred;
- one production package builds a Client Boon artifact and Server Boon artifact
  with shared pure contracts and explicit compatible protocol versions;
- generic independent gates pass for ProgramRole Server, paired bundles, real
  Number, JSON, HTTP/WS, indexed queries, redb collections, retained raster
  MapViewport, and browser WebGPU;
- every non-deferred semantic, HTTP, WS, visual, responsive, accessibility,
  resilience, security, persistence, migration, and performance gate passes
  from fresh source-bound artifacts;
- deterministic mode is fully reproducible and performs no public network
  access; Live mode is explicit, uses real Entur/raster services, and never
  falls back to fixture data;
- the 58,500-station query fixture proves complete bounded map results and
  indexed search without request-time catalog scans;
- process/container restart and a normal Coolify redeploy preserve all
  acknowledged redb sentinel state, restore before readiness, and resume
  watches without duplicate effects;
- `https://fjordpulse-boon.kavik.cz` serves the WebGPU Client, all required HTTP
  routes, WSS realtime, Admin diagnostics, and the exact deployed build identity;
- production smoke passes for map, `førde` search, station, timetable, vehicle,
  Focus, reconnect, Admin, health, readiness, HTTPS, and WSS;
- architecture scans find no FjordPulse-specific branch in generic production
  code, no app logic in Rust/JS/TS, no direct browser Entur/redb access, no
  duplicate renderer/runtime, no Boon workaround for an engine defect, and no
  Python source or invocation;
- deployment and Admin truthfully state that backup/restore automation, HA,
  clustering, and multi-node operation are not implemented;
- final reports are generated after the last tracked edit and bind source SHA,
  Client/Server artifact digests, schema/index hashes, image digest, browser
  WebGPU adapter, dataset digest, and deployment URL.

Documentation, a static UI, a deterministic-only demo, a Server role enum, a
redb proof of concept, a map screenshot, partial story coverage, stale reports,
or a successful deployment without restart/redeploy proof is not completion.

## References

Current Boon contracts:

- `docs/architecture/NATIVE_GPU_PIPELINE.md`
- `docs/architecture/RUNTIME_MODEL.md`
- `docs/architecture/DELTA_PROTOCOL.md`
- `docs/architecture/LANGUAGE_SEMANTICS.md`
- `docs/plans/BOON_PERSISTENCE_ARCHITECTURE_PLAN.md`
- `docs/plans/TYPE_INFERENCE_AND_TYPECHECKING_PLAN.md`
- `docs/plans/PERSONS_PRO_LOCAL_FIRST_IMPLEMENTATION_PLAN.md`
- `examples/server_outputs.bn`

Pinned FjordPulse reference at
`dd6e750c2ca9dec3041f66ceda31d30379d4027a`:

- `README.md`
- `PROGRESS.md`
- `FINAL_READINESS_REVIEW.md`
- `docs/ARCHITECTURE.md`
- `docs/03_api_contract.md`
- `docs/04_realtime_protocol.md`
- `docs/05_testing_strategy.md`
- `docs/PRODUCTION_DEPLOYMENT_PLAN.md`
- `docs/user-stories/`
- `docs/design/`
- `contracts/http/openapi.yaml`
- `contracts/realtime/`
- `contracts/fixtures/`
