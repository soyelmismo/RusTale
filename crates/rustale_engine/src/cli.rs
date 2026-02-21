// CLI module for command line interface functionality
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct CliArgs {
    pub headless: bool,
    pub proxy: Option<String>,
    pub data_dir: Option<PathBuf>,
    pub verbose: bool,
    pub online_mode: Option<String>,
    pub branch: Option<String>,
    pub game_version: Option<String>,
    pub java_exec_args: Option<String>,
    pub server_args: Option<String>,
    pub tunnel: Option<String>,
}

impl Default for CliArgs {
    fn default() -> Self {
        Self {
            headless: false,
            proxy: None,
            data_dir: None,
            verbose: false,
            online_mode: None,
            branch: None,
            game_version: None,
            java_exec_args: None,
            server_args: None,
            tunnel: None,
        }
    }
}

pub fn parse_args() -> CliArgs {
    // Basic argument parsing - can be enhanced with clap later
    let args = std::env::args().collect::<Vec<_>>();
    
    let mut cli_args = CliArgs::default();
    
    for (i, arg) in args.iter().enumerate() {
        match arg.as_str() {
            "--headless" => cli_args.headless = true,
            "--verbose" | "-v" => cli_args.verbose = true,
            "--proxy" | "-p" => {
                if i + 1 < args.len() {
                    cli_args.proxy = Some(args[i + 1].clone());
                }
            }
            "--data-dir" | "-d" => {
                if i + 1 < args.len() {
                    cli_args.data_dir = Some(PathBuf::from(&args[i + 1]));
                }
            }
            _ => {}
        }
    }
    
    cli_args
}
