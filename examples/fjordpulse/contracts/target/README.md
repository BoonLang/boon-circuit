# FjordPulse Target Contract Fixtures

This directory contains source-controlled fixtures emitted by the current
deterministic FjordPulse Server Boon artifact. The immutable pinned-product
oracle remains under `../reference/`.

Refresh the HTTP fixtures deliberately with:

```text
cargo test -p boon_server_runtime --test fjordpulse_http_contract \
  regenerate_fjordpulse_target_http_fixtures_from_server_outputs \
  -- --ignored --exact
```

The generator binds the generic loopback server, compiles the real Server Boon
source, validates every response with the strict bounded JSON decoder, and
writes the raw response bytes. The normal contract test never updates fixtures;
it compares decoded responses exactly against these committed files.

A captured target fixture is evidence of current deterministic output, not by
itself approval for a compatibility delta. Any field that differs from the
pinned contract must also be covered by
`../../traceability/compatibility_delta_ledger.json`, paired schemas and valid
and invalid fixtures, compatible protocol versions, and black-box evidence
before the implementation can claim compatibility.
