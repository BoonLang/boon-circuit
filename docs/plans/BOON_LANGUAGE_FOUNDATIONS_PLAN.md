# Boon Language Foundations Plan

Date: 2026-07-26

Status: authoritative replacement architecture and implementation plan. This
document defines the intended flag-day language contract. It does not claim
that the current parser, typechecker, runtime, persistence layer, Wasm target,
GPU path, or FPGA path already implements that contract.

Under the combined order in [`steps.md`](steps.md), this plan's semantic,
runtime, target, migration, and deletion work lands before the full formal
roadmap. Its formal-dependent final acceptance closes only after formal phases
0–5.

## Purpose

Define the smallest coherent universal Boon language foundation that can
support:

- the production compiler, verifier, optimizer, and runtime;
- ordinary server and frontend applications;
- deterministic persistence and distributed execution;
- native and Wasm execution;
- bounded GPU and FPGA lowering;
- formal verification.

The plan deliberately avoids copying Rust's surface into Boon. It does not add
unrestricted pointers, unrestricted recursion, imperative loops, public
integer-width types, a public Boolean type, or a separate core type for every
compiler data structure.

The resulting language has:

- one exact `NUMBER`;
- `TEXT` and `BYTES`;
- raw fixed-width `BITS[N]`;
- Tags and tagged payloads;
- structural objects;
- delta-native `LIST`, `SET`, and `MAP`;
- Boon's existing flow and temporal constructs;
- `FLUSH` as explicit fail-fast flow control.

Everything else should first be expressed as a library, target profile, or
compiler lowering.

Self-hosting, a compiler written in Boon, BoonInBoon, and a Boon operating
system are explicit non-goals and are not prerequisites for this plan's
completion. A later project may use the resulting language and compiler
artifacts for those experiments, but this plan neither designs nor validates
them.

## Authority And Flag-Day Rule

For the topics listed below, this plan supersedes conflicting semantic claims
in older plans and architecture documents:

- public truth values and the absence of a public `BOOL`;
- matching;
- exact `NUMBER`;
- one-based positions;
- `BITS[N]`;
- `MAP` and `SET`;
- collection authority and collection use inside `HOLD`;
- bounded repetition through `HOLD + Stream/pulses`;
- `FLUSH`;
- executable language-feature coverage.

It does not supersede unrelated native GPU, renderer, persistence durability,
distributed topology, or performance contracts.

`BOON_COMPILER_PERFORMANCE_PLAN.md` owns current compiler latency, memory,
session, invalidation, cancellation, and scaling work. It may pull forward the
compiler-internal arenas, interners, bounded worklists, and transient builders
described here when they preserve today's public semantics. Doing so does not
implement or claim any future foundations phase.

There is no compatibility mode for the old behavior. Each implementation phase
is an atomic flag-day replacement for its topic and must update source examples,
tests, schemas, persistence fixtures, wire fixtures, documentation, and target
backends together. The repository is still developing the first Boon language
versions; retaining a binary64 profile, zero-based API aliases, `NaN`
sentinels, public `BOOL` diagnostics, or old pattern behavior would create two
languages.

Forbidden compatibility mechanisms include:

- deprecated aliases or old spellings accepted by the parser;
- feature flags, semantic profiles, or environment switches selecting old
  behavior;
- adapter/shim APIs that translate an old Boon API into the new one;
- dual AST, type, IR, plan, runtime, persistence, or wire representations;
- dual-read, dual-write, version-negotiation, or automatic data migration for
  the old pre-release encodings;
- fallback from a failed new operation to the old implementation;
- keeping obsolete examples, tests, golden files, comments, or active
  documentation “for reference.”

The implementation sequence for every replaced topic is:

1. implement the new contract through every layer;
2. migrate every active caller and fixture in the same change;
3. delete the old APIs, enum variants, branches, encodings, tests, and docs;
4. regenerate new golden artifacts from scratch;
5. fail the merge gate if any executable old surface remains.

Git history is the archive. Obsolete implementation and active documentation
do not remain in the tree merely because they may be historically interesting.
Development databases and pre-release persisted fixtures are discarded rather
than migrated.

Until an implementation phase lands, current executable behavior remains the
behavior described by the active engine. A plan is not a fallback parser or a
license to partially accept new syntax. Once a phase lands, no code path from
that topic's previous behavior remains.

This rule does not prohibit explicit conversion at an external protocol
boundary, such as converting a protocol-defined zero-based offset to a
one-based Boon position. It also does not prohibit keeping a semantically
ineligible GPU/FPGA region on the native interpreter. Those choices preserve
the one current language contract; they do not preserve an old Boon contract.

Application-authored state evolution using the current `DRAIN`/`DRAINING`
contract may remain. It must operate only on current canonical values and must
not contain decoders, variants, or branches for the removed pre-release
language representations.

## Executive Decisions

1. `True` and `False` are ordinary Tags. There is no public `BOOL` type.
2. `NUMBER` is one exact canonical rational value kind.
3. A source literal such as `42` is always `NUMBER`; it is never inferred as
   `BITS`.
4. `BITS[N]` is a raw fixed-width bit sequence with no stored signedness.
5. User-visible positions begin at one. Counts, lengths, and shift amounts may
   be zero.
6. Matching supports exact NUMBER, TEXT, and BITS literals, Tags,
   tagged-payload binding, wildcard, and whole-value binding. BYTES and
   collections use explicit equality. Matching does not support runtime type or
   arbitrary structural matching.
7. `MAP` keeps the syntax `MAP { key => value }`.
8. `LIST`, `SET`, and `MAP` are delta-native authorities and are forbidden
   recursively inside `HOLD` state.
9. `MAP` writes are explicit upserts. A changing write key does not silently
   rename or remove the old entry.
10. `MAP` and `SET` enumerate canonically, never in hash or incidental
    insertion order.
11. Boon does not add `Loop/*`, `fold`, `scan`, or `reduce`. Bounded repeated
    state transitions use `HOLD + Stream/pulses`.
12. `FLUSH` aborts the enclosing expression/operator activation. It is not
    collection-item isolation, a public error wrapper, state migration,
    permanent stream cancellation, or implicit hardware backpressure.
13. Queue, Stack, Deque, Arena, Interner, Worklist, UnionFind, Table, and Memory
    are not new public value kinds.
14. Optimizers recognize checked IR semantics, effects, ownership, and access
    patterns, not privileged library function spellings.
15. GPU and FPGA are eligibility targets. They must reject or leave on the CPU
    any region whose exact semantics and bounds cannot be proved.
16. Every changed language/API contract is a flag-day deletion: no old alias,
    shim, profile, decoder, fixture, or executable branch survives its phase.

## Public Value Algebra

The public data universe is:

```text
NUMBER
TEXT
BYTES
BITS[N]
Tag
Tag[field: value, ...]
[field: value, ...]
LIST
SET
MAP
```

`LIST<T>`, `SET<T>`, `MAP<K, V>`, closed Tag sets, object rows, and refinements
are useful compiler descriptions. They are not a requirement to expose generic
type annotations in Boon source.

`[]` is the empty object. It does not introduce a separate unit type.

`Null` in source, if an application chooses that spelling, is an ordinary bare
Tag. It is not absence, a null pointer, a bottom value, or a separate public
type. Internal absence/presence uses a private envelope and must not be encoded
as the public `Null` Tag.

Likewise, `Error` and `Error[...]` are ordinary application Tags. Runtime,
compiler, transport, and host faults use private fault channels until a
boundary deliberately translates them into a specific closed application Tag.
They are not a privileged public `Error` value kind.

`Dependency/catch_cycle(value:, on_cycle:)` is the explicit recovery boundary
for a runtime-dependent derived/list cycle. Its `on_cycle` argument is an
ordinary application value, normally a member of the result's closed Tag set;
the private cycle fault itself is never matchable, stored, persisted, or
serialized. Other engine faults remain terminal.

The flag-day migration removes public/runtime ambiguity from the current
`Value::Bool`, `Value::Null`, and `Value::Error` representations:

- True/False become ordinary Tags;
- internal absence receives its own private representation;
- internal faults receive a private non-data representation;
- persisted values, wire values, schemas, and fingerprints are regenerated;
- golden tests prove that a source `Null` or `Error[...]` round-trips as a Tag
  while absence and host faults cannot round-trip as application data.

`SOURCE`, `SKIP`, `THEN`, `HOLD`, `LATEST`, `WHEN`, `WHILE`, `DRAIN`,
`DRAINING`, and `FLUSH` describe presence, time, migration, or control. They are
not additional ordinary data values.

Host resources such as files, sockets, database connections, GPU buffers,
secrets, process handles, and devices are not serializable Boon values. They
remain host-owned capabilities or effects.

### True And False

`True` and `False` are singleton Tags:

```text
Tag(True)
Tag(False)
```

The typechecker may infer the closed set:

```text
True | False
```

Equality, comparison, predicates, `Set/contains`, and `Bits/get` return those
Tags. Existing library namespaces such as `Bool/not` may remain as ordinary
libraries over `True | False`; the namespace name does not create a language
type.

The parser must normalize `True` and `False` through ordinary Tag handling.
Public AST inspection, diagnostics, type hints, persistence, and canonical wire
inspection must not claim that Boon has `BOOL`.

Backends may use Rust `bool`, a Wasm integer lane, a GPU predicate, or one FPGA
bit. Those are private representations of a closed Tag set.

### Internal Refinements

The compiler may infer facts such as:

- whole Number;
- nonnegative Number;
- nonzero Number;
- bounded Number interval;
- exact fixed scale;
- exact dyadic value;
- BITS width;
- closed Tag set;
- key-safe value;
- collection capacity;
- pure expression;
- non-escaping authority;
- bounded work;
- non-flushing expression.

These are proof and lowering facts. They are not runtime-matchable public types.

## Matching

### V1 Pattern Surface

Allowed patterns are:

```text
__
lowercase_binding
exact NUMBER literal
exact TEXT literal
exact BITS[N] literal
BareTag
Tag[payload_field_bindings]
```

Examples:

```boon
value |> WHEN {
    42 => answer()
    TEXT { admin } => administrator()
    BITS[7] { 2u0110011 } => RegisterInstruction
    Found[value] => use(value: value)
    NotFound => missing()
    other => fallback(value: other)
}
```

A lowercase binding always binds the complete selected value. It never changes
meaning because a same-named variable exists in an outer scope.

`Found[value]` is the one compound-pattern exception. It:

1. discriminates the known Tag `Found`;
2. binds the Tag's inferred payload field named `value`;
3. does not recursively match the field's structure.

For a multi-field payload:

```boon
InvalidNumber[reason, position] => report(
    reason: reason
    position: position
)
```

Every listed payload name must be a real field. Nested payload patterns,
renaming, runtime type tests, and implicit field search are invalid.

### Rejected Patterns

Reject with targeted diagnostics:

- raw object patterns such as `[field: pattern]`;
- `LIST { ... }` patterns;
- `SET { ... }` patterns;
- `MAP { ... }` patterns;
- runtime type patterns;
- nested or destructuring BITS patterns;
- masked BITS mini-patterns such as `1???`;
- `NaN`;
- dynamic pin/comparison patterns such as `{expected}`;
- any pattern whose binding-versus-comparison meaning depends on scope.

Unsupported compound patterns must never fall through to an uppercase Tag
comparison. In particular, `LIST { ... }` must not be accepted as the Tag
`LIST`.

