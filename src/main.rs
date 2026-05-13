use std::env;
use std::process;

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let Some(config) = CliConfig::from_env()? else {
        return Ok(());
    };

    loomis::serve_html(&config.path, config.port).map_err(|error| error.to_string())
}

struct CliConfig {
    path: String,
    port: u16,
}

impl CliConfig {
    fn from_env() -> Result<Option<Self>, String> {
        let mut args = env::args();
        let bin = args.next().unwrap_or_else(|| String::from("loomis"));
        let mut path = String::from("example");
        let mut port = 3000_u16;

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--path" => {
                    path = args
                        .next()
                        .ok_or_else(|| String::from("--path requires a value"))?;
                }
                "--port" => {
                    let value = args
                        .next()
                        .ok_or_else(|| String::from("--port requires a value"))?;
                    port = value
                        .parse::<u16>()
                        .map_err(|_| format!("invalid port: {value}"))?;
                }
                "--help" | "-h" => {
                    print_usage(&bin);
                    return Ok(None);
                }
                _ => {
                    return Err(format!("unknown argument: {arg}\n\n{}", usage_text(&bin)));
                }
            }
        }

        Ok(Some(Self { path, port }))
    }
}

fn print_usage(bin: &str) {
    println!("{}", usage_text(bin));
}

fn usage_text(bin: &str) -> String {
    format!(
        "Usage: {bin} [--path <html-dir>] [--port <port>]\n\nDefaults:\n  --path example\n  --port 3000"
    )
}
