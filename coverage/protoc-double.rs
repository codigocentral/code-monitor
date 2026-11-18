//! Command double for build.rs unit tests. It never compiles a schema.
use std::{env, fs, path::PathBuf, process};
fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args == ["--version"] {
        println!("libprotoc 33.2 (unit double)");
        return;
    }
    let scratch = PathBuf::from(env::var_os("TMPDIR").expect("owned scratch root"));
    let log = PathBuf::from(env::var_os("UNIT_PROTOC_ARGS").expect("unit invocation log"));
    assert!(log.starts_with(&scratch));
    fs::write(&log, args.join("\n")).unwrap();
    if env::var("UNIT_PROTOC_FAIL").as_deref() == Ok("1") {
        eprintln!("unit protoc failure");
        process::exit(2);
    }
    let index = args
        .iter()
        .position(|arg| arg == "-o")
        .expect("descriptor output flag");
    let output = PathBuf::from(&args[index + 1]);
    assert!(output.starts_with(&scratch));
    let fixture = env::var_os("UNIT_DESCRIPTOR_FIXTURE").expect("public descriptor fixture");
    fs::copy(fixture, output).unwrap();
}
