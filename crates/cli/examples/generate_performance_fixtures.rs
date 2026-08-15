use std::{env, fs, io, path::PathBuf};

#[path = "../benches/support/fixtures.rs"]
mod fixtures;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output_directory = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("usage: generate_performance_fixtures <directory>"))?;
    fs::create_dir_all(&output_directory)?;

    for request_count in fixtures::WORKSPACE_SIZES {
        let path = output_directory.join(format!("workspace-{request_count}.yml"));
        fs::write(&path, fixtures::bundled_workspace(request_count))?;
        println!("{}", path.display());
    }

    Ok(())
}
