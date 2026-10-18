use super::*;
use crate::{
    client::MonitorClient,
    dashboard::{
        data::{campaign_data_units::queue_poll, fetch_server_data},
        ConnectionStatus, SortColumn, SortOrder,
    },
    unit_grpc::{self as sdk, State},
};
use shared::{notifications::NotificationConfig, types::*};
use std::sync::{Arc, Mutex};
fn render(app: &DashboardApp) -> String {
    let mut terminal = tui::Terminal::new(tui::backend::TestBackend::new(240, 75)).unwrap();
    terminal.draw(|frame| draw_ui(frame, app)).unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol.as_str())
        .collect()
}
async fn fixture() -> (DashboardApp, uuid::Uuid) {
    let server = ServerEndpoint {
        id: uuid::Uuid::new_v4(),
        name: "Unit".into(),
        address: "fixture.invalid".into(),
        port: 1,
        description: Some("Unit description".into()),
        access_token: Some("unit-token".into()),
    };
    let id = server.id;
    let mut app = DashboardApp::new(vec![server], NotificationConfig::default(), None);
    let state = Arc::new(Mutex::new(State::default()));
    queue_poll(&state, true, false);
    sdk::connect_next(state);
    let client = MonitorClient::connect("http://fixture.invalid:1", 5, false)
        .await
        .unwrap();
    app.connected_clients.insert(id, client);
    app.connection_status
        .insert(id, ConnectionStatus::Connected);
    fetch_server_data(&mut app, id).await;
    (app, id)
}
#[tokio::test]
async fn full_dashboard_renders_real_cached_processes_databases_and_all_tabs() {
    let (mut app, id) = fixture().await;
    let base = app.processes_cache[&id][0].clone();
    app.processes_cache.insert(
        id,
        [(80., Some(10)), (30., Some(0)), (5., None)]
            .into_iter()
            .enumerate()
            .map(|(i, (cpu, swap))| ProcessInfo {
                pid: 10 + i as u32,
                name: format!("unit-process-{i}"),
                cpu_usage_percent: cpu,
                swap_bytes: swap,
                user: "unit-user".into(),
                command_line: "unit command".into(),
                ..base.clone()
            })
            .collect(),
    );
    let base = app.postgres_cache[&id][0].clone();
    let settings = vec![
        PostgresSetting {
            name: "work_mem".into(),
            value: "8".into(),
            unit: Some("MB".into()),
            source: "configuration file".into(),
            source_file: Some("postgresql.auto.conf".into()),
            source_line: Some(7),
        },
        PostgresSetting {
            name: "shared_buffers".into(),
            value: "128".into(),
            unit: None,
            source: "default".into(),
            source_file: None,
            source_line: None,
        },
        PostgresSetting {
            name: "max_connections".into(),
            value: "100".into(),
            unit: None,
            source: "command line".into(),
            source_file: None,
            source_line: None,
        },
    ];
    app.postgres_cache.insert(
        id,
        [90, 50, 2]
            .into_iter()
            .enumerate()
            .map(|(i, connections)| PostgresClusterInfo {
                name: format!("pg-unit-{i}"),
                host: "db.invalid".into(),
                port: 5432,
                connections_total: connections,
                cache_hit_ratio: if i == 0 { 89. } else { 99. },
                databases: if i == 2 {
                    vec![]
                } else {
                    vec![PostgresDatabaseInfo {
                        name: "unit-db".into(),
                        size_bytes: 1024,
                        num_backends: i as u32,
                        cache_hit_ratio: 99.,
                        transactions: 2,
                        stats_reset_at: Some(crate::unit_sqlite::now()),
                    }]
                },
                top_queries: if i == 2 {
                    vec![]
                } else {
                    vec![TopQuery {
                        query: "select a deliberately long unit query for truncation behavior"
                            .into(),
                        calls: 8,
                        total_exec_time_ms: 12.,
                        mean_exec_time_ms: 1.5,
                    }]
                },
                settings: if i == 0 { settings.clone() } else { vec![] },
                ..base.clone()
            })
            .collect(),
    );
    let base = app.mariadb_cache[&id][0].clone();
    app.mariadb_cache.insert(id,[90,50,2].into_iter().enumerate().map(|(i,connections)|MariaDBClusterInfo{name:format!("maria-unit-{i}"),host:"db.invalid".into(),port:3306,connections_active:connections,schemas:if i==2 {vec![]}else{vec![MariaDBSchemaInfo{name:"unit-schema".into(),size_bytes:1024,table_count:1,last_write_at:Some(crate::unit_sqlite::now()),write_time_available:true}]},processes:if i==2{vec![]}else{vec![MariaDBProcessInfo{id:8,user:"unit-user".into(),host:"client.invalid".into(),db:Some("unit-schema".into()),command:if i==0{"a long database command that exceeds thirty characters".into()}else{"Query".into()},time_seconds:3,state:"executing".into(),info:if i==0{Some("a long SQL statement that exceeds forty characters in this fixture".into())}else{None}}]},..base.clone()}).collect());
    for tab in Tab::ALL {
        app.current_tab = *tab;
        let output = render(&app);
        assert!(
            output.contains(tab.title().split_whitespace().last().unwrap()),
            "{tab:?}"
        );
        assert!(output.contains("Unit"));
    }
    app.current_tab = Tab::Processes;
    app.show_details = true;
    for column in [
        SortColumn::Name,
        SortColumn::Status,
        SortColumn::Pid,
        SortColumn::Cpu,
        SortColumn::Memory,
        SortColumn::Uptime,
    ] {
        for order in [SortOrder::Ascending, SortOrder::Descending] {
            app.sort_column = column;
            app.sort_order = order;
            let output = render(&app);
            assert!(output.contains("unit-process-"));
            assert!(output.contains("unit command"));
        }
    }
    app.process_filter = "nothing-matches".into();
    assert!(render(&app).contains("No processes match the filter"));
    app.process_filter.clear();
    for selected in 0..3 {
        app.selected_item_idx = selected;
        for tab in [Tab::Postgres, Tab::MariaDB] {
            app.current_tab = tab;
            let output = render(&app);
            assert!(output.contains(if tab == Tab::Postgres {
                "Postgres Clusters"
            } else {
                "MariaDB Clusters"
            }));
        }
    }
    app.current_tab = Tab::Postgres;
    app.selected_item_idx = 0;
    app.postgres_cache.get_mut(&id).unwrap()[0].settings = vec![PostgresSetting {
        name: "unit-default".into(),
        value: "8".into(),
        unit: None,
        source: "default".into(),
        source_file: None,
        source_line: None,
    }];
    assert!(render(&app).contains("unit-default"));
    app.current_tab = Tab::Processes;
    app.processes_cache.remove(&id);
    assert!(render(&app).contains("Not connected"));
    let mut terminal = tui::Terminal::new(tui::backend::TestBackend::new(90, 30)).unwrap();
    terminal
        .draw(|frame| {
            let area = frame.size();
            processes::draw_process_details(frame, &app, area)
        })
        .unwrap();
    let output: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol.as_str())
        .collect();
    assert!(output.contains("Select a process"));
}
#[tokio::test]
async fn dashboard_disconnected_empty_and_input_modes_keep_visible_guidance() {
    let (mut app, id) = fixture().await;
    for status in [
        ConnectionStatus::Disconnected,
        ConnectionStatus::Connecting,
        ConnectionStatus::Error("unit failure".into()),
        ConnectionStatus::Connected,
    ] {
        app.connection_status.insert(id, status);
        app.current_tab = Tab::Overview;
        assert!(render(&app).contains("Unit"));
    }
    app.status_message = "unit status message".into();
    assert!(render(&app).contains("unit status message"));
    for (mode, text) in [
        (InputMode::Command, "Enter command"),
        (InputMode::AddServer, "Enter value"),
        (InputMode::EditToken, "Enter token"),
        (InputMode::ConfirmDisconnect, "Press Y"),
        (InputMode::Settings, "Navigate"),
    ] {
        app.input_mode = mode;
        assert!(render(&app).contains(text));
    }
    app.input_mode = InputMode::Normal;
    app.show_help = true;
    assert!(render(&app).contains("Help"));
    app.show_help = false;
    app.show_alerts = true;
    assert!(render(&app).contains("Alert"));
    app.show_alerts = false;
    app.postgres_cache.insert(id, vec![]);
    app.current_tab = Tab::Postgres;
    assert!(render(&app).contains("No Postgres clusters"));
    app.postgres_cache.remove(&id);
    assert!(render(&app).contains("Not connected"));
    app.mariadb_cache.insert(id, vec![]);
    app.current_tab = Tab::MariaDB;
    assert!(render(&app).contains("No MariaDB clusters"));
    app.mariadb_cache.remove(&id);
    assert!(render(&app).contains("Not connected"));
    app.servers.clear();
    for tab in Tab::ALL {
        app.current_tab = *tab;
        let output = render(&app);
        assert!(output.contains(tab.title().split_whitespace().last().unwrap()));
    }
}

