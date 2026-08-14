# Task: Hydrate FREE-TOKEN for planner and sub-agent settings

**Outcome:** Existing or browser-supplied chat settings with blank Tina4 MCP credentials use the resolved FREE-TOKEN, so planner/debug/coder agents do not fail after upgrading.

## Scope
- [x] Trace planner model selection and persisted/browser settings paths
- [x] Add a failing regression for blank persisted MCP credentials
- [x] Hydrate every Tina4 MCP model slot before agent selection
- [x] Verify supervisor, planner fallback, coder, and direct execution settings
- [ ] Release and sign corrective CLI v3.8.71

## Parity
| Surface | Status |
|---|---|
| Rust supervisor | Bug confirmed |
| Planner/debug sub-agents | Bug confirmed |
| Dev-admin SPA | Sends settings; backend must hydrate |

## Tests (real, positive + negative)
- [x] Blank Tina4 MCP slots receive the resolved token
- [x] Explicit Tina4 MCP credentials remain unchanged
- [x] Non-Tina4 providers remain unchanged
- [x] Full Rust suite (304 executed passed; 6 environment-dependent ignored)

## Bugs
- [x] `.tina4/chat/settings.json` and request settings bypass FREE-TOKEN resolution

## Commits
- bffc5aa  fix(agent): hydrate MCP token for planner settings

## Status: In Progress
