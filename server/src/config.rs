//! Configuration management for the monitoring server

use anyhow::Result;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as BASE64, Engine as _};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub update_interval_seconds: u64,
    pub max_clients: usize,
    pub enable_authentication: bool,
    pub log_level: String,
    /// Access token that clients must provide to connect
    /// If empty, a new one will be generated on startup
    #[serde(default)]
    pub access_token: String,
    /// List of authorized client public keys (base64 encoded)
    #[serde(default)]
    pub authorized_clients: Vec<AuthorizedClient>,
    /// Postgres clusters to monitor
    #[serde(default)]
    pub postgres_clusters: Vec<PostgresClusterConfig>,
    /// MariaDB clusters to monitor
    #[serde(default)]
    pub mariadb_clusters: Vec<MariaDBClusterConfig>,
    /// Systemd units to monitor
    #[serde(default)]
    pub systemd_units: Vec<String>,
    /// TLS configuration
    #[serde(default)]
    pub tls: Option<TlsConfig>,
}

/// TLS configuration for the server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsConfig {
    /// Path to the server certificate (PEM)
    pub cert_path: String,
    /// Path to the server private key (PEM)
    pub key_path: String,
    /// Path to the CA certificate for client verification (optional, enables mTLS)
    pub ca_path: Option<String>,
}

/// Configuration for a Postgres cluster to monitor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostgresClusterConfig {
    /// Friendly name for this cluster
    pub name: String,
    /// Connection host (default: localhost)
    #[serde(default = "default_localhost")]
    pub host: String,
    /// Connection port (default: 5432)
    #[serde(default = "default_pg_port")]
    pub port: u16,
    /// Database name to connect to (default: postgres)
    #[serde(default = "default_postgres_db")]
    pub database: String,
    /// Username (default: code-monitor)
    #[serde(default = "default_pg_user")]
    pub user: String,
    /// Password (optional; prefer socket+peer auth)
    pub password: Option<String>,
    /// Unix socket path (alternative to TCP)
    pub socket_path: Option<String>,
    /// Whether this cluster is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// Configuration for a MariaDB cluster to monitor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MariaDBClusterConfig {
    /// Friendly name for this cluster
    pub name: String,
    /// Connection host (default: localhost)
    #[serde(default = "default_localhost")]
    pub host: String,
    /// Connection port (default: 3306)
    #[serde(default = "default_mysql_port")]
    pub port: u16,
    /// Username (default: code-monitor)
    #[serde(default = "default_mysql_user")]
    pub user: String,
    /// Password (optional)
    pub password: Option<String>,
    /// Unix socket path (alternative to TCP)
    pub socket_path: Option<String>,
    /// Whether this cluster is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_mysql_port() -> u16 {
    3306
}
fn default_mysql_user() -> String {
    "code-monitor".to_string()
}

fn default_localhost() -> String {
    "localhost".to_string()
}
fn default_pg_port() -> u16 {
    5432
}
fn default_postgres_db() -> String {
    "postgres".to_string()
}
fn default_pg_user() -> String {
    "code-monitor".to_string()
}
fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizedClient {
    /// Friendly name for this client
    pub name: String,
    /// Base64 encoded public key
    pub public_key: String,
    /// When this client was authorized
    #[serde(default = "default_timestamp")]
    pub authorized_at: String,
}

fn default_timestamp() -> String {
    chrono::Utc::now().to_rfc3339()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            update_interval_seconds: 5,
            max_clients: 100,
            enable_authentication: true,
            log_level: "info".to_string(),
            access_token: String::new(),
            authorized_clients: Vec::new(),
            postgres_clusters: Vec::new(),
            mariadb_clusters: Vec::new(),
            systemd_units: Vec::new(),
            tls: None,
        }
    }
}

impl Config {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();

        if !path.exists() {
            let mut config = Self::default();
            config.generate_access_token();
            config.save(path)?;
            return Ok(config);
        }

        let content = fs::read_to_string(path)?;
        let mut config: Config = toml::from_str(&content)?;

        // Generate access token if empty
        if config.access_token.is_empty() {
            config.generate_access_token();
            config.save(path)?;
        }

