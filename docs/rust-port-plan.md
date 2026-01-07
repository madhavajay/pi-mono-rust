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
- **Session selector component for resuming sessions**:
  - `tui/components/session_selector.rs`: TUI-based session picker for `--resume` flag.
  - Multi-line session display (first message + metadata with relative timestamps).
  - Search filtering for session list.
  - Keyboard navigation (up/down arrows, enter to select, escape to cancel).
  - Scroll indicator for long session lists.
  - Integrated into `cli/runtime.rs::select_resume_session()` with fallback to line-based UI.
  - Unit tests covering filtering, navigation, rendering, and component structure.
- **Theme file watcher** - Full parity with TS theme.ts:
  - `start_theme_watcher()` / `stop_theme_watcher()` for custom theme file monitoring.
  - Only watches custom themes (not built-in dark/light).
  - Debounced file change handling (100ms) to avoid rapid reloads during editing.
  - Falls back to dark theme if watched file is deleted.
  - `on_theme_change()` callback registration for UI invalidation.
  - `init_theme()` and `set_theme()` with optional watcher enable.
  - Uses `notify` crate for cross-platform file watching.
- **Editor bracketed paste mode and control character filtering**:
  - Bracketed paste mode: Handles `\x1b[200~` and `\x1b[201~` markers for proper paste handling.
  - Multi-chunk paste buffering when paste spans multiple input events.
  - Line ending normalization (`\r\n` and `\r` to `\n`) during paste.
  - Control character filtering: Rejects C0 (0x00-0x1F except newline), DEL (0x7F), and C1 (0x80-0x9F) characters.
  - Tests for paste mode (single/multi-chunk, cursor position, line endings) and control char filtering.
- **TS extension loading tests**:
  - Test verifies TypeScript extensions can be loaded via jiti when available.
  - Falls back gracefully with proper error message when jiti is not installed.
  - Uses ESM `export default` syntax matching TS test conventions.
- **Interactive TUI slash command autocomplete**:
  - `tui/autocomplete.rs`: Extended to support slash command completion.
  - `SlashCommand` struct with name and description for autocomplete display.
  - `get_suggestions()` method triggers command autocomplete when typing `/` at line start.
  - `apply_completion()` handles slash command completion with trailing space.
  - `tui/components/select_list.rs`: New dropdown component for autocomplete display.
  - Arrow key navigation, scroll indicators, and selection highlighting.
  - Unit tests for command filtering, case-insensitive matching, and completion application.
- **New slash commands in interactive mode**:
  - `/clear` - Clears the screen/chat history.
  - `/copy` - Copies last assistant message to clipboard (platform-aware: pbcopy/xclip/wl-copy/clip.exe).
  - `/help` - Shows all available commands with descriptions.
  - `/new` - Starts a new session with fresh ID.
  - `/reset` - Resets the current session (clears messages).
  - `/session` - Shows session info (ID, message count, model).
  - `/theme [name]` - Lists or changes the current theme.
  - Editor autocomplete integration: Triggers on `/`, Tab for files, Up/Down/Tab/Enter for selection.
- **Bash command execution (`!` prefix)**:
  - Single `!` runs command and displays output in chat.
  - Double `!!` runs command without adding to context.
  - Displays command, stdout, exit code (if non-zero), and cancelled status.
  - Added to editor history for command recall.
- **Prompt template suggestions in autocomplete**:
  - Prompt templates from `~/.pi/agent/prompts` and `.pi/prompts` are now included in slash command autocomplete.
  - Shows template name and description (from frontmatter or first line).
  - Full parity with TS which adds `promptTemplates` to autocomplete.
- **Extension command suggestions in autocomplete**:
  - Extension commands are now included in slash command autocomplete.
  - `AgentSession` stores extension commands via `set_extension_commands()` / `extension_commands()`.
  - Commands include name and description from extension manifest.
- **Interactive UI selectors for /model and /settings**:
  - `ModelSelectorComponent`: Full UI picker for model selection with search filtering.
    - Displays models sorted by current first, then provider/id.
    - Shows reasoning indicator (⚡) and current model checkmark (✓).
    - Keyboard navigation (up/down, wrapping) with scroll indicators.
    - Fuzzy search filtering on model id and provider name.
  - `SettingsSelectorComponent`: Full UI picker for settings with value selection.
    - Two-level UI: settings list → value list for each setting.
    - Shows current value with checkmark indicator.
    - All settings supported: autocompact, show-images, auto-resize-images, steering-mode, follow-up-mode, thinking-level, theme, hide-thinking, collapse-changelog, double-escape-action.
    - Descriptions shown for selected items.
  - `/model` command now opens model picker UI when called with no arguments.
  - `/settings` command now opens settings picker UI when called with no arguments.
  - Both commands still support direct arguments (`/model claude-opus-4`, `/settings theme dark`).
