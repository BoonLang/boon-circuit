# Boon Structural Type Inference And Typechecking Plan

Date: 2026-05-29

Status: authoritative structural-inference conformance plan. A substantial
`boon_typecheck` implementation exists, but the exact-value model and mandatory
verified compiler spine below are not yet complete.

Under the combined order in [`steps.md`](steps.md), ownership needed for the
compiler spine lands first, full conformance follows the foundations flag-day,
and formal-dependent final acceptance closes after formal phases 0–5.

The public value algebra and compiler artifact ownership follow
[`BOON_LANGUAGE_FOUNDATIONS_PLAN.md`](BOON_LANGUAGE_FOUNDATIONS_PLAN.md). This
file defines structural inference within that contract; it does not introduce
alternate type/value profiles.

Cold and interactive compiler representation, invalidation, cancellation, and
latency/RSS acceptance are governed by
[`BOON_COMPILER_PERFORMANCE_PLAN.md`](BOON_COMPILER_PERFORMANCE_PLAN.md). This
plan continues to own the type terms, constraints, diagnostics, and accepted
language. Performance work may change their internal storage and scheduling,
but it must not change type semantics, relax the v1 recursion rejection, or
bypass the verified compiler spine.

## Purpose

Add fully inferred Boon typechecking so source code stays annotation-free while
the compiler stops relying on syntactic/rendering hacks such as recognizing
`List/map` only in `items:` and manually extracting document children from the
parser.

The checker must match Boon's user-facing data model:

- Tags and tagged objects are inferred from use. Users do not declare nominal
  types, classes, or modules.
- `True` and `False` are tags from the user's point of view, not a user-visible
  `Bool` type.
- `NUMBER` is an exact normalized rational; `BITS[N]` carries a statically
  known width; `LIST`, `SET`, and `MAP` retain typed authority shapes.
- Objects and tagged objects are structural. A value is accepted when it has the
  fields the consumer needs, even if it has more fields.
- `Element` is not a nominal language type. UI values satisfy renderer-neutral
  document contracts as structural objects or tagged objects.
- Events are not user-visible wrapper types. Any Boon data value can be present
  in a tick, or absent as `SKIP`; presence is compiler/runtime flow information.
- Internal absence and runtime faults are private compiler/runtime channels,
  never source values or inferred public variants.

## Current Problems To Remove

- `boon_parser::document_mapped_children` recognizes one `List/map` spelling
  and leaks document-rendering knowledge into the parser.
- IR and native playground lowering ask the parser for mapped document children
  instead of consuming typed render metadata.
- Legacy `Bool`, `Record`, `Element`, and `ListMapBinding` implementation
  terms must be deleted rather than retained as aliases or compatibility
  types.
- Generic tagged objects are not represented directly enough. `Oklch[...]` is
  currently special-cased, `Tag[...]` is not a first-class AST shape, and
  decimal numbers need proper expression support.
- Document AST lines are currently excluded from semantic table collection, but
  typechecking must still walk the full AST including `document`.
- Hidden identity checks currently reject names such as `id` and `TodoId` too
  early. User-visible ids are ordinary structural data; runtime keys and
  generations remain hidden below the language boundary.

## Library Research Decision

Use `ena` only for the low-level unification table. It is useful because
inference is mostly connecting unknowns until constraints resolve them:
`items:` expects a list whose items satisfy a render slot, `List/map` returns
`List<B>`, so the solver connects the unknown `B` to the structural constraints
required by that slot.

`ena` does not understand Boon. Boon owns all type terms, row constraints,
variant constraints, flow/presence rules, render contracts, and diagnostics.

Do not use these as the v1 checker core:

- Chalk: Rust trait-solver oriented, too specific and too heavy for Boon.
- Salsa: not a type solver and not mandated for compiler reuse. The persistent,
  dependency-indexed invalidation required by the compiler-performance plan
  remains behind Boon-owned compiler-session interfaces whether it uses Salsa
  internally or not.
- Generic HM crates: useful for reading, but Boon's object rows, variants,
  presence, source flow, hidden scopes, and render contracts require custom
  constraint generation.

If `ena` becomes unsuitable during the implementation spike, isolate it behind a
small `TypeVarStore` adapter and replace only that storage layer with a local
union-find implementation. Do not let the solver backend leak into Boon
semantics.

## Language Model

### Data Types

The user-facing data model is structural:

```text
VariantSet{Variant...}
Variant = Tag(name) | Tagged(tag: name, fields: ObjectShape)
ObjectShape{field: Type, ..., ..row}
Text
Bytes
Number                         exact normalized rational
Bits<width>
List<Type>
Set<key_type>
Map<key_type, value_type>
Function(args) -> Flow<Type>
TypeVar
```

