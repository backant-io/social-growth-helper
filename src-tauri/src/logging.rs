//! Local rotating logs with redaction (SHL-14). Logs never leave the machine, but they get pasted into bug
//! reports, so secrets and post text are scrubbed before a line is written. Callers should still avoid logging
//! such values; [`redact`] is the backstop, not a license.

use log::{Level, LevelFilter, Log, Metadata, Record};
use regex::{Captures, Regex};
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub const LOG_FILE: &str = "app.log";
const MAX_BYTES: u64 = 1024 * 1024;
const KEEP_ROTATED: usize = 4;
const REDACTED: &str = "[REDACTED]";

struct Rules {
    line_secret: Regex,
    bearer: Regex,
    key_value: Regex,
    known_token: Regex,
    quoted_text: Regex,
}

fn rules() -> &'static Rules {
    static RULES: OnceLock<Rules> = OnceLock::new();
    RULES.get_or_init(|| Rules {
        line_secret: Regex::new(r"(?i)\b(set-cookie|cookie|authorization)(\s*[:=]\s*)[^\r\n]*").unwrap(),
        bearer: Regex::new(r"(?i)\bbearer\s+[A-Za-z0-9._~+/=-]{8,}").unwrap(),
        key_value: Regex::new(
            r#"(?i)\b(access[_-]?token|refresh[_-]?token|auth[_-]?token|id[_-]?token|token|client[_-]?secret|secret|password|passwd|api[_-]?key|apikey|ct0|session[_-]?id)(["']?\s*[:=]\s*)("[^"]*"|'[^']*'|[^\s,;&}\])]+)"#,
        )
        .unwrap(),
        known_token: Regex::new(
            r"\b(sk-[A-Za-z0-9_-]{16,}|gh[pousr]_[A-Za-z0-9]{20,}|xox[abprs]-[A-Za-z0-9-]{10,}|AAAAAAAA[A-Za-z0-9%]{20,}|eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,})",
        )
        .unwrap(),
        quoted_text: Regex::new(
            r#"(?i)\b(text|draft|body|content|tweet|reply|prompt|message)(["']?\s*[:=]\s*)("(?:[^"\\]|\\.)*"|'[^']*')"#,
        )
        .unwrap(),
    })
}

/// Removes credentials (tokens, cookies, Authorization headers, API keys) and quoted post/draft/prompt text.
pub fn redact(input: &str) -> String {
    let r = rules();
    let s = r.line_secret.replace_all(input, |c: &Captures| format!("{}{}{REDACTED}", &c[1], &c[2]));
    let s = r.bearer.replace_all(&s, format!("Bearer {REDACTED}"));
    let s = r.key_value.replace_all(&s, |c: &Captures| format!("{}{}{REDACTED}", &c[1], &c[2]));
    let s = r.known_token.replace_all(&s, REDACTED);
    let s = r.quoted_text.replace_all(&s, |c: &Captures| {
        let chars = c[3].chars().count().saturating_sub(2);
        format!("{}{}\"[text redacted, {chars} chars]\"", &c[1], &c[2])
    });
    s.into_owned()
}

/// Same as [`redact`] plus the user's home directory collapsed to `~`, for text that is shared outside the machine.
pub fn redact_for_sharing(input: &str, home: Option<&Path>) -> String {
    let s = redact(input);
    match home.map(|h| h.display().to_string()).filter(|h| h.len() > 1) {
        Some(h) => s.replace(&h, "~"),
        None => s,
    }
}

/// Size-capped log file that keeps a few numbered generations (`app.log`, `app.log.1`, ...).
pub struct RotatingFile {
    path: PathBuf,
    max_bytes: u64,
    keep: usize,
    file: File,
    size: u64,
}

impl RotatingFile {
    pub fn open(dir: &Path, name: &str, max_bytes: u64, keep: usize) -> io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(name);
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        let size = file.metadata()?.len();
        Ok(Self { path, max_bytes, keep, file, size })
    }

    fn generation(&self, n: usize) -> PathBuf {
        let mut s = self.path.clone().into_os_string();
        s.push(format!(".{n}"));
        PathBuf::from(s)
    }

    fn rotate(&mut self) -> io::Result<()> {
        let _ = std::fs::remove_file(self.generation(self.keep));
        for n in (1..self.keep).rev() {
            let _ = std::fs::rename(self.generation(n), self.generation(n + 1));
        }
        std::fs::rename(&self.path, self.generation(1))?;
        self.file = OpenOptions::new().create(true).append(true).open(&self.path)?;
        self.size = 0;
        Ok(())
    }

    pub fn write_line(&mut self, line: &str) -> io::Result<()> {
        let bytes = line.len() as u64 + 1;
        if self.size > 0 && self.size + bytes > self.max_bytes {
            self.rotate()?;
        }
        writeln!(self.file, "{line}")?;
        self.size += bytes;
        Ok(())
    }
}

struct FileLogger {
    sink: Mutex<RotatingFile>,
}

impl Log for FileLogger {
    fn enabled(&self, m: &Metadata) -> bool {
        m.level() <= Level::Info
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = redact(&format!(
            "{} {:<5} {} {}",
            crate::util::now_rfc3339(),
            record.level(),
            record.target(),
            record.args()
        ));
        eprintln!("{line}");
        if let Ok(mut sink) = self.sink.lock() {
            let _ = sink.write_line(&line);
        }
    }

