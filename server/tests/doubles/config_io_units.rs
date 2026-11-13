//! Configuration I/O failures through external adapters, owned files only.
use super::*;
use std::cell::Cell;
#[derive(Clone, Copy, PartialEq)]
enum Fault {
    None,
    Serialize,
    Read,
    Write,
}
thread_local! {static FAULT:Cell<Fault>=const{Cell::new(Fault::None)};}
pub(crate) struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        FAULT.with(|f| f.set(Fault::None));
    }
}
pub mod fs {
    pub fn read_to_string(path: impl AsRef<std::path::Path>) -> std::io::Result<String> {
        if super::FAULT.with(|f| f.get() == super::Fault::Read) {
            Err(std::io::Error::other("unit read refused"))
        } else {
            std::fs::read_to_string(path)
        }
    }
    pub fn write(
        path: impl AsRef<std::path::Path>,
        content: impl AsRef<[u8]>,
    ) -> std::io::Result<()> {
        if super::FAULT.with(|f| f.get() == super::Fault::Write) {
            Err(std::io::Error::other("unit write refused"))
        } else {
            let absolute = if path.as_ref().is_absolute() {
                path.as_ref().to_path_buf()
            } else {
                std::env::current_dir()?.join(path.as_ref())
            };
            assert!(
                absolute.starts_with(std::env::var_os("TMPDIR").unwrap()),
                "config units only write owned fixtures"
            );
            std::fs::write(path, content)
        }
    }
}
pub mod toml {
    pub use ::toml::from_str;
    pub fn to_string_pretty<T: serde::Serialize>(value: &T) -> Result<String, ::toml::ser::Error> {
        if super::FAULT.with(|f| f.get() == super::Fault::Serialize) {
            Err(serde::ser::Error::custom("unit serializer refused"))
        } else {
            ::toml::to_string_pretty(value)
        }
    }
}
#[test]
fn readers_and_writers_propagate_every_provider_failure() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("unit.toml");
    let config = Config::load(&path).unwrap();
    let _guard = Guard;
    for (fault, expected) in [(Fault::Serialize, "serializer"), (Fault::Write, "write")] {
        FAULT.with(|f| f.set(fault));
        assert!(config
            .save(&path)
            .unwrap_err()
            .to_string()
            .contains(expected));
    }
    FAULT.with(|f| f.set(Fault::Read));
    assert!(Config::load(&path)
        .unwrap_err()
        .to_string()
        .contains("read refused"));
    FAULT.with(|f| f.set(Fault::Write));
    assert!(Config::load(root.path().join("new.toml"))
        .unwrap_err()
        .to_string()
        .contains("write refused"));
    FAULT.with(|f| f.set(Fault::None));
    Config::default().save(&path).unwrap();
    FAULT.with(|f| f.set(Fault::Write));
    assert!(Config::load(&path)
        .unwrap_err()
        .to_string()
        .contains("write refused"));
}
#[test]
fn invalid_env_numbers_and_empty_token_keep_prior_values() {
    let keys = [
        "CODE_MONITOR_UPDATE_INTERVAL",
        "CODE_MONITOR_MAX_CLIENTS",
        "CODE_MONITOR_ACCESS_TOKEN",
    ];
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
        keys.into_iter()
            .map(|key| (key, std::env::var_os(key)))
            .collect(),
    );
    std::env::set_var(keys[0], "bad");
    std::env::set_var(keys[1], "bad");
    std::env::set_var(keys[2], "");
    let mut config = Config {
        access_token: "keep".into(),
        ..Default::default()
    };
    config.apply_env_overrides();
    assert_eq!(config.update_interval_seconds, 5);
    assert_eq!(config.max_clients, 100);
    assert_eq!(config.access_token, "keep");
}

#[test]
fn load_covers_both_existing_token_paths_and_parse_validation_errors() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("all-load-paths.toml");
    let initial = Config::load(&path).unwrap();
    assert!(!initial.access_token.is_empty());
    assert_eq!(
        Config::load(&path).unwrap().access_token,
        initial.access_token
    );
    Config::default().save(&path).unwrap();
    assert!(!Config::load(&path).unwrap().access_token.is_empty());
    let invalid = Config {
        update_interval_seconds: 0,
        access_token: "unit-token".into(),
        ..Default::default()
    };
    invalid.save(&path).unwrap();
    assert!(Config::load(&path)
        .unwrap_err()
        .to_string()
        .contains("Invalid configuration in"));
    std::fs::write(&path, "invalid = [").unwrap();
    assert!(Config::load(&path).is_err());
}

pub(crate) fn refuse_write() -> Guard {
    FAULT.with(|f| f.set(Fault::Write));
    Guard
}

#[test]
fn missing_config_write_failure_uses_the_same_path_type_as_success() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("missing.toml");
    let _fault = refuse_write();
    assert!(Config::load(&path).is_err());
}
