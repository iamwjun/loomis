use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoomisConfig {
    pub servers: Vec<ServerConfig>,
}

impl LoomisConfig {
    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        let source = fs::read_to_string(path).map_err(|error| ConfigError::Read {
            path: path.to_path_buf(),
            source: error,
        })?;
        let raw = toml::from_str::<RawConfig>(&source).map_err(|error| ConfigError::Parse {
            path: path.to_path_buf(),
            source: error,
        })?;
        let base_dir = path.parent().unwrap_or_else(|| Path::new("."));

        Self::from_raw(raw, base_dir)
    }

    pub fn single_static_site(root_dir: impl AsRef<Path>, port: u16) -> Result<Self, ConfigError> {
        let raw = RawConfig {
            servers: vec![RawServerConfig {
                listen: format!("127.0.0.1:{port}"),
                server_name: Vec::new(),
                locations: vec![RawLocationConfig {
                    path: String::from("/"),
                    root: Some(root_dir.as_ref().to_path_buf()),
                    index: vec![String::from("index.html")],
                    proxy_pass: None,
                }],
            }],
        };

        Self::from_raw(raw, Path::new("."))
    }

    fn from_raw(raw: RawConfig, base_dir: &Path) -> Result<Self, ConfigError> {
        if raw.servers.is_empty() {
            return Err(ConfigError::Validation(String::from(
                "config must define at least one [[server]] block",
            )));
        }

        let mut servers = Vec::with_capacity(raw.servers.len());
        for (index, raw_server) in raw.servers.into_iter().enumerate() {
            servers.push(ServerConfig::from_raw(raw_server, base_dir, index)?);
        }

        validate_server_name_collisions(&servers)?;

        Ok(Self { servers })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    pub listen: SocketAddr,
    pub server_names: Vec<String>,
    pub locations: Vec<LocationConfig>,
}

