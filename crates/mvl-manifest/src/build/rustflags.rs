//! `CARGO_ENCODED_RUSTFLAGS` redaction.
//!
//! Two things never reach the binary: values that look like secrets, and the
//! `FROM` side of `--remap-path-prefix` (an absolute build path, which would
//! make the manifest depend on where the checkout lives).

const REDACTED: &str = "<redacted>";
const REMAP: &str = "--remap-path-prefix";

fn is_secret_key(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    [
        "token",
        "secret",
        "password",
        "passwd",
        "apikey",
        "api_key",
        "api-key",
        "auth",
        "credential",
    ]
    .iter()
    .any(|s| k.contains(s))
}

/// Replace `scheme://user:pass@host` userinfo.
fn redact_userinfo(s: &str) -> String {
    let Some(scheme_end) = s.find("://") else {
        return s.to_string();
    };
    let rest = &s[scheme_end + 3..];
    let host_end = rest.find('/').unwrap_or(rest.len());
    match rest[..host_end].rfind('@') {
        Some(at) => format!("{}{REDACTED}{}", &s[..scheme_end + 3], &rest[at..]),
        None => s.to_string(),
    }
}

/// Redact `key=value` segments (split on `,`) whose key looks secret.
fn redact_segment(arg: &str) -> String {
    let parts: Vec<String> = arg
        .split(',')
        .map(|seg| match seg.rsplit_once('=') {
            Some((k, _)) if is_secret_key(k) => {
                format!("{k}={REDACTED}")
            }
            _ => redact_userinfo(seg),
        })
        .collect();
    parts.join(",")
}

fn remap_value(v: &str) -> String {
    match v.split_once('=') {
        Some((_, to)) => format!("{REDACTED}={to}"),
        None => REDACTED.to_string(),
    }
}

pub fn redact(flags: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(flags.len());
    let mut remap_next = false;
    for f in flags {
        if remap_next {
            out.push(remap_value(f));
            remap_next = false;
        } else if f == REMAP {
            out.push(f.clone());
            remap_next = true;
        } else if let Some(v) = f.strip_prefix("--remap-path-prefix=") {
            out.push(format!("{REMAP}={}", remap_value(v)));
        } else {
            out.push(redact_segment(f));
        }
    }
    out
}

pub fn has_remap(flags: &[String]) -> bool {
    flags
        .iter()
        .any(|f| f == REMAP || f.starts_with("--remap-path-prefix="))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn remap_source_path_is_redacted_both_spellings() {
        assert_eq!(
            redact(&v(&[
                "--remap-path-prefix=/home/me/src=/build",
                "--remap-path-prefix",
                "/tmp/x=/y"
            ])),
            v(&[
                "--remap-path-prefix=<redacted>=/build",
                "--remap-path-prefix",
                "<redacted>=/y"
            ])
        );
    }

    #[test]
    fn secret_like_values_are_redacted() {
        assert_eq!(
            redact(&v(&[
                "-C",
                "link-arg=--api-token=hunter2",
                "--cfg",
                "feature=\"x\""
            ])),
            v(&[
                "-C",
                "link-arg=--api-token=<redacted>",
                "--cfg",
                "feature=\"x\""
            ])
        );
        assert_eq!(
            redact(&v(&["-Clink-args=-Wl,--secret=s3,-O1"])),
            v(&["-Clink-args=-Wl,--secret=<redacted>,-O1"])
        );
    }

    #[test]
    fn url_credentials_are_redacted() {
        assert_eq!(
            redact(&v(&["-Lhttps://user:pw@example.com/lib"])),
            v(&["-Lhttps://<redacted>@example.com/lib"])
        );
    }

    #[test]
    fn ordinary_flags_pass_through() {
        let f = v(&["-C", "target-cpu=native", "-Dwarnings"]);
        assert_eq!(redact(&f), f);
        assert!(!has_remap(&f));
        assert!(has_remap(&v(&["--remap-path-prefix=/a=/b"])));
    }
}
