use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use libloading::{Library, Symbol};

type IncrementFn = unsafe extern "C" fn(i32) -> i32;

fn main() -> ExitCode {
    // Allow argv[1] to override the framework path so the binary can probe
    // arbitrary LabVIEW-built frameworks; defaults to the workspace's own
    // SharedLib.framework so plain `cargo run` works.
    let args: Vec<String> = env::args().collect();
    let framework_path: PathBuf = if args.len() > 1 {
        PathBuf::from(&args[1])
    } else {
        [
            env!("CARGO_MANIFEST_DIR"),
            "target",
            "labview",
            "SharedLib.framework",
            "Versions",
            "A",
            "SharedLib",
        ]
        .iter()
        .collect()
    };

    println!("OS:           {} ({})", env::consts::OS, env::consts::ARCH);
    println!("Framework:    {}", framework_path.display());

    if !framework_path.exists() {
        eprintln!();
        eprintln!("Framework binary not found at: {}", framework_path.display());
        if args.len() == 1 {
            eprintln!("Open shared-library.lvproj in LabVIEW and build the");
            eprintln!("'Shared Library' build spec, then re-run.");
        }
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

    // Call Increment across a spread of inputs (zero, small positive,
    // negative, larger positive) so the increment relationship is shown
    // to hold for arbitrary values rather than just one fortunate case.
    let inputs: [i32; 5] = [0, 1, 5, 41, -1];
    let mut all_correct = true;
    for &input in &inputs {
        let result = unsafe { increment(input) };
        let expected = input.wrapping_add(1);
        let status = if result == expected { "OK" } else { "FAIL" };
        println!("Increment({input}) = {result} (expected {expected}) [{status}]");
        if result != expected {
            all_correct = false;
        }
    }

    if !all_correct {
        eprintln!();
        eprintln!("One or more Increment results did not match expected value.");
        return ExitCode::from(1);
    }

    ExitCode::SUCCESS
}