The solver may use internal derived terms, but none are source-visible aliases
or compatibility types:

- `ClosedTagSet{True, False}` for truth-function inputs and results
- `RenderableContract = renderer-neutral union of document/render slot
  contracts`
- `NoElement = VariantSet{Tag(NoElement)}` when a render slot explicitly allows
  no node

`Number` denotes the one exact rational `NUMBER` kind, never binary64.
`Bits<width>` is the typechecker spelling for source `BITS[N]`; width is a
positive compile-time whole Number and participates in type equality.
`Set<K>` and `Map<K, V>` enforce the foundations plan's key eligibility,
canonical equality/order, authority ownership, and target bounds. They are not
generic host hash-container types.

There are no nominal language types in this pass. `TodoId[id: ...]` is accepted
because it is a tagged variant with an `id` field, not because `TodoId` was
declared.

Bare tags and tagged-object variants are distinct in v1. `Panel` is a bare tag.
If empty tagged-object syntax such as `Panel[]` is parsed later, the parser must
normalize it to the bare tag for v1 rather than creating a separate zero-field
variant.

### Structural Constraints

The checker should generate constraints, not nominal equality checks:

```text
Equal(a, b)                         same concrete/inferred type
Assignable(actual, expected)         actual structurally satisfies expected
HasField(value, field, field_type)   object/tagged-object field access
HasVariant(value, variant)           pattern or exact tag requirement
SatisfiesRenderSlot(slot, actual)    render slot contract satisfaction
FlowCompatible(actual, expected)     continuous/present/absent compatibility
PatternCovers(input, arms)           pattern coverage/exhaustiveness evidence
```

`List/map` in an `items:` slot must not be described as nominal equality between
its result item and a render alias. Instead, the map result is `List<B>` and the
slot adds
`SatisfiesRenderSlot(items, List<B>)`. Solving that constraint applies the
renderer-neutral slot contract to `B`.

### Presence And SKIP

Data type and tick presence are separate:

```text
Flow<T> =
    continuous T
    tick-present T
    absent
```

`SKIP` is the source control spelling that produces the private absent flow
state. The type term does not contain a public `Skip`, `Null`, or option value.
Runtime faults use a separate private fault channel and cannot unify with
application data.

Rules:

| Construct | Input flow | Output flow |
| --- | --- | --- |
| Field access | any `Flow<ObjectShape>` or `Flow<Tagged(...fields)>` | same flow mode for the field type; absent stays absent |
| Pure function/operator call | continuous/present args | continuous if all args continuous; present if any arg is present; absent if any required arg is absent |
| `THEN` | tick-present-or-absent input | absent when input absent; present body result when input present; continuous input is a type error |
| `WHEN` | continuous or present-or-absent input | continuous selection for continuous input; absent-preserving selection for present-or-absent input |
| `LATEST` | compatible branch data types | continuous if it has a continuous fallback; otherwise present-or-absent |
| `WHILE` | continuous selector | continuous result; event-style/present-only selector is a type error |
| `HOLD` | continuous initial value plus present-or-absent update candidates | continuous stored value |
| `List/map` | list flow plus continuous template body per item | same flow mode as the input list; template result must be data, not `SKIP`, unless a target contract explicitly accepts `NoElement` |

`SKIP` is absence, not data. It is invalid as a renderable value. `NoElement` is
a render value that means no node for a slot. `visible: False` is a field on an
existing render object; it does not delete the node, does not replace the value
with `SKIP`, and does not expose hidden runtime identity.

### Tags And Truth Functions

`True` and `False` are singleton tag variants. They may widen to
the closed set `VariantSet{Tag(True), Tag(False)}` when a truth function
requires either value.

Names such as `Bool/not`, `Bool/and`, and `Bool/or` are ordinary standard
library functions over the closed `True | False` Tag set. Their namespace does
not declare a public Boolean type and is not a compatibility mode. The compiler
may lower the closed Tag set to compact internal bit columns when proved
equivalent, but diagnostics and semantic artifacts still describe Tags.

### Hidden Identity

Runtime keys, source ids, generations, bind epochs, slots, and scope paths are
not Boon values and do not appear in types. User fields named `id` are ordinary
data fields. A separate hidden-identity verifier should reject attempts to expose
runtime-only names or generated internals, but the core parser/typechecker must
not reject ordinary structural data such as `TodoId[id: ...]`.

## Implementation Plan

### Phase 1: Final Parser AST Cutover

- Add `AstExprKind::Object`, `AstExprKind::TaggedObject { tag, fields }`, and
  `AstExprKind::Tag`.
