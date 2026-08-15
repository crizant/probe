//! Command-line presentation adapter for Probe.

#![forbid(unsafe_code)]

/// Exit code used when command-line arguments are invalid.
pub const INVALID_ARGUMENTS_EXIT_CODE: u8 = 2;

/// Returns the stable top-level help text.
#[must_use]
pub const fn help() -> &'static str {
    concat!(
        "Probe - a fast, native, local-first API client\n",
        "\n",
        "Usage: probe [OPTIONS]\n",
        "\n",
        "Options:\n",
        "  -h, --help  Print help\n",
    )
}

/// Runs the CLI adapter for an argument sequence that excludes the executable name.
///
/// Successful output is returned to the caller so the binary remains a thin adapter
/// and the behavior can be tested without spawning a process.
pub fn run<I, S>(args: I) -> Result<&'static str, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut args = args.into_iter();

    match args.next() {
        None => Ok(help()),
        Some(argument) if matches!(argument.as_ref(), "-h" | "--help") => {
            if let Some(unexpected) = args.next() {
                return Err(format!(
                    "unexpected argument after help option: {}",
                    unexpected.as_ref()
                ));
            }
            Ok(help())
        }
        Some(argument) => Err(format!("unknown argument: {}", argument.as_ref())),
    }
}

#[cfg(test)]
mod tests {
    use super::{help, run};

    #[test]
    fn help_option_returns_usage() {
        assert_eq!(run(["--help"]), Ok(help()));
        assert!(help().contains("Usage: probe"));
    }

    #[test]
    fn unknown_argument_is_rejected() {
        assert_eq!(
            run(["--unknown"]),
            Err("unknown argument: --unknown".to_owned())
        );
    }
}
