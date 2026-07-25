//! Generates the sidecar gRPC client from the `.proto`.
//!
//! The generated code is never hand-edited and is excluded from the coverage
//! gate, since asserting on prost's output would test prost.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto = "../../proto/pgprox/auth/v1/auth.proto";
    println!("cargo:rerun-if-changed={proto}");
    println!("cargo:rerun-if-changed=../../proto");

    tonic_prost_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(&[proto], &["../../proto"])?;
    Ok(())
}