Exact collection comparison remains explicit:

```boon
actual == expected
|> WHEN {
    True => equal()
    False => different()
}
```

Collection observation uses ordinary APIs:

```boon
list |> List/get(position: 1)
map |> Map/get(key: selected_key)
set |> Set/contains(item: selected_item)
```

### MAP Arrow Grammar

`MAP` keeps:

```boon
MAP {
    key => value
}
```

`:` remains reserved for object fields and named call entries.

The parser must treat `=>` according to the nearest grammar construct. A
`WHEN`/`WHILE` arm separator is recognized only at the arm's outer delimiter
depth. Consequently, both of these are unambiguous:

```boon
result |> WHEN {
    Found => MAP {
        first_key => first_value
        second_key => second_value
    }

    NotFound => MAP {}
}
```

```boon
-- Possible future syntax, not valid in V1:
map |> WHEN {
    MAP {
        TEXT { id } => id
        TEXT { role } => Admin
    } => KnownAdmin
}
```

V1 must reject the second example because MAP matching is not supported, but it
must not reject it because the arrows are lexically ambiguous.

If MAP patterns are reconsidered later, that design must separately answer:

- exact key set versus required subset;
- whether a rest marker such as `...` is present;
- exact versus bound keys;
- whole-map versus per-key dependency tracking;
- failure and cost behavior for live maps.

No MAP-pattern behavior is reserved other than delimiter-safe parsing.

## Exact NUMBER

### Semantic Domain

`NUMBER` is an exact arbitrary-precision rational:

```text
numerator / denominator
denominator > 0
gcd(abs(numerator), denominator) = 1
zero = 0 / 1
```

Integer, decimal, and rational results are not different public kinds.

```boon
0.1 + 0.2 == 0.3
-- True

1 / 3 * 3 == 1
-- True

1.00 == 1
-- True
```

There is no negative zero, NaN, or infinity.

### Arithmetic

`+`, `-`, `*`, and `/` are exact.

`%` accepts only whole Numbers and uses Euclidean remainder. For whole `a` and
nonzero whole `b`, it returns the unique `r` for which:

```text
a = b * q + r
0 <= r < abs(b)
```

where `q` is whole.

A statically evident invalid operation is a compile error. A dynamically
encountered invalid operation is a deterministic terminal error for the
current Boon program/session. A containing server may isolate and report that
failed session, but the expression does not fabricate a Number.

Division by zero never returns:

- zero;
- NaN;
- infinity;
- a normal Number encoding an error;
- an implicit `FLUSH`.

Applications that want recovery must guard the denominator or call an explicit
tagged library operation:

```boon
denominator == 0
|> WHEN {
    True => DivisionByZero
    False => Divided[value: numerator / denominator]
}
```

The ordinary `/` operator must not return a tagged union because that would
infect every numeric expression.

### Overflow And Resource Exhaustion

NUMBER has no semantic storage width. Therefore it has no wrapping arithmetic
and no `Number/add_or_wrap`.

If an inferred `i32`, `i64`, fixed-point lane, compact rational, or other
specialized representation becomes too small, the implementation must widen or
deopt to the exact path. It must not expose the optimizer's chosen width by
wrapping or changing the result.

Execution profiles must bound denial-of-service costs:

- maximum numerator and denominator bits per value;
- maximum aggregate numeric memory per session;
- maximum arithmetic and GCD work per turn;
- maximum parsed digits;
- maximum formatted digits;
- target-specific FPGA/GPU widths.

Budget exhaustion is a deterministic terminal resource error. It does not
truncate, approximate, wrap, evict, or return a business error Tag.

### Rounding And Irrational Operations

Rounding names an exact quantum and a rule:

```boon
10 / 3
|> Number/round(
    to: 0.01
    using: NearestEven
)
```

The initial rule set is:

- `NearestEven`;
- `NearestAwayFromZero`;
- `TowardZero`;
- `TowardPositive`;
- `TowardNegative`;
- `AwayFromZero`.

`Number/floor`, `Number/ceil`, and `Number/truncate` may remain clear
convenience operations.

The `to:` quantum must be a strictly positive exact Number. A statically known
zero or negative quantum is a compile error; a dynamically invalid quantum is a
deterministic terminal API-domain error. It is never replaced with an absolute
value or backend default.

Irrational and transcendental operations are deferred from the initial exact
NUMBER core. When implemented, they also require an output quantum and rounding
rule, with the same strictly-positive-quantum validation:

```boon
2
|> Number/sqrt(
    to: 0.000001
    using: NearestEven
)
```

The result is the exact rational `1.414214`. No approximate Number subtype is
introduced. The implementation must refine certified integer intervals or an
equivalent algebraic-real representation until exactly one requested rounding
bucket is proved. Native and Wasm differential fixtures must check the proof
result. A backend approximation is never accepted as the language result.

Phase 2 initially lands exact rational arithmetic and rational rounding only.
Square root and other non-rational operations land later behind the certified
rounding contract.

Domain errors such as a negative square root follow the static-or-terminal
rule unless the application uses an explicit tagged safe operation.

### Money And Business Refinements

Currency, legal scale, unit, and display policy remain domain data:

```boon
amount: [
    currency: EUR
    value: 10 / 3
]

display:
    amount.value
    |> Number/round(
        to: 0.01
        using: NearestEven
    )
    |> Number/to_text(decimal_places: 2)
```

`1.20` and `1.2` are the same Number. If trailing zeros or legal scale matter,
the domain object records that policy.

### Parsing And Formatting

Strict parsing returns Tags:

```boon
input
|> Text/to_number()
|> WHEN {
    Parsed[value] => use(value: value)
    InvalidNumber[reason, position] => reject(
        reason: reason
        position: position
    )
}
```

Rules:

- the complete input is consumed;
- decimal and exponent notation parse exactly;
- canonical fraction notation supports round-trip formatting;
- radix parsing produces a whole Number;
- there is no `fallback:` argument;
- prefix scanning is a separate `Text/scan_number()` API;
- parse failure never produces `NaN`.

`Number/to_text()` without display options is exact and round-trippable:

- whole values use integer notation;
- terminating rationals use canonical decimal notation;
- other values use reduced `numerator/denominator` notation.

JSON and host boundaries must not silently pass through binary64. A rational
that cannot be represented exactly by the declared protocol requires an
explicit rounding policy or an exact text/byte encoding.

### Hidden Representations

Permitted private representations include:

- immediate signed or unsigned machine integers;
- wider integer lanes;
- scaled integer/fixed-point;
- compact small rational;
- arbitrary-precision rational.

Binary floating point is permitted only when every observable result is proven
identical, or inside a future explicit approximation contract.

Compiler proof facts may include whole, nonnegative, nonzero, interval, fixed
scale, dyadic, and numerator/denominator bounds. They are not public types or
patterns.

### NUMBER As A MAP Key

Key equality and hashing use the normalized rational. Therefore:

```text
1
1.0
2 / 2
```

are the same MAP key.

Canonical persistence and wire encoding store a minimal normalized signed
numerator and positive denominator.

## BITS[N]

### Definition

`BITS[N]` is an immutable fixed-width raw bit sequence.

- `N` is positive.
- `N` is compile-time known.
- `N` is part of the static type.
- BITS stores no signedness.
- A Number literal never becomes BITS by inference.

Examples:

```boon
flags: BITS[8] { 2u1010_0011 }
opcode: BITS[7] { 2u0110011 }
color: BITS[32] { 16uFF_80_40_FF }
```

The body contains exactly one encoded nonnegative integer token. The radix is
written before `u`; digits run from the most-significant displayed digit to the
least-significant displayed digit; underscores are ignored. Multiple fragments,
implicit concatenation, signs, and whitespace-separated tokens are rejected.

The token is decoded as a nonnegative integer, range-checked against `N`, and
left-zero-filled to exactly `N` bits. No other padding is implicit. The
radix/`u` spelling describes literal encoding and range checking. It does not
create an unsigned BITS type.

Reject:

- `BITS[0]`;
- `BITS[__]`;
- runtime-computed widths;
- literals that do not fit;
- implicit NUMBER, TEXT, or BYTES conversion.

### Position And Direction

Default positions follow displayed reading order:

```boon
leftmost: bits |> Bits/get(position: 1)
rightmost: bits |> Bits/get(position: 1, from: Right)
```

The default position 1 is the leftmost/most-significant bit. Hardware
specifications that count from the least-significant side use `from: Right`,
while still remaining one-based.

`Bits/get` returns `True | False` Tags. `Bits/set` consumes them.

### Core Operations

Initial operations:

- `Bits/width`;
- `Bits/get` and `Bits/set`;
- `Bits/slice` and `Bits/set_slice`;
- `Bits/concat`;
- `Bits/and`, `Bits/or`, `Bits/xor`, and `Bits/not`;
- logical left/right shifts;
- arithmetic right shift with explicit two's-complement interpretation;
- rotate left/right;
- zero extension;
- sign extension;
- explicit truncation;
- comparison with `Unsigned` or `TwosComplement` interpretation.

Shift amounts are quantities:

- they must be whole nonnegative Numbers;
- a statically invalid amount is a compile error and a dynamically invalid
  amount is a deterministic terminal API-domain error;
- shifting by zero is a no-op;
- a logical shift by at least the width yields zeros;
- an arithmetic right shift by at least the width yields all ones when the
  original most-significant bit is True and all zeros otherwise;
- rotate amounts are reduced modulo the width;
- no host undefined behavior is observable.

Operation widths are static:

- bitwise operations and shifts preserve `BITS[N]`;
- `Bits/concat` on `BITS[N]` and `BITS[M]` returns `BITS[N + M]`;
- the left concat operand remains the left, more-significant fragment;
- `Bits/slice(from:, count: C)` requires a positive compile-time `C` and
  returns `BITS[C]`;
- `Bits/set_slice(from:, value: BITS[M])` returns the original `BITS[N]`;
- mixed-width arithmetic is rejected until operands are explicitly extended or
  truncated;
- equal-width widening addition returns `BITS[N + 1]`.

A statically out-of-range `get`, `set`, `slice`, or `set_slice` is a compile
error. A dynamic invalid position is a deterministic terminal bounds error for
the default operation. Separately named `Bits/try_get`, `Bits/try_set`,
`Bits/try_slice`, and `Bits/try_set_slice` may return closed tagged outcomes
when recoverable bounds handling is needed. A replacement slice must fit
exactly; no truncation or extension is implicit.

### Arithmetic

Do not introduce an ambiguous default `Bits/add`.

Expose the result-width/overflow behavior in the operation:

```text
Bits/add_or_wrap
Bits/add_widening(interpretation:)
Bits/try_add(interpretation:) -> Added[value: BITS[N]] | Overflow
```

`add_or_wrap` is interpretation-independent modulo arithmetic.
`add_widening` extends its operands according to `Unsigned` or
`TwosComplement` and produces `BITS[N + 1]`.

Subtraction does not invent an ambiguous unsigned widening result for a
negative mathematical difference:

```text
Bits/subtract_or_wrap
Bits/try_subtract(interpretation:)
    -> Subtracted[value: BITS[N]] | Underflow | Overflow
```

An exact negative difference is computed as NUMBER or handled through the
checked tagged result. More widening arithmetic may be added only with an
equally explicit result width and interpretation.

