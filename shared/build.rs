fn main() {
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile(&["./src/proto/monitoring.proto"], &["./src/proto"])
        .expect("Failed to compile proto files");
}
