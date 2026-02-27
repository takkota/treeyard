use regex::Regex;
use serde_yaml_ng::Value;
use std::path::Path;
use std::sync::LazyLock;

/// Matches `${VAR:-default}:container` port patterns in string values.
/// Also accepts optional IP prefix (`IP:`) and protocol suffix (`/tcp`, `/udp`, `/sctp`).
static PORT_VAR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?:\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}:)?\$\{([A-Z][A-Z0-9_]*):-(\d+)\}:(\d+)(?:/(?:tcp|udp|sctp))?"#,
    )
    .unwrap()
});

/// Matches hardcoded `host:container` port patterns (plain string, no env var).
/// Also accepts optional IP prefix (`IP:`) and protocol suffix (`/tcp`, `/udp`, `/sctp`).
static HARDCODED_PORT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^(?:\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}:)?(\d+):(\d+)(?:/(?:tcp|udp|sctp))?$"#)
        .unwrap()
});

/// Matches a bare `${VAR:-default}` (for long-syntax `published` field).
static PORT_VAR_BARE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^\$\{([A-Z][A-Z0-9_]*):-(\d+)\}$"#).unwrap());

/// Matches `${COMPOSE_PROJECT_NAME:-<prefix>}` to extract the project prefix.
static PREFIX_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\$\{COMPOSE_PROJECT_NAME:-([^}]+)\}"#).unwrap());

/// Matches `${...}` variable substitution patterns for pre-processing.
static VAR_SUBST_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\$\{[^}]*\}").unwrap());

/// Matches pre-processing placeholders for restoration.
static PLACEHOLDER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"__WTC_VAR_(\d+)__").unwrap());

/// A port mapping with environment variable substitution
#[derive(Debug, Clone)]
pub struct PortMapping {
    pub env_var: String,
    pub default_port: u16,
    #[allow(dead_code)]
    pub container_port: u16,
    pub service: String,
}

/// A hardcoded port (no env var, cannot auto-offset)
#[derive(Debug, Clone)]
pub struct HardcodedPort {
    pub host_port: u16,
    pub container_port: u16,
    pub service: String,
}

/// A named volume with a fixed name (not using ${COMPOSE_PROJECT_NAME})
#[derive(Debug, Clone)]
pub struct FixedNameVolume {
    pub key: String,
    pub name: String,
}

/// A service with an explicit container_name
#[derive(Debug, Clone)]
pub struct FixedContainerName {
    pub service: String,
    pub name: String,
}

/// A service with an explicit hostname
#[derive(Debug, Clone)]
pub struct FixedHostname {
    pub service: String,
    pub hostname: String,
}

/// A directive that references another container by name
/// (network_mode/pid/ipc: "container:xxx")
#[derive(Debug, Clone)]
pub struct ContainerRef {
    pub service: String,
    pub directive: String,
    pub referenced_name: String,
}

/// A warning about a directive that may cause issues in multi-worktree setups
#[derive(Debug, Clone)]
pub struct ComposeWarning {
    pub service: String,
    pub directive: String,
    pub message: String,
}

/// Result of parsing docker-compose.yml
#[derive(Debug)]
pub struct ComposeInfo {
    pub project_prefix: Option<String>,
    pub port_mappings: Vec<PortMapping>,
    pub hardcoded_ports: Vec<HardcodedPort>,
    pub services: Vec<String>,
    pub network_keys: Vec<String>,
    pub fixed_name_volumes: Vec<FixedNameVolume>,
    pub fixed_container_names: Vec<FixedContainerName>,
    pub fixed_hostnames: Vec<FixedHostname>,
    pub container_refs: Vec<ContainerRef>,
    pub warnings: Vec<ComposeWarning>,
    /// Per-service depends_on lists (service name -> list of dependency service names).
    pub depends_on: std::collections::HashMap<String, Vec<String>>,
}

// ---------------------------------------------------------------------------
// Pre-processing: replace ${...} with safe placeholders before YAML parsing,
// since ${VAR:-default} is not valid YAML syntax.
// ---------------------------------------------------------------------------

fn preprocess(content: &str) -> (String, Vec<String>) {
    let mut placeholders = Vec::new();
    let processed = VAR_SUBST_RE.replace_all(content, |caps: &regex::Captures| {
        let original = caps[0].to_string();
        let idx = placeholders.len();
        placeholders.push(original);
        format!("__WTC_VAR_{idx}__")
    });
    (processed.to_string(), placeholders)
}

fn restore(s: &str, placeholders: &[String]) -> String {
    PLACEHOLDER_RE
        .replace_all(s, |caps: &regex::Captures| {
            let idx: usize = caps[1].parse().unwrap();
            // If the index is out of range (e.g. user's YAML literally contains
            // `__WTC_VAR_N__`), preserve the original text instead of silently
            // replacing it with an empty string.
            placeholders
                .get(idx)
                .cloned()
                .unwrap_or_else(|| caps[0].to_string())
        })
        .to_string()
}

// ---------------------------------------------------------------------------
// YAML helpers
// ---------------------------------------------------------------------------

// Frequently used YAML keys, pre-allocated to avoid repeated heap allocation.
static KEY_PORTS: LazyLock<Value> = LazyLock::new(|| Value::String("ports".into()));
static KEY_CONTAINER_NAME: LazyLock<Value> =
    LazyLock::new(|| Value::String("container_name".into()));
