# Boon Structural Type Inference And Typechecking Plan

Date: 2026-05-29

Status: implementation plan. This file is the contract for the compiler/type
inference work. It is not a claim that the typechecker already exists.

## Purpose

Add fully inferred Boon typechecking so source code stays annotation-free while
the compiler stops relying on syntactic/rendering hacks such as recognizing
`List/map` only in `items:` and manually extracting document children from the
parser.

The type system must match Boon's data model:

- Tags and tagged objects are inferred from use. Users do not declare nominal
  types, classes, or modules.
- `True` and `False` are tags from the user's point of view, not a user-visible
  `Bool` type.
- `Object` and `TaggedObject` are structural. A value is accepted when it has
  the fields the consumer needs, even if it has more fields.
- `Element` is not a nominal language type. Renderable UI values are tagged
  objects and objects that satisfy a platform/library render contract.
- `Event<T>` is not a user-visible type. Any Boon data value can be present in a
  tick, or absent as `SKIP`; presence is compiler/runtime flow information.

## Current Problems To Remove

- `boon_parser::document_mapped_children` recognizes a specific `List/map`
  spelling and leaks document-rendering knowledge into the parser.
- IR and native playground lowering ask the parser for mapped document children
  instead of consuming typed render metadata.
- Current names such as `Bool`, `Record`, and `Element` are partly inherited
  implementation terms and should not become user-facing type concepts.
- Some valid future Boon shapes, such as `items: make_rows(...)`, cannot be
  accepted generically until `items:` is checked by inferred result type.
- Invalid shapes, such as a data object list passed directly to `items:`, are
  currently hard to reject cleanly with useful diagnostics.

## Library Research Decision

Use `ena` for the low-level unification table. It is useful because inference is
mostly connecting unknowns until constraints resolve them: `items:` requires
`List<Renderable>`, `List/map` returns `List<B>`, so the solver connects `B` to
`Renderable`. `ena` provides the union-find/unification machinery so Boon does
not need a custom, bug-prone variable table.

`ena` does not understand Boon. Boon still owns all type terms, constraints,
structural object rules, tag-set rules, flow/presence rules, render contracts,
and diagnostics.

Do not use these as the v1 checker core:

- Chalk: Rust trait-solver oriented, too specific and too heavy for Boon.
- Salsa: useful later for incremental compiler queries, not a type solver.
- Generic HM crates: useful for reading, but Boon's row fields, tagged objects,
  presence, source flow, hidden scopes, and platform render contracts require
  custom constraint generation.

If `ena` becomes unsuitable during the implementation spike, isolate it behind a
small `TypeVarStore` trait and replace only that storage layer with a local
union-find implementation. Do not let the solver backend leak into Boon
semantics.

## Language Model

### Data Types

The user-facing data model is structural:

```text
TagSet{TagName...}
Text
Number
ObjectShape{field: Type, ...}
TaggedObjectShape{tag: TagName, fields: ObjectShape}
List<Type>
Function(args) -> Flow<Type>
TypeVar
```

Implementation aliases are allowed when they stay internal:

- `TrueFalse = TagSet{True, False}`
- `Renderable = platform/library union of renderable object and tagged-object
  shapes`
- `NoElement = TagSet{NoElement}` when the render platform accepts it

There are no nominal language types in this pass. `TodoId[id: ...]` is accepted
because it is a `TaggedObjectShape` with tag `TodoId` and an `id` field, not
because `TodoId` was declared.

### Presence And SKIP

Data type and tick presence are separate:

```text
Flow<T> =
    continuous T
    tick-present T
    absent SKIP
```

Rules:

- `THEN` gates on presence. If input is absent, output is `SKIP`; otherwise the
  body result is tick-present.
- `LATEST` merges present values and ignores `SKIP`.
- `WHEN` pattern matching keeps the matched value's flow mode, with absent input
  producing `SKIP`.
- `HOLD` stores data type `T`; update candidates must produce compatible `T`
  when present.
- `SKIP` is not a member of every data type. It is absence in `Flow<T>`.

### Structural Objects

Field access generates a structural constraint:

```text
HasField(value_type, "field_name", field_type)
```

