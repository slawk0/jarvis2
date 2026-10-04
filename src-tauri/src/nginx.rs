//! Nginx Manager: Jarvis-managed proxy hosts, certbot certificates, config
//! files and service control, for nginx on the host or in a container.
//!
//! A proxy host is one generated config file whose first line carries its
//! settings as JSON (the metadata marker). Saving writes the file, runs
//! `nginx -t`, and rolls the file back if the test fails.

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};

use crate::docker::{in_target, ExecTarget};
use crate::error::{AppError, AppResult, ErrorCode};
use crate::jobs::JobMeta;
use crate::shell::{q, validate};
use crate::ssh::session::Session;
use crate::state::AppState;

const MARKER: &str = "# jarvis-proxy-host: ";
const FILE_PREFIX: &str = "jarvis-";
const CREDENTIALS_DIR: &str = "/etc/letsencrypt/jarvis";

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NginxTarget {
    pub target: ExecTarget,
    /// Usually `/etc/nginx`.
    pub config_root: String,
}

impl NginxTarget {
    fn root(&self) -> AppResult<&str> {
        let root = self.config_root.trim_end_matches('/');
        validate::abs_path(root)?;
        Ok(root)
    }

    /// Host installs use sites-available + a symlink; containers use conf.d.
    fn uses_sites(&self) -> bool {
        self.target.is_host()
    }

    fn host_file(&self, id: &str) -> AppResult<String> {
        let root = self.root()?;
        let dir = if self.uses_sites() { "sites-available" } else { "conf.d" };
        Ok(format!("{root}/{dir}/{FILE_PREFIX}{}.conf", validate::slug("proxy host id", id)?))
    }

