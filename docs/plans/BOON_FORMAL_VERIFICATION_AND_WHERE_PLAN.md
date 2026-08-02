# Boon Formal Verification And `WHERE` Plan

Date: 2026-07-26

Status: **implementation-ready canonical plan**. This document records the
agreed application syntax, proof semantics, compiler architecture, playground
teaching portfolio, rollout, and acceptance criteria. It does not claim that
the current parser, typechecker, compiler, runtime, editor, or playground
already accepts or verifies `WHERE`.

This plan is the source of truth for implementing the feature. Once the
implementation is accepted, the normative language semantics must also be
integrated into `docs/architecture/LANGUAGE_SEMANTICS.md`; that document must
then link back here for implementation history and rollout details.

Reconciled on 2026-07-27 with
`BOON_LANGUAGE_FOUNDATIONS_PLAN.md` and
`BOON_PACKED_DATA_AND_DENSE_INTERNALS_PLAN.md`. Those plans own the public
value algebra and packed execution architecture. This plan owns proof syntax,
obligations, evidence, and the mandatory verification gate. In particular,
Boon `NUMBER` is an exact normalized rational, `True` and `False` are ordinary
Tags, and no public Boolean, floating-point, absence, or fault value is
introduced by formal verification.

Reconciled on 2026-08-02 with
[`BOON_COMPILER_PERFORMANCE_PLAN.md`](BOON_COMPILER_PERFORMANCE_PLAN.md).
That plan owns compiler latency and memory budgets, compiler-session and
invalidation architecture, cancellation, profiling, and the compiler-side
dense representations pulled forward to meet those budgets. This plan retains
exclusive ownership of proof meaning, required obligations, accepted evidence,
assurance policy, verifier manifests, and the rule that no executable artifact
may bypass `ContractVerifiedProgram`.

## Executive Decision

Boon gets exactly two application-facing formal-verification forms:

```boon
FUNCTION function_name(parameters) WHERE {
    parameter_conditions
} {
    body
}
```

```boon
expression
|> WHERE result {
    result_conditions
}
```

Both forms mean:

> The compiler must prove these conditions at this location.

Their positions assign different proof responsibilities:

- A function-header `WHERE` states what every caller must prove. The function
  body is verified under those established facts.
- A piped-value `WHERE` states what the producer of that exact value must
  prove. The identical value continues through the pipeline with the proved
  facts attached to it.
- A piped `WHERE` on the value returned by a function supplies that function's
  result guarantees to its callers.

Both forms are compile-time-only and are erased before executable graph
operations are produced. They never emit a per-evaluation assertion, branch,
filter, retry, panic, event suppression, or fallback selection. Persisted
authorities whose invariants survive restarts separately carry bounded
host-level proof-provenance metadata and an activation check; that declared
persistence cost is not an executable `WHERE` operation.

V1 certifies that every explicit `WHERE`, every instantiated call requirement,
and every obligation required to make those contracts meaningful has been
discharged. It does not claim that every operation in a program is defined,
that every effect succeeds, or that the entire compiler/runtime stack is
formally verified. Reports carry this explicit-contract coverage classification
so `ContractVerifiedProgram` cannot be mistaken for total-program safety.

V1 deliberately adds no:

- bare statement `WHERE`;
- implicit `IT` or other implicit proof subject;
- `REQUIRE`, `ENSURES`, `INVARIANT`, `TRANSITION`, `ASSERT`, `ASSUME`,
  `ADMIT`, `TRUSTED`, `VERIFY`, `CHECK`, or `WHERE?` keyword;
- proof-script language;
- runtime assertion fallback;
- app-visible proof-result value;
- separate refinement wrapper type.

The compiler recognizes induction, branch coverage, function composition, and
list algebra from ordinary Boon structure. Application developers continue to
write normal functions, pipelines, `WHEN`, `WHILE`, `THEN`, `LATEST`, `HOLD`,
records, and lists.

## Goals

- Keep the source surface local and small enough to teach in one screen.
- Let application developers specify useful safety properties next to the
  values and function boundaries they describe.
- Make impossible states, invalid transitions, unsafe calls, and broken
  structural relationships fail before a runnable artifact is emitted.
- Verify reactive state with the existing `HOLD` and `LATEST` semantics instead
  of adding a second state-machine notation.
- Keep runtime validation and error handling in ordinary typed Boon.
- State precisely whether a theorem applies to continuous values, present event
  payloads, successful results, or presence itself.
- Preserve Boon's structural values, hidden list identity, event presence,
  currentness, and exact-Number semantics.
- Produce source-level counterexamples that are useful to humans and AI.
- Make contracts stable inputs to refactoring, generation, migration, and API
  compatibility checks.
- Make `WHERE` itself add zero executable operations or evaluations. Ordinary
  runtime semantic hardening required by a proved model, such as exact list
  capacity validation, is specified and measured separately.
- Make the storage and activation cost of preserving proved invariants across
  persisted restarts explicit: invariant-stamp bytes, atomic commit work,
  provenance lookup, and activation latency are measured host costs, not
  executable `WHERE` operations.
- Later allow proven facts to guide generic optimization, but only with
  translation validation and measured evidence.
- Keep all proof examples generic. No parser, verifier, runtime, document,
  renderer, native host, or editor path may branch on a proof-example id.

## Non-Goals

- V1 does not prove general temporal liveness, fairness, or eventual delivery.
- V1 does not prove wall-clock deadlines, portable CPU/GPU cost, energy use, or
  frame rate.
- V1 does not prove service honesty, network availability, credentials,
  filesystem durability, or crash recovery without separately modeled platform
  contracts.
- V1 does not add unrestricted quantifiers, arbitrary user-written induction,
  higher-order theorem proving, or recursive proof scripts.
- V1 does not expose hidden list keys, runtime ids, slots, generations,
  pointers, scheduler choices, renderer identities, or optimizer plan ids.
- V1 does not prove an independently buggy compiler backend correct merely
  because the source program was proved.
- V1 does not certify whole-program absence of every possible typed error.
  Definedness is proved for contracted subject expressions and conditions, not
  silently generated for unrelated source.
- V1 does not turn testing over a convenient finite sample into proof over an
  unbounded domain.
- V1 does not add app-level escape hatches that silently accept an unproved
  obligation.

## The Developer Mental Model

There are four rules:

1. Header `WHERE`: callers prove the accepted input domain.
2. Pipeline `WHERE`: the current producer proves facts about the current value.
3. A proved value passes through unchanged; proof facts follow that value.
4. Runtime validation and runtime errors remain ordinary Boon code.

For rule 1, both an ordinary parameter and a required `PASSED.path` are
caller-supplied inputs. `PASS:` is only the call syntax that binds the latter.
`OUT`, row callbacks, element state, and captured mutable state are not silently
reclassified as caller parameters.

For example:

```boon
FUNCTION choose_nonnegative(value, fallback) WHERE {
    fallback >= 0
} {
    value >= 0
    |> WHEN {
        True => value
        False => fallback
    }
    |> WHERE result {
        result >= 0
    }
}
```

At a call:

```boon
choose_nonnegative(
    value: store.selected
    fallback: 0
)
```

the caller proves `0 >= 0`. The body uses that fact to establish that both
branches return a non-negative value. Callers may then use the returned
`result >= 0` fact.

## Normative Source Syntax

### Grammar Shape

The conceptual grammar is:

```text
function
    := FUNCTION name "(" parameters ")" function_where? block

function_where
    := WHERE condition_block

value_where
    := expression "|>" WHERE identifier condition_block

condition_block
    := "{" condition (separator condition)* "}"

separator
    := completed-line boundary | ","
```

The canonical authored style uses one condition per line without commas:

```boon
WHERE {
    lower >= 0
    upper <= 100
}
```

Compact source may use the normal comma separator:

```boon
WHERE { lower >= 0, upper <= 100 }
```

An empty condition block is invalid. A condition block is logical conjunction;
clause order has no semantic meaning.

A newline ends a clause only when it completes one top-level condition-block
child at the clause indentation. Deeper-indented lines, lines within an open
delimiter, and lines beginning with an explicit continuation operator belong
to the same clause. Thus a multiline infix expression, pipeline, named call, or
nested branch is not split into accidental conjuncts.

`WHERE` is contextual syntax only after a function parameter list or a pipeline
operator in the two shapes above. Existing identifier, tag, or callable-name
behavior outside those contexts remains unchanged.

The formatter must be extended to emit the canonical multiline form
idempotently. Current formatter behavior is not evidence that the new compound
function header or condition block is already supported.

### Function-Header Form

```boon
FUNCTION increment_percentage(value) WHERE {
    value >= 0
    value <= 99
} {
    value + 1
}
```

The header:

- may reference all parent-evaluated ordinary input parameters regardless of
  their textual order;
- may reference the inferred `PASSED` field paths required by the function and
  by calls that inherit its context; these are implicit contextual formals
  supplied by the caller just like ordinary parameters for proof purposes;
- may reference compiler-certified closed immutable constants;
- may call verified pure, total helpers whose requirements are discharged in
  the header and whose complete transitive dependency closure uses only the
  permitted boundary formals and closed constants—never `OUT`, an
  output-evaluated argument, compiler-supplied context, dynamic capture,
  effect, or hidden runtime identity;
- may not reference `PASS` itself, because it is call-clause syntax rather than
  a value;
- may not reference `OUT`, an ordinary parameter whose
  `CheckedEvaluationScope` is `Output`, or a compiler-supplied context such as
  `element`, because those are evaluated or supplied per contextual
  materialization rather than by the function's parent caller;
- may not reference function-body declarations, returned values, effect
  completions, dynamic lexical captures, future values, or hidden runtime
  identity;
- does not change function arity, argument order, runtime calling convention,
  or structural result type; inferred `PASSED` formals remain a separate
  contextual signature;
- is part of the checked public function signature and compatibility hash.

Every call substitutes its ordinary actual arguments and its resolved
`PASSED` context binding into the header conditions and must prove them. A call
without an explicit `PASS:` statically inherits the caller's contextual
formal; an explicit `PASS:` supplies the complete replacement context
expression for that call. Regions of the function body that depend only on
these boundary formals and verified public dependencies are checked once
symbolically. Checkpoints that depend on `OUT`, an output-evaluated argument,
or a compiler-supplied context are instead checked for every concrete
contextual materialization.

Header satisfiability is its own required obligation:

```text
exists parent-evaluated ordinary input parameters and PASSED context formals:
    every header condition is defined
    and every header condition is True
```

`unknown`, timeout, or an unsupported theory rejects the definition. A
satisfying model is replayed through the shared proof semantic evaluator.
Boon does not accept vacuous function proofs whose stated call domain is empty.

For reactive inputs, a caller obligation applies to every present payload that
may reach the call over time, not merely the initial value or values exercised
by a scenario.

### Piped-Value Form

```boon
expression
|> WHERE value {
    value >= 0
    value <= 100
}
```

The name after `WHERE` is required. It:

- names the exact continuous value or present payload produced by the entire
  pipeline prefix;
- exists only inside the condition block;
- is a proof-only binding and never a runtime declaration;
- does not escape into downstream source;
- does not change the value's type or representation.

Outer lexical values remain visible according to normal Boon scope. This
allows relational conditions:

```boon
[
    lower: lower_value
    upper: upper_value
]
|> WHERE bounds {
    bounds.lower == lower_value
    bounds.upper == upper_value
    bounds.lower <= bounds.upper
}
```

Proof facts remain attached to the refined value and its dependent uses. An
unused refinement does not inject unrelated ambient facts into sibling
expressions.

A fact may cross a lexical or temporal scope boundary only when all of its free
checked declarations remain visible there:

- a `BLOCK` result may retain result-only facts, but facts mentioning a private
  block declaration are dropped on exit;
- a transition fact mentioning a `HOLD` alias is available to that induction
  step, then is dropped after commit;
- a fact stated over a function's `PASSED` contextual formals may cross that
  function boundary as part of its checked contract and is substituted at each
  direct or inherited call;
- a fact mentioning a concrete `OUT`, output-evaluation scope,
  compiler-supplied context, or private capture stays inside its exact
  materialization graph;
- V1 performs no automatic existential projection to manufacture a weaker
  escaping formula.

Thus `after == before + 1` can prove a `HOLD` step locally, but `before` never
becomes accessible after the `HOLD`.

If the proof alias has the same name as an outer binding, it follows normal
lexical shadowing inside the condition block. The outer binding remains
unchanged and becomes visible again after the block. The editor should warn
about an avoidable collision, but it is not a second language rule.

Flow behavior is normative:

- For a continuous `Value<T>`, the alias names its current `T`.
- For an `Event<T>` or another presence-gated `Flow<T>`, verification occurs
  under the path assumption that the input is present, and the alias names that
  present `T` payload.
- The output retains the identical flow kind, presence, event sequence, and
  payload.
- A `WHERE` proves neither presence nor liveness.
- A required refinement proves that its subject expression and every condition
  are defined on every admitted path on which a value is produced.
- An ordinary tagged error value remains ordinary data and may be refined.
  A possible typed evaluation error in producing the subject instead fails the
  required proof; `WHERE` never catches or converts it.

The erasure law is:

```text
erase(expression |> WHERE alias { conditions }) = erase(expression)
```

The input expression is evaluated according to ordinary Boon semantics exactly
once. `WHERE` does not cause a second executable evaluation.

### Returned-Value Guarantees

When the value returned by a function is refined, boundary-visible conditions
become part of the checked result contract:

```boon
FUNCTION bounded_double(value) WHERE {
    value >= 0
    value <= 50
} {
    value * 2
    |> WHERE result {
        result >= 0
        result <= 100
    }
}
```

The default exported statement is:

> For every successful present returned payload, the returned conditions hold.

It does not establish that a result is present. The checked theorem records
payload conditions and the `applies_when_present` mode. Whether a present
return path was proved reachable is separate evidence/report metadata; it does
not alter the authored public theorem or its hash.

V1 exports only formulas whose free checked declarations are:

- the returned payload;
- parent-evaluated ordinary input parameters;
- declared implicit `PASSED` contextual formals;
- verified public constants.

`PASS`, `OUT`, output-evaluated ordinary parameters, compiler-supplied
contexts, and private body declarations are not allowed as free declarations
in an exported V1 formula. A proof may of course inspect the fixed
implementation and its fully enumerated captures, calls, and resources; those
dependencies are bound by the verified bundle. It may not rely on an unlisted
ambient assumption. Conditions using private values may still be proved
internally, but the compiler does not perform existential projection or infer
a different public formula. It never silently infers a new public precondition
from a failed body proof.

The normalized returned theorem is therefore:

```text
for all ordinary formals p and PASSED contextual formals c
such that Header(p, c) holds,
and for every other state, source, effect, external, scheduling, and
provider behavior admitted by the recorded Boon semantics:
    ProducedPresent(function, p, c, result)
    implies Defined(result) and ReturnedFormula(result, p, c)
```

An implementation dependency may support this proof, but only an explicitly
recorded verified invariant or provider theorem may restrict those quantified
ambient behaviors.

Public export requires a symbolic universal definition proof, not merely a
finite set of current materializations. If a returned checkpoint depends on a
concrete `OUT`, output-evaluated closure, element/provider context, row owner,
or other materialization-local assumption, V1:

- proves and attaches the fact only to each exact instantiated result in the
  current semantic graph;
- records visibly that it is a local returned checkpoint;
- does not place it in `FunctionTheorem.returned_theorem` or a public contract
  bundle;
- re-proves it for every future materialization rather than letting another
  caller reuse it.

A result guarantee becomes public only after the verifier establishes it
symbolically for all required materializations, typically through a
compiler-owned quantified contextual-operation summary that removes concrete
`OUT`/provider identities from the assumptions. The compiler never turns
finite materialization coverage into a universal theorem or silently labels a
local checkpoint public.

### `BLOCK` And Local Declarations

`WHERE` does not change Boon's declaration rules. Current Boon function scopes
can contain order-independent body declarations, while `BLOCK` creates a nested
expression-local declaration/result scope. This plan standardizes new
`WHERE` teaching source on an explicit `BLOCK` when a returned expression needs
temporary declarations; it does not require a pointless temporary for a direct
pipeline.

A direct returned expression needs no `BLOCK`:

```boon
FUNCTION choose_nonnegative(value, fallback) WHERE {
    fallback >= 0
} {
    value >= 0
    |> WHEN {
        True => value
        False => fallback
    }
    |> WHERE result {
        result >= 0
    }
}
```

A nested returned calculation with explicit temporary declarations uses
`BLOCK`:

```boon
FUNCTION bounds(input) WHERE {
    input >= -1000000
    input <= 1000000
} {
    BLOCK {
        lower_value: input - 1
        upper_value: input + 1

        [
            lower: lower_value
            upper: upper_value
        ]
        |> WHERE result {
            result.lower == lower_value
            result.upper == upper_value
            result.lower <= result.upper
        }
    }
}
```

There is no special declaration syntax inside a condition block. Reusable
predicate calculations are declared as ordinary verified pure functions.

Regardless of how a function body is written, none of its declarations is
visible from the function-header condition scope.

### Rejected Forms

These are invalid in V1:

```boon
WHERE {
    condition
}
```

```boon
value WHERE {
    condition
}
```

```boon
value
|> WHERE {
    condition
}
```

```boon
value
|> WHERE(value) {
    condition
}
```

```boon
FUNCTION f(value) REQUIRE {
    condition
} {
    value
}
```

The required explicit alias and the absence of a bare statement preserve
locality. The special no-parentheses pipeline shape also distinguishes `WHERE`
from an ordinary runtime function call.

## Condition Semantics

A condition block is one static logical formula formed from pure clauses whose
source type is the closed ordinary Tag set `True | False`.

Each condition must:

- typecheck as a present `True | False` Tag value;
- be deterministic;
- be pure;
- be total under the block's admitted assumptions;
- use modeled Boon semantics;
- avoid effects, state writes, asynchronous completion, host time, and hidden
  runtime identity.

A condition may refer to an immutable current payload or branch/event snapshot
already established in its lexical proof context. It may not create a `SOURCE`,
assume a future arrival, inspect hidden event sequence identity, or reread an
uncontrolled live source as though it were a stable value.

The verifier may use all conjuncts when establishing that the complete formula
is defined. Source order never turns one condition into an imperative guard
for the next.

For a piped refinement, every required clause obligation is:

```text
under the existing admitted path and facts:
    Defined(subject)
    and Defined(condition)
    and Value(condition) == True
```

Declared pure truth-valued helper functions are valid only when their bodies or
checked contracts are available to the verifier. An undeclared name is ordinary
invalid Boon syntax; formal verification does not create an implicit predicate
namespace.

The verifier may normalize these two Tags into a private logical Boolean sort.
That sort is solver machinery only. It never appears in source types, checked
public signatures, persistence, wire schemas, reports as a Boon type, or
runtime values.

Unstratified cycles in proof dependencies are rejected. A condition cannot
justify itself through a downstream result or through a mutually circular set
of unproved refinements.

The one recognized well-founded proof cycle is `HOLD` induction:

- the base proof cannot use the induction hypothesis;
- the step proof may use the invariant only for the prior committed state;
- a candidate checkpoint cannot justify itself;
- no fact travels backward across an ordinary non-temporal edge;
- multi-cell induction remains rejected unless state is structurally grouped.

Because clause labels are intentionally absent from source syntax, stable
condition ids are derived from the enclosing checked semantic owner, normalized
formula hash, and a duplicate disambiguator. Formatting and movement within the
same condition block do not change a distinct clause id.

## Proof Responsibility And Fact Flow

### Header Polarity

For:

```boon
FUNCTION f(value) WHERE {
    value >= 0
} {
    value
}
```

the definition is checked under `value >= 0`, while each caller proves the same
condition after substituting its actual argument.

Moving a condition from a returned value to the header is therefore not a
cosmetic refactor. It moves responsibility from implementation to callers and
is an API-contract change.

### Producer Polarity

For:

```boon
candidate
|> WHERE result {
    result >= 0
}
```

the containing implementation must establish `candidate >= 0` at that exact
point.

Facts become available only after the obligation is discharged. A failed or
unknown obligation produces no verified program.

### Function Composition

Verified function signatures export:

- exact ordinary-parameter shape and evaluation scopes;
- exact inferred `PASSED` contextual-formal shape;
- exact `OUT` and compiler-supplied contextual shape;
- checked input theorem;
- checked returned-value theorem and presence quantification;
- the public callable/effect shape and theorem-statement hash;
- a separately bound evidence bundle containing complete capture, call,
  resource, external, type, flow, implementation-effect, and persistence
  dependency footprints;
- source-bundle, semantic-profile, verification-manifest, and assurance
  provenance.

Calls substitute exact checked argument identities and resolved contextual
formal bindings into input conditions. Returned facts are substituted back
into the caller's proof context. Inlining may accelerate verification but
cannot change the contract semantics.