Objects and tagged objects should use row-style constraints internally:

```text
ObjectShape{title: Text, completed: TagSet{True, False}, ..row}
TaggedObjectShape{tag: Oklch, fields: ObjectShape{lightness: Number, ..row}}
```

This lets functions accept any object that has the fields they read. Extra fields
do not make a value incompatible. Missing required fields produce diagnostics.

### Tags And Tagged Objects

Tags are inferred from usage. Tagged object syntax is:

```boon
TodoId[id: Ulid/generate()]
Oklch[lightness: 0.97, chroma: 0.02]
Hidden[text: TEXT { Edit todo }]
```

Pattern matching over tags and tagged objects narrows the matched value:

```boon
material |> WHEN {
    InputInterior[focus] => ...
    ButtonDelete[hover] => ...
    Panel => ...
}
```

The checker must treat `True` and `False` like ordinary tags. Existing operator
names such as `Bool/not` and `Bool/and` may stay for compatibility, but their
signatures are over `TagSet{True, False}`.

## Implementation Plan

### Phase 1: Parser And Typecheck Crate Skeleton

- Add `crates/boon_typecheck` to the workspace.
- Add dependency on `ena` only inside `boon_typecheck`.
- Introduce `Type`, `TypeVar`, `TypeScheme`, `FlowType`, `Constraint`,
  `TypeDiagnostic`, `ExprTypeTable`, `FunctionTypeTable`, `RenderSlotTable`,
  and `TypeCheckReport`.
- Parser follow-up: introduce explicit AST names for `Object` and
  `TaggedObject`. Existing `AstExprKind::Record` can be kept as a compatibility
  alias during migration, but new checker diagnostics must say `object`.
- Parser follow-up: parse generic PascalCase tagged objects instead of
  special-casing `Oklch[...]` as a string.

### Phase 2: Constraint Generation

- Assign a type variable to every expression id and to every function parameter
  and function result.
- Generate literal constraints:
  - `True`, `False`, `All`, `Completed`, etc. become `TagSet{...}`.
  - `TEXT { ... }` and string literals become `Text`.
  - numbers become `Number`.
  - `[field: value]` becomes `ObjectShape`.
  - `Tag[field: value]` becomes `TaggedObjectShape`.
- Generate field/path constraints with `HasField`.
- Generate function call and pipe constraints from a builtin/library signature
  registry plus inferred user functions.
- Generate list constraints for `LIST`, `List/map`, `List/retain`,
  `List/append`, `List/remove`, `List/count`, and Cells list helpers.
- Generate flow constraints for `SOURCE`, `THEN`, `LATEST`, `WHEN`, `WHILE`,
  and `HOLD`.

### Phase 3: Builtin And Platform Signature Registry

Create a registry that can describe standard library and platform functions
without hardcoding those rules into parser helpers.

Required v1 groups:

- `Text/*`
- `Number/*`
- `Bool/not` and `Bool/and` over `TagSet{True, False}`
- `List/*`
- `Element/*`
- `Document/*`
- `Oklch[...]`
- Cells formula helpers already used by examples

`Element/*` functions return structural renderable tagged objects or objects
according to the platform contract. For example, `Element/stripe(...)` returns a
renderable value whose tag/fields satisfy the native document renderer. This is
an internal contract, not a user-visible nominal `Element`.

### Phase 4: Render-Slot Typing

Document render slots impose expected types:

```text
document root: Renderable
child: Renderable
items: List<Renderable>
children: List<Renderable>
contents: List<Renderable or Text-compatible value according to platform slot>
```

`List/map` is not special in render slots. The ordinary generic signature:

```text
List<A> |> List/map(item, new: expr_returning_B) -> List<B>
```

combined with `items: List<Renderable>` is enough to infer `B = Renderable`.

This means all of these should be handled by type, not by syntax:

```boon
items: todos |> List/map(todo, new: todo_row(todo: todo))
items: make_rows(todos: todos)
items: LIST { header(), footer() }
```

Do not automatically flatten nested lists. This is invalid until an explicit
concat/flatten operator is designed:

