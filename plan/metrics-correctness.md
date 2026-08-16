# Task: Make native metrics reliable across languages

**Outcome:** `tina4 metrics` scores production source consistently across supported languages, accepts repeatable exclusions, reports an honest test-reference signal, and detects comment-only Type-2 clones.

## Scope

- [x] Audit the released `v3.8.75` engine against all framework corpora.
- [x] Add repeatable `--exclude GLOB` with production-source defaults.
- [x] Rename `has_tests` to `has_referencing_test` in JSON and dev-admin consumption.
- [x] Parse test imports, dynamic imports, and referenced exported declarations.
- [x] Ignore namespace-only references that create Ruby false positives.
- [x] Normalize nested callable scopes across Python, PHP, Ruby, TypeScript/JavaScript, and Rust.
- [x] Ignore comments in Type-2 duplicate fingerprints.
- [x] Update the Feature 121 ADR, contract, audit, and user documentation.

## Parity

| Rule | Python | PHP | Ruby | Node.js/TS | Rust |
| --- | --- | --- | --- | --- | --- |
| Source exclusions | ✅ | ✅ | ✅ | ✅ | ✅ |
| Test-reference accuracy | ✅ | ✅ | ✅ | ✅ | ✅ |
| Nested callable scope | ✅ | ✅ | ✅ | ✅ | ✅ |
| Comment-insensitive Type-2 | ✅ | ✅ | ✅ | ✅ | ✅ |

## Tests

- [x] Positive and negative repeatable exclusion cases.
- [x] Default exclusions cover declarations and conventional test/spec files.
- [x] Single-line, multiline, aliased, and dynamic import references.
- [x] Generic test filename referencing a declared source symbol.
- [x] Shared namespace alone does not mark a source file referenced.
- [x] Equivalent nested callables produce equivalent function boundaries and decision allocation.
- [x] Comment-only differences still produce one duplicate group; structurally different code does not.
- [x] Existing JSON, `--top`, severity gates, and parse-health behavior remain green.
- [x] Full Rust suite and production binary clippy pass at HEAD. Repository-wide all-target clippy retains one unrelated `setup.rs` layout warning.
- [x] Rebuilt client completes every framework corpus with zero refused files.

## Bugs

- [x] METRICS-SCAN-SCOPE: repeatable exclusions and safe production defaults implemented.
- [x] METRICS-TEST-FALSE-NEGATIVE: parsed imports, dynamic imports, and public symbols implemented.
- [x] METRICS-TEST-FALSE-POSITIVE: shared Ruby namespace evidence rejected.
- [x] METRICS-SCOPE-PARITY: callable ownership aligned across five languages.
- [x] METRICS-DRY-TYPE2: comments removed from fingerprints.
- [x] METRICS-PARITY-GHOST: retired Python adapter removed as a false external oracle.

## Commits

- Pending.

## Status: Local implementation and verification complete; Linux lab verification pending