#[tokio::test]
async fn severity_styles_image_versions_and_database_origins_reach_the_buffer() {
    let (mut app, id) = fixture().await;
    app.current_tab = Tab::Overview;
    {
        let info = app.system_info_cache.get_mut(&id).unwrap();
        info.memory_used_bytes = 65;
        info.memory_pressure.as_mut().unwrap().full_avg60 = 1.;
        info.disk_info[0].usage_percent = 80.;
    }
    let output = render(&app);
    assert!(output.contains("65.0%"));
    assert!(output.contains("full 1.00%"));
    let base = app.containers_cache[&id][0].clone();
    app.containers_cache.insert(
        id,
        [
            VersionStatus::Drifted,
            VersionStatus::UpToDate,
            VersionStatus::LocalBuild,
            VersionStatus::Unknown,
        ]
        .into_iter()
        .enumerate()
        .map(|(i, status)| ContainerInfo {
            name: format!("unit-version-{i}"),
            image: "unit/image:tag".into(),
            memory_limit_set: true,
            memory_limit_bytes: 100,
            memory_usage_bytes: 80,
            swap_bytes: Some(30),
            restart_count: 0,
            health: "healthy".into(),
            health_detail: None,
            image_version: Some(ImageVersionInfo {
                image_ref: "unit/image:tag".into(),
                registry: "unit.invalid".into(),
                repository: "unit/image".into(),
                tag: "tag".into(),
                local_digest: None,
                remote_digest: None,
                status,
                checked_at: None,
                error: None,
            }),
            ..base.clone()
        })
        .collect(),
    );
    app.current_tab = Tab::Containers;
    let output = render(&app);
    assert!(output.contains("unit-version-0"));
    assert!(output.contains("(local)"));
    assert!(output.contains(" ↑"));
    let mut healthy = base;
    healthy.name = "unit-healthy".into();
    healthy.restart_count = 0;
    healthy.memory_limit_set = true;
    healthy.memory_limit_bytes = 100;
    healthy.memory_usage_bytes = 10;
    healthy.swap_bytes = Some(0);
    healthy.health = "healthy".into();
    healthy.health_detail = None;
    healthy.image_version = None;
    app.containers_cache.insert(id, vec![healthy]);
    assert!(render(&app).contains("unit-healthy"));
    let cluster = &mut app.postgres_cache.get_mut(&id).unwrap()[0];
    cluster.settings = vec![
        PostgresSetting {
            name: "unit-first".into(),
            value: "1".into(),
            unit: None,
            source: "configuration file".into(),
            source_file: Some("postgresql.conf".into()),
            source_line: Some(1),
        },
        PostgresSetting {
            name: "unit-override".into(),
            value: "2".into(),
            unit: Some("MB".into()),
            source: "configuration file".into(),
            source_file: Some("postgresql.auto.conf".into()),
            source_line: Some(2),
        },
    ];
    app.current_tab = Tab::Postgres;
    let output = render(&app);
    assert!(output.contains("mixed sources"));
    assert!(output.contains("unit-override"));
    let cluster = &mut app.mariadb_cache.get_mut(&id).unwrap()[0];
    cluster.processes = vec![MariaDBProcessInfo {
        id: 1,
        user: "unit".into(),
        host: "fixture.invalid".into(),
        db: None,
        command: "Query".into(),
        time_seconds: 1,
        state: "executing".into(),
        info: Some("unit short SQL".into()),
    }];
    app.current_tab = Tab::MariaDB;
    assert!(render(&app).contains("unit short SQL"));
    app.connected_clients.clear();
    app.current_tab = Tab::Overview;
    assert!(render(&app).contains("Unit"));
}

#[tokio::test]
async fn swap_without_kernel_pressure_keeps_the_swap_row_visible() {
    let (mut app, id) = fixture().await;
    let info = app.system_info_cache.get_mut(&id).unwrap();
    info.swap.total_bytes = 1024;
    info.memory_pressure = None;
    app.current_tab = Tab::Overview;
    let output = render(&app);
    assert!(output.contains("Swap"));
    assert!(!output.contains("PSI some"));
}