impl ServerConfig {
    fn from_raw(
        raw: RawServerConfig,
        base_dir: &Path,
        index: usize,
    ) -> Result<Self, ConfigError> {
        let listen = raw.listen.parse::<SocketAddr>().map_err(|_| {
            ConfigError::Validation(format!(
                "server[{index}] has invalid listen address: {}",
                raw.listen
            ))
        })?;

        if raw.locations.is_empty() {
            return Err(ConfigError::Validation(format!(
                "server[{index}] must define at least one [[server.location]] block"
            )));
        }

        let mut seen_paths = HashSet::new();
        let mut locations = Vec::with_capacity(raw.locations.len());
        for (location_index, raw_location) in raw.locations.into_iter().enumerate() {
            let location = LocationConfig::from_raw(raw_location, base_dir, index, location_index)?;
            if !seen_paths.insert(location.path.clone()) {
                return Err(ConfigError::Validation(format!(
                    "server[{index}] defines duplicate location path: {}",
                    location.path
                )));
            }
            locations.push(location);
        }

        Ok(Self {
            listen,
            server_names: raw
                .server_name
                .into_iter()
                .map(|name| name.to_ascii_lowercase())
                .collect(),
            locations,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocationConfig {
    pub path: String,
    pub handler: LocationHandler,
    pub index: Vec<String>,
}

impl LocationConfig {
    fn from_raw(
        raw: RawLocationConfig,
        base_dir: &Path,
        server_index: usize,
        location_index: usize,
    ) -> Result<Self, ConfigError> {
        let RawLocationConfig {
            path: raw_path,
            root,
            index: raw_index,
            proxy_pass,
        } = raw;

        if !raw_path.starts_with('/') {
            return Err(ConfigError::Validation(format!(
                "server[{server_index}].location[{location_index}] path must start with '/'"
            )));
        }

        let path = normalize_location_path(&raw_path);
        let has_custom_index = !raw_index.is_empty();
        let index = if raw_index.is_empty() {
            vec![String::from("index.html")]
        } else {
            raw_index
        };

        let handler = match (root, proxy_pass) {
            (Some(root), None) => {
                let resolved = resolve_config_path(base_dir, &root);
                let canonical = resolved.canonicalize().map_err(|_| {
                    ConfigError::Validation(format!(
                        "server[{server_index}].location[{location_index}] root does not exist: {}",
                        resolved.display()
                    ))
                })?;
                if !canonical.is_dir() {
                    return Err(ConfigError::Validation(format!(
                        "server[{server_index}].location[{location_index}] root is not a directory: {}",
                        resolved.display()
                    )));
                }

                LocationHandler::Static { root: canonical }
            }
            (None, Some(proxy_pass)) => {
                if !has_custom_index {
                    LocationHandler::Proxy {
                        upstream: ProxyTarget::parse(&proxy_pass).map_err(|message| {
                            ConfigError::Validation(format!(
                                "server[{server_index}].location[{location_index}] {message}"
                            ))
                        })?,
                    }
                } else {
                    return Err(ConfigError::Validation(format!(
                        "server[{server_index}].location[{location_index}] proxy locations cannot define index"
                    )));
                }
            }
            (Some(_), Some(_)) => {
                return Err(ConfigError::Validation(format!(
                    "server[{server_index}].location[{location_index}] must define either root or proxy_pass, not both"
                )));
            }
            (None, None) => {
                return Err(ConfigError::Validation(format!(
                    "server[{server_index}].location[{location_index}] must define root or proxy_pass"
                )));
            }
        };

        Ok(Self {
            path,
            handler,
            index,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocationHandler {
    Static { root: PathBuf },
    Proxy { upstream: ProxyTarget },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyTarget {
    pub host: String,
    pub port: u16,
    pub base_path: String,
}

impl ProxyTarget {
    fn parse(value: &str) -> Result<Self, String> {
        let Some(remainder) = value.strip_prefix("http://") else {
            return Err(String::from("proxy_pass must start with http://"));
        };

        let (authority, raw_path) = match remainder.split_once('/') {
            Some((authority, path)) => (authority, format!("/{path}")),
            None => (remainder, String::from("/")),
        };

        if authority.is_empty() {
            return Err(String::from("proxy_pass is missing an upstream host"));
        }

        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port)) if !host.is_empty() => {
                let port = port
                    .parse::<u16>()
                    .map_err(|_| format!("proxy_pass contains an invalid port: {port}"))?;
                (host.to_string(), port)
            }
            _ => (authority.to_string(), 80),
        };

        Ok(Self {
            host,
            port,
            base_path: normalize_proxy_base_path(&raw_path),
        })
    }

    pub fn authority(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

#[derive(Debug)]
pub enum ConfigError {
    Read { path: PathBuf, source: io::Error },
    Parse { path: PathBuf, source: toml::de::Error },
    Validation(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => {
                write!(f, "failed to read config {}: {source}", path.display())
            }
            Self::Parse { path, source } => {
                write!(f, "failed to parse config {}: {source}", path.display())
            }
            Self::Validation(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
            Self::Validation(_) => None,
        }
    }
}

fn validate_server_name_collisions(servers: &[ServerConfig]) -> Result<(), ConfigError> {
    let mut listeners: HashMap<SocketAddr, HashSet<String>> = HashMap::new();
    let mut default_servers = HashSet::new();

    for server in servers {
        if server.server_names.is_empty() && !default_servers.insert(server.listen) {
            return Err(ConfigError::Validation(format!(
                "multiple default servers configured for {}",
                server.listen
            )));
        }

        let names = listeners.entry(server.listen).or_default();
        for server_name in &server.server_names {
            if !names.insert(server_name.clone()) {
                return Err(ConfigError::Validation(format!(
                    "duplicate server_name '{}' configured for {}",
                    server_name, server.listen
                )));
            }
        }
    }

    Ok(())
}

fn normalize_location_path(path: &str) -> String {
    if path == "/" {
        return String::from("/");
    }

    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        String::from("/")
    } else {
        trimmed.to_string()
    }
}

fn normalize_proxy_base_path(path: &str) -> String {
    if path == "/" {
        return String::from("/");
    }

    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        String::from("/")
    } else if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    }
}

fn resolve_config_path(base_dir: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.join(path)
    }
}

#[derive(Debug, Deserialize)]
struct RawConfig {
    #[serde(rename = "server")]
    servers: Vec<RawServerConfig>,
}

#[derive(Debug, Deserialize)]
struct RawServerConfig {
    listen: String,
    #[serde(default)]
    server_name: Vec<String>,
    #[serde(default, rename = "location")]
    locations: Vec<RawLocationConfig>,
}

#[derive(Debug, Deserialize)]
struct RawLocationConfig {
    path: String,
    root: Option<PathBuf>,
    #[serde(default)]
    index: Vec<String>,
    proxy_pass: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn single_static_site_builds_default_server() {
        let root = create_temp_dir();
        fs::write(root.join("index.html"), "ok").expect("failed to write fixture");

        let config =
            LoomisConfig::single_static_site(&root, 3000).expect("config should be valid");

        assert_eq!(config.servers.len(), 1);
        assert_eq!(config.servers[0].listen.to_string(), "127.0.0.1:3000");
        assert_eq!(config.servers[0].locations[0].path, "/");

        remove_temp_dir(&root);
    }

    #[test]
    fn load_from_path_resolves_relative_static_root() {
        let root = create_temp_dir();
        let site_dir = root.join("site");
        fs::create_dir_all(&site_dir).expect("failed to create site dir");
        fs::write(site_dir.join("index.html"), "ok").expect("failed to write fixture");
        let config_path = root.join("loomis.toml");
        fs::write(
            &config_path,
            r#"
[[server]]
listen = "127.0.0.1:3000"
server_name = ["example.test"]

  [[server.location]]
  path = "/"
  root = "site"
"#,
        )
        .expect("failed to write config");

        let config = LoomisConfig::load_from_path(&config_path).expect("config should load");
        let LocationHandler::Static { root } = &config.servers[0].locations[0].handler else {
            panic!("expected static handler");
        };

        assert_eq!(
            root,
            &site_dir
                .canonicalize()
                .expect("failed to canonicalize fixture dir")
        );
        assert_eq!(
            config.servers[0].locations[0].index,
            vec![String::from("index.html")]
        );

        remove_temp_dir(&root);
    }

    #[test]
    fn rejects_duplicate_server_names_on_same_listener() {
        let root = create_temp_dir();
        fs::write(root.join("index.html"), "ok").expect("failed to write fixture");
        let config_path = root.join("loomis.toml");
        fs::write(
            &config_path,
            r#"
[[server]]
listen = "127.0.0.1:3000"
server_name = ["example.test"]

  [[server.location]]
  path = "/"
  root = "."

[[server]]
listen = "127.0.0.1:3000"
server_name = ["example.test"]

  [[server.location]]
  path = "/"
  root = "."
"#,
        )
        .expect("failed to write config");

        let error = LoomisConfig::load_from_path(&config_path).expect_err("config should fail");

        assert!(error
            .to_string()
            .contains("duplicate server_name 'example.test' configured for 127.0.0.1:3000"));

        remove_temp_dir(&root);
    }

    #[test]
    fn rejects_location_without_handler() {
        let root = create_temp_dir();
        let config_path = root.join("loomis.toml");
        fs::write(
            &config_path,
            r#"
[[server]]
listen = "127.0.0.1:3000"

  [[server.location]]
  path = "/"
"#,
        )
        .expect("failed to write config");

        let error = LoomisConfig::load_from_path(&config_path).expect_err("config should fail");

        assert!(error
            .to_string()
            .contains("server[0].location[0] must define root or proxy_pass"));

        remove_temp_dir(&root);
    }

    #[test]
    fn parses_proxy_upstream_with_default_port() {
        let upstream =
            ProxyTarget::parse("http://backend.internal/api").expect("proxy target should parse");

        assert_eq!(upstream.host, "backend.internal");
        assert_eq!(upstream.port, 80);
        assert_eq!(upstream.base_path, "/api");
    }

    fn create_temp_dir() -> PathBuf {
        let unique = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("loomis-config-test-{unique}"));
        fs::create_dir_all(&root).expect("failed to create temp dir");
        root
    }

    fn remove_temp_dir(root: &Path) {
        fs::remove_dir_all(root).expect("failed to clean up temp dir");
    }
}
