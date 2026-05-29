# Boon Structural Type Inference And Typechecking Plan

Date: 2026-05-29

Status: implementation plan. This file is the contract for the compiler/type
inference work. It is not a claim that the typechecker already exists.

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
- Objects and tagged objects are structural. A value is accepted when it has the
  fields the consumer needs, even if it has more fields.
- `Element` is not a nominal language type. UI values satisfy renderer-neutral
  document contracts as structural objects or tagged objects.
- Events are not user-visible wrapper types. Any Boon data value can be present
  in a tick, or absent as `SKIP`; presence is compiler/runtime flow information.

## Current Problems To Remove

- `boon_parser::document_mapped_children` recognizes one `List/map` spelling
  and leaks document-rendering knowledge into the parser.
- IR and native playground lowering ask the parser for mapped document children
  instead of consuming typed render metadata.
- Current names such as `Bool`, `Record`, and `Element` are partly inherited
  implementation terms and must not become user-facing type concepts.
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
- Salsa: useful later for incremental compiler queries, not a type solver.
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
Number
List<Type>
Function(args) -> Flow<Type>
TypeVar
```

Implementation aliases are allowed when they stay internal:

- `TrueFalse = VariantSet{Tag(True), Tag(False)}`
- `RenderableContract = renderer-neutral union of document/render slot
  contracts`
- `NoElement = VariantSet{Tag(NoElement)}` when a render slot explicitly allows
  no node

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
    absent SKIP
```

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

### Tags And Boolean Compatibility

`True` and `False` are singleton tag variants. They may widen to
`VariantSet{Tag(True), Tag(False)}` in boolean contexts.

Existing operator names such as `Bool/not` and `Bool/and` may stay for source
compatibility, but their signatures operate on `TrueFalse`. The compiler may
lower `TrueFalse` fields to compact internal bool columns when that is an
optimization, but diagnostics must still describe tags, not a user-visible
`Bool`.

### Hidden Identity

Runtime keys, source ids, generations, bind epochs, slots, and scope paths are
not Boon values and do not appear in types. User fields named `id` are ordinary
data fields. A separate hidden-identity verifier should reject attempts to expose
runtime-only names or generated internals, but the core parser/typechecker must
not reject ordinary structural data such as `TodoId[id: ...]`.

## Implementation Plan

### Phase 1: Parser AST Migration

- Add `AstExprKind::Object`, `AstExprKind::TaggedObject { tag, fields }`, and
  `AstExprKind::Tag`.
- Keep current `Bool`, `Enum`, and `Record` only as temporary compatibility
  aliases. The typechecker must immediately normalize them to `Tag` and
  `Object`.
- Parse PascalCase tagged objects generically, including `Oklch[...]`,
  `Hidden[...]`, and `TodoId[id: ...]`.
- Parse decimal numbers as `Number`.
- Add real spans for nested expressions, object fields, call arguments, pattern
  arms, and tagged-object fields. Whole-line spans are not enough for
  typechecking diagnostics.
- Typechecking must walk the full AST, including the `document` statement. The
  existing semantic parser table collection may continue excluding document
  lines for source/state/list discovery.

### Phase 2: Typecheck Crate Skeleton

- Add `crates/boon_typecheck` to the workspace.
- Add `ena` only inside `boon_typecheck`.
- Introduce `Type`, `Variant`, `ObjectShape`, `TypeVar`, `TypeScheme`,
  `FlowType`, `Constraint`, `TypeDiagnostic`, `ExprTypeTable`,
  `FunctionTypeTable`, `RenderSlotTable`, `ListMapBinding`, and
  `TypeCheckReport`.
- Keep `TypeVarStore` as the only abstraction that depends on `ena`.

### Phase 3: Constraint Generation And Solving

- Assign a type variable to every expression id, function parameter, function
  result, pattern binding, object field, and render slot.
- Generate literal constraints:
  - `True`, `False`, `All`, `Completed`, etc. become singleton variants.
  - `TEXT { ... }` and string literals become `Text`.
  - integers and decimals become `Number`.
  - `[field: value]` becomes `ObjectShape`.
  - `Tag[field: value]` becomes a tagged variant.
- Generate structural constraints for paths and fields with `HasField`.
- Generate function call and pipe constraints from a builtin/library signature
  registry plus inferred user functions.
- Generate flow constraints for `SOURCE`, `THEN`, `LATEST`, `WHEN`, `WHILE`,
  `HOLD`, and source payload field access.
