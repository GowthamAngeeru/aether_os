use protoc_bin_vendored::protoc_bin_path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=brain/proto/aether.proto");
    println!("cargo:rerun-if-changed=build.rs");

    let protoc_path =
        protoc_bin_path().map_err(|e| format!("Failed to locate vendored protoc binary: {}", e))?;

    std::env::set_var("PROTOC", protoc_path);

    tonic_build::configure()
        .build_server(false)
        .build_client(true)
        .compile(&["brain/proto/aether.proto"], &["brain/proto"])
        .map_err(|e| {
            format!(
                "tonic_build failed to compile brain/proto/aether.proto: {}\n\
                Check that the proto file exists and has valid syntax.",
                e
            )
        })?;

    println!("cargo:warning=✅ gRPC bridge compiled from brain/proto/aether.proto");
    Ok(())
}
