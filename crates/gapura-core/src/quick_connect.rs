//! Parse an `ssh` command line into a connection draft (PRD-adjacent QoL
//! feature). Handles the flags a connection manager cares about —
//! `user@host`, `-p`, `-i`, `-l` — and reports everything else as warnings
//! instead of guessing.

use serde::{Deserialize, Serialize};

/// A connection draft parsed from an ssh command line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SshCommandDraft {
    pub username: Option<String>,
    pub host: String,
    pub port: Option<u16>,
    pub identity_path: Option<String>,
    /// Flags that were understood but not applied, shown in the review UI.
    pub warnings: Vec<String>,
}

/// Parse something like
/// `ssh -i ~/keys/aws.pem -p 2222 deploy@db1.example.com`.
pub fn parse_ssh_command(input: &str) -> Result<SshCommandDraft, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("type an ssh command first, e.g. ssh user@host".into());
    }
    // Drop a leading `ssh` (with or without a trailing space) if present.
    let rest = if trimmed == "ssh" {
        ""
    } else {
        trimmed
            .strip_prefix("ssh ")
            .or_else(|| trimmed.strip_prefix("SSH "))
            .unwrap_or(trimmed)
    };
    let mut tokens = rest.split_whitespace();

    let mut username: Option<String> = None;
    let mut port: Option<u16> = None;
    let mut identity: Option<String> = None;
    let mut host: Option<String> = None;
    let mut warnings: Vec<String> = Vec::new();

    while let Some(token) = tokens.next() {
        match token {
            "-p" => match tokens
                .next()
                .and_then(|v| v.parse::<u16>().ok())
                .filter(|p| *p > 0)
            {
                Some(p) => port = Some(p),
                None => return Err("-p needs a port number".into()),
            },
            "-i" => match tokens.next() {
                Some(path) => identity = Some(path.to_string()),
                None => return Err("-i needs a key file path".into()),
            },
            "-l" => match tokens.next() {
                Some(u) => username = Some(u.to_string()),
                None => return Err("-l needs a username".into()),
            },
            "-A" | "-a" | "-C" | "-v" | "-N" | "-f" | "-4" | "-6" | "-t" | "-T" => {
                // Known-safe boolean flags; the connection itself does not
                // need them at creation time.
            }
            "-L" | "-R" | "-D" | "-J" => {
                let value = tokens.next().unwrap_or("");
                warnings.push(format!(
                    "{token} {value} is not imported — set up forwarding in the connection's settings"
                ));
            }
            "-o" => {
                tokens.next(); // consume the option value
            }
            other if other.starts_with('-') => {
                warnings.push(format!("flag {other} was ignored"));
            }
            other => {
                if host.is_some() {
                    return Err(format!("unexpected extra argument: {other}"));
                }
                match other.split_once('@') {
                    Some((u, h)) if !u.is_empty() && !h.is_empty() => {
                        username = Some(u.to_string());
                        host = Some(h.to_string());
                    }
                    _ => host = Some(other.to_string()),
                }
            }
        }
    }

    let host = host.ok_or_else(|| "no host given, e.g. ssh user@host".to_string())?;
    Ok(SshCommandDraft {
        username,
        host,
        port,
        identity_path: identity,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_user_at_host() {
        let d = parse_ssh_command("ssh deploy@db1.example.com").unwrap();
        assert_eq!(d.host, "db1.example.com");
        assert_eq!(d.username.as_deref(), Some("deploy"));
        assert_eq!(d.port, None);
        assert!(d.warnings.is_empty());
    }

    #[test]
    fn parses_flags() {
        let d = parse_ssh_command("ssh -i ~/keys/aws.pem -p 2222 ubuntu@15.232.158.183").unwrap();
        assert_eq!(d.host, "15.232.158.183");
        assert_eq!(d.port, Some(2222));
        assert_eq!(d.identity_path.as_deref(), Some("~/keys/aws.pem"));
        assert!(d.warnings.is_empty());
    }

    #[test]
    fn parses_dash_l_and_ignores_safe_flags() {
        let d =
            parse_ssh_command("ssh -v -C -l ops -o ServerAliveInterval=60 db1.internal").unwrap();
        assert_eq!(d.host, "db1.internal");
        assert_eq!(d.username.as_deref(), Some("ops"));
        assert!(d.warnings.is_empty());
    }

    #[test]
    fn warns_about_forwarding_flags() {
        let d = parse_ssh_command("ssh -L 5432:localhost:5432 ops@db1").unwrap();
        assert_eq!(d.host, "db1");
        assert!(d.warnings.iter().any(|w| w.contains("-L")));
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_ssh_command("ssh").is_err());
        assert!(parse_ssh_command("").is_err());
        assert!(parse_ssh_command("ssh -p notaport h").is_err());
        assert!(parse_ssh_command("ssh host extra").is_err());
    }
}