No backend or distributed compiler may rediscover contracts from function-name
strings.

## `OUT`, `PASS`, And `PASSED`

These three spellings must not be grouped under one proof rule.

`PASS`/`PASSED` is a parameter-passing alternative: the caller supplies one
structural context record explicitly, or a nested call inherits the caller's
record statically. For verification, each required `PASSED.path` is an implicit
named contextual formal and works like an ordinary parameter.

`OUT` points in the other direction. The called function or contextual
operation supplies an output binder, often once per row or other repeated
materialization. It is not a caller-supplied header input.

The value directions are:

```text
ordinary actual             caller -> explicit ordinary formal
PASS record                 caller -> implicit PASSED contextual formals
inherited PASSED context    enclosing function -> nested contextual formals
OUT                         called producer -> dependent call expression
output-evaluated argument   caller computation -> each OUT materialization
compiler context            compiler/host provider -> exact call materialization
```

| Construct | Where it may affect proof in V1 | Reusable public contract? | Who must fix a failure? |
|---|---|---|---|
| Parent-evaluated ordinary input | Header requirement, returned theorem, or local piped refinement | Yes | The caller for a header; the local producer for a pipeline |
| `PASSED.path` | Header requirement, returned theorem, or local piped refinement | Yes, as an implicit contextual formal | The explicit or inherited context provider for a header |
| `PASS` | Never a formula term; it binds contextual actuals at a call | No independent theorem | The caller assembling the context |
| `OUT` | Local piped refinement after contextual binding | No app-authored `OUT` contract | The producer/contextual operation or its input facts |
| Output-evaluated ordinary parameter | Local proof for every output evaluation | Only through a compiler-owned operation summary in V1 | The caller-supplied dependent computation |
| Compiler-supplied context such as `element` | Local proof at its exact provider materialization | No app-authored public contract in V1 | The context provider and local consumer |

### `PASS` And `PASSED` Are Contextual Parameters

`PASS` itself cannot appear in a condition. It is a separately parsed call
clause, not a Boon runtime value. The expression after `PASS:` constructs the
callee's complete context. It may reuse fields from the current `PASSED`
context to implement an application-level extension, narrowing, or
replacement, but there is no implicit runtime merge or global context stack.

Every direct `PASSED.path` read and every requirement inherited from a nested
call contributes to the function's inferred contextual signature. The checked
representation must assign stable `ContextFormalId`s to those paths. It must
not use one compilation-global widened `PASSED` record assembled from unrelated
call sites.

A nested callee requirement propagates into the caller's context scheme only
when that call inherits context. An explicit `PASS:` cuts that propagation;
instead, the ordinary/context/resource dependencies used to construct its
explicit actual belong to the caller's dependency manifest and call
obligation. Compatible inherited requirements merge at a finite principal-row
fixed point. Proof guarantees never participate in that type fixed point or
justify their own contract cycle.

At each call edge, the checked program records exactly one of:

```text
ContextBinding::Explicit(expression)
ContextBinding::Inherited(context_formal)
ContextBinding::None
```

`None` is legal only when the callee requires no contextual fields. Explicit
`PASS:` replaces the inherited record for that call; its record expression may
explicitly reuse inherited fields. The contextual scheme is structurally
row-polymorphic: compatible extra fields are allowed but cannot influence a
theorem unless projected. Missing fields, incompatible field types or flows,
and inheritance across a forbidden role, ownership, asynchronous, or snapshot
boundary fail during semantic checking. The complete actual expression and
its dependencies remain in the call proof-context key even when the callee
projects only a subset.

Bare `PASSED` and record-valued prefixes such as `PASSED.store` require an
observation rule; they are not silently reduced to their currently known leaf
paths:

- **projected observation** uses a finite set of named descendant fields and
  keeps an open row tail;
- **transparent context forwarding** in an explicit `PASS:` preserves the
  whole row variable and origin without observing its extra fields;
- **whole/subrecord observation** includes structural equality, shape-sensitive
  spread/serialization, opaque-helper use, or a contract/result whose meaning
  depends on the complete record.

V1 permits the first two. A proof-relevant whole/subrecord observation is
accepted only when semantic analysis establishes a closed exact subtree and
binds that complete shape into the callable theorem and cache key; otherwise
the contracted use is rejected. Existing source with no `WHERE` retains its
current behavior. This prevents extra context fields from changing a theorem
that was hashed only from selected projections.

Ordinary actuals and the explicit `PASS:` expression are evaluated in the
caller's environment. A `PASSED` projection inside that expression therefore
means the caller's context; the rebound callee context becomes visible only
inside the callee. This prevents accidental self-reference.

A function can state a contextual caller requirement directly:

```boon
FUNCTION total_value() WHERE {
    PASSED.store.total >= 0
} {
    PASSED.store.total
    |> WHERE result {
        result == PASSED.store.total
        result >= 0
    }
}

result:
    total_value(
        PASS: [store: store]
    )
```

The function theorem is checked symbolically:

```text
for all ordinary formals p and required PASSED context formals c:
    Header(p, c)
    implies every body obligation
    and every produced-present returned guarantee
```

The call obligation substitutes `store` for the contextual formal and proves
that the header is defined and true. A second call with a different store uses
the same modular theorem with a different substitution.

Nested calls inherit the same formal rather than forcing repeated plumbing:

```boon
FUNCTION total_panel() WHERE {
    PASSED.store.total >= 0
} {
    total_value()
}
```

`total_panel` uses its own header assumption to discharge `total_value`'s
inherited-context requirement. The compiler never silently infers a stronger
public precondition from the body. If the wrapper omits the needed header or
cannot establish it by another verified fact, the wrapper fails verification.

Required contextual path, structural type, `FlowMode`, presence/error shape,
snapshot/role scheme, and observation mode are part of the callable's public
context scheme and theorem hash. Whether one concrete caller binds that scheme
with explicit `PASS:` or inherited forwarding belongs to the call proof-context
and private evidence/cache identity, not the callee's public API identity. A
returned theorem may relate its result to `PASSED.store.total`, just as it may
relate the result to an ordinary parameter.

The quantification is per semantic call instance. If the `PASS` actual itself
depends on an `OUT` row, element context, or other repeated owner, the callee
theorem remains modular but that caller obligation is instantiated for every
such materialization. This is the same distinction as a normal generic
function theorem versus each concrete call.

“Per call” does not mean a one-time initial-value copy. Context fields retain
their ordinary Boon flow/currentness semantics, so the theorem ranges over
every admitted current value or present payload over time.

An unused function that depends only on ordinary formals plus projected
open-row-transparent or statically closed `PASSED` formals can be verified
symbolically. It does not require a concrete call merely because it uses
`PASSED`; an unclosed proof-relevant whole-context observation is rejected
rather than sampled from calls.

### Local Proofs Over `OUT`

An `OUT` binding may be used by a piped `WHERE` inside the lexical call scope in
which its concrete contextual payload exists:

```boon
rows
|> List/map(
    item
    new:
        item
        |> WHERE row {
            row.points >= 0
            row.points <= 100
        }
)
```

This does not assume that every arbitrary `item` is valid. The obligation is
quantified over every item payload that the resolved `List/map` invocation may
supply. It can be discharged only from:

- facts already attached to the input list;
- a verified standard-operation summary;
- facts proved by the producer of each row;
- the current branch and materialization context.

Forwarding `item: entry` through a wrapper preserves the exact contextual
binding, facts, and proof provenance. Forwarding does not prove a stronger
condition. `OUT` remains static wiring and never becomes a storable proof port,
runtime handle, or hidden identity.

Application-authored public contracts over `OUT` are deferred in V1. Standard
operations may expose compiler-owned contextual summaries. An app can still
prove a local property of a concrete `OUT` payload where it is used.

An ordinary-looking argument is not always parent-evaluated. Current checked
calls record `CheckedEvaluationScope::Output { formal }` for arguments such as
contextual `new`, `if`, or `key` computations. Such a formal denotes a
dependent computation evaluated in each `OUT` scope, not one value supplied
before the call. V1 excludes it from header `WHERE` and from app-authored
public result formulas. Its local obligations are instantiated for every
output materialization, and only an explicit compiler-owned operator summary
may export a general theorem about it.

Forwarding an `OUT` through wrappers must preserve its output net, evaluation
port, repeated owner, concrete scope, type substitutions, and proof
provenance. Two calls with the same coarse output ownership but different
`PASS` actuals are distinct proof contexts.

### Other Compiler-Supplied Contexts

`CheckedCallableContext`/`CheckedCallContext` is a third channel, separate from
both `PASSED` and `OUT`. The current example is `ElementState`, exposed to Boon
as fields such as `element.hovered`. The compiler creates it for a concrete
render-call context and injects it into dependent argument expressions.

These values are provider/materialization-local in V1:

- they cannot appear in a function header;
- they may be inspected by a local piped `WHERE` once the exact context exists;
- they cannot remain free in a public result theorem;
- their provider identity, flow type, projection, and materialization are part
  of the proof-context key;
- hidden element ids, generations, or renderer ownership never become formula
  variables.

### Contextual Soundness Rules

- `PASS` remains final and separate from ordinary call entries.
- `PASSED` contextual formals use the same substitution and caller-obligation
  model as parent-evaluated ordinary parameters.
- An explicit `PASS:` is a complete checked rebinding for its callee; a nested
  call with no `PASS:` forwards the exact inherited formal.
- Proof facts flow forward through the same resolved contextual connections as
  values; they never flow backward from a result into its passed context.
- A call obligation whose actual context depends on `OUT` or another repeated
  provider is instantiated for every relevant row, scope, and call
  materialization, not sampled from one runtime instance.
- Presence and typed-error rules remain unchanged for contextual payloads.
- Hidden row keys, generations, runtime ids, and context implementation details
  never enter formulas or reports.
- Cross-module use may serialize normalized `PASSED` contextual formals as part
  of a verified callable theorem. Cross-role transport may not serialize
  implicit context, `OUT`, or provider-local facts; it must reify bounded
  ordinary data under a verified transport contract or fail closed.
- Cycles through `OUT`, `PASS`, or `PASSED` are checked by the shared contextual
  semantic graph before verification; proof facts cannot make an invalid
  wiring cycle legal.

## Complete Function Input And Dependency Model

The declared parameter list is not a function's complete semantic interface.
For verification, a dependency is anything that can change the returned value,
flow presence, typed error, event correlation, state transition, effect,
currentness, ordering, or truth of a proof. Every dependency must be classified
even when it is erased, implicit, compiler-generated, or not addressable from
Boon source.

The current language has the following closed inventory:

| Dependency channel | Current examples/representation | Verification treatment |
|---|---|---|
| Parent-evaluated ordinary formal | Named parameter; pipe supplies the first | Universal boundary formal; header and result eligible |
| Implicit context formal | `PASSED.store.*` from explicit or inherited `PASS` | Universal boundary formal; header and result eligible |
| Producer output formal | fresh/forwarded `OUT`, contextual list row | Universal over concrete output materializations; local in V1 |
| Output-evaluated ordinary formal | contextual `new`, `if`, `when`, `key`, or similar argument with `EvaluationScope::Output` | Dependent computation per output materialization; local/summary-only in V1 |
| Compiler-supplied call context | `ElementState`, including `element.*` | Provider/materialization-local; never a V1 header formal |
| Immutable lexical capture | literal-derived/module/root constant | Fixed, fully hashed dependency; header use only when compiler-certified closed and immutable |
| Dynamic lexical capture | outer field, record/BLOCK/pattern binding, row value, root/module value | Closure-converted exact capture; never silently treated as a parameter |
| Source/event input | `SOURCE`, source payload projection, tick presence and event sequence | Universal over every admitted present payload; no inferred liveness |
| State/list authority | `HOLD`, prior-state alias, `LATEST`, mutable `LIST`, row state, restored persistence | Snapshot/transition/list induction with exact owner and mutation coverage |
| Local declaration/path fact | function body field, `BLOCK`, record sibling, pattern binder, branch fact | Internal proof binder with lexical/path lifetime; not a boundary input |
| Callable dependency | user/module function, builtin, contextual operator, pure helper | Exact callable identity plus transitive theorem, summary, capture, and effect closure |
| Omitted/defaulted call input | optional standard/render/effect argument | Canonical semantic default is an explicit hashed dependency; omission and the equivalent explicit value normalize identically |
| Host effect and completion | typed host call, request intent, asynchronous/single/stream result | Conditions cannot invoke it; result facts require a versioned provider contract and retain typed failure |
| Role-qualified dependency | `Client/store.*`, `Session/store.*`, `Server/store.*`, remote pure call | Explicit import/transport assume-guarantee dependency; contracted crossing rejected in V1 |
| Runtime-context/ambient intrinsic | `SessionInfo/*`, route/current-context and identity-generation operations | Explicitly classified host/provider input or effect; never assumed pure merely because it has no arguments or coarse effect bits |
| Migration authority | `DRAIN`, `DRAINING`, predecessor plan/catalog and restored leaves | Phase 4 relation over exact old authority, transform, and new authority |
| Persistence activation provenance | invariant stamp, last-writer manifest, persistence compatibility key, commit protocol | Host assurance/coverage dependency for restored authorities; never a formula value |
| Type/flow instance | type substitution, structural row, `FlowMode`, presence/error shape | Alpha-normalized universal or concrete instance in every signature and obligation |
| Structural/representation semantics | tag/variant set, record field order/open tail, list capacity, fixed/dynamic bytes size, render contract | Public shape or private evidence dependency according to visibility; never reconstructed from a coarse type name |
| Semantic metadata | statement value-use/render-slot result kind, currentness, order chain/dynamic direction, event/source/host-port correlation, possible-cause graph, target Number/list profile | Non-value semantic dependency included in proof and evidence/cache identity |
| Verification/assurance input | imported theorem bundle, standard summary, certificate/kernel, verifier policy/version | Determines whether proof evidence is acceptable; does not become a Boon formula binder or runtime value |
| Hidden routing identity | row/source/session ids, owner keys, generations, cursor authentication, runtime/plan ids | May control coverage/routing but is forbidden as a formula variable or reported app value |

These rows intentionally include more than value inputs. A dependency receives
one or more explicit roles:

- a **logical binder** quantified in a public or local formula;
- a **fixed semantic definition** used while proving that formula;
- a **resource/provider behavior** over which the proof ranges or from which it
  imports a stated guarantee;
- a **coverage/routing discriminator** that determines which instances, paths,
  transitions, rows, or outcomes must be checked;
- an **assurance dependency** needed to accept evidence or safely activate a
  persisted verified artifact.

Only the first category becomes an app-visible theorem variable. The other
categories are still required for soundness, invalidation, coverage, or trust,
but private implementation changes among them must not masquerade as a public
contract/API change.

This inventory covers every current checked declaration category:

| `CheckedDeclarationKind` | Dependency class above |
|---|---|
| `ValueParameter` | Parent-evaluated or output-evaluated ordinary formal, according to its checked evaluation scope |
| `OutParameter`, `FreshOut` | Producer output formal and exact output net/materialization |
| `PatternBinding` | Branch-local declaration/path fact, or a closure capture if a nested function reads it |
| `Field` | Closed immutable constant, local declaration, or dynamic/resource capture after transitive classification |
| `Source` | Source/event input and payload/correlation metadata |
| `Hold` | State authority and proof-only pre-state/transition witnesses |
| `List` | List authority, row scope, mutation, ordering, and currentness |
| `ElementState` | Compiler-supplied call context |
| `Function`, `Builtin` | Statically resolved callable, summary/default, intrinsic, or host-effect dependency |
| `External` | Role-qualified value/call and transport/provider dependency |

`Root`, `Function`, `Block`, `Record`, `RepeatedOutput`, and `CallContext`
scopes determine which instance owns each dependency and whether it is
boundary-visible, lexical, repeated, or provider-local.

`Function`, `Field`, `Source`, `Hold`, `List`, `Block`, `Spread`, and
expression statement kinds preserve declaration/ownership semantics that
cannot be reconstructed from a child expression alone, including source event
shape, `HOLD` identity, list capacity, spread provenance, and statement
value-use. Match-pattern variants determine path conditions, coverage, and
bound projections. Callable kind, parameter kind, and evaluation scope
determine which provider owns each obligation.

Every current checked expression category is also covered. `Read`, `Passed`,
`ExternalRead`, and `Drain` identify value/resource origins; `Source`, `Hold`,
`Latest`, and `Draining` introduce source/state/migration semantics; `Call`
introduces ordinary/context/`OUT` actuals plus callable dependencies; and
`When`, `While`, `Then`, `Infix`, `MatchArm`, `Block`, text templates,
records/objects/tagged objects, lists, and bytes compose child dependencies
under their path, flow, declaration, spread, and evaluation rules. Literals and
delimiters introduce no ambient value input. `Invalid` is never verifiable.

Literals have no external value dependency, but their exact checked value and
Number/BYTES/TEXT semantics still participate in the containing expression
digest. Function names and types alone never stand in for the dependency
closure.

The current type model has a `Function` shape, but every checked call edge
still names a statically resolved callable declaration; the checked program has
no opaque runtime call target as another input channel. If independently
flowing or dynamically selected first-class function values are added later,
their formal type must include a quantified behavioral contract covering
ordinary/context/`OUT` inputs, presence, errors, captures, and effects. They
cannot be admitted by treating a function value as an address, display name,
or coarse argument/result type.

### Closure Conversion For Proofs

The semantic layer constructs one exhaustive manifest for every callable:

```text
CallableDependencyManifest {
    callable_semantic_key
    parent_value_formals
    passed_context_formals
    passed_context_observation_modes
    out_formals
    output_evaluated_formals
    compiler_contexts
    lexical_captures
    resource_reads_and_writes
    called_callables_and_summaries
    normalized_defaults_and_requiredness
    external_values_and_calls
    runtime_intrinsics_and_host_effects
    migration_predecessors
    persistence_activation_requirements
    type_and_flow_instances
    structural_and_representation_semantics
    semantic_profile_dependencies
}
```

Each entry uses stable semantic identities and records the exact projection,
type, flow/presence/error shape, evaluation scope, role/owner, and relevant
snapshot or event phase. It also records multiplicity (once, per tick, per
event, per row/materialization, per transition, or per activation), lifetime,
public/private visibility, alias/resource origin, and whether the dependency
is a logical binder, fixed definition, behavior, coverage discriminator, or
assurance input. This is compiler-internal closure conversion; it adds no app
keyword and does not force developers to manually repeat ordinary lexical
dependencies as parameters.

Optionality is semantic, not parser trivia. Every callable signature retains
requiredness and either a canonical checked default expression or an explicit
semantic-default summary. An omitted argument and its exactly equivalent
explicit default normalize to the same call; changing a default invalidates
the callable and dependent proof caches.

A default is traversed through the same exhaustive dependency analysis as an
explicit actual. A closed public default and the public semantic definitions
needed to interpret it belong to the public callable shape. A default that
reads mutable state, source/event data, host context, `PASSED`, `OUT`, or
another materialization cannot be hidden behind “omitted”: V1 either treats
the exact call as materialization-local under a compiler-owned contextual
summary or rejects exporting a modular app-authored theorem for that default.

The manifest distinguishes:

- **formula binders**, which may occur free in a normalized public statement;
- **implementation dependencies**, which may be used to prove the statement
  and are bound by semantic/evidence hashes;
- **assumption dependencies**, whose theorem or provider guarantee must be
  visible and accepted by policy;
- **coverage dependencies**, such as every row, transition arm, effect outcome,
  or predecessor schema over which the proof must range.

A result-only theorem is not automatically independent of ambient state merely
because that state does not occur syntactically in the formula. The verifier
checks both free binders and the complete provenance of assumptions used by
the proof. Conversely, a function may read state or an effect result and still
export an unconditional extensional fact if it proves that fact for every
modeled state or result, or from an explicitly referenced verified provider
invariant.

### Lexical Captures

Current function scopes can resolve declarations from parent scopes. These
captures live in checked `Read { target, projection, source }` nodes rather
than in `CheckedCallableSignature.parameters`, so the compiler must compute
them explicitly.

Capture rules are:

- a closed immutable constant or context-free pure helper may be used in a
  header; its normalized value/callable semantics and transitive theorem hashes
  are part of the contract;
- record fields and spreads are traversed through their resolved provenance;
  a structurally ordinary record containing a source/state/list authority alias
  or later `resource_only` field is a dynamic resource capture, not a closed
  constant;
- any other capture is excluded from the header because its value is not
  supplied by that caller;
- a private capture cannot remain a free binder in an exported formula;
- a proof over the fixed implementation may use an exact private capture only
  through its recorded definition, resource semantics, or verified provider
  theorem;
- changing a captured definition, projection, flow, owner, or dependency
  invalidates the implementation evidence bundle and every dependent proof
  cache entry, but does not change the public contract-statement hash when its
  public shape and formulas are unchanged;
