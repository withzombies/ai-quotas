use jiff::Timestamp;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

// ---------- Claude ----------

pub struct ClaudeCreds {
    pub access_token: String,
    pub plan: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeCredFile {
    claude_ai_oauth: Option<ClaudeOauth>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeOauth {
    access_token: Option<String>,
    /// Epoch milliseconds.
    expires_at: Option<i64>,
    #[serde(default)]
    scopes: Vec<String>,
    subscription_type: Option<String>,
}

pub fn parse_claude_creds(json: &str, now: Timestamp) -> Result<ClaudeCreds, String> {
    let file: ClaudeCredFile =
        serde_json::from_str(json).map_err(|e| format!("bad credentials JSON: {e}"))?;
    let oauth = file
        .claude_ai_oauth
        .ok_or("no claudeAiOauth entry in credentials")?;
    let access_token = oauth.access_token.ok_or("no access token in credentials")?;

    if let Some(ms) = oauth.expires_at
        && Timestamp::from_millisecond(ms).is_ok_and(|exp| exp <= now)
    {
        return Err("token expired — run 'claude auth login'".to_string());
    }
    for required in ["user:inference", "user:profile"] {
        if !oauth.scopes.iter().any(|s| s == required) {
            return Err(format!(
                "token missing scope {required} — log in via Claude Code, not setup-token"
            ));
        }
    }
    Ok(ClaudeCreds {
        access_token,
        plan: oauth.subscription_type,
    })
}

// ---------- Codex ----------

pub struct CodexCreds {
    pub access_token: String,
    pub account_id: Option<String>,
    pub refresh_token: Option<String>,
}

#[derive(Deserialize)]
struct CodexAuthFile {
    tokens: Option<CodexTokens>,
}

#[derive(Deserialize)]
struct CodexTokens {
    access_token: Option<String>,
    account_id: Option<String>,
    refresh_token: Option<String>,
}

pub fn parse_codex_auth(json: &str) -> Result<CodexCreds, String> {
    let file: CodexAuthFile =
        serde_json::from_str(json).map_err(|e| format!("bad auth.json: {e}"))?;
    let tokens = file
        .tokens
        .ok_or("no tokens in auth.json — run 'codex login'")?;
    let access_token = tokens
        .access_token
        .ok_or("no access token in auth.json — run 'codex login'")?;
    Ok(CodexCreds {
        access_token,
        account_id: tokens.account_id,
        refresh_token: tokens.refresh_token,
    })
}

// ---------- Grok ----------

pub struct GrokCreds {
    pub key: String,
    pub user_id: Option<String>,
}

#[derive(Deserialize)]
struct GrokAuthEntry {
    key: Option<String>,
    auth_mode: Option<String>,
    user_id: Option<String>,
    oidc_issuer: Option<String>,
    expires_at: Option<serde_json::Value>,
}

const XAI_ISSUER: &str = "https://auth.x.ai";

/// `expires_at` shape is unverified across CLI versions: accept epoch seconds,
/// epoch milliseconds, or an RFC3339 string; anything else skips the check.
fn parse_expiry(value: &serde_json::Value) -> Option<Timestamp> {
    match value {
        serde_json::Value::Number(n) => {
            let n = n.as_i64()?;
            if n > 100_000_000_000 {
                Timestamp::from_millisecond(n).ok()
            } else {
                Timestamp::from_second(n).ok()
            }
        }
        serde_json::Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

pub fn parse_grok_auth(json: &str, now: Timestamp) -> Result<GrokCreds, String> {
    let entries: BTreeMap<String, GrokAuthEntry> =
        serde_json::from_str(json).map_err(|e| format!("bad auth.json: {e}"))?;

    // Billing rejects plain API keys: only an OIDC/external login against
    // auth.x.ai is eligible.
    let eligible = entries.into_values().find(|e| {
        matches!(e.auth_mode.as_deref(), Some("oidc") | Some("external"))
            && e.oidc_issuer.as_deref() == Some(XAI_ISSUER)
    });
    let Some(entry) = eligible else {
        return Err(
            "no OIDC login found — run 'grok login' (API keys cannot query billing)".to_string(),
        );
    };
    if let Some(exp) = entry.expires_at.as_ref().and_then(parse_expiry)
        && exp <= now
    {
        return Err("token expired — run 'grok login'".to_string());
    }
    let key = entry.key.ok_or("no key in auth.json — run 'grok login'")?;
    Ok(GrokCreds {
        key,
        user_id: entry.user_id,
    })
}

// ---------- Z.ai ----------

pub fn zai_key_from(
    env_key: Option<String>,
    file_contents: Option<String>,
) -> Result<String, String> {
    let key = env_key
        .or(file_contents)
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty());
    key.ok_or_else(|| {
        "no API key (set ZAI_API_KEY or write ~/.config/ai-quotas/zai-api-key)".to_string()
    })
}

/// An env token is opaque: no expiry or scope metadata to pre-check, so it is
/// passed through and the API response decides.
pub fn claude_creds_from_env(token: Option<String>) -> Option<ClaudeCreds> {
    let token = token?.trim().to_string();
    if token.is_empty() {
        return None;
    }
    Some(ClaudeCreds {
        access_token: token,
        plan: None,
    })
}

// ---------- I/O (thin, untested) ----------

fn home() -> Option<PathBuf> {
    std::env::home_dir()
}

fn env_path(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

fn read(path: PathBuf) -> Result<String, String> {
    std::fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))
}

/// All Claude credential sources present on this machine, in Claude Code's own
/// precedence: CLAUDE_CODE_OAUTH_TOKEN env, macOS Keychain, credentials file.
/// The caller tries them in order — a source can hold a token the API rejects
/// (setup tokens are inference-only) while a later source works, so returning
/// only the first would wrongly block the provider.
pub fn claude_cred_sources(now: Timestamp) -> Vec<(&'static str, Result<ClaudeCreds, String>)> {
    let mut sources = Vec::new();
    if let Some(creds) = claude_creds_from_env(std::env::var("CLAUDE_CODE_OAUTH_TOKEN").ok()) {
        sources.push(("env token", Ok(creds)));
    }
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("security")
            .args([
                "find-generic-password",
                "-s",
                "Claude Code-credentials",
                "-w",
            ])
            .output();
        if let Ok(out) = out
            && out.status.success()
        {
            let json = String::from_utf8_lossy(&out.stdout);
            sources.push(("keychain", parse_claude_creds(json.trim(), now)));
        }
    }
    if let Some(dir) = env_path("CLAUDE_CONFIG_DIR").or_else(|| home().map(|h| h.join(".claude")))
        && let Ok(json) = read(dir.join(".credentials.json"))
    {
        sources.push(("credentials file", parse_claude_creds(&json, now)));
    }
    sources
}

pub fn load_codex_creds() -> Result<CodexCreds, String> {
    let dir = env_path("CODEX_HOME")
        .or_else(|| home().map(|h| h.join(".codex")))
        .ok_or("cannot determine home directory")?;
    load_codex_creds_from_path(&dir.join("auth.json"))
}

pub fn load_codex_creds_from_path(path: &std::path::Path) -> Result<CodexCreds, String> {
    let json = read(path.to_path_buf())
        .map_err(|e| format!("no credentials — run 'codex login' ({e})"))?;
    parse_codex_auth(&json)
}

pub fn load_grok_creds(now: Timestamp) -> Result<GrokCreds, String> {
    let path = env_path("GROK_AUTH_PATH").unwrap_or(
        env_path("GROK_HOME")
            .or_else(|| home().map(|h| h.join(".grok")))
            .ok_or("cannot determine home directory")?
            .join("auth.json"),
    );
    let json = read(path).map_err(|e| format!("no credentials — run 'grok login' ({e})"))?;
    parse_grok_auth(&json, now)
}

pub fn load_zai_key() -> Result<String, String> {
    let env_key = std::env::var("ZAI_API_KEY").ok();
    let file = home()
        .map(|h| h.join(".config/ai-quotas/zai-api-key"))
        .and_then(|p| std::fs::read_to_string(p).ok());
    zai_key_from(env_key, file)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    /// Cred structs deliberately do not implement Debug (token leak hazard),
    /// so Result::unwrap_err is unavailable.
    fn err<T>(r: Result<T, String>) -> String {
        match r {
            Ok(_) => panic!("expected an error"),
            Err(e) => e,
        }
    }

    const NOW: &str = "2026-09-19T12:00:00Z";
    // 2027-01-01T00:00:00Z in ms / s
    const FUTURE_MS: i64 = 1_798_761_600_000;
    const PAST_MS: i64 = 1_789_000_000_000;

    #[test]
    fn claude_creds_happy_path() {
        let json = format!(
            r#"{{"claudeAiOauth": {{
                "accessToken": "sk-ant-oat01-test",
                "refreshToken": "sk-ant-ort01-test",
                "expiresAt": {FUTURE_MS},
                "scopes": ["user:inference", "user:profile"],
                "subscriptionType": "max"
            }}}}"#
        );
        let creds = parse_claude_creds(&json, ts(NOW)).unwrap();
        assert_eq!(creds.access_token, "sk-ant-oat01-test");
        assert_eq!(creds.plan.as_deref(), Some("max"));
    }

    #[test]
    fn claude_creds_expired_and_missing_scope() {
        let expired = format!(
            r#"{{"claudeAiOauth": {{"accessToken": "t", "expiresAt": {PAST_MS},
                "scopes": ["user:inference", "user:profile"]}}}}"#
        );
        assert_eq!(
            err(parse_claude_creds(&expired, ts(NOW))),
            "token expired — run 'claude auth login'"
        );

        let inference_only = format!(
            r#"{{"claudeAiOauth": {{"accessToken": "t", "expiresAt": {FUTURE_MS},
                "scopes": ["user:inference"]}}}}"#
        );
        assert_eq!(
            err(parse_claude_creds(&inference_only, ts(NOW))),
            "token missing scope user:profile — log in via Claude Code, not setup-token"
        );
    }

    #[test]
    fn codex_auth_parses_tokens() {
        let json = r#"{"auth_mode": "chatgpt", "OPENAI_API_KEY": null,
            "tokens": {"id_token": "jwt", "access_token": "at", "refresh_token": "rt",
                       "account_id": "acc-123"},
            "last_refresh": "2026-09-15T00:00:00Z"}"#;
        let creds = parse_codex_auth(json).unwrap();
        assert_eq!(creds.access_token, "at");
        assert_eq!(creds.account_id.as_deref(), Some("acc-123"));
        assert_eq!(creds.refresh_token.as_deref(), Some("rt"));
    }

    #[test]
    fn codex_auth_without_tokens_is_actionable() {
        assert_eq!(
            err(parse_codex_auth(r#"{"OPENAI_API_KEY": "sk-..."}"#)),
            "no tokens in auth.json — run 'codex login'"
        );
    }

    #[test]
    fn grok_auth_selects_oidc_entry() {
        let json = r#"{
            "xai::api_key": {"key": "xai-123", "auth_mode": "api_key"},
            "https://auth.x.ai::b1a00492-073a-47ea-816f-4c329264a828": {
                "key": "oidc-token", "auth_mode": "oidc", "user_id": "u-1",
                "oidc_issuer": "https://auth.x.ai",
                "expires_at": "2027-01-01T00:00:00Z"
            }
        }"#;
        let creds = parse_grok_auth(json, ts(NOW)).unwrap();
        assert_eq!(creds.key, "oidc-token");
        assert_eq!(creds.user_id.as_deref(), Some("u-1"));
    }

    #[test]
    fn grok_auth_rejects_api_key_only_and_expired() {
        let api_only = r#"{"xai::api_key": {"key": "xai-123", "auth_mode": "api_key"}}"#;
        assert_eq!(
            err(parse_grok_auth(api_only, ts(NOW))),
            "no OIDC login found — run 'grok login' (API keys cannot query billing)"
        );

        let expired = r#"{"https://auth.x.ai::c": {
            "key": "t", "auth_mode": "oidc", "oidc_issuer": "https://auth.x.ai",
            "expires_at": 1789000000
        }}"#;
        assert_eq!(
            err(parse_grok_auth(expired, ts(NOW))),
            "token expired — run 'grok login'"
        );
    }

    #[test]
    fn grok_expiry_accepts_seconds_ms_and_rfc3339() {
        let s = parse_expiry(&serde_json::json!(1_798_761_600)).unwrap();
        let ms = parse_expiry(&serde_json::json!(FUTURE_MS)).unwrap();
        let rfc = parse_expiry(&serde_json::json!("2027-01-01T00:00:00Z")).unwrap();
        assert_eq!(s, rfc);
        assert_eq!(ms, rfc);
        assert_eq!(parse_expiry(&serde_json::json!(null)), None);
    }

    #[test]
    fn claude_env_token_passes_through_untouched() {
        let creds = claude_creds_from_env(Some(" sk-ant-oat01-envtoken\n".into())).unwrap();
        assert_eq!(creds.access_token, "sk-ant-oat01-envtoken");
        assert!(creds.plan.is_none());
        assert!(claude_creds_from_env(Some("  ".into())).is_none());
        assert!(claude_creds_from_env(None).is_none());
    }

    #[test]
    fn zai_key_prefers_env_then_file_and_trims() {
        assert_eq!(
            zai_key_from(Some("env-key".into()), Some("file-key".into())).unwrap(),
            "env-key"
        );
        assert_eq!(
            zai_key_from(None, Some("file-key\n".into())).unwrap(),
            "file-key"
        );
        assert!(err(zai_key_from(None, None)).contains("ZAI_API_KEY"));
        assert!(zai_key_from(Some("  ".into()), None).is_err());
    }
}
