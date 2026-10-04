//! Client for the Pangolin Integration API (plain HTTPS, not SSH).
//!
//! The webview never sees the API key: requests are made here with the key
//! from the OS keyring, to a sanitised path under the configured base URL.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::error::{AppError, AppResult, ErrorCode};
use crate::state::AppState;
use crate::store::secrets::GLOBAL;

pub const SECRET_API_KEY: &str = "pangolin/api-key";
const TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, Deserialize, Type)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl HttpMethod {
    fn as_reqwest(self) -> reqwest::Method {
        match self {
            HttpMethod::Get => reqwest::Method::GET,
            HttpMethod::Post => reqwest::Method::POST,
            HttpMethod::Put => reqwest::Method::PUT,
            HttpMethod::Patch => reqwest::Method::PATCH,
            HttpMethod::Delete => reqwest::Method::DELETE,
        }
    }
}

/// Accept only plain API paths: `/v1/...` made of unreserved characters.
pub fn sanitize_path(path: &str) -> AppResult<&str> {
    let ok = path.starts_with("/v1/") || path == "/v1";
    let chars_ok = path.bytes().all(|b| b.is_ascii_alphanumeric() || b"/_-.~".contains(&b));
    if ok && chars_ok && !path.contains("..") && !path.contains("//") && path.len() <= 512 {
        Ok(path)
    } else {
        Err(AppError::invalid("Invalid API path"))
    }
}

/// Normalise the user-entered base URL (scheme required, no trailing slash).
pub fn normalize_base(url: &str) -> AppResult<String> {
    let url = url.trim().trim_end_matches('/');
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .ok_or_else(|| AppError::invalid("The API URL must start with https://"))?;
    if rest.is_empty() || rest.contains(char::is_whitespace) || rest.contains(['?', '#']) {
        return Err(AppError::invalid("Invalid API URL"));
    }
    Ok(url.to_string())
}

fn client() -> AppResult<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(TIMEOUT)
        .connect_timeout(Duration::from_secs(10))
        .user_agent(concat!("jarvis-server-manager/", env!("CARGO_PKG_VERSION")))
        .build()?)
}

struct Response {
    status: u16,
    body: String,
}

async fn send(
    base: &str,
    key: &str,
    method: HttpMethod,
    path: &str,
    query: &[(String, String)],
    body: Option<&str>,
) -> AppResult<Response> {
    let mut request = client()?.request(method.as_reqwest(), format!("{base}{path}")).bearer_auth(key).query(query);
    if let Some(body) = body {
        request = request.header(reqwest::header::CONTENT_TYPE, "application/json").body(body.to_string());
    }
    let response = request.send().await?;
    let status = response.status().as_u16();
    let body = response.text().await?;
    Ok(Response { status, body })
}

/// Pangolin wraps errors as `{ "message": "...", ... }`.
fn error_message(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(str::to_string))
        .unwrap_or_else(|| body.chars().take(300).collect())
}

fn check(response: Response) -> AppResult<String> {
    match response.status {
        200..=299 => Ok(response.body),
        401 | 403 => Err(AppError::new(ErrorCode::PangolinUnauthorized, error_message(&response.body))),
        404 => Err(AppError::new(ErrorCode::NotFound, error_message(&response.body))),
        status => Err(AppError::new(ErrorCode::Http, format!("HTTP {status}: {}", error_message(&response.body)))),
    }
}

fn configured(state: &AppState) -> AppResult<(String, String)> {
    let settings = state.settings.get().pangolin;
    let key = state
        .secrets
        .get(GLOBAL, SECRET_API_KEY)?
        .filter(|k| !k.is_empty())
        .ok_or_else(|| AppError::code(ErrorCode::PangolinNotConfigured))?;
    let base = format!("{}{}", normalize_base(&settings.api_url)?, settings.base_path);
    Ok((base, key))
}