- nested, shadowed, projected, record/BLOCK/pattern, repeated-row, and detached
  persisted captures all receive distinct stable semantic identities;
- compiler-created row capture fields are proof implementation details and
  never leak into source formulas or reports.

This keeps the source local: developers use normal lexical names, while the
compiler makes their proof consequences explicit.

### State, Sources, Lists, And Effects

The existing four Boolean `CheckedEffectSummary` fields are useful as a coarse
warning but are not a sound dependency or cache key. Two functions can have
the same four bits while reading different `HOLD` cells, different source
payloads, or different host operations.

Likewise, the current order-key `expression_is_pure` and
`expression_is_total` helpers are not verification classifiers: they may treat
ambient reads as leaf expressions and break recursion optimistically. `WHERE`
uses a new exhaustive semantic purity/definedness analysis over the complete
dependency manifest. It fails closed on cycles or unclassified intrinsics.

`SemanticProgram` therefore records a resource-specific transitive footprint:

- exact source ports and payload projections, including flow presence,
  sequence/correlation, scope, and interval metadata;
- exact state/list authorities and projections read or written, including
  current snapshot, previous committed state, candidate write, and commit
  phases;
- list row owners, contextual callbacks, captures, mutations, order/currentness
  provenance, and hidden retained-state capture semantics;
- exact host operation, typed intent/result/error schema, completion mode,
  provider contract, durability/idempotency policy, and assurance dependency;
- exact external role value/call, transport/result flow, provider theorem, and
  trust boundary.

No-argument ambient operations—including current session/route/context reads
and identity generation—must each have an explicit semantic classification.
Argument count and the coarse effect bitset never imply referential
transparency.

Conditions remain pure: a `WHERE` block cannot issue a source, mutate state, or
invoke a host operation. A piped `WHERE` may inspect an already produced
effect or external result as ordinary typed data. Any claim beyond its
structural type must come from a visible accepted provider theorem; proof never
invents success, availability, durability, ordering, or liveness.

### Call And Proof Instantiation Identity

Every call obligation and materialization-local checkpoint has a proof-context
key covering:

```text
callable semantic key
ordinary actual bindings, required/default status, and evaluation scopes
PASSED origin, explicit/inherited binding, and forwarding frames
OUT net, port, owner, evaluation scope, and materialization
compiler-supplied contexts
lexical capture bindings
alpha-normalized type substitutions
flow/presence/error and event/snapshot modes
statement value-use/materialization-result kind and host-port correlation
canonical producer role, route scope, wire edge, and invocation/currentness mode
referenced theorem, summary, provider, and profile hashes
```

The key is intentionally more complete than output ownership provenance.
Changing only a `PASS` actual, captured state owner, host operation, type
instance, or predecessor catalog must prevent proof-cache reuse.

Type variables are alpha-normalized; process-local `TypeVar` numbers are never
serialized as theorem identity. Recursive type-requirement inference may use a
least fixed point, but proof contracts may not justify one another circularly.
A recursive proof requires an explicit well-founded induction supported by the
verifier or is rejected.

### Exhaustiveness Gate

No existing field named `dependencies` is assumed complete. Today, local read
edges, state/source dependencies, distributed imports, output materialization,
list callbacks, migration graphs, and executable child edges are stored in
several different structures.

Phase 0 adds one exhaustive semantic dependency enumerator. It must:

1. classify every `CheckedScopeKind`, `CheckedDeclarationKind`,
   `CheckedStatementKind`, `CheckedExpressionKind`, `CheckedMatchPattern`,
   `CheckedTextSegment`, `CheckedCallableKind`, `CheckedValueUse`,
   `SemanticOccurrenceKind`, parameter/evaluation-scope discriminant,
   `CheckedCallEntry`, callable/call context, contextual operation,
   order-direction variant, `FlowMode`, type/shape variant, `ProgramRole`, and
   every later semantic resource variant, including an explicit “no semantic
   dependency” result where appropriate; this traversal also covers side
   tables for pattern selectors/projections, source-unit/module identity,
   source payload/scoped/interval metadata, order chains and dynamic
   directions, type substitutions, `PASS`, contextual materializations,
   state-update arms, list mutations, detached captures, statement
   value-use/materialization-result kinds, output-root/named-value/render-slot
   metadata, host-port endpoint/correlation metadata, record-field/spread
   provenance, source/expression/function type tables, and later resource-only
   aliases;
2. reject a new or unknown variant until its dependency, quantification,
   visibility, erasure, hashing, and diagnostic rules are defined;
3. compare the enumerated transitive closure with every proof obligation,
   theorem signature, cache key, and erasure mapping;
4. test that changing each dependency class changes the appropriate digest or
   invalidates verification;
5. prove that no hidden runtime identity becomes a logical term even though
   its materialization coverage remains exact.

Enum exhaustiveness alone is insufficient because a new ordinary struct field
can otherwise compile while being silently ignored. Phase 0 therefore also
adds an exhaustive field schema for every dependency-bearing checked,
semantic, lowering-boundary, and proof record. At minimum this covers all
fields of `CheckedProgram`, `CheckedProgramLoweringMetadata`, scopes,
declarations, statements, expressions and their child records, callables,
parameters, calls, call entries, contexts, source/pattern/order/type tables,
external environments, semantic resource/materialization records, migration
records, and proof-relevant erasure bridges such as
`ErasedFieldDef.resource_only`.

Every field has exactly one explicit disposition record; a traversed field may
carry several semantic roles:

```text
DependencyFieldDisposition {
    traversal: Recurse | SemanticAtom | None
    roles: {
        FormulaBinder
        ResourceOrProvider
        CoverageOrRouting
        AssuranceOrActivation
        DiagnosticOrSource
        IntentionallyNonSemantic
        ForbiddenInVerifiedSlice
    }
    public_or_private_visibility
    hash_and_erasure_policy
}
```

The implementation must make field addition fail closed. Use generated field
tags/derive support or owned exhaustive destructuring with no `..`, backed by
an architecture check that compares the exact source-level field set with the
classifier registry. Wildcard/rest destructuring, silent `serde` flattening,
and default “ignore unknown field” behavior are forbidden in this boundary.
Adding, renaming, or removing a field changes the generated schema fingerprint
and fails compilation or the architecture test until its traversal, public/
private visibility, quantification, erasure, hashing, cache, and diagnostic
rules are reviewed. Fields classified diagnostic/nonsemantic are named just as
explicitly as semantic fields. The canonical classifier/schema fingerprint is
stored as `dependency_classifier_schema_hash` in every required-obligation
manifest, so cached evidence cannot cross an unreviewed classifier change.

A contract slice with unknown resource coverage or a relevant dynamic lowering
fallback fails closed. Unrelated legacy UI regions need not block an
independent proof when semantic slicing proves they cannot affect its
statement, assumptions, or coverage.

This closed inventory is a V1 soundness gate, not optional optimizer metadata.

## Reactive Semantics

Proofs use Boon's existing reactive and presence semantics. They do not impose
an imperative execution model.

### `WHEN` And `WHILE`

Branch matches and `True | False` conditions enter the corresponding proof
context.

For:

```boon
selector
|> WHEN {
    First => first_value
    Second => second_value
}
|> WHERE result {
    result >= 0
}
```

the verifier proves `result >= 0` for every reachable value-producing branch.
A fact survives the merge only when every reachable producer establishes it.

The same path-sensitive rule applies to continuous `WHILE`.

### `THEN`, Events, And `SKIP`

For:

```boon
event
|> THEN {
    result
}
|> WHERE value {
    value >= 0
}
```

the condition applies to each present result. It does not prove that the event
is present, eventually occurs, or occurs within a deadline.

`SKIP` remains absence, not a value that vacuously satisfies an application
predicate.

### `LATEST`

A condition after `LATEST` must hold for every candidate that may legally win:

```boon
LATEST {
    candidate_a
    candidate_b
}
|> WHERE selected {
    selected >= 0
}
```

The proof model must use the real event-sequence, presence, tie, `PRIORITY`, and
`EXCLUSIVE` semantics. It may not assume a source order or preferred winner that
the language does not guarantee.

A condition on only one candidate refines only that candidate. It does not
automatically refine the merged `LATEST` result.

If no candidate is present, `LATEST` produces `SKIP`; an outer `WHERE` does not
prove that a winner exists. Equal greatest event sequences remain a hard
definedness failure unless `PRIORITY` or proved `EXCLUSIVE` semantics resolves
them. Proving a predicate about every legal selected value does not by itself
prove conflict freedom; the required subject-definedness obligation does.

### `HOLD` Transition Assertions

The `HOLD` alias remains visible only inside its body:

```boon
0
|> HOLD before {
    event |> THEN {
        before < 5
        |> WHEN {
            True =>
                before + 1
                |> WHERE after {
                    after == before + 1
                }

            False => before
        }
    }
}
```

No proof feature extends `before` outside the `HOLD` body.

The checked implementation may currently resolve the alias and owning
declaration to the same declaration identity. Verification must mint a
distinct proof-only pre-state witness tied to the exact semantic state id,
update arm, cause snapshot, and owner. Otherwise an invalid transition could
collapse `before` and `after` into one logical term. This witness is erased and
never becomes a Boon binding.

### `HOLD` Invariants

A piped `WHERE` on a `HOLD` result is verified by induction:

```boon
initial
|> HOLD before {
    next_candidates
}
|> WHERE current {
    current >= 0
}
```

The compiler generates:

```text
base:
    prove initial is defined
    prove initial >= 0

step:
    assume before >= 0
    assume the selected triggering path and established input contracts
    prove the selected candidate is defined
    prove after >= 0 for every candidate that may commit

no update:
    after = before

activation:
    fresh authority -> use the proved initializer base
    restored authority -> require compatible verified invariant provenance
```

The proof engine may internally retain the previous state, trigger snapshot,
candidate set, and selected next state as proof witnesses. Executable IR keeps
only ordinary `HOLD` state.

The source initializer is not the runtime base when persistence restores an
existing authority. V1 therefore adds a host/persistence activation gate, not
a hidden app assertion:

```text
AuthorityActivationRequirementV1 {
    authority_persistence_key
    authority_semantic_schema_hash
    invariant_statement_hash
    persistence_compatibility_key
    accepted_commit_protocol_versions
    checker_policy_and_version
}

LastWriterActivationBasisV1 =
    FreshInitializer {
        base_obligation_id
        base_evidence_hash
    }
    | RestoredRun {
        activation_receipt_hash
    }

VerifiedAuthorityInvariantStampV1 {
    authority_persistence_key
    authority_semantic_schema_hash
    invariant_statement_hash
    last_writer_semantic_program_digest
    last_writer_verification_manifest_digest
    last_writer_activation_basis: LastWriterActivationBasisV1
    persistence_compatibility_key
    commit_protocol_version
}

VerifiedActivationReceiptV1 {
    artifact_source_bundle_digest
    current_verification_manifest_digest
    activation_requirement_hash
    authority_persistence_key
    restored_value_digest
    restored_stamp_digest
    resolved_last_writer_program_digest
    resolved_last_writer_manifest_digest
    checker_policy_and_version
    status = accepted
    receipt_hash
}
```

`authority_persistence_key` is the host's stable logical authority identity,
not a process-local `SemanticStateId`. The domain-separated
`invariant_statement_hash` covers that key, the exact authority/schema shape,
the normalized invariant formula, and every contract-visible semantic profile
or definition needed to interpret it. It excludes source spans, private
dependency manifests, provider program digests, proof timings, and local
checkpoint ids.

All activation identities use the same specified canonical encoding discipline
as contract bundles and have non-recursive, domain-separated derivations:

```text
invariant_statement_hash =
    H(
        "boon.authority-invariant-statement.v1",
        canonical(
            authority_persistence_key,
            authority_schema,
            normalized_invariant_formula,
            contract_visible_semantic_definitions
        )
    )

persistence_compatibility_key =
    H(
        "boon.persistence-compatibility.v1",
        canonical(
            storage_schema_and_codec,
            authority_layout,
            crash_consistent_commit_contract
        )
    )

activation_requirement_hash =
    H(
        "boon.authority-activation-requirement.v1",
        canonical(AuthorityActivationRequirementV1)
    )

base_evidence_hash =
    H(
        "boon.accepted-obligation-evidence-core.v1",
        canonical(AcceptedObligationEvidenceCoreV1 for base_obligation_id)
    )

restored_value_digest =
    H(
        "boon.restored-authority-value.v1",
        canonical_decoded_value(authority_schema, installed_value)
    )

restored_stamp_digest =
    H(
        "boon.authority-invariant-stamp.v1",
        canonical(VerifiedAuthorityInvariantStampV1)
    )

receipt_hash =
    H(
        "boon.authority-activation-receipt.v1",
        canonical(VerifiedActivationReceiptV1 without receipt_hash)
    )
```

`canonical_decoded_value` is the versioned persistence value codec, not Rust
debug output, host-endian memory, or unordered `serde` maps. The requirement
list is sorted by stable authority key. Stamp DTOs contain no self hash; a
receipt's stored `receipt_hash` is excluded exactly as shown. Readers
recompute and compare every requirement, statement, compatibility, value,
base evidence, stamp, manifest, and receipt digest before acceptance.

Compilation enumerates and hashes
`AuthorityActivationRequirementV1` for every persisted contracted authority.
The reusable verified artifact and executable plan carry those static
requirements without reading a deployment's store. They contain no claim that
a particular restored value has already passed activation.

The persistence layer commits the authority value and stamp under the same
crash-consistent protocol. On activation, the host validates integrity and
semantic compatibility before the verified artifact may use the restored
value. It checks the exact authority, schema, invariant statement, persistence
compatibility key, and commit protocol. It also resolves and validates the
last-writer program/manifest evidence and activation basis named by the stamp
under the current assurance policy, while independently requiring the current
artifact to have proved the same invariant and all of its current transitions.

Only after those deployment-specific checks does the host create a
`VerifiedActivationReceiptV1` binding the exact artifact, current verification
manifest, static requirement, restored value digest, restored stamp, resolved
last-writer evidence, and checker version. The runtime must possess a valid
receipt before installing that restored authority into the graph. The receipt
is runtime/host evidence, not part of `VerificationManifest`,
`ContractVerifiedProgram`, a public theorem, or compile-time proof success.
Fresh initialization follows the proved initializer base and writes the first
stamp with `FreshInitializer` naming the exact accepted base-obligation
evidence; it does not pretend to have restored-state evidence.

If a run began from restored state and later commits an update, its new stamp
uses `RestoredRun` and names that run's accepted activation receipt. The
receipt is durably retained in a host-owned evidence catalog before a
value/stamp commit may reference it. On a later restart, the checker resolves
the basis and validates:

- a fresh basis against the named base obligation in the named last-writer
  verification manifest; or
- a restored basis against the retained checker-created receipt, including
  matching authority, artifact/program, verification manifest, activation
  requirement, and predecessor stamp.

Thus a transition proof's assumed pre-state invariant is never reconstructed
from a static writer manifest alone. The trusted host receipt attests that the
previous run discharged its runtime restore base; the stamp crash-consistently
binds that attestation to later writes. Receipt garbage collection is
reference-aware and cannot remove a receipt while any committed authority
stamp names it. Missing, untrusted, or mismatched basis evidence fails
activation. Cryptographic protection against a malicious storage
administrator remains outside V1's stated storage-honesty model; application
source cannot construct stamps or receipts.

The last-writer digests are provenance, not equality requirements on the
current implementation. A private refactor may produce a different program,
dependency manifest, and evidence bundle while preserving the same
invariant-statement hash. Such a refactor re-verifies, accepts the old value
when its named predecessor evidence is available and valid, and writes the new
last-writer provenance on the next commit. It does not force a reset or
migration merely because private code changed.

A legacy/uncontracted value, missing/mismatched stamp, changed invariant,
changed authority/schema or persistence compatibility key, unsupported commit
protocol, unavailable/invalid last-writer evidence, or unverified current
transition proof blocks activation. The developer must use an ordinary
explicit validation/import/reset flow or a verified preservation migration;
Phase 4 adds general old-invariant-to-new-invariant transport.

This stamp is host metadata, never a Boon value or a proof by itself. It binds
the restored state to already accepted induction evidence and makes the
runtime/persistence commit protocol an explicit
`trusted_persistence_activation_v1` assurance dependency. The trusted
activation component covers value/stamp integrity checking, atomic commit
binding, last-writer evidence resolution, and compatibility checks. It neither
proves storage integrity or durability beyond that declared contract nor adds
a per-update `WHERE` check.

An erroneous initializer or candidate is a failed subject-definedness
obligation; it is not a state that vacuously satisfies the invariant.

An invariant over multiple atomically related state fields requires those
fields to be grouped into one structural `HOLD` value. V1 rejects attempts to
prove an atomic transition across independently committing `HOLD` cells.

An outer invariant may use the current held value, stable parameters, verified
constants, and pure values derived from that same atomic state. Relations to
independently changing reactive state are unsupported until a shared snapshot
semantics is defined.

The same activation rule applies to persisted list authorities and their
proved structural invariants. Last-writer evidence binds the exact prior
mutation summaries; the current artifact separately proves its current
mutation summaries. Row/list stamps bind the authority, schema, invariant
statement, and persistence compatibility identity, so a fresh-list proof
cannot justify restored rows.

## Structural Records And Lists

Refinement does not create nominal wrappers. Records and lists retain ordinary
structural equality, serialization, persistence, and runtime representation.

The explicit alias avoids record sibling-shadowing ambiguity:

```boon
record
|> WHERE result {
    result.start <= result.end
}
```

V1 list reasoning is based on versioned summaries for resolved standard
operations. A report distinguishes a machine-verified summary from a
compiler-owned trusted summary; neither category is inferred from a callable
name. The initial exact lemma schemas are:

- a literal list has its literal cardinality;
- `List/map` preserves cardinality and order when its mapped result is present
  under the operation's actual presence rules;
- `List/retain` returns a stable-order subsequence whose count does not exceed
  the input count;
- complementary `True | False` `retain` predicates partition a snapshot list;
- `List/append` and `List/remove` obey their exact cardinality laws under their
  documented presence and match rules;
- `List/every` establishes its element predicate for a literal list or a
  supported mapped list;
- `List/find` preserves the operation's actual `Found`/`NotFound`, order, and
  presence behavior.

Supported proofs include:

- cardinality bounds;
- element predicates expressed through existing list operations;
- partition relationships such as active plus completed equals total;
- properties preserved through only those enumerated map/filter/mutation
  schemas;
- stable order and currentness where the language already guarantees them.

Every summary is bound to:

- the exact resolved callable and semantic-profile identity;
- exact argument/result types and preconditions;
- presence, typed-error, ordering, and currentness behavior;
- `Number` conversion behavior for cardinality;
- a checked proof/certificate for `verified_standard_summary`, or an explicit
  `trusted_standard_summary` classification for a reference implementation;
- an implementation/semantic digest;
- an assurance classification and summary hash.

The verifier uses stable checked callable identities and compiler-owned summary
ids. It never infers a theorem by matching source strings such as
`"List/count"`. Differential and negative tests guard implementations against
their summaries, but are regression evidence rather than proof of a trusted
summary.

List cardinality has an independent versioned resource contract:

```text
ListCapacityProfileV1 {
    max_semantic_cardinality
    maximum_cardinality_encoding
    terminal_capacity_fault
}
```

The compiler records the selected profile in `SemanticProgram`, proof evidence,
and the executable artifact. All list construction, append, materialization,
and growth paths check `max_semantic_cardinality` before commit and produce the
profile's deterministic terminal resource fault when it would be exceeded.
This bound is not derived from a floating-point mantissa, a host `usize`, or an
optimizer-selected storage width. Portable bundles use one declared bound
supported by every selected target; a target unable to honor it rejects the
artifact before execution.

`List/count` converts the bounded nonnegative cardinality to exact `NUMBER`
without loss. Proofs use an unbounded mathematical integer for intermediate
cardinality reasoning plus the explicit capacity bound when connecting that
reasoning to an executable list. Platform allocation can fail earlier as an
ordinary terminal resource fault, but no successfully committed list can
exceed its artifact's declared capacity profile.

No optimizer miss may silently become an assumed list theorem or an implicit
runtime scan. Unsupported list algebra fails closed. Open-ended structural
induction over arbitrary list callbacks is deferred.

## Number Semantics

Runtime and verifier share a versioned `ExactNumberSemanticProfileV1`.
Every Boon `NUMBER` is the unique normalized rational:

```text
numerator: arbitrary-precision signed integer
denominator: arbitrary-precision positive integer
gcd(abs(numerator), denominator) = 1
zero = 0 / 1
```

