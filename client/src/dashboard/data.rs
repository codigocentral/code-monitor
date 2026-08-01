use anyhow::Result;
use shared::types::ServerEndpoint;

use crate::client::MonitorClient;

use super::{ConnectionStatus, DashboardApp};
pub(super) async fn connect_to_server(
    server: &ServerEndpoint,
    update_interval: u64,
    tls_config: Option<&shared::types::ClientTlsConfig>,
) -> Result<MonitorClient> {
    MonitorClient::connect_with_token(
        &format!("http://{}:{}", server.address, server.port),
        update_interval,
        true,
        server.access_token.as_deref(),
        tls_config,
    )
    .await
}

pub(super) async fn fetch_server_data(app: &mut DashboardApp, server_id: uuid::Uuid) -> Result<()> {
    // First, fetch all data from the client
    let data = {
        if let Some(client) = app.connected_clients.get_mut(&server_id) {
            let system_info = client.get_system_info().await.ok();
            let services = client.get_services().await.ok();
            let processes = client.get_processes(100, None).await.ok();
            let networks = client.get_network_info().await.ok();
            let containers = client.get_containers(None).await.ok();
            let postgres = client.get_postgres_info().await.ok();
            let mariadb = client.get_mariadb_info().await.ok();
            let systemd = client.get_systemd_info().await.ok();
            let tls = client.get_tls_info().await.ok();
            let ports = client.get_listening_ports().await.ok();
            Some((
                system_info,
                services,
                processes,
                networks,
                containers,
                postgres,
                mariadb,
                systemd,
                tls,
                ports,
            ))
        } else {
            None
        }
    };

    // Then update the app state
    if let Some((
        system_info,
        services,
        processes,
        networks,
        containers,
        postgres,
        mariadb,
        systemd,
        tls,
        ports,
    )) = data
    {
        if let Some(ref info) = system_info {
            let mem_percent =
                (info.memory_used_bytes as f64 / info.memory_total_bytes as f64) * 100.0;
            app.update_history(server_id, info.cpu_usage_percent, mem_percent);

            // Save to storage
            if let Some(ref storage) = app.storage {
                let server_id_str = server_id.to_string();
                if let Err(e) = storage.store(&server_id_str, info) {
                    tracing::warn!("Failed to store metrics: {}", e);
                }
            }

            // Evaluate alerts
            let server_id_str = server_id.to_string();
            let server_name = app
                .get_selected_server()
                .map(|s| s.name.clone())
                .unwrap_or_else(|| "Unknown".to_string());
            let mem_percent =
                (info.memory_used_bytes as f64 / info.memory_total_bytes.max(1) as f64) * 100.0;
            let disk_percent = info
                .disk_info
                .first()
                .map(|d| (d.used_bytes as f64 / d.total_bytes.max(1) as f64) * 100.0)
                .unwrap_or(0.0);

            let new_alerts = app.alert_manager.process_metrics(
                &server_id_str,
                &server_name,
                info.cpu_usage_percent,
                mem_percent,
                disk_percent,
            );

            // Dispatch notifications for new alerts
            for alert in &new_alerts {
                let _ = app.notification_dispatcher.dispatch(alert).await;
            }

            app.system_info_cache.insert(server_id, info.clone());
        }
        if let Some(services) = services {
            app.services_cache.insert(server_id, services);
        }
        if let Some(processes) = processes {
            app.processes_cache.insert(server_id, processes);
        }
        if let Some(networks) = networks {
            app.network_cache.insert(server_id, networks);
        }
        if let Some(containers) = containers {
            let server_name = app
                .servers
                .iter()
                .find(|s| s.id == server_id)
                .map(|s| s.name.clone())
                .unwrap_or_else(|| "Unknown".to_string());
            let server_key = server_id.to_string();

            let restarts: Vec<(String, u32)> = containers
                .iter()
                .map(|c| (c.name.clone(), c.restart_count))
                .collect();
            for alert in
                app.alert_manager
                    .process_container_restarts(&server_key, &server_name, &restarts)
            {
                let _ = app.notification_dispatcher.dispatch(&alert).await;
            }

            let broken_checks: Vec<String> = containers
                .iter()
                .filter(|c| {
                    c.health_detail
                        .as_ref()
                        .map(|h| h.assess() == shared::types::HealthAssessment::BrokenCheck)
                        .unwrap_or(false)
                })
                .map(|c| c.name.clone())
                .collect();
            if let Some(alert) = app.alert_manager.process_broken_healthchecks(
                &server_key,
                &server_name,
                &broken_checks,
            ) {
                let _ = app.notification_dispatcher.dispatch(&alert).await;
            }

            app.containers_cache.insert(server_id, containers);
        }
        if let Some(postgres) = postgres {
            app.postgres_cache.insert(server_id, postgres);
        }
        if let Some(mariadb) = mariadb {
            app.mariadb_cache.insert(server_id, mariadb);
        }
        if let Some(systemd) = systemd {
            let failed_names: Vec<String> = systemd
                .failed_units
                .iter()
                .map(|u| u.name.clone())
                .collect();

            let server_name = app
                .servers
                .iter()
                .find(|s| s.id == server_id)
                .map(|s| s.name.clone())
                .unwrap_or_else(|| "Unknown".to_string());

            if let Some(alert) = app.alert_manager.process_systemd_failed_units(
                &server_id.to_string(),
                &server_name,
                &failed_names,
            ) {
                let _ = app.notification_dispatcher.dispatch(&alert).await;
            }

            app.systemd_cache.insert(server_id, systemd.units);
            app.systemd_failed_cache
                .insert(server_id, systemd.failed_units);
        }
        if let Some(tls) = tls {
            let server_name = app
                .servers
                .iter()
                .find(|s| s.id == server_id)
                .map(|s| s.name.clone())
                .unwrap_or_else(|| "Unknown".to_string());

            let certificates: Vec<(String, i64)> = tls
                .certificates
                .iter()
                .map(|c| (c.name.clone(), c.days_until_expiry))
                .collect();

            for alert in app.alert_manager.process_tls_certificates(
                &server_id.to_string(),
                &server_name,
                &certificates,
                tls.renewal_unit_name
                    .as_deref()
                    .zip(tls.renewal_unit_status.as_deref()),
            ) {
                let _ = app.notification_dispatcher.dispatch(&alert).await;
            }

            app.tls_cache.insert(server_id, tls);
        }
        if let Some(ports) = ports {
            let server_name = app
                .servers
                .iter()
                .find(|s| s.id == server_id)
                .map(|s| s.name.clone())
                .unwrap_or_else(|| "Unknown".to_string());

            let exposed: Vec<String> = ports
                .iter()
                .filter(|p| p.is_exposed_datastore())
                .map(|p| {
                    format!(
                        "{}:{}{}",
                        p.address,
                        p.port,
                        if p.process_name.is_empty() {
                            String::new()
                        } else {
                            format!(" ({})", p.process_name)
                        }
                    )
                })
                .collect();

            if let Some(alert) = app.alert_manager.process_exposed_datastores(
                &server_id.to_string(),
                &server_name,
                &exposed,
            ) {
                let _ = app.notification_dispatcher.dispatch(&alert).await;
            }

            app.ports_cache.insert(server_id, ports);
        }
    }
    Ok(())
}

