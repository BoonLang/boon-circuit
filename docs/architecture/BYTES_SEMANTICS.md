# BYTES Semantics

This document records the v1 language/runtime contract for Boon `BYTES`.

Status: partial implementation. The parser, typechecker signatures, IR
expression coverage, and legacy runtime literal carrier have initial support.
PlanExecutor storage, full builtin runtime bodies, example refactors, and final
verification gates are still part of the active BYTES/MachinePlan roadmap.

## Values

`BYTE` is one unsigned byte in the range `0..255`.

`BYTES` is an ordered byte sequence. It is not `TEXT`, not `LIST<NUMBER>`, and
not a host-only opaque object. Runtime/debug summaries may show byte length,
hash, and storage kind, but executable plans must carry typed byte storage and
typed byte operation refs.

## Constructors

Supported source forms:

```boon
BYTES {}
BYTES[__] { 16u89, 16u50, 16u4E, 16u47 }
BYTES[4] { 16u89, 16u50, 16u4E, 16u47 }
BYTES[64] {}
```

`BYTES {}` is dynamic-size. `BYTES[__]` infers an exact fixed length only when
all items have fixed length. `BYTES[N]` has exact fixed length `N`.

An empty `BYTES[N] {}` is a zero-filled fixed byte sequence of length `N`.

Nested `BYTES` constructors flatten logically:

```boon
BYTES[__] { BYTES[2] { 16u01, 16u02 }, 16u03 }
```

has type `BYTES[3]`.

## Byte Literals

Explicit-base byte literals use the v1 form:

```boon
16uFF
10u255
2u10101010
```

The parser rejects unsupported bases, invalid digits, empty digits, and values
larger than `255`.

## TEXT Boundaries

`TEXT` never implicitly becomes `BYTES`, and `BYTES` never implicitly becomes
`TEXT`.

Use explicit conversion operations:

```boon
text |> Text/to_bytes(encoding: Utf8)
bytes |> Bytes/to_text(encoding: Utf8)
formula_text |> Text/to_bytes(encoding: Ascii)
bytes |> Bytes/to_hex
TEXT { FF } |> Bytes/from_hex
```

When a `TEXT` value appears inside a `BYTES` constructor, the typechecker should
emit a direct error suggesting `Text/to_bytes`.

`Ascii` is a strict boundary for byte-indexed grammars. Encoding rejects any
non-ASCII `TEXT`, and decoding rejects any byte above `0x7F`; this keeps byte
offsets equal to Boon text positions for examples such as Cells formula
operator scanning.

## Endian And Numeric Access

Multi-byte numeric operations must specify endian explicitly:

```boon
bytes |> Bytes/read_unsigned(offset: 0, byte_count: 4, endian: Little)
bytes |> Bytes/write_unsigned(offset: 0, byte_count: 4, endian: Big, value: 1)
```

`byte_count` is limited to `1`, `2`, `4`, or `8` in v1. The current typechecker
registers these builtin signatures and checks literal `byte_count` values plus
`endian: Little|Big`. Runtime operation bodies and non-literal constant
resolution remain Phase 7/9 work.

## Bounds And Conversion Failures

Use the existing Boon recoverable-error convention rather than Rust panics:

- malformed literals are parser diagnostics;
- incompatible constructor items are typechecker diagnostics;
- out-of-bounds runtime reads/writes produce deterministic Boon errors;
- decoding failures produce deterministic Boon errors with the requested
  encoding named;
- host-endian behavior is forbidden.

No accepted BYTES path may panic, read out of bounds, expose uninitialized
memory, or depend on the host machine endian.

## Current Implementation Notes

The legacy runtime can currently carry inline, blob-ref, and page-ref runtime
bytes from bridge paths. Source-language BYTES literals currently lower through
the legacy evaluator as inline runtime bytes. Blob/page-backed concatenation in
constructors is intentionally rejected in the legacy evaluator until Phase 7
adds measured dynamic byte storage and operation bodies.

The final PlanExecutor path must not execute parser AST or string paths for
BYTES. It must use typed IDs, typed storage layout, typed operation regions,
and verified semantic deltas.