- Reject recursive functions in v1 with a clear diagnostic before trying to
  infer recursive schemes.

### Phase 4: Builtin And Render Contract Registries

Create two registries:

- `BuiltinSignatureRegistry` for `Text/*`, `Number/*`, `Bool/*` over
  `TrueFalse`, `List/*`, Cells helpers, and generic source/language operators.
- `RenderContractRegistry` for renderer-neutral `Document/new`, each
  `Element/*`, style objects, source binding fields, text slots, list slots, and
  expected output document node kinds.

The render registry must describe contracts in terms of Boon structural data and
renderer-neutral document output. It must not encode native GPU details.

### Phase 5: RenderSlotTable And ListMapBinding

`RenderSlotTable` must contain:

```text
slot_statement_id
slot_name
expected_contract
value_expr_id
actual_type
diagnostics
optional_list_map_binding_id
item_scope_id
template_function
template_args
materialization_policy
```

`ListMapBinding` must contain:

```text
map_expr_id
input_list_type
item_binding_name
item_type
result_type
item_scope_id
result_kind = RuntimeValue | RenderSlotMaterialization
```

`List/map` remains the ordinary generic data operator. Render behavior comes
from `SatisfiesRenderSlot`, not from parser syntax.

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

- `boon_ir::lower` calls `boon_typecheck::check` before building the typed IR.
- The IR stores or references the expression type table, function type table,
  source payload shape table, render slot table, and list-map bindings.
- Replace `document_mapped_children` in IR lowering with `RenderSlotTable` and
  `ListMapBinding` metadata.
- Replace native playground document lowering from parser AST helpers with
  compiler/runtime output: `DocumentFrame` or `DocumentPatch` plus typed render
  metadata hashes.
- Native preview still receives Boon source and must not learn example-specific
  shortcuts.
- Reports must include `typecheck_report_hash`, `render_slot_table_hash`,
  `typed_render_metadata_used`, unresolved type variable count, and render slot
  failure count.

### Phase 7: Diagnostics And Reports

Diagnostics must be source-span based and use Boon vocabulary:

- "expected a list of renderable values for `items:`"
- "object is missing field `title`"
- "tagged object `Oklch[...]` field `lightness` must be a number"
- "`Bool/not` expects `True` or `False` tag"
- "`THEN` requires a tick-present-or-absent value"
- "`SKIP` cannot initialize a held value"

Do not expose internal terms such as `TypeVar`, `Bool`, `Record`, `Event`, or
nominal `Element` in user-facing diagnostics.

Reports should include:

- expression count and checked expression count,
- unresolved type variable count,
- dynamic fallback count,
- render slot count and failure count,
- builtin signature coverage,
- source payload shape coverage,
- full-document typecheck coverage,
- list-map binding count by `RuntimeValue` and `RenderSlotMaterialization`.

Final acceptance requires unresolved type variables, dynamic fallback count, and
render slot failure count to be zero for TodoMVC, Cells, and Counter.

## Test Plan

Plan-file checks after editing:

```bash
rg -n "VariantSet|Flow<T>|RenderContractRegistry|RenderSlotTable|ListMapBinding|Assignable|SatisfiesRenderSlot" docs/plans/TYPE_INFERENCE_AND_TYPECHECKING_PLAN.md
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
- decimal numbers,
- per-field and per-argument spans.

Typechecker positive fixtures:

- TodoMVC, Cells, and Counter pass with zero annotations.
- `True` and `False` infer as tags and work with `Bool/not`.
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
- `Bool/not` called on a variant set that is not `TrueFalse`.
- `LATEST` branches with incompatible data types.
- `HOLD` initialized with one data shape and updated with an incompatible
  shape.
- `THEN` used on a continuous value.
- `SKIP` used where a continuous value or render value is required.
- `NoElement` used where a normal data value is required.
- Recursive functions until recursion inference is explicitly designed.

Native verification after integration:

```bash
cargo xtask verify-native-gpu-preview-e2e --example todomvc --report target/reports/native-gpu/preview-e2e-todomvc-typecheck.json
cargo xtask verify-native-gpu-preview-e2e --example cells --report target/reports/native-gpu/preview-e2e-cells-typecheck.json
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
- No public `Bool`, `Record`, `Event`, or `Element` types.
- No automatic list flattening.
- No recursive function inference unless designed separately.
- No broad compiler incrementalization with Salsa in the first pass.

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
