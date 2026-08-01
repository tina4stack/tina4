# DRY detection: threshold reconciliation and what the design pass really found

Status: **decided 2026-08-01**. Scope is `src/metrics.rs` on
`feature/metrics-rust-pascal`. Everything below was measured on this machine
(macOS 26.5.2, arm64, rustc 1.94.0, tree-sitter 0.26.11) against the working
tree, not inherited from the design document.

Design pass under review:
`/Users/andrevanzuydam/IdeaProjects/tina4-documentation/plan/v3/dry-detector-design.md`.

## The short version

| Item | Design recommends | Decision | Why |
|---|---|---|---|
| Eligibility floor | 40 AST nodes on outermost FUNCTIONS | keep 60 nodes AND 6 lines on ANY subtree | different unit, see below |
| Subsumption pass | delete it | keep it | it is load-bearing for the shipped unit |
| Second re-parse per file | delete it | DELETED | pure waste, zero behaviour change |
| `is_error` before `is_extra` in `fold` | reorder | n/a, no `fold` shipped | the equivalent shipped fix is different, see below |
| `CLOSE = 0x1_0000` not `u16::MAX` | change it | n/a, no CLOSE sentinel shipped | design-doc item only |
| Recursion depth guard | add one | ADDED, once at file level | fixes a real pre-existing crash |
| Python docstrings, Ruby `{}` vs `do end` | handle explicitly | NOT handled, documented instead | changes which clones report |

## Why the two floors are not comparable

The design counts nodes in **one outermost function**. The shipped detector
counts nodes in **every subtree of every file**, then suppresses non-maximal
groups. A 40-node function and a 40-node subtree are not the same population,
so moving 60 to 40 without moving the unit would not import the design's
calibration. It would just lower a threshold that was calibrated for something
else.

Adopting the design's unit is the real proposal, and it is a **contract change**,
not a tuning change. The design measured its own migration cost: `--fail-on warn`
newly fails with +23 offenders on tina4-python, +24 on tina4-php, +13 on
tina4-nodejs, +3 on tina4-ruby, and `--fail-on error` gains 4 / 3 / 0 / 3. Every
existing CI run that uses those flags changes colour. That needs a `Breaking:`
changelog entry, a migration note, and a maintainer decision. It does not belong
inside a parse-health-guard change, so it is not in this one.

The design's own reasoning for the unit change is sound and worth doing later.
Two things follow it for free and are the actual prize, bigger than the digit:

* the quadratic subsumption pass (`group_clones`) becomes unnecessary, because
  outermost-only removes the nesting it exists to collapse;
* it makes the fingerprint cheap enough to carry on `FunctionInfo` instead of a
  separate fragment vector.

Until the unit changes, the subsumption pass stays. It is not dead code today: it
is what stops one duplicated block being reported once per wrapper level, and
`nested_copies_of_one_clone_are_reported_once` fails without it.

## The second re-parse: removed

`fragments_for_source` parsed every file a second time, with the same parser and
the same grammar, purely to collect clone fragments. `analyze_source` already had
the tree. Fragment collection moved inside `analyze_source`, which now returns a
single `FileAnalysis` verdict, and `fragments_for_source` is gone.

Behaviour is identical by construction (same input, same grammar, same tree), and
that was verified rather than assumed: over 1,875 real files, duplication findings
and offender lists are byte-identical before and after. See the drift section.

## The fingerprint collision: NOT in shipped code

The design reports that in its proposed `fold`, `!is_named() || is_extra()` runs
before `is_error()`, that an ERROR node has `is_extra() == true`, and that the
error check is therefore dead code. It measures two different unparseable Python
files folding to the same fingerprint `18dfa007c633f6c1`.

**Both node-flag facts reproduce here. The collision does not, because there is no
`fold` in the shipped tree.**

Confirmed on tree-sitter 0.26.11, dumping a real Python ERROR node:

    kind=ERROR named=true is_error=true missing=false extra=true kind_id=65535

So `is_extra()` really is true on an ERROR node, and `kind_id()` really is 65535,
which really is `u16::MAX`. Anyone implementing the design must put
`is_error`/`is_missing` first and must not use `u16::MAX` as the CLOSE sentinel.
Those corrections stand.

But `58f7f73` ships `collect_fragments`, not `fold`. It hashes `node.kind()`
strings with child hashes, has no `is_extra` guard, no `is_named` guard on the
hash, and no CLOSE sentinel. Measured on the two design fixtures, it produces
**different** hashes:

    "def f(:\n  ???? %%%\n"   -> fedc047392fa4002  (count 10)
    "class ][ oops @@@\n"     -> f21f9b799d6730b0  (count  9)

So the specific defect as described is not present in the shipped code, and
reordering guards that do not exist would fix nothing.

## What IS wrong in the shipped code, measured

The underlying concern is real, and the shipped code has its own version of it:
**it hashes parse errors and will emit them as clone candidates.**

