use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use flate2::write::GzEncoder;
use flate2::Compression;

fn main() {
    println!("cargo:rerun-if-changed=assets/dashboard.html");

    let out_dir = env::var("OUT_DIR").expect("OUT_DIR environment variable not set");
    let dest_path = PathBuf::from(out_dir).join("dashboard.html.gz");

    let html_path = Path::new("assets/dashboard.html");
    if !html_path.exists() {
        println!("cargo:warning=assets/dashboard.html not found, attempting to build with bun...");
        let build_status = Command::new("bun")
            .args(["run", "build"])
            .current_dir("frontend")
            .status();

        let success = build_status.map(|s| s.success()).unwrap_or(false);
        if !success || !html_path.exists() {
            panic!(
                "assets/dashboard.html is missing. Please run 'cargo fe-build' to compile the frontend dashboard before building."
            );
        }
    }

    let html_bytes = fs::read(html_path).expect("Failed to read assets/dashboard.html");

    let output_file =
        File::create(&dest_path).expect("Failed to create destination dashboard.html.gz");
    let mut encoder = GzEncoder::new(output_file, Compression::best());
    encoder
        .write_all(&html_bytes)
        .expect("Failed to compress dashboard.html");
    encoder
        .finish()
        .expect("Failed to finish gzip compression");
}
