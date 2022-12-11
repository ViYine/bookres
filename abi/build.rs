use std::process::Command;

fn main() {
    tonic_build::configure()
        .out_dir("src/pb")
        .compile(&["protos/reservation.proto"], &["protos"])
        .unwrap();

    // run cargo fmt for generated files
    Command::new("cargo").args(["fmt"]).output().unwrap();

    // Tell Cargo that if the given file changes, to rerun this build script.
    println!("cargo:rerun-if-changed=protos/reservation.proto");
}
