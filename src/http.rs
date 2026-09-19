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
