fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile(&["./src/proto/monitoring.proto"], &["./src/proto"])
        .map_err(|e| format!("Failed to compile proto file ./src/proto/monitoring.proto: {e}"))?;
    Ok(())
}
