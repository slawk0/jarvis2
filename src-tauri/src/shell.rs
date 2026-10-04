//! Shell quoting, an argv-style command builder and input validators.
//!
//! Every user-derived string that reaches a remote shell goes through
//! [`q`] (directly or via [`Cmd`]) and, where the value has a known shape,
//! through one of the validators in [`validate`].

use crate::error::{AppError, AppResult};

/// POSIX single-quote a string so the shell sees it as exactly one word.
pub fn q(s: &str) -> String {
    let safe = !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"_@%+=:,./-".contains(&b));
    if safe {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

/// Builds one command line from separately quoted arguments.
///
/// `lit` takes only `&'static str`, so raw shell syntax can come from source
/// code but never from runtime data.
#[derive(Debug, Clone, Default)]
pub struct Cmd {
    parts: Vec<String>,
}

impl Cmd {
    pub fn new(program: &'static str) -> Self {
        Self { parts: vec![program.to_string()] }
    }

    /// Start from a dynamic prefix made of already-validated words
    /// (for example `docker` vs `sudo docker`, or a CrowdSec command prefix).
    pub fn from_words<I, S>(words: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Self { parts: words.into_iter().map(|w| q(w.as_ref())).collect() }
    }

    pub fn arg(mut self, value: impl AsRef<str>) -> Self {
        self.parts.push(q(value.as_ref()));
        self
    }

    pub fn args<I, S>(mut self, values: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.parts.extend(values.into_iter().map(|v| q(v.as_ref())));
        self
    }

    /// Unquoted literal shell syntax (flags, redirections, pipes).
    pub fn lit(mut self, raw: &'static str) -> Self {
        self.parts.push(raw.to_string());
        self
    }

    /// `--flag value` with the value quoted.
    pub fn opt(mut self, flag: &'static str, value: impl AsRef<str>) -> Self {
        self.parts.push(flag.to_string());
        self.parts.push(q(value.as_ref()));
        self
    }

    /// `--flag=value` with the value quoted.
    pub fn opt_eq(mut self, flag: &'static str, value: impl AsRef<str>) -> Self {
        self.parts.push(format!("{flag}={}", q(value.as_ref())));
        self
    }

    pub fn arg_if(self, cond: bool, value: impl AsRef<str>) -> Self {
        if cond {
            self.arg(value)
        } else {
            self
        }
    }

    pub fn lit_if(self, cond: bool, raw: &'static str) -> Self {
        if cond {
            self.lit(raw)
        } else {
            self
        }
    }

    pub fn opt_some(self, flag: &'static str, value: Option<impl AsRef<str>>) -> Self {
        match value {
            Some(v) if !v.as_ref().is_empty() => self.opt(flag, v),
            _ => self,
        }
    }

    pub fn build(&self) -> String {
        self.parts.join(" ")
    }
}

impl std::fmt::Display for Cmd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.build())
    }
}

pub mod validate {
    use super::*;
    use std::net::IpAddr;

    fn fail<T>(what: &str, value: &str) -> AppResult<T> {
        let shown: String = value.chars().take(80).collect();
        Err(AppError::invalid(format!("Invalid {what}: {shown:?}")))
    }

    fn all(value: &str, max: usize, ok: impl Fn(u8) -> bool) -> bool {
        !value.is_empty() && value.len() <= max && !value.starts_with('-') && value.bytes().all(ok)
    }

