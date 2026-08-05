# Pascal / Delphi support in the metrics engine — PARKED

Status: **Parked 2026-08-01** by maintainer decision. Not blocked on anything
technical; deprioritised. Everything below is measured, not estimated, so the
next person inherits the numbers instead of re-deriving them.

Scope when parked: the native metrics engine (`src/metrics.rs`, ADR-0002) gained
Rust support and cross-file duplication in the same pass. Both shipped. Pascal
did not, for the reasons measured here.

## Verdict

`tree-sitter-pascal` 0.10.2 (Isopod, MIT, crates.io) **cannot parse the real
tina4delphi corpus**, so `.pas` / `.dpr` / `.dpk` are deliberately NOT claimed by
`Lang::from_path`. The decision is locked by the `pascal_is_not_claimed` test in
`src/metrics.rs`, which fails if anyone wires the extensions up.

Reporting an MI derived from a tree where half the decision points sit inside
ERROR regions would be worse than reporting nothing. A missing metric is
recoverable; a confident wrong one is not.

## The damage measurement

Corpus: `/Users/andrevanzuydam/IdeaProjects/tina4delphi`, 39 files
(`.pas`/`.dpr`/`.dpk`), 40,719 lines.

Methodology: parse each file, then count the DISTINCT source lines covered by
the span of any ERROR node, not descending into an ERROR node's children. A line
inside an ERROR region is a line whose decision points and operators are
invisible to CC and Halstead.

| grammar | lines in an ERROR region | % corpus | clean files |
|---|---|---|---|
| tree-sitter-pascal 0.10.2 | 20,977 / 40,719 | **51.5%** | 22 / 39 |

Worst files: `Tina4Frond.pas` 100% (7,393 lines), `Tina4HTMLRender.pas` 92.8%
(13,060 lines), `Tina4OpenSSL.pas` 72.7%, `uHtmlRender.pas` 100%.

Note the shape of the failure: the three largest files, 54% of the codebase,
are the corrupt ones. A file-count summary ("22 of 39 parse cleanly") badly
understates it.

## Two root causes, each independently fatal

Both isolated with minimal repros rather than inferred from the corpus.

**Cause 1 — Delphi 10.3+ inline loop variables. Dominant.**

```pascal
for var Child in Box.Children do   // ERROR
```

264 occurrences in the corpus: `Tina4HTMLRender.pas` 190, `Tina4Frond.pas` 49,
`Tina4Core.pas` 10. Open upstream as
[Isopod/tree-sitter-pascal issue #15](https://github.com/Isopod/tree-sitter-pascal/issues/15);
maintainer last active ~7 months before this investigation.

**Cause 2 — conditional compilation spanning a syntactic boundary.**

```pascal
function LoadOpenSSL: Boolean;
{$IFNDEF IOS}
var
  I: Integer;
{$ENDIF}
begin
```

The grammar lexes `{$...}` as a `{ }` block comment. That is harmless where both
branches are independently valid in place, and fatal where they are not — a
`var` section between a function header and its `begin`, or an `{$IFDEF}` that
splits a `uses` clause so the first branch's `;` terminates it early. 203
directives in the corpus. This needs a real preprocessor; tree-sitter has none.

## What a fix to cause 1 alone would buy

Diagnostic only — a `sed` strip of the `var` keyword (`for var X in` -> `for X
in`) purely to attribute damage. **This is not a shipping path and must never be
one**: rewriting source before parsing produces numbers that look fine.

| file | before | after var-strip |
|---|---|---|
| Tina4Frond.pas | 100% | 0.2% |
| Tina4HTMLRender.pas | 92.8% | 0.1% |
| Tina4Core.pas | 5.2% | 0.4% |
| Tina4OpenSSL.pas | 72.7% | **72.7%** (pure cause-2 damage) |
| **corpus total** | **51.5%** | **4.4%** |

So cause 1 dominates and is a bounded grammar job; cause 2 is confined to
essentially one file. Fixing cause 1 would very likely be enough to ship behind
the parse-health guard, making cause 2 optional rather than blocking.

## Why we did NOT vendor tree-sitter-delphi13

`Alexl-git/tree-sitter-delphi13` was measured against the same corpus and is
technically excellent:

| grammar | % corpus in an ERROR region | clean files |
|---|---|---|
| tree-sitter-pascal 0.10.2 | 51.5% | 22 / 39 |
| **delphi13 (master)** | **0.0%** | **39 / 39** |

Both root causes fixed; produces real structure rather than a permissive blob
(it correctly rejects malformed input). MIT, ABI 14, vendorable with a
hand-written `Cargo.toml`.

Rejected on supply chain, not capability: single automated-agent commit
identity, zero stars, a 17MB machine-generated `parser.c` plus a hand-written
~20KB `scanner.c` processing untrusted input, and its own scanner corpus tests
are red. Too much unreviewable C to take into a shipping binary.

## If this is ever resumed

The maintainer's preferred route, agreed before parking: **fork 0.10.2 and patch
`grammar.js` ourselves.** Vendoring our own reviewed diff on top of a known-good
MIT base is a categorically different proposition from accepting a large
generated parser from an anonymous account — we can read our own diff.

Order of work:

1. Patch `grammar.js` for inline variables. Cover BOTH forms — `for var X in Y
   do` and `for var X := A to B do` — and check whether inline `var` appears in
   plain statement position anywhere in the corpus too rather than assuming the
   for-loop is the only site. (`var X := 5;` and `var X: Integer := 5;` in a
   statement block already parse fine on 0.10.2; both were verified.)
2. Re-measure against the full corpus after every change. Expect ~4.4%.
3. Only then decide on `{$IFDEF}`, on evidence: quantify how much of the
   residual it accounts for. "The guard refuses these N files" is an acceptable
   outcome and is better than a fragile preprocessor.
4. Upstream the fix as a PR against issue #15 so the fork has a public
   rationale, even if it sits.

## Related work that DID ship

- **Rust support** (`cd4dae8`) — the engine could not previously measure its own
  implementation language.
- **Cross-file duplication / DRY** (`58f7f73`) — did not exist in any form
  before; not a Pascal dependency.
- **Parse-health guard** — the defect this investigation exposed, now SHIPPED.
  The engine emitted confident LOC/CC/MI for any file that failed to parse, in
  every language. Pascal is only where it was caught, because damage was measured
  explicitly instead of trusting the parser.

  `parse_health` and `MIN_PARSE_HEALTH` in `src/metrics.rs` are the shipped
  implementation of the exact measurement this document describes: union the line
  spans of every ERROR node without descending into their children, divide by
  total lines. A file below 0.95 is refused, named in the report, and kept out of
  every average. So if the grammar work in this document is ever resumed, the
  `sed`-strip harness is not needed to re-measure damage: point the built binary
  at the corpus and read `parse_health` per file out of `--json`.

  The floor was calibrated on the corpora that DO parse, over 1,875 files in
  tina4-python, tina4-php, tina4-ruby, tina4-nodejs, tina4-js and this CLI's src:
  1,873 at health exactly 1.000, two at 0.993 and 0.994, none below 0.95. Against
  tina4delphi's 51.5% that is not a gradient, it is a cliff, which is what makes
  a single cross-language floor defensible.

## Reproducing the measurements

The throwaway harness lived in the session scratchpad and is not committed. It
is ~80 lines: walk a directory, parse each file, union the line spans of all
ERROR nodes without descending into their children, and report distinct covered
lines per file plus a corpus total. `parse_health` in `src/metrics.rs` is the
shipped implementation of the same idea and is the natural base to rebuild it
on.