Integer, decimal, and exponent source literals parse exactly; exact division
constructs rationals, and canonical fraction text round-trips through the
specified text API. Arithmetic normalizes its exact rational result; it does
not round, overflow, wrap, or create hidden floating-point states. Division by
zero and invalid operation domains are deterministic terminal faults unless an
explicitly safe operation returns an ordinary closed Tag result. Rounding is
itself an exact operation with an authored positive quantum and rule, as
specified by the language foundations plan.

`boon_data` exposes one low-level exact semantic API for normalization,
arithmetic, comparison, rational rounding, whole-number/index conversion, and
list-cardinality conversion. `boon_plan_executor`, `boon_verify`, native/Wasm
backends, and test evaluators consume it; none reimplements observable Number
behavior independently.

The proof kernel models numerators, denominators, normalization, definedness,
and comparisons exactly. A solver may use its exact integer/rational theories,
but a model or certificate is replayed through the shared semantic evaluator.
No binary floating-point approximation, host float, interval guess, or
mathematical-real rewrite may change an observable rational result.

V1 may prove:

- exact constant calculations;
- equality and order;
- bounded linear rational arithmetic;
- whole-number and bounded-index facts;
- small exact-rational state machines;
- explicitly supported exact rounding operations.

Unsupported nonlinear, transcendental, or otherwise unmodeled arithmetic
returns `unsupported`, not a guessed result. Proof and runtime profiles bound
numerator/denominator bits, aggregate numeric memory, arithmetic/GCD work,
parsed digits, and formatted digits. Exceeding such a bound is a deterministic
terminal resource failure; it never truncates, approximates, wraps, or proves a
weaker statement.

An obligation that requires an exact numeric result must also establish
definedness under the artifact's numeric resource profile, or preserve the
terminal-fault path in its modeled outcome. Solver timeout/memory exhaustion is
a separate compile-time `timeout`/`unknown` result and likewise never becomes
proof success.

`List/count` returns an exact Boon `NUMBER`. The proof engine may reason about
cardinality as an internal unbounded nonnegative integer, but its executable
connection must establish the selected `ListCapacityProfileV1` bound and the
lossless exact-Number conversion.

## `BITS[N]`, `MAP`, `SET`, And `FLUSH` Proof Models

The verifier implements the public algebra from
`BOON_LANGUAGE_FOUNDATIONS_PLAN.md`; it does not invent proof-only application
types.

For `BITS[N]`:

- `N` is a positive compile-time width and part of the type;
- equality, bitwise operations, concatenation, slicing, shifts, rotations,
  extension, explicit truncation, and modulo operations use the exact
  width-index and bit-order rules;
- signed or unsigned interpretation is explicit for operations that require
  it;
- bounds, shift, conversion, underflow, and overflow results follow the exact
  ordinary-operation or closed-Tag contracts;
- solver bit-vectors are private encodings of these semantics, never a new
  source type.

For `MAP<K, V>` and `SET<K>`:

- proof equality is extensional and uses complete canonical Boon key equality,
  not hash buckets or insertion history;
- only the key-safe closed values accepted by the language foundations plan
  enter the proof model;
- `Map/get`, `Set/contains`, upsert/add/remove, canonical enumeration, and
  per-turn conflict resolution use the same presence, ordering, and source
  sequence rules as execution;
- finite-map/set summaries bind exact key/value types, semantic profile,
  canonical encoding, operation contract, and implementation digest;
- a hash collision, worker schedule, or backend iteration order can establish
  no proof fact.

`FLUSH` is a private control effect, not a data value. Verification tracks
normal and flushing paths through the same activation tree used by semantic
lowering. A flushing path aborts the owning candidate subtree, publishes no
state or collection delta from that subtree, and dispatches no staged
downstream effect. The hidden status is erased at the language-defined lexical
boundary into the ordinary payload Tag. It is never matched, persisted,
serialized, transported across a distributed cut, or used as a theorem
subject. A `WHERE` condition itself must remain total and therefore cannot
flush.

## Runtime Validation And Error Handling

`WHERE` never performs runtime validation. Runtime-derived data may satisfy a
static obligation only when ordinary Boon branches, tags, and checked contracts
provide sufficient path facts.

Runtime input is handled with ordinary tagged values and branches:

```boon
FUNCTION accept_percentage(number) WHERE {
    number >= 0
    number <= 100
} {
    Accepted[percentage: number]
}

draft
|> Text/to_number()
|> WHILE {
    InvalidNumber[reason, position] =>
        Rejected[message: TEXT { Enter a number }]

    Parsed[number] =>
        number >= 0
        |> Bool/and(right: number <= 100)
        |> WHEN {
            True =>
                accept_percentage(number: number)

            False =>
                Rejected[message: TEXT { Use a value from 0 to 100 }]
        }
}
```

The positive branch supplies the path facts required by the function header.
`Accepted` and `Rejected` are ordinary structural data.

A failed static proof:

- is a compiler diagnostic;
- emits no new runnable artifact;
- cannot be caught by application code;
- does not turn into a panic, `Error`, `SKIP`, fallback, or hidden runtime
  assertion.

During playground hot reload, failure must keep the last successfully verified
program running and show the new source diagnostic in the dev window.

## Playground Teaching Portfolio

The portfolio intentionally uses a few focused lessons and then upgrades real
applications. It does not create one example per proof-theory concept.

### New Examples

| Order | Manifest id | Label | Primary lesson |
|---:|---|---|---|
| 49 | `where_safe_choice` | `WHERE · Safe Choice` | Ordinary and `PASSED` header requirements, branch proof, returned guarantee |
| 50 | `where_bounded_counter` | `WHERE · Bounded Counter` | Bounded state and `HOLD` induction |
| 51 | `where_runtime_input` | `WHERE · Runtime Input` | Runtime validation before a statically constrained call |
| 52 | `where_verified_rows` | `WHERE · Verified Rows` | Contracts through records and list summaries |

All four use the existing `basic` category and the shared basic-example budget
unless measurements justify a dedicated budget. They are ordinary source-driven
preview applications.

Rollout is deliberately staggered:

- Phase 1 ships `where_safe_choice`;
- Phase 2 ships `where_runtime_input`, `where_bounded_counter`, and the
  `flow_operators` upgrade;
- Phase 3 ships `where_verified_rows` and the TodoMVC capstone.

### `where_safe_choice`

The first declaration teaches pass-through and alias locality before the
function lesson:

```boon
answer:
    21 + 21
    |> WHERE value {
        value == 42
    }
```

The same app then teaches boundary contracts:

```boon
FUNCTION choose_nonnegative(value, fallback) WHERE {
    fallback >= 0
} {
    value >= 0
    |> WHEN {
        True => value
        False => fallback
    }
    |> WHERE result {
        result >= 0
    }
}
```

It also shows that `PASSED` is the implicit-parameter alternative, including
one inherited call:

```boon
FUNCTION choose_from_context(value) WHERE {
    PASSED.defaults.fallback >= 0
} {
    choose_nonnegative(
        value: value
        fallback: PASSED.defaults.fallback
    )
    |> WHERE result {
        result >= 0
    }
}

FUNCTION choose_panel(value) WHERE {
    PASSED.defaults.fallback >= 0
} {
    choose_from_context(value: value)
}

context_choice:
    choose_panel(
        value: selected
        PASS: [
            defaults: [
                fallback: 0
            ]
        ]
    )
```

`choose_from_context` receives no explicit `PASS:` because it inherits
`choose_panel`'s contextual formal. Both headers are modular caller
requirements; neither is an `OUT` materialization proof.

The UI:

- selects `-4` or `7` with ordinary buttons;
- calls the function with `fallback: 0`;
- displays `0` or `7`;
- explains the caller and producer responsibilities in static teaching text.

The scenario verifies the visible and semantic values. Source comments invite:

- changing `value == 42` to `value == 41`, which must fail locally;
- changing the caller to `fallback: -1`, which must fail at the call;
- returning `value` from the negative branch, which must fail the result proof;
- changing the contextual fallback to `-1`, which must fail at
  `choose_panel`;
- removing `choose_panel`'s contextual header, which must fail its inherited
  call to `choose_from_context` rather than infer a precondition silently.

### `where_bounded_counter`

Core model:

```boon
count:
    0
    |> HOLD before {
        LATEST {
            sources.increment |> THEN {
                before < 5 |> WHEN {
                    True => before + 1

                    False => before
                }
            }

            sources.decrement |> THEN {
                before > 0 |> WHEN {
                    True => before - 1

                    False => before
                }
            }

            sources.reset |> THEN {
                0
            }
        }
    }
    |> WHERE current {
        current >= 0
        current <= 5
    }
```

The static outer `WHERE` is proved by base/step induction. The runtime scenario
exercises:

1. initial value `0`;
2. decrement at zero remains `0`;
3. increment reaches `1`;
4. repeated increments saturate at `5`;
5. an additional increment remains `5`;
6. reset returns to `0`.

Removing the decrement guard must report the shortest transition trace:

```text
previous state: 0
trigger: decrement
candidate state: -1
failed condition: current >= 0
```

### `where_runtime_input`

The model uses the same input-event shape as current source-driven examples:

```boon
FUNCTION accept_percentage(number) WHERE {
    number >= 0
    number <= 100
} {
    Accepted[percentage: number]
}

store: [
    sources: [
        draft_input: [
            events: [
                change: SOURCE
            ]
        ]
    ]

    draft:
        TEXT {}
        |> HOLD previous {
            sources.draft_input.events.change.text
        }

    result:
        draft
        |> Text/to_number()
        |> WHILE {
            InvalidNumber[reason, position] =>
                Rejected[message: TEXT { Enter a number }]

            Parsed[number] =>
                number >= 0
                |> Bool/and(right: number <= 100)
                |> WHEN {
                    True =>
                        accept_percentage(number: number)

                    False =>
                        Rejected[
                            message: TEXT { Use a value from 0 to 100 }
                        ]
                }
        }
]
```

The app contains:

- a normal `Element/text_input` whose `element.events` connect to
  `store.sources.draft_input.events`;
- ordinary `Text/to_number()` handling;
- visible `Accepted` and `Rejected` branches;
- a call to a header-constrained function only inside the validated branch.

Source comments invite moving the call before validation. That edit must fail at
compile time while the previous valid preview remains live.

Its scenario uses the existing `type_text` action and requires
`store.sources.draft_input.events.change` as the expected source event. The
source has a real `document:` root. It must not introduce a `Validate`
namespace merely for the lesson.

### `where_verified_rows`

Core model:

```boon
FUNCTION score(row_name, row_points) WHERE {
    row_points >= 0
    row_points <= 100
} {
    [
        name: row_name
        points: row_points
    ]
    |> WHERE row {
        row.points >= 0
        row.points <= 100
    }
}

scores:
    LIST {
        score(row_name: TEXT { Ada }, row_points: 90)
        score(row_name: TEXT { Linus }, row_points: 80)
        score(row_name: TEXT { Grace }, row_points: 100)
    }
    |> WHERE rows {
        List/count(list: rows) == 3

        rows
        |> List/every(item, if:
            item.points >= 0
            |> Bool/and(right: item.points <= 100)
        )
    }
```

The UI renders the three rows and a count. Negative fixtures cover:

- a caller passing `120`;
- a returned record adding an invalid offset;
- an incorrect expected count;
- a broken element-wide condition.

### Existing `flow_operators` Upgrade

The current result can gain:

```boon
result:
    operation |> WHILE {
        Addition => input_a + input_b
        Subtraction => input_a - input_b
    }
    |> WHERE value {
        value >= 13
        value <= 29
    }
```

This is the small real-source branch-coverage proof. It does not require a
second dedicated branch example.

### Existing TodoMVC Capstone

The real TodoMVC model eventually gains:

```boon
store:
    [
        -- existing model
    ]
    |> WHERE state {
        state.active_count + state.completed_count
            == List/count(list: state.todos)
    }
```

This is a pointwise list-algebra theorem over each admitted Todo snapshot. It
must cover the existing seeded rows, append, toggle, edit, remove,
clear-completed, filter, and row-local state behavior. It is accepted only when
the list verifier proves the relation for every supported live mutation, not
only the initial list or recorded scenario. It is not evidence that V1 can
perform joint induction across TodoMVC's several independent `HOLD` cells.

Later compatible conditions may include:

- visible count does not exceed total count;
- active and completed predicates form a partition;
- stable list order is preserved by filtering;
- row-local edits preserve the row schema.

### Existing Migration Capstones

Migration is a post-V1 extension. Exact identity transfer is a compiler-owned
property of `DRAIN`; app source must not reread the retiring `DRAINING`
authority through its ordinary name. Once the old state's proved invariant is
transported across the migration edge, Counter can express a property of the
migrated output:

```boon
DRAIN { count }
|> WHERE migrated {
    migrated >= 0
}
|> HOLD before {
    sources.increment |> THEN {
        before + 1
    }
}
```

The old/new identity theorem itself belongs to the compiler's migration
semantic graph. If a future application contract must relate old and new
values, it requires an explicit proof-only migration witness; ordinary source
name resolution must not expose the draining authority illegally.

Todo migrations later prove preservation of stable data while schemas add,
rename, split, or retire fields. The proof engine uses actual `DRAIN`,
`DRAINING`, authority, and migration-sequence semantics; it does not treat
migrations as ordinary stateless function calls.

The proof input includes the real `MigrationPredecessorBinding`: predecessor
application/persistence plans, application and schema identities, migration
recipe/catalog hashes, source leaves, destinations, transfer/transform
semantics, and state/list fingerprints. Fresh initializers are never a proxy
for persisted predecessor authority. Phase 4 must also state explicitly
whether durable effect-outbox intent/key/result/invocation state is outside a
given preservation theorem or include its schemas and leaves in the same
dependency and coverage hashes.

`DRAIN { PASSED.path }` is a distinct migration-context dependency, not an
ordinary `PASSED` read. The current checked lowering can lose that distinction.
Before Phase 4, typechecking must either reject this form precisely or preserve
both its `ContextFormalId` and `DRAIN` marker through semantic migration
elaboration. It may never silently become a normal contextual read.

## Example Artifacts And Evidence

Positive examples are the only proof examples registered in
`examples/manifest.toml`.

Each new example owns:

```text
examples/<id>.bn
examples/<id>.scn
examples/basic_examples.budget.toml
```

or a directory-shaped source unit layout if the final source genuinely needs
multiple modules.

Add a generic manifest expectation:

```toml
formal_proof = true
formal_contract_digest = "sha256:<formal-contract-set-v1>"
formal_clause_count = 3
```

This does not affect application semantics. `formal_proof` opts the entry into
the gate; the normalized contract digest and clause count make deleting or
weakening the expected theorem set an explicit reviewed change. The digest is
formatting-independent and is derived from the canonically ordered tagged
multiset of:

- every local authored `WHERE` contract-statement hash, including non-public
  checkpoints/invariants;
- every complete local public `theorem_hash`, including its public callable
  type/default/context/effect/presence shape as well as formulas;
- every imported public `theorem_hash`.

```text
formal_contract_digest =
    H(
        "boon.formal-contract-set.v1",
        canonical_ordered_tagged_multiset(
            local_contract_statement_hashes,
            local_public_theorem_hashes,
            imported_public_theorem_hashes
        )
    )
```

Thus a public-shape change with unchanged text conditions and an internal
checkpoint change are both visible. `cargo xtask refresh-formal-contracts`
prints the normalized statement/theorem diff before updating these fields;
`cargo xtask refresh-formal-contracts --check` performs no writes and fails on
drift. This minimal semantic diff ships in Phase 1; Phase 5 adds richer editor
presentation. `formal_clause_count` counts authored normalized conditions, not
generated call, branch, or induction obligations; completeness of those
generated obligations belongs to `RequiredObligationManifest`, while accepted
evidence belongs to the successful `VerificationManifest`.

Because the current manifest DTO rejects unknown fields, `ExampleEntry`, its
serializer, validation, and repository-manifest tests must add these three
fields together.

Do not add a handwritten `.proof.toml` in V1. The compiler-generated proof
report is authoritative and source-digest-bound.

Invalid sources never enter the runnable manifest. Add a generic compile-fail
fixture area and runner:

```text
examples/where_negative_templates/
examples/where_negative_templates/cases.toml
```

Each `[[case]]` records:

```toml
id = "header_caller_violation"
source = "examples/where_negative_templates/header_caller_violation.bn"
expected_diagnostic_id = "formal.caller_requirement_refuted"
expected_category = "refuted"
expected_owner = "call:accept_percentage"
expected_primary_span_text = "accept_percentage(number: number)"
proof_report_required = true
artifact_forbidden = true
```

The runner checks that the reported primary span resolves to the expected
source slice, not merely that compilation failed. This is a new enforceable
fixture manifest; a directory of `.bn` templates alone is not evidence.

Ordinary `.scn` files verify runtime behavior only. They are never proof
evidence. Every previewable source retains a real `document:` root. The current
catalog is flat, so formal examples are ordinary entries rather than a hidden
proof-only category.

The standalone `cargo xtask verify-formal-examples` gate:

1. loads manifest entries with `formal_proof = true`;
2. compiles and formally verifies all source units;
3. requires the complete instantiated verification manifest to be valid;
4. matches the normalized contract digest and declared clause count;
5. confirms `WHERE` created no executable runtime operation;
6. verifies that the manifest source, `Scenario.source`, and compiled
   `SourceBundleDigestV1` identify the same source bundle;
7. runs the ordinary deterministic scenario as behavioral evidence;
8. verifies the negative fixture suite separately;
9. writes each bounded `boon.proof-report.v1` to
   `target/reports/formal-v1/proofs/<example-id>.json`;
10. writes versioned, canonically ordered schema `boon.formal-examples.v1` to
   `target/reports/formal-v1/examples.json`;
11. fails on a non-success entry, a per-source report larger than 1 MiB, or an
    aggregate report larger than 1 MiB.

The report includes compiler, source, contract, scenario, semantic-profile, and
summary digests. The xtask command is dispatched without loading or requiring
the native handoff manifest. Its schema and CI route are independent of native
GPU reports.

Do not add formal-example reports to the native GPU handoff manifest merely
because the example has a preview. Native rendering evidence and source-proof
evidence remain distinct.

### Canonical Source Identity

Phase 0 freezes `SourceBundleDigestV1` and replaces the repository's competing
single-unit, runtime-unit, and package-source digests wherever formal source
identity is required.

For each compilation:

1. derive a UTF-8 logical project-relative entrypoint and unit paths;
2. normalize separators to `/`, and reject absolute paths, `.`/`..` traversal,
   or duplicate normalized paths;
3. sort units by raw normalized-path bytes;
4. hash with SHA-256:

```text
"boon.source-bundle.v1\0"
u64be(entrypoint_path_byte_length)
entrypoint_path_bytes
u64be(unit_count)
for each unit:
    u64be(path_byte_length)
    path_bytes
    u64be(source_byte_length)
    exact_source_bytes
```

The digest excludes absolute host paths, mtimes, and source-list input order.
Compiler reports, runtime artifacts, package metadata, scenarios, and the
formal gate reuse this one implementation. The gate requires the normalized
manifest entrypoint and `Scenario.source` to match and the compiled
`SourceBundleDigestV1` to match the report.

## Compiler Architecture

The mandatory semantic gate is:

```text
CheckedProgram
    -> SemanticProgram
    -> ContractVerifiedProgram
    -> ErasedProgram
```

Every artifact-producing production frontend and every backend follows that
exact ownership order, including programs that author no local `WHERE`.
`ParsedProgram` precedes it; `MachinePlan`, `PhysicalPlan`, native/Wasm code,
and hardware IR follow it. No compiler flag, cache format, precompiled package,
distributed role, test fixture, or target-specific lowering may skip the gate.

That rule applies to every request capable of publishing an executable
artifact. A diagnostics-only compiler request may stop after its complete
checked diagnostics are available, but it produces no `SemanticProgram`,
`ContractVerifiedProgram`, `ErasedProgram`, `MachinePlan`, verified preview,
package, or runnable cache entry. Switching request profiles is not a bypass:
any later executable request continues through the mandatory semantic and
verification spine from exact source-bound inputs.

The complete required pipeline is:

```text
Boon source
    |
    v
ParsedProgram
    |
    v
CheckedProgram
    |
    v
boon_semantic::elaborate
    |
    v
SemanticProgram
    |
    v
boon_verify
    |
    +--> ProofReport
    |
    v
ContractVerifiedProgram
    |
    +--> interface projection
    |        |
    |        v
    |    VerifiedPublicContractBundleV1
    |
    +--> ProofReportReference
    |
    `--> boon_ir::erase_and_lower
             |
             v
         ErasedProgram {
             semantic_program_digest
             verification_manifest_digest
         }
             |
             v
         MachinePlan / document / persistence / runtime backend lowering
