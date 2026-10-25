use std::process::Command;

fn main() {
    let pkg_version = env!("CARGO_PKG_VERSION");
    let (major, minor, patch) = parse_semver(pkg_version);

    let (git_hash, is_dirty, commit_count, commit_date) = query_git();

    let sw_ver_str = if git_hash.is_empty() || git_hash == "unknown" {
        pkg_version.to_string()
    } else if is_dirty {
        format!("{}+{}.dirty", pkg_version, git_hash)
    } else {
        format!("{}+{}", pkg_version, git_hash)
    };

    // Strictly monotonic u32 for Matter SoftwareVersion:
    // (major * 10_000_000) + (minor * 100_000) + (patch * 10_000) + commit_count
    let sw_ver_num = (major as u32 * 10_000_000)
        + (minor as u32 * 100_000)
        + (patch as u32 * 10_000)
        + (commit_count as u32);

    println!("cargo:rustc-env=HVAC_SW_VER_STR={}", sw_ver_str);
    println!("cargo:rustc-env=HVAC_SW_VER_NUM={}", sw_ver_num);
    println!("cargo:rustc-env=HVAC_BUILD_DATE={}", commit_date);
    println!("cargo:rustc-env=HVAC_GIT_HASH={}", git_hash);
    println!("cargo:rustc-env=HVAC_IS_DIRTY={}", is_dirty);

    // Re-run build.rs when git HEAD, index, or the build script itself changes
    if let Ok(output) = Command::new("git").args(["rev-parse", "--git-dir"]).output() {
        if output.status.success() {
            let git_dir = String::from_utf8_lossy(&output.stdout).trim().to_string();
            println!("cargo:rerun-if-changed={}/HEAD", git_dir);
            println!("cargo:rerun-if-changed={}/index", git_dir);
        }
    }
    println!("cargo:rerun-if-changed=build.rs");
}

fn query_git() -> (String, bool, u64, String) {
    let git_hash = Command::new("git")
        .args(["rev-parse", "--short=7", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let is_dirty = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
        .unwrap_or(false);

    let commit_count = Command::new("git")
        .args(["rev-list", "--count", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<u64>().ok())
        .unwrap_or(0);

    let commit_date = Command::new("git")
        .args(["log", "-1", "--format=%cs"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "".to_string());

    (git_hash, is_dirty, commit_count, commit_date)
}

fn parse_semver(version: &str) -> (u16, u16, u16) {
    let mut parts = version.split('.');
    let major = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let patch = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    (major, minor, patch)
}