    fn flush(&self) {
        if let Ok(mut sink) = self.sink.lock() {
            let _ = sink.file.flush();
        }
    }
}

/// Installs the file logger and a panic hook that records panics before the default handler runs. Idempotent.
pub fn init(dir: &Path) -> io::Result<()> {
    let sink = RotatingFile::open(dir, LOG_FILE, MAX_BYTES, KEEP_ROTATED)?;
    if log::set_boxed_logger(Box::new(FileLogger { sink: Mutex::new(sink) })).is_ok() {
        log::set_max_level(LevelFilter::Info);
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            log::error!("panic: {info}");
            previous(info);
        }));
    }
    Ok(())
}

/// Last `n` log lines, oldest first, reading the previous generation when the current file is short.
pub fn tail(dir: &Path, n: usize) -> Vec<String> {
    let read = |name: String| std::fs::read_to_string(dir.join(name)).unwrap_or_default();
    let mut lines: Vec<String> = read(format!("{LOG_FILE}.1")).lines().map(str::to_string).collect();
    lines.extend(read(LOG_FILE.to_string()).lines().map(str::to_string));
    let skip = lines.len().saturating_sub(n);
    lines.split_off(skip)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_never_survive() {
        let samples = [
            ("Authorization: Bearer abcdef1234567890", "abcdef1234567890"),
            ("GET /cb?access_token=s3cr3tvalue&x=1", "s3cr3tvalue"),
            ("token: \"hunter2hunter2\"", "hunter2hunter2"),
            ("cookie: auth_token=aaaa1111; ct0=bbbb2222", "aaaa1111"),
            ("Set-Cookie: ct0=zzzz9999; Path=/", "zzzz9999"),
            ("key sk-ant-abcdefghijklmnopqrstu used", "abcdefghijklmnopqrstu"),
            ("ghp_abcdefghijklmnopqrstuvwxyz0123456789 leaked", "ghp_abcdefghijklmnopqrstuvwxyz"),
            ("jwt eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.abcdefghijk", "eyJhbGciOiJIUzI1NiJ9"),
            ("{\"api_key\":\"k-12345678\"}", "k-12345678"),
            ("password=p@ss word", "p@ss"),
        ];
        for (input, secret) in samples {
            let out = redact(input);
            assert!(!out.contains(secret), "{input:?} leaked {secret:?} -> {out:?}");
            assert!(out.contains(REDACTED), "{out:?}");
        }
    }

    #[test]
    fn post_and_prompt_text_is_replaced_by_its_length() {
        let out = redact(r#"drafted reply text="Great thread, totally agree!" for post 1"#);
        assert!(!out.contains("Great thread"), "{out}");
        assert!(out.contains("text redacted, 28 chars"), "{out}");
        assert!(out.contains("for post 1"));
        assert!(!redact(r#"Draft { body: "secret plans" }"#).contains("secret plans"));
    }

    #[test]
    fn ordinary_lines_are_untouched() {
        for line in [
            "2026-11-26T10:00:00Z INFO app migrated database v1 -> v2",
            "closing X browser on quit: browser not connected",
            "tokenizer loaded in 12ms",
        ] {
            assert_eq!(redact(line), line);
        }
    }

    #[test]
    fn sharing_collapses_the_home_directory() {
        let out = redact_for_sharing("db at /home/pavel/.local/share/x/app.db", Some(Path::new("/home/pavel")));
        assert_eq!(out, "db at ~/.local/share/x/app.db");
        assert_eq!(redact_for_sharing("a /b", Some(Path::new("/"))), "a /b");
    }

    #[test]
    fn rotation_caps_size_and_keeps_numbered_generations() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = RotatingFile::open(dir.path(), "t.log", 100, 2).unwrap();
        for i in 0..40 {
            f.write_line(&format!("line-{i:02}-xxxxxxxxxxxxxxxxxxxx")).unwrap();
        }
        let size = |n: &str| std::fs::metadata(dir.path().join(n)).map(|m| m.len()).ok();
        assert!(size("t.log").unwrap() <= 100);
        assert!(size("t.log.1").is_some() && size("t.log.2").is_some());
        assert!(size("t.log.3").is_none(), "oldest generation is dropped");
        let newest = std::fs::read_to_string(dir.path().join("t.log")).unwrap();
        assert!(newest.contains("line-39"));
    }

    #[test]
    fn reopening_appends_and_tail_spans_generations() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut f = RotatingFile::open(dir.path(), LOG_FILE, 60, 3).unwrap();
            for i in 0..4 {
                f.write_line(&format!("entry-{i}-pad-pad-pad-pad")).unwrap();
            }
        }
        let mut f = RotatingFile::open(dir.path(), LOG_FILE, 60, 3).unwrap();
        f.write_line("entry-4-after-reopen").unwrap();
        let t = tail(dir.path(), 3);
        assert_eq!(t.len(), 3);
        assert!(t.last().unwrap().contains("after-reopen"));
        assert!(tail(dir.path(), 1000).len() <= 5);
        assert!(tail(&dir.path().join("none"), 5).is_empty());
    }
}
