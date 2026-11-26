//! "Copy diagnostics" report (SHL-14): environment facts plus the recent redacted log, as plain text the user can
//! paste into a bug report. The whole report passes through redaction again before it is returned.

use crate::logging::redact_for_sharing;
use std::path::Path;

pub const LOG_TAIL_LINES: usize = 200;

pub struct DiagnosticsInput<'a> {
    pub version: &'a str,
    pub os: &'a str,
    pub arch: &'a str,
    pub display_server: &'a str,
    pub browser: String,
    pub browser_check: String,
    pub secrets_backend: String,
    pub schema_version: Option<u32>,
    pub data_dir: &'a Path,
    pub log_dir: &'a Path,
    pub log_tail: Vec<String>,
    pub home: Option<&'a Path>,
}

pub fn build(i: &DiagnosticsInput) -> String {
    let schema = i.schema_version.map(|v| v.to_string()).unwrap_or_else(|| "unknown".into());
    let mut out = format!(
        "Social Growth Helper diagnostics\n\
         version: {}\n\
         platform: {} {}\n\
         display server: {}\n\
         browser: {}\n\
         browser check: {}\n\
         secret storage: {}\n\
         database schema: v{}\n\
         data dir: {}\n\
         log dir: {}\n\n\
         --- last {} log lines (redacted) ---\n",
        i.version,
        i.os,
        i.arch,
        i.display_server,
        i.browser,
        i.browser_check,
        i.secrets_backend,
        schema,
        i.data_dir.display(),
        i.log_dir.display(),
        i.log_tail.len(),
    );
    for line in &i.log_tail {
        out.push_str(line);
        out.push('\n');
    }
    redact_for_sharing(&out, i.home)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input<'a>(tail: Vec<String>, dir: &'a Path) -> DiagnosticsInput<'a> {
        DiagnosticsInput {
            version: "0.1.0",
            os: "linux",
            arch: "x86_64",
            display_server: "wayland",
            browser: "chrome at /home/pavel/bin/chrome".into(),
            browser_check: "ok (157.0.8103.50)".into(),
            secrets_backend: "Secret Service".into(),
            schema_version: Some(2),
            data_dir: dir,
            log_dir: dir,
            log_tail: tail,
            home: Some(Path::new("/home/pavel")),
        }
    }

    #[test]
    fn report_has_the_environment_and_no_secrets() {
        let dir = Path::new("/home/pavel/.local/share/social-growth-helper");
        let r = build(&input(
            vec![
                "INFO launched".into(),
                "WARN refresh failed access_token=abc123def456 for user".into(),
                r#"INFO draft text="my private reply""#.into(),
            ],
            dir,
        ));
        for needle in ["version: 0.1.0", "display server: wayland", "database schema: v2", "browser check: ok (157.0.8103.50)"] {
            assert!(r.contains(needle), "missing {needle}\n{r}");
        }
        assert!(!r.contains("abc123def456") && !r.contains("my private reply"), "{r}");
        assert!(!r.contains("/home/pavel"), "home dir must be collapsed: {r}");
        assert!(r.contains("~/.local/share/social-growth-helper"));
        assert!(r.contains("INFO launched"));
    }
}
