//! Client configuration management

use anyhow::Result;
use shared::types::*;
use std::fs;
use uuid::Uuid;

pub struct ClientConfigManager {
    config_path: std::path::PathBuf,
}

impl ClientConfigManager {
    pub fn new(config_path: &std::path::Path) -> Result<Self> {
        let manager = Self {
            config_path: config_path.to_path_buf(),
        };

        // Create default config if it doesn't exist
        if !config_path.exists() {
            manager.create_default_config()?;
        }

        Ok(manager)
    }

    fn create_default_config(&self) -> Result<()> {
        let default_config = ClientConfig {
            servers: Vec::new(),
            update_interval_seconds: 5, // Default 5 seconds as requested
            auto_reconnect: true,
            reconnect_delay_seconds: 5,
            private_key_path: None,
            public_key_path: None,
            tls: None,
            notifications: shared::notifications::NotificationConfig::default(),
        };

        self.save_config(&default_config)?;
        Ok(())
    }

    pub fn load_config(&self) -> Result<ClientConfig> {
        if !self.config_path.exists() {
            return Ok(ClientConfig {
                servers: Vec::new(),
                update_interval_seconds: 5,
                auto_reconnect: true,
                reconnect_delay_seconds: 5,
                private_key_path: None,
                public_key_path: None,
                tls: None,
                notifications: shared::notifications::NotificationConfig::default(),
            });
        }

        let content = fs::read_to_string(&self.config_path)?;
        let config: ClientConfig = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn save_config(&self, config: &ClientConfig) -> Result<()> {
        let content = toml::to_string_pretty(config)?;
        fs::write(&self.config_path, content)?;
        Ok(())
    }

    pub fn config_path(&self) -> &std::path::Path {
        &self.config_path
    }

    pub fn add_server(&mut self, server: ServerEndpoint) -> Result<()> {
        let mut config = self.load_config()?;

        // Check if server with same address and port already exists
        if config
            .servers
            .iter()
            .any(|s| s.address == server.address && s.port == server.port)
        {
            return Err(anyhow::anyhow!(
                "Server with address {}:{} already exists",
                server.address,
                server.port
            ));
        }

        config.servers.push(server);
        self.save_config(&config)?;
        Ok(())
    }

    pub fn remove_server(&mut self, id: Uuid) -> Result<()> {
        let mut config = self.load_config()?;

        let original_len = config.servers.len();
        config.servers.retain(|s| s.id != id);

        if config.servers.len() == original_len {
            return Err(anyhow::anyhow!("Server with ID {} not found", id));
        }

        self.save_config(&config)?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn update_server(&mut self, id: Uuid, updated_server: ServerEndpoint) -> Result<()> {
        let mut config = self.load_config()?;

        if let Some(server) = config.servers.iter_mut().find(|s| s.id == id) {
            *server = updated_server;
            self.save_config(&config)?;
            Ok(())
        } else {
            Err(anyhow::anyhow!("Server with ID {} not found", id))
        }
    }

    #[allow(dead_code)]
    pub fn get_server(&self, id: Uuid) -> Result<ServerEndpoint> {
        let config = self.load_config()?;

        config
            .servers
            .into_iter()
            .find(|s| s.id == id)
            .ok_or_else(|| anyhow::anyhow!("Server with ID {} not found", id))
    }

    #[allow(dead_code)]
    pub fn list_servers(&self) -> Result<Vec<ServerEndpoint>> {
        let config = self.load_config()?;
        Ok(config.servers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_config_manager() {
        // Use a temp directory and a config file path that doesn't exist yet
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("test-config.toml");

        // File doesn't exist yet, so ClientConfigManager will create a default config
        let mut manager = ClientConfigManager::new(&config_path).unwrap();

        // Test adding a server
        let server = ServerEndpoint {
            id: uuid::Uuid::new_v4(),
            name: "test-server".to_string(),
            address: "192.168.1.100".to_string(),
            port: 50051,
            description: Some("Test server".to_string()),
            access_token: None,
        };

        assert!(manager.add_server(server.clone()).is_ok());

        // Test loading and listing servers
        let servers = manager.list_servers().unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].name, "test-server");

        // Test removing server
        assert!(manager.remove_server(server.id).is_ok());

        let servers = manager.list_servers().unwrap();
        assert_eq!(servers.len(), 0);
    }

    #[test]
    fn test_config_manager_duplicate_server() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("test-config.toml");
        let mut manager = ClientConfigManager::new(&config_path).unwrap();

        let server1 = ServerEndpoint {
            id: uuid::Uuid::new_v4(),
            name: "server-1".to_string(),
            address: "192.168.1.100".to_string(),
            port: 50051,
            description: None,
            access_token: None,
        };
        let server2 = ServerEndpoint {
            id: uuid::Uuid::new_v4(),
            name: "server-2".to_string(),
            address: "192.168.1.100".to_string(),
            port: 50051,
            description: None,
            access_token: None,
        };

        assert!(manager.add_server(server1).is_ok());
        let result = manager.add_server(server2);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already exists"));
    }

    #[test]
    fn test_config_manager_update_server() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("test-config.toml");
        let mut manager = ClientConfigManager::new(&config_path).unwrap();

        let server = ServerEndpoint {
            id: uuid::Uuid::new_v4(),
            name: "old-name".to_string(),
            address: "192.168.1.100".to_string(),
            port: 50051,
            description: None,
            access_token: None,
        };
        manager.add_server(server.clone()).unwrap();

        let updated = ServerEndpoint {
            id: server.id,
            name: "new-name".to_string(),
            address: "192.168.1.200".to_string(),
            port: 50052,
            description: Some("Updated".to_string()),
            access_token: Some("token".to_string()),
        };

        assert!(manager.update_server(server.id, updated).is_ok());
        let retrieved = manager.get_server(server.id).unwrap();
        assert_eq!(retrieved.name, "new-name");
        assert_eq!(retrieved.address, "192.168.1.200");
        assert_eq!(retrieved.port, 50052);
    }