- Delete legacy `Bool`, `Enum`, and `Record` AST/type variants in the same
  flag-day change that introduces generic Tags and objects. Do not normalize
  them through aliases or retain an old decoder.
- Parse PascalCase tagged objects generically, including `Oklch[...]`,
  `Hidden[...]`, and `TodoId[id: ...]`.
- Parse integer, fraction, and decimal literals as exact `Number`; add
  width-bearing `BITS[N]`, `LIST`, `SET`, and `MAP` expression shapes without
  routing them through host container/value guesses.
- Add real spans for nested expressions, object fields, call arguments, pattern
  arms, and tagged-object fields. Whole-line spans are not enough for
  typechecking diagnostics.
- Typechecking must walk the full AST, including the `document` statement.
  Parser tables remain syntax evidence only; source, state, collection, and
  document discovery must come from `CheckedProgram` and `SemanticProgram`.

### Phase 2: CheckedProgram Ownership

- Keep `crates/boon_typecheck` as the sole owner of structural inference and
  `CheckedProgram`.
- Keep `ena`, if retained, isolated inside `boon_typecheck`.
- Introduce `Type`, `Variant`, `ObjectShape`, `TypeVar`, `TypeScheme`,
  `FlowType`, `Constraint`, `TypeDiagnostic`, `ExprTypeTable`,
  `FunctionTypeTable`, `TypedCallTable`, `RenderContractTable`,
  `CheckedRenderSlotTable`, `CollectionShapeTable`, and `TypeCheckReport`.
- Make each `CheckedProgram` snapshot an owned, self-contained compiler
  database containing these tables. It must not borrow a `ParsedProgram`, and
  no second builder may reconstruct the checked tables after solving. No parser
  AST table or render-template binder is authoritative after typechecking.
- Preserve source-unit and declaration identities across `CompilerSession`
  revisions when the corresponding declarations survive an edit. Keep compact
  expression, type, constraint, and graph indexes dense and snapshot-local;
  those indexes are never persistence, runtime, or cross-revision identity.
- Store reverse dependency indexes for declarations, expressions, constraints,
  render contracts, and semantic consumers so an update can invalidate its
  exact affected cone. The same indexes must support a fresh in-process
  database with no reused artifacts; persistent reuse is an optimization, not
  a prerequisite for correctness or cold-speed acceptance.
- Keep `TypeVarStore` as the only abstraction that depends on `ena`.

### Phase 3: Constraint Generation And Solving

- Assign a type variable to every expression id, function parameter, function
  result, pattern binding, object field, and render slot.
- Generate literal constraints:
  - `True`, `False`, `All`, `Completed`, etc. become singleton variants.
  - `TEXT { ... }` and string literals become `Text`.
  - `BYTES { ... }` becomes `Bytes`.
  - integers, fractions, and decimals become exact `Number`.
  - a `BITS[N]` literal becomes `Bits<N>` and must fit its exact width.
  - `[field: value]` becomes `ObjectShape`.
  - `Tag[field: value]` becomes a tagged variant.
  - `LIST`, `SET`, and `MAP` infer homogeneous element/key/value shapes,
    including closed tagged unions, and reject ineligible MAP/SET keys.
- Generate structural constraints for paths and fields with `HasField`.
- Generate function call and pipe constraints from a builtin/library signature
  registry plus inferred user functions.
- Generate flow constraints for `SOURCE`, `THEN`, `LATEST`, `WHEN`, `WHILE`,
  `HOLD`, `FLUSH`, and source payload field access. Keep absence and faults
  private and disjoint from application data constraints.
- Generate authority/escape constraints that reject recursively nested
  LIST/SET/MAP values inside HOLD and invalid multi-parent collection
  ownership as required by the foundations plan.
- Reject recursive functions in v1 with a clear diagnostic before trying to
  infer recursive schemes.

### Phase 4: Builtin And Render Contract Registries

Create two registries:

- `BuiltinSignatureRegistry` for `Text/*`, `Bytes/*`, exact `Number/*`,
  width-checked `Bits/*`, `Bool/*` over the closed `True | False` Tag set,
  `List/*`, `Set/*`, `Map/*`, Cells helpers, and generic source/language
  operators.
- `RenderContractRegistry` for renderer-neutral `Document/new`, each
  `Element/*`, style objects, source binding fields, text slots, list slots, and
  expected output document node kinds.

The render registry must describe contracts in terms of Boon structural data and
renderer-neutral document output. It must not encode native GPU details.

### Phase 5: Checked Render Metadata And Semantic List Views

`CheckedRenderSlotTable`, stored in `CheckedProgram`, must contain:

