use std::{env, process::ExitCode};

fn main() -> ExitCode {
    let output = probe_cli::run(env::args().skip(1));
    print!("{}", output.stdout);
    eprint!("{}", output.stderr);
    ExitCode::from(output.exit_code)
}