static KEY_HOSTNAME: LazyLock<Value> = LazyLock::new(|| Value::String("hostname".into()));
static KEY_NETWORK_MODE: LazyLock<Value> = LazyLock::new(|| Value::String("network_mode".into()));
static KEY_PID: LazyLock<Value> = LazyLock::new(|| Value::String("pid".into()));
static KEY_IPC: LazyLock<Value> = LazyLock::new(|| Value::String("ipc".into()));
static KEY_DOMAINNAME: LazyLock<Value> = LazyLock::new(|| Value::String("domainname".into()));
static KEY_MAC_ADDRESS: LazyLock<Value> = LazyLock::new(|| Value::String("mac_address".into()));
static KEY_LABELS: LazyLock<Value> = LazyLock::new(|| Value::String("labels".into()));
static KEY_DEVICES: LazyLock<Value> = LazyLock::new(|| Value::String("devices".into()));
static KEY_NETWORKS: LazyLock<Value> = LazyLock::new(|| Value::String("networks".into()));
static KEY_IPV4_ADDRESS: LazyLock<Value> = LazyLock::new(|| Value::String("ipv4_address".into()));
static KEY_IPV6_ADDRESS: LazyLock<Value> = LazyLock::new(|| Value::String("ipv6_address".into()));
static KEY_TARGET: LazyLock<Value> = LazyLock::new(|| Value::String("target".into()));
static KEY_PUBLISHED: LazyLock<Value> = LazyLock::new(|| Value::String("published".into()));

/// Get a string value from a YAML value (handles both String and Number).
fn value_to_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Check if a YAML sequence contains a string matching a pattern.
fn seq_has_match(seq: &[Value], pattern: &str) -> bool {
    seq.iter()
        .any(|v| v.as_str().is_some_and(|s| s.contains(pattern)))
}

/// Check if a YAML mapping has any key matching a pattern.
fn mapping_keys_have_match(mapping: &serde_yaml_ng::Mapping, pattern: &str) -> bool {
    mapping
        .keys()
        .any(|k| k.as_str().is_some_and(|s| s.contains(pattern)))
}

// ---------------------------------------------------------------------------
// Main parse function
// ---------------------------------------------------------------------------

pub fn parse(path: &Path) -> anyhow::Result<ComposeInfo> {
    let content = std::fs::read_to_string(path)
        .map_err(|_| crate::error::Error::ComposeFileNotFound(path.to_path_buf()))?;

    let (processed, placeholders) = preprocess(&content);
    let mut doc: Value = serde_yaml_ng::from_str(&processed)
        .map_err(|e| anyhow::anyhow!("failed to parse {}: {e}", path.display()))?;

    // Resolve YAML merge keys (`<<: *alias`) so that inherited fields are
    // visible when walking the tree.  This must happen before any inspection.
    doc.apply_merge()
        .map_err(|e| anyhow::anyhow!("failed to resolve merge keys in {}: {e}", path.display()))?;

    let mut info = ComposeInfo {
        project_prefix: None,
        port_mappings: Vec::new(),
        hardcoded_ports: Vec::new(),
        services: Vec::new(),
        network_keys: Vec::new(),
        fixed_name_volumes: Vec::new(),
        fixed_container_names: Vec::new(),
        fixed_hostnames: Vec::new(),
        container_refs: Vec::new(),
        warnings: Vec::new(),
        depends_on: std::collections::HashMap::new(),
    };

    // --- services ---
    if let Some(services) = doc.get("services").and_then(Value::as_mapping) {
        for (key, value) in services {
            if let Some(svc_name) = key.as_str() {
                info.services.push(svc_name.to_string());
                if let Some(svc_mapping) = value.as_mapping() {
                    parse_service(svc_name, svc_mapping, &placeholders, &mut info);
                    // Parse depends_on
                    if let Some(deps) = parse_depends_on(svc_mapping) {
                        if !deps.is_empty() {
                            info.depends_on.insert(svc_name.to_string(), deps);
                        }
                    }
                }
            }
        }
    }

    // --- networks ---
    if let Some(networks) = doc.get("networks").and_then(Value::as_mapping) {
        for key in networks.keys() {
            if let Some(name) = key.as_str() {
                info.network_keys.push(name.to_string());
            }
        }
    }

    // --- volumes ---
    if let Some(volumes) = doc.get("volumes").and_then(Value::as_mapping) {
        for (key, value) in volumes {
            if let Some(vol_key) = key.as_str() {
                // Check for `name:` property on the volume
                if let Some(name_val) = value.get("name").and_then(Value::as_str) {
                    let name = restore(name_val, &placeholders);
                    if !name.contains("${") && !name.is_empty() {
                        info.fixed_name_volumes.push(FixedNameVolume {
                            key: vol_key.to_string(),
                            name,
                        });
                    }
                }
            }
        }
    }

    // --- project prefix (from COMPOSE_PROJECT_NAME defaults anywhere in the tree) ---
    let mut prefixes = Vec::new();
    collect_prefixes(&doc, &placeholders, &mut prefixes);

    if !prefixes.is_empty() {
        let mut prefix = prefixes[0].clone();
        for p in &prefixes[1..] {
            let common_len = prefix
                .chars()
                .zip(p.chars())
                .take_while(|(a, b)| a == b)
                .count();
            prefix.truncate(common_len);
        }
        let prefix = prefix.trim_end_matches(['-', '_']);
        if !prefix.is_empty() {
            info.project_prefix = Some(prefix.to_string());
        }
    }

    Ok(info)
}

// ---------------------------------------------------------------------------
// Per-service parsing
// ---------------------------------------------------------------------------

/// Parse `depends_on` for a service. Handles both list and mapping forms:
///   depends_on: [db, redis]
///   depends_on:
///     db:
///       condition: service_healthy
fn parse_depends_on(mapping: &serde_yaml_ng::Mapping) -> Option<Vec<String>> {
    static KEY_DEPENDS_ON: LazyLock<Value> = LazyLock::new(|| Value::String("depends_on".into()));
    let val = mapping.get(&*KEY_DEPENDS_ON)?;
    match val {
        Value::Sequence(seq) => {
            let deps: Vec<String> = seq
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
            Some(deps)
        }
        Value::Mapping(m) => {
            let deps: Vec<String> = m
                .keys()
                .filter_map(|k| k.as_str().map(|s| s.to_string()))
                .collect();
            Some(deps)
        }
        _ => None,
    }
}

