fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto_root = std::path::Path::new("../../proto");

    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(
            &[
                proto_root.join("agent.proto"),
                proto_root.join("job.proto"),
                proto_root.join("ledger.proto"),
            ],
            &[proto_root, &std::path::PathBuf::from(".")],
        )?;

    // Re-run if any proto file changes
    println!("cargo:rerun-if-changed=../../proto/agent.proto");
    println!("cargo:rerun-if-changed=../../proto/job.proto");
    println!("cargo:rerun-if-changed=../../proto/ledger.proto");

    Ok(())
}
