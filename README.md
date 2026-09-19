# quotas

One command that shows how much of each AI subscription you have used, when each limit resets (in your local timezone), and which subscription to use right now.

```
$ quotas
PROVIDER  PLAN  WINDOW  USED  RESETS
claude    max   5h       33%  in 2h 13m (Sep 19 14:13)
claude    max   week     13%  in 3d 0h (Sep 22 12:00)
codex     pro   week     11%  in 6d 18h (Sep 26 12:51)
zai       pro   5h        0%  -
zai       pro   week      1%  in 3d 17h (Sep 23 12:00)
grok      -     week     72%  in 1d 17h (Sep 21 12:04)
Verdict: use zai — 99% headroom on its tightest window (week), resets in 3d 17h.
```

Supported providers: **Claude** (claude.ai Pro/Max via Claude Code), **Codex** (ChatGPT Plus/Pro), **Z.ai** (GLM coding plan), **Grok** (SuperGrok via the Grok CLI).

Works on macOS and Linux.

## Install

```sh
cargo install --path .
```

## Setup

`quotas` reuses the credentials that each provider's own tool already stores. It never writes to any credential store.

| Provider | What it needs | How to get it |
|---|---|---|
| claude | Claude Code login | Log in to [Claude Code](https://claude.com/claude-code) once. macOS: read from the Keychain; Linux: `~/.claude/.credentials.json`. |
| codex | Codex CLI login | Run `codex login` once (`~/.codex/auth.json`). |
| zai | Coding-plan API key | Set `ZAI_API_KEY`, or write the key to `~/.config/quotas/zai-api-key`. Keys: [z.ai/manage-apikey](https://z.ai/manage-apikey/apikey-list). |
| grok | Grok CLI login | Install the [Grok CLI](https://docs.x.ai/build/cli/reference) and run `grok login` once (`~/.grok/auth.json`). A plain `XAI_API_KEY` cannot query billing. |

A provider that is not set up simply shows an `unavailable` row with the reason; the others still work.

## Usage

```sh
quotas                      # all providers, table + verdict
quotas --provider claude    # one provider (repeatable)
```

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

MIT
