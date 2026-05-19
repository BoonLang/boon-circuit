# Delta Protocol

The circuit runtime must be delta-native. LIST and field changes should flow to
renderers, persistence, and other runtimes without copying whole app state.

## Layers

There are three related delta layers.

Semantic deltas:

```text
source events
state cell changes
list insert/remove/move
field changes
source bind/unbind
```

Render deltas:

```text
element insert/remove/move
property/text/style changes
event binding changes
```

Network/persistence deltas:

```text
authoritative semantic state changes
client source events
ack/reject/rebase messages
```

The semantic layer is canonical. Render and network deltas are lowerings.

## Semantic Delta Shape

Illustrative structure:

```text
StateDelta {
  program_hash,
  runtime_id,
  base_epoch,
  next_epoch,
  changes: [
    ListInsert {
      list_id,
      scope,
      key,
      generation,
      position,
      initial_fields
    },
    ListRemove {
      list_id,
      scope,
      key,
      generation
    },
    ListMove {
      list_id,
      scope,
      key,
      generation,
      position
    },
    FieldSet {
      list_id,
      scope,
      key,
      generation,
      field_path,
      value,
      changed_at
    },
    SourceBind {
      scope,
      source_path,
      source_id,
      generation,
      bind_epoch
    },
    SourceUnbind {
      source_id,
      bind_epoch
    }
  ]
}
```

Nested scopes may be encoded as compact ids on the wire, but the logical model is
a scope path.

For keyed data, every semantic/render/network delta uses the canonical runtime
identity tuple:

```text
(runtime_id, program_hash, list_id, parent_scope, item_key, generation)
```

Generation is mandatory on all keyed field, move, remove, source-bind, and
source-unbind facts. It is not only an insert/remove concern.

## Source Event Shape

Browser/native UI events enter as source events:

```text
SourceEvent {
  program_hash,
  runtime_id,
  source_id,
  scope,
  generation,
  bind_epoch,
  client_seq,
  payload
}
```

Generation and bind epoch are mandatory for keyed list item sources. Generation
prevents stale DOM or network events from writing into a reused row slot.
Bind epoch prevents a late event from an old renderer binding from writing into a
new binding that happens to reuse a source id. Source ids may be reused only
after an acknowledgement/barrier or guarded by a new bind epoch.

## Boon To Ply

The Boon runtime should not ask Ply to diff a virtual DOM.

Instead:

```text
Boon semantic delta
  -> render lowering
  -> Ply render patch
```

Example:

```text
FieldSet(list=todos, key=42, generation=7, field=completed, value=true)
  -> SetProperty(element=/todos:42:7/checkbox, checked=true)
  -> SetClass(element=/todos:42:7/root, completed=true)
```

Insertion:

```text
ListInsert(list=todos, key=42, generation=7, fields={title, completed, editing})
  -> InsertElement(parent=/todo-list, key=/todos:42:7/root, position=...)
  -> SetText(/todos:42:7/title, title)
  -> BindSource(/todos:42:7/checkbox.click, source_id=...)
```

Removal:

```text
ListRemove(list=todos, key=42, generation=7)
  -> RemoveElement(/todos:42:7/root)
  -> SourceUnbind(...)
```

This gives deterministic DOM/native UI changes without list diffing.

## Browser Runtime To Server Runtime

There are two common modes.

Client-input/server-authoritative:

```text
browser sends SourceEvent
server runs tick
server sends StateDelta
browser applies StateDelta
```

Shared deterministic runtime:

```text
browser runs immediate optimistic tick
server runs same tick authoritatively
server sends ack or correcting StateDelta
```

The first pass should implement the simpler server-authoritative protocol shape
even if the playground runs only locally.

## Determinism Rules

Every delta should carry enough protocol identity to be replayed:

```text
program_hash
runtime_id
epoch/tick
list_id or expr_id
scope_path or compact scope id
item_key
generation
field_path
source_id
value
```

Renderers should not infer protocol identity from array positions. Positions are
layout facts; keys are hidden protocol/runtime facts. These keys are not Boon
values and are never used for Boon equality.

Replay rules:

- `base_epoch` must match the receiver's current epoch.
- `next_epoch` must advance by exactly one unless applying an explicit snapshot.
- repeated `(runtime_id, client_seq)` source events are deduplicated.
- applied server deltas carry authoritative `server_tick`.
- if epoch, generation, or bind epoch mismatches, the receiver rejects the delta
  and requests a snapshot recovery.

## Whole Snapshots

Snapshots are still useful for:

- initial load
- debugging
- persistence checkpoints
- recovery after protocol mismatch

But normal app operation should use deltas. A snapshot is not the update
protocol.