```

Today, `boon_ir` performs semantic elaboration that verification needs:
producer roots and `OutNet`, contextual materialization, derived-list storage
and targets, source/state bindings, exact list mutations, state-update arms,
dependency and possible-cause graphs, and migration graphs. That logic must not
be copied into a verifier.

Add `crates/boon_semantic` and move the semantics-essential portion of current
IR lowering into it. Its `SemanticProgram` retains:

- resolved producer and contextual-wiring graphs;
- stable `PASSED` contextual formals, every explicit/inherited context binding,
  and every concrete call instantiation;
- each concrete `OUT`, output-evaluation scope, compiler-supplied call context,
  and repeated materialization;
- the exhaustive `CallableDependencyManifest` and per-call proof-context keys;
- flow kind, presence, typed-error, and event candidate information;
- derived-list storage, targets, callbacks, mutations, and currentness;
- exact source, state, `HOLD`, `LATEST`, resource/effect, dependency, capture,
  and possible-cause structure;
- migration authority, predecessor-catalog, and sequence graphs;
- proof checkpoints and public contracts;
- stable mappings back to checked declarations, expressions, and spans.

Both VC generation and executable lowering consume this exact representation.
`boon_verify` depends on `boon_semantic`; `boon_ir` consumes only a
`ContractVerifiedProgram`. This avoids a dependency cycle and prevents proving
one semantic model while executing another. `docs/architecture/LANGUAGE_SEMANTICS.md`
must ultimately state that the verifier consumes `SemanticProgram`, while
backends consume its proof-erased `ErasedProgram` lowering.

The compiler may construct private `SemanticCore` shards while discovering
contextual materializations and distributed dependencies. A core is an
unsealed builder representation: it is not serializable as a runnable artifact,
cannot be accepted by `boon_verify` or `boon_ir`, and cannot escape through an
artifact-producing compiler result. `SemanticProgram::seal` freezes the exact
resolved graph, callable-dependency manifest, proof-context identities,
semantic-profile inputs, source mapping, and canonical verifier inputs. The
verifier derives its required-obligation and evidence manifests from that
sealed program; compiler-side sharding may accelerate construction but may not
replace, weaken, or privately reinterpret those canonical manifests.

The compiler packages these projections without putting proof nodes into
executable IR:

```text
VerifiedCompilationUnit {
    erased_program
    verified_public_contract_bundles
    authority_activation_requirements
    proof_report_reference
}
```

Compiler backends accept only the opaque `ErasedProgram` produced by
`boon_ir::erase_and_lower(ContractVerifiedProgram)`. Its constructors and
verification-provenance fields are private, and a raw deserialized value is not
accepted as a source-compilation result. Executable expression/state graphs
contain neither proof wrappers nor contract nodes. Deployment/runtime sidecars
retain only the static authority activation requirements needed to gate
persisted restore. Module, package, and later distributed interface compilation
consume the verified bundles. Tooling follows the report reference. This side
path prevents both silent contract loss at erasure and accidental proof objects
as Boon runtime values.

Every artifact-producing compiler entrypoint imports transitive public
contracts, elaborates the semantic graph, constructs its complete
required-obligation manifest, verifies it, and only then lowers. A trivial
successful manifest is allowed only when that complete set is empty. The
absence of a local `WHERE` token is not enough: an imported contracted call can
still create an obligation.

### Semantic And Executable Type Ownership

Phase 0 freezes this dependency and ownership split:

| Crate | Owns | Must not own or depend on |
|---|---|---|
| new `boon_contract` | Data-only theorem/bundle and invariant-stamp DTOs, canonical encodings, schema versions, hashes | Checked ids, solver state, executable ids |
| `boon_typecheck` | `CheckedProgram`, checked ids, `CheckedContract` formulas/spans with no validity claim | Verified evidence or `boon_verify` types |
| new `boon_semantic` | Complete pre-backend graph and `Semantic*Id`s | `boon_verify`, `boon_ir`, or executable ids |
| new `boon_verify` | Obligations, evidence, manifests, verified bundles, private `ContractVerifiedProgram` construction | Executable lowering |
| `boon_ir` | `Executable*Id`s, proof erasure, semantic-to-executable mapping, `ErasedProgram` | Context discovery or independent semantic elaboration |
| `boon_compiler` | Pipeline orchestration, distributed fixed point, structured compile outcome | A bypass around semantic verification |

`CheckedContract` lets the typechecker reason about the authored formula and
signature without claiming it has been proved. `boon_verify` is the only layer
that may turn it into verified evidence. Serialized theorem and bundle DTOs
live below both crates to avoid a dependency cycle, but bundle validation and
trusted construction remain verifier-owned.

Choose distinct semantic identities, including at least:

```text
SemanticExprId
SemanticValueId
SemanticCallableId
SemanticMaterializationId
SemanticStateId
SemanticListId
SemanticMigrationId
```

Move `contextual_expansion`, `OutNet`, semantic migration, state/list
dependency construction, and their pre-backend graph types into
`boon_semantic`, replacing executable ids with these semantic ids.
`ProducerFunctionLoweringRequest` becomes a semantic
`ProducerMaterializationRequest`. `boon_ir` allocates executable ids only in
one explicit `SemanticToExecutableMap`; it does not rediscover contexts,
mutations, candidates, or migration edges.

### Distributed Semantic Fixed Point

Distributed compilation cannot verify a role while concrete producer and
contextual materializations are still changing. Its required order is:

1. parse and check every role;
2. solve checked type and authored-theorem interfaces without assuming the
   theorems valid;
3. elaborate private role `SemanticCore` shards;
4. link those cores and discover distributed calls;
5. derive `ProducerMaterializationRequest`s and update only affected cores
   until the bundle reaches a deterministic fixed point;
6. seal each role exactly once as a `SemanticProgram`, freeze the
   `BundleSemanticProgram` digest, and construct complete canonical bundle/role
   obligation manifests;
7. verify the sealed programs under one compatible policy;
8. erase verified roles and produce executable plans and interface bundles.

No pre-verification `boon_ir` lowering may be used for call discovery, and no
semantic relinking may occur after `SemanticProgram::seal` or
`ContractVerifiedProgram` construction. A changed core invalidates its seal;
the bundle must return to the fixed-point step rather than patch a sealed or
verified artifact.
V1 rejects a contracted role/external crossing; the fixed point still applies
to all existing uncontracted distributed compilation so it cannot become a
verification bypass. Phase 7 adds verified cross-role contract transport.

### Compiler Outcome And Failed-Proof Reports

Every compiler entrypoint returns a structured boundary rather than flattening
verification into `Box<dyn Error>` or a display string:

```text
CompileOutcome<Artifact> {
    diagnostics
    proof_report: Option<ProofReportV1>
    result: Result<Artifact, CompileFailure>
}
```

`ProofReportV1` uses schema id `boon.proof-report.v1`, has canonical field/list
ordering, is source-bound, and is limited to 1 MiB per source bundle. Its
`semantic_core` projection and `semantic_core_digest` are deterministic for the
same source, semantic program, policy, and verifier version. Timings, cache
measurements, and diagnostic presentation are outside that projection and may
vary between runs.

V1 sets `MAX_REQUIRED_OBLIGATIONS_V1 = 512`,
`MAX_INLINE_OBLIGATION_RESULT_CORE_BYTES_V1 = 1024`, and
`MAX_PROOF_REPORT_DIAGNOSTIC_BYTES_V1 = 65536`. Before starting any proof, the
verifier constructs the complete required manifest and a report skeleton, then
reserves the worst-case inline bytes for every required result plus bounded
diagnostics. It rejects with `formal.resource_limit.obligation_count` or
`formal.resource_limit.result_core` or
`formal.resource_limit.report_capacity` if a count, individual reserved core,
or exact canonical worst-case total would exceed its limit or 1 MiB. Parse,
type, semantic, or this proof preflight failure may therefore occur before a
proof report exists.

Once verification starts, the reservation guarantees that the report contains
one complete deterministic result core for every completed obligation.
Counterexample presentation and solver traces are diagnostics: they have
separate per-result/total caps and, when larger, retain a deterministic relevant
projection, full internal-model digest, and explicit truncation marker rather
than displacing result/coverage entries. `refuted`, `unknown`, `timeout`,
`unsupported`, manifest incompleteness, or disallowed assurance then returns
the complete bounded result-core report with `Err(CompileFailure)` and no
artifact.

```text
semantic_core_digest =
    H(
        "boon.proof-report-semantic-core.v1",
        canonical(ProofReportV1.semantic_core)
    )
