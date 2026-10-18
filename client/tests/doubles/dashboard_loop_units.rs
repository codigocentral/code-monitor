//! Actual dashboard event loop against TestBackend and scripted input, no terminal.
use super::*;
use crate::unit_grpc as sdk;
use crossterm::event::{KeyEvent, KeyEventKind};
use std::sync::{Arc, Mutex};
fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}
fn chars(text: &str) -> Vec<KeyCode> {
    text.chars().map(KeyCode::Char).collect()
}
fn fixture() -> (DashboardApp, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let server = ServerEndpoint {
        id: uuid::Uuid::new_v4(),
        name: "Unit".into(),
        address: "fixture.invalid".into(),
        port: 1,
        description: None,
        access_token: None,
    };
    let app = DashboardApp::new(vec![server], NotificationConfig::default(), None)
        .with_config_path(dir.path().join("client.toml"));
    (app, dir)
}
fn memory_terminal() -> Terminal<unit_terminal::CrosstermBackend> {
    unit_terminal::reset(None);
    Terminal::new(unit_terminal::CrosstermBackend::new(unit_terminal::stdout())).unwrap()
}
async fn run(app: &mut DashboardApp, mut keys: Vec<KeyCode>) {
    keys.push(KeyCode::Char('q'));
    unit_events::script(
        keys.into_iter()
            .map(|code| (100, Ok(Some(key(code)))))
            .collect(),
    );
    let mut terminal = memory_terminal();
    app.running = true;
    run_app(&mut terminal, app).await.unwrap();
    assert!(!app.running);
}
async fn connect(app: &mut DashboardApp) -> sdk::SharedState {
    let state = Arc::new(Mutex::new(sdk::State::default()));
    sdk::connect_next(state.clone());
    let id = app.servers[0].id;
    app.connected_clients.insert(
        id,
        connect_to_server(&app.servers[0], 5, None).await.unwrap(),
    );
    app.connection_status
        .insert(id, ConnectionStatus::Connected);
    state
}
#[tokio::test]
async fn navigation_sorting_filters_commands_help_and_details() {
    let (mut app, _dir) = fixture();
    run(
        &mut app,
        vec![
            KeyCode::Char('?'),
            KeyCode::Char('z'),
            KeyCode::Enter,
            KeyCode::Tab,
            KeyCode::BackTab,
            KeyCode::Char('3'),
            KeyCode::Char('o'),
            KeyCode::Char('o'),
            KeyCode::Char('o'),
            KeyCode::Char('O'),
            KeyCode::Char('O'),
            KeyCode::Down,
            KeyCode::Up,
            KeyCode::Home,
            KeyCode::End,
            KeyCode::PageDown,
            KeyCode::PageUp,
            KeyCode::Char('d'),
            KeyCode::Char('/'),
            KeyCode::Char('a'),
            KeyCode::Backspace,
            KeyCode::Char('x'),
            KeyCode::Enter,
        ],
    )
    .await;
    assert_eq!(app.process_filter, "x");
    assert!(app.show_details);
    assert_eq!(app.sort_column, SortColumn::Name);
    run(
        &mut app,
        vec![
            KeyCode::Char('x'),
            KeyCode::Char(':'),
            KeyCode::Char('y'),
            KeyCode::Esc,
        ],
    )
    .await;
    assert!(app.process_filter.is_empty());
    assert!(app.input_buffer.is_empty());
    run(
        &mut app,
        vec![
            KeyCode::Char('1'),
            KeyCode::Char('o'),
            KeyCode::Char('O'),
            KeyCode::Char('/'),
            KeyCode::Char(':'),
            KeyCode::Char('h'),
            KeyCode::Enter,
        ],
    )
    .await;
    assert!(!app.show_help);
    app.current_tab = Tab::Processes;
    app.sort_column = SortColumn::Pid;
    run(&mut app, vec![KeyCode::Char('o')]).await;
    assert_eq!(app.sort_column, SortColumn::Name);
}
#[tokio::test]
async fn settings_bounds_and_toggle_outcomes() {
    let (mut app, _dir) = fixture();
    app.use_ascii_icons = false;
    run(
        &mut app,
        vec![
            KeyCode::Char('s'),
            KeyCode::Up,
            KeyCode::Enter,
            KeyCode::Enter,
            KeyCode::Down,
            KeyCode::Enter,
            KeyCode::Left,
            KeyCode::Right,
            KeyCode::Down,
            KeyCode::Down,
            KeyCode::Enter,
            KeyCode::Enter,
            KeyCode::Up,
            KeyCode::Char('z'),
            KeyCode::Esc,
        ],
    )
    .await;
    assert!(!app.use_ascii_icons);
    assert!(app.auto_connect);
    assert_eq!(app.update_interval, 6);
    app.update_interval = 60;
    app.settings_selection = 1;
    app.input_mode = InputMode::Settings;
    run(
        &mut app,
        vec![KeyCode::Enter, KeyCode::Right, KeyCode::Char('s')],
    )
    .await;
    assert_eq!(app.update_interval, 60);
    app.update_interval = 1;
    app.settings_selection = 1;
    app.input_mode = InputMode::Settings;
    run(&mut app, vec![KeyCode::Left, KeyCode::Esc]).await;
    assert_eq!(app.update_interval, 1);
    app.settings_selection = 99;
    app.input_mode = InputMode::Settings;
    run(&mut app, vec![KeyCode::Enter, KeyCode::Esc]).await;
}
#[tokio::test]
async fn add_server_wizard_persists_config_and_handles_cancellation() {
    let (mut app, _dir) = fixture();
    let mut keys = vec![KeyCode::Char('a')];
    keys.extend(chars("Newx"));
    keys.extend([KeyCode::Backspace, KeyCode::Enter]);
    keys.extend(chars("fixture.invalid"));
    keys.push(KeyCode::Enter);
    keys.push(KeyCode::Enter);
    keys.extend(chars("token"));
    keys.extend([KeyCode::Enter, KeyCode::Char('y'), KeyCode::Enter]);
    run(&mut app, keys).await;
    assert_eq!(app.servers.len(), 2);
    assert_eq!(app.servers[1].name, "New");
    assert_eq!(app.servers[1].port, 50051);
    assert_eq!(app.servers[1].access_token.as_deref(), Some("token"));
    let saved = crate::config::ClientConfigManager::new(&app.config_path)
        .unwrap()
        .load_config()
        .unwrap();
    assert!(saved.servers.iter().any(|s| s.name == "New"));
    app.input_mode = InputMode::AddServer;
    app.add_server_state.step = AddServerStep::Confirm;
    app.input_buffer = "n".into();
    run(&mut app, vec![KeyCode::Enter]).await;
    assert_eq!(app.servers.len(), 2);
    run(
        &mut app,
        vec![KeyCode::Char('a'), KeyCode::Down, KeyCode::Esc],
    )
    .await;
    assert!(app.input_buffer.is_empty());
    app.input_mode = InputMode::AddServer;
    app.add_server_state.step = AddServerStep::Confirm;
    app.add_server_state.name = "NoToken".into();
    app.add_server_state.address = "fixture.invalid".into();
    app.add_server_state.port = "bad".into();
    app.input_buffer = "Y".into();
    run(&mut app, vec![KeyCode::Enter]).await;
    assert_eq!(app.servers[2].port, 50051);
    assert!(app.servers[2].access_token.is_none());
}
#[tokio::test]
async fn connection_confirmation_refresh_and_disconnect_clear_caches() {
    let (mut app, _dir) = fixture();
    let id = app.servers[0].id;
    let state = Arc::new(Mutex::new(sdk::State::default()));
    data::campaign_data_units::queue_poll(&state, true, false);
    sdk::connect_next(state.clone());
    run(&mut app, vec![KeyCode::Enter]).await;
    assert!(app.is_connected(&id));
    assert!(app.system_info_cache.contains_key(&id));
    run(
        &mut app,
        vec![KeyCode::Enter, KeyCode::Char('z'), KeyCode::Char('n')],
    )
    .await;
    assert!(app.is_connected(&id));
    data::campaign_data_units::queue_poll(&state, false, true);
    data::campaign_data_units::queue_poll(&state, false, true);
    run(
        &mut app,
        vec![
            KeyCode::Char('r'),
            KeyCode::Char('R'),
            KeyCode::Enter,
            KeyCode::Char('y'),
        ],
    )
    .await;
    assert!(!app.is_connected(&id));
    assert!(!app.system_info_cache.contains_key(&id));
    sdk::reject_next();
    run(&mut app, vec![KeyCode::Enter]).await;
    assert!(matches!(
        app.connection_status[&id],
        ConnectionStatus::Error(_)
    ));
    let state = Arc::new(Mutex::new(sdk::State::default()));
    data::campaign_data_units::queue_poll(&state, false, true);
    sdk::connect_next(state);
    run(
        &mut app,
        vec![KeyCode::Char('C'), KeyCode::Char('C'), KeyCode::Char('D')],
    )
    .await;
    assert!(app.connected_clients.is_empty());
    assert!(!app.is_connected(&id));
}
#[tokio::test]
async fn token_editor_and_delete_persist_owned_config() {
    let (mut app, _dir) = fixture();
    let mut manager = crate::config::ClientConfigManager::new(&app.config_path).unwrap();
    manager.add_server(app.servers[0].clone()).unwrap();
    run(
        &mut app,
        vec![
            KeyCode::Char('t'),
            KeyCode::Char('a'),
            KeyCode::Char('b'),
            KeyCode::Backspace,
            KeyCode::Up,
            KeyCode::Enter,
        ],
    )
    .await;
    assert_eq!(app.servers[0].access_token.as_deref(), Some("a"));
    assert_eq!(
        manager.load_config().unwrap().servers[0]
            .access_token
            .as_deref(),
        Some("a")
    );
    run(&mut app, vec![KeyCode::Char('t'), KeyCode::Enter]).await;
    assert!(app.servers[0].access_token.is_none());
    run(
        &mut app,
        vec![KeyCode::Char('t'), KeyCode::Char('x'), KeyCode::Esc],
    )
    .await;
    assert!(app.servers[0].access_token.is_none());
    connect(&mut app).await;
    run(
        &mut app,
        vec![
            KeyCode::Delete,
            KeyCode::Delete,
            KeyCode::Char('t'),
            KeyCode::Enter,
            KeyCode::Char('r'),
        ],
    )
    .await;
    assert!(app.servers.is_empty());
    assert!(manager.load_config().unwrap().servers.is_empty());
    assert_eq!(app.server_list_state.selected(), None);
}
#[tokio::test]
async fn event_errors_non_keys_release_repeat_debounce_and_control_exit() {
    let (mut app, _dir) = fixture();
    let mut terminal = memory_terminal();
    for read_error in [false, true] {
        if read_error {
            unit_events::read_error();
            unit_events::script(vec![(0, Ok(Some(key(KeyCode::Char('q')))))]);
        } else {
            unit_events::script(vec![(0, Err(std::io::Error::other("unit poll refused")))]);
        }
        assert!(run_app(&mut terminal, &mut app)
            .await
            .unwrap_err()
            .to_string()
            .contains(if read_error {
                "read refused"
            } else {
                "poll refused"
            }));
    }
    unit_events::script(vec![
        (0, Ok(None)),
        (0, Ok(Some(Event::Resize(180, 60)))),
        (
            0,
            Ok(Some(Event::Key(KeyEvent::new_with_kind(
                KeyCode::Tab,
                KeyModifiers::NONE,
                KeyEventKind::Release,
            )))),
        ),
        (
            0,
            Ok(Some(Event::Key(KeyEvent::new_with_kind(
                KeyCode::Tab,
                KeyModifiers::NONE,
                KeyEventKind::Repeat,
            )))),
        ),
        (0, Ok(Some(key(KeyCode::Tab)))),
        (100, Ok(Some(key(KeyCode::Tab)))),
        (
            100,
            Ok(Some(Event::Key(KeyEvent::new(
                KeyCode::Char('c'),
                KeyModifiers::CONTROL,
            )))),
        ),
    ]);
    run_app(&mut terminal, &mut app).await.unwrap();
    assert_eq!(app.current_tab, Tab::Services);
    assert!(!app.running);
}
#[tokio::test]
async fn periodic_refresh_uses_virtual_elapsed_time() {
    let (mut app, _dir) = fixture();
    let state = connect(&mut app).await;
    data::campaign_data_units::queue_poll(&state, true, false);
    unit_events::script(vec![
        (5000, Ok(None)),
        (0, Ok(Some(key(KeyCode::Char('q'))))),
    ]);
    let mut terminal = memory_terminal();
    run_app(&mut terminal, &mut app).await.unwrap();
    assert_eq!(state.lock().unwrap().calls.len(), 10);
    assert_eq!(app.cpu_history[&app.servers[0].id], vec![95]);
}
#[tokio::test]
async fn dashboard_lifecycle_uses_only_buffer_and_restores_screen() {
    unit_terminal::reset(None);
    unit_events::script(vec![(100, Ok(Some(key(KeyCode::Char('q')))))]);
    run_dashboard(vec![], 5, NotificationConfig::default(), None)
        .await
        .unwrap();
    unit_terminal::STATE.with(|state| {
        let state = state.borrow();
        assert_eq!(state.calls.first(), Some(&"enable"));
        assert_eq!(state.calls.last(), Some(&"show"));
        assert!(state.calls.contains(&"disable"));
        assert!(String::from_utf8_lossy(&state.bytes).contains("\x1b[?1049h"));
        assert!(String::from_utf8_lossy(&state.bytes).contains("\x1b[?1049l"));
    });
}
#[tokio::test]
async fn dashboard_bootstrap_autoconnect_success_and_error_use_transport_double() {
    let (app, _dir) = fixture();
    let server = app.servers[0].clone();
    for success in [false, true] {
        unit_terminal::reset(None);
        if success {
            let state = Arc::new(Mutex::new(sdk::State::default()));
            data::campaign_data_units::queue_poll(&state, true, false);
            sdk::connect_next(state);
        } else {
            sdk::reject_next();
        }
        unit_events::script(vec![(100, Ok(Some(key(KeyCode::Char('q')))))]);
        run_dashboard(vec![server.clone()], 5, NotificationConfig::default(), None)
            .await
            .unwrap();
        unit_terminal::STATE.with(|state| {
            assert_eq!(
                state
                    .borrow()
                    .calls
                    .iter()
                    .filter(|&&c| c == "draw")
                    .count(),
                2
            )
        });
    }
}
#[tokio::test]
async fn dashboard_lifecycle_propagates_each_sdk_error() {
    // Error positions before the event poll do not consume any input.
    for operation in ["enable", "write", "flush", "size", "draw", "draw-flush"] {
        unit_terminal::reset(Some((operation, 1)));
        let error = run_dashboard(vec![], 5, NotificationConfig::default(), None)
            .await
            .unwrap_err();
        assert!(error.to_string().contains(operation));
    }
    for operation in ["disable", "backend-write", "backend-flush", "show"] {
        unit_terminal::reset(Some((operation, 1)));
        unit_events::script(vec![(100, Ok(Some(key(KeyCode::Char('q')))))]);
        assert!(
            run_dashboard(vec![], 5, NotificationConfig::default(), None)
                .await
                .unwrap_err()
                .to_string()
                .contains(operation)
        );
    }
    unit_terminal::reset(None);
    unit_events::script(vec![(
        0,
        Err(std::io::Error::other("unit poll lifecycle error")),
    )]);
    assert!(
        run_dashboard(vec![], 5, NotificationConfig::default(), None)
            .await
            .unwrap_err()
            .to_string()
            .contains("poll lifecycle")
    );
    unit_terminal::STATE.with(|state| assert!(state.borrow().calls.contains(&"disable")));
}
#[tokio::test]
async fn server_navigation_handles_connected_success_error_and_empty_lists() {
    for code in [
        KeyCode::Char('j'),
        KeyCode::Char('k'),
        KeyCode::Left,
        KeyCode::Right,
    ] {
        for success in [false, true] {
            let (mut app, _dir) = fixture();
            let id = app.servers[0].id;
            if success {
                let state = Arc::new(Mutex::new(sdk::State::default()));
                data::campaign_data_units::queue_poll(&state, true, false);
                sdk::connect_next(state);
            } else {
                sdk::reject_next();
            }
            run(&mut app, vec![code]).await;
            assert_eq!(app.is_connected(&id), success);
            if success {
                run(&mut app, vec![code]).await;
                assert!(app.is_connected(&id));
            }
            app.servers.clear();
            run(&mut app, vec![code]).await;
            assert!(app.get_selected_server().is_none());
        }
    }
    let (mut app, _dir) = fixture();
    sdk::reject_next();
    run(&mut app, vec![KeyCode::Char('C')]).await;
    assert!(matches!(
        app.connection_status[&app.servers[0].id],
        ConnectionStatus::Error(_)
    ));
}
#[tokio::test]
async fn alerts_popup_bounds_acknowledgement_and_empty_state() {
    let (mut app, _dir) = fixture();
    for id in ["one", "two", "three"] {
        app.alert_manager.add_rule(AlertRule {
            id: id.into(),
            name: id.into(),
            alert_type: AlertType::DiskHigh,
            severity: AlertSeverity::Warning,
            enabled: true,
            threshold: 1.,
            duration_seconds: 0,
            servers: vec![],
            channels: vec![],
        });
    }
    app.alert_manager
        .process_metrics("unit", "Unit", 0., 0., 99.);
    // Rules share state by type; use separate hosts to create independent alerts.
    app.alert_manager
        .process_metrics("other", "Other", 0., 0., 99.);
    assert!(app.alert_manager.get_active_alerts().len() >= 2);
    app.selected_alert_idx = 99;
    app.show_alerts = true;
    run(
        &mut app,
        vec![
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::Char('z'),
            KeyCode::Char('a'),
            KeyCode::Esc,
        ],
    )
    .await;
    assert!(app.status_message.contains("acknowledged"));
    app.alert_manager = AlertManager::new();
    app.selected_alert_idx = 0;
    run(
        &mut app,
        vec![
            KeyCode::Char('A'),
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::Char('a'),
            KeyCode::Char('A'),
        ],
    )
    .await;
    assert_eq!(app.selected_alert_idx, 0);
    assert!(!app.show_alerts);
    // Valid selection can move down; it cannot move beyond the final alert.
    app.alert_manager.add_rule(AlertRule {
        id: "disk-unit".into(),
        name: "disk-unit".into(),
        alert_type: AlertType::DiskHigh,
        severity: AlertSeverity::Warning,
        enabled: true,
        threshold: 1.,
        duration_seconds: 0,
        servers: vec![],
        channels: vec![],
    });
    for host in ["one", "two"] {
        app.alert_manager.process_metrics(host, host, 0., 0., 99.);
    }
    run(
        &mut app,
        vec![
            KeyCode::Char('A'),
            KeyCode::Down,
            KeyCode::Down,
            KeyCode::Up,
            KeyCode::Enter,
        ],
    )
    .await;
    assert_eq!(app.selected_alert_idx, 0);
}
#[test]
fn terminal_font_indicators_and_platform_defaults() {
    struct Restore(Vec<(&'static str, Option<std::ffi::OsString>)>);
    impl Drop for Restore {
        fn drop(&mut self) {
            for (key, value) in &self.0 {
                if let Some(value) = value {
                    std::env::set_var(key, value);
                } else {
                    std::env::remove_var(key);
                }
            }
        }
    }
    let _restore = Restore(
        ["TERM_PROGRAM", "WT_SESSION"]
            .into_iter()
            .map(|key| (key, std::env::var_os(key)))
            .collect(),
    );
    for key in ["TERM_PROGRAM", "WT_SESSION"] {
        std::env::remove_var(key);
    }
    assert!(!DashboardApp::detect_nerd_font_support_for_platform(true));
    assert!(DashboardApp::detect_nerd_font_support_for_platform(false));
    std::env::set_var("TERM_PROGRAM", "vscode");
    assert!(DashboardApp::detect_nerd_font_support_for_platform(true));
    std::env::set_var("TERM_PROGRAM", "unit-terminal");
    std::env::set_var("WT_SESSION", "unit-session");
    assert!(DashboardApp::detect_nerd_font_support_for_platform(true));
}
#[tokio::test]
async fn startup_policy_disabled_and_initial_draw_failure() {
    let (mut app, _dir) = fixture();
    app.auto_connect = false;
    let mut terminal = memory_terminal();
    let endpoints = app.servers.clone();
    connect_startup_servers(&mut terminal, &mut app, &endpoints, 5)
        .await
        .unwrap();
    assert!(app.connected_clients.is_empty());
    assert!(app.status_message.is_empty());
    unit_terminal::reset(Some(("draw", 1)));
    let error = run_dashboard(endpoints, 5, NotificationConfig::default(), None)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("draw refused"));
}
#[test]
fn previous_server_and_tls_list_length_include_empty_and_populated_cache() {
    let (mut app, _dir) = fixture();
    let mut second = app.servers[0].clone();
    second.id = uuid::Uuid::new_v4();
    app.servers.push(second);
    app.selected_server_idx = 1;
    app.previous_server();
    assert_eq!(app.selected_server_idx, 0);
    app.current_tab = Tab::Tls;
    assert_eq!(app.get_current_list_len(), 0);
    app.tls_cache
        .insert(app.servers[0].id, TlsSnapshot::default());
    assert_eq!(app.get_current_list_len(), 0);
    app.processes_cache.clear();
    assert!(app.get_filtered_processes().is_empty());
}
#[tokio::test]
async fn modes_preserve_empty_selection_and_configuration_errors() {
    let (mut app, dir) = fixture();
    run(&mut app, vec![KeyCode::Char('r'), KeyCode::Char('z')]).await;
    app.servers.clear();
    app.input_mode = InputMode::EditToken;
    run(&mut app, vec![KeyCode::Enter]).await;
    app.input_mode = InputMode::ConfirmDisconnect;
    run(&mut app, vec![KeyCode::Char('y')]).await;
    app.input_mode = InputMode::Command;
    run(
        &mut app,
        vec![KeyCode::Up, KeyCode::Backspace, KeyCode::Esc],
    )
    .await;
    // An empty port is resolved to the documented default.
    app.input_mode = InputMode::AddServer;
    app.add_server_state.step = AddServerStep::Port;
    app.input_buffer.clear();
    run(&mut app, vec![KeyCode::Enter, KeyCode::Esc]).await;
    // Failed config creation must not prevent adding/deleting in-memory entries.
    app.config_path = dir.path().join("absent-parent/unit.toml");
    app.input_mode = InputMode::AddServer;
    app.add_server_state.step = AddServerStep::Confirm;
    app.add_server_state.name = "Unit".into();
    app.add_server_state.address = "fixture.invalid".into();
    app.add_server_state.port = "1".into();
    app.input_buffer = "y".into();
    run(&mut app, vec![KeyCode::Enter]).await;
    assert_eq!(app.servers.len(), 1);
    run(&mut app, vec![KeyCode::Char('t'), KeyCode::Enter]).await;
    assert!(app.servers[0].access_token.is_none());
    run(&mut app, vec![KeyCode::Delete]).await;
    assert!(app.servers.is_empty());
    // Existing malformed content and a valid config without the selected id are preserved.
    app.servers.push(ServerEndpoint {
        id: uuid::Uuid::new_v4(),
        name: "Unit".into(),
        address: "fixture.invalid".into(),
        port: 1,
        description: None,
        access_token: None,
    });
    app.config_path = dir.path().join("invalid.toml");
    std::fs::write(&app.config_path, "invalid = [").unwrap();
    run(&mut app, vec![KeyCode::Char('t'), KeyCode::Enter]).await;
    assert_eq!(
        std::fs::read_to_string(&app.config_path).unwrap(),
        "invalid = ["
    );
    std::fs::remove_file(&app.config_path).unwrap();
    crate::config::ClientConfigManager::new(&app.config_path).unwrap();
    run(&mut app, vec![KeyCode::Char('t'), KeyCode::Enter]).await;
    // Deleting the last of multiple entries clamps selection; deleting the first keeps it.
    let mut second = app.servers[0].clone();
    second.id = uuid::Uuid::new_v4();
    app.servers.push(second);
    app.selected_server_idx = 1;
    run(&mut app, vec![KeyCode::Delete]).await;
    assert_eq!(app.selected_server_idx, 0);
    assert_eq!(app.server_list_state.selected(), Some(0));
}
#[test]
fn no_selected_server_returns_no_processes() {
    let (mut app, _dir) = fixture();
    app.servers.clear();
    assert!(app.get_filtered_processes().is_empty());
}
#[tokio::test]
async fn drawing_failure_is_propagated_by_the_loop() {
    let (mut app, _dir) = fixture();
    unit_terminal::reset(Some(("draw", 1)));
    let mut terminal =
        Terminal::new(unit_terminal::CrosstermBackend::new(unit_terminal::stdout())).unwrap();
    assert!(run_app(&mut terminal, &mut app)
        .await
        .unwrap_err()
        .to_string()
        .contains("draw refused"));
}