    /// Generic identifier: container, image, unit, package, network names.
    pub fn name<'a>(what: &str, value: &'a str) -> AppResult<&'a str> {
        if all(value, 255, |b| b.is_ascii_alphanumeric() || b"_.@:/+-".contains(&b)) {
            Ok(value)
        } else {
            fail(what, value)
        }
    }

    /// Stricter identifier without path separators (file-name safe).
    pub fn slug<'a>(what: &str, value: &'a str) -> AppResult<&'a str> {
        if all(value, 128, |b| b.is_ascii_alphanumeric() || b"_.@-".contains(&b)) && value != "." && value != ".." {
            Ok(value)
        } else {
            fail(what, value)
        }
    }

    pub fn unit(value: &str) -> AppResult<&str> {
        if all(value, 255, |b| b.is_ascii_alphanumeric() || b"_.@:\\-".contains(&b)) {
            Ok(value)
        } else {
            fail("unit name", value)
        }
    }

    pub fn username(value: &str) -> AppResult<&str> {
        let bytes = value.as_bytes();
        let first_ok = bytes.first().is_some_and(|b| b.is_ascii_lowercase() || *b == b'_');
        let rest_ok = bytes
            .iter()
            .enumerate()
            .all(|(i, b)| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_' || *b == b'-' || (*b == b'$' && i == bytes.len() - 1));
        if first_ok && rest_ok && value.len() <= 32 {
            Ok(value)
        } else {
            fail("user or group name", value)
        }
    }

    pub fn abs_path(value: &str) -> AppResult<&str> {
        if value.starts_with('/') && value.len() <= 4096 && !value.bytes().any(|b| b == 0 || b == b'\n' || b == b'\r') {
            Ok(value)
        } else {
            fail("absolute path", value)
        }
    }

    /// A single path component (no separators, not `.`/`..`).
    pub fn file_name(value: &str) -> AppResult<&str> {
        if !value.is_empty()
            && value.len() <= 255
            && value != "."
            && value != ".."
            && !value.bytes().any(|b| b == 0 || b == b'/' || b == b'\n')
        {
            Ok(value)
        } else {
            fail("file name", value)
        }
    }

    pub fn env_key(value: &str) -> AppResult<&str> {
        let mut bytes = value.bytes();
        let first = bytes.next();
        if first.is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
            && value.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            && value.len() <= 128
        {
            Ok(value)
        } else {
            fail("variable name", value)
        }
    }

    pub fn single_line<'a>(what: &str, value: &'a str) -> AppResult<&'a str> {
        if value.bytes().any(|b| b == 0 || b == b'\n' || b == b'\r') {
            fail(what, value)
        } else {
            Ok(value)
        }
    }

    pub fn ip(value: &str) -> AppResult<&str> {
        match value.parse::<IpAddr>() {
            Ok(_) => Ok(value),
            Err(_) => fail("IP address", value),
        }
    }

    pub fn ip_or_cidr(value: &str) -> AppResult<&str> {
        let (addr, prefix) = match value.split_once('/') {
            Some((a, p)) => (a, Some(p)),
            None => (value, None),
        };
        let Ok(parsed) = addr.parse::<IpAddr>() else {
            return fail("IP address or CIDR", value);
        };
        if let Some(prefix) = prefix {
            let max = if parsed.is_ipv4() { 32 } else { 128 };
            match prefix.parse::<u8>() {
                Ok(n) if n <= max => {}
                _ => return fail("CIDR prefix", value),
            }
        }
        Ok(value)
    }

    /// Hostname, domain (optionally wildcard) or IP address.
    pub fn host(value: &str) -> AppResult<&str> {
        if value.parse::<IpAddr>().is_ok() {
            return Ok(value);
        }
        let body = value.strip_prefix("*.").unwrap_or(value);
        let labels_ok = body.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        });
        if labels_ok && value.len() <= 253 {
            Ok(value)
        } else {
            fail("host name", value)
        }
    }

    pub fn port(value: u32) -> AppResult<u16> {
        match u16::try_from(value) {
            Ok(p) if p > 0 => Ok(p),
            _ => Err(AppError::invalid(format!("Invalid port: {value}"))),
        }
    }

    /// `80`, `8000:8100` or `8000-8100` (normalised to the given separator).
    pub fn port_or_range(value: &str, sep: char) -> AppResult<String> {
        let parts: Vec<&str> = value.split([':', '-']).collect();
        let parsed: Option<Vec<u16>> = parts.iter().map(|p| p.trim().parse::<u16>().ok().filter(|n| *n > 0)).collect();
        match parsed.as_deref() {
            Some([a]) => Ok(a.to_string()),
            Some([a, b]) if a <= b => Ok(format!("{a}{sep}{b}")),
            _ => fail("port or port range", value),
        }
    }

    pub fn email(value: &str) -> AppResult<&str> {
        let ok = value.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty()
                && local.bytes().all(|b| b.is_ascii_alphanumeric() || b"._%+-".contains(&b))
                && host(domain).is_ok()
                && domain.contains('.')
        });
        if ok {
            Ok(value)
        } else {
            fail("email address", value)
        }
    }

    /// Five-field cron expression or an `@keyword`.
    pub fn cron_expr(value: &str) -> AppResult<&str> {
        let v = value.trim();
        const KEYWORDS: [&str; 8] = ["@reboot", "@yearly", "@annually", "@monthly", "@weekly", "@daily", "@midnight", "@hourly"];
        if KEYWORDS.contains(&v) {
            return Ok(value);
        }
        let fields: Vec<&str> = v.split_whitespace().collect();
        const NAMES: [&str; 19] = [
            "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec", "sun", "mon", "tue", "wed", "thu", "fri",
            "sat",
        ];
        // One value: a number or a month/weekday name.
        let value_ok = |v: &str| {
            (!v.is_empty() && v.len() <= 4 && v.bytes().all(|b| b.is_ascii_digit())) || NAMES.contains(&v.to_ascii_lowercase().as_str())
        };
        // A list of `*`, `value`, `a-b`, each optionally with `/step`.
        let field_ok = |f: &&str| {
            f.split(',').all(|part| {
                let (range, step) = match part.split_once('/') {
                    Some((r, s)) => (r, Some(s)),
                    None => (part, None),
                };
                let step_ok = step.is_none_or(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()));
                let range_ok = range == "*"
                    || match range.split_once('-') {
                        Some((a, b)) => value_ok(a) && value_ok(b),
                        None => value_ok(range),
                    };
                step_ok && range_ok
            })
        };
        if fields.len() == 5 && fields.iter().all(field_ok) {
            Ok(value)
        } else {
            fail("cron expression", value)
        }
    }

    pub fn one_of<'a>(what: &str, value: &'a str, allowed: &[&str]) -> AppResult<&'a str> {
        if allowed.contains(&value) {
            Ok(value)
        } else {
            fail(what, value)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::validate as v;
    use super::*;

    #[test]
    fn quotes_only_when_needed() {
        assert_eq!(q("nginx.service"), "nginx.service");
        assert_eq!(q("/var/log/app-1.log"), "/var/log/app-1.log");
        assert_eq!(q(""), "''");
        assert_eq!(q("a b"), "'a b'");
        assert_eq!(q("it's"), "'it'\\''s'");
        assert_eq!(q("$(reboot)"), "'$(reboot)'");
        assert_eq!(q("a;b"), "'a;b'");
        assert_eq!(q("`id`"), "'`id`'");
        assert_eq!(q("line\nbreak"), "'line\nbreak'");
    }

    #[test]
    fn cmd_builder_quotes_args_but_not_literals() {
        let c = Cmd::new("docker")
            .lit("logs --tail")
            .arg("100")
            .arg("my app; rm -rf /")
            .opt("--since", "1 h")
            .opt_eq("--format", "{{json .}}")
            .lit("2>&1");
        assert_eq!(c.build(), "docker logs --tail 100 'my app; rm -rf /' --since '1 h' --format='{{json .}}' 2>&1");
    }

    #[test]
    fn cmd_from_words_quotes_each_word() {
        let c = Cmd::from_words(["sudo", "docker exec x"]).arg("ps");
        assert_eq!(c.build(), "sudo 'docker exec x' ps");
    }

    #[test]
    fn name_validators() {
        assert!(v::name("container", "web_1.prod").is_ok());
        assert!(v::name("image", "ghcr.io/org/app:1.2").is_ok());
        assert!(v::name("container", "-rf").is_err());
        assert!(v::name("container", "a b").is_err());
        assert!(v::name("container", "a;b").is_err());
        assert!(v::name("container", "").is_err());
        assert!(v::slug("id", "a/b").is_err());
        assert!(v::slug("id", "..").is_err());
        assert!(v::unit("getty@tty1.service").is_ok());
        assert!(v::unit("dev-disk-by\\x2duuid.swap").is_ok());
        assert!(v::unit("x y.service").is_err());
    }

    #[test]
    fn username_validator() {
        assert!(v::username("deploy").is_ok());
        assert!(v::username("_apt").is_ok());
        assert!(v::username("machine$").is_ok());
        assert!(v::username("Root").is_err());
        assert!(v::username("1abc").is_err());
        assert!(v::username("a$b").is_err());
        assert!(v::username("").is_err());
    }

    #[test]
    fn path_validators() {
        assert!(v::abs_path("/etc/nginx/nginx.conf").is_ok());
        assert!(v::abs_path("/with space/and 'quote'").is_ok());
        assert!(v::abs_path("relative").is_err());
        assert!(v::abs_path("/a\nb").is_err());
        assert!(v::file_name("a.txt").is_ok());
        assert!(v::file_name("a/b").is_err());
        assert!(v::file_name("..").is_err());
    }

    #[test]
    fn network_validators() {
        assert!(v::ip("10.0.0.1").is_ok());
        assert!(v::ip("::1").is_ok());
        assert!(v::ip("10.0.0").is_err());
        assert!(v::ip_or_cidr("10.0.0.0/8").is_ok());
        assert!(v::ip_or_cidr("2001:db8::/32").is_ok());
        assert!(v::ip_or_cidr("10.0.0.0/33").is_err());
        assert!(v::ip_or_cidr("example.com").is_err());
        assert!(v::host("example.com").is_ok());
        assert!(v::host("*.example.com").is_ok());
        assert!(v::host("exa mple.com").is_err());
        assert!(v::host("-bad.com").is_err());
        assert!(v::host("a;b").is_err());
        assert_eq!(v::port(22).unwrap(), 22);
        assert!(v::port(0).is_err());
        assert!(v::port(70000).is_err());
        assert_eq!(v::port_or_range("8000-8100", ':').unwrap(), "8000:8100");
        assert_eq!(v::port_or_range("443", ':').unwrap(), "443");
        assert!(v::port_or_range("9-1", ':').is_err());
        assert!(v::port_or_range("abc", ':').is_err());
        assert!(v::email("ops@example.com").is_ok());
        assert!(v::email("ops@localhost").is_err());
        assert!(v::email("a b@example.com").is_err());
    }

    #[test]
    fn misc_validators() {
        assert!(v::env_key("MY_VAR1").is_ok());
        assert!(v::env_key("1X").is_err());
        assert!(v::env_key("A-B").is_err());
        assert!(v::cron_expr("*/5 * * * *").is_ok());
        assert!(v::cron_expr("0 3 * * mon-fri").is_ok());
        assert!(v::cron_expr("@daily").is_ok());
        assert!(v::cron_expr("* * * *").is_err());
        assert!(v::cron_expr("* * * * * ; rm").is_err());
        assert!(v::cron_expr("$(x) * * * *").is_err());
        assert!(v::one_of("policy", "ACCEPT", &["ACCEPT", "DROP"]).is_ok());
        assert!(v::one_of("policy", "NOPE", &["ACCEPT", "DROP"]).is_err());
        assert!(v::single_line("command", "a\nb").is_err());
    }
}