For `Unsigned`, checked subtraction reports `Underflow` when the mathematical
result is negative. For `TwosComplement`, it reports `Overflow` when the signed
result is outside the representable `N`-bit range. Backends must not substitute
one Tag for the other.

`add_or_wrap` and `subtract_or_wrap` are exactly modulo `2^N` and behave
identically in debug, release, native, Wasm, GPU, and FPGA execution.

### Conversions

NUMBER to BITS is explicit and tagged:

```boon
number
|> Number/to_bits(
    width: 32
    interpretation: Unsigned
)
```

The width is a positive compile-time Number. The input must be whole.
`Unsigned` accepts `0` through `2^N - 1`; `TwosComplement` accepts
`-2^(N - 1)` through `2^(N - 1) - 1` and encodes the signed value in two's
complement.

```text
Converted[value]
NotWhole
OutOfRange
```

BITS to NUMBER names `Unsigned` or `TwosComplement`.

BYTES conversion:

- names byte order;
- is exact;
- requires a byte-aligned width unless an explicit padding operation is used;
- validates dynamic BYTES length against the expected static width.

TEXT has no direct BITS conversion. Encode TEXT to BYTES first.

### Matching

Only exact BITS literals are patterns:

```boon
opcode |> WHEN {
    BITS[7] { 2u0110011 } => RegisterInstruction
    __ => UnknownInstruction
}
```

Field extraction is explicit:

```boon
opcode:
    instruction
    |> Bits/slice(
        from: 1
        count: 7
    )
```

Masked matching is explicit composition:

```boon
masked: instruction |> Bits/and(with: mask)

masked |> WHEN {
    BITS[8] { 2u1010_0000 } => ...
    __ => ...
}
```

Do not add wildcard/captured subfields inside BITS patterns.

### Appropriate Uses

BITS is appropriate for:

- FPGA registers, buses, ALUs, counters, CRCs, and instruction words;
- packed protocols and file formats;
- cryptographic fixed words and rotations;
- compiler machine encodings and relocation masks;
- Wasm/GPU masks and packed pixels;
- formal SMT bitvector models.

BITS is not appropriate for:

- prices;
- durations;
- list positions;
- ordinary counts;
- arbitrary byte buffers;
- text;
- pointers, ownership, or permissions.

Fixed BITS state is valid in `HOLD`.

## One-Based Positions

All user-visible collection/text/byte/bit positions begin at one:

- LIST position 1 is the first occurrence;
- TEXT position 1 is the first Unicode scalar value;
- BYTES position 1 is the first byte;
- BITS position 1 is the leftmost bit by default.

TEXT grapheme-cluster navigation is a separate explicit API. Compiler/source
offsets use BYTES so diagnostics remain exact.

Positions must be whole Numbers at least one. A statically known zero,
negative, or non-whole position is a compile error. A dynamic value outside
that domain is a deterministic terminal API-domain error; it is never rounded
or clamped.

For LIST/TEXT/BYTES lookup, a positive whole position beyond the current length
returns `NotFound`. For fixed BITS, a position beyond the width is a bounds
error in the default operation; a separately named `Bits/try_*` operation is
used when that outcome is application data.

Counts and lengths are quantities:

- an empty collection has length zero;
- `Stream/skip(count: 0)` is valid;
- `Bits/shift_left(by: 0)` is valid;
- a LIST/TEXT/BYTES slice count may be zero if that API admits an empty result;
- a BITS slice count is at least one because `BITS[0]` is invalid.

Slices use a one-based starting position plus a count. They do not use a
zero-based half-open public range. For LIST/TEXT/BYTES, an exact slice is valid
when:

```text
1 <= from <= length + 1
0 <= count
from - 1 + count <= length
```

Thus `from: length + 1, count: 0` is the valid empty cursor at the end. A
default slice outside those bounds is a deterministic terminal bounds error; a
separately named safe operation may return a tagged result.

LIST positional mutations use the previous committed turn snapshot:

- update/remove source positions range from `1` through `length`;
- insert positions range from `1` through `length + 1`, where `length + 1`
  appends;
- move source and destination positions range from `1` through `length`;
- move destination names the moved occurrence's final position after the
  operation; moving a position to itself is a no-op.

Default mutation with a positive but out-of-range position is a deterministic
terminal bounds error. A recoverable application uses a separately named
tagged `try_*` operation. Concurrent structural operations still resolve
through authority conflict rules before these bounds are applied.

`List/range(from:, to:)` produces Number values over an inclusive range. Its
values may include zero or negative Numbers; it does not represent collection
positions.

External protocols that specify zero-based offsets convert explicitly at the
boundary.

## TEXT And BYTES

No new source cursor, span, or builder value kind is required.

Core scanning/building operations should include:

- `get(position:)`;
- `slice(from:, count:)`;
- `find(...) -> Found[position] | NotFound`;
- `split_once(...) -> Split[left, right] | NotFound`;
- exact encoding/decoding;
- bounded concat/join;
- bulk construction.

Source spans are ordinary data:

```boon
[
    start: one_based_byte_position
    length: byte_count
]
```

Lexers operate on BYTES for exact source locations and decode TEXT explicitly.

The compiler may lower non-escaping concat/join/build pipelines into mutable
buffers, ropes, slices, or arenas. This is an optimization and ownership
decision, not new syntax.

## LIST, SET, And MAP Authorities

### Authority Model

LIST, SET, and MAP own structural memory below the language boundary. Their
observable values are immutable structural snapshots plus semantic deltas.

Each logical turn:

1. reads the previous committed collection;
2. evaluates candidate operations;
3. resolves conflicts;
4. commits atomically;
5. publishes affected-key/item deltas;
6. wakes only affected observations and aggregates.

No partially committed collection is observable.

The collection authority is state. It is not copied into `HOLD`.

An authority is created by each checked LIST/SET/MAP construction expression
within its dynamic scope. Re-evaluation of that same expression scope addresses
the same authority; a separate function-call or row scope receives a distinct
authority. The checked expression path plus dynamic scope, not a host pointer,
is its persistent identity.

Bindings, object fields, function arguments, and function returns forward the
same authority. They do not copy a collection or create a second authority.
`Map/upsert`, `Set/add`, LIST mutation operations, and their remove/update
counterparts submit an operation to that authority and return its live public
view, so pipeline chaining continues to address the same authority.

If two aliases write the same authority in one logical turn, their operations
meet the ordinary causal conflict rules below. Aliasing never grants
last-scheduler-wins behavior. A collection-producing transformation such as
`List/map` creates a separate derived authority owned by that checked
transformation expression.

Aliasing an authority for reads/writes within its existing scope is distinct
from attaching it as structural state beneath another authority. V1 permits
each nested authority exactly one owning MAP-key or LIST-occurrence parent. It
must be constructed within that parent's value/row scope. Attaching an existing
authority under a second parent, constructing an authority cycle, or returning
a nested authority beyond its owner lifetime is a type/effect error.

Authority identity is not a public value. Equality and canonical value
serialization compare public contents only. Persistence attaches to the
authority creation path and dynamic scope, never to whichever alias happens to
name it.

### Syntax

```boon
items: LIST {
    first
    second
}

roles: SET {
    Admin
    Editor
}

users: MAP {
    alice_id => alice
    bob_id => bob
}
```

Semantics:

- LIST is ordered and permits duplicate occurrences.
- SET contains unique values; add/remove are idempotent.
- MAP contains at most one value for an equal key.

### MAP Upsert

The canonical write is:

```boon
map
|> Map/upsert(
    entry: [
        key: user_id
        value: user
    ]
)
```

Using one `entry:` value preserves key/value correlation.

These are semantically identical:

```boon
MAP {
    user_id => user
}
```

```boon
MAP {}
|> Map/upsert(
    entry: [
        key: user_id
        value: user
    ]
)
```

A constant pair emits one initial upsert.

A continuous pair emits:

- one initial upsert;
- another upsert whenever the combined key/value snapshot changes
  semantically;
- no operation for reevaluation that produces the same pair.

MAP stores committed scalar/value snapshots, not source references. If a value
contains a nested collection, that field forwards the nested authority identity
rather than snapshotting its contents; the nested authority remains scoped by
the enclosing MAP key.

Example:

```text
key A, value 1  -> Upsert(A, 1)
key B, value 1  -> Upsert(B, 1); A remains
key B, value 2  -> Upsert(B, 2)
key A, value 2  -> Upsert(A, 2)
```

A key change does not rename or remove an old address. Changing a real primary
key is an explicit remove plus upsert in one logical turn.

If the value contains a nested authority, a changing key cannot attach that
same authority under the retained old key and the new key. The producer must
construct a fresh nested authority in the new key scope, or explicitly remove
and migrate the old entry under a checked operation. A second-parent attachment
is rejected.

`THEN` samples one entry operation. Later unrelated dependency changes do not
change that committed entry.

Branch deactivation does not retract committed data. Removal is explicit:

```boon
map |> Map/remove(key: user_id)
```

Removing a missing key is idempotent. A later operation may reinsert it.

### Map/get

`Map/get` is a live address observation:

```boon
users
|> Map/get(key: selected_id)
|> WHEN {
    Found[user] => show(user: user)
    NotFound => show_missing()
}
```

It observes:

```text
present       -> Found[value]
replacement   -> Found[new_value]
removed       -> NotFound
reinserted    -> Found[new_value]
```

An observation key changing from A to B switches the observation to B.
Unrelated-key changes do not wake it.

The MAP has one inferred value shape `V`, possibly a closed tagged union.
`Map/get` therefore has:

```text
Found[value: V] | NotFound
```

A MAP does not change result type when a value is replaced. Heterogeneous
business cases use explicit Tags.

If a prior continuous producer changes again, that change is a new upsert. The
lookup is not subscribed to the old producer; it observes the newly committed
operation at the address.

### Key Eligibility And Equality

Initial key-safe values:

- NUMBER;
- TEXT;
- BYTES;
- BITS;
- bare Tags;
- bounded closed tagged payloads and objects recursively composed from
  key-safe values.

Reject as keys:

- LIST, SET, or MAP;
- SOURCE/SKIP/flow values;
- open or unbounded values without canonical encoding;
- host resources or capabilities.

There is no public `KEY`, `HASHABLE`, or comparator type. Key safety is a
compiler property.

Two keys are equal exactly when Boon structural equality says they are equal.
An internal hash collision has no semantic effect; implementations compare
complete keys before deciding equality.

### Conflicts

For one semantic MAP key or SET item in one logical turn:

- identical operations may coalesce;
- repeated removes may coalesce;
- causally ordered operations use the operation with the greater source-event
  sequence;
- conflicting operations with the same greatest sequence are a hard error;
- incomparable sequenceless conflicting writes are a hard error;
- hash iteration, worker timing, scheduler order, and source-text order never
  choose a winner.

Every statically equal duplicate MAP literal key is a compile error, even when
the values are equal. Runtime-identical operations may still coalesce.

Operations on different keys commit in the same turn.

### Ordering

MAP and SET equality are independent of insertion history.

Public enumeration, debugging, persistence, hashing, and canonical
serialization use canonical semantic key order, never hash-table order or
insertion order.

Canonical ordering is type-directed:

- NUMBER by numeric value;
- TEXT by exact UTF-8 byte sequence, with no implicit normalization;
- BYTES lexicographically;
- BITS lexicographically in displayed bit order;
- Tags by canonical UTF-8 Tag name followed by payload;
- closed objects by canonical field-name order followed by field values.

