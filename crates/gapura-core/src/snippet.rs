//! Snippet templating (PRD F14).
//!
//! A snippet command may reference the connection it runs against:
//! `{{host}}`, `{{user}}`, `{{port}}` and `{{name}}`. Expansion is a plain
//! string substitution on purpose — no shell evaluation — so a snippet can
//! never execute anything the user did not write. Unknown placeholders are
//! left untouched instead of being silently dropped.

use crate::model::Connection;

/// Replace the supported placeholders with values from `connection`.
pub fn expand_placeholders(command: &str, connection: &Connection) -> String {
    command
        .replace("{{host}}", &connection.host)
        .replace("{{user}}", &connection.username)
        .replace("{{port}}", &connection.port.to_string())
        .replace("{{name}}", &connection.name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AuthMethod;
    use chrono::Utc;
    use uuid::Uuid;

    fn connection() -> Connection {
        Connection {
            id: Uuid::new_v4(),
            name: "prod-vps".into(),
            group_id: None,
            host: "203.0.113.10".into(),
            port: 2222,
            username: "deploy".into(),
            auth: AuthMethod::Agent,
            tags: vec![],
            color: None,
            jump_host_id: None,
            notes: String::new(),
            pinned: false,
            last_connected_at: None,
            connect_count: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn expands_all_supported_placeholders() {
        let out = expand_placeholders(
            "ssh-copy-id -p {{port}} {{user}}@{{host}}  # {{name}}",
            &connection(),
        );
        assert_eq!(out, "ssh-copy-id -p 2222 deploy@203.0.113.10  # prod-vps");
    }

    #[test]
    fn leaves_unknown_placeholders_alone() {
        let out = expand_placeholders("echo {{unknown}} {{host}}", &connection());
        assert_eq!(out, "echo {{unknown}} 203.0.113.10");
    }

    #[test]
    fn plain_commands_are_unchanged() {
        let command = "sudo systemctl restart nginx && journalctl -u nginx -n 20";
        assert_eq!(expand_placeholders(command, &connection()), command);
    }
}
