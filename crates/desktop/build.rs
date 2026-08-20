use std::{env, error::Error, ffi::OsStr};

const WINDOWS_ICON: &str = "assets/app-icon/windows/Probe.ico";

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo::rerun-if-changed={WINDOWS_ICON}");

    if env::var_os("CARGO_CFG_TARGET_OS").as_deref() == Some(OsStr::new("windows")) {
        winresource::WindowsResource::new()
            .set_icon(WINDOWS_ICON)
            .compile()?;
    }

    Ok(())
}