If insertion order is application data, store that order in a LIST.

### SET

```boon
SET {
    selected_role
}
```

has the same semantics as:

```boon
SET {}
|> Set/add(item: selected_role)
```

A changing item adds the new value and retains the old value until explicit
removal. Duplicate additions are idempotent.

`Set/contains` is a live observation returning `True | False` Tags.

SET elements follow the same key-eligibility and canonical-order rules as MAP
keys.

LIST cannot replace SET without changing:

- duplicate semantics;
- membership complexity;
- equality;
- delta conflicts;
- persistence shape;
- FPGA storage.

### LIST

LIST has one additional hidden implementation concept: a stable occurrence key.
It is required because duplicates and order are semantic.

A literal item creates one occurrence. A live item value may update that
occurrence. `List/append` creates a new occurrence for each append event.

The hidden occurrence key supports:

- stable row-local scalar `HOLD`;
- source routing;
- moves;
- stale-event rejection;
- delta generation.

It is not a Boon value and cannot be matched, compared, read, or included in
public/canonical value serialization. Internal persistence and delta-wire
metadata may encode occurrence identity and generation for routing and stale
event rejection.

LIST equality compares the ordered sequence of public item values. Hidden
occurrence keys never affect equality, hashing, or canonical serialization.

Canonical operations include:

- `List/get(position:) -> Found[value] | NotFound`;
- append;
- insert;
- update;
- remove;
- move;
- length;
- empty test;
- inclusive `List/range`.

Queue/stack/deque behavior is ordinary library composition over those
operations. It does not create new collection syntax.

### Nested Authorities

Collections may own nested collection authorities outside HOLD. A nested
authority is scoped by its enclosing MAP key or LIST occurrence and keeps its
own delta identity. The ownership graph is a finite acyclic tree even though
ordinary in-scope bindings may alias a node.

Removing the enclosing MAP key or LIST occurrence tombstones that nested scope
and advances its generation. Any stale delta addressed to the removed
generation is rejected. Reinserting an equal key or a new occurrence creates a
fresh nested authority generation; old contents do not resurrect unless an
explicit migration or restore operation supplies them.

Observers derived inside the removed scope deactivate with it. A stale
operation that somehow arrives after generation checking is rejected as an
internal stale-authority fault, not retargeted to a new generation. Recursive
equality, hashing, and canonical persistence terminate over the acyclic
ownership tree.

Targets may reject unbounded nesting or capacity. The semantic model does not
pretend a nested collection is a scalar snapshot.

### Collection-In-HOLD Rejection

Reject LIST, SET, or MAP recursively anywhere in HOLD state:

```boon
LIST {} |> HOLD state { ... }
```

```boon
[
    values: MAP {}
]
|> HOLD state { ... }
```

The check is type-directed. If any possible closed variant, open row, or nested
field can contain a collection authority, that HOLD state type is invalid.

Valid cases:

- scalar HOLD inside a LIST row scope;
- scalar or collection-free object HOLD inside a MAP key/value row scope;
- fixed BITS state;
- NUMBER, TEXT, BYTES, Tags, and collection-free objects in HOLD.

LIST-occurrence-local and MAP-key-local HOLD cells are owned by that parent
generation. Removing the occurrence/key destroys those cells; reinsertion
initializes fresh state. The HOLD cell itself is not flattened into the MAP
upsert snapshot, although its current collection-free output can participate in
the entry's public value and deltas.

A collection already owns state and a commit boundary. Wrapping it in HOLD
would obscure delta semantics, cause snapshot replacement, and fail to map
cleanly to bounded memories.

### Persistence And Physical Lowering

Durable collection state includes:

- current canonical contents;
- collection revision;
- operation/event deduplication frontier;
- hidden generations required for stale-delta rejection.

Distributed concurrency is serialized into authoritative Boon turns. MAP does
not silently become a CRDT. Offline multiwriter/CRDT semantics are deferred.

Possible physical lowerings:

- LIST: slot memory, validity/generation columns, and order vector;
- SET: keyed memory plus validity;
- MAP: ordered map, hash plus full-key comparison, CAM, perfect hash, or
  direct-address memory;
- dense bounded Number keys: array/BRAM lowering;
- server persistence: database upsert/delete operations and generated indexes.

Capacity exhaustion never evicts an unrelated entry. It is rejected at compile
time when provable and otherwise causes a deterministic terminal target error.

`TABLE` is persistent/indexed MAP policy, not a new value kind.

A hardware memory is a bounded dense LIST/MAP lowering plus target
port/latency policy, not a new value kind or source keyword.

Arena, builder, interner, and worklist performance come from lifetime,
uniqueness, escape, and access analysis. Function names do not receive secret
semantics.

## Bounded Repetition Without Loop APIs

Do not add:

- `Loop/*`;
- `List/fold`;
- `List/scan`;
- `List/reduce`;
- imperative `for` or `while`.

`WHILE` remains continuous conditional selection, not iteration.

Finite repeated state transition uses:

```boon
count
|> Stream/pulses()
|> THEN {
    next_state
}
```

with scalar/object state in HOLD.

Canonical Fibonacci:

```boon
FUNCTION fibonacci(position) {
    position
    |> THEN {
        position |> WHILE {
            1 => 1

            n =>
                [previous: 0, current: 1]
                |> HOLD state {
                    n - 1
                    |> Stream/pulses()
                    |> THEN {
                        [
                            previous: state.current
                            current: state.previous + state.current
                        ]
                    }
                }
                |> Stream/skip(count: n - 1)
                |> .current
        }
    }
}
```

The checked entry rejects non-whole positions and positions below one.

The outer `THEN` is intentional: it samples one position occurrence and creates
one activation-local state scope. Position 10 yields 55, and a later position
input starts again from the initializer rather than continuing from the prior
answer. There is no hardcoded Fibonacci table.

### Baseline Pulse Semantics

A `Stream/pulses` batch is one causal activation containing a frozen finite
count of ordered semantic microturns. The count is sampled once when the batch
starts; dependency changes during the batch affect a later source activation,
not the current count.

A HOLD initializer runs once per state-cell lifetime; `Stream/pulses` never
implicitly resets it. A HOLD constructed inside a `THEN` body is scoped to that
one input occurrence, remains alive through all child pulse microturns, and is
discarded when the body completes. Persistent state is written with HOLD
outside that activation-local `THEN`.

The `THEN` entry snapshot, including the pulse count and captured inputs, is
frozen for the batch. This dynamic scope is not a FLUSH lexical boundary.

HOLD publishes its valid initializer when that state-cell scope begins, then
processes each pulse as one microturn:

1. read the previous committed HOLD state;
2. evaluate one transition against one stable microturn snapshot;
3. resolve its candidates and effects;
4. commit the valid candidate;
5. publish that state before the next pulse begins.

No state commits during evaluation of a microturn. The commit occurs only when
that microturn succeeds, preserving the runtime's stable-snapshot rule while
making successive pulses observe successive committed states.

`Stream/skip(count: N)` can hide the first `N` emissions on that downstream
path. It does not retroactively remove HOLD emissions observed by another
branch, semantic state commits, deltas, or persistence records.

If pulse `k` FLUSHes, its candidate is discarded and pulses `k + 1` onward are
not evaluated. Commits from successful earlier pulse microturns remain. A later
independent source activation starts normally from the last successfully
committed state.

### Fusion

`HOLD + Stream/pulses` has the baseline microturn semantics above. Optimization
is opportunistic and must preserve that complete observable trace.

Fusion into a tight loop requires proof that:

- the pulse count for the activation is finite and frozen;
- work and state are within the target budget;
- state and intermediate values do not escape;
- no observer reads intermediate states;
- no persistence or authoritative delta observes intermediate states;
- no effect/log order changes;
- no FLUSH winner or failure timing changes;
- no collection conflict changes;
- no dynamic dependency is reread differently between pulses.

If proof fails, execute the baseline semantics. Do not reject ordinary HOLD
merely because it is not fusible.

Every fusion fixture runs with optimization disabled and enabled and compares:

- final value;
- emitted values;
- semantic delta trace;
- effects and order;
- FLUSH/failure result;
- persistence batch.

A compiler worklist is a LIST authority plus explicit work budget and ordinary
LIST operations. There is no public Worklist type. The same escape analysis may
lower a non-escaping worklist to a transient deque.

## FLUSH

### Purpose

`FLUSH` is explicit fail-fast control for a pipeline expression that cannot
produce a meaningful normal result after an error.

```boon
operation()
|> WHEN {
    Ok[value] => value
    error => FLUSH { error }
}
|> downstream()
```

If the error arm runs, `downstream()` is bypassed.

Use an ordinary tagged result when:

- the caller should decide locally;
- errors should be accumulated;
- collection items should fail independently.

Use FLUSH when:

- the remaining expression must not run;
- a collection operator should abort as a whole;
- a bounded pulse activation should stop.

Fatal arithmetic/resource errors do not implicitly become FLUSH.

### Public And Internal Model

Source syntax:

```boon
FLUSH { ErrorTag[field: value] }
```

The payload's static type must be a closed Tag, tagged object, or closed union
of those variants. Each runtime payload is one such variant. It must not
contain:

- LIST, SET, or MAP;
- SOURCE/SKIP/flow state;
- a host authority or capability.

Conceptual checked effect:

```text
Normal<T> + Flush<E>
```

Conceptual runtime envelope:

```text
Normal[value] | Flushed[payload]
```

`Flushed`/`FLUSHED` is not:

- a public Tag;
- a public type;
- matchable source syntax;
- serializable data;
- persistent state;
- a distributed wire value.

The typechecker tracks the escape effect internally. At its lexical boundary,
the user sees the ordinary closed result:

```text
T | E
```

### Lexical Boundaries

FLUSH bypasses the remainder of the current expression occurrence until one of
these boundaries:

- a named binding initializer;
- a user FUNCTION return;
- a BLOCK final result;
- the host/root result.

At the boundary, the hidden mark is removed and its payload becomes an ordinary
Tag/tagged object.

Collection `new:`/`if:` bodies and HOLD bodies are controlled subexpressions,
not FLUSH boundaries. Their owning operator receives the hidden status.

Function boundaries are intentional:

```boon
FUNCTION validate(value) {
    value |> WHEN {
        Valid[result] => Valid[value: result]
        error => FLUSH { error }
    }
}

validated: input |> validate()
```

`validated` receives the ordinary success-or-error value. A caller that wants
to continue fail-fast propagation explicitly matches and re-FLUSHes:

```boon
validated
|> WHEN {
    Valid[value] => value
    error => FLUSH { error }
}
|> next_step()
```

There is no implicit cross-function exception effect.

### HOLD

A potentially flushing HOLD initializer is invalid. A state cell must begin
with a valid storable value on every target.

If a HOLD body flushes:

1. the candidate is not stored;
2. the last valid committed state remains;
3. the FLUSH status propagates through the owning expression;
4. later independent triggers evaluate normally and may update the state.

FLUSH does not permanently poison a state cell.

### Collections

FLUSH inside a collection callback aborts the whole operator activation.

For `List/map`:

```text
item 1 -> value
item 2 -> FLUSH { InvalidItem[position: 2] }
item 3 -> value
```

the operator returns the error at its enclosing boundary. It does not return:

- a partial LIST;
- `LIST<Value | InvalidItem>`;
- a LIST with item 2 removed;
- a retained previous value presented as success.

Per-item isolation is written without FLUSH:

```boon
items
|> List/map(
    item
    new:
        item
        |> validate()
        |> WHEN {
            Valid[value] => Valid[value: value]
            error => InvalidItem[error: error]
        }
)
```

Deterministic first failure:

- LIST uses the lowest one-based semantic occurrence position;
- MAP/SET bulk operations use canonical key order;
- worker completion, scheduler, hash, GPU lane, or FPGA timing never chooses.

Parallel speculation is allowed only when:

- callbacks are pure;
- later speculative results/effects cannot commit;
- the canonical first failure is reproduced exactly.

Effectful fail-fast callbacks execute in semantic order or are rejected from
parallel/GPU lowering.

### Commit And Effects

Every source/root activation has a semantic activation ID. Nested function
calls, operator evaluations, callbacks, and pulse microturns receive child IDs.
Candidate state writes, collection operations, persistence deltas, and staged
effect intents are attributed to their activation path.

FLUSH propagates from its origin through the owning operator to the documented
lexical boundary. At each level it aborts that failed activation subtree.
Candidates and staged effects attributed to the aborted subtree are discarded;
successful sibling subtrees outside it remain eligible to commit.

Whole-operator collection fail-fast means the owning collection activation is
in the aborted subtree. Earlier speculative item candidates from that same
`List/map` or bulk MAP/SET activation are therefore discarded too. By contrast,
earlier successful pulse microturns are already separate committed child
turns, so a later flushing pulse does not roll them back.

A flushed authoritative activation must not publish a partial persistence batch
or semantic delta for its aborted subtree.

FLUSH is not general rollback. State/effects committed by earlier independent
activations remain committed. An external effect already dispatched before a
later FLUSH cannot be undone.

Effect intents are staged until their owning activation succeeds whenever the
host protocol permits it. The compiler must not move effects across a possible
FLUSH, and downstream effects must not dispatch after the FLUSH path is
selected. If a host effect cannot be staged, a region that might FLUSH after
dispatch is rejected unless an explicit boundary commits the earlier effect
first.

### Streams And Pulses

FLUSH aborts one causal activation:

- remaining bounded pulses created by that activation stop;
- prior successful pulse microturn commits remain;
- downstream work for that activation is bypassed;
- the permanent SOURCE/subscription remains active;
- a later independent event reevaluates normally.

FLUSH is not:

- permanent stream closure;
- automatic resource cleanup;
- automatic upstream cancellation;
- protocol backpressure;
- state migration;
- a hardware trap.

Hosts may build those protocols explicitly around tagged outcomes and effects.

### LATEST And Competing FLUSH Values

FLUSH does not gain global priority over unrelated branches.

`LATEST` first resolves candidate envelopes by the ordinary source-event
sequence/conflict rules. If the selected candidate is flushed, the hidden
status propagates.

Two conflicting flush payloads with the same winning sequence are a hard
conflict. Scheduler order never selects one.

### Target Lowering

Native/Wasm may represent FLUSH as an internal status plus payload.

A distributed/transport cut is not an implicit FLUSH boundary, and the hidden
status is never serialized. A compiler must keep a live-FLUSH region on one
side of the cut, place the cut after an existing documented lexical boundary
where the payload is ordinary data, or reject the partition.

FPGA may use a bounded status/control sideband only after proving payload width,
operator order, and commit suppression.

GPU lowering is eligible only when:

- work is pure and bounded;
- deterministic first-failure reduction is available;
- speculative later lanes have no observable effects;
- no global stream/resource cancellation is implied.

Otherwise the region remains on CPU or GPU lowering is rejected.

Do not specify FLUSH as “one extra bit per value.” Representation depends on
payload type, ownership, and target control flow.

### Chosen FLUSH Semantics

The sibling `~/repos/boon/docs/language/FLUSH.md` describes whole-operator
fail-fast. The experimental
`~/repos/boon_experiments/docs/new_boon/2.6_ERROR_HANDLING.md` later describes
item-isolated collection behavior.

This plan chooses whole-operator fail-fast.

Those conflicting drafts are research evidence, not compatibility contracts.
When the language repositories are synchronized to this plan, rewrite or delete
the conflicting documents; do not leave both semantics presented as usable.

The distinction is intentional:

- ordinary error Tag in an item result means item isolation;
- FLUSH means abort the enclosing operator activation.

Any backend that parses FLUSH and lowers it as an ordinary passthrough is
incorrect. Unsupported FLUSH must be a compile error, never a no-op.

## Executable FLUSH Example

FLUSH currently appears in:

- `examples/todo_mvc_physical/BUILD.bn`;
- `examples/novywave/BUILD.bn`.

Those files are manifest `build_files`. Current manifest validation only checks
that they exist, and the active circuit compiler does not implement FLUSH.
Therefore they do not count as executable language coverage.

The FLUSH implementation phase adds atomically:

```text
examples/flush_error_propagation.bn
examples/flush_error_propagation.scn
```

and a catalog entry using `examples/basic_examples.budget.toml`.

The example must exercise:

1. successful normal pipeline completion;
2. downstream bypass after FLUSH;
3. named-binding-initializer boundary unwrapping;
4. FUNCTION return followed by explicit caller re-FLUSH;
5. BLOCK-final-result boundary unwrapping;
6. host/root-result boundary unwrapping and reporting;
7. ordinary WHEN handling after each exposed payload;
8. HOLD update failure preserving prior state;
9. later valid HOLD recovery;
10. whole-List/map fail-fast;
11. ordinary tagged per-item isolation without FLUSH;
12. deterministic first failure;
13. no partial output or downstream effect.

The example must use `True` and `False` as Tags and must not expose `FLUSHED`.

The same phase updates the two BUILD sources and their comments to this
contract. They receive parser/typechecker fixtures immediately, but they do not
count as executable FLUSH coverage until the Build capability profile actually
compiles and runs them.

BUILD files receive a separate compile/typecheck gate under a future Build
capability profile. File presence never counts as semantic coverage.

## Language Feature Coverage

There is currently no authoritative mapping from public language constructs to
compiled examples and tests.

Add:

```text
examples/language_feature_coverage.toml
```

The parser owns the canonical registry of implemented language feature IDs and
spellings. The coverage manifest maps each implemented public feature ID to:

- at least one positive source fixture;
- an executable example or conformance scenario;
- parser coverage;
- typechecker coverage;
- IR/compiler coverage;
- runtime coverage when the feature is executable;
- persistence/wire coverage when it is serializable/stateful;
- target eligibility/rejection coverage;
- negative fixtures where misuse is meaningful.

Add:

```text
cargo xtask verify-language-surface
```

The gate fails when:

- an implemented public feature lacks a coverage entry;
- a coverage entry names an unimplemented feature;
- a referenced fixture is not compiled;
- a keyword appears only in comments/TEXT;
- a BUILD file is counted without being parsed/typechecked;
- parser, formatter, and inspector spellings disagree;
- an internal-only representation appears as a public feature.

`examples/flow_operators.bn` remains useful coverage for SOURCE, HOLD, LATEST,
THEN, WHEN, and WHILE. FLUSH receives its own executable example because its
failure and commit behavior require scenarios.

This registry is a coverage contract, not a reason to add one showcase
application per token. Small conformance fixtures may cover low-level syntax;
user-facing examples cover behavior worth teaching.

## Compiler Artifact Spine

Every compiler and target path uses one authoritative artifact sequence:

```text
ParsedProgram
-> CheckedProgram
-> SemanticProgram
-> ContractVerifiedProgram
-> ErasedProgram
-> MachinePlan
-> PhysicalPlan or CoreHardwareIR
```

The boundaries and owners are fixed:

- `boon_syntax` owns syntax vocabulary, AST/source DTOs, source spans, and the
  canonical language-feature registry. `boon_parser` owns parsing, validation,
  formatting, tracing, and opaque `ParsedProgram` issuance.
- `boon_typecheck` produces `CheckedProgram` with resolved declarations,
  structural types, flow types, and typed calls.
- `boon_semantic` produces `SemanticProgram` by expanding contextual
  functions, validating `OutNet`, assigning semantic ownership, building typed
  list views and dependency manifests, and recording proof obligations.
- `boon_verify` is the mandatory gate and produces
  `ContractVerifiedProgram` only after every mandatory obligation is
  discharged. Programs without an authored `WHERE` clause still pass through
  the gate and receive a verified empty-authored-contract record plus all
  compiler-generated safety obligations; rejection or unsupported mandatory
  proof produces diagnostics and no verified artifact.
- `boon_ir` produces `ErasedProgram` by erasing `WHERE`, `OUT`, `PASS`, and
  transparent wrappers only after verification. It does not rediscover or
  weaken verified semantics.
- machine planning consumes only `ErasedProgram`. Portable execution proceeds
  through `MachinePlan` and target-selected `PhysicalPlan`; eligible hardware
  proceeds through `CoreHardwareIR`.

No runtime, persistence layer, native/Wasm backend, GPU path, FPGA path, or
hardware backend may consume parser AST, bypass `ContractVerifiedProgram`, or
reconstruct contextual semantics. Debug/source provenance may be retained as
non-authoritative metadata, but executable identity and behavior derive from
the artifact sequence above.

## General Engine Libraries Without New Types

The compiler and other graph-processing workloads need:

- source scanning;
- a canonical source formatter/emitter;
- spans and diagnostics;
- stacks and work queues;
- flat syntax/type arenas;
- symbol tables and interning;
- maps and sets;
- sorting;
- graph traversal;
- SCC/topological algorithms;
- union-find;
- fixed-point/worklist evaluation;
- canonical encoding.

Representations:

```text
source bytes        -> BYTES
canonical source    -> TEXT/BYTES built with bulk builders
span                -> [start, length]
node id             -> tagged NUMBER
flat arena          -> LIST of tagged nodes
symbol table        -> MAP from TEXT/tagged key to tagged id
visited             -> SET of tagged id
work queue          -> LIST
diagnostics         -> LIST of tagged diagnostic objects
target word         -> BITS[N]
```

Opaque typed IDs and named module signatures may improve large interfaces
later. They are not required for the first Boon compiler; Tags provide an
adequate initial distinction:

```boon
NodeId[value: 42]
SymbolId[value: 7]
```

Graph algorithms live in ordinary engine/library modules. Sorting, SCC,
topological traversal, union-find, canonical encoding, and queue helpers are
not syntax and do not receive compiler-name-based shortcuts.

The optimizer may lower non-escaping authorities to:

- dense vectors;
- mutable hash/ordered maps;
- bitsets;
- transient deques;
- bump arenas;
- string/byte builders.

It must prove that:

- the authority does not escape;
- incremental observation is not visible;
- persistence is not attached;
- hidden identity remains unobservable;
- the lowered result and failure trace are equivalent.

## Foundation Delivery Stages

1. Land canonical Tags-only truth and Tag encoding.
2. Land fail-closed matching and one-based positions.
3. Land FLUSH and its commit/effect semantics.
4. Land exact NUMBER.
5. Land MAP/SET and extended delta/persistence protocols.
6. Land BITS.
7. Land generic escape/uniqueness analysis and transient collection lowering.
8. Land verified `HOLD + Stream/pulses` fusion.

