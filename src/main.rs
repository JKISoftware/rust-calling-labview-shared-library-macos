use std::env;
use std::ffi::{CStr, CString};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use libloading::{Library, Symbol};

type IncrementFn = unsafe extern "C" fn(i32) -> i32;

unsafe fn dlerror_str() -> String {
    let p = libc::dlerror();
    if p.is_null() {
        "(no dlerror)".to_string()
    } else {
        CStr::from_ptr(p).to_string_lossy().into_owned()
    }
}

fn main() -> ExitCode {
    // Optional argv[1]: override the framework path so the binary can probe
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

    // Optional WORKAROUND env var: try one of the candidate teardown
    // workarounds investigated for this bug. See README for the full
    // result table.
    //
    //   ""         (default)  faithful reproducer using libloading; exits 139
    //   "forget"   skip Rust Library::drop -> dlclose; still exits 139
    //   "nodelete" raw dlopen with RTLD_NODELETE; still exits 139
    //   "exit"     end with libc::_exit() instead of returning; exits 0
    let workaround = env::var("WORKAROUND").unwrap_or_default();

    println!("OS:           {} ({})", env::consts::OS, env::consts::ARCH);
    println!("Framework:    {}", framework_path.display());
    if !workaround.is_empty() {
        println!("Workaround:   {workaround}");
    }

    if !framework_path.exists() {
        eprintln!();
        eprintln!("Framework binary not found at: {}", framework_path.display());
        if args.len() == 1 {
            eprintln!("Open shared-library.lvproj in LabVIEW and build the");
            eprintln!("'Shared Library' build spec, then re-run.");
        }
        return ExitCode::from(2);
    }

    let all_correct = match workaround.as_str() {
        "nodelete" => run_with_raw_dlopen(&framework_path),
        _ => run_with_libloading(&framework_path, &workaround),
    };

    let exit_code: i32 = if all_correct { 0 } else { 1 };

    if workaround == "exit" {
        // libc::_exit bypasses atexit handlers, C++ static destructors, and
        // dyld terminator routines entirely. Empirically this is the only
        // workaround that produces a clean exit code rather than SIGSEGV 139.
        println!("Exiting via libc::_exit({exit_code})");
        unsafe { libc::_exit(exit_code) };
    }

    if !all_correct {
        eprintln!();
        eprintln!("One or more Increment results did not match expected value.");
    }
    ExitCode::from(exit_code as u8)
}

/// Default loader: libloading. Mirrors the loader used in production
/// LabVIEW host code. When `workaround == "forget"` the Library is
/// `mem::forget`-ed so its Drop never runs `dlclose`.
fn run_with_libloading(path: &Path, workaround: &str) -> bool {
    let library = match unsafe { Library::new(path) } {
        Ok(lib) => lib,
        Err(e) => {
            eprintln!("Library::new failed: {e}");
            return false;
        }
    };
    println!("Loaded OK (libloading)");

    // Copy the function pointer out of the Symbol so the Symbol's borrow of
    // `library` ends before we (optionally) `mem::forget` it below.
    let increment_fn: IncrementFn = {
        let increment: Symbol<IncrementFn> = match unsafe { library.get(b"Increment") } {
            Ok(sym) => sym,
            Err(e) => {
                eprintln!("Symbol lookup for `Increment` failed: {e}");
                return false;
            }
        };
        *increment
    };

    let ok = call_increments(increment_fn);

    if workaround == "forget" {
        println!("std::mem::forget(library) — skipping Library::drop");
        std::mem::forget(library);
    }

    ok
}

/// Raw libc loader, used when `WORKAROUND=nodelete` so the dlopen flag
/// bits can include `RTLD_NODELETE` (libloading does not expose dlopen
/// flags directly).
fn run_with_raw_dlopen(path: &Path) -> bool {
    let path_c = match CString::new(path.as_os_str().as_encoded_bytes()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Path contains NUL: {e}");
            return false;
        }
    };
    let flags = libc::RTLD_NOW | libc::RTLD_NODELETE;
    println!("dlopen flags: RTLD_NOW | RTLD_NODELETE (0x{flags:x})");
    let handle = unsafe { libc::dlopen(path_c.as_ptr(), flags) };
    if handle.is_null() {
        eprintln!("dlopen failed: {}", unsafe { dlerror_str() });
        return false;
    }
    println!("Loaded OK (libc::dlopen)");

    let sym_name = CString::new("Increment").unwrap();
    let sym = unsafe { libc::dlsym(handle, sym_name.as_ptr()) };
    if sym.is_null() {
        eprintln!("dlsym for `Increment` failed: {}", unsafe { dlerror_str() });
        return false;
    }
    let increment: IncrementFn = unsafe { std::mem::transmute(sym) };

    let ok = call_increments(increment);

    // With RTLD_NODELETE set, dlclose does not actually unload — but we
    // still call it so the test exercises the same path that runs at
    // process teardown.
    let _ = unsafe { libc::dlclose(handle) };
    println!("dlclose returned");

    ok
}

/// Call the loaded `Increment` function across a spread of inputs (zero,
/// small positive, negative, larger positive). The increment relationship
/// holding for arbitrary values is much stronger evidence than a single
/// fortunate case.
fn call_increments(increment: IncrementFn) -> bool {
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
    all_correct
}
