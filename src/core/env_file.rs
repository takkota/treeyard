use anyhow::{Context, Result};
use regex::Regex;
use std::fs;
use std::path::Path;

/// Read a variable's value from an env file. Returns None if not found.
/// Surrounding quotes (double or single) are stripped from the value.
pub fn get_var(path: &Path, var: &str) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    let prefix = format!("{var}=");
    for line in content.lines() {
        if let Some(val) = line.strip_prefix(&prefix) {
            return Some(strip_quotes(val));
        }
    }
    None
}

fn strip_quotes(s: &str) -> String {
    if s.len() >= 2
        && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')))
    {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

/// Set or update a variable in an env file.
/// If the variable exists, replace its value. Otherwise, append it.
pub fn set_var(path: &Path, var: &str, value: &str) -> Result<()> {
    let content = if path.exists() {
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?
    } else {
        String::new()
    };

    let prefix = format!("{var}=");
    let new_line = format!("{var}={value}");
    let mut found = false;
    let mut lines: Vec<String> = content
        .lines()
        .map(|line| {
            if line.starts_with(&prefix) {
                found = true;
                new_line.clone()
            } else {
                line.to_string()
            }
        })
        .collect();

    if !found {
        lines.push(new_line);
    }

    let mut output = lines.join("\n");
    // Preserve trailing newline
    if content.ends_with('\n') || !content.contains('\n') {
        output.push('\n');
    }

    fs::write(path, output).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

/// Replace ports in a batch to avoid cascading replacements.
/// Takes a slice of (old_port, new_port) pairs and performs all replacements in a single pass.
pub fn replace_ports_in_urls_batch(path: &Path, pairs: &[(u16, u16)]) -> Result<()> {
    let pairs: Vec<_> = pairs.iter().filter(|(old, new)| old != new).collect();
    if pairs.is_empty() || !path.exists() {
        return Ok(());
    }

    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;

    // Build a single regex matching any of the old ports after ://localhost:, ://127.0.0.1:, or ://0.0.0.0:
    let port_alts: Vec<String> = pairs.iter().map(|(old, _)| old.to_string()).collect();
    let pattern = format!(
        r"://(localhost|127\.0\.0\.1|0\.0\.0\.0):({})(?P<after>[^0-9]|$)",
        port_alts.join("|")
    );
    let re = Regex::new(&pattern).unwrap();

    // Build a lookup map from old -> new
    let map: std::collections::HashMap<u16, u16> = pairs.iter().map(|&&(o, n)| (o, n)).collect();

    let updated = re
        .replace_all(&content, |caps: &regex::Captures| {
            let host = &caps[1];
            let old: u16 = caps[2].parse().unwrap_or(0);
            let after = &caps["after"];
            match map.get(&old) {
                Some(&new) => format!("://{host}:{new}{after}"),
                None => caps[0].to_string(),
            }
        })
        .to_string();

    if updated != content {
        fs::write(path, &updated).with_context(|| format!("failed to write {}", path.display()))?;
    }
    Ok(())
}

/// Create .env from .env.example if .env doesn't exist.
/// Returns true if a new .env was created.
pub fn ensure_env_file(toplevel: &Path) -> Result<bool> {
    let env_path = toplevel.join(".env");
    if env_path.exists() {
        return Ok(false);
    }

    let example = toplevel.join(".env.example");
    if example.exists() {
        fs::copy(&example, &env_path).with_context(|| {
            format!(
                "failed to copy {} to {}",
                example.display(),
                env_path.display()
            )
        })?;
        Ok(true)
    } else {
        fs::write(&env_path, "").context("failed to create empty .env")?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Single-pair port replacement (test helper).
    fn replace_port_in_urls(path: &Path, old_port: u16, new_port: u16) -> Result<()> {
        replace_ports_in_urls_batch(path, &[(old_port, new_port)])
    }

    #[test]
    fn test_set_and_get_var() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(".env");
        fs::write(&path, "FOO=bar\nBAZ=qux\n").unwrap();

        set_var(&path, "FOO", "new_bar").unwrap();
        assert_eq!(get_var(&path, "FOO"), Some("new_bar".into()));
        assert_eq!(get_var(&path, "BAZ"), Some("qux".into()));
    }

    #[test]
    fn test_set_new_var() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(".env");
        fs::write(&path, "FOO=bar\n").unwrap();

        set_var(&path, "NEW_VAR", "value").unwrap();
        assert_eq!(get_var(&path, "NEW_VAR"), Some("value".into()));
    }

    #[test]
    fn test_replace_port_in_urls() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(".env");
        fs::write(
            &path,
            "API_URL=http://localhost:3000/api\nOTHER=http://127.0.0.1:3000/ws\n",
        )
        .unwrap();

        replace_port_in_urls(&path, 3000, 3010).unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("localhost:3010/api"));
        assert!(content.contains("127.0.0.1:3010/ws"));
    }

    #[test]
    fn test_replace_port_no_substring_false_positive() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(".env");
        fs::write(
            &path,
            "A=http://localhost:3000/api\nB=http://localhost:30001/other\n",
        )
        .unwrap();

        replace_port_in_urls(&path, 3000, 3010).unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("localhost:3010/api"));
        // Port 30001 must NOT be affected
        assert!(content.contains("localhost:30001/other"));
    }

    #[test]
    fn test_replace_ports_batch_no_cascade() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(".env");
        fs::write(
            &path,
            "A=http://localhost:3000/api\nB=http://localhost:3010/ws\n",
        )
        .unwrap();

        // Port A: 3000->3010, Port B: 3010->3020
        // Without batch, A would become 3010, then cascade to 3020.
        replace_ports_in_urls_batch(&path, &[(3000, 3010), (3010, 3020)]).unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("localhost:3010/api"));
        assert!(content.contains("localhost:3020/ws"));
    }

    #[test]
    fn test_replace_port_in_urls_0000() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(".env");
        fs::write(
            &path,
            "A=http://0.0.0.0:3000/api\nB=http://localhost:3000/x\n",
        )
        .unwrap();

        replace_port_in_urls(&path, 3000, 3010).unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("0.0.0.0:3010/api"));
        assert!(content.contains("localhost:3010/x"));
    }

    #[test]
    fn test_get_var_strips_quotes() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(".env");
        fs::write(&path, "A=\"hello\"\nB='world'\nC=noquotes\nD=\"\"\nE='\n").unwrap();

        assert_eq!(get_var(&path, "A"), Some("hello".into()));
        assert_eq!(get_var(&path, "B"), Some("world".into()));
        assert_eq!(get_var(&path, "C"), Some("noquotes".into()));
        assert_eq!(get_var(&path, "D"), Some("".into()));
        // Mismatched quote — should not strip
        assert_eq!(get_var(&path, "E"), Some("'".into()));
    }

    #[test]
    fn test_ensure_env_from_example() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join(".env.example"), "KEY=val\n").unwrap();

        let created = ensure_env_file(dir.path()).unwrap();
        assert!(created);
        assert_eq!(
            fs::read_to_string(dir.path().join(".env")).unwrap(),
            "KEY=val\n"
        );
    }

    #[test]
    fn test_ensure_env_already_exists() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join(".env"), "EXISTING=1\n").unwrap();

        let created = ensure_env_file(dir.path()).unwrap();
        assert!(!created);
    }
}
