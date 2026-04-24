use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use libloading::{Library, Symbol};

type IncrementFn = unsafe extern "C" fn(i32) -> i32;

fn main() -> ExitCode {
    // Resolve the framework binary relative to the workspace root so
    // `cargo run` works from any current directory.
    let framework_path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "target",
        "labview",
        "SharedLib.framework",
        "Versions",
        "A",
        "SharedLib",
    ]
    .iter()
    .collect();

    println!("OS:           {} ({})", env::consts::OS, env::consts::ARCH);
    println!("Framework:    {}", framework_path.display());

    if !framework_path.exists() {
        eprintln!();
        eprintln!("Framework binary not found.");
        eprintln!("Open shared-library.lvproj in LabVIEW and build the");
        eprintln!("'Shared Library' build spec, then re-run.");
        return ExitCode::from(2);
    }

    let library = match unsafe { Library::new(&framework_path) } {
        Ok(lib) => lib,
        Err(e) => {
            eprintln!("Library::new failed: {e}");
            return ExitCode::from(1);
        }
    };
    println!("Loaded OK");

    let increment: Symbol<IncrementFn> = match unsafe { library.get(b"Increment") } {
        Ok(sym) => sym,
        Err(e) => {
            eprintln!("Symbol lookup for `Increment` failed: {e}");
            return ExitCode::from(1);
        }
    };

    let result = unsafe { increment(5) };
    println!("Increment(5) = {result}");

    ExitCode::SUCCESS
}