        Ok(config)
    }

    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let content = toml::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
    }

    /// Generate a user-friendly access token
    pub fn generate_access_token(&mut self) {
        let mut rng = rand::thread_rng();
        let bytes: [u8; 16] = rng.gen();
        self.access_token = BASE64.encode(bytes);
    }

    /// Validate an access token
    pub fn validate_access_token(&self, token: &str) -> bool {
        if !self.enable_authentication {
            return true;
        }
        self.access_token == token
    }

    /// Add an authorized client
    #[allow(dead_code)]
    pub fn add_authorized_client(&mut self, name: String, public_key: String) {
        // Check if already exists
        if self
            .authorized_clients
            .iter()
            .any(|c| c.public_key == public_key)
        {
            return;
        }

        self.authorized_clients.push(AuthorizedClient {
            name,
            public_key,
            authorized_at: chrono::Utc::now().to_rfc3339(),
        });
    }

    /// Remove an authorized client by name
    pub fn remove_authorized_client(&mut self, name: &str) -> bool {
        let original_len = self.authorized_clients.len();
        self.authorized_clients.retain(|c| c.name != name);
        self.authorized_clients.len() != original_len
    }

    /// Check if a public key is authorized
    #[allow(dead_code)]
    pub fn is_client_authorized(&self, public_key: &str) -> bool {
        if !self.enable_authentication {
            return true;
        }
        self.authorized_clients
            .iter()
            .any(|c| c.public_key == public_key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_config_save_load() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        let config = Config {
            update_interval_seconds: 10,
            max_clients: 50,
            enable_authentication: false,
            log_level: "debug".to_string(),
            access_token: "test-token".to_string(),
            authorized_clients: Vec::new(),
            postgres_clusters: Vec::new(),
            mariadb_clusters: Vec::new(),
            systemd_units: Vec::new(),
            tls: None,
        };

        config.save(path).unwrap();
        let loaded_config = Config::load(path).unwrap();

        assert_eq!(
            config.update_interval_seconds,
            loaded_config.update_interval_seconds
        );
        assert_eq!(config.max_clients, loaded_config.max_clients);
        assert_eq!(
            config.enable_authentication,
            loaded_config.enable_authentication
        );
        assert_eq!(config.log_level, loaded_config.log_level);
    }

    #[test]
    fn test_access_token_generation() {
        let mut config = Config::default();
        assert!(config.access_token.is_empty());
        config.generate_access_token();
        assert!(!config.access_token.is_empty());
        assert!(config.access_token.len() >= 20);
    }

    #[test]
    fn test_validate_access_token_disabled() {
        let mut config = Config::default();
        config.enable_authentication = false;
        config.access_token = "secret".to_string();
        assert!(config.validate_access_token("anything"));
    }

    #[test]
    fn test_validate_access_token_enabled_valid() {
        let mut config = Config::default();
        config.enable_authentication = true;
        config.access_token = "secret".to_string();
        assert!(config.validate_access_token("secret"));
    }

    #[test]
    fn test_validate_access_token_enabled_invalid() {
        let mut config = Config::default();
        config.enable_authentication = true;
        config.access_token = "secret".to_string();
        assert!(!config.validate_access_token("wrong"));
    }

    #[test]
    fn test_remove_authorized_client() {
        let mut config = Config::default();
        config.authorized_clients.push(AuthorizedClient {
            name: "client-1".to_string(),
            public_key: "pk1".to_string(),
            authorized_at: "2024-01-01T00:00:00Z".to_string(),
        });
        config.authorized_clients.push(AuthorizedClient {
            name: "client-2".to_string(),
            public_key: "pk2".to_string(),
            authorized_at: "2024-01-01T00:00:00Z".to_string(),
        });

        assert!(config.remove_authorized_client("client-1"));
        assert_eq!(config.authorized_clients.len(), 1);
        assert!(!config.remove_authorized_client("client-1"));
    }

    #[test]
    fn test_add_authorized_client_deduplicate() {
        let mut config = Config::default();
        config.add_authorized_client("client-1".to_string(), "pk1".to_string());
        config.add_authorized_client("client-1-again".to_string(), "pk1".to_string());
        assert_eq!(config.authorized_clients.len(), 1);
    }

    #[test]
    fn test_is_client_authorized_disabled() {
        let mut config = Config::default();
        config.enable_authentication = false;
        assert!(config.is_client_authorized("any"));
    }

    #[test]
    fn test_is_client_authorized_enabled() {
        let mut config = Config::default();
        config.enable_authentication = true;
        config.authorized_clients.push(AuthorizedClient {
            name: "client-1".to_string(),
            public_key: "pk1".to_string(),
            authorized_at: "2024-01-01T00:00:00Z".to_string(),
        });
        assert!(config.is_client_authorized("pk1"));
        assert!(!config.is_client_authorized("pk2"));
    }

    #[test]
    fn test_postgres_cluster_config_defaults() {
        let config: PostgresClusterConfig = toml::from_str(r#"
            name = "test"
        "#).unwrap();
        assert_eq!(config.name, "test");
        assert_eq!(config.host, "localhost");
        assert_eq!(config.port, 5432);
        assert_eq!(config.database, "postgres");
        assert_eq!(config.user, "code-monitor");
        assert!(config.enabled);
    }

    #[test]
    fn test_mariadb_cluster_config_defaults() {
        let config: MariaDBClusterConfig = toml::from_str(r#"
            name = "test"
        "#).unwrap();
        assert_eq!(config.name, "test");
        assert_eq!(config.host, "localhost");
        assert_eq!(config.port, 3306);
        assert_eq!(config.user, "code-monitor");
        assert!(config.enabled);
    }

    #[test]
    fn test_config_default_values() {
        let config = Config::default();
        assert_eq!(config.update_interval_seconds, 5);
        assert_eq!(config.max_clients, 100);
        assert!(config.enable_authentication);
        assert_eq!(config.log_level, "info");
        assert!(config.access_token.is_empty());
        assert!(config.tls.is_none());
    }

    #[test]
    fn test_config_load_creates_default_when_missing() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let path = temp_dir.path().join("nonexistent-config.toml");

        let config = Config::load(&path).unwrap();
        assert_eq!(config.update_interval_seconds, 5);
        assert_eq!(config.max_clients, 100);
        assert!(config.enable_authentication);
        assert!(!config.access_token.is_empty(), "Token should be auto-generated");

        // File should have been created
        assert!(path.exists());
    }

    #[test]
    fn test_config_load_regenerates_empty_token() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        let mut config = Config::default();
        config.access_token = String::new();
        config.save(path).unwrap();

        let loaded = Config::load(path).unwrap();
        assert!(!loaded.access_token.is_empty(), "Empty token should be regenerated on load");
    }

    #[test]
    fn test_config_full_roundtrip() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        let config = Config {
            update_interval_seconds: 30,
            max_clients: 200,
            enable_authentication: true,
            log_level: "warn".to_string(),
            access_token: "my-secret-token".to_string(),
            authorized_clients: vec![AuthorizedClient {
                name: "client-1".to_string(),
                public_key: "abc123".to_string(),
                authorized_at: "2024-06-01T12:00:00Z".to_string(),
            }],
            postgres_clusters: vec![PostgresClusterConfig {
                name: "pg-main".to_string(),
                host: "pg.example.com".to_string(),
                port: 5433,
                database: "app".to_string(),
                user: "monitor".to_string(),
                password: Some("secret".to_string()),
                socket_path: Some("/run/pg".to_string()),
                enabled: false,
            }],
            mariadb_clusters: vec![MariaDBClusterConfig {
                name: "maria-main".to_string(),
                host: "maria.example.com".to_string(),
                port: 3307,
                user: "monitor".to_string(),
                password: Some("secret".to_string()),
                socket_path: None,
                enabled: true,
            }],
            systemd_units: vec!["nginx.service".to_string(), "postgresql.service".to_string()],
            tls: Some(TlsConfig {
                cert_path: "/etc/ssl/server.crt".to_string(),
                key_path: "/etc/ssl/server.key".to_string(),
                ca_path: Some("/etc/ssl/ca.crt".to_string()),
            }),
        };

        config.save(path).unwrap();
        let loaded = Config::load(path).unwrap();

        assert_eq!(loaded.update_interval_seconds, 30);
        assert_eq!(loaded.max_clients, 200);
        assert_eq!(loaded.log_level, "warn");
        assert_eq!(loaded.access_token, "my-secret-token");
        assert_eq!(loaded.authorized_clients.len(), 1);
        assert_eq!(loaded.postgres_clusters.len(), 1);
        assert_eq!(loaded.mariadb_clusters.len(), 1);
        assert_eq!(loaded.systemd_units.len(), 2);
        assert!(loaded.tls.is_some());

        let tls = loaded.tls.unwrap();
        assert_eq!(tls.cert_path, "/etc/ssl/server.crt");
        assert_eq!(tls.key_path, "/etc/ssl/server.key");
        assert_eq!(tls.ca_path, Some("/etc/ssl/ca.crt".to_string()));

        let pg = &loaded.postgres_clusters[0];
        assert_eq!(pg.name, "pg-main");
        assert_eq!(pg.host, "pg.example.com");
        assert_eq!(pg.port, 5433);
        assert_eq!(pg.database, "app");
        assert_eq!(pg.user, "monitor");
        assert_eq!(pg.password, Some("secret".to_string()));
        assert_eq!(pg.socket_path, Some("/run/pg".to_string()));
        assert!(!pg.enabled);

        let maria = &loaded.mariadb_clusters[0];
        assert_eq!(maria.name, "maria-main");
        assert_eq!(maria.host, "maria.example.com");
        assert_eq!(maria.port, 3307);
        assert_eq!(maria.user, "monitor");
        assert_eq!(maria.password, Some("secret".to_string()));
        assert!(maria.enabled);
    }

    #[test]
    fn test_tls_config_roundtrip() {
        let tls = TlsConfig {
            cert_path: "/certs/server.crt".to_string(),
            key_path: "/certs/server.key".to_string(),
            ca_path: None,
        };

        let serialized = toml::to_string_pretty(&tls).unwrap();
        let deserialized: TlsConfig = toml::from_str(&serialized).unwrap();

        assert_eq!(deserialized.cert_path, tls.cert_path);
        assert_eq!(deserialized.key_path, tls.key_path);
        assert_eq!(deserialized.ca_path, tls.ca_path);
    }

    #[test]
    fn test_authorized_client_default_timestamp() {
        let client = AuthorizedClient {
            name: "test".to_string(),
            public_key: "pk".to_string(),
            authorized_at: default_timestamp(),
        };
        assert!(!client.authorized_at.is_empty());
        assert!(client.authorized_at.contains('T'));
    }

    #[test]
    fn test_add_authorized_client_new() {
        let mut config = Config::default();
        config.add_authorized_client("client-a".to_string(), "pk-a".to_string());
        assert_eq!(config.authorized_clients.len(), 1);
        assert_eq!(config.authorized_clients[0].name, "client-a");
        assert_eq!(config.authorized_clients[0].public_key, "pk-a");
    }

    #[test]
    fn test_is_client_authorized_empty_list() {
        let mut config = Config::default();
        config.enable_authentication = true;
        assert!(!config.is_client_authorized("any-key"));
    }

    #[test]
    fn test_postgres_cluster_config_with_overrides() {
        let config: PostgresClusterConfig = toml::from_str(r#"
            name = "custom"
            host = "192.168.1.50"
            port = 5433
            database = "metrics"
            user = "admin"
            enabled = false
        "#).unwrap();
        assert_eq!(config.name, "custom");
        assert_eq!(config.host, "192.168.1.50");
        assert_eq!(config.port, 5433);
        assert_eq!(config.database, "metrics");
        assert_eq!(config.user, "admin");
        assert!(!config.enabled);
    }

    #[test]
    fn test_mariadb_cluster_config_with_overrides() {
        let config: MariaDBClusterConfig = toml::from_str(r#"
            name = "custom"
            host = "192.168.1.60"
            port = 3307
            user = "admin"
            enabled = false
        "#).unwrap();
        assert_eq!(config.name, "custom");
        assert_eq!(config.host, "192.168.1.60");
        assert_eq!(config.port, 3307);
        assert_eq!(config.user, "admin");
        assert!(!config.enabled);
    }

    #[test]
    fn test_config_load_invalid_toml() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();
        fs::write(path, "this is not valid toml [[[").unwrap();
        let result = Config::load(path);
        assert!(result.is_err());
    }
}