These stages harden the ordinary compiler/runtime stack; they do not build or
bootstrap a compiler written in Boon. Their acceptance is the implementation,
deletion, differential, and target-validation work specified below.

## Target Contracts

### All Targets

Reject or retain on an eligible host rather than changing semantics when the
target cannot prove:

- exact NUMBER representation;
- BITS operation support;
- collection capacity/work bounds;
- deterministic conflict resolution;
- FLUSH order and commit behavior;
- equivalent observable values, deltas, effects, and failures.

Debug/release/native/Wasm/GPU/FPGA behavior must not differ in arithmetic,
shift, ordering, conflict, or failure semantics.

Every differential or equivalence run records the same versioned execution
profile and budget digest on all compared targets. A GPU or FPGA target may
reject a region before execution. Its eligibility proof must show that an
accepted region cannot exhaust the profile's Number, memory, or work budget.

Targets may offer profiles with different limits, so admission may differ
between profile digests. Once a run is admitted under the same digest, values,
deltas, effects, ordering, and failure behavior must agree.

### Native Interpreter And Optimized CPU

The software runtime supports the complete language subject to explicit
resource budgets:

- arbitrary-precision rationals;
- arbitrary fixed BITS widths through multiword storage;
- dynamic LIST/MAP/SET authorities;
- full FLUSH semantics.

The optimized interpreter remains the semantic oracle. Native-region lowering
is optional and must be translation-validated against it.

### Wasm

Wasm implements software semantics in linear memory.

- exact NUMBER never crosses JavaScript as JS `Number`;
- NUMBER uses canonical numerator/denominator encoding;
- BITS carries width plus canonical bytes;
- collection order never relies on JavaScript object iteration;
- arithmetic and collection implementation remains in Rust/Wasm or equivalent
  canonical code.

### GPU

Only pure bounded regions are eligible.

Reject GPU lowering for:

- arbitrary rationals without proven exact finite representation;
- dynamic authority/allocation;
- persistence or host effects;
- unbounded pulses;
- unsupported BITS operations;
- FLUSH without deterministic pure first-failure reduction.

Exact integer/fixed-rational Number regions may lower after range and
denominator-closure proofs.

Approximate f32/f64 arithmetic requires a future explicit approximation
contract. It must never be inferred from NUMBER.

Rendering may quantize an exact document Number only at an explicit host/render
boundary with documented rounding. That conversion is not Boon arithmetic.

### FPGA

Require compile-time/profile bounds for:

- LIST/MAP/SET capacity and nesting;
- TEXT/BYTES length;
- pulse counts and total microsteps;
- NUMBER numerator/denominator widths;
- BITS width;
- collection ports and latency;
- bulk traversal;
- FLUSH payload layout;
- delta buffers and queues.

MAP lowering must prove collision handling and worst-case access behavior.
Canonical MAP/SET traversal may use bounded scanning or sorting.

FLUSH is an expression-bypass control path. It is not automatically a
ready/valid protocol, backpressure, interrupt, or trap.

### Server And Persistence

Server authority sequences turns deterministically.

Persist:

- exact normalized NUMBER;
- BITS width plus canonical bytes;
- canonical MAP/SET contents;
- complete-turn collection operations and generations.

Crash injection must prove that a flushed or failed authoritative activation
does not leave a partial persistence batch.

Offline multiwriter/CRDT behavior is deferred.

## Formal Verification Contract

Model:

- NUMBER as mathematical rationals plus definedness/resource side conditions;
- BITS as fixed-size bitvectors;
- `True` and `False` as Tag constructors;
- FLUSH as an internal control sum/effect;
- bounded LIST/MAP/SET authorities as transition systems;
- collection-free scalar/object HOLD with induction;
- ordered pulse microturns with prior successful commits;
- FLUSH-aborted activation subtrees and staged-effect suppression;
- collection invariants separately from HOLD induction.

An SMT prover may use its internal Boolean sort. Public proof diagnostics and
source models still describe the Tags `True | False`.

Required proof/validation boundaries:

- source proof does not imply backend correctness;
- each optimization identifies source IR, target IR, assumptions, and
  validator;
- exact NUMBER specialization includes range/scale proof;
- BITS operations compare against SMT bitvector semantics;
- generated RTL receives bounded equivalence or translation validation;
- `HOLD + pulses` fusion has trace equivalence;
- collection operations preserve authority invariants;
- native/Wasm run differential semantic traces;
- unsupported proof fragments report `unsupported`, never guessed success.

The separate formal-verification plan must be reconciled with this foundation:

- replace binary64/`FiniteReal` assumptions;
- remove `NaN` examples;
- replace public Boolean terminology with `True | False` Tags;
- add BITS, MAP/SET authority, activation-tree FLUSH, pulse microturn commits,
  collection-free object HOLD, and exact-resource side conditions.

That file is maintained independently and must not be overwritten as part of
this plan's creation.

## Operating-System Boundary

This data model is not sufficient to claim a safe Boon kernel.

Defer:

- linear/affine capabilities and ownership;
- pointers, provenance, and address spaces;
- volatile/MMIO semantics;
- atomics and memory ordering;
- interrupts, scheduling, and preemption;
- DMA/IOMMU and device lifetimes;
- privilege transitions, boot, ABI, and linking;
- unsafe FFI;
- allocator, stack, and no-panic contracts;
- separation logic and concurrency proofs;
- hard real-time/WCET guarantees.

BITS can encode an address. It cannot prove permission, lifetime, exclusivity,
or volatility.

MAP is not physical memory.

FLUSH is not a processor trap.

Those capabilities require a separate resource/effect architecture after the
ordinary language and compiler/runtime contracts are stable.

## Example Migration Portfolio

### Existing Examples

#### Fibonacci

Replace the hardcoded zero-based LIST table in `examples/fibonacci.bn` with the
one-based `HOLD + Stream/pulses + Stream/skip` implementation.

Generate displayed positions from 1 through 10. Position 10 remains 55.

#### TodoMVC Physical

Replace the apparent `LIST { theme, mode }` pattern in
`examples/todo_mvc_physical/Theme/Theme.bn` with nested Tag selection.

The sibling selected/hovered LIST pair becomes nested `True`/`False` Tag
selection.

Outputs and scenarios remain unchanged.

#### Cells

Replace:

- every `NaN` branch;
- zero-based LIST/TEXT/BYTES access;
- A0-style spreadsheet addresses.

Use:

- `Parsed[value] | InvalidNumber[reason, position]`;
- `Found[position] | NotFound`;
- one-based A1-style addresses;
- explicit formula `DivisionByZero` handling.

Shift the scenarios consistently, including seeded cells and the first empty
input row. Include the zero-origin generator in `examples/cells/model.bn` and
the documented formulas in `docs/examples/CELLS_CIRCUIT_STYLE.md`; neither may
continue teaching A0-origin indexing.

#### NovyWave

Replace `NaN` numeric fallback with tagged parsing.

Use one-based TEXT/BYTES positions.

Runtime-width uploaded waveform data remains BYTES plus an explicit bit count
or a tagged waveform representation. It does not become `BITS[N]` until width
is statically known.

#### BYTES Fixtures And HTTP Echo

Migrate or delete every repository `examples/bytes_*` fixture, scenario,
compiler/runtime test, generated artifact, and document that uses zero-based
`index:`/`offset:`. Rewrite `docs/architecture/BYTES_SEMANTICS.md` to the
one-based contract. Manifest membership or an “inactive” label is not an excuse
to keep an old API example.

Migrate `examples/server_http_echo.bn` path access to position 1.

### New Checkout/Order Example

Add one application-shaped example combining:

- a MAP catalog from tagged SKU to live product/price;
- a SET of applied coupons or permissions;
- a LIST basket;
- exact prices and explicit rounding;
- scalar selected-SKU HOLD;
- no collection inside HOLD.

It proves:

1. MAP literal and `Map/upsert` trace equivalence;
2. live `Map/get`;
3. replacement, removal, and reinsertion;
4. changing write key adds a new address;
5. injected internal hash collision safety;
6. SET idempotence;
7. LIST order and duplicates;
8. canonical MAP/SET enumeration;
9. exact `0.10 + 0.20`;
10. delta-native persistence.

This replaces separate toy MAP, SET, and exact-decimal examples.

### BITS Portfolio

#### Priority Encoder

Use one-based `Bits/get` or explicit mask/slice operations. Do not preserve
fixed LIST structural patterns or add wildcard BITS patterns.

#### ALU

Exercise:

- bitwise logic;
- shifts and rotates;
- exact equality returning True/False Tags;
- `add_or_wrap`;
- widening and checked arithmetic;
- explicit signed/unsigned interpretation.

#### LFSR

Use fixed BITS in HOLD and one-based access. Hardware tap notation that counts
from LSB uses `from: Right`.

#### Ripple Adder

Retire or rename the misleading serial-adder fixture. A fixed ripple-adder
conformance fixture may show explicit gate topology without adding a generic
loop surface.

#### Protocol Header

Add a non-FPGA fixture that:

- packs fixed opcode/flag/length fields;
- converts BITS/BYTES with named byte order;
- decodes with explicit slices/masks and exact literals;
- runs identically on native and Wasm.

### FLUSH Example

Add `flush_error_propagation` exactly as specified in the FLUSH section.

The two current BUILD files remain useful real uses but do not count as
executable coverage until the Build profile compiles them.

## Negative Fixtures

Add compile-fail fixtures for:

- visible/public `BOOL` assumptions;
- `NaN`;
- LIST/SET/MAP/object/type patterns;
- nested/destructuring/masked BITS patterns;
- implicit NUMBER-to-BITS conversion;
- invalid BITS width or literal fit;
- negative/non-whole shift and rotate amounts;
- zero/negative rounding quantum;
- position zero in positional APIs;
- negative positions;
- non-whole positions;
- LIST/SET/MAP recursively inside HOLD;
- nested-authority second parent, cycle, or lifetime escape;
- potentially flushing HOLD initializer;
- FLUSH payload containing a collection or host authority;
- matching, persisting, or serializing `FLUSHED`;
- duplicate MAP literal keys;
- unresolved same-turn MAP/SET conflicts;
- unbounded FPGA/GPU lowering;
- an optimizer accepting trace-changing pulse fusion.

Unsupported features fail closed. No target or legacy engine may accept them as
passthrough/no-op behavior.

## Implementation Phases

### Phase 0: Freeze And Inventory

- Land this plan.
- Freeze the authoritative
  `ParsedProgram -> CheckedProgram -> SemanticProgram ->
  ContractVerifiedProgram -> ErasedProgram` ownership boundary and inventory
  every compiler/runtime/backend entry point that bypasses it.
- Add the syntax-owned feature registry design.
- Add `examples/language_feature_coverage.toml` and its verifier skeleton.
- Record current parser/type/value/schema fingerprints.
- Inventory all zero-based, `NaN`, public Bool, binary64, and pattern uses.
- Add a machine-checked inventory across every workspace crate for
  `FiniteReal`; `Value::{Bool, Null, Error}`;
  `StoredValue::{Bool, Null, Error}`; `DataTypePlan::{Bool, Null, Error}`;
  `PlanValueType::Bool`; `PlanConstantValue::Bool`;
  `boon_effect_schema::ValueType::Bool`; executor/runtime `EvalValue`
  sentinels; wire `TAG_FALSE`/`TAG_TRUE`/`TAG_NULL`/`TAG_ERROR`; and the
  privileged `Error/new`/`Error/text` builtins and call sites.
