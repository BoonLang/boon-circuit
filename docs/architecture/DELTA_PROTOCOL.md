# Delta Protocol

The circuit runtime must be delta-native. LIST and field changes should flow to
renderers, persistence, and other runtimes without copying whole app state.

## Layers

There are three related delta layers.

Semantic deltas:

```text
source events
cell changes
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
      position
    },
    FieldSet {
      scope,
      key,
      field_path,
      value,
      changed_at
    },
    SourceBind {
      scope,
      source_path,
      source_id,
      generation
    },
    SourceUnbind {
      source_id
    }
  ]
}
```

Nested scopes may be encoded as compact ids on the wire, but the logical model is
a scope path.

## Source Event Shape

Browser/native UI events enter as source events:

```text
SourceEvent {
  program_hash,
  runtime_id,
  source_id,
  scope,
  generation,
  client_seq,
  payload
}
```

Generation is mandatory for keyed list item sources. It prevents stale DOM or
network events from writing into a reused row slot.

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
FieldSet(/todos:42, completed, true)
  -> SetProperty(element=/todos:42/checkbox, checked=true)
  -> SetClass(element=/todos:42/root, completed=true)
```

Insertion:

```text
ListInsert(/todos, key=42, fields={title, completed, editing})
  -> InsertElement(parent=/todo-list, key=/todos:42/root, position=...)
  -> SetText(/todos:42/title, title)
  -> BindSource(/todos:42/checkbox.click, source_id=...)
```

Removal:

```text
ListRemove(/todos, key=42)
  -> RemoveElement(/todos:42/root)
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

## Whole Snapshots

Snapshots are still useful for:

- initial load
- debugging
- persistence checkpoints
- recovery after protocol mismatch

But normal app operation should use deltas. A snapshot is not the update
protocol.
