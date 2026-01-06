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
- Session migration now matches TS v3 (hookMessage -> custom) and writes version 3 headers.
- CLI arg parsing parity tests.
- Print mode wired to Anthropic Messages API and OpenAI Responses API.
- Auth reuse via `~/.pi/agent/auth.json` or `PI_CODING_AGENT_DIR` fallback.
- **OAuth support for Anthropic** - Full parity with TS OAuth implementation:
  - Reads OAuth credentials from `auth.json` (`type: "oauth"` with access/refresh/expires).
  - Sends required headers: `anthropic-beta: oauth-2025-04-20,fine-grained-tool-streaming-2025-05-14,interleaved-thinking-2025-05-14`.
  - System prompt format matches TS: two-element array with Claude Code identification as first element, both with `cache_control: {type: "ephemeral"}`.
- Print mode uses AgentSession tool loop with built-in `read`, `write`, `edit`, `bash`, `grep`, `find`, `ls`.
- Default tool allowlist now matches TS (`read`, `bash`, `edit`, `write`) with `grep`/`find`/`ls` opt-in; extension tools are included when `--tools` is not specified.
- Grep now supports directory search with regex/literal matching plus truncation notices for grep/find/ls output.
- Print mode now uses `AgentSession` with session persistence (`--continue`, `--session`, `--session-dir`, `--no-session`).
- Session default directory respects `PI_CODING_AGENT_DIR` for `sessions/`.
- Bash tool now truncates tail output with temp file preservation when output exceeds limits.
- Read tool now detects PNG/JPEG/GIF/WebP via file signatures.
- System prompt builder now mirrors TS defaults (tools/guidelines, project context files, skills; `--skills`/`--no-skills`).
- Prompt templates from `~/.pi/agent/prompts` and `.pi/prompts` are loaded and expanded for `/command` inputs.
- App config now reads `piConfig` from `package.json` when available (app name/config dir + env var), and project paths respect the configured dir name.
- HTML export now uses the TS template assets with the default dark theme and supports CLI `--export` + RPC `export_html`.
- CLI `--thinking` sets the initial thinking level and persists it to the session.
- AgentSession now clamps thinking level to model capabilities (reasoning + xhigh) and persists default thinking level.
- Settings manager now loads `settings.json` (global + project override) for compaction/retry/theme defaults and persists global updates.
- RPC prompt streamingBehavior now matches TS (only queues when streaming; images are ignored during queueing).
- RPC mode now supports `openai-responses` models alongside `anthropic-messages`.
- Interactive mode now uses a basic TUI (chat history + editor) over raw terminal input (full parity pending).
- Interactive mode now renders assistant tool calls/results, thinking blocks, and bash execution messages instead of text-only output.
- CLI `--resume` now lists available sessions and prompts for a selection in the line-based UI.
- CLI now parses `--extension`/`-e` and persists extension paths to settings; JS extensions can now run compaction hooks via the Node host (TS support pending).
- Extension discovery now scans global/project directories plus configured paths and hands JS/TS extensions to the host (TS uses `jiti` when available).
- CLI now parses extension-defined flags and passes flag values into the JS extension host (`getFlag` supported).
- Added a Rust-side extension runner for JS extension metadata (tools/commands/flags/shortcuts/renderers), shortcut conflict warnings, context event emit, and error listeners.
- Extension tool_call/tool_result hooks now wrap built-in tools, allowing extensions to block or override tool outputs.
- Extension-registered tools now expose parameter schemas, are included in API tool specs, and execute via the JS extension host.
- RPC mode now forwards extension UI requests/responses (select/confirm/input/editor + notify/status/widget/title/editor text) through the JS extension host.
- Interactive mode now supports `/export` and `/compact` (with custom instructions) in the basic TUI.
- RPC `compact` now accepts custom instructions like the TS implementation.
- Theme JSON loading is available for interactive mode (built-in + custom theme directories), with editor border styling wired to theme colors.
- Interactive mode now supports `/share` via GitHub gists (`gh` CLI) with shareable preview URLs.
- Interactive mode now supports `/model`, `/settings`, `/changelog`, and `/hotkeys` in the basic TUI (line-based flow).
- CLI refactor: extracted RPC handlers, API wrappers, and mode runners into dedicated modules; main.rs is now slim.
- Streaming now uses SSE parsing for Anthropic/OpenAI Responses and emits incremental agent events.
- **Usage statistics extraction** - Full parity with TS for token counting and cost calculation:
  - Anthropic streams: Extracts `input_tokens`, `output_tokens`, `cache_read_input_tokens`, `cache_creation_input_tokens` from `message_start` and `message_delta` events.
  - OpenAI Responses streams: Extracts usage from `response.completed` event including `cached_tokens` from `input_tokens_details`.
  - Cost calculation: Computes costs using model pricing (per-million rates) and updates `usage.cost` with input/output/cacheRead/cacheWrite/total.
