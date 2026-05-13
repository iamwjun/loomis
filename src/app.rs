use crate::config::LoomisConfig;
use crate::server::{serve_config, serve_html, ServerError};
use std::env;
use std::fmt;
use std::path::PathBuf;

pub fn run_cli() -> Result<(), CliError> {
    let args: Vec<String> = env::args().collect();
    run_with_args(args)
}

fn run_with_args<I>(args: I) -> Result<(), CliError>
where
    I: IntoIterator<Item = String>,
{
    let cli = CliConfig::parse(args)?;
    if cli.help {
        print_usage(&cli.bin);
        return Ok(());
    }

    if let Some(config_path) = cli.config {
        let config = LoomisConfig::load_from_path(&config_path).map_err(CliError::Config)?;
        return serve_config(&config).map_err(CliError::Server);
    }

    let path = cli.path.unwrap_or_else(|| PathBuf::from("example"));
    let port = cli.port.unwrap_or(3000);
    serve_html(&path, port).map_err(CliError::Server)
}

#[derive(Debug)]
pub enum CliError {
    Config(crate::config::ConfigError),
    InvalidArgument(String),
    Server(ServerError),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(error) => write!(f, "{error}"),
            Self::InvalidArgument(message) => write!(f, "{message}"),
            Self::Server(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for CliError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Config(error) => Some(error),
            Self::InvalidArgument(_) => None,
            Self::Server(error) => Some(error),
        }
    }
}

#[derive(Debug, Default)]
struct CliConfig {
    bin: String,
    config: Option<PathBuf>,
    path: Option<PathBuf>,
    port: Option<u16>,
    help: bool,
}

impl CliConfig {
    fn parse<I>(args: I) -> Result<Self, CliError>
    where
        I: IntoIterator<Item = String>,
    {
        let mut args = args.into_iter();
        let bin = args.next().unwrap_or_else(|| String::from("loomis"));
        let mut cli = CliConfig {
            bin,
            ..Self::default()
        };

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--config" => {
                    let value = args.next().ok_or_else(|| {
                        CliError::InvalidArgument(String::from("--config requires a value"))
                    })?;
                    cli.config = Some(PathBuf::from(value));
                }
                "--path" => {
                    let value = args.next().ok_or_else(|| {
                        CliError::InvalidArgument(String::from("--path requires a value"))
                    })?;
                    cli.path = Some(PathBuf::from(value));
                }
                "--port" => {
                    let value = args.next().ok_or_else(|| {
                        CliError::InvalidArgument(String::from("--port requires a value"))
                    })?;
                    cli.port = Some(value.parse::<u16>().map_err(|_| {
                        CliError::InvalidArgument(format!("invalid port: {value}"))
                    })?);
                }
                "--help" | "-h" => cli.help = true,
                _ => {
                    return Err(CliError::InvalidArgument(format!(
                        "unknown argument: {arg}\n\n{}",
                        usage_text(&cli.bin)
                    )));
                }
            }
        }

        if cli.config.is_some() && (cli.path.is_some() || cli.port.is_some()) {
            return Err(CliError::InvalidArgument(String::from(
                "--config cannot be combined with --path or --port",
            )));
        }

        Ok(cli)
    }
}

fn print_usage(bin: &str) {
    println!("{}", usage_text(bin));
}

fn usage_text(bin: &str) -> String {
    format!(
        "Usage: {bin} [--config <path>] [--path <html-dir>] [--port <port>]\n\nModes:\n  --config <path>  Start from a Loomis TOML config file\n  --path <dir>     Start the legacy single-site static server\n  --port <port>    Override the legacy static server port\n\nDefaults:\n  --path example\n  --port 3000"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_config_mode() {
        let cli = CliConfig::parse(vec![
            String::from("loomis"),
            String::from("--config"),
            String::from("examples/loomis.toml"),
        ])
        .expect("cli args should parse");

        assert_eq!(cli.config, Some(PathBuf::from("examples/loomis.toml")));
        assert!(cli.path.is_none());
        assert!(cli.port.is_none());
    }

    #[test]
    fn rejects_mixed_config_and_legacy_args() {
        let error = CliConfig::parse(vec![
            String::from("loomis"),
            String::from("--config"),
            String::from("examples/loomis.toml"),
            String::from("--port"),
            String::from("8080"),
        ])
        .expect_err("cli args should fail");

        assert_eq!(
            error.to_string(),
            "--config cannot be combined with --path or --port"
        );
    }
}
