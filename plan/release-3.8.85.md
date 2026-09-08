# Release Tina4 Client 3.8.85

**Outcome:** Publish the signed Tina4 CLI whose AI-skills refresh survives a CDN
outage instead of dying on it.

## Scope

- [x] `tina4 update` / skills refresh survives a `503` (or other transient CDN
  failure) from the Varnish tier in front of `raw.githubusercontent.com`: it retries
  and falls back, and a genuinely failed download reports the real reason and exits
  non-zero rather than printing a raw exception and calling it success (#22).
- [x] Keep the CLI runtime dependency-free.

## Verification

- [x] `cargo clippy -- -D warnings` clean.
- [x] `cargo test` green; the binary reports `tina4 3.8.85`.
- [x] `cargo build --release` fine.

## Published

- Client tag: `v3.8.85`.
- Release: <https://github.com/tina4stack/tina4/releases/tag/v3.8.85>
- SimplySign produced the signed Windows artifact; checksums regenerated over the
  signed bytes before the draft was published.

Status: Complete
