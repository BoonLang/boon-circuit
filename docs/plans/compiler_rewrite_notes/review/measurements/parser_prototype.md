# Parser prototype figure: re-measurement of BOON_COMPILER_REWRITE_PLAN.md §0 / §4.2

Measured 2026-09-29 23:03-23:11 on branch `compiler-rewrite-plan`, git HEAD `a60a11d6865fcc63bb86847e1d17df79e3e4f12d`, repo root as cwd, i7-9700K, Linux 6.18.7, noisy desktop (load average 1.6-2.7 throughout; see "Load" below).

## Claims under test

> A prototype (standalone, not repo code) parses NovyWave in 2.6 ms (today 65 ms), TodoMVC in 0.64 ms (today 18 ms), counter in 0.021 ms, and all 141 example files in 6.9 ms. (§4.2, lines 259-261; §0 line 36 says "parse NovyWave | 64 ms | 2.6 ms (prototype-measured)"; §6 line 807 uses 2.6 / 0.64 / 0.021 ms as the basis of the ≤4 / ≤1.2 / ≤0.05 ms front-end targets.)

> The prototype accepts every one of the 141 current example files under these rules. (§4.2, line 287)

> `TEXT {` switches to raw mode ... In raw mode, newlines and `--` are text (D18). (§4.2, lines 268-270)

## Verdict in one paragraph

All four timing figures reproduce within 6% on the unmodified prototype (best-of-30 in-process minimum per process, 20 processes): NovyWave unit set min 2.744 ms (plan 2.6), TodoMVC unit set 0.669 ms (plan 0.64), counter 0.020 ms (plan 0.021), all tracked example files 6.995 ms (plan 6.9 for "141"). The figures are best-case minima; the median of every one of 600 iterations is 2.80 / 0.67 / 0.022 / 7.10 ms and the p95 is 2.93 / 0.74 / 0.027 / 7.34 ms, so the shape of the claim holds under either statistic. "141 example files" = the 142 tracked `examples/**/*.bn` minus the one file the prototype rejects (`examples/language_surface/future/where_contracts.bn`, a planned-feature `WHERE` fixture that today's compiler also rejects); the other 141 parse with zero diagnostics. Across all 177 tracked `.bn` files the prototype rejects 3 (the two `WHERE` fixtures and `testdata/typed_passkey_effects.bn`, whose bare `|> HOLD name` form today's parser accepts). Today's parse phase re-measured at 67-71 ms (HEAD) / 65.6 ms (sep28 binary) for NovyWave and 17.6-17.9 ms for TodoMVC, so the "today" column also holds; the prototype is 24-26x faster on the two large fixtures. The important caveat is what the prototype is: a lexer, a recursive-descent/Pratt parser into a flat arena, and per-definition fingerprints, with no name resolution, no value decoding, several constructs parsed-and-dropped, and an incomplete grammar (details in "What the prototype does and does not do").

## Prototype under test

- Source: `docs/plans/compiler_rewrite_notes/drafts/fe_proto.rs`, 610 lines, sha256 `c0edee81e1de55970682168c7e015f8bd029b3b425a6f0ecd733aefee9c6c455`, copied unmodified to the scratchpad (the repo file was not touched).
- Compiler: `rustc 1.97.1 (8bab26f4f 2026-07-14)`, host `x86_64-unknown-linux-gnu`, LLVM 22.1.6 (`raw/proto/rustc_version.txt`).
- Build (0.8 s, 5 dead-code warnings, no errors):

```bash
S=/tmp/claude-1000/-home-martinkavik-repos-boon-circuit/19403ab8-10d9-4589-8161-fdcdd61d4820/scratchpad/measure/proto
cp docs/plans/compiler_rewrite_notes/drafts/fe_proto.rs $S/fe_proto.rs
cd $S && rustc -O --edition 2021 -C codegen-units=1 fe_proto.rs -o fe_proto
```

- A second, patched copy `fe_proto_iter.rs` (diff: `raw/proto/fe_proto_iter.diff`, 45 lines) adds two things and changes nothing in the lexer/parser: (1) in `bench` mode it prints `iter lex_ms=.. all_ms=..` for every internal iteration so the full distribution can be reported, and (2) a `dump` mode that prints per-file token/node/def counts and, with `DUMP_TOKENS=1`, every token and node. Built with the same command (`-o fe_proto_iter`).

### What `fe_proto bench` measures (from reading `main`, lines 540-609)

