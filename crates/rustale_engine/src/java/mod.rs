use anyhow::Result;
use std::path::PathBuf;

pub use rustale_shared::java::{is_jre_installed_at, get_jre_exec_path, proxy, tracking};
pub use rustale_shared::java::detection::JavaInfo;

pub mod detection;

/// Gets the path to the Java executable from the tools/jre/latest directory
/// (Engine-specific because it uses GamePaths)
pub fn get_java_exec(base_dir: &PathBuf) -> Result<String> {
    rustale_shared::java::get_java_exec(base_dir)
}