```boon
items: LIST { header(), todos |> List/map(todo, new: todo_row(todo: todo)) }
```

### Phase 5: IR And Native Integration

- `boon_ir::lower` calls `boon_typecheck::check` before building the typed IR.
- The IR stores the checker report or the relevant derived tables:
  - expression type table,
  - function type table,
  - source payload shape table,
  - render slot table,
  - typed row-scope/list-map metadata.
- Replace `document_mapped_children` in IR lowering with render metadata from
  the typechecked program.
- Replace the native playground's `document_mapped_children` caller with typed
  render metadata. The preview process still receives Boon source and must not
  learn example-specific shortcuts.
- Keep hidden runtime identity out of the type model. Runtime keys, generations,
  source ids, bind epochs, slots, and scopes never become Boon values.

### Phase 6: Diagnostics And Reports

Diagnostics must be source-span based and use Boon vocabulary:

- "expected a list of renderable values for `items:`"
- "object is missing field `title`"
- "tagged object `Oklch[...]` field `lightness` must be a number"
- "`Bool/not` expects `True` or `False` tag"
- "`SKIP` cannot initialize a held value"

Do not expose internal terms such as `TypeVar`, `Bool`, `Record`, `Event`, or
`Element` in user-facing diagnostics.

Reports should include:

- count of expressions checked,
- unresolved type variable count,
- dynamic fallback count,
- render slot count,
- render slot failures,
- builtin signature coverage,
- source payload shape coverage.

Final acceptance requires unresolved type variables and dynamic fallback count
to be zero for TodoMVC, Cells, and Counter.

## Test Plan

Unit gates:

```bash
cargo test -p boon_typecheck --lib --no-fail-fast
cargo test -p boon_parser -p boon_ir -p boon_runtime --lib --no-fail-fast
cargo test -p boon_native_playground --bin boon_native_playground cells_ -- --test-threads=1
```

Positive fixtures:

- TodoMVC, Cells, and Counter pass typechecking.
- `True` and `False` infer as tags and work with `Bool/not`.
- `TodoId[id: ...]`, `Oklch[...]`, `Hidden[...]`, and style objects infer as
  structural tagged objects or objects.
- `items: some_function_returning_renderables()` works without `List/map`.
- `List/map` outside render slots still returns ordinary data lists.
- Functions accept structurally compatible objects with extra fields.

Negative fixtures:

- `items: todos` where `todos` is a list of data objects.
- `List<List<Renderable>>` passed to `items:`.
- Missing required object field.
- Unknown path field without `?`/absence handling.
- Wrong tagged-object field type.
- `Bool/not` called on a tag set that is not `TagSet{True, False}`.
- `LATEST` branches with incompatible data types.
- `HOLD` initialized with one data shape and updated with an incompatible
  shape.
- `SKIP` used where a continuous value is required.
- Recursive functions until recursion inference is explicitly designed.

Native verification after integration:

```bash
cargo xtask verify-native-gpu-preview-e2e --example todomvc --report target/reports/native-gpu/preview-e2e-todomvc-typecheck.json
cargo xtask verify-native-gpu-preview-e2e --example cells --report target/reports/native-gpu/preview-e2e-cells-typecheck.json
```

Smell gate:

```bash
rg -n "document_mapped_children|DocumentMappedChildren|Element/repeat" crates examples
```

After type-driven render lowering lands, this must return no matches.

## Non-Goals For V1

- No Boon type annotation syntax.
- No nominal type declarations.
- No public `Bool`, `Record`, `Event`, or `Element` types.
- No automatic list flattening.
- No recursive function inference unless designed separately.
- No broad compiler incrementalization with Salsa in the first pass.

## Acceptance Criteria

- The plan file exists and is referenced by future typechecking work.
- `boon_typecheck` rejects invalid programs before IR/runtime execution.
- TodoMVC, Cells, and Counter typecheck with zero annotations.
- `List/map` render behavior is ordinary generic inference plus `items:` slot
  constraints, not parser/render-specific syntax recognition.
- Renderable values are checked structurally as objects/tagged objects.
- User-facing diagnostics use Boon terms and do not leak hidden runtime identity
  or internal solver concepts.
