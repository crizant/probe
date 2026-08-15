use std::{env, process::ExitCode};

fn main() -> ExitCode {
    match probe_cli::run(env::args().skip(1)) {
        Ok(output) => {
            print!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(probe_cli::INVALID_ARGUMENTS_EXIT_CODE)
        }
    }
}