fn parse_service(
    name: &str,
    mapping: &serde_yaml_ng::Mapping,
    placeholders: &[String],
    info: &mut ComposeInfo,
) {
    parse_ports(name, mapping, placeholders, info);
    parse_container_name(name, mapping, placeholders, info);
    parse_hostname(name, mapping, placeholders, info);
    parse_container_directives(name, mapping, placeholders, info);
    parse_warnings(name, mapping, info);
}

fn parse_ports(
    name: &str,
    mapping: &serde_yaml_ng::Mapping,
    placeholders: &[String],
    info: &mut ComposeInfo,
) {
    let ports = match mapping.get(&*KEY_PORTS).and_then(Value::as_sequence) {
        Some(p) => p,
        None => return,
    };

    for port_val in ports {
        // Long syntax: port entry is a YAML mapping with target/published keys
        if let Some(port_mapping) = port_val.as_mapping() {
            parse_long_syntax_port(name, port_mapping, placeholders, info);
            continue;
        }

        let port_str = match value_to_string(port_val) {
            Some(s) => restore(&s, placeholders),
            None => continue,
        };

        // Port with env var substitution
        if let Some(caps) = PORT_VAR_RE.captures(&port_str) {
            let default_port: u16 = caps[2].parse().unwrap_or(0);
            info.port_mappings.push(PortMapping {
                env_var: caps[1].to_string(),
                default_port,
                container_port: caps[3].parse().unwrap_or(0),
                service: name.to_string(),
            });
            if default_port > 60000 {
                info.warnings.push(ComposeWarning {
                    service: name.to_string(),
                    directive: "ports".into(),
                    message: format!(
                        "default port {} is very high — may overflow 65535 with only a few worktrees",
                        default_port
                    ),
                });
            }
            continue;
        }

        // Hardcoded port (only if no variable substitution)
        if !port_str.contains("${") {
            if let Some(caps) = HARDCODED_PORT_RE.captures(&port_str) {
                info.hardcoded_ports.push(HardcodedPort {
                    host_port: caps[1].parse().unwrap_or(0),
                    container_port: caps[2].parse().unwrap_or(0),
                    service: name.to_string(),
                });
                continue;
            }
        }

        // Container-only port (e.g. "3000") — no host port, no conflict
        if !port_str.contains(':') {
            continue;
        }

        // Unrecognized port pattern with explicit host mapping — warn
        info.warnings.push(ComposeWarning {
            service: name.to_string(),
            directive: "ports".into(),
            message: format!(
                "Unrecognized port format '{}' — cannot auto-offset. Use ${{VAR:-DEFAULT}}:CONTAINER format.",
                port_str
            ),
        });
    }
}

fn parse_long_syntax_port(
    name: &str,
    port_mapping: &serde_yaml_ng::Mapping,
    placeholders: &[String],
    info: &mut ComposeInfo,
) {
    let container_port = match port_mapping.get(&*KEY_TARGET) {
        Some(Value::Number(n)) => n.as_u64().map(|v| v as u16),
        Some(Value::String(s)) => s.parse::<u16>().ok(),
        _ => None,
    };
    let container_port = match container_port {
        Some(p) => p,
        None => return,
    };

    let published = match port_mapping.get(&*KEY_PUBLISHED) {
        Some(v) => v,
        None => return, // No published port = expose only, no conflict
    };

    match published {
        Value::Number(n) => {
            if let Some(host_port) = n.as_u64() {
                info.hardcoded_ports.push(HardcodedPort {
                    host_port: host_port as u16,
                    container_port,
                    service: name.to_string(),
                });
            }
        }
        Value::String(s) => {
            let restored = restore(s, placeholders);
            if let Some(caps) = PORT_VAR_BARE_RE.captures(&restored) {
                let default_port: u16 = caps[2].parse().unwrap_or(0);
                info.port_mappings.push(PortMapping {
                    env_var: caps[1].to_string(),
                    default_port,
                    container_port,
                    service: name.to_string(),
                });
                if default_port > 60000 {
                    info.warnings.push(ComposeWarning {
                        service: name.to_string(),
                        directive: "ports".into(),
                        message: format!(
                            "default port {} is very high — may overflow 65535 with only a few worktrees",
                            default_port
                        ),
                    });
                }
            } else if let Ok(port) = restored.parse::<u16>() {
                info.hardcoded_ports.push(HardcodedPort {
                    host_port: port,
                    container_port,
                    service: name.to_string(),
                });
            } else {
                info.warnings.push(ComposeWarning {
                    service: name.to_string(),
                    directive: "ports (long syntax)".into(),
                    message: format!(
                        "Unrecognized published port '{}' — cannot auto-offset.",
                        restored
                    ),
                });
            }
        }
        _ => {}
    }
}

fn parse_container_name(
    name: &str,
    mapping: &serde_yaml_ng::Mapping,
    placeholders: &[String],
    info: &mut ComposeInfo,
) {
    if let Some(val) = mapping.get(&*KEY_CONTAINER_NAME).and_then(Value::as_str) {
        let val = restore(val, placeholders);
        if !val.contains("${") && !val.is_empty() {
            info.fixed_container_names.push(FixedContainerName {
                service: name.to_string(),
                name: val,
            });
        }
    }
}

fn parse_hostname(
    name: &str,
    mapping: &serde_yaml_ng::Mapping,
    placeholders: &[String],
    info: &mut ComposeInfo,
) {
    if let Some(val) = mapping.get(&*KEY_HOSTNAME).and_then(Value::as_str) {
        let val = restore(val, placeholders);
        if !val.contains("${") && !val.is_empty() {
            info.fixed_hostnames.push(FixedHostname {
                service: name.to_string(),
                hostname: val,
            });
        }
    }
}

