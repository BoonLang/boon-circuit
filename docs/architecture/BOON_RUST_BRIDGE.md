# Boon-Rust Bridge

Status: architecture note

This document defines how Boon programs should use Rust crates without weakening
the Boon data model. It is not a playground-specific mechanism. The same shape
must work for a Boon application repository created outside `boon-circuit`.

## Core Rule

Boon-visible bridge values are pure data. They are serializable, comparable, and
schema-described.

Rust may own resources. Boon owns descriptions, requests, results, diagnostics,
facts, and bounded pages.

The bridge is therefore not an object FFI. It is a typed data membrane:

```text
Boon data
  -> canonical bridge request
  -> Rust adapter and chosen Rust crate
  -> canonical bridge result or completion event
  -> normal Boon runtime tick
```

No Rust pointer, native handle, `Arc<Mutex<_>>`, file descriptor, socket, database
pool, GPU texture, parser tree, tensor allocation, or crate-specific object is a
Boon value. Boon equality never depends on Rust object identity, process-local
addresses, cache hits, task ids, or loaded sessions.

This follows the existing language contract:

- Boon values compare structurally.
- Runtime/source/list identities stay below the language boundary.
- `SOURCE` is the host input boundary.
- `HOLD` commits only at the runtime commit phase.
- Semantic deltas are canonical; render, network, persistence, and native deltas
  are lowerings.

## User Project Shape

A Boon programmer should not edit the playground or fork `boon-circuit` just to
use a Rust crate. A normal app repository should be enough:

```text
my_wave_app/
  Boon.toml
  Cargo.toml
  Cargo.lock
  src/
    App.bn
    WaveformView.bn
  bridge/
    Cargo.toml
    src/lib.rs
    src/schemas.rs
  rust/
    my_domain_lib/
      Cargo.toml
```

`Boon.toml` describes the Boon package and the bridge package:

```toml
[package]
name = "my_wave_app"
entry = "src/App.bn"

[source]
files = [
  "src/App.bn",
  "src/WaveformView.bn",
]

[rust_bridge]
mode = "static"
package = "my_wave_app_bridge"
path = "bridge"
```

The root `Cargo.toml` is an ordinary Cargo workspace:

```toml
[workspace]
resolver = "2"
members = [
  "bridge",
  "rust/my_domain_lib",
]
```

The generated runner is not a starter workspace member. `boon build` owns
`.boon/generated-runner` and invokes Cargo with that generated manifest directly.
The `.boon/` directory is build output and should normally be ignored, while
`Boon.toml`, `Cargo.toml`, `Cargo.lock`, Boon source files, and bridge source
files are project source.

Because `.boon/generated-runner` lives below the app directory but is not a
member of the app workspace, its generated `Cargo.toml` must opt out of the
parent workspace with an empty `[workspace]` table. The alternative is for
`boon init` to add `.boon/generated-runner` to the root workspace `exclude` list,
but the empty generated workspace is the default because it keeps `.boon/`
self-contained and ignored.

The bridge crate owns the user's chosen Rust dependencies:

```toml
[package]
name = "my_wave_app_bridge"
version = "0.1.0"
edition = "2024"

[dependencies]
boon_bridge = "=0.1.0"
wellen = "0.24"
rayon = "1"
```

If the programmer also writes a custom Rust library for the app, it is still a
normal Cargo dependency of the bridge crate:

```toml
[dependencies]
my_domain_lib = { path = "../rust/my_domain_lib" }
```

Cargo remains the authority for Rust dependency resolution. A bridge crate may
use crates.io dependencies, git dependencies, path dependencies, features,
target-specific dependencies, and build scripts in the standard Cargo way. Boon
does not invent a second dependency solver for Rust.

## Boon SDK Crates

The installed Boon toolchain supplies the SDK crates used by external projects:

- `boon_bridge` is the small public crate a user's bridge depends on.
- `boon_runtime` is used by the generated runner, not normally by the bridge
  crate directly.
- host target crates are selected by `boon build` for the requested target.

`boon init` should write the bridge dependency using the exact SDK version that
matches the installed CLI:

```toml
[dependencies]
boon_bridge = "=0.1.0"
```

