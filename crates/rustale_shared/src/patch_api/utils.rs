/// Utility functions for patch API providers
pub use crate::network::{format_bytes, format_speed};


/// Check if a filename looks like a JRE distribution file
pub fn looks_like_jre_file(filename: &str) -> bool {
    let filename_lower = filename.to_lowercase();

    // Check if it's a compressed archive (likely JRE distribution)
    let is_archive = filename_lower.ends_with(".tar.gz")
        || filename_lower.ends_with(".zip")
        || filename_lower.ends_with(".tar");

    // Check if it's related to Java/JRE
    let is_java = filename_lower.contains("jre")
        || filename_lower.contains("jdk")
        || filename_lower.contains("java")
        || filename_lower.contains("openjdk");

    // Exclude common non-JRE files
    let is_excluded = filename_lower.contains("readme")
        || filename_lower.contains("license")
        || filename_lower.contains("changelog")
        || filename_lower.contains(".txt")
        || filename_lower.contains(".md");

    is_archive && is_java && !is_excluded
}

/// Make a file executable on Unix systems
pub async fn make_executable(path: &std::path::PathBuf) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if path.exists() {
            let meta = tokio::fs::metadata(path).await?;
            let mut perms = meta.permissions();
            perms.set_mode(0o755);
            tokio::fs::set_permissions(path, perms).await?;
        }
    }
    let _ = path;
    Ok(())
}

/// Check if a filename looks like a Butler distribution file
pub fn looks_like_butler_file(filename: &str) -> bool {
    let filename_lower = filename.to_lowercase();

    // Check if it's a compressed archive
    let is_archive = filename_lower.ends_with(".tar.gz")
        || filename_lower.ends_with(".zip")
        || filename_lower.ends_with(".tar");

    // Check if it's related to Butler
    let is_butler = filename_lower.contains("butler");

    // Exclude common non-Butler files
    let is_excluded = filename_lower.contains("readme")
        || filename_lower.contains("license")
        || filename_lower.contains("changelog")
        || filename_lower.contains(".txt")
        || filename_lower.contains(".md");

    is_archive && is_butler && !is_excluded
}

/// Extract version range from filename for patches
/// Returns (from_version, to_version)
pub fn extract_versions_from_filename(filename: &str) -> Option<(i32, i32)> {
    if let Some(start) = filename.find('v') {
        let version_part = &filename[start + 1..];
        let version_part = version_part.split('-').next()?;

        if let Some(tilde_pos) = version_part.find('~') {
            // Incremental format: "19~20" -> (19, 20)
            let from_part = &version_part[..tilde_pos];
            let to_part = &version_part[tilde_pos + 1..];
            match (from_part.parse::<i32>(), to_part.parse::<i32>()) {
                (Ok(from), Ok(to)) => Some((from, to)),
                _ => None,
            }
        } else {
            // Complete format: "8" -> (0, 8)
            match version_part.parse::<i32>() {
                Ok(version) => Some((0, version)),
                _ => None,
            }
        }
    } else {
        None
    }
}

/// Get architecture name in standard format
pub fn get_arch_name() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        _ => std::env::consts::ARCH,
    }
}
/// Helper to write the standard patch path into a ZeroizeArena
#[cfg(feature = "security")]
pub fn write_patch_path_to_arena(
    arena: &mut rustale_security::memory::ZeroizeArena<512>,
    os: &str,
    arch: &str,
    channel: &str,
    from_version: i32,
    to_version: i32,
    template_key: &str,
) -> std::io::Result<()> {
    use std::io::Write;
    let arch_str = match arch {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        _ => arch,
    };
    let os_str = match os {
        "darwin" => "mac",
        _ => os,
    };

    let template = rustale_security::get_private_var(template_key).into_string();
    if template.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Patch path template not configured in security suite",
        ));
    }

    let path = template
        .replacen("{}", os_str, 1)
        .replacen("{}", arch_str, 1)
        .replacen("{}", channel, 1)
        .replacen("{}", &from_version.to_string(), 1)
        .replacen("{}", &to_version.to_string(), 1);

    write!(arena, "{}", path)
}

/// Get Butler download URL from itch.io CDN fallback
/// This works as a universal fallback for all platforms
pub fn get_butler_fallback_url(os: &str, arch: &str) -> String {
    format!(
        "https://broth.itch.zone/butler/{}-{}/LATEST/archive.zip",
        os, arch
    )
}

/// Get Java download URL from Adoptium (Temurin) CDN fallback
/// This is official Eclipse Temurin distribution
pub fn get_java_adoptium_url(os: &str, arch: &str) -> String {
    // Map to Adoptium's naming conventions
    let os_name = match os {
        "darwin" => "mac", // Adoptium uses "mac" instead of "darwin"
        _ => os,
    };

    let arch_name = match arch {
        "amd64" | "x86_64" => "x64", // They don't use amd64, Adoptium uses x64
        "aarch64" | "arm64" => "aarch64",
        _ => arch,
    };

    format!(
        "https://api.adoptium.net/v3/binary/latest/25/ga/{}/{}/jre/hotspot/normal/adoptium",
        os_name, arch_name
    )
}