fn parse_container_directives(
    name: &str,
    mapping: &serde_yaml_ng::Mapping,
    placeholders: &[String],
    info: &mut ComposeInfo,
) {
    for (directive, key) in [
        ("network_mode", &*KEY_NETWORK_MODE),
        ("pid", &*KEY_PID),
        ("ipc", &*KEY_IPC),
    ] {
        let val = match mapping.get(key).and_then(Value::as_str) {
            Some(v) => restore(v, placeholders),
            None => continue,
        };

        // network_mode: host
        if directive == "network_mode" && val == "host" {
            info.warnings.push(ComposeWarning {
                service: name.to_string(),
                directive: "network_mode: host".into(),
                message: "Host network mode bypasses port mapping. Port offsets will not take effect for this service.".into(),
            });
        }

        // container:xxx references
        if let Some(container_name) = val.strip_prefix("container:") {
            info.container_refs.push(ContainerRef {
                service: name.to_string(),
                directive: directive.to_string(),
                referenced_name: container_name.trim().to_string(),
            });
        }
    }
}

fn parse_warnings(name: &str, mapping: &serde_yaml_ng::Mapping, info: &mut ComposeInfo) {
    // domainname
    if mapping.contains_key(&*KEY_DOMAINNAME) {
        info.warnings.push(ComposeWarning {
            service: name.to_string(),
            directive: "domainname".into(),
            message: "domainname may conflict between worktrees if services share a network."
                .into(),
        });
    }

    // mac_address
    if mapping.contains_key(&*KEY_MAC_ADDRESS) {
        info.warnings.push(ComposeWarning {
            service: name.to_string(),
            directive: "mac_address".into(),
            message:
                "Hardcoded MAC address will conflict if multiple worktrees run on the same network."
                    .into(),
        });
    }

    // labels (traefik)
    parse_traefik_warning(name, mapping, info);

    // devices
    if let Some(devices) = mapping.get(&*KEY_DEVICES).and_then(Value::as_sequence) {
        if !devices.is_empty() {
            info.warnings.push(ComposeWarning {
                service: name.to_string(),
                directive: "devices".into(),
                message:
                    "Host device mappings may conflict if multiple worktrees share the same device."
                        .into(),
            });
        }
    }

    // static IP in service-level networks
    if let Some(networks) = mapping.get(&*KEY_NETWORKS).and_then(Value::as_mapping) {
        for (_, net_config) in networks {
            if let Some(net_mapping) = net_config.as_mapping() {
                if net_mapping.contains_key(&*KEY_IPV4_ADDRESS)
                    || net_mapping.contains_key(&*KEY_IPV6_ADDRESS)
                {
                    info.warnings.push(ComposeWarning {
                        service: name.to_string(),
                        directive: "static IP".into(),
                        message: "Static IP assignment may conflict between worktrees on the same network.".into(),
                    });
                }
            }
        }
    }
}

fn parse_traefik_warning(name: &str, mapping: &serde_yaml_ng::Mapping, info: &mut ComposeInfo) {
    let traefik_pattern = "traefik.http.routers.";

    let has_traefik = match mapping.get(&*KEY_LABELS) {
        // labels as list: - "traefik.http.routers..."
        Some(Value::Sequence(seq)) => seq_has_match(seq, traefik_pattern),
        // labels as mapping: traefik.http.routers...: value
        Some(Value::Mapping(m)) => mapping_keys_have_match(m, traefik_pattern),
        _ => false,
    };

    if has_traefik {
        info.warnings.push(ComposeWarning {
            service: name.to_string(),
            directive: "labels (traefik)".into(),
            message: "Traefik router labels may conflict between worktrees. Consider parameterizing router names and Host rules.".into(),
        });
    }
}

// ---------------------------------------------------------------------------
// Recursive prefix collection
// ---------------------------------------------------------------------------