`boon check-bridge` must reject a bridge crate compiled against a different
`boon_bridge` ABI or canonical schema version than the CLI/runtime will use. The
generated runner must use the same SDK version and must record the Boon SDK
version, bridge schema hashes, bridge crate package id, enabled Cargo features,
target triple, and `Cargo.lock` digest in build metadata.

During `boon-circuit` development, path overrides can point these crates at a
local checkout. That is a development override only; the app-level contract is
still a Boon SDK version plus normal Cargo dependency resolution.

## What Boon Imports

Boon source imports bridge modules, not raw Rust crates:

```boon
IMPORT wellen.v1 AS Wave

wave_file: File/ref(path: input.path)

wave_open: Wave/open(file: wave_file, options: [
    hierarchy: True
])
```

`wellen.v1` is a bridge contract exported by `my_wave_app_bridge`. It is not the
Rust crate name `wellen`, and it is not permission for Boon code to hold a
`wellen::Waveform`.

The bridge contract defines:

```text
module name: wellen.v1
provider crate: wellen
provider version: 0.24.x
exports:
  open: effect(OpenWaveformRequest) -> WaveformOpened
  hierarchy_page: effect(HierarchyPageRequest) -> HierarchyPage
  signal_page: effect(WavePageRequest) -> WavePage
schemas:
  OpenWaveformRequest
  WaveformOpened
  WaveformRef
  HierarchyPageRequest
  HierarchyPage
  SignalSetRef
  WavePageRequest
  WavePage
  WavePageRef
  BridgeDiagnostic
```

In this document, names ending in `Ref` mean canonical descriptor values such as
digest, schema, provider version, locator, and page spec. They never mean opaque
Rust handles, resource-table keys, allocation ids, or process-local session ids.

The Boon compiler validates imports against the registered module names, export
kinds, schema versions, and schema hashes. A missing module, changed schema, or
wrong effect kind is a build error.

## Static Bridge Mode By Default

The v1 bridge mode should be static bridge mode: compile-time registration of
the bridge crate into the generated runner.

`boon build` should generate or update an app runner package under a generated
directory such as `.boon/generated-runner`. That runner depends on:

- `boon_runtime`;
- `boon_bridge`;
- host target crates needed by the selected platform;
- the user's bridge crate by Cargo path, package name, or workspace dependency.

Its generated manifest starts with:

```toml
[workspace]

[package]
name = "my_wave_app_runner"
version = "0.0.0"
edition = "2024"
publish = false
```

The generated runner imports the bridge crate directly:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut registry = boon_bridge::BridgeRegistry::new();
    my_wave_app_bridge::register(&mut registry)?;

    let package = boon_runtime::load_package("Boon.toml")?;
    boon_runtime::validate_bridge_imports(&package, &registry)?;
    boon_runtime::run_package(package, registry)
}
```

This gives Rust programmers a normal project workflow:

```bash
boon check-bridge
boon check
boon build
boon run
```

It also gives Boon a stable runtime contract:

- the app binary contains the bridge code it was compiled against;
- the Cargo lockfile records Rust dependency resolution;
- the bridge registry is known before the Boon runtime starts;
- bridge schemas can be included in the Boon program hash;
- deployment can ship one app binary plus app assets.

This does not require a fully static native executable. Cargo still owns native
link directives from build scripts, target-specific dependencies, crate types,
system libraries, and shared-library packaging required by the selected Rust
crates.

Dynamic `cdylib` loading is a future mode, not the default. It can be useful for
development reload, but it must add ABI validation, schema hashes, platform
library handling, and stricter safety rules. It still must not expose native
handles as Boon values.

## Bridge Registry

The bridge crate exposes one ordinary Rust registration function:

```rust
pub fn register(registry: &mut boon_bridge::BridgeRegistry) -> boon_bridge::Result<()> {
    registry.module("wellen.v1")
        .provider("wellen")
        .provider_version(schemas::WELLEN_PROVIDER_VERSION)
        .bridge_crate("my_wave_app_bridge")
        .bridge_crate_version(env!("CARGO_PKG_VERSION"))
        .effect("open", schemas::OPEN_WAVEFORM_REQUEST, schemas::WAVEFORM_OPENED, open_waveform)
        .effect("hierarchy_page", schemas::HIERARCHY_PAGE_REQUEST, schemas::HIERARCHY_PAGE, hierarchy_page)
        .effect("signal_page", schemas::WAVE_PAGE_REQUEST, schemas::WAVE_PAGE, signal_page)
        .finish()
}
```

This API shape is illustrative. The important contract is:

- exported names are stable strings in the bridge module namespace;
- inputs and outputs have canonical schemas and schema hashes;
- each export declares `pure` or `effect`;
- provider name, provider version, bridge crate version, and feature set are
  recorded in diagnostics/build metadata;
- runtime host state lives behind the registry, not inside Boon values.

The registry may keep a host-side resource table for loaded files, database
pools, parser instances, GPU devices, worker pools, and caches. That table is
derived execution state. Entries are addressed only by canonical request or
descriptor data and must not be emitted as Boon-visible ids, equality keys,
ordering keys, freshness markers, or semantic diagnostics.

## Bridge Crate Adapter Shape

A bridge crate should be small. It maps between canonical Boon bridge data and
the chosen Rust crates:

```text
bridge/src/lib.rs       registry and handler functions
bridge/src/schemas.rs   canonical schema ids and generated schema values
bridge/src/wellen.rs    optional adapter module for one provider
```

Example skeleton:

```rust
mod schemas;
mod wellen_adapter;