pub(super) async fn handle_command(app: &mut DashboardApp, command: &str) {
    let parts: Vec<&str> = command.split_whitespace().collect();
    if parts.is_empty() {
        return;
    }

    match parts[0] {
        "connect" | "c" => {
            if let Some(server) = app.get_selected_server().cloned() {
                match connect_to_server(&server, app.update_interval, app.tls_config.as_ref()).await
                {
                    Ok(client) => {
                        app.connected_clients.insert(server.id, client);
                        app.connection_status
                            .insert(server.id, ConnectionStatus::Connected);
                        app.status_message = format!("Connected to {}", server.name);
                        let _ = fetch_server_data(app, server.id).await;
                    }
                    Err(e) => {
                        app.status_message = format!("Connection failed: {}", e);
                    }
                }
            }
        }
        "disconnect" | "d" => {
            if let Some(server) = app.get_selected_server().cloned() {
                app.connected_clients.remove(&server.id);
                app.connection_status
                    .insert(server.id, ConnectionStatus::Disconnected);
                app.status_message = format!("Disconnected from {}", server.name);
            }
        }
        "refresh" | "r" => {
            if let Some(server_id) = app.get_selected_server_id() {
                if let Err(e) = fetch_server_data(app, server_id).await {
                    app.status_message = format!("Refresh failed: {}", e);
                }
            }
        }
        "quit" | "q" => {
            app.running = false;
        }
        "help" | "h" => {
            app.status_message = "Commands: connect, disconnect, refresh, quit, help".to_string();
        }
        _ => {
            app.status_message = format!("Unknown command: {}", parts[0]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::notifications::NotificationConfig;
    use shared::types::ServerEndpoint;

    fn create_test_app() -> DashboardApp {
        let servers = vec![ServerEndpoint {
            id: uuid::Uuid::new_v4(),
            name: "test-server".to_string(),
            address: "127.0.0.1".to_string(),
            port: 50051,
            description: None,
            access_token: None,
        }];
        DashboardApp::new(servers, NotificationConfig::default(), None)
    }

    #[tokio::test]
    async fn test_handle_command_empty() {
        let mut app = create_test_app();
        handle_command(&mut app, "").await;
        assert!(app.running);
        assert_eq!(app.status_message, "");
    }

    #[tokio::test]
    async fn test_handle_command_quit() {
        let mut app = create_test_app();
        handle_command(&mut app, "quit").await;
        assert!(!app.running);
    }

    #[tokio::test]
    async fn test_handle_command_q() {
        let mut app = create_test_app();
        handle_command(&mut app, "q").await;
        assert!(!app.running);
    }

    #[tokio::test]
    async fn test_handle_command_help() {
        let mut app = create_test_app();
        handle_command(&mut app, "help").await;
        assert!(app.status_message.contains("Commands"));
    }

    #[tokio::test]
    async fn test_handle_command_h() {
        let mut app = create_test_app();
        handle_command(&mut app, "h").await;
        assert!(app.status_message.contains("Commands"));
    }

    #[tokio::test]
    async fn test_handle_command_unknown() {
        let mut app = create_test_app();
        handle_command(&mut app, "unknown_cmd").await;
        assert!(app.status_message.contains("Unknown command"));
    }

    #[tokio::test]
    async fn test_handle_command_disconnect() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;
        app.connection_status
            .insert(server_id, ConnectionStatus::Connected);
        handle_command(&mut app, "disconnect").await;
        assert!(matches!(
            app.connection_status.get(&server_id),
            Some(ConnectionStatus::Disconnected)
        ));
        assert!(app.status_message.contains("Disconnected"));
    }

    #[tokio::test]
    async fn test_handle_command_refresh_no_client() {
        let mut app = create_test_app();
        handle_command(&mut app, "refresh").await;
        // fetch_server_data returns Ok when no client is connected
        assert_eq!(app.status_message, "");
    }

    #[tokio::test]
    async fn test_handle_command_connect_error() {
        let mut app = create_test_app();
        // The server address is 127.0.0.1:50051 with nothing listening
        handle_command(&mut app, "connect").await;
        assert!(app.status_message.contains("Connection failed"));
    }

    #[tokio::test]
    async fn test_fetch_server_data_no_client() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;
        let result = fetch_server_data(&mut app, server_id).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_connect_to_server_error() {
        let server = ServerEndpoint {
            id: uuid::Uuid::new_v4(),
            name: "bad-server".to_string(),
            address: "127.0.0.1".to_string(),
            port: 1,
            description: None,
            access_token: None,
        };
        let result = connect_to_server(&server, 5, None).await;
        assert!(result.is_err());
    }
}