    async fn run(&self, session: &Session, script: &str) -> AppResult<crate::ssh::session::Output> {
        session.exec(in_target(session, &self.target, script, true).await?.secs(90)).await
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProxySsl {
    pub enabled: bool,
    /// certbot certificate name (its `live/<name>` directory).
    pub certificate: String,
    pub force_https: bool,
    pub http2: bool,
    pub hsts: bool,
    pub hsts_subdomains: bool,
    pub hsts_preload: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProxyHost {
    pub id: String,
    pub domains: Vec<String>,
    /// `http` or `https` (how nginx talks to the upstream).
    pub scheme: String,
    pub forward_host: String,
    pub forward_port: u32,
    pub websockets: bool,
    pub block_exploits: bool,
    pub cache_assets: bool,
    pub ssl: ProxySsl,
    /// Extra directives placed inside the `location /` block.
    pub advanced: String,
    pub enabled: bool,
}

/// Generate the nginx config for a proxy host.
pub fn render_proxy_host(host: &ProxyHost) -> AppResult<String> {
    validate::slug("proxy host id", &host.id)?;
    if host.domains.is_empty() {
        return Err(AppError::invalid("Enter at least one domain name"));
    }
    for domain in &host.domains {
        validate::host(domain)?;
    }
    let scheme = validate::one_of("scheme", &host.scheme, &["http", "https"])?;
    let upstream_host = validate::host(host.forward_host.trim())?;
    let upstream = if upstream_host.contains(':') { format!("[{upstream_host}]") } else { upstream_host.to_string() };
    let port = validate::port(host.forward_port)?;
    if host.advanced.contains('\0') {
        return Err(AppError::invalid("Invalid custom configuration"));
    }
    if host.ssl.enabled {
        validate::name("certificate", &host.ssl.certificate)?;
    }

    // The marker never includes `enabled`: that is a property of the file's location.
    let mut meta = host.clone();
    meta.enabled = true;
    let names = host.domains.join(" ");
    let mut out = format!(
        "{MARKER}{}\n# Managed by Jarvis Server Manager. Edits made here are overwritten on the next save.\n",
        serde_json::to_string(&meta)?
    );

    let redirect = host.ssl.enabled && host.ssl.force_https;
    if redirect {
        out.push_str(&format!(
            "server {{\n    listen 80;\n    listen [::]:80;\n    server_name {names};\n\n    location /.well-known/acme-challenge/ {{ root /var/www/html; }}\n    location / {{ return 301 https://$host$request_uri; }}\n}}\n\n"
        ));
    }

    out.push_str("server {\n");
    if !redirect {
        out.push_str("    listen 80;\n    listen [::]:80;\n");
    }
    if host.ssl.enabled {
        let h2 = if host.ssl.http2 { " http2" } else { "" };
        out.push_str(&format!("    listen 443 ssl{h2};\n    listen [::]:443 ssl{h2};\n"));
    }
    out.push_str(&format!("    server_name {names};\n\n"));
    if host.ssl.enabled {
        let cert = &host.ssl.certificate;
        out.push_str(&format!(
            "    ssl_certificate /etc/letsencrypt/live/{cert}/fullchain.pem;\n    ssl_certificate_key /etc/letsencrypt/live/{cert}/privkey.pem;\n"
        ));
        if host.ssl.hsts {
            let mut value = String::from("max-age=63072000");
            if host.ssl.hsts_subdomains {
                value.push_str("; includeSubDomains");
            }
            if host.ssl.hsts_preload {
                value.push_str("; preload");
            }
            out.push_str(&format!("    add_header Strict-Transport-Security \"{value}\" always;\n"));
        }
        out.push('\n');
    }
    if !redirect {
        out.push_str("    location /.well-known/acme-challenge/ { root /var/www/html; }\n\n");
    }
    if host.block_exploits {
        out.push_str(
            "    # Block common exploit probes\n    location ~* \"(\\.\\./|\\.git/|\\.env$|/wp-config\\.php|/etc/passwd|<script|union.*select)\" { return 403; }\n\n",
        );
    }
    if host.cache_assets {
        out.push_str(&format!(
            "    location ~* \\.(?:css|js|jpg|jpeg|gif|png|svg|ico|webp|woff2?|ttf)$ {{\n        proxy_pass {scheme}://{upstream}:{port};\n        proxy_set_header Host $host;\n        proxy_set_header X-Forwarded-Proto $scheme;\n        expires 7d;\n        add_header Cache-Control \"public\";\n    }}\n\n"
        ));
    }
    out.push_str(&format!(
        "    location / {{\n        proxy_pass {scheme}://{upstream}:{port};\n        proxy_set_header Host $host;\n        proxy_set_header X-Real-IP $remote_addr;\n        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;\n        proxy_set_header X-Forwarded-Proto $scheme;\n"
    ));
    if host.websockets {
        out.push_str("        proxy_http_version 1.1;\n        proxy_set_header Upgrade $http_upgrade;\n        proxy_set_header Connection \"upgrade\";\n");
    }
    for line in host.advanced.lines().filter(|l| !l.trim().is_empty()) {
        out.push_str(&format!("        {}\n", line.trim()));
    }
    out.push_str("    }\n}\n");
    Ok(out)
}

/// Read a proxy host back from a generated file.
pub fn parse_proxy_host(content: &str, enabled: bool) -> Option<ProxyHost> {
    let json = content.lines().next()?.strip_prefix(MARKER)?;
    let mut host: ProxyHost = serde_json::from_str(json).ok()?;
    host.enabled = enabled;
    Some(host)
}

const FILE_MARK: &str = "#jarvis-file:";

fn split_files(out: &str) -> Vec<(String, String)> {
    let mut files: Vec<(String, String)> = Vec::new();
    for line in out.lines() {
        if let Some(path) = line.strip_prefix(FILE_MARK) {
            files.push((path.to_string(), String::new()));
        } else if let Some((_, content)) = files.last_mut() {
            content.push_str(line);
            content.push('\n');
        }
    }
    files
}

pub async fn list_hosts(session: &Session, target: &NginxTarget) -> AppResult<Vec<ProxyHost>> {
    let root = q(target.root()?);
    let script = if target.uses_sites() {
        // enabled = a sites-enabled entry with the same name exists
        format!(
            "for f in {root}/sites-available/{FILE_PREFIX}*.conf; do [ -f \"$f\" ] || continue; \
             n=$(basename \"$f\"); if [ -e {root}/sites-enabled/\"$n\" ]; then s=1; else s=0; fi; \
             echo \"{FILE_MARK}$s\"; cat \"$f\"; echo; done; true"
        )
    } else {
        format!(
            "for f in {root}/conf.d/{FILE_PREFIX}*.conf {root}/conf.d/{FILE_PREFIX}*.conf.disabled; do [ -f \"$f\" ] || continue; \
             case \"$f\" in *.disabled) s=0 ;; *) s=1 ;; esac; echo \"{FILE_MARK}$s\"; cat \"$f\"; echo; done; true"
        )
    };
    let out = target.run(session, &script).await?.into_stdout()?;
    let mut hosts: Vec<ProxyHost> =
        split_files(&out).into_iter().filter_map(|(state, content)| parse_proxy_host(&content, state == "1")).collect();
    hosts.sort_by(|a, b| a.domains.cmp(&b.domains));
    Ok(hosts)
}

fn reload_script() -> &'static str {
    "if command -v systemctl >/dev/null 2>&1 && systemctl is-active nginx >/dev/null 2>&1; then systemctl reload nginx; else nginx -s reload; fi"
}

/// Write a proxy host, test the configuration and reload; roll back on a failed test.
pub async fn save_host(session: &Session, target: &NginxTarget, host: &ProxyHost) -> AppResult<()> {
    let content = render_proxy_host(host)?;
    let file = target.host_file(&host.id)?;
    let root = q(target.root()?);
    let name = format!("{FILE_PREFIX}{}.conf", host.id);
    let (f, n) = (q(&file), q(&name));
    // Where the file must (not) be linked/named for the wanted state.
    let place = match (target.uses_sites(), host.enabled) {
        (true, true) => format!("ln -sfn {f} {root}/sites-enabled/{n}"),
        (true, false) => format!("rm -f {root}/sites-enabled/{n}"),
        (false, true) => format!("rm -f {f}.disabled"),
        (false, false) => format!("mv -f {f} {f}.disabled"),
    };
    let undo_place = match (target.uses_sites(), host.enabled) {
        (true, true) => format!("[ -n \"$had\" ] || rm -f {root}/sites-enabled/{n}"),
        (false, false) => format!("rm -f {f}.disabled"),
        _ => "true".to_string(),
    };
    // Exit 9 = `nginx -t` rejected the new config and the previous state was restored.
    let script = format!(
        "mkdir -p \"$(dirname {f})\" {root}/sites-enabled 2>/dev/null; \
         had=; [ -e {f} ] && {{ had=1; cp -p {f} {f}.jarvis-bak; }}; \
         cat > {f} || exit 1; {place}; \
         if out=$(nginx -t 2>&1); then rm -f {f}.jarvis-bak; {reload}; \
         else {undo_place}; if [ -n \"$had\" ]; then mv -f {f}.jarvis-bak {f}; else rm -f {f}; fi; printf '%s\\n' \"$out\"; exit 9; fi",
        reload = reload_script()
    );
    let exec = in_target(session, &target.target, &script, true).await?.stdin(content).secs(90);
    let out = session.exec(exec).await?;
    match out.code {
        0 => Ok(()),
        9 => Err(AppError::new(ErrorCode::ConfigTestFailed, out.stdout.trim())),
        _ => Err(out.into_stdout().unwrap_err()),
    }
}

// ---------------------------------------------------------------- certificates

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Certificate {
    pub name: String,
    pub domains: Vec<String>,
    /// As printed by certbot, e.g. `2026-12-01 10:00:00+00:00`.
    pub expiry: String,
    /// Negative when already expired; `None` if certbot did not say.
    pub days_left: Option<i32>,
    pub path: String,
}

/// Parse `certbot certificates`.
pub fn parse_certificates(text: &str) -> Vec<Certificate> {
    let mut certs: Vec<Certificate> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix("Certificate Name:") {
            certs.push(Certificate {
                name: name.trim().to_string(),
                domains: Vec::new(),
                expiry: String::new(),
                days_left: None,
                path: String::new(),
            });
            continue;
        }
        let Some(cert) = certs.last_mut() else { continue };
        if let Some(domains) = line.strip_prefix("Domains:") {
            cert.domains = domains.split_whitespace().map(str::to_string).collect();
        } else if let Some(expiry) = line.strip_prefix("Expiry Date:") {
            let (date, note) = expiry.split_once('(').unwrap_or((expiry, ""));
            cert.expiry = date.trim().to_string();
            cert.days_left = if note.contains("EXPIRED") {
                Some(-1)
            } else {
                note.split_whitespace().find_map(|w| w.parse::<i32>().ok()).or_else(|| note.contains("VALID").then_some(0))
            };
        } else if let Some(path) = line.strip_prefix("Certificate Path:") {
            cert.path = path.trim().to_string();
        }
    }
    certs
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(tag = "method", rename_all = "camelCase")]
pub enum IssueMethod {
    /// certbot's nginx plugin (edits nothing: `certonly`).
    Nginx,
    Webroot {
        path: String,
    },
    Standalone,
    DnsCloudflare,
    DnsDigitalocean,
    DnsRoute53,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IssueRequest {
    pub domains: Vec<String>,
    pub email: String,
    pub method: IssueMethod,
}

/// Keyring names of the DNS provider credentials.
pub const SECRET_CLOUDFLARE: &str = "nginx/dns/cloudflare-token";
pub const SECRET_DIGITALOCEAN: &str = "nginx/dns/digitalocean-token";
pub const SECRET_ROUTE53_KEY: &str = "nginx/dns/route53-access-key";
pub const SECRET_ROUTE53_SECRET: &str = "nginx/dns/route53-secret-key";

/// The certbot command (without credentials setup) for an issue request.
pub fn issue_command(request: &IssueRequest) -> AppResult<String> {
    if request.domains.is_empty() {
        return Err(AppError::invalid("Enter at least one domain name"));
    }
    let mut cmd = String::from("certbot certonly --non-interactive --agree-tos");
    cmd.push_str(&format!(" -m {}", q(validate::email(request.email.trim())?)));
    match &request.method {
        IssueMethod::Nginx => cmd.push_str(" --nginx"),
        IssueMethod::Webroot { path } => cmd.push_str(&format!(" --webroot -w {}", q(validate::abs_path(path.trim())?))),
        IssueMethod::Standalone => cmd.push_str(" --standalone"),
        IssueMethod::DnsCloudflare => {
            cmd.push_str(&format!(" --dns-cloudflare --dns-cloudflare-credentials {CREDENTIALS_DIR}/cloudflare.ini"))
        }
        IssueMethod::DnsDigitalocean => {
            cmd.push_str(&format!(" --dns-digitalocean --dns-digitalocean-credentials {CREDENTIALS_DIR}/digitalocean.ini"))
        }
        IssueMethod::DnsRoute53 => cmd.push_str(" --dns-route53"),
    }
    for domain in &request.domains {
        cmd.push_str(&format!(" -d {}", q(validate::host(domain.trim())?)));
    }
    Ok(cmd)
}

fn secret(state: &AppState, profile: &str, name: &str, what: &str) -> AppResult<String> {
    state.secrets.get(profile, name)?.filter(|s| !s.trim().is_empty()).ok_or_else(|| AppError::invalid(format!("Enter the {what} first")))
}

/// Write the provider credentials on the server (mode 600) and return the
/// shell prefix that makes them available to certbot.
async fn prepare_credentials(state: &AppState, session: &Session, target: &NginxTarget, method: &IssueMethod) -> AppResult<String> {
    let profile = &session.profile.id;
    let (file, content) = match method {
        IssueMethod::DnsCloudflare => (
            "cloudflare.ini",
            format!("dns_cloudflare_api_token = {}\n", secret(state, profile, SECRET_CLOUDFLARE, "Cloudflare API token")?),
        ),
        IssueMethod::DnsDigitalocean => (
            "digitalocean.ini",
            format!("dns_digitalocean_token = {}\n", secret(state, profile, SECRET_DIGITALOCEAN, "DigitalOcean API token")?),
        ),
        IssueMethod::DnsRoute53 => (
            "route53.env",
            format!(
                "AWS_ACCESS_KEY_ID={}\nAWS_SECRET_ACCESS_KEY={}\n",
                secret(state, profile, SECRET_ROUTE53_KEY, "AWS access key ID")?,
                secret(state, profile, SECRET_ROUTE53_SECRET, "AWS secret access key")?
            ),
        ),
        _ => return Ok(String::new()),
    };
    if content.matches('\n').count() != content.lines().count() {
        return Err(AppError::invalid("Credentials cannot contain line breaks"));
    }
    let path = format!("{CREDENTIALS_DIR}/{file}");
    // The file is created empty with mode 600 before the secret is written into it.
    let script = format!(
        "mkdir -p {CREDENTIALS_DIR} && chmod 700 {CREDENTIALS_DIR} && (umask 077; : > {p}) && chmod 600 {p} && cat > {p}",
        p = q(&path)
    );
    let exec = in_target(session, &target.target, &script, true).await?.stdin(content);
    session.exec(exec).await?.into_stdout()?;
    Ok(if matches!(method, IssueMethod::DnsRoute53) { format!("set -a; . {}; set +a; ", q(&path)) } else { String::new() })
}

async fn certbot_job(
    app: &AppHandle,
    state: &AppState,
    target: &NginxTarget,
    title: String,
    script: String,
    shown: String,
) -> AppResult<String> {
    let session = state.session()?;
    let exec = in_target(&session, &target.target, &format!("{script} 2>&1"), true).await?;
    state.jobs.start(app, session, JobMeta::visible(title, shown), exec).await
}

// ---------------------------------------------------------------- commands

#[tauri::command]
#[specta::specta]
pub async fn nginx_hosts(state: State<'_, AppState>, target: NginxTarget) -> AppResult<Vec<ProxyHost>> {
    let session = state.session()?;
    list_hosts(&session, &target).await
}

#[tauri::command]
#[specta::specta]
pub async fn nginx_host_save(state: State<'_, AppState>, target: NginxTarget, host: ProxyHost) -> AppResult<()> {
    let session = state.session()?;
    save_host(&session, &target, &host).await
}

#[tauri::command]
#[specta::specta]
pub async fn nginx_host_delete(state: State<'_, AppState>, target: NginxTarget, id: String) -> AppResult<()> {
    let session = state.session()?;
    let file = q(&target.host_file(&id)?);
    let root = q(target.root()?);
    let name = q(&format!("{FILE_PREFIX}{id}.conf"));
    let script =
        format!("rm -f {root}/sites-enabled/{name} {file} {file}.disabled; nginx -t >/dev/null 2>&1 && {{ {}; }}; true", reload_script());
    target.run(&session, &script).await?.into_stdout()?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn nginx_certificates(state: State<'_, AppState>, target: NginxTarget) -> AppResult<Vec<Certificate>> {
    let session = state.session()?;
    let out = target.run(&session, "certbot certificates 2>/dev/null").await?;
    if out.code == 127 {
        return Err(AppError::new(ErrorCode::DependencyMissing, "certbot"));
    }
    Ok(parse_certificates(&out.stdout))
}

/// Request a new certificate (streamed job).
#[tauri::command]
#[specta::specta]
pub async fn nginx_cert_issue(app: AppHandle, state: State<'_, AppState>, target: NginxTarget, request: IssueRequest) -> AppResult<String> {
    let session = state.session()?;
    let command = issue_command(&request)?;
    if target.target.is_host() {
        // Ask for the sudo password before any credentials are written.
        session.sudo_plan().await?;
    }
    let prefix = prepare_credentials(&state, &session, &target, &request.method).await?;
    let title = format!("Issue certificate · {}", request.domains.join(", "));
    certbot_job(&app, &state, &target, title, format!("{prefix}{command}"), command).await
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum CertAction {
    Renew { name: String, force: bool },
    RenewAll,
    DryRun,
    Delete { name: String },
}

/// Renew / dry-run / delete certificates (streamed job).
#[tauri::command]
#[specta::specta]
pub async fn nginx_cert_action(app: AppHandle, state: State<'_, AppState>, target: NginxTarget, action: CertAction) -> AppResult<String> {
    // Route 53 credentials come from the environment file, if one was written.
    let env = format!("[ -f {CREDENTIALS_DIR}/route53.env ] && {{ set -a; . {CREDENTIALS_DIR}/route53.env; set +a; }}; ");
    let (title, command) = match &action {
        CertAction::Renew { name, force } => (
            format!("Renew certificate · {name}"),
            format!(
                "certbot renew --non-interactive --cert-name {}{}",
                q(validate::name("certificate", name)?),
                if *force { " --force-renewal" } else { "" }
            ),
        ),
        CertAction::RenewAll => ("Renew all certificates".to_string(), "certbot renew --non-interactive".to_string()),
        CertAction::DryRun => ("Certificate renewal dry run".to_string(), "certbot renew --dry-run --non-interactive".to_string()),
        CertAction::Delete { name } => (
            format!("Delete certificate · {name}"),
            format!("certbot delete --non-interactive --cert-name {}", q(validate::name("certificate", name)?)),
        ),
    };
    certbot_job(&app, &state, &target, title, format!("{env}{command}"), command).await
}

/// Config files under the nginx root that can be edited.
#[tauri::command]
#[specta::specta]
pub async fn nginx_files(state: State<'_, AppState>, target: NginxTarget) -> AppResult<Vec<String>> {
    let session = state.session()?;
    let root = q(target.root()?);
    let out = target
        .run(
            &session,
            &format!(
                "[ -f {root}/nginx.conf ] && echo {root}/nginx.conf; \
                 for d in sites-available sites-enabled conf.d snippets; do \
                   for f in {root}/$d/*; do [ -f \"$f\" ] && echo \"$f\"; done; done; true"
            ),
        )
        .await?
        .into_stdout()?;
    Ok(out.lines().map(str::to_string).filter(|l| !l.is_empty()).collect())
}

fn config_path<'a>(target: &NginxTarget, path: &'a str) -> AppResult<&'a str> {
    let root = target.root()?;
    validate::abs_path(path)?;
    if path.starts_with(&format!("{root}/")) && !path.contains("/../") {
        Ok(path)
    } else {
        Err(AppError::invalid("The file is outside the nginx configuration directory"))
    }
}

#[tauri::command]
#[specta::specta]
pub async fn nginx_file_read(state: State<'_, AppState>, target: NginxTarget, path: String) -> AppResult<String> {
    let session = state.session()?;
    let path = config_path(&target, &path)?;
    target.run(&session, &format!("cat -- {}", q(path))).await?.into_stdout()
}

/// Save a config file, test and reload. A failed test restores the old content.
#[tauri::command]
#[specta::specta]
pub async fn nginx_file_write(state: State<'_, AppState>, target: NginxTarget, path: String, content: String) -> AppResult<()> {
    let session = state.session()?;
    let f = q(config_path(&target, &path)?);
    let script = format!(
        "cp -p {f} {f}.jarvis-bak 2>/dev/null; cat > {f} || exit 1; \
         if out=$(nginx -t 2>&1); then rm -f {f}.jarvis-bak; {reload}; \
         else [ -e {f}.jarvis-bak ] && mv -f {f}.jarvis-bak {f}; printf '%s\\n' \"$out\"; exit 9; fi",
        reload = reload_script()
    );
    let exec = in_target(&session, &target.target, &script, true).await?.stdin(content).secs(90);
    let out = session.exec(exec).await?;
    match out.code {
        0 => Ok(()),
        9 => Err(AppError::new(ErrorCode::ConfigTestFailed, out.stdout.trim())),
        _ => Err(out.into_stdout().unwrap_err()),
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum NginxControl {
    Test,
    Reload,
    Restart,
    Status,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ControlResult {
    pub ok: bool,
    pub output: String,
}

#[tauri::command]
#[specta::specta]
pub async fn nginx_control(state: State<'_, AppState>, target: NginxTarget, action: NginxControl) -> AppResult<ControlResult> {
    let session = state.session()?;
    let script = match (action, target.target.is_host()) {
        (NginxControl::Test, _) => "nginx -t 2>&1".to_string(),
        (NginxControl::Reload, _) => format!("nginx -t 2>&1 && {{ {}; }} 2>&1 && echo 'Reloaded.'", reload_script()),
        (NginxControl::Restart, true) => "nginx -t 2>&1 && systemctl restart nginx 2>&1 && echo 'Restarted.'".to_string(),
        (NginxControl::Status, true) => "systemctl status nginx --no-pager -l -n 20 2>&1; nginx -v 2>&1".to_string(),
        (NginxControl::Status, false) => {
            "nginx -v 2>&1; echo; nginx -T 2>/dev/null | grep -c server_name | sed 's/^/server_name directives: /'".to_string()
        }
        (NginxControl::Restart, false) => {
            // Restarting nginx inside a container means restarting the container.
            let ExecTarget::Container { container } = &target.target else { unreachable!() };
            let line = format!("docker restart {} 2>&1 && echo 'Container restarted.'", q(validate::name("container", container)?));
            let out = session.exec(crate::docker::exec(&session, line).await?.secs(120)).await?;
            return Ok(ControlResult { ok: out.success(), output: out.combined() });
        }
    };
    let out = target.run(&session, &script).await?;
    Ok(ControlResult { ok: out.success() || matches!(action, NginxControl::Status), output: out.combined() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host() -> ProxyHost {
        ProxyHost {
            id: "h1".into(),
            domains: vec!["app.example.com".into(), "www.app.example.com".into()],
            scheme: "http".into(),
            forward_host: "127.0.0.1".into(),
            forward_port: 3000,
            websockets: true,
            block_exploits: false,
            cache_assets: false,
            ssl: ProxySsl {
                enabled: false,
                certificate: String::new(),
                force_https: false,
                http2: false,
                hsts: false,
                hsts_subdomains: false,
                hsts_preload: false,
            },
            advanced: String::new(),
            enabled: true,
        }
    }

    #[test]
    fn renders_plain_http_host() {
        let config = render_proxy_host(&host()).unwrap();
        let mut lines = config.lines();
        assert!(lines.next().unwrap().starts_with(MARKER));
        assert!(config.contains("    listen 80;\n    listen [::]:80;\n    server_name app.example.com www.app.example.com;\n"));
        assert!(config.contains("        proxy_pass http://127.0.0.1:3000;\n"));
        assert!(config.contains("proxy_set_header Upgrade $http_upgrade;"));
        assert!(!config.contains("443"));
        assert_eq!(config.matches("server {").count(), 1);
    }

    #[test]
    fn renders_ssl_with_redirect_and_hsts() {
        let mut h = host();
        h.ssl = ProxySsl {
            enabled: true,
            certificate: "app.example.com".into(),
            force_https: true,
            http2: true,
            hsts: true,
            hsts_subdomains: true,
            hsts_preload: false,
        };
        h.advanced = "client_max_body_size 50m;\n\n  proxy_read_timeout 300;".into();
        h.cache_assets = true;
        h.block_exploits = true;
        let config = render_proxy_host(&h).unwrap();
        assert_eq!(config.matches("server {").count(), 2);
        assert!(config.contains("location / { return 301 https://$host$request_uri; }"));
        assert!(config.contains("    listen 443 ssl http2;\n"));
        assert!(config.contains("ssl_certificate /etc/letsencrypt/live/app.example.com/fullchain.pem;"));
        assert!(config.contains("add_header Strict-Transport-Security \"max-age=63072000; includeSubDomains\" always;"));
        assert!(config.contains("        client_max_body_size 50m;\n        proxy_read_timeout 300;\n"));
        assert!(config.contains("expires 7d;"));
        assert!(config.contains("return 403;"));
    }

    #[test]
    fn marker_round_trips() {
        let mut h = host();
        h.advanced = "# a \"quoted\" comment".into();
        let config = render_proxy_host(&h).unwrap();
        assert_eq!(parse_proxy_host(&config, true).unwrap(), h);
        let disabled = parse_proxy_host(&config, false).unwrap();
        assert!(!disabled.enabled);
        assert!(parse_proxy_host("server { }", true).is_none());
    }

    #[test]
    fn rejects_invalid_hosts() {
        let mut h = host();
        h.domains = vec!["bad domain;".into()];
        assert!(render_proxy_host(&h).is_err());
        let mut h = host();
        h.domains.clear();
        assert!(render_proxy_host(&h).is_err());
        let mut h = host();
        h.forward_host = "127.0.0.1; return 200".into();
        assert!(render_proxy_host(&h).is_err());
        let mut h = host();
        h.forward_port = 0;
        assert!(render_proxy_host(&h).is_err());
        let mut h = host();
        h.id = "../x".into();
        assert!(render_proxy_host(&h).is_err());
    }

    #[test]
    fn parses_certbot_certificates() {
        let text = "Saving debug log to /var/log/letsencrypt/letsencrypt.log\n\n- - - - -\nFound the following certs:\n  Certificate Name: example.com\n    Serial Number: 4a\n    Key Type: ECDSA\n    Domains: example.com www.example.com\n    Expiry Date: 2026-12-01 10:00:00+00:00 (VALID: 59 days)\n    Certificate Path: /etc/letsencrypt/live/example.com/fullchain.pem\n    Private Key Path: /etc/letsencrypt/live/example.com/privkey.pem\n  Certificate Name: old.example.com\n    Domains: old.example.com\n    Expiry Date: 2026-01-01 10:00:00+00:00 (INVALID: EXPIRED)\n    Certificate Path: /etc/letsencrypt/live/old.example.com/fullchain.pem\n";
        let certs = parse_certificates(text);
        assert_eq!(certs.len(), 2);
        assert_eq!(certs[0].domains, vec!["example.com", "www.example.com"]);
        assert_eq!(certs[0].days_left, Some(59));
        assert_eq!(certs[0].expiry, "2026-12-01 10:00:00+00:00");
        assert_eq!(certs[1].days_left, Some(-1));
        assert!(parse_certificates("No certificates found.\n").is_empty());
    }

    #[test]
    fn certbot_issue_commands() {
        let request =
            |method| IssueRequest { domains: vec!["example.com".into(), "*.example.com".into()], email: "ops@example.com".into(), method };
        assert_eq!(
            issue_command(&request(IssueMethod::Nginx)).unwrap(),
            "certbot certonly --non-interactive --agree-tos -m ops@example.com --nginx -d example.com -d '*.example.com'"
        );
        assert!(issue_command(&request(IssueMethod::Webroot { path: "/var/www/html".into() }))
            .unwrap()
            .contains("--webroot -w /var/www/html"));
        assert!(issue_command(&request(IssueMethod::DnsCloudflare))
            .unwrap()
            .contains("--dns-cloudflare-credentials /etc/letsencrypt/jarvis/cloudflare.ini"));
        assert!(issue_command(&request(IssueMethod::Webroot { path: "relative".into() })).is_err());
        let mut bad = request(IssueMethod::Standalone);
        bad.email = "not an email".into();
        assert!(issue_command(&bad).is_err());
        bad = request(IssueMethod::Standalone);
        bad.domains = vec!["a b".into()];
        assert!(issue_command(&bad).is_err());
    }

    #[test]
    fn config_paths_stay_inside_the_root() {
        let target = NginxTarget { target: ExecTarget::Host, config_root: "/etc/nginx/".into() };
        assert!(config_path(&target, "/etc/nginx/sites-available/default").is_ok());
        assert!(config_path(&target, "/etc/passwd").is_err());
        assert!(config_path(&target, "/etc/nginx/../passwd").is_err());
        assert_eq!(target.host_file("abc").unwrap(), "/etc/nginx/sites-available/jarvis-abc.conf");
        let container = NginxTarget { target: ExecTarget::Container { container: "web".into() }, config_root: "/etc/nginx".into() };
        assert_eq!(container.host_file("abc").unwrap(), "/etc/nginx/conf.d/jarvis-abc.conf");
    }
}