use boon_bridge::{
    BridgeContext, BridgeRegistry, BridgeResult, BridgeTask, BridgeValue,
};

pub fn register(registry: &mut BridgeRegistry) -> BridgeResult<()> {
    registry
        .module("my_domain.v1")
        .provider("my_domain_lib")
        .provider_version(my_domain_lib::VERSION)
        .bridge_crate("my_wave_app_bridge")
        .bridge_crate_version(env!("CARGO_PKG_VERSION"))
        .pure(
            "normalize_signal_name",
            schemas::NORMALIZE_SIGNAL_NAME_REQUEST,
            schemas::NORMALIZE_SIGNAL_NAME_RESULT,
            normalize_signal_name,
        )
        .effect(
            "open_waveform",
            schemas::OPEN_WAVEFORM_REQUEST,
            schemas::WAVEFORM_OPENED,
            open_waveform,
        )
        .finish()
}

fn normalize_signal_name(input: BridgeValue) -> BridgeResult<BridgeValue> {
    let request = schemas::NormalizeSignalNameRequest::decode(input)?;
    let normalized = my_domain_lib::normalize_signal_name(&request.name);
    schemas::NormalizeSignalNameResult { normalized }.encode()
}

fn open_waveform(ctx: BridgeContext, task: BridgeTask) -> BridgeResult<()> {
    let request = schemas::OpenWaveformRequest::decode(task.input)?;
    ctx.spawn_blocking(task.request_id, move || {
        let opened = wellen_adapter::open_waveform(request)?;
        schemas::WaveformOpened::from_opened(opened).encode()
    })
}
```

The skeleton is intentionally explicit about the adapter boundary:

- `my_domain_lib` and Wellen types are decoded/used inside Rust handlers only;
- handler outputs are encoded canonical bridge values;
- `spawn_blocking` receives a canonical task id for completion routing, not a
  Boon-visible native task handle;
- the effect handler returns completion data through the bridge executor, not by
  mutating a Boon `HOLD`.

Schema definitions may be handwritten first and generated later. Either way,
they must produce stable schema ids and canonical encode/decode behavior:

```rust
pub const NORMALIZE_SIGNAL_NAME_REQUEST: SchemaId =
    SchemaId::new("my_domain.v1.NormalizeSignalNameRequest", "sha256:...");

pub const OPEN_WAVEFORM_REQUEST: SchemaId =
    SchemaId::new("wellen.v1.OpenWaveformRequest", "sha256:...");
```

## Resource Authority

The bridge crate is trusted Rust code compiled into the app binary. It is not a
memory sandbox for arbitrary untrusted code. Resource access still needs an
explicit app/host policy because effect exports may touch files, networks,
databases, GPUs, clocks, or operating-system services.

`Boon.toml` should declare the app grants:

```toml
[rust_bridge.capabilities.filesystem.wave_traces]
access = ["read"]
roots = ["./traces", "/mnt/lab/waves"]

[rust_bridge.capabilities.network.wave_metadata]
access = ["https"]
hosts = ["metadata.internal.example"]

