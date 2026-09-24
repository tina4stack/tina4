# Inbound dependency licence policy

This policy is submitted with the CLI 3.8.90 release pull request for maintainer
review. It defines the machine-enforced dependency choices for this repository;
it is not a claim of legal certification or OpenChain conformance.

First-party Tina4 source is MPL-2.0, with a separate commercial option offered by
Code Infinity as described in COMMERCIAL-LICENSE.md. Third-party code retains
its own licence and notices.

## Accepted dependency choices

The `deny.toml` allowlist accepts MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause,
ISC, Zlib, Unlicense, CC0-1.0, BSL-1.0, Unicode-3.0, CDLA-Permissive-2.0 and
MPL-2.0. For an OR expression, at least one permitted choice must exist; for an
AND expression, every required licence must be permitted. No GPL/LGPL choice
is approved by this policy merely because a crate also offers a permitted OR
alternative. Any new licence or exception requires an explicit reviewed policy
change; missing/unrecognized declarations fail the release inventory check.

## Evidence and obligations

PR CI and release audit run `cargo deny check licenses` against the locked graph.
The SBOM records full upstream declarations without rewriting them into the
selected option. Third-party notices accompany release binaries, Debian packages
and the CLI container. Existing notices and copyright attributions are preserved.
MPL source-disclosure obligations and any commercial agreement remain applicable.

The release inventory's `approvalStatus: not-assessed` is deliberately narrower:
it does not pretend to be an individual legal opinion on each component. The
separate cargo-deny result is the auditable repository-policy gate.
