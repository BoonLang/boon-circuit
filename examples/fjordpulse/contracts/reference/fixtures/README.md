# Contract fixtures

HTTP fixtures are individual response bodies named by `http/index.json`.
Realtime fixtures are arrays and must be validated one array element at a time
against the corresponding client or server schema.

Files prefixed with `invalid-` contain `{reason, message}` cases. Their nested
`message` value is expected to fail validation. They are deliberate contract
tests, not examples that implementations may accept.
