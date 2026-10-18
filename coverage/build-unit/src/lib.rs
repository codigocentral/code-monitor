#[cfg(test)]
mod units {
    // Compile and execute the actual tracked build script, without a copy of
    // its implementation. The only child program it calls is a command double.
    include!("../../../shared/build.rs");
    use std::{env, ffi::OsString, path::PathBuf};

    struct Environment {
        cwd: PathBuf,
        values: Vec<(&'static str, Option<OsString>)>,
        scratch: tempfile::TempDir,
        out: PathBuf,
        log: PathBuf,
    }
    impl Environment {
        fn new(fails: bool) -> Self {
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .to_path_buf();
            let scratch = tempfile::tempdir().unwrap();
            let out = scratch.path().join("generated");
            std::fs::create_dir(&out).unwrap();
            let log = scratch.path().join("protoc-args.txt");
            let keys = [
                "OUT_DIR",
                "PROTOC",
                "UNIT_DESCRIPTOR_FIXTURE",
                "UNIT_PROTOC_ARGS",
                "UNIT_PROTOC_FAIL",
            ];
            let values = keys
                .into_iter()
                .map(|key| (key, env::var_os(key)))
                .collect();
            let cwd = env::current_dir().unwrap();
            env::set_current_dir(root.join("shared")).unwrap();
            env::set_var("OUT_DIR", &out);
            env::set_var(
                "PROTOC",
                env::var_os("UNIT_PROTOC_DOUBLE").expect("build command double configured"),
            );
            env::set_var(
                "UNIT_DESCRIPTOR_FIXTURE",
                root.join("coverage/build-unit/fixtures/monitoring.bin"),
            );
            env::set_var("UNIT_PROTOC_ARGS", &log);
            env::set_var("UNIT_PROTOC_FAIL", if fails { "1" } else { "0" });
            Self {
                cwd,
                values,
                scratch,
                out,
                log,
            }
        }
    }
    impl Drop for Environment {
        fn drop(&mut self) {
            env::set_current_dir(&self.cwd).unwrap();
            for (key, value) in &self.values {
                if let Some(value) = value {
                    env::set_var(key, value);
                } else {
                    env::remove_var(key);
                }
            }
        }
    }
    #[test]
    fn compiles_both_client_and_server_from_the_declared_schema() {
        let fixture = Environment::new(false);
        main().unwrap();
        let arguments = std::fs::read_to_string(&fixture.log).unwrap();
        assert!(arguments.contains("--include_imports"));
        assert!(arguments.contains("--include_source_info"));
        assert!(arguments.contains("./src/proto/monitoring.proto"));
        let output = std::fs::read_to_string(fixture.out.join("monitoring.rs")).unwrap();
        assert!(output.contains("pub mod monitor_service_client"));
        assert!(output.contains("pub mod monitor_service_server"));
        assert!(output.contains("pub struct SystemInfoResponse"));
        assert!(fixture.scratch.path().exists());
    }
    #[test]
    fn compiler_failure_preserves_schema_context_and_does_not_emit_code() {
        let fixture = Environment::new(true);
        let error = main().unwrap_err().to_string();
        assert!(error.contains("Failed to compile proto file ./src/proto/monitoring.proto"));
        assert!(error.contains("unit protoc failure"));
        assert!(fixture.log.exists());
        assert!(!fixture.out.join("monitoring.rs").exists());
    }
}
