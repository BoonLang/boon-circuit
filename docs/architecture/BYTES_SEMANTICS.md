# BYTES Semantics

This document records the v1 language/runtime contract for Boon `BYTES`.

Status: partial implementation. The parser, typechecker signatures, IR
expression coverage, PlanExecutor byte storage, and runtime byte carriers have
initial support. Full builtin runtime bodies, example refactors, and final
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

When conversion input is a static `TEXT` literal, the typechecker may refine the
result to a fixed `BYTES[N]` length and reject malformed static data before
lowering:

- `Text/to_bytes(encoding: Utf8)` uses the literal's UTF-8 byte length;
- `Text/to_bytes(encoding: Ascii)` uses the literal byte length only when every
  byte is ASCII, otherwise it is a typechecker diagnostic;
- `Bytes/from_hex` uses the decoded byte length for valid static hex and
  rejects odd-length or invalid static hex text;
- `Bytes/from_base64` uses the decoded byte length for valid static base64 and
  rejects invalid static base64 text.

Non-literal `TEXT` values remain dynamic `BYTES` conversions. Malformed dynamic
data is checked by the runtime/PlanExecutor boundary, not guessed by the
compiler.

## Endian And Numeric Access

Multi-byte numeric operations must specify endian explicitly:

```boon
bytes |> Bytes/read_unsigned(offset: 0, byte_count: 4, endian: Little)
bytes |> Bytes/write_unsigned(offset: 0, byte_count: 4, endian: Big, value: 1)
```

`byte_count` is limited to `1`, `2`, `4`, or `8` in v1. The typechecker
registers these builtin signatures and checks static `byte_count` values plus
`endian: Little|Big`.

BYTES scalar arguments may use a narrow static integer expression subset:
integer literals and checked `+`, `-`, and `*` over integer literals. This
subset is folded by the typechecker and emitted in the resolved constant table
so semantic IR and MachinePlan lowering still receive typed constants, not AST
or string expressions. Unsupported literal-only static formulas such as
division and modulo are compiler errors. Calls, identifiers, field reads,
comparisons, and other dynamic values are not folded constants; they remain
dynamic Boon values and must be handled by runtime/lowering rather than being
rejected merely because they are not static.

## Bounds And Conversion Failures

Use the existing Boon recoverable-error convention rather than Rust panics:

- malformed literals are parser diagnostics;
- incompatible constructor items are typechecker diagnostics;
- fixed-size BYTES operations with statically known out-of-bounds indexes or
  ranges are typechecker diagnostics;
- out-of-bounds runtime reads/writes produce deterministic Boon errors;
- decoding failures produce deterministic Boon errors with the requested
  encoding named;
- host-endian behavior is forbidden.

No accepted BYTES path may panic, read out of bounds, expose uninitialized
memory, or depend on the host machine endian.

## Current Implementation Notes

PlanExecutor/runtime byte paths can currently carry inline, shared, blob-ref,
and page-ref runtime bytes from bridge paths and source/runtime execution paths.
Source-language BYTES literals currently lower to inline runtime bytes.
Runtime-owned dynamic payloads larger than the
source-event inline limit use shared executable storage: public summaries still
expose only storage kind, digest, and byte length, while byte operations borrow
the shared payload through the private runtime representation. Blob/page-backed
concatenation in constructors is intentionally rejected until a resolver exists
for descriptor-only external byte references.

The final PlanExecutor path must not execute parser AST or string paths for
BYTES. It must use typed IDs, typed storage layout, typed operation regions,
and verified semantic deltas.
