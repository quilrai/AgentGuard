# AgentGuard

Local guardrails and token saving for coding agents.

Website: https://agentguard.quilrai.dev

Downloads:

- [Mac, Apple silicon - 12MB](https://github.com/quilrai/AgentGuard/releases/latest/download/AgentGuard-Apple-Silicon.dmg)
- [Ubuntu, x86_64 .deb](https://github.com/quilrai/AgentGuard/releases/latest/download/AgentGuard-Linux-x86_64.deb)

Windows build: coming soon.

![AgentGuard screenshot](screenshots/home.png)

## What It Is

AgentGuard is a Tauri desktop app. It runs locally, listens on `localhost:8008`, and connects to agent CLIs/IDEs through hooks.

No proxy. No cloud relay. Hook events come in, policy decisions go out.

Supports:

- Claude Code
- Codex CLI
- Cursor IDE

## Save Costs With Claude Code

AgentGuard cuts token waste before it reaches Claude Code.

- Smart file-read cache: unchanged files come back as compact references instead of being resent.
- Context-aware reads: full, diff, or line-range responses are chosen based on what changed.
- Shell output compression: noisy output from builds, tests, package managers, logs, `git`, `grep`, `rg`, `find`, `ls`, `curl`, and more gets summarized.
- Search compression: large `grep`/`rg` results are grouped by file and trimmed to the useful matches.
- Cheaper shell habits: `rg`-style search, targeted reads, and summarized file discovery instead of giant recursive dumps.
- Diff compression: keeps the actual changes, drops excess context.
- JSON crusher: trims huge arrays, long strings, and deep nesting while preserving structure.

Use the Token Saver tab and apply recommended settings for Claude Code.

## Guardian Agent

AgentGuard puts a local policy layer in front of coding agents.

- Sensitive data guard: blocks prompts, tool inputs, file reads, shell commands, and MCP calls that match secrets, credentials, API keys, PII, or custom patterns.
- Dependency guard: blocks known compromised or vulnerable packages before the agent installs them.
- Update advisor: nudges the agent toward newer exact-pinned package versions instead of relying on stale model memory.
- Token limit: blocks oversized requests per agent.
- Local logs: records prompts, tool calls, blocks, detections, and token savings in SQLite.

Dependency protection checks install commands and dependency files such as `package.json`, `requirements.txt`, `pyproject.toml`, `Cargo.toml`, `go.mod`, `Gemfile`, `pom.xml`, `build.gradle`, `.csproj`, and `composer.json`.

Vulnerability data comes from OSV. Latest-version checks cover PyPI, npm, crates.io, RubyGems, NuGet, and Go.

## What The App Has

- Agent hook installer for Claude Code, Codex CLI, and Cursor IDE.
- Per-agent DLP settings and custom pattern management.
- Per-agent dependency protection toggles: Vulnerability Guard and Update Advisor.
- Claude Code token-saving toggles.
- Dashboard for requests, tools, token usage, latency, and savings.
- Logs page for searchable local event history.
- Behaviour monitor for read-first discipline, exploration balance, bash reliance, and tool tempo.
- Garden view for project/module/file activity, import graphs, and tree-sitter symbols.

## How It Works

AgentGuard writes hook scripts into the agent's config area:

- Claude Code: `~/.claude/settings.json` and `~/.claude/hooks/`
- Codex CLI: `~/.codex/hooks.json` and `~/.codex/hooks/`
- Cursor IDE: `~/.cursor/hooks/`

The hooks POST events to the local Tauri server. If the app is not running, hooks fail open so the agent is not stuck.

Local database:

```txt
~/.quilrdlpapp/proxy_requests.db
```

## Build The App Yourself

Requirements:

- Node.js
- npm
- Rust
- Tauri system dependencies for your OS

Run:

```sh
npm install
npm run tauri dev
```

Build:

```sh
npm run tauri build
```
