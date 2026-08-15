use std::{env, io, process::ExitCode};

fn main() -> ExitCode {
    let mut stdin = io::stdin().lock();
    let output = probe_cli::run_with_stdin(env::args().skip(1), &mut stdin);
    print!("{}", output.stdout);
    eprint!("{}", output.stderr);
    ExitCode::from(output.exit_code)
}