- **OAuth login/logout flow**:
  - `coding_agent/oauth.rs`: Full OAuth implementation for Anthropic, OpenAI Codex, and GitHub Copilot.
  - PKCE code challenge generation (SHA-256 + Base64URL encoding).
  - Anthropic OAuth: Authorization URL generation, token exchange, refresh token support.
  - OpenAI Codex OAuth: Authorization URL with local callback server on port 1455, token exchange, JWT account ID extraction.
  - GitHub Copilot OAuth: Device code flow structure (polling not yet wired in TUI).
  - `OAuthSelectorComponent`: TUI picker for selecting OAuth provider to login/logout.
  - `LoginDialogComponent`: TUI dialog for OAuth flow with URL display, code input, progress/error states.
  - `/login` command: Opens OAuth provider selector, initiates browser-based OAuth flow.
  - `/logout` command: Opens OAuth provider selector (showing logged-in providers), removes credentials.
  - Credentials stored via `ModelRegistry::set_credential()` / `remove_credential()`.
  - Browser auto-open via platform-specific commands (xdg-open/open/start).
- **Clipboard image paste**:
  - Ctrl+V in TUI editor checks clipboard for image content.
  - Linux: Supports both Wayland (wl-paste) and X11 (xclip) clipboard access.
  - macOS: Uses pngpaste utility.
  - Windows: Uses PowerShell to read clipboard image.
  - Image written to temp file with UUID-based name (pi-clipboard-{UUID}.png).
  - File path inserted at cursor position for inclusion in message.
- **Google Gemini CLI (Cloud Code Assist) provider** - Full streaming support:
  - `api/google_gemini_cli.rs`: Complete provider implementation with SSE streaming.
  - Request/response types for Cloud Code Assist API (`v1internal:streamGenerateContent`).
  - Message converter: Converts internal messages to Gemini Content format with user/model roles.
  - Thinking support: Gemini 2.5+ models emit thinking blocks with `thought: true` parts.
  - Tool calling: Function call handling with auto-generated IDs, function response formatting.
  - System instructions via separate `systemInstruction` field.
  - Project discovery: `discover_gemini_project()` calls `loadCodeAssist`/`onboardUser` APIs.
  - OAuth token refresh: `refresh_google_cloud_token()` with Google OAuth2 endpoints.
  - Auth resolution: Checks auth.json first, then `~/.gemini/oauth_creds.json` (official gemini CLI).
  - Automatic token refresh when expired, project ID discovery from Cloud Code Assist API.
  - CLI integration: `--provider google-gemini-cli --model gemini-2.5-flash` (or gemini-2.5-pro, etc.).
  - **Live tests**: `tests/subscription_live_test.rs` with `gemini_cli_live_streaming_text` and `gemini_cli_live_tool_call`.
- **PyO3 Python bindings** (feature-gated under `python` feature):
  - `src/python/mod.rs`: Python bindings for embedding pi-mono-rust in Python applications.
  - `PyAuthStorage`: Wrapper for credential storage (get/set/remove/list/reload).
  - `PyAgentSession`: Wrapper for agent sessions (prompt, subscribe, abort, session management).
  - **API streaming integration**: PyAgentSession now properly wires up real API streaming for:
    - Anthropic Messages API (via `build_anthropic_stream_fn`)
    - OpenAI Responses API (via `build_openai_stream_fn`)
    - OpenAI Codex Responses API (via `build_codex_stream_fn`)
    - Google Gemini CLI API (via `build_gemini_stream_fn`)
  - OAuth helper functions: `anthropic_get_auth_url`, `anthropic_exchange_code`, `anthropic_refresh_token`.
  - OAuth helper functions: `openai_codex_get_auth_url`, `openai_codex_exchange_code`, `openai_codex_refresh_token`.
  - Event streaming: Python callbacks receive session events as dicts (agent events, compaction events).
  - All classes marked `unsendable` for single-threaded use (matching Rc/RefCell internals).
  - Build: `maturin develop --features python` (in pi-mono-rust directory with a Python venv).
  - **Gemini CLI auth fallback**: `AuthStorage.has_auth()` and `get_api_key()` now detect `~/.gemini/oauth_creds.json` for `google-gemini-cli` provider.
  - **Gemini CLI creds parsing fix**: `expiry_date` field parsed as float (official gemini CLI writes floats, not ints).
  - Note: Tools not yet wired in PyO3 (basic chat mode only; tool support TODO).

## Remaining Gaps (Accurate as of 2026-01-07)

### Interactive TUI:
- All major features implemented. No known gaps.

### TS Extensions:
- JS extensions work, TS extensions load via jiti when available (with fallback message if jiti not installed)

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