[rust_bridge.capabilities.time]
access = "deny"
```

Bridge exports declare what they require:

```rust
registry
    .module("wellen.v1")
    .effect("open", schemas::OPEN_WAVEFORM_REQUEST, schemas::WAVEFORM_OPENED, open_waveform)
    .requires_capability("filesystem", "wave_traces")
    .finish()
```

The compiler and runtime responsibilities are:

- `boon check` validates that imported effect exports have matching declared
  grants;
- `boon check-bridge` reports every export capability requirement;
- the host executor denies missing grants before scheduling Rust work;
- denied work produces a canonical diagnostic completion;
- OS-level sandboxing may be added by a host, but the Boon bridge contract cannot
  rely on OS sandboxing being present.

## Pure Exports

A pure export is a deterministic Rust function over canonical bridge data:

```text
BridgeValue -> BridgeValue
```

It may run during a Boon tick because repeated calls with equal inputs must
produce equal outputs. It must not read the clock, random state, environment,
filesystem, network, process globals, mutable caches with observable behavior, or
thread race outcomes.

Good pure exports:

- parse a small string into a canonical AST summary;
- normalize a path-like text value without touching the filesystem;
- validate a record against a schema;
- compute a hash over bounded input bytes;
- map a small domain object to another data object.

Bad pure exports:

- open a waveform file;
- query a database;
- perform HTTP;
- ask a parser for an incremental tree stored from a previous call;
- run GPU inference;
- use system time;
- read a global cache and return different diagnostics depending on cache state.

Pure bridge failures become canonical diagnostics, not Rust panics escaping into
the Boon runtime.

Concrete pure example:

```boon
IMPORT semver.v1 AS Semver

parsed_version: Semver/parse(text: release.version_text)
is_supported: Semver/satisfies(
    version: parsed_version,
    requirement: TEXT { >=1.2.0 <2.0.0 }
)
```

The bridge crate can wrap the Rust `semver` crate, but only the parsed version
record, comparison result, and diagnostics cross into Boon:

```rust
registry
    .module("semver.v1")
    .provider("semver")
    .provider_version(schemas::SEMVER_PROVIDER_VERSION)
    .pure("parse", schemas::SEMVER_PARSE_REQUEST, schemas::SEMVER_PARSE_RESULT, parse_semver)
    .pure("satisfies", schemas::SEMVER_SATISFIES_REQUEST, schemas::SEMVER_SATISFIES_RESULT, satisfies)
    .finish()
```

## Effect Exports

An effect export starts host work. It does not commit Boon state directly.
The compiler should reject effect exports in continuously recomputed pure
positions unless the request is explicitly event-gated or otherwise deduplicated
by canonical request identity. A re-render or repeated dependency evaluation must
not accidentally start the same filesystem, network, GPU, or parser task again.

```text
Boon tick emits request data
  -> host bridge executor schedules Rust work
  -> work completes, fails, times out, or is canceled
  -> completion enters the runtime as SOURCE data
  -> HOLD/LATEST handle it normally
```

The request and completion are pure data:

```text
BridgeTaskRequest {
  module,
  export,
  request_id,
  request_key,
  request_epoch,
  input_schema_hash,
  input_digest,
  input,
  budget,
  cancellation_key,
  cancellation_epoch
}