- Reads all argument files into memory once (file I/O is outside the timer).
- 30 iterations; each iteration creates a fresh `Interner` (allocation outside the timer), then times (a) lexing all files, and separately (b) `parse_unit` (lex + parse into the SoA arena) + `collect_defs` (path hashes and two 64-bit fingerprints per definition) for all files. It prints the **minimum** of the 30 for each (`lex_ms`, `lex+parse+defs_ms`), plus token/node/def/diag totals, MB/s and ns/token. **The plan's 2.6 / 0.64 / 0.021 / 6.9 ms are therefore best-of-30 in-process minima on pre-read bytes, single-threaded.**
- Then a "warm" experiment: inserts one character into a TEXT literal near the middle of the largest file, re-parses that one file 30 times, fingerprints and diffs definitions, and prints the best time and how many definitions changed.
- `fe_proto check <files>` parses each file once and prints up to 3 diagnostics per file plus `files=N with_errors=M`. Lints (indentation, bare-expression) are merged into the diagnostics, so "with_errors=0" means zero diagnostics of any kind.

## File sets

| set | list file | files | lines (`wc -l`) | bytes |
| --- | --- | ---: | ---: | ---: |
| counter | `raw/proto/files_counter.txt` (`examples/counter.bn`) | 1 | 140 | 3,352 |
| todo_run | `raw/proto/files_todo_run.txt` (`examples/todo_mvc_physical/RUN.bn` alone) | 1 | 1,117 | 34,661 |
| todo | `raw/proto/files_todo.txt` = manifest `source_files` of `todo_mvc_physical` | 8 | 3,576 | 103,127 |
| novywave_run | `raw/proto/files_novywave_run.txt` (`examples/novywave/RUN.bn` alone) | 1 | 4,949 | 208,274 |
| novywave | `raw/proto/files_novywave.txt` = manifest `source_files` of `novywave` | 8 | 11,926 | 433,371 |
| examples | `raw/proto/files_examples.txt` = `git ls-files 'examples/*.bn'` | 142 | 30,122 | 1,082,320 |
| all_tracked | `raw/proto/files_all_tracked.txt` = `git ls-files '*.bn'` | 177 | 31,056 | 1,108,627 |

The 3,576 and 11,926 line figures match `compiler_input_source_lines` in `budgets/compiler.toml` exactly, confirming that the plan's fixture sizes are the whole manifest unit sets (8 files each), not the RUN.bn files alone.

## Commands

```bash
# raw file lists
git ls-files '*.bn' | sort > raw/proto/files_all_tracked.txt          # 177
git ls-files 'examples/*.bn' | sort > raw/proto/files_examples.txt    # 142
# (novywave / todo lists typed from examples/manifest.toml source_files; see files_*.txt)

# acceptance
$S/fe_proto check $(cat raw/proto/files_<set>.txt)          # -> raw/proto/check_<set>.txt

# timing: 20 processes per set, full stdout kept per run (tools/proto_run.sh)
RUNS=20 bash tools/proto_run.sh orig $S/fe_proto          # -> raw/proto/bench_orig_<set>.txt
RUNS=20 bash tools/proto_run.sh iter $S/fe_proto_iter     # -> raw/proto/bench_iter_<set>.txt
python3 tools/proto_stats.py                              # -> raw/proto/summary.json, raw/proto/stats_output.md

# per-file counts
$S/fe_proto_iter dump $(cat raw/proto/files_<set>.txt)    # -> raw/proto/dump_counts_<set>.txt

# today's parser (fresh-process mode allows exactly one sample per process; run 3x each)
./target/release/boon_cli compiler-sample examples/counter.bn                --intent diagnostics --mode fresh-process --samples 1
./target/release/boon_cli compiler-sample examples/todo_mvc_physical/RUN.bn  --intent diagnostics --mode fresh-process --samples 1
./target/release/boon_cli compiler-sample examples/novywave/RUN.bn           --intent diagnostics --mode fresh-process --samples 1
#   -> raw/proto/today_sample_<id>_<n>.json ; same once with the sep28 binary -> raw/proto/sep28_sample_<id>_1.json
```

