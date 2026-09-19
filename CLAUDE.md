# ai-quotas

CLI that shows quota usage and reset times for AI subscriptions (Claude, Codex/ChatGPT, Z.ai, Grok), then recommends which one to use now. All four providers are private, undocumented APIs — treat every schema as unstable.

## Commands

- `cargo test` — fully offline; always safe to run.
- `cargo clippy -- -D warnings` — must be clean before every commit.
- `cargo fmt` — run before every commit.
- `cargo run` — hits live provider APIs with real local credentials. Do not run in loops; the Claude endpoint rate-limits aggressively.

## Architecture

| Module | Role |
|---|---|
| `src/main.rs` | clap args, `thread::scope` fan-out (one thread per provider), render, verdict, exit codes |
| `src/model.rs` | `ProviderStatus` / `QuotaWindow` — the normalized model everything consumes |
| `src/creds.rs` | credential loading: thin I/O wrappers + pure, tested parsers |
| `src/http.rs` | thin blocking GET helper + curl-subprocess fallback |
| `src/render.rs` | pure: `(&[ProviderStatus], now, tz) -> String` table |
| `src/verdict.rs` | pure deterministic scoring |
| `src/providers/*.rs` | per provider: pure `parse_usage(&str, now) -> Result<ProviderStatus, String>` + thin `fetch()` |

Rule: parsing is pure and fixture-tested; `fetch()` is thin I/O glue that maps every failure into `ProviderStatus::unavailable(reason)`. Provider registry is a fixed array in `providers/mod.rs` — no trait objects.

## Hard rules

- **Never write** to `~/.codex/auth.json`, `~/.grok/auth.json`, `~/.claude/.credentials.json`, or the macOS Keychain. Codex token refresh is in-memory only. Claude and Grok tokens are never refreshed (expired → actionable `unavailable` message).
- Tolerant deserialization everywhere: `Option` fields, no `deny_unknown_fields`. Schema surprises degrade to `unavailable`/`unknown` — never a panic, never rendered as 0%.
- One provider's failure must not affect the others or the exit code (exit 0 if ≥1 provider succeeds).
- Classify Codex windows by `limit_window_seconds`, never by primary/secondary slot name.
- Cross-platform (macOS + Linux): no hardcoded home paths; macOS-only code behind `cfg(target_os = "macos")` with a shared fallback path.

## Workflow

TDD: write the failing test first. Every commit compiles, passes `cargo test`, and is clippy-clean with zero warnings. No dead code — glue lands in the same commit as its first caller. Simple and boring over clever.

Fixtures live in `tests/fixtures/<provider>/*.json`, loaded with `include_str!` from module tests. Each provider has: a happy-path fixture, a minimal fixture (optional fields missing), and a hostile fixture (drifted schema) asserting graceful degradation.

## API quirks (hard-won; do not "clean up")

- **Claude** `GET api.anthropic.com/api/oauth/usage`: requires `anthropic-beta: oauth-2025-04-20`, `User-Agent: claude-cli/<version>`, `x-app: cli` (extracted from the actual Claude Code binary). The edge may 403 non-curl TLS fingerprints → curl subprocess fallback. `utilization` is already 0–100. **Scope trap:** the endpoint needs `user:profile`; tokens from `claude setup-token` / `CLAUDE_CODE_OAUTH_TOKEN` are inference-only by design and get a **persistent 429** (not a 403!) — verified via `/api/oauth/profile` returning `oauth_scope_insufficient` for the same token. Only `claude auth login` mints full-scope creds (stored in the macOS Keychain / `~/.claude/.credentials.json`). Because any one source can hold a rejected token while another works, `try_fetch` walks all sources (env → Keychain → file) and reports every failure. Keychain items can be stale leftovers from old logins (seen live: dead August item beside a working env-token login).
- **Codex** `GET chatgpt.com/backend-api/wham/usage`: needs `ChatGPT-Account-Id` header. `reset_at` is unix seconds. Missing window = unavailable, not 0%. Refresh via `auth.openai.com/oauth/token`, client_id `app_EMoamEEZ73f0CkXaXp7hrann`.
- **Z.ai** `GET api.z.ai/api/monitor/usage/quota/limit`: `Authorization` is the **raw key, no `Bearer` prefix**. Envelope `{code, msg, data}`. `nextResetTime` is epoch **milliseconds**. Window identified by `(unit, number)`: (3,5)=5h, (6,1)=week. Type strings have drifted (`TOKENS_LIMIT`→`CREDIT_LIMIT`).
- **Grok** `GET cli-chat-proxy.grok.com/v1/billing?format=credits`: needs `X-XAI-Token-Auth: xai-grok-cli` and `x-userid` headers. Billing requires OIDC auth (issuer `https://auth.x.ai`) from `~/.grok/auth.json`, scope key `https://auth.x.ai::b1a00492-073a-47ea-816f-4c329264a828`; plain API keys are rejected. USD cents arrive as `{"val": 123}` and **`{}` means $0** (proto3 omits zero scalars).