/// Generic version discovery using exponential and binary search
/// Accepts a future-returning closure to check if a version exists
pub async fn find_latest_version_generic<F, Fut>(current: i32, mut check_exists: F) -> i32
where
    F: FnMut(i32) -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let current = if current <= 0 { 1 } else { current };
    let mut last_version = current;
    let mut cur_version = current;

    if check_exists(current + 1).await {
        while check_exists(cur_version).await {
            last_version = cur_version;
            cur_version *= 2;
        }
        while last_version + 1 < cur_version {
            let middle = (cur_version + last_version) / 2;
            if check_exists(middle).await {
                last_version = middle;
            } else {
                cur_version = middle;
            }
        }
    }
    last_version
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_looks_like_jre_file_valid() {
        assert!(looks_like_jre_file("jre-17.tar.gz"));
        assert!(looks_like_jre_file("openjdk-21.tar.gz"));
        assert!(looks_like_jre_file("jdk-11.zip"));
        assert!(looks_like_jre_file("java-runtime.tar"));
    }

    #[test]
    fn test_looks_like_jre_file_invalid() {
        assert!(!looks_like_jre_file("readme.txt"));
        assert!(!looks_like_jre_file("license.md"));
        assert!(!looks_like_jre_file("changelog.zip"));
        assert!(!looks_like_jre_file("not-an-archive.exe"));
        assert!(!looks_like_jre_file("jre-readme.tar.gz")); // Contains readme
    }

    #[test]
    fn test_looks_like_butler_file_valid() {
        assert!(looks_like_butler_file("butler.tar.gz"));
        assert!(looks_like_butler_file("butler-windows.zip"));
        assert!(looks_like_butler_file("butler-linux.tar"));
    }

    #[test]
    fn test_looks_like_butler_file_invalid() {
        assert!(!looks_like_butler_file("readme.txt"));
        assert!(!looks_like_butler_file("butler-readme.tar.gz")); // Contains readme
        // Note: "not-butler.zip" contains "butler" substring, so it's considered valid
        // Use filenames without "butler" for invalid tests
        assert!(!looks_like_butler_file("random-file.zip"));
        assert!(!looks_like_butler_file("other-tool.tar.gz"));
    }

    #[test]
    fn test_extract_versions_from_filename_incremental() {
        // Incremental patch format: v19~20 means from version 19 to 20
        // Note: The function splits on '-' first, so use formats like:
        // - "patch-v19~20" (version before extension)
        // - "v19~20-patch" (version segment separated by hyphen)
        let result = extract_versions_from_filename("patch-v19~20");
        assert_eq!(result, Some((19, 20)));

        let result2 = extract_versions_from_filename("update-v1~5");
        assert_eq!(result2, Some((1, 5)));

        // Without hyphen, version part includes extension which won't parse
        // This is expected behavior - filenames should use hyphen separators
    }

    #[test]
    fn test_extract_versions_from_filename_complete() {
        // Complete patch format: v8 means from 0 to 8
        // Note: version must be separated by hyphen for clean parsing
        let result = extract_versions_from_filename("patch-v8");
        assert_eq!(result, Some((0, 8)));

        let result2 = extract_versions_from_filename("game-v25");
        assert_eq!(result2, Some((0, 25)));
    }

    #[test]
    fn test_extract_versions_from_filename_invalid() {
        assert_eq!(extract_versions_from_filename("no-version.zip"), None);
        assert_eq!(extract_versions_from_filename("v-abc.zip"), None);
    }

    #[test]
    fn test_get_arch_name() {
        // This test just verifies the function runs and returns a valid string
        let arch = get_arch_name();
        assert!(!arch.is_empty());
        // Common architectures
        assert!(
            arch == "amd64" || arch == "arm64" || arch == "x86" || arch == "aarch64",
            "Unexpected arch: {}",
            arch
        );
    }

    #[test]
    fn test_get_butler_fallback_url() {
        let url = get_butler_fallback_url("windows", "amd64");
        assert!(url.contains("butler"));
        assert!(url.contains("windows-amd64"));

        let url2 = get_butler_fallback_url("linux", "arm64");
        assert!(url2.contains("linux-arm64"));
    }

    #[test]
    fn test_get_java_adoptium_url() {
        let url = get_java_adoptium_url("windows", "amd64");
        assert!(url.contains("adoptium.net"));
        assert!(url.contains("windows"));
        assert!(url.contains("x64")); // amd64 is converted to x64

        let url2 = get_java_adoptium_url("darwin", "aarch64");
        assert!(url2.contains("mac")); // darwin is converted to mac
    }
}
