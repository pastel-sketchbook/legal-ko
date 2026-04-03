# legal-ko

대한민국 법령을 검색하고 읽는 Rust 도구.
[legalize-kr](https://github.com/legalize-kr/legalize-kr) 저장소의 전체 법령 데이터를
터미널에서 바로 탐색할 수 있습니다.

Browse, search, and read all Korean laws from the terminal. Fetches live data
from the [legalize-kr](https://github.com/legalize-kr/legalize-kr) repository.

## Features

- **TUI** (`legal-ko`) — ratatui 기반 터미널 UI, Vim 키바인딩, 14가지 테마
- **CLI** (`legal-ko-cli`) — LLM 친화적 CLI, `--json` 출력 지원
- **LLM Skill** — AI 에이전트가 자연어 질문으로 법률을 검색할 수 있는 스킬 포함

## Install

Requires Rust 2024 edition (1.85+).

```bash
cargo build --workspace --release
cp target/release/legal-ko ~/bin/legal-ko
cp target/release/legal-ko-cli ~/bin/legal-ko-cli
```

Or use [Task](https://taskfile.dev):

```bash
task install
```

## TUI

```bash
legal-ko        # or: task run
```

### Keybindings

| Key | Action |
|-----|--------|
| `j` / `k` | Move down / up |
| `g` / `G` | Top / bottom |
| `Ctrl+d` / `Ctrl+u` | Page down / up |
| `Enter` | Open law |
| `/` | Search |
| `↑` / `↓` | Navigate results while searching |
| `Ctrl+k` / `Ctrl+j` | Navigate results while searching (vim) |
| `c` | Filter by category |
| `d` | Filter by department |
| `n` / `p` | Next / previous article (제X조) |
| `a` | Article list popup |
| `B` | Toggle bookmark |
| `b` | Bookmarks only |
| `t` | Cycle theme |
| `o` | Open AI agent split pane |
| `?` | Help |
| `Esc` / `q` | Back / quit |

### Themes

14 themes (7 dark + 7 light), persisted across sessions:

Default, Gruvbox, Solarized, Ayu, Flexoki, Zoegi, FFE Dark,
Default Light, Gruvbox Light, Solarized Light, Flexoki Light, Ayu Light, Zoegi Light, FFE Light

## CLI

LLM-friendly interface. All subcommands support `--json`.

```bash
# List all laws
legal-ko-cli list --json --limit 10

# Search by title
legal-ko-cli search "민법" --json

# Read a law
legal-ko-cli show "kr/민법/법률" --json

# List articles
legal-ko-cli articles "kr/민법/법률" --json

# Bookmarked laws
legal-ko-cli bookmarks --json
```

### Subcommands

| Command | Description |
|---------|-------------|
| `list` | List laws, optionally filtered by `--category`, `--department`, `--bookmarks` |
| `search <query>` | Title search (Meilisearch or naive fallback) |
| `show <id>` | Full law content (markdown, frontmatter stripped) |
| `articles <id>` | List articles (제X조) with line indices |
| `bookmarks` | List bookmarked laws |
| `context` | Current TUI browsing state (for OpenCode integration) |
| `navigate <id>` | Send navigate command to TUI (`--article` for article jump) |
| `speak <id>` | TTS playback (requires `--features tts`) |

Law IDs follow the path format: `kr/{법령명}/{유형}` (e.g., `kr/형법/법률`)

## LLM Skill

The `.agents/skills/legal-ko-search/` skill enables AI agents to find relevant
Korean laws from natural language questions.

**Example:** "전세 문제가 있어. 관련 법을 찾아줘."

The skill translates colloquial legal questions into `legal-ko-cli` search
commands, reads law content, and cites specific articles — with a mandatory
disclaimer that results are not legal advice. See
[SKILL.md](.agents/skills/legal-ko-search/SKILL.md) for the full workflow.

## AI Agent Integration

The TUI has bidirectional communication with AI coding agents for
AI-assisted law browsing. Supported agents:

| Agent | Binary |
|-------|--------|
| [OpenCode](https://opencode.ai) | `opencode` |
| [Gemini CLI](https://github.com/google-gemini/gemini-cli) | `gemini` |
| [GitHub Copilot CLI](https://docs.github.com/en/copilot) | `copilot` |
| [Amp](https://amp.dev) | `amp` |

**Open a split:** Press `o` in the TUI to open the agent picker popup. Only
agents found on `$PATH` are listed. If only one agent is installed, it opens
directly (no popup). The last-used choice is persisted across sessions.

Split panes use a 40:60 ratio (TUI gets 40%, agent gets 60%) and support
tmux, WezTerm, Zellij, and Ghostty.

**TUI → OpenCode (context):** The TUI writes its browsing state to
`~/.cache/legal-ko/context.json` on every navigation event. OpenCode reads it
via `legal-ko-cli context --json` to understand what the user is looking at.

**OpenCode → TUI (navigate):** OpenCode sends navigation commands via
`legal-ko-cli navigate`, and the TUI picks them up on the next tick (~50ms).
Behaviour is context-aware:

```bash
# On list view: scrolls to and highlights the law
legal-ko-cli navigate "kr/주택임대차보호법/법률"

# On detail view: jumps to the article (prefix match)
legal-ko-cli navigate "kr/주택임대차보호법/법률" --article "제3조"
```

## Architecture

```
crates/
  core/     lib    — models, HTTP client, cache, parser, bookmarks, context, search
  tui/      bin    — ratatui terminal UI (legal-ko)
  cli/      bin    — clap CLI with --json (legal-ko-cli)
```

- **Data source**: GitHub API (legalize-kr/legalize-kr)
- **Caching**: `~/.cache/legal-ko/` (SHA256-keyed, per law file)
- **Context**: `~/.cache/legal-ko/context.json` (TUI→OpenCode), `command.json` (OpenCode→TUI)
- **Config**: `~/.config/legal-ko/` (bookmarks, theme & agent preferences)
- **Search**: Optional [Meilisearch](https://www.meilisearch.com/) backend
  (`meilisearch` feature), falls back to title substring matching

### Meilisearch (optional)

```bash
# Build with Meilisearch support
cargo build --workspace --release --features meilisearch

# Configure
export LEGAL_KO_MEILI_URL=http://localhost:7700
export LEGAL_KO_MEILI_KEY=your-key        # optional
export LEGAL_KO_MEILI_INDEX=legal_ko_laws  # optional, this is the default
```

### TTS (optional)

Text-to-speech is behind the `tts` feature flag (requires
[vibe-rust](https://github.com/anthropics/vibe-rust)):

```bash
cargo build --workspace --release --features tts
```

| Key | Action |
|-----|--------|
| `r` | Read current article aloud |
| `R` | Read full law |
| `s` | Stop playback |
| `T` | Toggle TTS profile |

## Development

```bash
task check:all   # typos + fmt check + clippy + tests
task test         # cargo test --workspace
task clippy       # cargo clippy --workspace
task fmt          # cargo fmt --all
task dev          # debug build
task run:dev      # run TUI (debug)
task run:cli:dev  # run CLI (debug)
task loc          # lines of code (tokei)
```

## Data Source

All law data comes from [legalize-kr](https://github.com/legalize-kr/legalize-kr),
which collects Korean legislation from the
[국가법령정보센터 OpenAPI](https://open.law.go.kr).
Law texts are Korean government public works and freely available.

## License

MIT — see [LICENSE](LICENSE).

Law texts accessed through this tool are public works of the Republic of Korea government.