- **OpenAI Codex provider parity** - Full streaming support for Codex API (`openai-codex-responses`):
  - Constants module with Codex-specific headers and URLs.
  - Request transformer: model normalization, reasoning effort/summary config, text verbosity, encrypted content handling, prompt cache keys.
  - Response handler: Codex-specific error parsing (rate limits, auth errors, SSE parsing).
  - Prompts module: Codex instructions fetching from GitHub with ETag caching, pi-codex-bridge injection.
  - Stream module: JWT decoding for account ID, SSE parsing, event emission for reasoning/text/tool calls.
  - Integration with session.rs for `openai-codex-responses` API type.
  - Auth module extended with `resolve_openai_codex_credentials` for separate Codex OAuth tokens.
  - **Tests ported from TS**: `tests/openai_codex_test.rs` covers request transformation, model normalization, error parsing, include handling, and SSE parsing (matching `openai-codex.test.ts`, `openai-codex-include.test.ts`, `openai-codex-stream.test.ts`).
- **Inline image support (Kitty/iTerm2 protocols)** - Full parity with TS terminal-image.ts:
  - `terminal_image.rs`: Terminal capability detection (Kitty, Ghostty, WezTerm, iTerm2, VSCode, Alacritty).
  - Kitty graphics protocol: Single chunk and chunked (4096-byte) encoding with columns/rows parameters.
  - iTerm2 inline images protocol: OSC 1337 encoding with width/height/name/aspect ratio options.
  - Image dimension parsing: PNG (IHDR chunk), JPEG (SOF markers), GIF (header), WebP (VP8/VP8L/VP8X).
  - Row calculation: Scales image to target width cells with configurable cell dimensions.
  - Image component: TUI component with fallback text for unsupported terminals.
  - **Tests ported from TS**: `tests/tui_terminal_image_test.rs` covers Kitty/iTerm2 encoding, dimension parsing for all formats, row calculation, and fallback text (matching `image-test.ts`).
  - Interactive mode wired to render images inline when `show-images` setting is true and terminal supports images.
- **Interactive TUI keybindings system**:
  - `tui/keys.rs`: Full keyboard input handling for legacy terminal sequences and Kitty keyboard protocol.
  - Support for modifier keys (ctrl, shift, alt, and combinations).
  - Arrow keys, functional keys (Home/End/Delete/Insert/PageUp/PageDown).
  - `matches_key()` for key matching, `parse_key()` for key identification.
  - Global Kitty protocol state tracking for mode-aware sequence handling.
  - Unit tests covering legacy sequences, Kitty CSI u format, and modifier combinations.
- **Expandable text component for tool outputs**:
  - `tui/components/expandable.rs`: Expandable trait and ExpandableText component.
  - Collapse/expand support with configurable preview lines.
  - ToolPreviewConfig for per-tool preview line limits.
  - Unit tests for truncation, expansion, and rendering.
- **Session tree selector component**:
  - `tui/components/tree_selector.rs`: Full session tree navigation with ASCII art visualization.
  - Tree flattening with active path tracking and gutter rendering (├─, └─, │ connectors).
  - Filter modes (default, no-tools, user-only, labeled-only, all) with cycling via Ctrl+O.
  - Search/filtering by text with real-time updates.
  - Keyboard navigation (up/down, page up/down with left/right).
  - Label editing support (press 'l' to edit).
  - TreeSelectorComponent wrapper with header and search UI.

## Remaining Gaps (Next)
- Interactive TUI parity: session selector (for resuming sessions), theme reload with file watcher, input component improvements.
- TS extensions support in the JS host (jiti-based loading) + parity tests.

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
