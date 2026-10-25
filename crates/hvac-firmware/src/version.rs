//! Build and version metadata generated at compile time.
//!
//! Provides dynamic software versioning, git commit hashes, and monotonically
//! increasing numeric version identifiers for Matter Basic Information & OTA.

// ============================================================================
// Public Constants
// ============================================================================

/// Human-readable software version string for Matter Basic Information & Apple Home.
///
/// Example: `"0.1.0+7206e35"` or `"0.1.0+7206e35.dirty"`.
pub const SW_VER_STR: &str = env!("HVAC_SW_VER_STR");

/// Numeric software version for Matter / OTA.
///
/// Derived monotonically as `(major * 10_000_000) + (minor * 100_000) + (patch * 10_000) + commit_count`.
pub const SW_VER: u32 = parse_u32(env!("HVAC_SW_VER_NUM"));

/// Git commit hash (short 7-character string).
#[allow(dead_code)]
pub const GIT_HASH: &str = env!("HVAC_GIT_HASH");

/// Build/commit date in ISO-8601 format (`YYYY-MM-DD`).
pub const BUILD_DATE: &str = env!("HVAC_BUILD_DATE");

/// Whether the working tree had uncommitted changes at build time.
pub const IS_DIRTY: bool = match_bool(env!("HVAC_IS_DIRTY"));

// ============================================================================
// Private Helpers
// ============================================================================

const fn parse_u32(s: &str) -> u32 {
    let bytes = s.as_bytes();
    let mut val = 0u32;
    let mut i = 0;
    while i < bytes.len() {
        val = val * 10 + (bytes[i] - b'0') as u32;
        i += 1;
    }
    val
}

const fn match_bool(s: &str) -> bool {
    let bytes = s.as_bytes();
    bytes.len() == 4
        && bytes[0] == b't'
        && bytes[1] == b'r'
        && bytes[2] == b'u'
        && bytes[3] == b'e'
}