fn collect_prefixes(value: &Value, placeholders: &[String], prefixes: &mut Vec<String>) {
    match value {
        Value::String(s) => {
            let restored = restore(s, placeholders);
            if let Some(caps) = PREFIX_RE.captures(&restored) {
                prefixes.push(caps[1].to_string());
            }
        }
        Value::Mapping(m) => {
            for (k, v) in m {
                collect_prefixes(k, placeholders, prefixes);
                collect_prefixes(v, placeholders, prefixes);
            }
        }
        Value::Sequence(seq) => {
            for v in seq {
                collect_prefixes(v, placeholders, prefixes);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn parse_str(content: &str) -> ComposeInfo {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        parse(f.path()).unwrap()
    }

    #[test]
    fn test_parse_port_with_env_var() {
        let info = parse_str(indoc! {"
            services:
              web:
                ports:
                  - \"${WEB_PORT:-3000}:3000\"
        "});
        assert_eq!(info.port_mappings.len(), 1);
        assert_eq!(info.port_mappings[0].env_var, "WEB_PORT");
        assert_eq!(info.port_mappings[0].default_port, 3000);
        assert_eq!(info.port_mappings[0].container_port, 3000);
        assert_eq!(info.port_mappings[0].service, "web");
    }

    #[test]
    fn test_parse_hardcoded_port() {
        let info = parse_str(indoc! {"
            services:
              db:
                ports:
                  - \"5432:5432\"
        "});
        assert_eq!(info.hardcoded_ports.len(), 1);
        assert_eq!(info.hardcoded_ports[0].host_port, 5432);
        assert_eq!(info.hardcoded_ports[0].service, "db");
    }

    #[test]
    fn test_parse_project_prefix() {
        let info = parse_str(indoc! {"
            services:
              web:
                image: nginx
            networks:
              app-network:
                name: ${COMPOSE_PROJECT_NAME:-myapp}-network
            volumes:
              data:
                name: ${COMPOSE_PROJECT_NAME:-myapp}_data
        "});
        assert_eq!(info.project_prefix.as_deref(), Some("myapp"));
    }

    #[test]
    fn test_parse_multiple_services() {
        let info = parse_str(indoc! {"
            services:
              web:
                ports:
                  - \"${WEB_PORT:-3000}:3000\"
              api:
                ports:
                  - \"${API_PORT:-8080}:8080\"
              worker:
                image: worker
        "});
        assert_eq!(info.services, vec!["web", "api", "worker"]);
        assert_eq!(info.port_mappings.len(), 2);
        assert_eq!(info.port_mappings[1].service, "api");
    }

    #[test]
    fn test_parse_4space_indent() {
        let info = parse_str(indoc! {"
            services:
                web:
                    ports:
                        - \"${WEB_PORT:-3000}:3000\"
                api:
                    ports:
                        - \"${API_PORT:-8080}:8080\"
        "});
        assert_eq!(info.services, vec!["web", "api"]);
        assert_eq!(info.port_mappings.len(), 2);
        assert_eq!(info.port_mappings[0].service, "web");
        assert_eq!(info.port_mappings[1].service, "api");
    }

    #[test]
    fn test_parse_4space_indent_networks() {
        let info = parse_str(indoc! {"
            services:
                web:
                    image: nginx
            networks:
                app-network:
                    driver: bridge
        "});
        assert_eq!(info.services, vec!["web"]);
        assert_eq!(info.network_keys, vec!["app-network"]);
    }

    #[test]
    fn test_parse_network_keys() {
        let info = parse_str(indoc! {"
            services:
              web:
                image: nginx
            networks:
              app-network:
                driver: bridge
        "});
        assert_eq!(info.network_keys, vec!["app-network"]);
    }

    #[test]
    fn test_parse_fixed_name_volumes() {
        let info = parse_str(indoc! {"
            services:
              web:
                image: nginx
            volumes:
              data:
                name: my-fixed-data
              cache:
                name: my-cache
        "});
        assert_eq!(info.fixed_name_volumes.len(), 2);
        assert_eq!(info.fixed_name_volumes[0].key, "data");
        assert_eq!(info.fixed_name_volumes[0].name, "my-fixed-data");
        assert_eq!(info.fixed_name_volumes[1].key, "cache");
        assert_eq!(info.fixed_name_volumes[1].name, "my-cache");
    }

    #[test]
    fn test_parse_volumes_skips_variable_substitution() {
        let info = parse_str(indoc! {"
            services:
              web:
                image: nginx
            volumes:
              data:
                name: ${COMPOSE_PROJECT_NAME:-myapp}_data
              logs:
                name: ${MY_PREFIX}_logs
              cache:
                name: my-fixed-cache
        "});
        // Only the fixed-name volume should be detected
        assert_eq!(info.fixed_name_volumes.len(), 1);
        assert_eq!(info.fixed_name_volumes[0].key, "cache");
        assert_eq!(info.fixed_name_volumes[0].name, "my-fixed-cache");
    }

    #[test]
    fn test_parse_volumes_without_name() {
        let info = parse_str(indoc! {"
            services:
              web:
                image: nginx
            volumes:
              data:
                driver: local
        "});
        // No fixed-name volumes (no name: property)
        assert_eq!(info.fixed_name_volumes.len(), 0);
    }

    #[test]
    fn test_parse_volumes_4space_indent() {
        let info = parse_str(indoc! {"
            services:
                web:
                    image: nginx
            volumes:
                data:
                    name: my-fixed-data
        "});
        assert_eq!(info.fixed_name_volumes.len(), 1);
        assert_eq!(info.fixed_name_volumes[0].key, "data");
        assert_eq!(info.fixed_name_volumes[0].name, "my-fixed-data");
    }

    // --- container_name tests ---

    #[test]
    fn test_parse_container_name() {
        let info = parse_str(indoc! {"
            services:
              web:
                container_name: myapp-web
                image: nginx
              db:
                container_name: myapp-db
                image: postgres
        "});
        assert_eq!(info.fixed_container_names.len(), 2);
        assert_eq!(info.fixed_container_names[0].service, "web");
        assert_eq!(info.fixed_container_names[0].name, "myapp-web");
        assert_eq!(info.fixed_container_names[1].service, "db");
        assert_eq!(info.fixed_container_names[1].name, "myapp-db");
    }

    #[test]
    fn test_parse_container_name_skips_variable() {
        let info = parse_str(indoc! {"
            services:
              web:
                container_name: ${COMPOSE_PROJECT_NAME}-web
                image: nginx
        "});
        assert_eq!(info.fixed_container_names.len(), 0);
    }

    #[test]
    fn test_parse_container_name_quoted() {
        let info = parse_str(indoc! {"
            services:
              web:
                container_name: \"myapp-web\"
                image: nginx
        "});
        assert_eq!(info.fixed_container_names.len(), 1);
        assert_eq!(info.fixed_container_names[0].name, "myapp-web");
    }

    // --- hostname tests ---

    #[test]
    fn test_parse_hostname() {
        let info = parse_str(indoc! {"
            services:
              web:
                hostname: myhost
                image: nginx
        "});
        assert_eq!(info.fixed_hostnames.len(), 1);
        assert_eq!(info.fixed_hostnames[0].service, "web");
        assert_eq!(info.fixed_hostnames[0].hostname, "myhost");
    }

    #[test]
    fn test_parse_hostname_skips_variable() {
        let info = parse_str(indoc! {"
            services:
              web:
                hostname: ${MY_HOST}
                image: nginx
        "});
        assert_eq!(info.fixed_hostnames.len(), 0);
    }

    // --- container ref tests ---

    #[test]
    fn test_parse_network_mode_container_ref() {
        let info = parse_str(indoc! {"
            services:
              app:
                container_name: myapp
                image: myapp
              sidecar:
                network_mode: \"container:myapp\"
                image: sidecar
        "});
        assert_eq!(info.container_refs.len(), 1);
        assert_eq!(info.container_refs[0].service, "sidecar");
        assert_eq!(info.container_refs[0].directive, "network_mode");
        assert_eq!(info.container_refs[0].referenced_name, "myapp");
    }

    #[test]
    fn test_parse_pid_container_ref() {
        let info = parse_str(indoc! {"
            services:
              app:
                container_name: myapp
                image: myapp
              debugger:
                pid: \"container:myapp\"
                image: busybox
        "});
        assert_eq!(info.container_refs.len(), 1);
        assert_eq!(info.container_refs[0].directive, "pid");
        assert_eq!(info.container_refs[0].referenced_name, "myapp");
    }

    #[test]
    fn test_parse_ipc_container_ref() {
        let info = parse_str(indoc! {"
            services:
              app:
                container_name: myapp
                image: myapp
              worker:
                ipc: \"container:myapp\"
                image: worker
        "});
        assert_eq!(info.container_refs.len(), 1);
        assert_eq!(info.container_refs[0].directive, "ipc");
        assert_eq!(info.container_refs[0].referenced_name, "myapp");
    }

    // --- warning tests ---

    #[test]
    fn test_parse_warning_network_mode_host() {
        let info = parse_str(indoc! {"
            services:
              monitor:
                network_mode: \"host\"
                image: prom
        "});
        assert_eq!(info.warnings.len(), 1);
        assert_eq!(info.warnings[0].service, "monitor");
        assert_eq!(info.warnings[0].directive, "network_mode: host");
    }

    #[test]
    fn test_parse_warning_mac_address() {
        let info = parse_str(indoc! {"
            services:
              iot:
                mac_address: \"02:42:ac:11:00:02\"
                image: iot
        "});
        assert_eq!(info.warnings.len(), 1);
        assert_eq!(info.warnings[0].directive, "mac_address");
    }

    #[test]
    fn test_parse_warning_domainname() {
        let info = parse_str(indoc! {"
            services:
              mail:
                domainname: example.com
                image: mail
        "});
        assert_eq!(info.warnings.len(), 1);
        assert_eq!(info.warnings[0].directive, "domainname");
    }

    #[test]
    fn test_parse_warning_traefik_labels() {
        let info = parse_str(indoc! {"
            services:
              web:
                image: nginx
                labels:
                  - \"traefik.http.routers.web.rule=Host(`myapp.local`)\"
        "});
        assert_eq!(info.warnings.len(), 1);
        assert_eq!(info.warnings[0].directive, "labels (traefik)");
    }

    #[test]
    fn test_parse_warning_devices() {
        let info = parse_str(indoc! {"
            services:
              gpu:
                image: nvidia
                devices:
                  - /dev/nvidia0:/dev/nvidia0
        "});
        assert_eq!(info.warnings.len(), 1);
        assert_eq!(info.warnings[0].directive, "devices");
    }

    #[test]
    fn test_parse_warning_static_ip() {
        let info = parse_str(indoc! {"
            services:
              db:
                image: postgres
                networks:
                  backend:
                    ipv4_address: 172.20.0.10
            networks:
              backend:
                driver: bridge
        "});
        assert_eq!(info.warnings.len(), 1);
        assert_eq!(info.warnings[0].directive, "static IP");
    }

    #[test]
    fn test_parse_no_warnings_for_clean_compose() {
        let info = parse_str(indoc! {"
            services:
              web:
                image: nginx
                ports:
                  - \"${WEB_PORT:-3000}:3000\"
              db:
                image: postgres
        "});
        assert!(info.warnings.is_empty());
        assert!(info.fixed_container_names.is_empty());
        assert!(info.fixed_hostnames.is_empty());
        assert!(info.container_refs.is_empty());
    }

    #[test]
    fn test_parse_combined_all_features() {
        let info = parse_str(indoc! {"
            services:
              app:
                container_name: myapp
                hostname: myhost
                image: myapp
                ports:
                  - \"${APP_PORT:-3000}:3000\"
              sidecar:
                network_mode: \"container:myapp\"
                image: sidecar
              monitor:
                network_mode: \"host\"
                image: prom
        "});
        assert_eq!(info.fixed_container_names.len(), 1);
        assert_eq!(info.fixed_hostnames.len(), 1);
        assert_eq!(info.container_refs.len(), 1);
        assert_eq!(info.warnings.len(), 1);
        assert_eq!(info.port_mappings.len(), 1);
    }

    // --- regression tests ---

    #[test]
    fn test_multiple_devices_produce_single_warning() {
        let info = parse_str(indoc! {"
            services:
              gpu:
                image: nvidia
                devices:
                  - /dev/nvidia0:/dev/nvidia0
                  - /dev/nvidia1:/dev/nvidia1
                  - /dev/nvidiactl:/dev/nvidiactl
        "});
        let device_warnings: Vec<_> = info
            .warnings
            .iter()
            .filter(|w| w.directive == "devices")
            .collect();
        assert_eq!(
            device_warnings.len(),
            1,
            "should produce exactly one devices warning per service"
        );
    }

    #[test]
    fn test_multiple_traefik_labels_produce_single_warning() {
        let info = parse_str(indoc! {"
            services:
              web:
                image: nginx
                labels:
                  - \"traefik.http.routers.web.rule=Host(`myapp.local`)\"
                  - \"traefik.http.routers.web.entrypoints=websecure\"
                  - \"traefik.http.routers.web.tls=true\"
        "});
        let traefik_warnings: Vec<_> = info
            .warnings
            .iter()
            .filter(|w| w.directive == "labels (traefik)")
            .collect();
        assert_eq!(
            traefik_warnings.len(),
            1,
            "should produce exactly one traefik warning per service"
        );
    }

    #[test]
    fn test_labels_then_ports() {
        let info = parse_str(indoc! {"
            services:
              web:
                labels:
                  - \"traefik.http.routers.web.rule=Host(`myapp.local`)\"
                ports:
                  - \"${WEB_PORT:-3000}:3000\"
                container_name: myapp-web
        "});
        assert_eq!(info.warnings.len(), 1);
        assert_eq!(info.warnings[0].directive, "labels (traefik)");
        assert_eq!(info.port_mappings.len(), 1);
        assert_eq!(info.fixed_container_names.len(), 1);
    }

    #[test]
    fn test_network_mode_host_unquoted() {
        let info = parse_str(indoc! {"
            services:
              app:
                network_mode: host
                image: myapp
        "});
        assert_eq!(info.warnings.len(), 1);
        assert_eq!(info.warnings[0].directive, "network_mode: host");
    }

    // --- YAML anchor & merge key tests ---

    #[test]
    fn test_parse_service_with_yaml_anchor() {
        let info = parse_str(indoc! {"
            services:
              db: &db
                image: postgres
                ports:
                  - \"${DB_PORT:-5432}:5432\"
              runner_db:
                <<: *db
                command: /bin/sh
        "});
        assert!(info.services.contains(&"db".to_string()));
        assert!(info.services.contains(&"runner_db".to_string()));
        // db has explicit ports; runner_db inherits via merge key
        assert!(info.port_mappings.iter().any(|p| p.service == "db"));
    }

    #[test]
    fn test_parse_service_with_merge_key_and_anchors() {
        let info = parse_str(indoc! {"
            services:
              base: &base
                image: myapp
              app: &app
                <<: *base
                ports:
                  - \"${APP_PORT:-3000}:3000\"
              backend:
                <<: *app
                expose:
                  - \"80\"
        "});
        assert_eq!(info.services, vec!["base", "app", "backend"]);
        assert!(info.port_mappings.iter().any(|p| p.service == "app"));
        // Merge keys are resolved by apply_merge(), so backend inherits
        // ports from app.
        assert!(info.port_mappings.iter().any(|p| p.service == "backend"));
    }

    #[test]
    fn test_parse_merge_key_overrides() {
        // When a service defines its own ports, they override the merged ones
        let info = parse_str(indoc! {"
            services:
              base: &base
                image: myapp
                ports:
                  - \"${BASE_PORT:-3000}:3000\"
              app:
                <<: *base
                ports:
                  - \"${APP_PORT:-8080}:8080\"
        "});
        // app's own ports override base's ports
        let app_ports: Vec<_> = info
            .port_mappings
            .iter()
            .filter(|p| p.service == "app")
            .collect();
        assert_eq!(app_ports.len(), 1);
        assert_eq!(app_ports[0].env_var, "APP_PORT");
    }

    #[test]
    fn test_parse_traefik_labels_as_mapping() {
        let info = parse_str(indoc! {"
            services:
              web:
                image: nginx
                labels:
                  traefik.http.routers.web.rule: \"Host(`myapp.local`)\"
        "});
        assert_eq!(info.warnings.len(), 1);
        assert_eq!(info.warnings[0].directive, "labels (traefik)");
    }

    // --- pre-processing tests ---

    #[test]
    fn test_preprocess_and_restore() {
        let input = "port: ${WEB_PORT:-3000}:3000\nname: ${COMPOSE_PROJECT_NAME:-myapp}";
        let (processed, placeholders) = preprocess(input);
        assert!(!processed.contains("${"));
        assert_eq!(placeholders.len(), 2);

        let restored = restore(&processed, &placeholders);
        assert_eq!(restored, input);
    }

    #[test]
    fn test_restore_preserves_unknown_placeholder() {
        // If user's YAML literally contains __WTC_VAR_N__, it should not
        // silently turn into an empty string.
        let placeholders = vec!["${ONLY_ONE}".to_string()];
        let input = "__WTC_VAR_0__ and __WTC_VAR_99__";
        let restored = restore(input, &placeholders);
        // Index 0 is restored; index 99 is out of range and kept verbatim.
        assert_eq!(restored, "${ONLY_ONE} and __WTC_VAR_99__");
    }

    #[test]
    fn test_hardcoded_port_unquoted() {
        // serde_yaml_ng follows YAML 1.2, which does NOT have sexagesimal
        // number interpretation, so `5432:5432` is treated as a string.
        // Unquoted hardcoded ports are correctly detected.
        let info = parse_str(indoc! {"
            services:
              db:
                ports:
                  - 5432:5432
        "});
        assert_eq!(info.hardcoded_ports.len(), 1);
        assert_eq!(info.hardcoded_ports[0].host_port, 5432);
        assert_eq!(info.hardcoded_ports[0].container_port, 5432);
    }

    // --- error path tests ---

    #[test]
    fn test_parse_file_not_found() {
        let result = parse(Path::new("/nonexistent/docker-compose.yml"));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("docker-compose.yml"));
    }

    #[test]
    fn test_parse_invalid_yaml() {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(b"services:\n  web:\n    - invalid: [yaml\n")
            .unwrap();
        let result = parse(f.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("failed to parse"));
    }

    #[test]
    fn test_parse_empty_compose() {
        let info = parse_str("# empty file\n");
        assert!(info.services.is_empty());
        assert!(info.port_mappings.is_empty());
        assert!(info.project_prefix.is_none());
    }

    // --- ipv6_address warning test ---

    #[test]
    fn test_parse_warning_static_ip_ipv6() {
        let info = parse_str(indoc! {"
            services:
              db:
                image: postgres
                networks:
                  backend:
                    ipv6_address: \"2001:db8::10\"
            networks:
              backend:
                driver: bridge
        "});
        assert_eq!(info.warnings.len(), 1);
        assert_eq!(info.warnings[0].directive, "static IP");
    }

    // --- placeholder collision test ---

    #[test]
    fn test_placeholder_in_range_collision() {
        // If user's YAML literally contains __WTC_VAR_0__, it should be
        // handled gracefully (it will be treated as the first placeholder).
        let info = parse_str(indoc! {"
            services:
              web:
                image: __WTC_VAR_0__
                ports:
                  - \"${WEB_PORT:-3000}:3000\"
        "});
        // The ${WEB_PORT:-3000} becomes placeholder index 0.
        // __WTC_VAR_0__ in the image field is just a string that happens
        // to look like a placeholder — no crash, parsing proceeds.
        assert_eq!(info.port_mappings.len(), 1);
        assert_eq!(info.services, vec!["web"]);
    }

    #[test]
    fn test_merge_key_inherits_container_name() {
        // Verify that apply_merge() resolves inherited fields beyond just ports.
        let info = parse_str(indoc! {"
            services:
              base: &base
                container_name: myapp-base
                image: myapp
              derived:
                <<: *base
                image: derived
        "});
        // derived inherits container_name from base via merge key
        assert!(info
            .fixed_container_names
            .iter()
            .any(|fc| fc.service == "derived" && fc.name == "myapp-base"));
    }

    // --- IP-bound port tests ---

    #[test]
    fn test_hardcoded_port_ip_bound() {
        let info = parse_str(indoc! {"
            services:
              db:
                ports:
                  - \"127.0.0.1:5432:5432\"
        "});
        assert_eq!(info.hardcoded_ports.len(), 1);
        assert_eq!(info.hardcoded_ports[0].host_port, 5432);
        assert_eq!(info.hardcoded_ports[0].container_port, 5432);
        assert_eq!(info.hardcoded_ports[0].service, "db");
    }

    #[test]
    fn test_var_port_ip_bound() {
        let info = parse_str(indoc! {"
            services:
              web:
                ports:
                  - \"127.0.0.1:${WEB_PORT:-3000}:3000\"
        "});
        assert_eq!(info.port_mappings.len(), 1);
        assert_eq!(info.port_mappings[0].env_var, "WEB_PORT");
        assert_eq!(info.port_mappings[0].default_port, 3000);
        assert_eq!(info.port_mappings[0].container_port, 3000);
    }

    // --- protocol suffix tests ---

    #[test]
    fn test_hardcoded_port_with_protocol() {
        let info = parse_str(indoc! {"
            services:
              dns:
                ports:
                  - \"53:53/udp\"
                  - \"53:53/tcp\"
        "});
        assert_eq!(info.hardcoded_ports.len(), 2);
        assert_eq!(info.hardcoded_ports[0].host_port, 53);
        assert_eq!(info.hardcoded_ports[1].host_port, 53);
    }

    #[test]
    fn test_var_port_with_protocol() {
        let info = parse_str(indoc! {"
            services:
              web:
                ports:
                  - \"${WEB_PORT:-3000}:3000/tcp\"
        "});
        assert_eq!(info.port_mappings.len(), 1);
        assert_eq!(info.port_mappings[0].env_var, "WEB_PORT");
        assert_eq!(info.port_mappings[0].container_port, 3000);
    }

    #[test]
    fn test_ip_bound_with_protocol() {
        let info = parse_str(indoc! {"
            services:
              dns:
                ports:
                  - \"0.0.0.0:53:53/udp\"
        "});
        assert_eq!(info.hardcoded_ports.len(), 1);
        assert_eq!(info.hardcoded_ports[0].host_port, 53);
        assert_eq!(info.hardcoded_ports[0].container_port, 53);
    }

    // --- long syntax port tests ---

    #[test]
    fn test_long_syntax_hardcoded() {
        let info = parse_str(indoc! {"
            services:
              web:
                ports:
                  - target: 3000
                    published: 3000
                    protocol: tcp
        "});
        assert_eq!(info.hardcoded_ports.len(), 1);
        assert_eq!(info.hardcoded_ports[0].host_port, 3000);
        assert_eq!(info.hardcoded_ports[0].container_port, 3000);
        assert_eq!(info.hardcoded_ports[0].service, "web");
    }

    #[test]
    fn test_long_syntax_with_variable() {
        let info = parse_str(indoc! {"
            services:
              web:
                ports:
                  - target: 3000
                    published: \"${WEB_PORT:-3000}\"
        "});
        assert_eq!(info.port_mappings.len(), 1);
        assert_eq!(info.port_mappings[0].env_var, "WEB_PORT");
        assert_eq!(info.port_mappings[0].default_port, 3000);
        assert_eq!(info.port_mappings[0].container_port, 3000);
    }

    #[test]
    fn test_long_syntax_no_published() {
        // No published port = expose only, no conflict, no warning
        let info = parse_str(indoc! {"
            services:
              web:
                ports:
                  - target: 3000
        "});
        assert!(info.hardcoded_ports.is_empty());
        assert!(info.port_mappings.is_empty());
        assert!(info.warnings.is_empty());
    }

    #[test]
    fn test_long_syntax_published_string_number() {
        // published as a string that is a plain number
        let info = parse_str(indoc! {"
            services:
              web:
                ports:
                  - target: 3000
                    published: \"8080\"
        "});
        assert_eq!(info.hardcoded_ports.len(), 1);
        assert_eq!(info.hardcoded_ports[0].host_port, 8080);
        assert_eq!(info.hardcoded_ports[0].container_port, 3000);
    }

    // --- container-only port (no warning) ---

    #[test]
    fn test_container_only_port_no_warning() {
        let info = parse_str(indoc! {"
            services:
              web:
                ports:
                  - \"3000\"
        "});
        assert!(info.hardcoded_ports.is_empty());
        assert!(info.port_mappings.is_empty());
        assert!(info.warnings.is_empty());
    }

    // --- unrecognized port pattern warning ---

    #[test]
    fn test_warning_port_range() {
        let info = parse_str(indoc! {"
            services:
              web:
                ports:
                  - \"7000-7005:5000-5005\"
        "});
        assert!(info.hardcoded_ports.is_empty());
        assert!(info.port_mappings.is_empty());
        let port_warnings: Vec<_> = info
            .warnings
            .iter()
            .filter(|w| w.directive == "ports")
            .collect();
        assert_eq!(port_warnings.len(), 1);
        assert!(port_warnings[0].message.contains("7000-7005:5000-5005"));
    }

    #[test]
    fn test_long_syntax_unrecognized_published() {
        let info = parse_str(indoc! {"
            services:
              web:
                ports:
                  - target: 3000
                    published: \"${UNKNOWN_SYNTAX}\"
        "});
        assert!(info.hardcoded_ports.is_empty());
        assert!(info.port_mappings.is_empty());
        let port_warnings: Vec<_> = info
            .warnings
            .iter()
            .filter(|w| w.directive == "ports (long syntax)")
            .collect();
        assert_eq!(port_warnings.len(), 1);
    }
}