BridgeTaskCompletion {
  request_id,
  request_key,
  request_epoch,
  status,
  output_schema_hash,
  input_digest,
  cancellation_epoch,
  output,
  diagnostics
}
```

`request_id` is a canonical correlation value chosen from Boon/domain data, or a
runtime-private routing key kept below the language boundary. If it is
Boon-visible, it must not be a host task id, pointer, allocation id, session id,
process-local counter, or permission token. It does not grant access to a Rust
future, worker, file handle, stream, or native resource.

Effect scheduling rules:

- `request_key` is the canonical hash of module, export, input schema hash,
  canonical input bytes, relevant capability grant ids, and effect options.
- Repeating the same live `request_key` deduplicates to the existing host task
  instead of starting duplicate Rust work.
- A completion is accepted only if its request key, request epoch, input digest,
  schema hashes, and cancellation epoch still match the latest live request.
- If a request is canceled or times out, the runtime may receive one canonical
  canceled/timeout completion. A later success from the old Rust task is stale
  and must be dropped or recorded only as host diagnostics.
- Out-of-order completions from older request epochs must not mutate current
  Boon state.
- Schema-drifted completions are rejected before they become `SOURCE` data.
- Replay must use recorded completion data. It must not re-run arbitrary
  filesystem, network, clock, or GPU work unless the replay host explicitly
  chooses a new live execution mode.

Effect exports are required for:

- filesystem access;
- network access;
- database queries and transactions;
- async tasks;
- multithreaded parsing where completion order matters;
- GPU and accelerator work;
- large file decoding;
- streams and subscriptions;
- time, randomness, and operating-system resources.

## Canonical Data Types

The bridge ABI should use a narrower public data set than any current internal
runtime value enum:

```text
Null
Bool
Int
Decimal
Text
Bytes
List
Record with sorted keys
Tagged enum
Result
Diagnostic
BlobRef
ArtifactRef
PageRef
```

Canonical encoding rules:

- sorted record keys;
- stable enum tags;
- explicit schema versions;
- explicit byte length and digest for external bytes;
- no unordered map equality;
- no pointer or reference identity;
- no `NaN`, infinity, or `-0.0` ambiguity in v1 bridge numbers;
- no process-local object ids in equality.

The v1 bridge ABI should use decimal or integer values for comparable numeric
data. Floating-point values may be added later only behind an explicit schema
annotation and golden vectors that define finite-only encoding, `-0.0`
normalization, `NaN` rejection or canonicalization, infinity handling, ordering,
and equality across processes.

Large bytes should not be copied through ordinary Boon state by default. They
should use bounded inline `Bytes` only below a configured limit. Larger payloads
use `BlobRef`:

```text
BlobRef {
  digest: "sha256:...",
  byte_len: 104857600,
  media_type: "application/vnd.boon.wave-page",
  storage: "bridge-cache",
  encoding: "arrow-ipc"
}
```

The blob may live in a Rust-owned cache, mmap, database, object store, or GPU
download buffer, but Boon equality compares the descriptor data, not the backing
allocation.

## Artifact And Page References

External artifacts are represented as canonical descriptors:

```text
ArtifactRef {
  kind: "wellen.waveform",
  provider: "wellen",
  provider_version: "0.24.2",
  bridge_module: "wellen.v1",
  contract_version: "waveform.v1",
  identity: "sha256:...",
  locator: {
    kind: "file",
    path: "/data/cpu_trace.vcd",
    size: 9876543210,
    modified_ns: 1780000000000000000
  },
  reproducibility: "content-addressed"
}
```

Path-only data can be a request input, but it is not enough for a stable artifact
identity because the file contents may change. A successful result must carry a
strong content identity, snapshot identity, or explicit weak-fingerprint marker
with degraded reproducibility.

Pages are bounded windows over an artifact:

```text
PageRef {
  source: ArtifactRef(...),
  page_kind: "wave.signal.samples",
  page_spec: {
    signal_ids: ["top.cpu.clk", "top.cpu.state"],
    time_start: 100000,
    time_end: 200000,
    encoding: "packed-vcd-page"
  },
  digest: "sha256:...",
  byte_len: 2097152
}
```

Two pages compare equal when the source identity, page spec, digest, and byte
length compare equal. They do not compare by whether the same Rust cache entry is
currently loaded.

## Wellen Example

Wellen stresses the bridge because waveform files can be gigabytes, VCD parsing
can use multiple threads, and a viewer normally accesses selected signals and
time windows rather than the whole trace.

Boon-visible data:

```text
OpenWaveformRequest {
  file: FileRef {
    path,
    expected_size,
    expected_digest
  },
  options: {
    load_hierarchy: true,
    preferred_page_bytes: 4194304
  }
}

WaveformOpened {
  waveform: WaveformRef,
  hierarchy: HierarchyPage,
  diagnostics
}

WaveformRef {
  artifact: ArtifactRef(kind: "wellen.waveform", ...),
  timescale,
  signal_count,
  hierarchy_digest
}

HierarchyPageRequest {
  waveform: WaveformRef,
  root_path,
  depth,
  page_size
}

HierarchyPage {
  waveform: WaveformRef,
  nodes,
  next_page,
  diagnostics
}

WavePageRequest {
  waveform: WaveformRef,
  signal_ids,
  time_start,
  time_end,
  encoding
}