```text
slot_statement_id
slot_name
expected_contract
value_expr_id
actual_type
diagnostics
typed_value_identity
```

It records only structural type satisfaction. It does not encode a parser
template, special map spelling, or executable materialization policy.

`boon_semantic` then derives ordinary typed logical list/render views in
`SemanticProgram`:

```text
semantic_view_id
source_list_authority
typed_operator_chain
item_scope_and_owner
result_item_type
dependency_manifest
order_and_currentness_provenance
render_demand_contract, when demanded by a render slot
proof_obligations
```

`List/map` remains the ordinary generic data operator. Render behavior comes
from `SatisfiesRenderSlot` plus the ordinary semantic view, not from parser
syntax or a `ListMapBinding` compatibility object. The semantic phase performs
the same contextual-call expansion and ownership analysis for render and
non-render uses.

Valid render examples:

```boon
items: todos |> List/map(todo, new: todo_row(todo: todo))
items: make_rows(todos: todos)
items: LIST { header(), footer() }
```

Invalid until an explicit concat/flatten operator exists:

```boon
items: LIST { header(), todos |> List/map(todo, new: todo_row(todo: todo)) }
```

### Phase 6: IR, Runtime, And Native Integration

- Enforce the single compiler spine:

  ```text
  ParsedProgram
  -> CheckedProgram
  -> SemanticProgram
  -> ContractVerifiedProgram
  -> ErasedProgram
  -> MachinePlan
  -> PhysicalPlan or CoreHardwareIR
  ```

- `boon_typecheck` produces `CheckedProgram`; it never writes executable IR.
- `boon_semantic` consumes only `CheckedProgram` and produces typed views,
  semantic ownership, dependency manifests, contextual expansion, and proof
  obligations in `SemanticProgram`.
- `boon_verify` produces mandatory `ContractVerifiedProgram`, including when
  source contains no authored `WHERE`.
- `boon_ir` consumes only the verified artifact, erases proof/contextual
  surface, and produces `ErasedProgram`.
- Replace `document_mapped_children` and every parser/template rediscovery path
  with `CheckedRenderSlotTable` plus the verified semantic view.
- Replace native playground document lowering from parser AST helpers with
  compiler/runtime output: `DocumentFrame` or `DocumentPatch` plus typed render
  metadata hashes.
- Native preview still receives Boon source and must not learn example-specific
  shortcuts.
- Reports must include `typecheck_report_hash`,
  `checked_render_slot_table_hash`, `semantic_program_hash`,
  `contract_verified_program_hash`, `typed_render_metadata_used`, unresolved
  type variable count, and render slot failure count.

### Phase 7: Diagnostics And Reports

Diagnostics must be source-span based and use Boon vocabulary:

- "expected a list of renderable values for `items:`"
- "object is missing field `title`"
- "tagged object `Oklch[...]` field `lightness` must be a number"
- "`Bool/not` expects `True` or `False` tag"
- "`THEN` requires a tick-present-or-absent value"
- "`SKIP` cannot initialize a held value"

Do not expose internal terms such as `TypeVar`, a public Boolean type,
record/element nominal types, event wrappers, or solver rows in user-facing
diagnostics. A diagnostic may name the ordinary function `Bool/not`.

Reports should include:

- expression count and checked expression count,
- unresolved type variable count,
- dynamic fallback count,
- render slot count and failure count,
- builtin signature coverage,
- source payload shape coverage,
- full-document typecheck coverage,
- exact-Number, BITS-width, MAP/SET key, authority, and private-channel
  constraint coverage,
- semantic typed-view count and proof-obligation count.

Final acceptance requires unresolved type variables, dynamic fallback count, and
render slot failure count to be zero for TodoMVC, Cells, and Counter.

## Test Plan

Plan-file checks after editing:

```bash
rg -n "VariantSet|Flow<T>|RenderContractRegistry|CheckedRenderSlotTable|Assignable|SatisfiesRenderSlot|ContractVerifiedProgram" docs/plans/TYPE_INFERENCE_AND_TYPECHECKING_PLAN.md
! rg -n "B[[:space:]]*=[[:space:]]*Renderable|Event<[[:alpha:]]+>|nominal[[:space:]]+Element" docs/plans/TYPE_INFERENCE_AND_TYPECHECKING_PLAN.md
```

The first command must find all key terms. The second command must find no
matches.

Future implementation unit gates:

```bash
cargo test -p boon_typecheck --lib --no-fail-fast
cargo test -p boon_parser -p boon_ir -p boon_runtime --lib --no-fail-fast
cargo test -p boon_native_playground --bin boon_native_playground cells_ -- --test-threads=1
```

