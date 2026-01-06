Rust port status update (top-level Rust, TS submodule in `pi-mono/`)

Progress summary:
- Rust CLI can run prompts against Anthropic and uses the existing `~/.pi/agent/auth.json` credentials.
- RPC mode wired and covered by Rust tests; handlers for prompt/steer/follow-up/session/model/cycle/compaction/tools/export/branching/messages are in place.
- Rust test harness exists and runs via `rs-test.sh`. TypeScript tests are run via `ts-test.sh`.
- `test.sh` restored to upstream behavior for TS tests; Rust tests are isolated to `rs-test.sh`.
- Clippy fixes applied across agent, coding-agent, ai, and tui modules; compile succeeds with `./clippy.sh`.

Known gaps / next steps:
- Update shell scripts to new repo structure (Rust at repo root, TS in `pi-mono/` submodule).
- Map CLI flags and behaviors to match the TS `pi` CLI exactly (minimize dead code; add as features land).
- Port remaining TS unit tests into Rust (currently stub tests exist; continue implementing to make them pass).
- Validate CLI parity with real prompts for Anthropic (target: `./pi.sh -p "The color of the sky is?"` matches `pi`).