/// Proxy one request to the Pangolin API and return the response body (JSON text).
#[tauri::command]
#[specta::specta]
pub async fn pangolin_request(
    state: State<'_, AppState>,
    method: HttpMethod,
    path: String,
    query: Vec<(String, String)>,
    body: Option<String>,
) -> AppResult<String> {
    let (base, key) = configured(&state)?;
    let path = sanitize_path(&path)?;
    if let Some(body) = &body {
        serde_json::from_str::<serde_json::Value>(body)?;
    }
    check(send(&base, &key, method, path, &query, body.as_deref()).await?)
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PangolinOrg {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PangolinStatus {
    pub configured: bool,
    pub api_url: String,
    pub org_id: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PangolinVerify {
    /// Organisations visible to the key; empty for a key scoped to one org.
    pub orgs: Vec<PangolinOrg>,
    /// The key works but cannot list organisations (enter the org ID manually).
    pub limited: bool,
}

pub fn parse_orgs(body: &str) -> Vec<PangolinOrg> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return Vec::new();
    };
    let list = value.pointer("/data/orgs").or_else(|| value.get("data")).and_then(|v| v.as_array()).cloned().unwrap_or_default();
    list.iter()
        .filter_map(|org| {
            let id = org.get("orgId").or_else(|| org.get("id"))?.as_str()?.to_string();
            let name = org.get("name").and_then(|n| n.as_str()).unwrap_or(&id).to_string();
            Some(PangolinOrg { id, name })
        })
        .collect()
}

#[tauri::command]
#[specta::specta]
pub fn pangolin_status(state: State<'_, AppState>) -> PangolinStatus {
    let settings = state.settings.get().pangolin;
    PangolinStatus { configured: state.secrets.exists(GLOBAL, SECRET_API_KEY), api_url: settings.api_url, org_id: settings.org_id }
}

/// Save the connection settings and verify them. `api_key = None` keeps the stored key.
#[tauri::command]
#[specta::specta]
pub async fn pangolin_configure(
    state: State<'_, AppState>,
    api_url: String,
    api_key: Option<String>,
    org_id: String,
) -> AppResult<PangolinVerify> {
    let base = normalize_base(&api_url)?;
    let key = match api_key.filter(|k| !k.trim().is_empty()) {
        Some(key) => key.trim().to_string(),
        None => state.secrets.get(GLOBAL, SECRET_API_KEY)?.ok_or_else(|| AppError::invalid("Enter the API key"))?,
    };

    // The API lives under /v1 on Pangolin Cloud and under /api/v1 on some self-hosted setups.
    let mut base_path = None;
    let mut last = None;
    for candidate in ["", "/api"] {
        match send(&format!("{base}{candidate}"), &key, HttpMethod::Get, "/v1/", &[], None).await {
            Ok(r) if (200..300).contains(&r.status) => {
                base_path = Some(candidate);
                break;
            }
            Ok(r) => last = Some(check(r).unwrap_err()),
            Err(e) => last = Some(e),
        }
    }
    let Some(base_path) = base_path else {
        return Err(last.unwrap_or_else(|| AppError::code(ErrorCode::Http)));
    };
    let full = format!("{base}{base_path}");

    let orgs = send(&full, &key, HttpMethod::Get, "/v1/orgs", &[], None).await?;
    let (orgs, limited) = match orgs.status {
        200..=299 => (parse_orgs(&orgs.body), false),
        401 | 403 => {
            // Org-scoped key: it must at least reach the organisation it was given.
            let org = org_id.trim();
            if org.is_empty() {
                (Vec::new(), true)
            } else {
                let path = format!("/v1/org/{org}");
                check(send(&full, &key, HttpMethod::Get, sanitize_path(&path)?, &[], None).await?)?;
                (Vec::new(), true)
            }
        }
        _ => return Err(check(orgs).unwrap_err()),
    };

    state.secrets.set(GLOBAL, SECRET_API_KEY, &key)?;
    let mut settings = state.settings.get();
    settings.pangolin.api_url = base;
    settings.pangolin.base_path = base_path.to_string();
    settings.pangolin.org_id = org_id.trim().to_string();
    state.settings.set(settings)?;
    Ok(PangolinVerify { orgs, limited })
}

/// Forget the stored API key.
#[tauri::command]
#[specta::specta]
pub fn pangolin_clear(state: State<'_, AppState>) -> AppResult<()> {
    state.secrets.delete(GLOBAL, SECRET_API_KEY)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_sanitising() {
        assert!(sanitize_path("/v1/orgs").is_ok());
        assert!(sanitize_path("/v1/org/my-org/site-resources").is_ok());
        assert!(sanitize_path("/v1/client/12/block").is_ok());
        assert!(sanitize_path("/v2/orgs").is_err());
        assert!(sanitize_path("/v1/../admin").is_err());
        assert!(sanitize_path("/v1//x").is_err());
        assert!(sanitize_path("/v1/x?y=1").is_err());
        assert!(sanitize_path("/v1/x@evil.example").is_err());
        assert!(sanitize_path("https://evil.example/v1/").is_err());
    }

    #[test]
    fn base_url_normalising() {
        assert_eq!(normalize_base(" https://api.pangolin.net/ ").unwrap(), "https://api.pangolin.net");
        assert_eq!(normalize_base("http://10.0.0.5:3003").unwrap(), "http://10.0.0.5:3003");
        assert!(normalize_base("api.pangolin.net").is_err());
        assert!(normalize_base("https://").is_err());
        assert!(normalize_base("https://x/?a=1").is_err());
    }

    #[test]
    fn org_list_shapes() {
        let nested = r#"{"data":{"orgs":[{"orgId":"acme","name":"Acme Inc"},{"orgId":"solo"}]}}"#;
        let orgs = parse_orgs(nested);
        assert_eq!(orgs.len(), 2);
        assert_eq!((orgs[0].id.as_str(), orgs[0].name.as_str()), ("acme", "Acme Inc"));
        assert_eq!(orgs[1].name, "solo");
        assert_eq!(parse_orgs(r#"{"data":[{"orgId":"a","name":"A"}]}"#).len(), 1);
        assert!(parse_orgs("not json").is_empty());
    }

    #[test]
    fn error_messages_prefer_the_api_message() {
        assert_eq!(error_message(r#"{"message":"Key lacks permission","success":false}"#), "Key lacks permission");
        assert_eq!(error_message("plain text"), "plain text");
    }
}