Parser positive fixtures:

- object syntax,
- generic tagged-object syntax,
- `Oklch[...]`,
- `Hidden[...]`,
- `TodoId[id: ...]`,
- exact integer/fraction/decimal numbers,
- width-bearing BITS literals,
- LIST/SET/MAP literals,
- per-field and per-argument spans.

Typechecker positive fixtures:

- TodoMVC, Cells, and Counter pass with zero annotations.
- `True` and `False` infer as tags and work with `Bool/not`.
- `Bool/and` and `Bool/or` accept and return only the closed
  `True | False` Tag set.
- rationally equal NUMBER expressions unify and compare exactly; no binary64
  type or value path is inferred.
- BITS operations preserve exact width, and explicit NUMBER/BITS conversions
  enforce their declared bounds.
- MAP/SET keys accept only the foundations plan's canonical key types and
  retain homogeneous key/value/element shapes.
- `Oklch[...]`, `Hidden[...]`, style objects, and `TodoId[id: ...]` infer as
  structural variants or objects.
- `items: some_function_returning_renderables()` works without `List/map`.
- `List/map` outside render slots still returns ordinary data lists.
- Functions accept structurally compatible objects with extra fields.

Negative fixtures:

- `items: todos` where `todos` is a list of data objects.
- `List<List<RenderableContract>>` passed to `items:`.
- Missing required object field.
- Unknown path field without `?` or absence handling.
- Wrong tagged-object field type.
- Wrong style field type.
- `Bool/not` called on a variant set other than closed `True | False`.
- a BITS literal that does not fit its declared width or mixes widths without
  an explicit conversion;
- an ineligible MAP/SET key, heterogeneous collection shape, collection inside
  HOLD, or invalid nested-authority ownership;
- private absence or runtime fault used as application data;
- `LATEST` branches with incompatible data types.
- `HOLD` initialized with one data shape and updated with an incompatible
  shape.
- `THEN` used on a continuous value.
- `SKIP` used where a continuous value or render value is required.
- `NoElement` used where a normal data value is required.
- Recursive functions until recursion inference is explicitly designed.

Native verification after integration:

```bash
cargo xtask verify-counter-dev --report target/reports/report-v2/counter-dev.json
cargo xtask verify-cells --report target/reports/report-v2/cells.json
cargo xtask verify-all --check-existing --report target/reports/report-v2/verify-all.json
```

Smell gates after type-driven render lowering lands:

```bash
rg -n "document_mapped_children|DocumentMappedChildren|Element/repeat" crates examples
rg -n "boon_parser_document_ast_to_boon_document_model" crates
```

Both commands must return no matches.

## Non-Goals For V1

- No Boon type annotation syntax.
- No nominal type declarations.
- No public Boolean, record, event-wrapper, or element nominal types.
- No public absence or runtime-fault value types.
- No automatic list flattening.
- No recursive function inference unless designed separately.
- No Salsa mandate and no incremental-query framework inside the type solver.
  Boon-owned `CompilerSession` invalidation and reuse are nevertheless required
  by the compiler-performance plan after the cache-disabled cold core passes.

## Acceptance Criteria

- `boon_typecheck` rejects invalid programs before IR/runtime execution.
- TodoMVC, Cells, and Counter typecheck with zero annotations.
- `List/map` render behavior is ordinary generic inference plus render-slot
  constraints, not parser/render-specific syntax recognition.
- Renderable values are checked structurally as objects or tagged objects
  satisfying renderer-neutral contracts.
- Native render reports prove typed render metadata is used.
- User-facing diagnostics use Boon terms and do not leak hidden runtime identity
  or internal solver concepts.
- Exact NUMBER, BITS widths, LIST/SET/MAP shapes, canonical key eligibility,
  ordinary Tags, and private absence/fault channels are inferred and rejected
  consistently across the full AST.
- `CheckedProgram` owns structural types and render contracts;
  `SemanticProgram` owns typed contextual/list views, ownership, dependencies,
  and proof obligations; no parser/template metadata survives as an
  authoritative side channel.
- `CheckedProgram` is an owned database rather than a borrowed checker result;
  session-stable declaration identity is distinct from snapshot-local dense
  indexes, and dependency-indexed reuse produces the same diagnostics and
  artifact hashes as a fresh cache-disabled database.
- Every accepted program reaches `ContractVerifiedProgram` before
  `ErasedProgram`, and no runtime/backend consumes an unverified or parser-level
  artifact.
- Legacy Bool/Record/Element variants, `ListMapBinding`, dynamic type fallback,
  and parser render rediscovery are deleted rather than aliased.