- Define the canonical source formatter/emitter and its idempotence corpus.
- Add fail-closed tests for currently misleading compound patterns.
- Classify every conflicting active document as rewrite-or-delete. Preserve
  unrelated sections only by rewriting the file to this contract; delete a
  wholly superseded file instead of leaving a historical disclaimer.
- Add per-phase zero-legacy gates for every repository Boon source outside
  dedicated compile-fail inputs, plus Rust identifiers, schemas, wire tags,
  fixtures, examples, documents, and generated artifacts.

Exit: the repository can distinguish current implementation from planned
surface, cannot claim keyword coverage from file presence, and has a complete
deletion ledger for every replaced representation/API.

### Phase 1: Tags, Matching, And FLUSH

- Parse True/False as ordinary Tags.
- Delete public Bool patterns, types, diagnostics, AST/IR variants, schemas, and
  runtime value branches. A new private compact lowering may represent the
  closed `True | False` Tag set only after those old paths are gone.
- Separate private absence/fault channels from source `Null`/`Error` Tags.
- Delete canonical `Value::{Bool, Null, Error}`,
  `StoredValue::{Bool, Null, Error}`, `DataTypePlan::{Bool, Null, Error}`, old
  executor/runtime sentinels, and their wire tags. Backend-private bits,
  presence envelopes, and fault channels must be newly named,
  nonserializable representations with no old decoder.
- Remove the privileged `Error/new` and `Error/text` builtins. Migrate Cells
  and other callers to specific ordinary Tags and explicit payload fields.
- Make unsupported structural/type patterns targeted compile errors.
- Implement the seed canonical formatter for the supported Phase 1 source.
- Lock delimiter-depth grammar machinery and negative ambiguity diagnostics;
  positive MAP/WHEN formatter round-trips land with MAP in Phase 3.
- Add checked FLUSH effect and lexical boundaries.
- Add IR/runtime FLUSH propagation, HOLD behavior, collection fail-fast, and
  commit suppression.
- Add `flush_error_propagation` and executable coverage.

Exit: FLUSH cannot be silently ignored; every supported path has deterministic
native semantics, and no old Bool/Error/FLUSH passthrough path remains.

### Phase 2: Exact NUMBER And One-Based Positions

- Replace `FiniteReal(f64)` with canonical rational storage.
- Replace NUMBER hashing, ordering, persistence, wire encoding, and schema
  fingerprints.
- Add resource budgets.
- Add exact rational rounding.
- Reject nonpositive rounding quanta statically or terminally as specified.
- Record certified rounding as the prerequisite for later square-root and
  transcendental operations; do not approximate them in this phase.
- Replace parsing/formatting.
- Remove NaN pattern syntax.
- Remove every NaN sentinel.
- Delete every `FiniteReal` representation, f64 semantic branch, zero-based API
  spelling, old persistence/wire encoding, and obsolete golden fixture.
- Migrate LIST/TEXT/BYTES positions and examples.
- Update Cells, Fibonacci, NovyWave, HTTP echo, and BYTES fixtures.

Exit: exact and one-based golden tests pass; repository gates prove no old
profile, alias, decoder, fixture, or runtime branch remains.

### Phase 3: MAP And SET Authorities

- Add parser/typechecker/value/IR/plan/runtime representation.
- Add delimiter-safe MAP literal parsing and canonical MAP/WHEN formatter
  round-trips while continuing to reject MAP patterns semantically.
- Add committed operations, deltas, persistence, wire encoding, and editor
  inspection.
- Add canonical equality/order and collision-safe lookup.
- Add `Map/upsert`, `Map/remove`, `Map/get`, `Set/add`, `Set/remove`, and
  `Set/contains`.
- Add recursive collection-in-HOLD rejection.
- Add nested authority ownership/generation handling.
- Reject multi-parent, cyclic, and lifetime-escaping nested authorities.
- Add checkout/order example.

Exit: single-key changes remain keyed deltas and do not copy whole collections.

### Phase 4: BITS

- Add arbitrary-width literal parsing without routing through u8 storage.
- Add width-aware static types.
- Add canonical value/persistence/wire representation.
- Add core operations, exact bounds behavior, and explicit conversions.
- Add exact-literal matching.
- Add hardware and protocol fixtures.
- Add exhaustive small-width and property/differential tests.

Exit: native/Wasm results match; bounded RTL checks agree with bitvector
semantics.

### Phase 5: Generic Transient Lowering

- Add authority escape/lifetime analysis.
- Lower non-escaping compiler maps, sets, lists, queues, arenas, and builders to
  efficient mutable storage.
- Treat the compatible compiler-hot-path subset already landed under
  `BOON_COMPILER_PERFORMANCE_PLAN.md` as an implementation input, then prove
  that it follows the general lifetime, uniqueness, escape, and access rules
  here rather than preserving a compiler-name special case.
- Preserve public immutable/delta semantics.
- Prohibit lowering when incremental observation, persistence, or escape makes
  it visible.
- Add allocation and snapshot-copy budgets.

Exit: compiler-shaped benchmarks do not rebuild/copy full collections per
token or node.

### Phase 6: Pulse Fusion

- Add bounded `HOLD + Stream/pulses` recognition in checked IR.
- Prove complete trace equivalence.
- Add disabled/enabled differential runs.
- Test repeated Fibonacci inputs to prove fresh activation-local initialization.
- Add negative eligibility diagnostics.

Exit: Fibonacci and compiler worklist fixtures accelerate without semantic
change.

### Phase 7: Target And Formal Validation

- Complete native/Wasm differential suites.
- Prove every compiler entry point passes through
  `ContractVerifiedProgram`; reject direct parser/checked-program backend
  lowering and unverified no-WHERE shortcuts.
- Add exact Number proof summaries.
- Add BITS/RTL equivalence checks.
- Add collection invariant checks.
- Add FLUSH transition/commit models.
- Add GPU/FPGA eligibility and rejection reports.

Exit: unsupported regions are reported, never approximated or silently run
with changed semantics.

## Verification Matrix

### Parser And Formatter

- every implemented feature has positive syntax coverage;
- every rejected pattern has a targeted diagnostic;
- True/False parse as Tags;
- FLUSH has a dedicated AST/effect path;
- MAP arrows round-trip inside WHEN outputs;
- formatting is idempotent and parse-format-parse preserves the canonical AST;
- future-looking MAP pattern input fails for unsupported semantics, not arrow
  ambiguity;
- arbitrary-width BITS literals round-trip;
- zero, negative, and non-whole position diagnostics identify positions rather
  than generic Number errors.

### Typechecker

- closed `True | False` Tag inference;
- tag-payload binding fields;
- no runtime type matching;
- exact NUMBER refinements;
- BITS width agreement;
- homogeneous MAP key/value and SET element constraints;
- canonical key safety;
- recursive collection-in-HOLD rejection;
- single-parent acyclic nested-authority ownership;
- FLUSH effect and boundary union;
- potentially flushing HOLD initializer rejection;
- target bounds and eligibility facts.

### IR And Compiler

- no public Bool/NaN pattern variants;
- FLUSH cannot lower as passthrough;
- hidden FLUSH status cannot escape boundary erasure;
- collection operations carry typed authority/key identities;
- MAP/SET deltas are first-class;
- pulse fusion records proof assumptions;
- target lowering consumes checked semantics, not function-name strings.

### Runtime

- exact rational arithmetic and deterministic budgets;
- one-based position behavior;
- keyed Map/get currentness;
- conflict resolution independent of scheduler/hash order;
- no collection snapshot inside HOLD;
- activation-local HOLD inside THEN resets for each input occurrence;
- persistent HOLD outside THEN retains its state across activations;
- deterministic FLUSH first failure;
- every named-binding, FUNCTION, BLOCK, and host/root FLUSH boundary unwraps;
- no partial aborted collection/state commit;
- later activation recovery;
- no whole-collection copy for one-key edit.

### Persistence And Wire

- normalized equivalent Numbers serialize identically;
- BITS encodes width and canonical bytes;
- MAP/SET serialize in canonical order;
- hidden LIST occurrence keys/generations remain internal except required
  protocol metadata;
- FLUSHED never serializes;
- crash injection proves complete-turn atomicity;
- new decoders accept only the new schema/encoding; old bytes fail ordinary
  schema validation without invoking an old decoder or migration branch.

### Target Differential Tests

- native and Wasm produce identical values, deltas, effects, and failures;
- exhaustive small BITS widths match reference semantics;
- GPU-eligible regions match CPU;
- GPU-ineligible regions report why;
- FPGA bounded models match interpreter traces;
- debug and release never differ in wrapping, shifts, order, or conflicts.

### Example Scenarios

- `flush_error_propagation`;
- Fibonacci;
- Cells;
- NovyWave;
- TodoMVC Physical;
- checkout/order collections;
- BITS ALU/LFSR/priority/ripple fixtures;
- protocol header.

## Recommended Repository Gates

Focused implementation tests:

```bash
cargo test \
  -p boon_parser \
  -p boon_typecheck \
  -p boon_ir \
  -p boon_data \
  -p boon_plan \
  -p boon_compiler \
  -p boon_plan_executor \
  -p boon_runtime \
  -p boon_persistence \
  -p boon_wire \
  -p boon_example_manifest \
  -p xtask
```

Direct downstream consumers:

```bash
cargo test \
  -p boon_effect_schema \
  -p boon_document_model \
  -p boon_document \
  -p boon_list_access \
  -p boon_host \
  -p boon_host_runtime \
  -p boon_http_runtime \
  -p boon_wellen_host \
  -p boon_server_runtime \
  -p boon_native_gpu \
  -p boon_web_effect_host \
  -p boon_web_host \
  -p boon_native_playground
```

Final workspace regression:

```bash
cargo test --workspace
```

Language coverage:

```bash
cargo xtask verify-language-surface
```

The implementation phases must also add focused scenario commands for the new
examples and use the repository's current native GPU manifest only when a
changed example is part of the native handoff surface.

Documentation-only work does not require native GPU handoff reports.

## Repository Touchpoints

### Current Semantic Sources To Replace

- `docs/architecture/LANGUAGE_SEMANTICS.md`
  - binary64 Number;
  - current positions;
  - LIST-only collection surface;
  - no FLUSH contract.
- `docs/architecture/BYTES_SEMANTICS.md`
  - zero-based index/offset examples.
- `docs/architecture/NUMBER_SPECIALIZATION_EXPERIMENT.md`
  - delete it from the active tree because its semantic conclusion measures the
    removed representation strategy;
  - carry any still-useful benchmark method into a new exact-NUMBER benchmark
    document rather than retaining a contradictory historical file.
- `docs/architecture/LIST_MODEL.md`
  - extend authority model to MAP/SET and collection-in-HOLD rejection.
- `docs/architecture/DELTA_PROTOCOL.md`
  - add MAP/SET operations and aborted-turn rules.
- `docs/architecture/RUNTIME_MODEL.md`
  - add exact values, MAP/SET stores, FLUSH, and transient lowering.

### Plans To Reconcile

- `docs/plans/TYPE_INFERENCE_AND_TYPECHECKING_PLAN.md`
  - already correct that True/False are Tags;
  - add exact Number, BITS, MAP/SET, FLUSH effects, and pattern restrictions.
