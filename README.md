# AgentGuard

**[Download for Mac (Apple silicon) - 12MB](https://github.com/quilrai/AgentGuard/releases/latest/download/AgentGuard-Apple-Silicon.dmg)**

**Download Windows (Coming Soon)**

Desktop App for monitoring and controlling llm requests (with focus on coding agents)

![AgentGuard screenshot](screenshots/home.png)

**Fully local (on-device), desktop app for**
- block or get notified on high request or token usage by agents
- searchable complete history for LLM requests by agents
- pass-through proxy server for LLM requests
- block or redact requests with sensitive information (pre-defined and user-defined patterns)
- codex, claude, cursor supported out of the box with tool call and token monitoring

## How it works

- **codex and claude code**: Codex and Claude Code support a configurable base URL, which lets AgentGuard route all requests through its local server.
- **cursor**: Cursor has limited hooks that AgentGuard uses to block or monitor requests (auto-redaction and exact token counts are not supported).

**Custom LLM endpoints**
- In the app, you can configure a custom chat completions endpoint
- This feature is useful if you are using your own token with a LLM endpoint, and you want to monitor / control data

## Detections

- Block or Redact data going to LLMs automatically with intelligent pattern matching
- Pre-defined patterns for general use cases (API Keys, credentials, etc)