Seven lines of pure Python garbage fold into ONE ERROR node of 69 nodes spanning
7 lines. That clears `MIN_CLONE_NODES` (60) and `MIN_CLONE_LINES` (6) unaided, and
the pre-change `collect_fragments` emitted it. Two files broken the same way were
therefore reported as duplicates of each other on the strength of rubble.

Fixed twice, deliberately:

1. `analyze_source` refuses any file below the parse-health floor, so its
   fragments are never collected at all.
2. `collect_fragments` returns a `parsed_cleanly` flag and never emits a fragment
   from a subtree containing an ERROR or MISSING node. This makes the guarantee
   unconditional instead of contingent on the floor, so it still holds for error
   regions inside a file healthy enough to be reported, and it does not weaken if
   the floor is ever tuned.

Locked by `an_error_region_never_becomes_a_clone_fragment` (which first asserts
the fixture still contains a qualifying ERROR node, so it cannot rot into a test
of nothing) and `two_differently_broken_files_are_never_duplicates_of_each_other`.

## The pre-existing crash, separate from all of the above

`collect_functions`, `count_own_decisions`, `py_halstead`, `generic_halstead` and
`collect_fragments` all recurse once per AST level with no bound. The design names
only the first; there are five.

Reproduced against the **58f7f73 release binary**, so this predates duplication and
predates Rust support:

    $ tina4 metrics --path .        # one 60,000-term Python expression, one term per line
    thread 'main' has overflowed its stack
    fatal runtime error: stack overflow, aborting
    EXIT=134

`looks_minified` does not catch it: the file is 10 bytes per line against a
threshold of 200.

Fixed with ONE iterative depth check in `analyze_source` rather than a depth
parameter threaded through five recursive functions. Fewer lines, one place, and
it cannot itself overflow the stack it protects.

`MAX_AST_DEPTH = 800` is bracketed by measurement, not chosen:

* deepest real file in the corpus is **79** (`tina4/src/agent.rs`), over 1,875
  files; per-repo maxima 50 / 50 / 35 / 56 / 31 / 79;
* first abort is at depth **1800**, in the harshest environment the walks run in,
  a debug build on a 2 MiB cargo-test worker thread. Depth 1700 still completes.

800 is 10x the deepest thing anyone has written and half the worst-case ceiling.

## Python docstrings and Ruby block delimiters: documented, not fixed

The design flags both as measured MISSes. Re-measured against the SHIPPED
implementation, both confirm:

| case | shipped result |
|---|---|
| Python function with vs without a docstring | MISS |
| Ruby `xs.each { }` vs `xs.each do end` | MISS (26 nodes both, different hash) |

A third, which the design does not call out for the shipped code and which is
bigger than either: **comments are hashed**. Adding a comment breaks the match in
all five languages (Python `#`, Rust `//` and `///`, PHP `/** */`, TS jsdoc,
Ruby `#`).

That last one makes the doc wrong. `CLAUDE.md` and the `metrics.rs` section
comment both claimed the engine finds "Type-2 (renamed)" clones. Roy/Cordy Type-II
is verbatim "identical fragments except for variations in identifiers, literals,
types, layout and comments". The engine tolerates identifiers, same-kind literals
and layout, but not comments, so it is **Type-1 plus consistent renaming** and
nothing more.

Both docs now say that, and
`comments_are_hashed_so_this_is_not_full_type_2` pins it in both directions: the
positive half asserts renames, same-kind literal changes and reformatting stay
invisible, the negative half asserts comments do not. Making the hash
comment-blind is a genuine option (skip `is_extra()` nodes), but it changes which
clones get reported, which is the same contract change as the unit swap, and it
has the extra trap that `is_extra()` is also true on ERROR nodes. Discuss it, do
not smuggle it in.

## Drift

Zero. The pre-change binary (built from `58f7f73`) and the post-change binary were
run back to back over the same six trees:

| repo | files | drifted | refused |
|---|---|---|---|
| tina4-python | 408 | 0 | 0 |
| tina4-php | 450 | 0 | 0 |
| tina4-ruby | 357 | 0 | 0 |
| tina4-nodejs | 552 | 0 | 0 |
| tina4-js | 89 | 0 | 0 |
| tina4 (src) | 19 | 0 | 0 |
| **total** | **1,875** | **0** | **0** |

Compared per file on loc, complexity, avg_complexity, functions, maintainability,
halstead_volume, dep_count, coupling_efferent, coupling_afferent, instability and
has_tests. No file appeared, disappeared or moved. The `duplication` array and the
`offenders` array are byte-identical in all six.

The JSON contract change is purely additive: `summary.files_refused`,
top-level `unparsed`, and `parse_health` on each `file_metrics` entry. Nothing was
removed or renamed, and `most_complex_functions` and `dependency_graph` are
byte-identical.
