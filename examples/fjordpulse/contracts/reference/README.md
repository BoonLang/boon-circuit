# FjordPulse Contracts

These are the canonical browser-facing boundaries shared by fake adapters, real
adapters, PHP DTOs/validators, TypeScript types/validators, and contract tests.

## Canonical files

```text
http/openapi.yaml
realtime/envelope.schema.json
realtime/client-message.schema.json
realtime/server-message.schema.json
```

The top-level realtime files above are authoritative. Do not introduce a
`contracts/realtime/messages/` directory or a second schema source.

Protocol v1 rules:

- every WebSocket message carries `protocolVersion: 1`;
- every client command carries a correlation `id`, `type`, and typed `payload`;
- every server message carries `createdAt`;
- every database-originated notification carries `eventId`, `entityId`,
  `scope`, `version`, `createdAt`, and typed `payload`;
- `station_snapshot_changed` is the one canonical station database event;
  the two older named subresource notifications remain schema-covered only as
  views of that same event identity, never as a direct post-write path;
- authoritative snapshots are sent on watch/reconnect and have a version but no
  fabricated database event id;
- unsupported versions, unknown message types, and unknown payload fields are
  invalid;
- UTC RFC3339 timestamps are used at every boundary.

`fixtures/` contains deterministic valid examples and intentionally invalid
negative cases. Array fixtures are validated one element at a time. The
machine-readable mapping from endpoints/messages to the story backlog lives in
`traceability.json`; it also marks stories that have no independent wire shape
so all 108 stories are accounted for without inventing endpoints.
