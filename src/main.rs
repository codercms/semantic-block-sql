mod cli;

use std::process::ExitCode;

use clap::{CommandFactory, FromArgMatches};
use cli::Cli;

fn main() -> ExitCode {
    let matches = Cli::command()
        .version(env!("CARGO_PKG_VERSION"))
        .get_matches();
    let cli = Cli::from_arg_matches(&matches).expect("Clap validated CLI arguments");

    match cli.run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("semblock: {error}");
            error.exit_code()
        }
    }
}
