# Rust Port Plan (Pi Coding Agent)

## Goal
Ship a Rust binary that is feature-parity with the current TypeScript `pi` CLI
(`packages/coding-agent`) across interactive, print, and RPC modes while
preserving config formats, session files, and extension behavior.

## Source of Truth
Primary references (JS/TS):
- Entry: `packages/coding-agent/src/main.ts`, `packages/coding-agent/src/cli.ts`
- Core session: `packages/coding-agent/src/core/agent-session.ts`
- Session persistence: `packages/coding-agent/src/core/session-manager.ts`
- Tools: `packages/coding-agent/src/core/tools/*`
- Modes: `packages/coding-agent/src/modes/*`
- Config paths: `packages/coding-agent/src/config.ts`
- Extensions: `packages/coding-agent/src/core/hooks/*`, `packages/coding-agent/src/core/skills.ts`,
  `packages/coding-agent/src/core/custom-tools/*`
- Assets: `packages/coding-agent/src/modes/interactive/theme/*`,
  `packages/coding-agent/src/core/export-html/*`

## Parity Checklist (Complete Feature Surface)

### CLI + Args
- All CLI flags from README/`parseArgs` (provider/model, `--mode`, `--print`, `--session`,
  `--session-dir`, `--continue`, `--resume`, `--no-session`, `--models`, `--tools`,
  `--thinking`, `--hook`, `--no-skills`, `--skills`, `--export`, `--help`, `--version`).
- `@file` inputs + image attachments behavior.
- Exit codes and error messages match current behavior.

### Config + Paths
- Read config from `piConfig` in package.json (name + configDir).
- Respect `ENV_AGENT_DIR` (e.g., `PI_CODING_AGENT_DIR`).
- Same default paths under `~/.pi/agent`:
  - `auth.json`, `models.json`, `settings.json`
  - `themes/`, `commands/`, `tools/`, `sessions/`
  - debug log file (e.g., `pi-debug.log`)
- `SYSTEM.md` discovery (project `.pi/SYSTEM.md` then `~/.pi/agent/SYSTEM.md`).

### Data Model + Session Files
- Preserve JSONL format for sessions, including v1/v2 migrations.
- Tree structure: `id`, `parentId`, leaf handling, branching, labels.
- Compaction summary entries, branch summaries, custom entries, and custom messages.
- Session stats calculations identical to TS.

### Agent Session Semantics
- Message queuing (steer/follow-up), streaming vs non-streaming behavior.
- Auto-compaction rules (threshold + overflow recovery).
- Auto-retry rules and cancellation semantics.
- Bash execution handling, output truncation, and context injection rules.
- Slash command expansion (file-based and hook commands).

### Tools
- Built-in tools: read, write, edit, bash, ls, find, grep, truncate.
- Same input validation and error handling.
- Respect `.gitignore` for file search tools.
- Output formatting and tool result structure consistent with TS.

### Model + Provider Integration
- Model registry and discovery (`models.json`, built-ins).
- API key resolution (env + auth.json, OAuth).
- Provider compatibility flags and per-provider options.
- Thinking level support including `xhigh` clamping.
- Streaming event compatibility with current `pi-ai` semantics.

### Modes
- **Print mode**: text output (final assistant text) and JSON event stream.
- **RPC mode**: stdin/stdout JSON protocol + hook UI requests/responses.
- **Interactive mode**: full TUI parity.

### Interactive TUI
- UI layout, keybindings, editor behavior, autocomplete, and slash command support.
- Theme loading (built-in + custom), live reload.
- Inline images (Kitty/iTerm2), clipboard paste.
- Tool output expansion, thinking toggle, status line, loader behavior.
- Session tree/branch selectors, model/settings selectors, hook UI.
- `/share` via `gh` CLI and `/export` HTML.

### Extensions
- Hooks: lifecycle events + UI context APIs.
- Skills: discovery + loading + warnings.
- Custom tools: load, call, render, session events.

### Assets + Packaging
- Theme JSON files.
- HTML export templates + vendor assets.
- Docs/examples packaging parity (for standalone binary).
- Windows shell path detection and settings override.

## Rust Workspace Layout
Current crate root: `rust/` with `pi` bin + lib.
Planned modules (initial, not final):
- `src/core/*`: session manager, messages, compaction, settings.
- `src/tools/*`: built-in tools.
- `src/modes/*`: print/json, rpc, interactive.
- `src/cli/*`: args parsing and command routing.
- `src/assets/*`: theme + export template handling.

## Porting Milestones
1) **Core Data Model**
   - Port message types and session JSONL format.
   - Implement migrations and tree traversal.
   - Port tests for session manager + compaction (deterministic).
2) **Headless CLI**
   - CLI args + print/json mode.
   - RPC mode protocol implementation.
   - Stub model backend for deterministic tests.
3) **Tooling**
   - read/write/edit/bash + search tools.
   - Attachments + truncation.
4) **Model Integration**
   - Provider clients + streaming event types.
   - Auth/OAuth and models registry.
5) **Interactive TUI**
   - Editor + keybindings + selectors.
   - Themes + images + clipboard.
6) **Extensions**
   - Hooks, skills, custom tools + UI contexts.
7) **Packaging**
   - Standalone binary assets and layout.
   - Cross-platform behavior parity.

## Current Rust Status (Implemented)
- Session manager + compaction logic with ported tests (deterministic only).
- CLI arg parsing parity tests.
- Print mode wired to Anthropic Messages API and OpenAI Responses API (non-streaming).
- Auth reuse via `~/.pi/agent/auth.json` or `PI_CODING_AGENT_DIR` fallback.
- Minimal tool calling loop in print mode with built-in `read`, `write`, `edit`, `bash`.

## Test Plan
### Baseline (TS)
- `bash ts-test.sh` (current TS unit tests).
### Rust Unit Tests
- Mirror session manager tests from `packages/coding-agent/test/session-manager/*`.
- Mirror compaction tests where deterministic.
- Add tool tests for read/write/edit/bash/search.
### Cross-Check (Parity)
- Use fixtures from `packages/coding-agent/test/fixtures/*`.
- Add golden tests for session JSONL outputs.
- Add RPC integration tests once Rust RPC is functional.

## Manual Run Plan
- CLI flags: `--help`, `--version`, `--print`, `--mode json`, `--mode rpc`.
- Sessions: create, resume, branch, tree navigation.
- Tools: read/write/edit/grep/find/ls/bash.
- Attachments: `@file` and images.
- `/export`, `/share`, `/changelog`, `/hotkeys`, `/model`, `/settings`.

## Definition of Done
- Rust binary accepts all CLI flags and matches TS output semantics.
- All unit tests ported and passing in Rust.
- RPC mode interoperable with existing clients.
- Interactive TUI parity for core workflows.
- Session files are compatible both ways (TS <-> Rust).

## Open Decisions
- Final crate breakdown vs single crate.
- Provider scope for first public Rust release.
- TUI backend choice and terminal feature support.
- Packaging strategy and asset bundling layout.