WavePage {
  page: PageRef,
  blob: BlobRef,
  sample_count,
  dropped_or_compressed_segments,
  diagnostics
}
```

Rust-side state:

```text
wellen::Hierarchy
wellen waveform/session objects
file handles
mmap or compressed buffers
decode indexes
thread pools
selected-signal caches
page caches
```

None of that Rust-side state crosses into Boon. If the process restarts, equal
requests against equal content produce equal `WaveformRef` and `PageRef` values,
even though the new process uses different Rust allocations and worker threads.

## Other Crate Archetypes

| Crate or archetype | Boon-visible data | Rust-owned state |
| --- | --- | --- |
| Arrow / Polars | dataset refs, schema records, lazy plan records, bounded record-batch page refs | Arrow buffers, `RecordBatch`, `DataFrame`, `LazyFrame`, scanners, SIMD and parallel executors, memory pools |
| Tantivy | index snapshot refs, query AST records, hit pages with ids/scores/highlights | `Index`, writer, reader, searcher snapshots, tokenizer objects, segment files, merge threads |
| tree-sitter | grammar refs, source version refs, edit records, syntax outline pages, capture pages | `Parser`, `Language`, `Tree`, `TreeCursor`, compiled `Query`, rope cache |
| reqwest / Tokio | HTTP request records, response refs, body page refs, completion events | Tokio runtime, tasks, futures, clients, connection pools, TLS state, sockets, timers, streams |
| SQL / RocksDB | schema refs, query template records, params, snapshot refs, row pages, checkpoint refs | pools, prepared statements, transactions, snapshots, iterators, WAL, compaction threads, block cache |
| Candle / ort | model refs, tensor descriptors, tensor page refs, inference request/result records | devices, tensor storage, model weights, ORT sessions, execution providers, GPU arenas, KV caches, thread pools |

These examples all use the same rule: Boon sees declarative identity, schema,
intent, epochs, and page windows. Rust owns capabilities, handles, schedulers,
buffers, caches, unsafe pointers, OS resources, and objects whose correctness
depends on lifetime or mutation identity.

## Build And Check Commands

A Boon project should have tool commands with explicit responsibilities:

```bash
boon init
boon check
boon check-bridge
boon build
boon run
```

First run:

```bash
boon init my_wave_app
cd my_wave_app
boon check-bridge
boon check
boon build
boon run
```

`cargo check --workspace` is still useful, but it checks the user-owned Rust
workspace members such as `bridge` and `rust/my_domain_lib`. It should not be the
first command required to create `.boon/generated-runner`; `boon build` creates
that generated package.

`boon init`:

- creates `Boon.toml`, starter Boon source, `bridge/Cargo.toml`, and
  `bridge/src/lib.rs`;
- pins `boon_bridge` to the installed Boon SDK version;
- writes `.boon/` to `.gitignore`;
- leaves the generated runner absent until `boon build`.

`boon check`:

- parses `Boon.toml`;
- reads Boon source units;
- validates Boon imports against bridge metadata produced by `boon check-bridge`
  or extracted on demand with the same rules;
- checks pure/effect usage;
- verifies schema hashes and program hashes.

`boon check-bridge`:

- runs `cargo check` for the bridge crate;
- asks the bridge crate for registry metadata;
- checks every exported schema can be canonicalized;
- rejects forbidden Boon-visible native handle types;
- reports provider crate versions and enabled features.

`boon build`:

- generates the app runner if needed;
- includes Boon source hashes, bridge schema hashes, bridge crate package id, and
  Cargo lockfile digest in build metadata;
- invokes Cargo to compile the runner and bridge crate;
- fails if bridge metadata observed at build time differs from the metadata used
  by Boon typechecking/lowering.

`boon run`:

- starts the selected host target;
- registers the bridge crate before loading the Boon runtime;
- starts host bridge executors for effect exports;
- routes bridge completions as `SOURCE` data.

## Dynamic Loading Later

Dynamic loading can be added later for development iteration, but it should not
be the first production path.

If added, it must require:

- explicit `mode = "dynamic"` in `Boon.toml`;
- a platform-specific artifact path or build command;
- a bridge `Cargo.toml` with `crate-type = ["cdylib"]` for the dynamic artifact;
- exported metadata and registration symbols with a stable ABI version;
- target triple, feature set, artifact hash, and Cargo lockfile digest checks;
- stable C ABI or generated ABI shim;
- module/schema hash handshake before any call;
- provider/version/feature checks;
- panic/unwind containment at the dynamic boundary;
- no unloading while host-side cached artifacts may still be needed to service
  active canonical Boon-visible descriptors;
- the same pure-data request/result/page model as static bridge mode.

Example dynamic manifest shape:

```toml
[rust_bridge]
mode = "dynamic"
package = "my_wave_app_bridge"
path = "bridge"
artifact = "target/release/libmy_wave_app_bridge.so"
target_triple = "x86_64-unknown-linux-gnu"
abi = "boon-bridge-c-v1"
expected_artifact_hash = "sha256:..."
```

The bridge crate would also need:

```toml
[lib]
crate-type = ["cdylib"]
```

Dynamic loading changes how Rust code is loaded. It must not change what Boon
values are. Boon-visible descriptors remain pure data and never point to loader
allocations, cache entries, or native objects.

## Anti-Patterns

Reject these designs:

- `BoonValue::Native(Box<dyn Any>)`;
- exposing `Arc<Mutex<T>>` or `Rc<RefCell<T>>` as a Boon value;
- exposing raw pointers, file descriptors, sockets, DB pools, GPU textures,
  parser trees, searchers, tensors, or windows;
- comparing bridge values by native pointer identity;
- direct Rust callbacks mutating `HOLD`;
- hidden global caches as the source of semantic truth;
- pure exports that read the filesystem, clock, random state, network, process
  environment, or mutable global state;
- continuous effect invocation without canonical request identity and deduping;
- stale, canceled, timed-out, schema-drifted, or out-of-order completions
  mutating current Boon state;
- ambient filesystem, network, GPU, clock, or database access without declared
  capabilities;
- unstable float encoding in Boon-visible values;
- schema-name drift between Boon imports, registry exports, and documentation;
- serde-derived Rust structs as the public schema without a stable bridge
  contract;
- Boon source importing arbitrary Rust crate paths directly;
- a playground-only `match example` bridge path.

## Acceptance Checklist

A bridge design is acceptable when:

- every Boon-visible bridge type has a canonical schema;
- canonical serialize/deserialize round trips are golden-tested;
- equality is stable across processes and runs;
- public bridge values have no native-handle variant;
- all effects re-enter as source data and normal runtime ticks;
- Rust panics, timeouts, and crate errors become diagnostics data;
- caches are derived only and invalidated by canonical inputs, schema hashes,
  provider versions, and crate features;
- pure exports are deterministic under repeated execution;
- duplicate, canceled, timed-out, schema-drifted, and out-of-order effect
  completions are tested;
- resource grants and denials are tested for each effect export;
- bridge numeric schemas have decimal-only v1 coverage or explicit float golden
  vectors;
- app build metadata includes Boon source hashes, bridge schema hashes, bridge
  crate identity, and Cargo lockfile digest;
- docs and tests cover at least one pure crate, one async/effect crate, one
  native-resource crate, and one large-page crate such as Wellen.

## Reference Links

- Cargo dependencies: <https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html>
- Cargo workspaces: <https://doc.rust-lang.org/cargo/reference/workspaces.html>
- Cargo build scripts: <https://doc.rust-lang.org/cargo/reference/build-scripts.html>
- Cargo targets and crate types: <https://doc.rust-lang.org/cargo/reference/cargo-targets.html>
- Wellen: <https://docs.rs/wellen/latest/wellen/>
- Arrow `RecordBatch`: <https://arrow.apache.org/rust/arrow/record_batch/index.html>
- Polars: <https://docs.rs/polars/latest/polars/>
- Tantivy: <https://docs.rs/tantivy/latest/tantivy/>
- tree-sitter: <https://docs.rs/tree-sitter/latest/tree_sitter/>
- reqwest: <https://docs.rs/reqwest/latest/reqwest/>
- SQLx: <https://docs.rs/sqlx/latest/sqlx/>
- RocksDB: <https://docs.rs/rocksdb/latest/rocksdb/>
- Candle: <https://github.com/huggingface/candle>
- ort: <https://docs.rs/ort/latest/ort/>
