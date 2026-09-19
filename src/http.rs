use std::time::Duration;

pub struct Response {
    pub status: u16,
    pub body: String,
}

pub fn get(url: &str, headers: &[(&str, String)]) -> Result<Response, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("http client: {e}"))?;
    let mut req = client.get(url);
    for (name, value) in headers {
        req = req.header(*name, value);
    }
    let resp = req.send().map_err(|e| format!("request failed: {e}"))?;
    let status = resp.status().as_u16();
    let body = resp.text().map_err(|e| format!("reading body: {e}"))?;
    Ok(Response { status, body })
}

pub fn post_json(url: &str, body: &str) -> Result<Response, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("http client: {e}"))?;
    let resp = client
        .post(url)
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .send()
        .map_err(|e| format!("request failed: {e}"))?;
    let status = resp.status().as_u16();
    let body = resp.text().map_err(|e| format!("reading body: {e}"))?;
    Ok(Response { status, body })
}

/// Fallback for endpoints that reject non-curl TLS fingerprints. The request
/// is passed as a curl config file on stdin so tokens never appear in the
/// process list.
pub fn get_via_curl(url: &str, headers: &[(&str, String)]) -> Result<Response, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new("curl")
        .args([
            "-sS",
            "--max-time",
            "10",
            "-w",
            "\n%{http_code}",
            "--config",
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("curl: {e}"))?;
    child
        .stdin
        .take()
        .expect("stdin piped")
        .write_all(curl_config(url, headers).as_bytes())
        .map_err(|e| format!("curl stdin: {e}"))?;
    let out = child.wait_with_output().map_err(|e| format!("curl: {e}"))?;

    let stdout = String::from_utf8_lossy(&out.stdout);
    let Some((body, code)) = stdout.rsplit_once('\n') else {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("curl failed: {}", stderr.trim()));
    };
    let status: u16 = code
        .trim()
        .parse()
        .map_err(|_| format!("curl: unparseable status {code:?}"))?;
    Ok(Response {
        status,
        body: body.to_string(),
    })
}

fn curl_config(url: &str, headers: &[(&str, String)]) -> String {
    let escape = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let mut config = format!("url = \"{}\"\n", escape(url));
    for (name, value) in headers {
        config.push_str(&format!("header = \"{}: {}\"\n", name, escape(value)));
    }
    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curl_config_quotes_url_and_headers() {
        let config = curl_config(
            "https://api.anthropic.com/api/oauth/usage",
            &[
                ("Authorization", "Bearer sk-ant-oat01-abc".to_string()),
                ("User-Agent", "claude-code/2.1.278".to_string()),
            ],
        );
        assert_eq!(
            config,
            "url = \"https://api.anthropic.com/api/oauth/usage\"\n\
             header = \"Authorization: Bearer sk-ant-oat01-abc\"\n\
             header = \"User-Agent: claude-code/2.1.278\"\n"
        );
    }

    #[test]
    fn curl_config_escapes_quotes() {
        let config = curl_config("https://example.com", &[("X-Odd", "a\"b\\c".to_string())]);
        assert!(config.contains("header = \"X-Odd: a\\\"b\\\\c\"\n"));
    }
}
