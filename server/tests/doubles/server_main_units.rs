//! Actual server CLI/bootstrap with in-memory scheduler, transport and signals.
use super::*;
use crate::{unit_console as console, unit_server_bootstrap as sdk};
use std::sync::{Arc, Mutex};
fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("server.toml");
    let config = Config {
        access_token: "unit-token".into(),
        ..Default::default()
    };
    config.save(&path).unwrap();
    (root, path)
}
fn args(path: &std::path::Path) -> Args {
    Args::try_parse_from([
        "monitor-server",
        "--config",
        path.to_str().unwrap(),
        "--address",
        "127.0.0.1",
        "--port",
        "50051",
        "--no-health",
    ])
    .unwrap()
}
#[test]
fn main_log_levels_auth_tls_transport_settings_and_shutdown() {
    let (_guard, _logs) = crate::unit_logs::capture();
    let (root, path) = fixture();
    for (level, expected) in [
        ("trace", tracing::Level::TRACE),
        ("debug", tracing::Level::DEBUG),
        ("info", tracing::Level::INFO),
        ("warn", tracing::Level::WARN),
        ("error", tracing::Level::ERROR),
        ("other", tracing::Level::INFO),
    ] {
        sdk::reset();
        let mut args = args(&path);
        args.log_level = level.into();
        let state = Arc::new(Mutex::new(crate::unit_service_monitor::State::default()));
        crate::unit_service_monitor::next(state.clone());
        sdk::next(args);
        main().unwrap();
        assert_eq!(state.lock().unwrap().starts, 1);
        assert_eq!(state.lock().unwrap().tls_dirs, [Vec::<String>::new()]);
        sdk::STATE.with(|s| {
            let s = s.borrow();
            assert_eq!(s.addresses, ["127.0.0.1:50051".parse().unwrap()]);
            assert!(s.calls.iter().any(|c| c == "add_service"));
        });
        sdk::logging::SETTINGS.with(|s| assert_eq!(s.borrow().last(), Some(&(expected, false))));
    }
    let cert = root.path().join("cert.pem");
    let key = root.path().join("key.pem");
    std::fs::write(&cert, b"unit-cert").unwrap();
    std::fs::write(&key, b"unit-key").unwrap();
    for mtls in [false, true] {
        let config = Config {
            access_token: "unit-token".into(),
            enable_authentication: false,
            tls: Some(config::TlsConfig {
                cert_path: cert.to_str().unwrap().into(),
                key_path: key.to_str().unwrap().into(),
                ca_path: if mtls {
                    Some(cert.to_str().unwrap().into())
                } else {
                    None
                },
            }),
            ..Default::default()
        };
        config.save(&path).unwrap();
        sdk::reset();
        let mut args = args(&path);
        args.log_json = true;
        sdk::next(args);
        main().unwrap();
        sdk::STATE.with(|s| assert_eq!(s.borrow().tls, 1));
        sdk::logging::SETTINGS.with(|s| assert!(s.borrow().last().unwrap().1));
    }
    std::fs::write(&path, "invalid = [").unwrap();
    sdk::reset();
    sdk::next(args(&path));
    main().unwrap();
}
#[test]
fn bootstrap_propagates_factory_tls_and_serving_errors() {
    let (root, path) = fixture();
    for stage in 0..4 {
        sdk::reset();
        if stage == 0 {
            crate::unit_service_monitor::next_error();
        } else if stage == 1 {
            sdk::STATE.with(|s| s.borrow_mut().factory_error = true);
        } else if stage == 2 {
            sdk::STATE.with(|s| s.borrow_mut().serve_error = true);
        } else {
            let cert = root.path().join("cert.pem");
            let key = root.path().join("key.pem");
            std::fs::write(&cert, b"unit").unwrap();
            std::fs::write(&key, b"unit").unwrap();
            Config {
                access_token: "unit-token".into(),
                tls: Some(config::TlsConfig {
                    cert_path: cert.to_str().unwrap().into(),
                    key_path: key.to_str().unwrap().into(),
                    ca_path: None,
                }),
                ..Default::default()
            }
            .save(&path)
            .unwrap();
            sdk::STATE.with(|s| s.borrow_mut().tls_error = true);
        }
        sdk::next(args(&path));
        assert!(main().unwrap_err().to_string().contains("unit"));
    }
    sdk::reset();
    let mut input = args(&path);
    input.address = "invalid-address".into();
    sdk::next(input);
    assert!(std::panic::catch_unwind(main).is_err());
}
#[tokio::test]
async fn graceful_shutdown_uses_scripted_ctrl_c_and_sigterm_failures() {
    use std::{
        future::Future,
        task::{Context, Poll},
    };
    let (_guard, logs) = crate::unit_logs::capture();
    for (ctrl_ready, ctrl_error, term_ready, term_error) in [
        (true, false, false, false),
        (false, true, false, false),
        (false, false, true, false),
        (false, false, false, true),
    ] {
        sdk::signal::reset();
        sdk::signal::STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.ctrl_ready = ctrl_ready;
            s.ctrl_error = ctrl_error;
            s.term_ready = term_ready;
            s.term_error = term_error;
        });
        let mut future = std::pin::pin!(shutdown_signal());
        let result = future
            .as_mut()
            .poll(&mut Context::from_waker(std::task::Waker::noop()));
        assert_eq!(matches!(result, Poll::Ready(())), ctrl_ready || term_ready);
    }
    let logs = logs.lock().unwrap();
    assert!(logs.iter().any(|l| l.contains("Ctrl+C handler")));
    assert!(logs.iter().any(|l| l.contains("SIGTERM handler")));
}
#[test]
fn prompts_validate_defaults_aliases_and_io_errors() {
    console::lines(&["", "text"]);
    assert_eq!(prompt_string("Name", "default").unwrap(), "default");
    assert_eq!(prompt_string("Name", "").unwrap(), "text");
    for (values, default, expected) in [
        (&["invalid", "yes"][..], false, true),
        (&["no"][..], true, false),
        (&[""][..], false, false),
        (&[""][..], true, true),
    ] {
        console::lines(values);
        assert_eq!(prompt_bool("Enable", default).unwrap(), expected);
    }
    console::lines(&["invalid", "7"]);
    assert_eq!(prompt_u64("Interval", 5).unwrap(), 7);
    console::lines(&["bad", "8"]);
    assert_eq!(prompt_usize("Count", 1).unwrap(), 8);
    console::script(vec![], Some(1));
    assert!(prompt_string("Name", "").is_err());
    console::script(vec![Err(std::io::Error::other("unit input refused"))], None);
    assert!(prompt_string("Name", "").is_err());
    assert_eq!(blank_to_none(" ".into()), None);
    assert_eq!(blank_to_none(" unit ".into()), Some("unit".into()));
}
#[test]
fn setup_commands_authorization_and_backup_mutate_only_owned_configuration() {
    let (root, path) = fixture();
    let path = path.to_str().unwrap();
    console::lines(&["no"]);
    init_config(path, false).unwrap();
    console::lines(&["5", "7", "no", "no", "unit.service, other.service"]);
    handle_command(&Commands::Init { force: true }, path).unwrap();
    let config = Config::load(path).unwrap();
    assert!(!config.enable_authentication);
    assert_eq!(config.systemd_units, ["unit.service", "other.service"]);
    handle_command(
        &Commands::AddClient {
            name: "Unit".into(),
            public_key: "unit-public-key".into(),
        },
        path,
    )
    .unwrap();
    handle_command(&Commands::ListClients, path).unwrap();
    assert_eq!(
        Config::load(path).unwrap().authorized_clients[0].name,
        "Unit"
    );
    let backup = root.path().join("backup.toml");
    handle_command(
        &Commands::ExportConfig {
            output: backup.to_str().unwrap().into(),
        },
        path,
    )
    .unwrap();
    handle_command(
        &Commands::ImportConfig {
            input: backup.to_str().unwrap().into(),
        },
        path,
    )
    .unwrap();
    assert_eq!(Config::load(path).unwrap().max_clients, 7);
    sdk::reset();
    let mut input = args(std::path::Path::new(path));
    input.command = Some(Commands::ShowToken);
    sdk::next(input);
    main().unwrap();
}
#[test]
fn health_task_runs_in_memory_and_reports_transport_failure() {
    let (_guard, logs) = crate::unit_logs::capture();
    let (_root, path) = fixture();
    for bind_error in [false, true] {
        *crate::health::unit_http::STATE.lock().unwrap() = crate::health::unit_http::State {
            bind_error,
            ..Default::default()
        };
        sdk::reset();
        let mut input = args(&path);
        input.no_health = false;
        sdk::next(input);
        main().unwrap();
        let state = crate::health::unit_http::STATE.lock().unwrap();
        assert_eq!(state.addresses.len(), 1);
        assert_eq!(state.served, usize::from(!bind_error));
    }
    assert!(logs
        .lock()
        .unwrap()
        .iter()
        .any(|l| l.contains("Health check server error")));
}
#[test]
fn json_environment_values_and_missing_client_ca_are_observable() {
    struct Restore(Option<std::ffi::OsString>);
    impl Drop for Restore {
        fn drop(&mut self) {
            if let Some(value) = &self.0 {
                std::env::set_var("CODE_MONITOR_LOG_JSON", value);
            } else {
                std::env::remove_var("CODE_MONITOR_LOG_JSON");
            }
        }
    }
    let _restore = Restore(std::env::var_os("CODE_MONITOR_LOG_JSON"));
    let (root, path) = fixture();
    for (value, expected) in [("1", true), ("TrUe", true), ("no", false)] {
        std::env::set_var("CODE_MONITOR_LOG_JSON", value);
        sdk::reset();
        sdk::next(args(&path));
        main().unwrap();
        sdk::logging::SETTINGS.with(|s| assert_eq!(s.borrow().last().unwrap().1, expected));
    }
    let cert = root.path().join("cert.pem");
    let key = root.path().join("key.pem");
    std::fs::write(&cert, b"unit-cert").unwrap();
    std::fs::write(&key, b"unit-key").unwrap();
    let config = Config {
        access_token: "unit-token".into(),
        tls: Some(config::TlsConfig {
            cert_path: cert.to_str().unwrap().into(),
            key_path: key.to_str().unwrap().into(),
            ca_path: Some(root.path().join("missing-ca").to_str().unwrap().into()),
        }),
        ..Default::default()
    };
    config.save(&path).unwrap();
    sdk::reset();
    sdk::next(args(&path));
    assert!(main().unwrap_err().to_string().contains("CA"));
}
#[test]
fn setup_generates_token_and_updates_tls_defaults_without_reading_services() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("nested/server.toml");
    console::lines(&["5", "7", "yes", "no", ""]);
    init_config(path.to_str().unwrap(), true).unwrap();
    let config = Config::load(&path).unwrap();
    assert!(!config.access_token.is_empty());
    assert!(config.enable_authentication);
    assert!(config.tls.is_none());
    console::lines(&[
        "5",
        "7",
        "yes",
        "no",
        "yes",
        "unit-cert.pem",
        "unit-key.pem",
        "",
        "unit.service",
    ]);
    init_config(path.to_str().unwrap(), true).unwrap();
    let config = Config::load(&path).unwrap();
    assert_eq!(config.tls.as_ref().unwrap().cert_path, "unit-cert.pem");
    assert!(config.tls.as_ref().unwrap().ca_path.is_none());
    console::lines(&["", "", "yes", "yes", "yes", "", "", "unit-ca.pem", ""]);
    init_config(path.to_str().unwrap(), true).unwrap();
    let config = Config::load(&path).unwrap();
    assert_eq!(config.tls.unwrap().ca_path.as_deref(), Some("unit-ca.pem"));
    assert_eq!(config.systemd_units, ["unit.service"]);
    console::lines(&["yes", "", "", "no", "no", ""]);
    init_config(path.to_str().unwrap(), false).unwrap();
    assert!(!Config::load(&path).unwrap().enable_authentication);
}
#[test]
fn every_setup_prompt_and_save_error_is_propagated() {
    for stage in 1..=6 {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("new.toml");
        console::script(
            ["5", "7", "yes", "no", ""]
                .into_iter()
                .map(|s| Ok(s.into()))
                .collect(),
            Some(stage),
        );
        assert!(
            init_config(path.to_str().unwrap(), true).is_err(),
            "setup stage {stage}"
        );
    }
    for kind in 0..3 {
        console::script(vec![Err(std::io::Error::other("unit prompt failed"))], None);
        let result = match kind {
            0 => prompt_bool("Enabled", true).map(|_| ()),
            1 => prompt_u64("Count", 5).map(|_| ()),
            _ => prompt_usize("Count", 5).map(|_| ()),
        };
        assert!(result.is_err());
    }
    let (root, path) = fixture();
    std::fs::write(&path, "invalid = [").unwrap();
    assert!(init_config(path.to_str().unwrap(), true).is_err());
    let output = root.path().join("new.toml");
    console::lines(&["5", "7", "yes", "no", ""]);
    {
        let _fault = crate::config::campaign_io_units::refuse_write();
        assert!(init_config(output.to_str().unwrap(), true).is_err());
    }
    let parent = root.path().join("file-parent");
    std::fs::write(&parent, "owned file").unwrap();
    let output = parent.join("new.toml");
    console::lines(&["5", "7", "yes", "no", ""]);
    assert!(init_config(output.to_str().unwrap(), true).is_err());
}
#[test]
fn optional_setup_prompts_propagate_io_failures_and_relative_owned_path_is_supported() {
    let (_root, path) = fixture();
    console::script(
        vec![Err(std::io::Error::other("unit confirmation refused"))],
        None,
    );
    assert!(init_config(path.to_str().unwrap(), false).is_err());
    for stage in [4, 6, 7, 8] {
        console::script(
            [
                "5",
                "7",
                "yes",
                "no",
                "yes",
                "unit-cert.pem",
                "unit-key.pem",
                "",
            ]
            .into_iter()
            .map(|s| Ok(s.into()))
            .collect(),
            Some(stage),
        );
        assert!(
            init_config(path.to_str().unwrap(), true).is_err(),
            "optional prompt {stage}"
        );
    }
    let root = tempfile::tempdir().unwrap();
    struct Restore(std::path::PathBuf);
    impl Drop for Restore {
        fn drop(&mut self) {
            std::env::set_current_dir(&self.0).unwrap();
        }
    }
    let _restore = Restore(std::env::current_dir().unwrap());
    std::env::set_current_dir(root.path()).unwrap();
    console::lines(&["5", "7", "yes", "no", ""]);
    init_config("relative.toml", true).unwrap();
    assert!(root.path().join("relative.toml").is_file());
}
#[test]
fn stateful_command_load_save_and_import_errors_propagate() {
    let (root, path) = fixture();
    let path = path.to_str().unwrap();
    let backup = root.path().join("backup.toml");
    Config::load(path).unwrap().save(&backup).unwrap();
    for cmd in [
        Commands::NewToken,
        Commands::RemoveClient {
            name: "Unit".into(),
        },
        Commands::DisableAuth,
        Commands::EnableAuth,
        Commands::AddClient {
            name: "Unit".into(),
            public_key: "unit".into(),
        },
        Commands::ExportConfig {
            output: backup.to_str().unwrap().into(),
        },
        Commands::ImportConfig {
            input: backup.to_str().unwrap().into(),
        },
    ] {
        let mut config = Config::load(path).unwrap();
        config.add_authorized_client("Unit".into(), "unit-key".into());
        config.save(path).unwrap();
        let _fault = crate::config::campaign_io_units::refuse_write();
        assert!(handle_command(&cmd, path).is_err());
    }
    let missing = root.path().join("missing/backup.toml");
    assert!(handle_command(
        &Commands::ImportConfig {
            input: missing.to_str().unwrap().into()
        },
        path
    )
    .is_err());
    std::fs::write(path, "invalid = [").unwrap();
    for cmd in [
        Commands::ShowToken,
        Commands::NewToken,
        Commands::ListClients,
        Commands::RemoveClient {
            name: "Unit".into(),
        },
        Commands::DisableAuth,
        Commands::EnableAuth,
        Commands::AddClient {
            name: "Unit".into(),
            public_key: "unit".into(),
        },
        Commands::ExportConfig {
            output: backup.to_str().unwrap().into(),
        },
        Commands::ImportConfig {
            input: backup.to_str().unwrap().into(),
        },
    ] {
        assert!(handle_command(&cmd, path).is_err());
    }
}
