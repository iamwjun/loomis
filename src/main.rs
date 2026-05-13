use std::process;

fn main() {
    if let Err(error) = loomis::run_cli() {
        eprintln!("{error}");
        process::exit(1);
    }
}