Percentiles: linear interpolation on the sorted sample (numpy's default), computed by `tools/proto_stats.py`.

## Load

- `uptime` before the `orig` series: `23:09:50 up 1 day, 10:24, 1 user, load average: 1.68, 1.77, 1.66`
- after `orig` / before `iter`: `23:10:13 ... load average: 1.90, 1.82, 1.68`
- after `iter`: `23:10:36 ... load average: 1.89, 1.82, 1.68`
- before/after the `compiler-sample` runs: `23:07:28 ... 2.65, 1.96, 1.70` / `23:07:32 ... 2.52, 1.94, 1.69`
- session start: `23:03:09 ... 2.36, 1.81, 1.60`

## Results

### A. Unmodified prototype, `bench` mode: per-process best-of-30 `lex+parse+defs_ms`, 20 processes per set

This is the statistic the plan reports (one process, best of 30). Counts are totals over the set.

| set | lines | tokens | nodes | defs | diags | min ms | median ms | p95 ms | max ms | lines/s (median) | MB/s (median) | plan |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| counter.bn | 140 | 552 | 272 | 22 | 0 | 0.020 | 0.021 | 0.021 | 0.021 | 6.7 M | 160 | 0.021 |
| todo_mvc_physical/RUN.bn alone | 1,117 | 4,529 | 2,377 | 97 | 0 | 0.245 | 0.246 | 0.248 | 0.251 | 4.5 M | 141 | - |
| TodoMVC unit set (8 files) | 3,576 | 13,713 | 7,641 | 175 | 0 | 0.669 | 0.672 | 0.676 | 0.677 | 5.3 M | 154 | 0.64 |
| novywave/RUN.bn alone | 4,949 | 22,733 | 12,420 | 841 | 0 | 1.265 | 1.276 | 1.284 | 1.289 | 3.9 M | 163 | - |
| NovyWave unit set (8 files) | 11,926 | 56,855 | 29,737 | 1,301 | 0 | 2.744 | 2.766 | 2.782 | 2.783 | 4.3 M | 157 | 2.6 |
| all tracked examples/**/*.bn (142) | 30,122 | 147,385 | 75,368 | 3,380 | 3 | 6.995 | 7.069 | 7.233 | 7.257 | 4.3 M | 153 | 6.9 (141) |
| all tracked *.bn (177) | 31,056 | 151,480 | 77,489 | 3,612 | 12 | 7.371 | 7.402 | 7.511 | 7.583 | 4.2 M | 150 | - |

Lexer-only best-of-30 (`lex_ms`), min / median / p95 over 20 processes: counter 0.013 / 0.013 / 0.013; TodoMVC set 0.389 / 0.391 / 0.392; NovyWave set 1.544 / 1.556 / 1.573; 142 examples 4.224 / 4.243 / 4.327; 177 files 4.378 / 4.414 / 4.468 ms. The lexer is 56-60% of lex+parse+defs on every set.

Warm re-parse of the largest file in the set after a one-character TEXT edit (best of 30, min / median / p95 over 20 processes): counter 0.020 / 0.020 / 0.020; RUN.bn of TodoMVC 0.236 / 0.237 / 0.240; novywave/RUN.bn 1.272 / 1.277 / 1.281 ms (`changed_defs=1 (literal-only 1)` in every run). Not a plan claim; recorded because the bench prints it.

### B. Patched copy: every iteration (20 processes x 30 = 600 samples) of `lex+parse+defs`

| set | n | min ms | median ms | p95 ms | max ms | lines/s (median) | lines/s (p95) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| counter.bn | 600 | 0.020 | 0.022 | 0.027 | 0.039 | 6,481,481 | 5,145,167 |
| todo_mvc_physical/RUN.bn alone | 600 | 0.218 | 0.231 | 0.264 | 0.294 | 4,841,786 | 4,237,320 |
| TodoMVC unit set (8 files) | 600 | 0.650 | 0.669 | 0.738 | 0.848 | 5,348,889 | 4,848,058 |
| novywave/RUN.bn alone | 600 | 1.208 | 1.261 | 1.319 | 1.485 | 3,923,574 | 3,752,284 |
| NovyWave unit set (8 files) | 600 | 2.647 | 2.795 | 2.930 | 3.353 | 4,267,134 | 4,070,154 |
| all tracked examples/**/*.bn (142) | 600 | 6.843 | 7.101 | 7.344 | 7.712 | 4,241,997 | 4,101,708 |
| all tracked *.bn (177) | 600 | 7.200 | 7.389 | 7.683 | 8.036 | 4,203,204 | 4,042,082 |

Lexer only, every iteration (min / median / p95 / max): counter 0.012 / 0.013 / 0.018 / 0.035; TodoMVC set 0.373 / 0.384 / 0.446 / 0.548; NovyWave set 1.503 / 1.588 / 1.684 / 1.951; 142 examples 4.007 / 4.180 / 4.348 / 4.633; 177 files 4.261 / 4.341 / 4.556 / 4.897 ms.

Reading: the per-iteration p95 is 6-25% above the best-of-30 minimum. The plan's figures are minima, but even the p95 of every iteration stays under the §6 front-end targets (≤4 / ≤1.2 / ≤0.05 ms) with margin: NovyWave p95 2.93 ms, TodoMVC 0.74 ms, counter 0.027 ms. Throughput is 4.0-4.3 M lines/s (150-163 MB/s) on the large sets, about 50 ns per token.

### C. Today's parser (`compiler-sample --intent diagnostics --mode fresh-process`), `phase.parse_ms`

| fixture (manifest source, whole unit set) | HEAD run 1 / 2 / 3 (ms) | sep28 binary, 1 run (ms) | plan says | prototype (A, median) | ratio HEAD median : prototype |
| --- | --- | ---: | ---: | ---: | ---: |
| counter (1 unit) | 3.525 / 3.553 / 3.694 | 4.074 | 7 ms total diag | 0.021 | 169x |
| todo_mvc_physical (8 units) | 17.699 / 17.729 / 17.631 | 17.887 | 18 | 0.672 | 26x |
| novywave (8 units) | 67.519 / 71.127 / 67.364 | 65.585 | 64 / 65 | 2.766 | 24x |

Today's `work.parse` counters for the same runs (identical across runs): counter `token_inspections=11,693 statement_visits=195 expression_visits=650`, `parsed_expressions=130`; todo `304,851 / 11,158 / 27,853`, `parsed_expressions=4,858`; novywave `1,172,743 / 30,663 / 97,952`, `parsed_expressions=17,721`.

### D. Node counts, prototype vs today (not like-for-like)

| fixture | prototype tokens | prototype arena nodes | today `parsed_expressions` | today `expression_visits` | today `statement_visits` |
| --- | ---: | ---: | ---: | ---: | ---: |
| counter | 552 | 272 | 130 | 650 | 195 |
| TodoMVC set | 13,713 | 7,641 | 4,858 | 27,853 | 11,158 |
| NovyWave set | 56,855 | 29,737 | 17,721 | 97,952 | 30,663 |

The prototype's node count includes every arena entry (fields, args, arms, interpolations, identifiers, paths, errors), while today's `parsed_expressions` is the checked-expression count and `expression_visits` counts parser visits, so the columns are different units; the ratio prototype-nodes : today-expressions is a steady 1.6-2.1 across fixtures, which is what one expects from the granularity difference, not evidence that either parser misses work. Today's `token_inspections` is 20.6x the prototype's token count on NovyWave and 22x on TodoMVC, consistent with the plan's description of a re-scanning line-merging parser, but it is an inspection counter, not a token count. Per-file prototype counts are in `raw/proto/dump_counts_<set>.txt` (e.g. `novywave/View/NovyView.bn`: 29,023 tokens, 14,488 nodes, 379 defs; `novywave/RUN.bn`: 22,733 tokens, 12,420 nodes, 841 defs).

## Acceptance ("accepts every one of the 141 current example files")

`raw/proto/check_examples.txt`, `check_all_tracked.txt`, `check_novywave.txt`, `check_todo.txt`, `check_counter.txt`.

- `git ls-files 'examples/*.bn'` = **142** files (all on disk are tracked; no example `.bn` was added, deleted or renamed since 2026-07-29, so the count was 142 when the plan was written too). The prototype reports `files=142 with_errors=1`: the rejected file is `examples/language_surface/future/where_contracts.bn` (`FUNCTION nonnegative(value) WHERE { ... } { ... }` -> "expected `{`", "expected `,`, newline or closing bracket", "unclosed bracket"). **So "141" is 142 minus this one file**; the plan's sentence is accurate for the 141 that remain, and the rejected file is a "future" fixture that today's compiler also rejects (`boon_cli check`: "`WHERE` belongs to planned language feature `where_contracts` and is rejected until that feature is implemented at line 1, column 29", `raw/proto/today_check_where_contracts.txt`). The plan does not say which file it excluded or why; it should.
- The 177 tracked `.bn` files (examples + `testdata/` + crate fixtures): `files=177 with_errors=3`. The third rejection is `testdata/typed_passkey_effects.bn` lines 7-10 (`workspace_id: TEXT { workspace-1 } |> HOLD workspace_id` etc.): the prototype's `HOLD` requires `HOLD name { ... }` and reports "expected `{`" on the next line. Today's parser accepts the bare `|> HOLD name` form (its `boon_cli check` fails later with "client programs must expose one retained document or scene root", `raw/proto/today_check_typed_passkey_effects.txt`). The second `WHERE` rejection is `testdata/phase0/fixtures/future_where.bn`.
- All 8 NovyWave units, all 8 TodoMVC units and counter.bn: zero diagnostics.
- The 2 tracked `BUILD.bn` files (multi-line TEXT with an interpolation and a `-- Generated from {icons_directory}` line inside the TEXT) and the 2 `Generated/Assets.bn` files (multi-line TEXT holding a data-URI SVG) are among the 141 accepted.
- No tracked `.bn` file contains a tab, so the missing L4(e) tab rejection (below) does not affect the count.

## D18: multi-line TEXT and `--` inside TEXT

Probes are in `raw/proto/probes/` (13 prototype-only snippets `p01..p13` plus 6 full programs derived from `examples/hello_world.bn` for today's compiler); the prototype's token/node dumps and diagnostics for all of them are in `raw/proto/probes_output.txt`; today's outputs are `raw/proto/probes_today_*`.

How the prototype lexes TEXT (lines 103-131): after the keyword `TEXT` and optional spaces, a `{` switches to raw mode. In raw mode bytes are copied into `TextChunk` tokens; a newline only records a line start; `{` ends the chunk and starts an interpolation that runs to the next `}` **or newline**, whose trimmed contents are interned as one symbol (`{PASSED.x}`, `{ store.value }` -> symbol `store.value`); the first `}` outside an interpolation closes the literal. There is no escape for `{` or `}`, no nesting (`depth` is written but never read), and `--` is never special inside raw mode. The parser makes one `Text` node per literal (`a` = number of parts) plus one `Interp` node per interpolation; chunks are not nodes and the literal's value is never computed (no dedent/trim rule exists).

| probe | source | prototype | today (HEAD `boon_cli check`/`dump-plan`) |
| --- | --- | --- | --- |
| p01 multi-line TEXT | `a: TEXT {⏎ line one⏎ line two⏎}` | accepted; one `TextChunk` `"\n    line one\n    line two\n"` | (top-level field form) accepted; value `"first line\n    second line"` |
| p02 `--` inside TEXT | `a: TEXT { keep -- this is text }` and a multi-line TEXT whose 2nd line is `-- also text` | accepted; chunks `" keep -- this is text "` and `"\n    -- also text\n    second\n"` (`--` is text, per D18) | single-line case **fails**: `--` is lexed as a comment, swallows the `}`, "unbalanced `)` at line 29, column 17" (`probes_today_check.txt`); multi-line case (top-level or `FUNCTION { [ field: TEXT {` shape) accepted with value `"-- also text\n    second line"` |
| p04 interpolations | `TEXT { hi {name} and {PASSED.x} and { store.value } }` | accepted; 3 `Interp` nodes, symbols `name`, `PASSED.x`, `store.value` | not probed |
| p03 literal `}` | `a: TEXT { x } y }` | **rejected** (first `}` closes the literal; ` y }` then errors) - no escape exists | not probed |
| p05 `{` then EOF | `a: TEXT { a { b }⏎c: 1` | rejected: "unterminated TEXT" (the `{ b }` became an interpolation and the literal never closes) | not probed |
| p09 interpolation across a newline | `TEXT { hi {name⏎} }` | rejected (interpolation stops at the newline) | not probed |
| p11 `--` after the closing brace | `a: TEXT { x } -- comment` | accepted; comment counted (`comments=1`) | not probed |
| p12 unterminated | `a: TEXT { unterminated⏎b: 2` | rejected: "unterminated TEXT" at the opening brace | not probed |
| p13 UTF-8 in TEXT, `--` inside a `"..."` string with an escaped quote | | accepted | not probed |
| p06 tabs as indentation | `a: [⏎<TAB>x: 1⏎<TAB>y: 2⏎]` | **accepted** (a tab counts as one indent column; L4(e) "tabs are rejected" is not implemented) | not probed |
| p07 `value: 1⏎ 2` | | rejected: "inconsistent element indentation", "bare expression in record" (matches §4.2's list of new errors) | not probed |
| p08 `a: x |> f() + 1` | | **accepted**, parsed as `Binary(PipeCall(x, f), 1)`; §4.2 lists it among "silent reinterpretations [that] become errors" but the prototype produces no diagnostic | not probed |
| p10 bare `a: 1 |> HOLD count` | | rejected: "expected `{`" | today's parser accepts (see typed_passkey_effects above) |

Two details about today's compiler that bound the comparison: it rejects a multi-line TEXT used directly as a call argument regardless of `--` ("a source unit cannot begin with an indented statement ... at line 10, column 9", `probes_today_multiline_plain_check.txt`), so the multi-line comparison was done in the two positions today accepts (top-level field, and the `FUNCTION { [ field: TEXT {` shape used by `Generated/Assets.bn`), where it keeps `--` lines as text and strips the first line's indentation while keeping later lines' indentation. The prototype keeps the raw bytes and defines no value.

## What the prototype does and does not do (from reading `fe_proto.rs`)

Does:
- One-pass byte lexer with a raw TEXT mode, comment skipping, interning of identifiers/tags/interpolation paths, `NL` flag and indent column per token; numbers with `.` fraction and glued radix suffix; `"..."` strings with backslash escapes (not unescaped); unknown bytes become `Err` tokens.
- Recursive-descent/Pratt parser into a struct-of-arrays arena (`kind, a, b, start, end, extra`) covering fields, functions, spreads, records, tagged records, calls with named/positional/`PASS:` args, pipes (`WHEN`/`WHILE` arms with tag patterns and `__` wildcard, `THEN`, `HOLD name {..}`, `DRAINING`, `LATEST`, `FLUSH`, `SKIP`, calls, `.field`), `LIST`/`SET`/`LATEST`/`MAP`/`BYTES`/`BITS`/`BLOCK`/`SOURCE`/`SKIP`/`FLUSH`/`PASSED`/`DRAIN`, binary operators with D11 precedence, unary minus, field access.
- Layout rules L1-L3 and L4(a)(b for `name:` only)(c)(d) as diagnostics plus "comparison operators do not chain"; error recovery to the next separator/closer, so it never bails out early and always parses to EOF.
- Per-definition path hashes and shape/literal fingerprints (`collect_defs`), used only by the warm experiment.

Does not:
- Resolve names, decode numbers, unescape strings, compute a TEXT value (no dedent rule), or validate anything semantic.
- Keep everything it parses: the `LIST[T]`/`BYTES[..]` bracket argument, the `HOLD` state name and a function's parameter list are parsed and dropped (the `Function` node stores only name and body), TEXT chunks are counted but not stored as nodes, `MAP` entries reuse the `=>` arm parser, arm/param/element counts are discarded in several places.
- Accept the bare `|> HOLD name` form, reject tabs (L4e), check indentation of a value after `=>` (L4b), escape `{`/`}` in TEXT, or allow an interpolation to span a line.
- Report error messages with any detail beyond a static string and a byte offset (the `check` mode converts it to line:col).
- Implement the "previous token" half of the header comment's layout rule explicitly (`open_end`/`newline_sep` are dead code); continuation after `( [ { : , |> =>` happens implicitly because the sub-parsers do not look at the newline flag before their first token.

So the measured time is for a lexer, an arena-building parser and fingerprints of the accepted grammar; a production front end adds value decoding, richer diagnostics, the missing forms and whatever the real `boonc_syntax` keeps that this arena drops. The margin to the §6 targets (≤4 / ≤1.2 / ≤0.05 ms versus 2.9 / 0.74 / 0.027 ms at every-iteration p95) is 1.4-1.9x, which is plausible headroom for those additions but not generous.

## Raw files

`raw/proto/`: `files_*.txt` (inputs), `check_*.txt` (acceptance), `bench_orig_*.txt` and `bench_iter_*.txt` (20 full process outputs each, with the exact command on each `### run` header), `summary.json` and `stats_output.md` (from `tools/proto_stats.py`), `dump_counts_*.txt` (per-file token/node/def counts), `today_sample_*_{1,2,3}.json` and `sep28_sample_*_1.json` (today's parser), `today_check_*.txt`, `probes/` and `probes_*.txt` (D18 probes), `fe_proto_iter.diff`, `rustc_version.txt`, `uptime_*.txt`.