```

The digest field itself is outside the projection and is recomputed on read.

`boon_cli verify ... --report <path>` writes this report atomically even when a
proof fails, then exits nonzero. The native compiler worker transports the
typed outcome to both preview and language/dev UI; it does not reduce proof
data to `String`, and the language worker does not reconstruct verifier results
by rerunning only parser/typechecker analysis. Hot reload retains the prior
verified artifact when the new outcome has no artifact.

The aggregate `boon.formal-examples.v1` report stores bounded summaries and
digest/path references to per-source proof reports rather than embedding them
without limit.

### Parser

`boon_parser` must add:

```text
AstFunctionContract
AstCondition
AstExprKind::Where {
    input
    alias
    conditions
}
```

The exact Rust names may follow repository conventions, but the AST must retain
stable ids and full source spans.

The selected two-brace header requires a real compound parser node rather than
treating the first `{...}` as the function body. Before header contracts are
accepted, the current line/tree parser must:

- recognize `FUNCTION name(parameters) WHERE {` as the start of a logical
  compound function header;
- parse the matching condition-block close with exact nesting and source
  range;
- require a second body opener after that close, either on the same `} {` line
  or as the next non-comment token;
- attach the condition block and body block as separate children of one
  function AST node;
- recover independently from a missing contract close, missing body opener,
  duplicate header `WHERE`, trailing header token, or empty condition block;
- preserve precise spans for the header, each condition, separator, both
  braces, and body;
- parse the complete header and reject every unconsumed token;
- support canonical multiline and compact condition blocks;
- add the special piped `WHERE` form before generic pipeline-call validation;
- keep ordinary pipeline functions parenthesized;
- update linked-input construction, expression ownership, traversal,
  namespacing, canonical value selection, and serialization;
- keep bare statement `WHERE` invalid.

Required parser tests include:

- compact and multiline headers, including same-line and next-line body
  openers;
- compact and multiline condition blocks;
- multiline infix, leading-pipeline, named-call, and nested-branch clauses;
- long and nested pipeline inputs;
- `WHERE` inside `THEN`, `WHEN`, `WHILE`, `LATEST`, `HOLD`, and `BLOCK`;
- module-qualified source units;
- precise spans;
- formatter round trips;
- trailing header garbage;
- missing pipeline alias;
- missing contract close and missing second body opener;
- compatibility uses of `WHERE` as an ordinary name outside the two special
  contexts;
- every rejected spelling listed in this plan.

The formatter gains an idempotent canonical rendering for the compound header
and both condition-block styles. It does not rely on today's one-child
line/tree representation accidentally accepting `} {`.

### Typechecker

`boon_typecheck` remains the authority for types, lexical identities, checked
callable identities, and flow/presence information.

It adds:

- a function-contract scope containing parent-evaluated ordinary input
  parameters and stable inferred `PASSED` contextual formals, while excluding
  `PASS` syntax, `OUT`, output-evaluated parameters, compiler contexts, dynamic
  captures, and body declarations;
- a principal structural context scheme per callable, inferred from direct
  `PASSED` projections and nested inherited requirements rather than from a
  compilation-global union of call sites;
- contextual observation modes that distinguish finite projections,
  transparent `PASS:` forwarding, and closed whole/subrecord observation;
- stable `ContextFormalId`s and resolved
  `Explicit(expression) | Inherited(formal) | None` bindings on every call;
- checked evaluation-scope classification for every ordinary parameter;
- a pipeline-condition scope containing the exact input flow type and the
  proof-only alias;
- checked condition ids and spans;
- purity and totality classification;
- function input conditions;
- returned-value conditions;
- returned-theorem presence quantification and separate reachability evidence;
- branch path facts;
- the exhaustive callable capture/resource/call/context dependency manifest and
  proof-assumption provenance;
- obligation ownership and diagnostic category;
- standard-library proof-summary ids;
- verifier-neutral checked theorem statement ids/hashes in module and external
  function signatures. Verified-bundle hashes/references are added only by
  `boon_verify`/compiler orchestration to module/package interface metadata,
  never owned by `boon_typecheck`.

Header conditions must not share the body scope. Boon's order-independent body
bindings would otherwise make body locals incorrectly visible in the header.

The current `CheckedExpressionKind::Passed { projection }` has no declaration
identity, and current context typing widens information observed across call
sites. That representation is insufficient for proof. Checked `PASSED` reads
must resolve to a stable contextual formal plus projection, with
alpha-normalized per-call row/type/flow substitutions. Compatible inherited
requirements merge at the callable boundary; incompatible field types, flow
modes, snapshot groups, roles, or capabilities reject. Unrelated call sites
must never widen one another's proof domain.

The typechecker rejects:

- conditions whose source type is not the closed `True | False` Tag set;
- absent `True | False` condition flows;
- effects or state writes inside conditions;
- unmodeled host/runtime identity;
- a proof alias used outside its condition block;
- a header reference to `PASS`, `OUT`, an output-evaluated formal, a
  compiler-supplied context, or a dynamic/private capture;
- an exported returned formula with free private, `PASS`, `OUT`,
  output-evaluated, or compiler-context declarations;
- an exported theorem whose proof uses an unrecorded ambient assumption even
  when that assumption is absent from the returned formula;
- unstratified circular assumptions;
- contracted calls whose contracts would be lost at a module or role boundary.

Alias-escape and collision behavior are typechecker/name-resolution tests, not
parser tests.

### `ContractVerifiedProgram`

Add a new `boon_verify` workspace crate. Its successful output wraps the exact
semantic program plus a completeness-checked private manifest:

```text
ContractVerifiedProgram {
    semantic_program
    verified_public_contract_bundles
    refinements
    verification_manifest
    coverage = ExplicitContractsV1
}

RequiredObligationManifest {
    checked_program_digest
    semantic_program_digest
    declared_contract_ids
    declared_condition_ids
    contract_coverage_by_id
    condition_coverage_by_id
    required_obligation_ids
    callable_dependency_manifest_hashes
    proof_context_key_hashes
    dependency_classifier_schema_hash
    authority_activation_requirement_hashes
    imported_verified_bundle_hashes
    semantic_profile_hashes
    summary_hashes
    verifier_policy
    requirement_digest
}

VerificationManifest {
    requirements
    accepted_obligation_evidence_core_by_id
    manifest_digest
}
```

`semantic_profile_hashes` includes the exact-Number profile, list-capacity
profile, every used `BITS[N]` operation profile, canonical MAP/SET key and
operation profiles, and the `FLUSH` activation/commit profile. Changing any
used profile invalidates the affected evidence and every dependent exported
bundle.

These embedded digests are also non-recursive and domain-separated:

```text
requirement_digest =
    H(
        "boon.required-obligation-manifest.v1",
        canonical(RequiredObligationManifest without requirement_digest)
    )

manifest_digest =
    H(
        "boon.verification-manifest.v1",
        canonical(
            finalized RequiredObligationManifest including requirement_digest,
            AcceptedObligationEvidenceCoreV1 values ordered by obligation id
        )
    )
```

The verification-manifest payload excludes its own `manifest_digest`, proof
timings and diagnostic presentation, exported bundles, concrete restored
stamps, and runtime activation receipts. Readers recompute the requirement
digest before the manifest digest and reject any mismatch.

Conceptual proof structures include:

```text
AcceptedObligationEvidenceCoreV1 {
    obligation_id
    normalized_vc_digest
    logical_status = valid
    replayable_proof_or_certificate_reference
    proof_or_checked_certificate_digest
    callable_dependency_manifest_hashes
    proof_context_key_hashes
    imported_assumption_bundle_hashes
    summary_evidence_hashes
    semantic_profile_hashes
    assurance_dependencies
    verifier_policy
    kernel_and_verifier_versions
}

FunctionContractStatement {
    callable_id
    ordinary_formals
    passed_context_formals
    caller_input_formula
    returned_theorem: Option<ReturnedTheorem>
    public_contract_shape_hash
    authored_contract_statement_hashes
}

FunctionVerificationRecord {
    authored_contract_statement_hashes
    callable_dependency_manifest_hash
    assumption_provenance
    authored_condition_ids
    local_returned_checkpoint_ids
    source_spans
}

ReturnedTheorem {
    payload_formula
    applies_when_present
}

RefinementCheckpoint {
    subject_value_id
    formula
    free_value_ids
    source_span
}

ProofObligation {
    owner
    assumptions
    goal
    dependencies
    status
    evidence
}

HoldInduction {
    hold_id
    invariant
    base_obligation
    step_obligations
    candidate_coverage
}
```

`AcceptedObligationEvidenceCoreV1` is raw, deterministic provider-local
evidence. It never contains the enclosing provider
`VerificationManifest` digest or any exported evidence-core/bundle hash.
`VerifiedContractEvidenceCoreV1` is a different post-manifest interface DTO
derived only after this raw evidence and the provider manifest have been
finalized.

Reachability, presence-production evidence, and local returned checkpoint
coverage live in `FunctionVerificationRecord`, obligation coverage, and proof
reports. They are not fields of the authored contract statement and therefore
cannot change its API identity.

Every function-header `WHERE` block and piped `WHERE` checkpoint has a stable
`ContractId`; each contained clause has its stable child `ConditionId`.
Definition coverage and call/materialization instantiation coverage are
orthogonal. One modular function theorem may be called both from an ordinary
scope and from several repeated `OUT` scopes.

Every authored contract also has a formatting- and span-independent statement
hash:

```text
contract_statement_hash =
    H(
        "boon.authored-contract-statement.v1",
        canonical(
            header_or_pipeline_kind,
            stable_contract_owner_key,
            binder_or_subject_type_flow_presence_shape,
            normalized_condition_formulas,
            referenced_contract_visible_semantic_definitions
        )
    )
```

It excludes private implementation dependencies, materialization ids,
`ContractId`/`ConditionId`, source spans, and evidence. Header/result public
API shape is additionally covered by the complete public theorem hash; a local
piped checkpoint still receives this statement hash even though it has no
public theorem.

```text
ContractCoverage {
    contract_id
    condition_ids
    definition: Symbolic | MaterializationDependent
    definition_obligation_ids
    instantiations: [
        ObligationInstantiation {
            proof_context_key
            materialization_ids
            condition_ids
            obligation_ids
        }
    ]
}

ConditionCoverage {
    condition_id
    symbolic_obligation_ids
    instantiated_obligation_ids
}
```

`Symbolic` definition obligations range over parent-evaluated ordinary formals
and `PASSED` contextual formals. An instantiation may have no
`materialization_ids` for an ordinary call, or exact ids when its actuals depend
on a concrete `OUT`, output-evaluation scope, compiler-supplied context,
repeated owner, or another V1 materialization-local resource. Thus the same
contract can retain one modular theorem and have both ordinary and
per-materialization caller obligations.

An uncalled materialization-dependent callback cannot make an impossible
`WHERE` disappear by creating zero obligations. Its required coverage records
the source contract and condition ids plus an unmaterialized-context
diagnostic, and compilation fails before a successful manifest can be
constructed. A function that uses only ordinary formals plus projected
open-row-transparent or statically closed `PASSED` formals remains symbolically
checkable even when uncalled. A later universal app-authored
`OUT`/callback contract may remove the remaining restriction only by defining
its scoped quantification explicitly.

Manifest completeness requires:

- exact equality between authored and recorded `ContractId`s;
- exact equality between authored child clauses and recorded `ConditionId`s;
- every `ConditionId` to be covered by its definition obligation and every
  semantically required call/materialization instantiation;
- every recorded obligation to name the conditions it discharges;
- exact equality between the union of all coverage obligation ids and
  `required_obligation_ids`;
- exact equality between persisted contracted state/list authorities in the
  semantic dependency closure and
  `authority_activation_requirement_hashes`.

A single obligation may discharge several clauses, but no clause may disappear
inside a contract-level aggregate.

The implementation uses stable checked declaration, expression, callable, and
value identities. Parser text and function-name strings are never semantic
keys.

Only `boon_verify` can construct `ContractVerifiedProgram`. Construction
requires exact set equality between `required_obligation_ids` and accepted
evidence ids, matching digests, an allowed logical/assurance status for each
obligation, complete contract and condition coverage across every required
instantiation, complete static activation requirements for persisted
contracted authorities, and the selected verification policy. It never
requires access to a runtime store or a concrete restored stamp. The wrapper
reports explicit-contract coverage; its name must not imply total-program
safety.

After proof preflight succeeds, failed verification retains the complete
`RequiredObligationManifest` plus every completed deterministic obligation
result core in `ProofReportV1`; it simply cannot construct the successful
`VerificationManifest` or program wrapper. A preflight resource-limit failure
instead emits its precise compile diagnostic before any obligation is started,
so it never promises an impossible partial report.

The provider `VerificationManifest` is finalized from requirements, including
their static authority-activation requirement hashes, and raw accepted
obligation evidence before exported public bundles are derived. It contains no
concrete restored stamp or activation receipt. Its canonical payload contains
no hash of a bundle that will in turn reference the manifest. Raw evidence may
reference imported provider bundles but never its own enclosing exported
bundle. This ordering keeps manifest, evidence-core, and bundle hashes acyclic.

An entry in `imported_verified_bundle_hashes` is added only after the provider
evidence path in “Modules, Roles, Effects, And Trust” has been checked. Merely
matching a serialized theorem hash never introduces assumptions.

`boon_ir` must accept `ContractVerifiedProgram`, never an arbitrary
`CheckedProgram` or unchecked `SemanticProgram`. No production raw-lowering
helper exists. A narrowly scoped test bypass, if unavoidable, is compile-time
gated and cannot enter a normal compiler API.

Phase 1 removes or privatizes the current source-to-IR entrypoints `lower`,
`lower_runtime`, `lower_with_external_types`,
`lower_runtime_with_external_types`,
`lower_runtime_with_external_types_and_producer_functions`, and
`lower_checked`. The sole production entrypoint returns an opaque
`ErasedProgram` whose private construction requires `ContractVerifiedProgram`.
Backend functions either accept that value or are `pub(crate)` helpers
reachable only after its provenance checks. A publicly deserializable raw
`ErasedProgram` is never accepted as a production source-compilation result.

An architecture test enumerates all production callers and fails if a new
unchecked lowering or raw-backend path appears.

### Proof Engine

The verifier is layered:

1. type, presence, purity, and definedness checks;
2. constant folding and structural equality normalization;
3. private logical-Boolean, Tag, exact-rational, `BITS[N]`, finite
   `MAP`/`SET`, and cardinality analysis;
4. modular function-contract substitution;
5. path-sensitive symbolic execution for `WHEN`, `WHILE`, and `THEN`;
6. exact `LATEST` candidate coverage;
7. single-`HOLD` induction;
8. the enumerated verified or trusted standard-list lemma schemas;
9. an optional later external solver for formulas outside the native decidable
   fragment;
10. independent replay of refuting models through the shared semantic
    evaluator.

Phase 1 uses a deterministic native prover for the supported private logical
Boolean, constant, structural, bounded-range, branch, exact-rational, and
fixed-width bit fragment. The current workspace has no selected SMT dependency,
so an external SMT solver is not a V1 prerequisite and must not be chosen
accidentally during implementation.

Phase 0 produces a solver ADR before any external solver is added. It freezes:

- backend, version, license, supported host/target matrix, and Rust interface;
- static/dynamic linking and packaging policy;
- offline installation and reproducibility;
- supported exact integer/rational, private logical-Boolean, bit-vector, and
  finite collection theories and their canonical encodings;
- deterministic seeds, time and memory limits;
- model extraction and validation;
- proof-certificate support or explicit trusted-unsat status.

An external backend must never convert timeout or unknown into success.
Counterexample replay validates a refuting model; it cannot validate an SMT
`unsat` answer.

Replay is owned by `boon_semantic` as a concrete evaluator for the supported
proof fragment. It consumes the already resolved `SemanticProgram`; it is not a
second parser, resolver, or typechecker. Supported finite cases are
differentially tested against an unoptimized `MachinePlan` execution.

### Proof Status

Logical result and assurance are separate report axes:

```text
logical_status:
    valid
    refuted
    unknown
    timeout
    unsupported

assurance_dependencies:
    native_decidable_kernel
    checked_certificate
    cryptographic_hash_sha256_v1
    trusted_smt
    verified_standard_summary
    trusted_standard_summary
    exhaustive_finite_domain
    trusted_persistence_activation_v1
    platform_conditional
```

An obligation can depend on more than one assurance category. V1 accepts only
`valid` results whose assurance set is permitted by the verifier policy and
contains no `platform_conditional`. Reports and UI display both axes; they do
not flatten trusted SMT or trusted summaries into an unqualified “proved”
badge.

The default V1 policy permits `native_decidable_kernel`,
`checked_certificate`, `cryptographic_hash_sha256_v1`,
`verified_standard_summary`,
`trusted_standard_summary`, `exhaustive_finite_domain`, and the narrowly scoped
`trusted_persistence_activation_v1`. The persistence category is permitted
only when compilation has emitted the exact static authority activation
requirement and the deployment host later binds the concrete restored value,
stamp, last-writer evidence, current induction manifest, compatibility key, and
checker version in a valid activation receipt. The concrete stamp and receipt
are never falsely recorded as compile-time obligation evidence. This is not a
general platform assumption; it may justify only the restored-authority
induction base, not an unrelated formula step. Trusted summary and persistence
dependencies remain visible. `trusted_smt` is
unavailable until the solver ADR and a later policy explicitly enable it;
`platform_conditional` is rejected in V1.

Reports distinguish a compile-time invariant proof for a fresh initializer and
all current transitions from end-to-end restored-state assurance that also
depends on the trusted persistence activation mechanism. A compile-time proof
report says that runtime activation is required; a separate host activation
report/receipt says whether one exact restored value was accepted. Neither is
presented as proof of filesystem durability, crash recovery outside the
declared commit protocol, or storage honesty.

`cryptographic_hash_sha256_v1` makes explicit that serialized identity,
incremental cache reuse, bundle import, and persistence receipts rely on the
canonical encoders being injective for their typed payloads and on SHA-256
collision/second-preimage resistance. An entirely in-memory logical kernel
step may not need that assumption, but any accepted result transported or
reused by digest does. Reports do not present a digest match as mathematical
truth without the corresponding evidence/validation path.

Exhaustive reasoning over a finite domain genuinely established by the Boon
program may yield `valid` with `exhaustive_finite_domain`. Searching an
implementation-selected sample or arbitrary bound can only provide test
evidence and never satisfies a required `WHERE`.

Platform-conditional proof becomes usable only in a later phase with an
explicit target-profile trust model. It is never silently displayed as a
closed proof.

### Standard-Library Summaries

Proof summaries are compiler-owned semantic artifacts associated with resolved
callable ids. Each summary has:

- a versioned semantic id;
- an exact callable and implementation/semantic digest;
- exact argument and result types;
- preconditions and returned facts;
- presence and error behavior;
- list order/currentness behavior where applicable;
- cardinality-to-`Number` conversion behavior;
- a checked proof/certificate for the verified classification, or an explicit
  trusted classification when only reference semantics or human audit exists;
- a hash participating in proof-cache invalidation.

Changing a standard operation without changing its proof-summary hash is a
compiler error and release-process failure.

V1 implements only the closed list-lemma set in “Structural Records And
Lists.” Adding a new summary schema is a reviewed semantic extension, not an
optimizer side effect. Differential tests remain required even for a
machine-verified summary, but do not upgrade a trusted summary's assurance.

### Incremental Verification

Incremental storage separates a logical solver-result cache from an accepted
evidence cache. A solver result is never itself permission to reuse a verified
bundle.

The solver-result key includes:

- normalized goal and assumptions;
- checked semantic identities;
- contextual materialization and semantic-graph-slice digests;
- public function/module statement hashes;
- standard-summary hashes;
- Number and list semantic profile ids;
- solver/kernel and VC-schema versions;
- proof mode and resource policy.

The accepted-evidence key additionally includes:

- every callable-dependency-manifest and proof-context-key hash;
- the current provider semantic-program and required-obligation-manifest
  digests, excluding its not-yet-finalized verification-manifest and exported
  bundle digests;
- already finalized imported-provider semantic-program/
  verification-manifest digests and imported theorem, evidence-core, and
  complete bundle hashes;
- exact verified/trusted summary evidence hashes;
- static authority-activation requirement hashes;
- verifier version, full assurance policy, trust roots/certificate checker
  versions, and permitted resource limits.

An accepted-cache hit revalidates all content hashes and the current assurance
policy, resolves the digest-bound proof/derivation/certificate body, and checks
that body again with the currently accepted local kernel before constructing
`VerificationManifest` or a public bundle. For a native proof mode without a
replayable positive derivation, the prover reruns; its old `valid` bit is only
a cache hint and can never be accepted evidence. Refuting models are likewise
replayed only as refutation diagnostics. A missing, corrupt, forged, stale, or
uncheckable evidence body becomes a miss/failure, not truth.

V1 does not trust the cache store. If a later policy permits an authenticated
trusted-result cache instead of local evidence checking, that cache, key
management, and authentication mechanism become an explicit TCB and assurance
dependency. `trusted_smt` records similarly remain unusable under the default
policy without the separately approved trust/certificate path.

An unchanged theorem statement is intentionally insufficient when private
dependencies, provider evidence, activation requirements, or trust policy have
changed.

A later finalized-artifact cache may key exported bundles by the already
constructed provider verification-manifest digest plus theorem/evidence inputs.
That post-manifest cache is never consulted as raw obligation evidence and
therefore does not make the provider manifest depend on its own export.

An edit invalidates only dependent obligations. Reports expose cold and
incremental parse, typecheck, VC-generation, and solver timing separately.

`BOON_COMPILER_PERFORMANCE_PLAN.md` owns the hard end-to-end cold/no-cache and
warm compiler budgets, including the time spent sealing and verifying an
executable request. Accepted evidence caches and compiler-session reuse cannot
satisfy either cold gate. This plan owns the additional solver-mode resource
and timeout ceilings: the first implementation freezes them in a
machine-readable verifier budget manifest from the measured phase breakdown
before proof implementation is accepted. No formal phase may defer those
ceilings to a later unspecified measurement. A timeout, `unknown`, unsupported
theory, invalid evidence, or missed soundness condition remains failure even
when a performance budget is missed.

### IR Erasure

Initial implementation:

```text
erase(function input contract) = no executable operation
erase(value refinement) = its input value
```

The normative transformation is:

```text
ContractVerifiedProgram
    -> proof-erased SemanticProgram projection
    -> opaque ErasedProgram with semantic/verification digests
```

`ErasedProgram` and the executable portion of `MachinePlan` contain no runtime
`WHERE` operation. The plan/package sidecar intentionally retains static
authority activation requirements; the host receipt gate is deployment
metadata/control, not a subject-value assertion or graph operation.
Verified public-contract bundles and compiler/debug proof reports remain
interface/sidecar projections in `VerifiedCompilationUnit`; they are not
discarded with executable proof nodes and never become runtime values.

Production erasure is compared with a test-only reference projection of the
same verified `SemanticProgram`, not with arbitrary hand-edited source.
Handwritten contracted/uncontracted source pairs remain useful illustrative
differential fixtures but are not the normative erasure oracle.

The canonical executable comparator strips proof, source, and debug metadata
and requires:

- equal executable values;
- equal event presence;
- equal `HOLD` and `LATEST` behavior;
- equal list order and currentness;
- equal effects and typed errors;
- stable expression/statement ids for unchanged source;
- equal canonical executable JSON;
- equal graph-node and capability-summary operation counts;
- equal scenarios and applicable budgets.

Proof and source package hashes may differ. The comparison is over canonical
executable semantics, not source identity.

Initial `SemanticProgram -> ErasedProgram` lowering remains in the V1 trusted
computing base. The comparator and runtime differential tests can expose
regressions; they do not constitute a formal proof of the lowering.

### Later Proof-Driven Optimization

Proof facts do not drive optimization in the first accepted implementation.
This keeps the initial erasure claim simple and measurable.

Later, `ContractVerifiedProgram` facts may enable:

- removal of redundant bounds and definedness checks;
- specialized list/index plans;
- bounded storage selection;
- narrower incremental recomputation;
- safe branch elimination;
- parallel or hardware scheduling where independence is proved;
- safer migration and schema-specialization paths.

Every fact-driven transformation requires:

- a generic optimization rule, never an example id;
- translation-validation evidence;
- semantic equivalence across values, presence, state, order, effects, and
  errors;
- a measured workload improvement;
- no regression for programs without the relevant proof facts.

Each later translation-validation report identifies the transformation,
validator version, source and target IR hashes, and validation result. A single
undifferentiated green “translation validated” flag is insufficient.

Formal source verification alone does not prove a buggy lowering, runtime, GPU
backend, or native renderer correct.

## Modules, Roles, Effects, And Trust

Authored checked theorems are part of function and module signatures; verified
bundles are produced only after verification. V1 composes contracts across
source modules verified in the same compilation. It rejects contracted
external or Client/Session/Server role crossings; Phase 7 adds those verified
transport paths. Silently dropping a theorem is never allowed.

The authored theorem and its verification evidence are separate:

```text
PublicContractTheoremV1 {
    schema_version
    public_callable_key
    public_contract_shape_hash
    passed_context_scheme
    normalized_header_clauses
    returned: Option<ReturnedTheorem>
    number_semantic_profile_hash
    referenced_public_statement_or_definition_hashes
}

VerifiedContractEvidenceCoreV1 {
    theorem_hash
    callable_dependency_manifest_hash
    provider_semantic_program_digest
    provider_verification_manifest_digest
    assumption_evidence_bundle_hashes
    summary_evidence_hashes
    coverage
    assurance_dependencies
    verifier_policy
    verifier_version
}

VerifiedPublicContractBundleV1 {
    theorem
    theorem_hash
    evidence_core
    evidence_core_hash
    bundle_hash
}

ProofReportDiagnostics {
    reachability
    counterexamples
    proof_timing
    cache_timing
    solver_diagnostics
}
```

The public callable key is a stable module/version/export identity; it contains
no body digest or process-local id. The public-contract-shape hash atomically
covers only developer-visible callable semantics:

- ordered ordinary input types, requiredness/canonical defaults, and their
  parent/output evaluation scopes;
- the alpha-normalized required `PASSED` context row/type/flow/snapshot/role
  scheme, observation modes, and any required closed subtree shapes;
- returned type, flow/presence mode, and typed-error shape;
- contextual `OUT`, dependent-computation, and compiler-context signature
  shapes;
- the public effect/capability classification, canonical role/transport mode,
  and any other caller-visible invocation semantics;
- semantic profiles and public semantic definitions needed to interpret the
  types or normalized formulas.

The exact private implementation closure is deliberately not part of this
shape or the theorem hash. The evidence core's callable-dependency-manifest
hash, provider semantic-program digest, verification-manifest digest,
assumption bundles, and summary evidence cover:

- lexical captures and their definitions;
- exact resource reads/writes and transitive effect targets;
- called callable, theorem, provider, and summary identities;
- external routes/wires, migration predecessors, persistence activation
  requirements, type instances, materializations, and semantic profiles;
- every other private or compiler-generated dependency enumerated earlier.

If an exact effect target is intentionally part of a public API, its public
capability/classification is represented in the public shape. Private resource
owners, route instances, and implementation call graphs remain evidence
dependencies.

The theorem may contain normalized `PASSED` contextual formals. It contains no
concrete `PASS` actual, materialization-local `OUT`, output-evaluated closure,
compiler context, private capture, hidden owner, or runtime identity.
Reachability status, counterexample presentation, and proof/cache performance
belong to report diagnostics, not the theorem, evidence core, or bundle hash.
Changing timing or diagnostic presentation therefore changes neither the
authored API nor evidence/cache identity. Presence-production and reachability
evidence likewise remain outside `ReturnedTheorem` and the theorem hash.

`FunctionTypeEntry` stores a verifier-neutral `CheckedContract`;
`ExternalFunctionType` stores only the corresponding checked theorem/reference
needed to diagnose a currently unsupported crossing. Verified bundles live in
compiler/module/package interface metadata outside `boon_typecheck`. A theorem
hash establishes identity and integrity, not truth. Before a supported importer
uses a returned fact, it must validate the complete bundle, public contract
shape, implementation dependency manifest, provider/manifest digests,
coverage, and local assurance policy, and must have one of:

- the provider source/`SemanticProgram` verified in the same compilation;
- a cached provider `SemanticProgram` whose digest is rechecked and whose
  verification is replayed under the local policy;
- a proof certificate checked by a locally accepted kernel.

The checked/module callable interface must also retain ordinary-parameter
requiredness/defaults and evaluation scopes, the principal `PASSED` context
scheme, compiler-context/`OUT` shape, and dependency-manifest reference.
Current external signatures that contain only ordinary args/result/coarse
effects are insufficient. Until a crossing carries this complete shape, a
callable with implicit context, captures, or a contract is rejected explicitly;
those dependencies are never dropped to fit the old interface.

External proof identity is derived from the settled canonical producer
role/function and wire edge after the distributed type fixed point has closed.
It is not derived from a checked-call field that merely records the current
consumer role, and provisional unresolved external types can never enter
verification evidence.

Remote producer lowering also checks the callable's free-declaration manifest.
A digest-bound closed constant may be admitted by the eventual interface
policy; a mutable store/source/list capture, implicit `PASSED`, `OUT`,
compiler context, or unexported external dependency must be reified as bounded
ordinary wire data under a verified contract or the export is rejected.

V1 guarantees the first path and may implement the second. It rejects a
precompiled external bundle that only claims `valid`; blindly trusting such a
claim would confuse contract integrity with proof. Certificate-backed or
provider-trust import is a later explicit extension.

Canonical bytes use an explicitly specified length-prefixed field encoding,
ordered lists, ordinal parameter binders in normalized formulas, and SHA-256.
Hashing does not depend on Rust `serde` map order, source formatting, or
process-local ids. Formula normalization is structural alpha-normalization plus
only explicitly sound rewrites. It preserves operand order, definedness, exact
rational semantics, `BITS[N]` width, and canonical collection ordering; a
rewrite that changes normalization, a possible terminal fault, or
`FLUSH`/commit behavior is forbidden.

The theorem hash drives contract/API compatibility and covers only the public
callable shape plus normalized authored statements and their public semantic
definitions. The bundle hash drives evidence/cache identity and covers the
canonical theorem plus canonical evidence core. A private refactor therefore
re-verifies and changes its dependency/evidence/bundle identities without
claiming an API-contract change. A public parameter/default/context/effect
classification, formula, or required public semantic-definition change
changes the theorem hash. Timings, wall-clock order, diagnostic text, and other
run-dependent report fields are excluded. In V1, any public-theorem hash change
invalidates dependent compilation and requires review; semantic compatibility
rules for safe strengthening or weakening are a later feature.

The derived hashes are non-recursive and domain-separated:

```text
theorem_hash =
    H("boon.contract-theorem.v1", canonical(theorem))

evidence_core_hash =
    H("boon.contract-evidence-core.v1", canonical(evidence_core))

bundle_hash =
    H(
        "boon.verified-contract-bundle.v1",
        canonical(theorem),
        canonical(evidence_core)
    )
```

The canonical payloads contain no self-hash fields.
`provider_verification_manifest_digest` names the already finalized
pre-export manifest described above; that manifest contains no hash of the
bundle being constructed.

V1 application code cannot write `ASSUME`, `TRUSTED`, `ADMIT`, or equivalent.

The initial trusted computing base includes:

- parser and resolver;
- typechecker;
- `SemanticProgram` elaboration;
- exhaustive dependency/field classification and obligation-manifest
  construction;
- VC generator;
- native proof kernel;
- canonical theorem/formula/value/manifest/evidence/receipt encoders,
  domain-separated hash derivations, and the stated SHA-256 cryptographic
  assumption;
- manifest completeness, imported-bundle/evidence/policy, certificate, and
  accepted-cache validators; cache lookup never bypasses these validators;
- any explicitly reported trusted standard summary;
- any later explicitly reported trusted SMT backend;
- for restored contracted authorities only, the versioned
  value-plus-invariant-stamp atomic commit, integrity/compatibility checker,
  last-writer evidence/activation-basis resolver, and host-owned activation
  receipt catalog;
- proof-to-IR erasure boundary.

Counterexample replay is diagnostic validation of `refuted`, not positive-proof
evidence. Runtime scenarios similarly test behavior but are not theorem
evidence.

Proof reports carry machine-enforced coverage fields for:

- explicit source contracts and instantiated calls;
- semantic elaboration;
- erasure evidence;
- each optimized IR translation;
- backend artifact evidence;
- runtime or host evidence, including fresh/restored authority activation and
  the exact persistence assurance category.

A report must not imply a layer was covered merely because a later runtime
scenario happened to pass.

External effects require ordinary runtime handling unless a later target profile
provides a versioned, visible provider contract. Availability, durability,
latency, authorization, and remote behavior cannot be invented by local
`WHERE`.

## Diagnostics

Diagnostics operate at Boon level and identify proof ownership.
Each structured diagnostic carries a stable diagnostic id, category, semantic
owner, primary/related source spans, logical status, assurance dependencies,
and report-obligation id; display text is not the machine contract.

### Caller Failure

```text
Cannot call `accept_percentage`.

Required by function header:
    number <= 100

Available path facts do not establish this condition.
```

For contextual input, the same diagnostic points to both the header projection
and the explicit `PASS:` origin or inherited forwarding chain:

```text
Cannot call `total_value`.

Required by contextual function header:
    PASSED.store.total >= 0

Effective context:
    inherited by `total_panel`
    introduced by PASS at <source span>

Available path facts do not establish this condition.
```

### Returned-Value Failure

```text
Cannot establish returned-value condition in `bounded_double`.

Condition:
    result <= 90

Counterexample:
    value = 50
    result = 100
```

### `HOLD` Failure

```text
Cannot preserve state condition.

Previous committed state:
    count = 0

Trigger:
    decrement

Candidate next state:
    count = -1

False condition:
    current >= 0
```

### List Failure

List diagnostics show only relevant structural rows and fields. They never show
hidden list keys, slots, generations, runtime ids, or renderer identities.

### Unknown Or Unsupported

The diagnostic distinguishes:

- refuted with a replayed counterexample;
- unknown;
- solver timeout;
- unsupported language or theory;
- missing external contract.

It may suggest:

- a missing function-header requirement;
- an earlier runtime validation branch;
- grouping related state into one structural `HOLD`;
- rewriting into a supported equivalent pure form.

It must not automatically weaken a result guarantee or add an unsafe
assumption.

## Editor, Playground, CLI, And AI

### Dev Window

The dev window gains a generic proof view:

- source gutter status for each `WHERE`;
- caller-requirement versus producer-guarantee distinction;
- ordinary versus implicit-`PASSED` caller inputs, with explicit/inherited
  context provenance;
- hoverable available facts and provenance;
- a bounded “depends on” view for captures, resources, providers, defaults,
  summaries, and semantic profiles;
- base versus transition status for `HOLD`;
- branch coverage;
- shortest source-level counterexample;
- unsupported or timeout reason;
- proof timing and cache status.

The app preview never receives example identity or proof-example shortcuts. It
receives source and the generic compile result.

### CLI

`boon_cli check` is strict: required proof failure is a normal compile failure.

Add this exact reporting command:

```text
boon_cli verify <source> [--target <profile>] --report <path>
```

It follows the existing `boon_cli` `check`, `run`, `dump-plan`, and `dump-ir`
command surface and emits:

- source and contract digests;
- obligation owner and source span;
- normalized condition;
- logical status and assurance dependencies;
- assumptions and coverage;
- counterexample where available;
- verifier, summary, and solver versions;
- resource policy;
- phase timings.

When verification is reached, the command writes a schema-valid report
atomically on both success and proof failure. A failed obligation exits nonzero
and produces no compiled artifact.

### AI Generation And Review

Structured proof reports support a contract-locked generation loop:

1. a human or higher-level design process establishes the contracts;
2. AI generates or refactors implementation;
3. the verifier returns precise failed obligations and counterexamples;
4. AI repairs the implementation without changing the contract;
5. semantic diff highlights any contract change for human review.

The diff must call out:

- moving a condition from result to function header;
- strengthening a header and therefore burdening callers;
- weakening or deleting a returned guarantee;
- reducing an actual program bound merely to make proof easier;
- moving validation after an effect;
- replacing static proof with runtime branching;
- introducing a platform assumption.

Humans review the theorem statements, domains, bounds, and trust chain. They do
not need to review generated solver scripts.

## Reliability, Performance, And Product Effects

### Reliability

The feature can statically reject:

- invalid calls;
- impossible local result guarantees;
- broken branch joins;
- state transitions that leave an invariant;
- initial state that never satisfied an invariant;
- inconsistent record relationships;
- broken list partitions and counts;
- after Phase 4, migration that fails a declared preservation condition.

These guarantees hold only over modeled Boon semantics and explicit trusted
boundaries. They do not replace runtime integration, native rendering, effect,
durability, or human usability verification.

### Performance

`WHERE` itself has no intended runtime cost because it is erased and does not
duplicate input evaluation. V1 separately adds ordinary executor validation
for the artifact's explicit `ListCapacityProfileV1`. That check is not a
runtime proof assertion; its branch/cost and terminal-capacity-fault behavior
must be measured and reported separately.

Persisted authorities carrying verified invariants also have an explicit host
cost: bounded stamp bytes, atomic value-plus-stamp commit work, last-writer
evidence retention/lookup, integrity and compatibility checks, and activation
receipt creation/validation and latency. These costs occur in
persistence/activation rather than as graph operations or per-use `WHERE`
checks. Benchmarks report fresh versus restored startup, requirement/stamp/
receipt size, write amplification, and lookup/receipt latency. Compile time
also increases and must be measured by phase.

Later proof facts can enable better generic plans, but performance claims
require:

1. proof of the source fact;
2. validation of the transformation;
3. correct result evidence;
4. measured runtime improvement;
5. no hidden example-specific path.

### AI Generation

Contracts make generation less guess-driven:

- input domains are explicit;
- result guarantees are explicit;
- counterexamples are machine-readable;
- refactors can be judged against stable semantics;
- contract gaming becomes visible in diffs;
- generated code cannot replace missing reasoning with an app-level `ASSUME`.

### Human And AI Verification

The useful review unit becomes a small local statement near the value it
describes. Humans decide whether that statement captures the product rule. AI
can search implementation space and respond to counterexamples. The compiler
remains the shared arbiter.

### Costs And Risks

- Solver latency can damage the edit loop without modular caching.
- Incorrect Number modeling can create unsound arithmetic proofs.
- Incorrect list summaries can create large unsoundness.
- Function-header requirements can be abused to move bugs to callers.
- Trivial or tautological conditions can create false confidence.
- Source proof can be overstated as backend or operational proof.
- External assumptions can become invisible if reports do not surface them.
- Proof examples can become toy-only if real TodoMVC and migration capstones are
  not completed.

The rollout and gates below exist to contain these risks.

## V1 Supported Scope

V1 comprises Phases 0 through 3 and is complete only when it supports:

- both agreed syntax forms;
- pure condition blocks over present `True | False` Tags;
- constants, structural equality, Tags, records, and supported exact-rational
  range reasoning under `ExactNumberSemanticProfileV1`;
- exact `BITS[N]` and finite `MAP`/`SET` proof models for the operations in the
  language-foundations phases completed before formal acceptance;
- `FLUSH` path, candidate-abort, and staged-effect suppression modeling without
  exposing its hidden status as data;
- modular function input and returned-value contracts over parent-evaluated
  ordinary and implicit `PASSED` contextual formals;
- exhaustive callable dependency manifests, resource-specific effect
  footprints, and proof-assumption provenance;
- complete transitive obligation manifests and versioned theorem/bundle
  transport across source modules in one compilation;
- fail-closed rejection of contracted external or role crossings;
- path-sensitive `WHEN`, `WHILE`, and `THEN`;
- `LATEST` candidate coverage;
- single-`HOLD` induction;
- fail-closed fresh/restored state and list activation with versioned invariant
  statements, last-writer evidence, and measured persistence stamps;
- the closed initial set of standard-list summary schemas;
- an explicit target-compatible list-capacity profile and lossless exact
  `List/count`;
- parameter-equivalent `PASS`/`PASSED` proof substitution plus V1-local rules
  for `OUT`, output-evaluated arguments, and compiler-supplied contexts;
- the four teaching examples;
- the TodoMVC partition invariant;
- deterministic proof-report semantic cores with separately non-hashed
  timing/cache diagnostics, plus stable negative diagnostic ids;
- minimal editor/compiler-worker diagnostics and last-valid-preview retention;
- mandatory erasure before runtime.

This certifies complete discharge of the explicit-contract manifest, not
whole-program safety, migration preservation, platform behavior, or backend
correctness.

## Deferred Semantic-Risk Extensions

Extend by semantic risk, not by adding keywords speculatively:

1. richer list mutation and ordering invariants;
2. migration preservation across record/list schema evolution;
3. versioned provider contracts for durable effects;
4. distributed assume/guarantee contracts with visible trust provenance;
5. bounded-response and other temporal properties;
6. information-flow and declassification properties;
7. target-specific cost and resource contracts.

Value-local safety properties may continue to use `WHERE`. Liveness,
hyperproperties, and physical-cost claims must not be forced into `WHERE` if
their semantics require a different future construct.

The roadmap does not pre-authorize another application keyword. In particular,
the Phase 4 migration witness is an internal semantic requirement until a
separate source-surface review proves that the two agreed `WHERE` forms cannot
express the needed developer contract cleanly.

## Implementation Phases

All phases in this plan are mandatory before canonical processor or other
hardware implementation begins. Phases 0 through 5 establish the source and
verification contract. Phase 6 is implemented jointly with packed
`KernelIR` selection and translation validation in
`BOON_PACKED_DATA_AND_DENSE_INTERNALS_PLAN.md`. Phase 7 starts only after the
Client/Session/Server role interfaces and distributed semantic fixed point are
stable. Research inventories may run earlier, but hardware implementation may
not consume a partial proof architecture.

The unified goal pulls forward only the `boon_semantic`, `boon_verify`,
required-manifest, `ContractVerifiedProgram`, and opaque `ErasedProgram`
artifact boundary needed to establish the compiler spine. That bootstrap slice
does not satisfy a Phase 0 or Phase 1 exit. Both phases are audited and
completed here after the final foundations and typed-list semantics exist.

### Phase 0: Freeze Semantics And Make Parsing Fail Closed

- Accept this document as the implementation contract.
- Add all accepted and rejected syntax fixtures first.
- Implement the compound two-brace function AST, complete token consumption,
  recovery, spans, and canonical formatting.
- Add the data-only `boon_contract` DTO/hash crate and freeze the crate/type
  ownership table.
- Extract the semantics-essential portion of current `boon_ir` into
  `boon_semantic`, and establish one `SemanticProgram` for verification and
  lowering.
- Replace projection-only/dynamic `PASSED` lookup with stable contextual
  formals, principal structural context schemes, and explicit/inherited/none
  call bindings.
- Freeze `CallableDependencyManifest`, the exhaustive dependency enumerator,
  exhaustive field-schema/disposition registry, schema-drift architecture
  gate, resource-specific effect footprints, and proof-context-key encoding.
- Freeze `AuthorityActivationRequirementV1`,
  `VerifiedAuthorityInvariantStampV1`, `VerifiedActivationReceiptV1`, the
  compile-time/runtime phase boundary, the fail-closed persistence-activation
  contract, last-writer evidence catalog, and
  `trusted_persistence_activation_v1` assurance scope for restored proved
  authorities.
- Freeze `ExactNumberSemanticProfileV1`, the shared exact Number operations,
  `ListCapacityProfileV1`, and exact `BITS[N]`/`MAP`/`SET`/`FLUSH` proof
  profiles.
- Freeze and centralize `SourceBundleDigestV1`.
- Freeze `PublicContractTheoremV1`,
  `VerifiedPublicContractBundleV1`, their canonical encodings, and hash rules.
- Define `RequiredObligationManifest`, successful `VerificationManifest`, and
  the private construction boundary.
- Freeze proof-preflight obligation/result/diagnostic byte limits and the
  exact worst-case report-reservation rule.
- Define the canonical executable comparator and test-only proof-erasure
  projection.
- Write the external-solver ADR; selecting a solver remains optional.
- Preserve exact behavior for all existing source without `WHERE` except the
  explicitly specified list-capacity boundary, which is an independent
  language/runtime semantic hardening rather than proof syntax.
- Record baseline AST, `CheckedProgram`, `SemanticProgram`, `ErasedProgram`,
  `MachinePlan`, stable ids, canonical executable JSON, graph/operation counts,
  scenarios, and applicable budget evidence.

Exit:

- no header token can be silently ignored;
- all rejected forms fail precisely;
- semantic elaboration is shared rather than duplicated;
- all current non-`WHERE` programs retain canonical executable behavior within
  the admitted list-capacity domain, and the new typed boundary is covered by
  dedicated differential/cost tests.

### Phase 1: Pure Values And Functions

- Add checked contracts and proof-only aliases.
- Complete `boon_verify`, `ContractVerifiedProgram`, and the
  completeness-checked manifest on the final semantic model.
- Emit verified theorem/evidence bundles for source-co-compiled modules and
  reject claim-only precompiled imports.
- Implement constants, private logical-Boolean reasoning over `True | False`
  Tags, structural equality, bounded supported exact-Number and `BITS[N]`
  reasoning, finite `MAP`/`SET` operations, `FLUSH` paths, ordinary and
  `PASSED` contextual substitution, lexical capture closure, and
  continuous-value `WHEN`/`WHILE` branch path facts.
- Route every verified-check and artifact-producing compiler entrypoint through
  verification; diagnostics-only requests stop before artifact construction.
- Remove/private all raw source-to-IR entrypoints and require opaque,
  verification-derived `ErasedProgram` at compiler backends.
- Add `CompileOutcome`, source-bound per-source reports with deterministic
  semantic cores, the exact
  `boon_cli verify` command, and core diagnostics.
- Add the standalone formal-example xtask and report schema.
- Add `refresh-formal-contracts [--check]` and the negative-case manifest
  runner.
- Integrate minimal proof diagnostics into the native compile worker and retain
  the last successfully verified preview after a failed edit.
- Ship `where_safe_choice`.

Exit:

- callers cannot violate headers;
- direct and inherited `PASS` calls discharge the same modular requirements as
  ordinary parameters, including polymorphic context instances;
- returned guarantees compose;
- unknown fails closed;
- imported contracts create obligations even when local source has no `WHERE`;
- no unresolved obligation reaches IR.

### Phase 2: Reactive State

- Model reactive `WHEN`/`WHILE`, event-mode branching, `THEN`, `SKIP`, and
  exact `LATEST` coverage.
- Add single-`HOLD` base and step obligations.
- Add fresh-versus-restored activation coverage and exact semantic-compatible
  scalar invariant-stamp/last-writer-evidence validation plus exact
  value-and-artifact-bound runtime activation receipts.
- Add transition-trace diagnostics.
- Ship `where_runtime_input`, `where_bounded_counter`, and the
  `flow_operators` upgrade.

Exit:

- removing any counter guard produces the expected source-level induction
  failure;
- `before` never escapes its body;
- an uncontracted or mismatched persisted scalar state cannot activate under a
  claimed invariant;
- an implementation-only refactor with the same invariant statement can
  activate valid old state after predecessor/current evidence validation;
- no per-update runtime `WHERE` assertion is introduced; restore compatibility
  is an explicit activation gate.

### Phase 3: Lists And Real TodoMVC

- Enforce the artifact's explicit `ListCapacityProfileV1` on every executor
  construction and mutation path, and replace unchecked host-size casts.
- Add the enumerated versioned standard-list summaries with assurance and
  implementation digests.
- Extend invariant stamps and activation checks to persisted list authorities,
  row schemas, and supported mutation-summary identities.
- Prove only the supported map/retain/append/remove/count/every/find
  relationships.
- Ship `where_verified_rows`.
- Add the real TodoMVC partition invariant.

Exit:

- the Todo invariant holds through every supported dynamic mutation;
- a broken complementary predicate produces a structural counterexample;
- hidden list identity never enters source or reports.

### Phase 4: Migrations

- Begin the post-V1 migration extension.
- Integrate proof obligations with actual migration authority and sequence
  semantics.
- Bind each proof to the exact predecessor application identity, schema
  version/hash, migration recipe/catalog hashes, and state/list leaf
  fingerprints; a changed predecessor catalog always re-verifies.
- Transport verified old-state properties through the compiler-owned `DRAIN`
  edge.
- Preserve a distinct checked/semantic migration-context read for
  `DRAIN { PASSED.path }`, or reject that source form until the representation
  exists.
- Add a proof-only predecessor witness before allowing app-authored old/new
  relational contracts.
- Add counter preservation.
- Add Todo record/list preservation conditions.
- Verify failed migration stages emit no accepted artifact.

Exit:

- preservation is proved over persisted authority, not only fresh defaults;
- migration reports remain separate from runtime scenarios.

### Phase 5: Editor, CLI, AI, And Incremental Budgets

- Complete the rich proof gutter and dev proof view beyond Phase 1 diagnostics.
- Refine CLI report inspection without changing the Phase 1 command contract.
- Add semantic contract diff.
- Measure cold and incremental verification.
- Add budgets based on measured baselines.
- Add cache correctness and invalidation tests.

Exit:

- a developer can understand each failed proof without inspecting solver text;
- AI tooling can consume stable structured diagnostics;
- incremental edits invalidate only dependent proofs.

### Phase 6: Proof-Driven Optimization

- Start only after formal Phases 0–5 and packed-runtime prerequisites are
  complete.
- Select measured generic opportunities jointly with packed `KernelIR`
  construction.
- Consume facts from `ContractVerifiedProgram` through a versioned verified
  fact projection; never reread source contracts in the optimizer.
- Add per-transformation translation validation between the verified semantic
  reference and packed kernel.
- Keep proof-disabled reference execution test-only for differential checks.
- Cover values, presence, exact Number, `BITS[N]`, canonical collection order,
  state/currentness, `FLUSH`, effects, and terminal faults.
- Benchmark correctness before speed.

Exit:

- every accepted optimization has semantic and measured evidence;
- every proof-selected packed kernel has source/target hashes and
  transformation-specific validation evidence;
- no example-specific branch exists;
- programs without relevant facts do not regress.

### Phase 7: External And Distributed Contracts

- Start only after Client/Session/Server interfaces, role identities, wire
  schemas, and the distributed semantic fixed point are stable.
- Version target-profile provider contracts.
- Extend cross-role function signatures.
- Add certificate-checked or explicitly trusted provider-bundle import.
- Surface every external assumption.
- Keep runtime failure handling.

Exit:

- no contract disappears across a role boundary;
- conditional proof is visibly distinct from closed proof;
- app source still has no hidden trust escape hatch.

## Test Matrix

### Parser Tests

- accepted header and pipeline forms;
- comma and newline condition separators;
- multiline infix/pipeline/call clauses and nested branches;
- same-line and next-line second function-body opener;
- complete token consumption;
- missing contract close/body opener recovery;
- `WHERE` remains an ordinary name outside its contextual syntax positions;
- all rejected spellings;
- precise spans and formatting stability.

### Typechecker Tests

- header visibility for parent-evaluated ordinary parameters and direct or
  transitively inherited `PASSED` contextual formals;
- header exclusion of `PASS`, `OUT`, output-evaluated ordinary parameters,
  compiler-supplied contexts, dynamic captures, and body declarations;
- rejection of a nominally pure header helper with a transitive `OUT`,
  output-evaluation/compiler context, ambient or unbound state/source, effect,
  external, migration, or hidden dependency, while allowing explicitly
  permitted ordinary/`PASSED` boundary formals;
- direct `PASS`, two-level inherited context, explicit context replacement,
  projected context fields, and missing-context diagnostics;
- principal context-row inference, compatible extra fields, conflicting
  type/flow/snapshot/role requirements, and distinct polymorphic call
  substitutions;
- projected bare/subrecord aliases and transparent `PASS: PASSED...`
  forwarding, plus closed-shape enforcement or precise rejection for
  proof-relevant whole-context equality, spread, opaque use, and result
  contracts;
- output-evaluation-scope inference through one and several wrappers, including
  conflicting `OUT` scopes;
- root, module, nested, projected, shadowed, record, `BLOCK`, pattern, row, and
  detached-state lexical capture classification;
- rejection of a purported closed record/header helper whose direct field or
  spread hides a source/state/list resource alias;
- omitted versus explicitly supplied canonical defaults, changed-default
  invalidation, and requiredness preservation in checked signatures;
- helpers wrapping every ambient intrinsic class, including session/route
  context, identity generation, external reads, and `element`;
- body-local invisibility from headers;
- alias locality and escape rejection;
- fact dropping at `BLOCK`, `HOLD`, and materialization scope exit;
- `True | False` condition typing, presence, purity, and totality;
- function result export over ordinary and `PASSED` formals;
- rejection of private/materialization-local free declarations or unrecorded
  ambient assumptions in exported contracts;
- source-module contract composition and contracted role/external rejection;
- unstratified cycle rejection and legal single-`HOLD` induction.

### Semantic Elaboration

- one contextual materialization graph feeds verification and lowering;
- stable `PASSED` contextual formals and exact explicit/inherited/none call
  bindings;
- exact `OUT`, output-evaluation, `ElementState`, and repeated
  materializations;
- proof-context identity includes both `PASS` origin/frames and complete `OUT`
  net/owner/port provenance;
- proof checkpoints preserve flow, presence, error, and event-candidate modes;
- exact capture, source, state/list, effect/external, migration-predecessor, and
  checked-source mappings;
- exhaustive dependency classification for every checked scope, declaration,
  statement, expression/text segment, match pattern, callable/parameter/
  evaluation kind, value-use/occurrence kind, call entry/context/contextual
  operation, order-direction, flow/type/shape, role, and relevant checked side
  table;
- deterministic distributed semantic fixed point before verification;
- distinct semantic/executable ids and one explicit mapping;
- no proof-only edge changes executable graph identity.

### Dependency Completeness And Cache Identity

- same ordinary signature and same four effect bits but different captured
  `HOLD`, `SOURCE`, list, external value, or host operation produce different
  implementation-dependency and evidence-bundle hashes while retaining the
  same public theorem hash when public shape/formulas are unchanged;
- changing a lexical capture definition or projection invalidates the
  implementation evidence and all dependent proof caches;
- a private refactor with unchanged public shape and normalized formulas keeps
  the theorem-statement hash stable, while a changed public default, contextual
  scheme, effect classification, semantic definition, or formula changes it;
- unchanged theorem statements cannot reuse accepted evidence after an
  imported evidence-core/bundle, provider manifest, callable dependency,
  activation requirement, verifier policy, trust root, or permitted assurance
  category changes;
- a self-consistent forged cached `valid` core, missing evidence body, changed
  evidence body, and stale certificate all fail local replay/checking; a proof
  mode without replayable positive evidence reruns instead of accepting its
  cached status;
- the same `OUT` provenance with a different explicit or inherited `PASS`
  actual cannot reuse materialized evidence;
- the same polymorphic helper at different row/result/key types receives
  alpha-stable but instance-distinct substitutions;
- a hidden detached row-state capture is present in coverage and absent from
  public formulas/reports;
- the same expression graph used as an ordinary runtime value versus a render
  slot receives the correct distinct materialization/currentness identity;
- host-port endpoint/correlation metadata participates whenever a proof slice
  reaches HTTP/WebSocket sources or response/action outputs;
- omitted and equivalent explicit defaults share normalized identity, while a
  changed default invalidates summary and proof caches;
- a distributed producer with an ambient mutable capture is rejected, while an
  allowed closed constant capture is fully bound by interface evidence;
- different migration predecessor catalogs, semantic-schema hashes, or leaf
  fingerprints cannot reuse preservation evidence;
- a same-statement private refactor may accept a restored invariant stamp only
  after validating its named last-writer evidence and current proof; a missing
  predecessor evidence record or changed invariant statement fails closed;
- adding a new semantic variant without a dependency classification fails the
  exhaustiveness gate.
- adding a field to any registered checked/semantic/lowering/proof record
  fails the field-schema gate until it is explicitly consumed or classified;
  fixtures specifically protect `CheckedCall.pass`, `type_substitutions`,
  every lowering-metadata table, and `ErasedFieldDef.resource_only`.

### Verifier

- valid constant obligations;
- refuted constants;
- contradictory/unknown header rejection and satisfying-model replay;
- ordinary and direct/inherited `PASSED` caller substitution;
- imported-call obligation completeness with no local `WHERE`;
- exact authored `ContractId` and child `ConditionId` coverage, including one
  modular theorem called from both ordinary and repeated/materialized scopes;
- exact equality between condition/instantiation coverage obligations,
  required obligations, and accepted evidence;
- symbolic checking of an uncalled projected/open-row-transparent or statically
  closed `PASSED` function, rejection of an unclosed proof-relevant whole
  context, and rejection of an unmaterialized `OUT`/provider-context
  checkpoint;
- branch coverage;
- `SKIP` does not imply liveness;
- returned payload guarantees do not imply presence;
- `LATEST` multiple-winner coverage;
- `HOLD` base failure;
- `HOLD` transition failure;
- fresh versus restored `HOLD` activation, including compatible
  statement/semantic-schema/stamp acceptance, last-writer and current-evidence
  validation, value/artifact/requirement-bound receipt creation,
  same-statement private-refactor acceptance, and rejection of a same-schema
  `-1` value written by an older uncontracted program under `current >= 0`;
- a restored run's subsequent commit records its activation-receipt basis; the
  next restart accepts the retained matching receipt and rejects missing,
  garbage-collected, wrong-authority, wrong-artifact, or wrong-predecessor
  basis evidence;
- distinct pre-state and post-state proof identities even when the checked
  lexical alias shares its declaration id with the state owner;
- per-materialization `OUT`, output-evaluated, and compiler-context facts and
  public-export rejection;
- a materialization-dependent returned checkpoint remains exact-call-local and
  cannot be reused by a different `OUT`/provider materialization; only a
  symbolic quantified summary permits public export;
- per-materialization caller obligations when an otherwise modular `PASSED`
  actual depends on `OUT` or another repeated context;
- supported exact rational arithmetic, normalization, comparison, and authored
  rounding;
- numerator/denominator, arithmetic-work, API-domain, and list-capacity
  resource boundaries;
- exact list-cardinality/profile bounds and complementary partition
  arithmetic;
- `BITS[N]` widths, slicing, shifts, interpretation, and checked conversions;
- extensional `MAP`/`SET` equality, canonical ordering, operation summaries,
  and collision/scheduler-independence;
- `FLUSH` candidate-abort, staged-effect suppression, lexical erasure, and
  non-persistability;
- unsupported arithmetic;
- each enumerated list summary and its implementation/semantic digest;
- timeout and unknown fail closed;
- replayed counterexamples.

### IR And Runtime

- no `WHERE` executable operation;
- production erasure/reference-projection canonical equivalence;
- no duplicate input evaluation;
- no graph-node or operation increase attributable to `WHERE` before
  proof-driven optimization; independent list-capacity validation is measured
  separately;
- bounded invariant-stamp size, atomic-commit write amplification,
  predecessor-evidence lookup, and fresh/restored activation latency are
  measured separately from erased `WHERE`;
- identical runtime scenarios;
- atomic authority-value/invariant-stamp persistence and fail-closed scalar/list
  restore activation, with no restored value installed before its exact
  receipt validates;
- failed hot reload preserves the last valid graph;
- structured failed-proof outcome retains a report and emits no artifact;
- no unresolved obligation can call raw lowering;
- all legacy raw lowering APIs are private/removed and architecture-scanned.

### Examples

- all four new positive scenarios;
- all source-comment teaching mutations represented as negative fixtures;
- upgraded `flow_operators`;
- real TodoMVC mutation coverage;
- manifest validation and catalog loading;
- source/scenario/compiled-unit binding;
- shared `SourceBundleDigestV1` fixtures across compiler/runtime/package;
- expected normalized contract digest and clause count;
- no invalid source in the manifest.

Migration-extension tests add counter and Todo preservation, distinct
`DRAIN { PASSED.path }` preservation-or-rejection behavior, predecessor-catalog
cache invalidation, and durable effect-outbox scope only in Phase 4.

### Tooling And Reports

- source-bound, canonically ordered report with deterministic semantic core and
  explicitly non-hashed operational timings;
- `boon.proof-report.v1` schema, atomic failure write, and 1 MiB bound;
- exact 512-obligation and canonical report-reservation boundaries, including
  513/preflight-capacity rejection before the solver starts, complete
  post-preflight result-core retention, and deterministic diagnostic
  projection/truncation digests;
- stable semantic owner ids;
- exact required/evidence obligation-set equality;
- imported theorem/evidence bundle validation under local assurance policy;
- precompiled claim-only bundle rejection;
- public-contract canonical encoding and hash fixtures;
- logical-status/assurance separation;
- source-level counterexamples;
- no hidden runtime ids;
- contract semantic diff;
- `refresh-formal-contracts [--check]` and negative-case manifest behavior;
- cache hit/miss and invalidation correctness;
- `boon.formal-examples.v1` schema, byte limit, and exit behavior;
- formal xtask independence from the native handoff manifest;
- report schema compatibility.

### Architecture

- no example-id branches;
- no app-level trust keyword;
- no runtime assertion fallback;
- no mathematical-real substitution for general Number;
- no duplicated semantic elaboration between verifier and lowering;
- no production unchecked lowering API;
- no wildcard/rest field consumption at dependency boundaries and exact
  generated field-schema/classifier equality;
- no contract loss at module or role boundaries;
- no native handoff gate weakened or overloaded.

The implementation runs focused package tests in dependency order:

```text
boon_data
boon_contract
boon_parser
boon_typecheck
boon_semantic
boon_verify
boon_ir
boon_compiler
boon_plan_executor
boon_app_package
boon_cli
boon_example_manifest
boon_runtime
boon_editor
boon_native_playground
xtask
```

Then it runs `cargo xtask verify-architecture`,
`cargo xtask verify-formal-examples`, and the applicable workspace tests.
Static syntax or proof changes do not require unrelated native GPU handoff
reports.

## Acceptance Criteria

### V1 Acceptance: Phases 0–3

1. The only new app-facing forms are the two forms in this document.
2. The compound function header has two distinct parsed blocks, and every
   header token is consumed or diagnosed.
3. Header scope contains parent-evaluated ordinary parameters and inferred
   `PASSED` contextual formals, but no `PASS`, `OUT`, output-evaluated
   parameter, compiler-supplied context, dynamic capture, or body declaration.
4. Pipeline aliases are required, proof-only, lexically local, and follow the
   stated shadowing rule.
5. Conditions are pure, total under admitted assumptions, defined, and produce
   present values of the closed `True | False` Tag set. Subjects may retain
   ordinary modeled reactive, state, or effect-result semantics; every
   admitted produced subject is defined, and `WHERE` adds no executable
   effect.
6. Header satisfiability is required; contradictory, unknown, timed-out, and
   unsupported domains are rejected.
7. Only `valid` results with V1-allowed assurance dependencies satisfy an
   obligation; arbitrary bounded search and platform-conditional evidence do
   not.
8. Every artifact-producing compile path constructs the shared
   `SemanticProgram`, complete verification manifest, and
   `ContractVerifiedProgram` before IR lowering. Diagnostics-only requests
   produce no executable or verified artifact.
9. Required obligation ids and evidence ids match exactly, including imported
   contracted calls in sources with no local `WHERE`; every authored
   `ContractId` and child `ConditionId` has definition and required
   call/materialization coverage. One modular ordinary/`PASSED` theorem may
   have both ordinary and concrete `OUT`/provider-context call
   instantiations.
10. Verified public-contract bundles survive supported module crossings under
    the importer's assurance policy and locally checkable provider evidence, or
    the contracted crossing is rejected; V1 always rejects contracted
    role/external crossings.
11. Initial executable graph IR contains no `WHERE` operation, proof wrapper,
    or per-evaluation runtime assertion generated from a contract. Static
    persisted-authority activation requirements survive in plan/package
    sidecars, and their fail-closed host receipt gate is required before
    restored state installation. Independent ordinary list-capacity validation
    remains part of list semantics.
12. Existing programs with no local or imported contracts retain stable ids,
    canonical executable JSON, graph/operation counts, scenarios, and
    applicable budgets within the admitted list-capacity domain; the new
    capacity boundary is tested and measured as a separately declared semantic
    change.
13. Production erasure matches the test-only reference projection of the same
    verified semantic program before proof-driven optimization.
14. All four examples compile, verify, produce the ordinary document/preview
    artifact, and pass scenarios, with their expected normalized theorem
    digest and clause count. Formal V1 acceptance requires no native GPU
    handoff report.
15. Every negative fixture fails with the intended diagnostic category and
    emits no new runnable artifact.
16. The bounded counter is proved by base and transition induction.
17. The real TodoMVC pointwise count invariant is proved through every
    supported live list mutation under the runtime-enforced exact list limit.
18. Reports identify coverage, logical status, assurance dependencies,
    profiles, source/contract/summary digests, versions, and timing.
19. The editor keeps the previous verified preview after a failed edit.
20. No example-specific compiler, runtime, renderer, host, or verifier behavior
    is introduced.
21. Source proof is never presented as native rendering, effect, durability,
    distributed availability, total-program safety, or backend correctness
    evidence.
22. Every callable has an exhaustive dependency manifest. Direct and inherited
    `PASSED` use, output-evaluation scopes, compiler contexts, captures,
    defaults, resources, effects, external providers, type/flow instances,
    structural/representation semantics, semantic profiles, persistence
    activation, assurance artifacts, and migration inputs participate in the
    appropriate public-statement, proof, evidence, coverage, and cache
    identities without exposing hidden runtime ids.
    The enum-and-field schema gate makes both a new variant and a new ordinary
    dependency-bearing struct field fail closed until explicitly classified.
23. A public returned theorem exists only after symbolic universal definition
    proof. A result fact depending on concrete `OUT`, output-evaluated, or
    compiler/provider materialization remains exact-call-local, is re-proved
    for future materializations, and never enters a public bundle.
24. Public theorem/API identity contains only public callable shape, normalized
    formulas, and required public semantic definitions. Private captures,
    resource targets, call graphs, current materializations, and proof
    dependencies change evidence/bundle identity and trigger re-verification
    without creating a false contract diff.
25. A restored contracted state/list authority activates only after exact
    semantic-schema/invariant/persistence compatibility, accepted last-writer evidence,
    current induction evidence, and the versioned persistence checker succeed
    and issue an exact artifact/requirement/value/stamp-bound receipt. Concrete
    receipts are host evidence and are never prerequisites or placeholders for
    compile-time `ContractVerifiedProgram` construction. Every later write
    binds either its fresh-base evidence or the restored run's retained
    activation receipt, preserving the induction lineage. Legacy, missing,
    mismatched, garbage-collected-while-referenced, or unverifiable provenance
    fails before the artifact becomes runnable; persistence stamp and
    activation costs are separately measured and reported.
26. Proof-report cardinality and exact byte reservation are checked before any
    obligation starts. Over-limit programs receive a precise preflight
    resource diagnostic; once proof starts, the 1 MiB report always retains
    complete result cores for every completed obligation, with only bounded
    diagnostic presentation eligible for explicit digest-backed truncation.

### Migration Extension Acceptance: Phase 4

1. Preservation is proved over existing persisted authority, not only fresh
   defaults.
2. `DRAIN` transport uses the shared migration semantic graph and never permits
   an illegal ordinary reference to `DRAINING` state.
3. If app-authored old/new relationships are introduced, they bind an explicit
   proof-only semantic witness through a separately approved source design.
4. Preservation evidence binds the exact predecessor application/persistence
   plans, semantic-schema/catalog/recipe hashes, and state/list leaves, and
   states the treatment of durable effect-outbox state.
5. Failed migration verification emits no accepted artifact.

### Tooling Acceptance: Phase 5

1. Developers can distinguish caller requirements, producer guarantees,
   logical status, and assurance without inspecting solver text.
2. Semantic contract diffs expose theorem weakening and responsibility shifts.
3. Incremental cache invalidation is dependency-correct and measured.

### Optimization Acceptance: Phase 6

1. Proof-driven optimization remains disabled until per-transformation
   translation validation and measurement pass.
2. Every accepted optimization is generic, preserves modeled semantics, and
   does not regress programs lacking its required facts.
3. Packed `KernelIR` selection consumes only verified fact projections and
   proves equivalent exact values, presence, order, state/currentness,
   `FLUSH`, effects, and terminal faults.

### External Contract Acceptance: Phase 7

1. No contract disappears across a role or provider boundary.
2. Every platform assumption is versioned and visible.
3. Conditional proof remains distinct from closed local proof, while ordinary
   runtime failure handling remains present.

## Repository Touchpoints

Expected implementation surfaces include:

- `docs/architecture/LANGUAGE_SEMANTICS.md`;
- `Cargo.toml`;
- `Cargo.lock`;
- `crates/boon_data` for shared exact `ExactNumberSemanticProfileV1`,
  `BITS[N]` operations, canonical collection keys, and lossless
  list-cardinality conversion;
- new `crates/boon_contract` for data-only theorem/bundle and invariant-stamp
  DTOs;
- `crates/boon_parser`;
- `crates/boon_typecheck`;
- new `crates/boon_semantic`;
- new `crates/boon_verify`;
- `crates/boon_ir`, including moving semantics-essential contextual expansion
  into `boon_semantic`;
- `crates/boon_plan` for static persisted-authority activation requirements
  and compatibility/stamp bindings carried by executable plans;
- `crates/boon_compiler`, structured outcomes, and the distributed semantic
  fixed point;
- `crates/boon_plan_executor` for checked arithmetic, exact `List/count`, and
  runtime-enforced list capacity;
- `crates/boon_cli`;
- `crates/boon_editor`;
- `crates/boon_native_playground` compilation worker, language analysis,
  protocol, and dev proof UI;
- `crates/boon_runtime` source-bundle digest, program retention, atomic
  value-plus-stamp persistence, last-writer evidence resolution,
  fresh/restored activation, exact activation-receipt creation/validation, and
  later migration scenarios;
- `crates/boon_app_package` for source, theorem/bundle, profile, accepted
  predecessor-evidence-catalog, and persistence-compatibility hashes;
- `crates/boon_example_manifest`;
- `crates/xtask`, including a dedicated formal report-schema module;
- `examples/manifest.toml`;
- new positive and negative example sources;
- current `flow_operators`, TodoMVC, and migration examples.

This list is architectural scope, not permission to special-case every layer.
Executable runtime, document, renderer, and native GPU crates should require no
`WHERE` operation.

## End State

A Boon developer should be able to read:

```boon
FUNCTION add_one(input) WHERE {
    input >= 0
    input <= 99
} {
    input + 1
    |> WHERE result {
        result >= 1
        result <= 100
    }
}
```

and know, without learning a theorem-prover sublanguage:

- every call proves the two input bounds;
- the implementation proves the two returned bounds;
- the result is still ordinary Boon data;
- there is no hidden per-use runtime assertion; persisted invariant restore
  uses the separately reported host activation gate;
- failed proof stops the new program before execution;
- runtime uncertainty is handled with ordinary Boon values and branches;
- compiler and editor diagnostics explain the smallest failing source-level
  case.

That compact mental model is the design constraint against which every
implementation and future extension must be reviewed.

## Historical Review Record

Date: 2026-07-26

This plan received three independent read-only review tracks:

1. Boon syntax, locality, current APIs, reactive semantics, and
   `OUT`/`PASS`/`PASSED`;
2. formal soundness, obligation completeness, trust, Number/list
   semantics, public theorems, and erasure;
3. current-repository parser, compiler, distributed, runtime, manifest,
   playground, CLI, xtask, and package integration.

The first passes drove the shared `SemanticProgram` boundary, exact
presence/definedness rules, well-founded `HOLD` induction, contextual proof
scope, complete obligation manifests, two-axis evidence status, exact Number
and list semantics, fail-closed parser design, formal-example evidence, and
phased acceptance.

The blocker-only second passes additionally drove:

- lexical fact dropping and per-materialization contextual verification;
- source `ContractId` coverage, including unmaterialized-context rejection;
- separation of authored theorems from verified evidence bundles;
- separation of public contract/API identity from private implementation,
  evidence, and cache identity;
- exact `PASS`/`PASSED` contextual-formal substitution, row observation modes,
  output-evaluated arguments, `OUT`, and compiler-supplied contexts;
- an exhaustive callable dependency manifest and fail-closed enum/side-table
  classification gate spanning every current value, resource, type, flow,
  structural, routing, persistence, and assurance channel;
- field-level schema/disposition exhaustiveness so new ordinary struct fields
  cannot bypass the enum gate;
- mixed symbolic/materialized `ContractId` and child `ConditionId` coverage;
- non-recursive theorem, obligation, manifest, evidence, bundle, proof-report,
  activation requirement, stamp, value, and receipt identities;
- distinct raw obligation evidence, provider verification manifests, exported
  evidence bundles, and cycle-free cache layers;
- a pre-verification obligation/report reservation limit that makes complete
  bounded failure reports implementable;
- restored-authority soundness: static compile requirements, runtime
  value-bound activation receipts, fresh/restored last-writer induction basis,
  receipt retention, visible trust, and measured persistence overhead;
- local-only returned facts for materialization-dependent `OUT`/provider
  results, with symbolic proof required for public export;
- evidence-preserving erasure with proof-derived activation metadata retained
  only in deployment/runtime sidecars;
- explicit semantic/executable crate and id ownership;
- the distributed semantic fixed point before verification;
- an explicit list-cardinality bound for the TodoMVC theorem;
- structured failed-proof compiler outcomes and bounded report schemas;
- canonical `SourceBundleDigestV1`;
- exact contract-refresh and negative-fixture tooling;
- closure of public unchecked lowering paths;
- exact phase ownership for every teaching example and real-source upgrade.

After those revisions, all three reviewers performed a blocker-only reread of
that 2026-07-26 snapshot and reported no remaining concrete blocker. The
2026-07-27 reconciliation replaces its floating-point and public-Boolean
assumptions with the final language-foundation contracts and adds packed-kernel
and hardware sequencing. Reviews were document/source inspection only; no
build, test, runtime, native GPU, commit, or push was performed.
