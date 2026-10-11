use std::{
    env,
    io::{self, BufRead},
    process::ExitCode,
};

fn main() -> ExitCode {
    let result = probe_cli::run_with_io(
        env::args().skip(1),
        &mut io::stdin(),
        &mut io::stdout(),
        &mut io::stderr(),
        |_| {
            let (tx, rx) = tokio::sync::mpsc::channel(16);
            // Detached blocking input is independent of the async runtime. A
            // terminal waiting for a line must not delay events or runtime exit.
            std::thread::spawn(move || {
                let mut reader = io::stdin().lock();
                loop {
                    let mut line = String::new();
                    match reader.read_line(&mut line) {
                        Ok(0) => break,
                        Ok(_) => {
                            if line.ends_with('\n') {
                                line.pop();
                                if line.ends_with('\r') {
                                    line.pop();
                                }
                            }
                            if tx.blocking_send(Ok(line)).is_err() {
                                break;
                            }
                        }
                        Err(error) => {
                            let _ = tx.blocking_send(Err(error));
                            break;
                        }
                    }
                }
            });
            Ok(rx)
        },
    );
    match result {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("error[output_error]: {error}");
            ExitCode::from(probe_cli::EXECUTION_EXIT_CODE)
        }
    }
}