    #[test]
    fn test_config_manager_get_server_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("test-config.toml");
        let manager = ClientConfigManager::new(&config_path).unwrap();

        let result = manager.get_server(uuid::Uuid::new_v4());
        assert!(result.is_err());
    }

    #[test]
    fn test_config_manager_remove_nonexistent() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("test-config.toml");
        let mut manager = ClientConfigManager::new(&config_path).unwrap();

        let result = manager.remove_server(uuid::Uuid::new_v4());
        assert!(result.is_err());
    }

    #[test]
    fn test_config_manager_load_default_when_missing() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("nonexistent-config.toml");
        let manager = ClientConfigManager::new(&config_path).unwrap();

        let config = manager.load_config().unwrap();
        assert!(config.servers.is_empty());
        assert_eq!(config.update_interval_seconds, 5);
        assert!(config.auto_reconnect);
    }

    #[test]
    fn test_config_manager_load_config_file_deleted() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("deleted-config.toml");
        let manager = ClientConfigManager::new(&config_path).unwrap();

        // Delete the file after creation to exercise the early-return path
        std::fs::remove_file(&config_path).unwrap();

        let config = manager.load_config().unwrap();
        assert!(config.servers.is_empty());
        assert_eq!(config.update_interval_seconds, 5);
        assert!(config.auto_reconnect);
        assert_eq!(config.reconnect_delay_seconds, 5);
        assert!(config.private_key_path.is_none());
        assert!(config.public_key_path.is_none());
        assert!(config.tls.is_none());
    }

    #[test]
    fn test_config_manager_update_server_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("test-config.toml");
        let mut manager = ClientConfigManager::new(&config_path).unwrap();

        let updated = ServerEndpoint {
            id: uuid::Uuid::new_v4(),
            name: "new-name".to_string(),
            address: "192.168.1.200".to_string(),
            port: 50052,
            description: Some("Updated".to_string()),
            access_token: Some("token".to_string()),
        };

        let result = manager.update_server(updated.id, updated);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_config_manager_save_and_load() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("test-config.toml");
        let manager = ClientConfigManager::new(&config_path).unwrap();

        let server = ServerEndpoint {
            id: uuid::Uuid::new_v4(),
            name: "test-server".to_string(),
            address: "192.168.1.100".to_string(),
            port: 50051,
            description: Some("Test".to_string()),
            access_token: Some("token".to_string()),
        };

        let mut config = manager.load_config().unwrap();
        config.servers.push(server);
        manager.save_config(&config).unwrap();

        let loaded = manager.load_config().unwrap();
        assert_eq!(loaded.servers.len(), 1);
        assert_eq!(loaded.servers[0].name, "test-server");
        assert_eq!(loaded.servers[0].access_token.as_ref().unwrap(), "token");
    }
}
