# ai-quotas

One command that shows how much of each AI subscription you have used, when each limit resets (in your local timezone), and which subscription to use right now.

```
$ ai-quotas
claude · max
  5h    [██░░░░░░░░░░░░░░░░░░]  11%  resets in 4h 9m (Sep 19 23:10)
  week  [█████████████░░░░░░░]  67%  resets in 4d 1h (Sep 23 21:00)

codex · pro
  week  [███░░░░░░░░░░░░░░░░░]  13%  resets in 6d 17h (Sep 26 12:51)

zai · pro
  5h    [██░░░░░░░░░░░░░░░░░░]   9%  resets in 4h 40m (Sep 19 23:40)
  week  [░░░░░░░░░░░░░░░░░░░░]   1%  resets in 3d 17h (Sep 23 12:00)

grok
  week  [██████████████░░░░░░]  72%  resets in 1d 17h (Sep 21 12:04)

Verdict: use zai — 91% headroom on its tightest window (5h), resets in 4h 40m.
```

Each provider has its own accent color and each bar is colored by severity (green under 50%, yellow under 80%, red above). Colors turn off automatically when piped, or with `NO_COLOR=1`.

Supported providers: **Claude** (claude.ai Pro/Max via Claude Code), **Codex** (ChatGPT Plus/Pro), **Z.ai** (GLM coding plan), **Grok** (SuperGrok via the Grok CLI).

Works on macOS and Linux.

## Install

```sh
cargo install --path .
```

## Setup

`ai-quotas` reuses the credentials that each provider's own tool already stores. It never writes to any credential store.

| Provider | What it needs | How to get it |
|---|---|---|
| claude | Claude Code login | Log in to [Claude Code](https://claude.com/claude-code) once. Read from `CLAUDE_CODE_OAUTH_TOKEN` if set, else the macOS Keychain, else `~/.claude/.credentials.json`. |
| codex | Codex CLI login | Run `codex login` once (`~/.codex/auth.json`). |
| zai | Coding-plan API key | Set `ZAI_API_KEY`, or write the key to `~/.config/ai-quotas/zai-api-key`. Keys: [z.ai/manage-apikey](https://z.ai/manage-apikey/apikey-list). |
| grok | Grok CLI login | Install the [Grok CLI](https://docs.x.ai/build/cli/reference) and run `grok login` once (`~/.grok/auth.json`). A plain `XAI_API_KEY` cannot query billing. |

A provider that is not set up simply shows an `unavailable` row with the reason; the others still work.

## Usage

```sh
ai-quotas                   # all providers, table + verdict
ai-quotas --provider claude # one provider (repeatable)
```

### Multiple Codex profiles

Use repeatable `--codex-profile LABEL=AUTH_JSON_PATH` options to query separate
Codex logins in one run:

```sh
ai-quotas --provider claude --provider codex \
  --codex-profile "Personal=$HOME/.codex/auth.json" \
  --codex-profile "Work=$HOME/.codex-work/auth.json"
```

Each profile appears as `codex (Personal)` or `codex (Work)` in the output and
verdict. Explicit profiles replace the default Codex query; `--provider` filters
still apply. Labels must be unique and nonempty. Quote arguments containing spaces.
Credential files are read in place and never modified. Without these options,
the existing single-profile behavior is unchanged.

Exit code 0 if at least one provider returned data, 1 if none did.

## How it works

Each provider is queried concurrently through the same private endpoint its own tooling uses:

- Claude: `api.anthropic.com/api/oauth/usage` (OAuth token, read-only; expired tokens are reported, never refreshed)
- Codex: `chatgpt.com/backend-api/wham/usage` (expired access tokens are refreshed in memory only; `auth.json` is never written)
- Z.ai: `api.z.ai/api/monitor/usage/quota/limit` (costs zero tokens, works even when the quota is exhausted)
- Grok: `cli-chat-proxy.grok.com/v1/billing` (the endpoint behind the Grok CLI's `/usage`)

**Caveat:** all four are private, undocumented APIs. They can change or break at any time; when they do, the affected provider degrades to an `unavailable` row.

## Development

```sh
cargo test                    # fully offline, fixture-driven
cargo clippy -- -D warnings
```

`cargo run` hits the live APIs with your real credentials — avoid running it in a tight loop.

## License

Apache 2.0