- `docs/plans/TYPED_LIST_PIPELINES_AND_QUERY_REMOVAL_PLAN.md`
  - replace finite Number/public Bool key language;
  - copy forward only lowering that passes the new semantic, representation,
    and zero-legacy gates; replace or delete every other part.
- `docs/plans/BOON_PERSISTENCE_ARCHITECTURE_PLAN.md`
  - extend LIST-only authority deltas to MAP/SET;
  - add exact Number/BITS encodings and FLUSH atomicity.
- `docs/plans/BOON_EXAMPLE_PORTFOLIO_PLAN.md`
  - replace binary64 assumptions;
  - add the compact replacement/conformance portfolio.
- `docs/plans/BOON_FORMAL_VERIFICATION_AND_WHERE_PLAN.md`
  - independently maintained;
  - replace binary64/NaN/public Boolean assumptions;
  - add the new formal models.

### Compiler/Runtime Blast Radius

- `crates/boon_data`;
- `crates/boon_parser`;
- `crates/boon_typecheck`;
- `crates/boon_ir`;
- `crates/boon_plan`;
- `crates/boon_compiler`;
- `crates/boon_plan_executor`;
- `crates/boon_runtime`;
- `crates/boon_persistence`;
- `crates/boon_wire`;
- `crates/boon_example_manifest`;
- `crates/xtask`;
- `crates/boon_effect_schema`;
- `crates/boon_document_model`;
- `crates/boon_document`;
- `crates/boon_list_access`;
- `crates/boon_host`;
- `crates/boon_host_runtime`;
- `crates/boon_http_runtime`;
- `crates/boon_wellen_host`;
- `crates/boon_server_runtime`;
- `crates/boon_native_gpu`;
- `crates/boon_web_effect_host`;
- `crates/boon_web_host`;
- editor and inspector paths;
- `crates/boon_native_playground`;
- native and Wasm hosts/targets;
- bounded GPU/FPGA paths.

BITS, MAP, and SET are end-to-end additions. They must not land as
parser-only syntax or debug-only generic values.

The Phase 0 inventory gate scans the entire workspace, generated schemas,
fixtures, and golden encodings for every old Number/Bool/Null/Error
representation and builtin enumerated in Phase 0. The corresponding
implementation phase requires zero executable occurrences. Dedicated
compile-fail inputs may contain removed source spellings, and inert invalid-byte
fixtures may contain an old encoding, solely to prove rejection. They are never
linked, parsed/decoded as valid data, or placed on an active example or golden
path.

## Acceptance Criteria

The foundations work is complete only when all of these hold:

1. Public source, diagnostics, type hints, persistence inspection, and wire
   inspection describe True/False as Tags, not BOOL.
2. The parser rejects arbitrary structural and runtime type patterns.
3. Tagged payload binding remains ergonomic and typed.
4. MAP uses `=>`, and nested MAP/WHEN arrows parse and format correctly.
5. `0.1 + 0.2 == 0.3` is True.
6. `1 / 3 * 3 == 1` is True.
7. Division by zero cannot create zero, NaN, infinity, or FLUSH implicitly.
8. Resource exhaustion cannot create an inexact Number.
9. Position 1 is the first LIST/TEXT/BYTES/BITS position.
10. Zero, negative, and non-whole positions fail without rounding or clamping.
11. Counts and shifts by zero remain valid.
12. BITS never infer from NUMBER.
13. BITS wrapping is explicit.
14. BITS literals, widths, bounds, concat order, shifts, and arithmetic match
    reference bitvector semantics.
15. MAP literal and `Map/upsert` emit equivalent operation traces.
16. A changing MAP write key leaves the old key until explicit removal.
17. Map/get observes replacement, removal, reinsertion, and query-key changes.
18. SET additions are idempotent.
19. MAP/SET enumeration is canonical across targets.
20. Injected hash collisions do not affect equality.
21. Duplicate literal keys fail, and runtime same-turn conflicts never resolve
    by scheduler/hash order.
22. No possible LIST/SET/MAP payload is accepted inside HOLD.
23. Scalar row-local HOLD remains valid.
24. FLUSH skips downstream work and unwraps at the documented boundary.
25. FLUSH inside HOLD preserves the last valid state; in a pulse batch,
    successful earlier microturn commits remain and later pulses stop.
26. FLUSH inside List/map aborts the whole operator with deterministic first
    failure.
27. Ordinary tagged item errors demonstrate explicit item isolation.
28. FLUSHED is not matchable, storable, persistent, or serializable.
29. A later source activation recovers after FLUSH.
30. Baseline `HOLD + Stream/pulses` follows committed microturn semantics, and
    its optimization has full trace equivalence.
31. Every implemented public feature has compiled positive coverage.
32. BUILD file presence alone cannot satisfy feature coverage.
33. Native/Wasm differential traces agree under the same versioned execution
    profile and budget digest.
34. GPU/FPGA reject unsupported regions rather than changing semantics.
35. Every accepted program follows
    `ParsedProgram -> CheckedProgram -> SemanticProgram ->
    ContractVerifiedProgram -> ErasedProgram`; no authored-contract-free path
    skips verification.
36. Machine, persistence, native/Wasm, GPU, FPGA, and hardware backends consume
    post-verification artifacts and never reinterpret parser AST or recover
    erased contextual semantics.
37. Formatting is idempotent, and parser/formatter/inspector spellings agree.
38. Source `Null` and `Error[...]` round-trip as Tags, while internal absence
    and faults cannot escape as application values.
39. Each activation-local HOLD inside THEN starts once from its initializer,
    survives that occurrence's pulses, and is discarded afterward.
40. Nested authorities are single-parent and acyclic; second-parent,
    cycle-forming, and lifetime-escaping attachments fail.
41. Zero/negative rounding quanta and invalid shift/rotate amounts fail
    deterministically rather than being normalized.
42. A live FLUSH effect never crosses a distributed cut or serializes.
43. Named-binding, FUNCTION, BLOCK-final, and host/root FLUSH boundaries all
    have executable conformance coverage.
44. No parser alias, feature flag, semantic profile, shim, dual
    representation, dual-read/write path, or automatic old-data migration
    remains for any replaced behavior.
45. Per-phase repository gates prove zero executable occurrences of removed
    APIs, enum variants, runtime branches, schemas, wire tags, and fixtures.
46. Obsolete examples, comments, tests, golden files, and active documents are
    rewritten or deleted in the same change; Git history is their only archive.
47. Removed spellings/bytes may appear only in isolated compile-fail or
    invalid-schema rejection fixtures that cannot be mistaken for active
    examples, persisted goldens, or valid wire data.

## Risks And Mitigations

### Exact Arithmetic Cost

Risk: rational numerator/denominator growth creates memory and latency spikes.

Mitigation: normalization, cross-cancellation, compact representations,
target-specific bounds, deterministic budgets, and proof-driven fixed
representations.

### Collection Authority Complexity

Risk: adding MAP/SET repeats LIST currentness and persistence complexity.

Mitigation: one common authority/turn/delta protocol, per-key dirty tracking,
canonical conflicts, and no snapshot-in-HOLD fallback.

### FLUSH Becomes Exception Machinery

Risk: hidden unwinding spreads across every runtime path or produces partial
commits.

Mitigation: lexical boundaries, checked escape effect, closed tagged payload,
whole-operator behavior, explicit caller re-FLUSH, and fail-closed unsupported
targets.

### Canonical Ordering Cost

Risk: deterministic MAP/SET iteration requires extra index/sort work.

Mitigation: lookup storage remains target-selected; canonical enumeration is a
separate bounded view/index. Applications that need insertion history use LIST.

### One-Based Migration Breadth

Risk: mixed positions/offsets survive in examples or host APIs.

Mitigation: flag-day rename to `position`/`from`/`count`, feature-matrix
fixtures, no aliases, and boundary conversion for external zero-based
protocols.

### Optimizer Semantic Drift

Risk: dense transient lowering or pulse fusion changes live observation,
effects, failure order, or persistence.

Mitigation: eligibility proofs, disabled/enabled differential traces, baseline
interpreter oracle, and rejection when equivalence is unknown.

### Too Many Public Types

Risk: compiler or OS work pressures the language toward Rust-like surface
growth.

Mitigation: require a demonstrated semantic gap before adding a value kind.
Prefer Tags, objects, LIST/SET/MAP, BITS, libraries, and target profiles.

## Deferred Decisions

- MAP/SET structural matching;
- named/opaque type declarations;
- general recursive named data types;
- unrestricted recursion;
- explicit public effect annotations;
- approximate GPU Number contract;
- CRDT/offline multiwriter MAP;
- tensors and accelerator-specific array types;
- OS capability/resource types;
- volatile memory and atomics;
- additional BITS arithmetic beyond demonstrated workloads;

Deferral is not rejection. Each item requires a concrete workload and a
semantic argument that the existing foundation cannot express safely or
efficiently.

Public `TABLE` and `MEMORY` syntax are deliberately not deferred: this
architecture rejects them. Persistent indexes and hardware memories remain
physical policies for ordinary typed `MAP`/`LIST` authorities.

## Primary References

Repository:

- [`LANGUAGE_SEMANTICS.md`](../architecture/LANGUAGE_SEMANTICS.md)
- [`RUNTIME_MODEL.md`](../architecture/RUNTIME_MODEL.md)
- [`LIST_MODEL.md`](../architecture/LIST_MODEL.md)
- [`DELTA_PROTOCOL.md`](../architecture/DELTA_PROTOCOL.md)
- [`BYTES_SEMANTICS.md`](../architecture/BYTES_SEMANTICS.md)
- [`TYPE_INFERENCE_AND_TYPECHECKING_PLAN.md`](TYPE_INFERENCE_AND_TYPECHECKING_PLAN.md)
- [`TYPED_LIST_PIPELINES_AND_QUERY_REMOVAL_PLAN.md`](TYPED_LIST_PIPELINES_AND_QUERY_REMOVAL_PLAN.md)
- [`BOON_PERSISTENCE_ARCHITECTURE_PLAN.md`](BOON_PERSISTENCE_ARCHITECTURE_PLAN.md)
- [`BOON_EXAMPLE_PORTFOLIO_PLAN.md`](BOON_EXAMPLE_PORTFOLIO_PLAN.md)
- [`BOON_FORMAL_VERIFICATION_AND_WHERE_PLAN.md`](BOON_FORMAL_VERIFICATION_AND_WHERE_PLAN.md)

External semantic references:

- [WebAssembly values](https://webassembly.github.io/spec/core/syntax/values.html)
- [SMT-LIB fixed-size bitvectors](https://smt-lib.org/theories-FixedSizeBitVectors.shtml)
- [WGSL](https://gpuweb.github.io/gpuweb/wgsl/)
- [NIST FIPS 180-4](https://nvlpubs.nist.gov/nistpubs/fips/nist.fips.180-4.pdf)

## End State

Boon remains small at the source level:

```text
exact values
explicit Tags
delta-native collections
bounded visible state transitions
explicit fail-fast flow
target-independent semantics
```

The interpreter, optimizer, persistence layer, formal tools, Wasm target, and
bounded accelerators may become sophisticated. Application code does not
acquire representation-width annotations, structural pattern machinery,
imperative loops, storage-engine types, or target-specific behavior merely to
make that sophistication possible.

This end state is complete without self-hosting. It intentionally makes no
claim that the same data model solves safe operating-system resource
management.
