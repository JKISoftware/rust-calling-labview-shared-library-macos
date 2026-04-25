use std::env;
use std::ffi::{CStr, CString};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use libloading::{Library, Symbol};

#[cfg(target_os = "macos")]
mod cocoa {
    use std::ffi::c_char;

    pub type Id = *mut std::ffi::c_void;
    type Class = *mut std::ffi::c_void;
    type Sel = *mut std::ffi::c_void;

    #[link(name = "AppKit", kind = "framework")]
    extern "C" {
        fn NSApplicationLoad() -> bool;
    }

    extern "C" {
        fn objc_getClass(name: *const c_char) -> Class;
        fn sel_registerName(name: *const c_char) -> Sel;
        fn objc_msgSend();
    }

    pub fn nsapplication_load() -> bool {
        unsafe { NSApplicationLoad() }
    }

    /// `[NSApplication sharedApplication]`
    pub fn shared_application() -> Id {
        unsafe {
            let cls = objc_getClass(b"NSApplication\0".as_ptr() as *const c_char);
            let sel = sel_registerName(b"sharedApplication\0".as_ptr() as *const c_char);
            let msg: extern "C" fn(Class, Sel) -> Id =
                std::mem::transmute(objc_msgSend as *const ());
            msg(cls, sel)
        }
    }

    /// `[app setActivationPolicy:policy]` — 0 = Regular.
    pub fn set_activation_policy(app: Id, policy: isize) {
        unsafe {
            let sel =
                sel_registerName(b"setActivationPolicy:\0".as_ptr() as *const c_char);
            let msg: extern "C" fn(Id, Sel, isize) =
                std::mem::transmute(objc_msgSend as *const ());
            msg(app, sel, policy)
        }
    }

    /// `[app finishLaunching]`
    pub fn finish_launching(app: Id) {
        unsafe {
            let sel = sel_registerName(b"finishLaunching\0".as_ptr() as *const c_char);
            let msg: extern "C" fn(Id, Sel) =
                std::mem::transmute(objc_msgSend as *const ());
            msg(app, sel)
        }
    }

    /// `[app activateIgnoringOtherApps:flag]`
    pub fn activate_ignoring_other_apps(app: Id, flag: bool) {
        unsafe {
            let sel = sel_registerName(
                b"activateIgnoringOtherApps:\0".as_ptr() as *const c_char,
            );
            let msg: extern "C" fn(Id, Sel, bool) =
                std::mem::transmute(objc_msgSend as *const ());
            msg(app, sel, flag)
        }
    }

    /// `[NSRunLoop currentRunLoop]`
    pub fn current_run_loop() -> Id {
        unsafe {
            let cls = objc_getClass(b"NSRunLoop\0".as_ptr() as *const c_char);
            let sel = sel_registerName(b"currentRunLoop\0".as_ptr() as *const c_char);
            let msg: extern "C" fn(Class, Sel) -> Id =
                std::mem::transmute(objc_msgSend as *const ());
            msg(cls, sel)
        }
    }

    /// `[NSDate dateWithTimeIntervalSinceNow:seconds]`
    pub fn date_with_interval_since_now(seconds: f64) -> Id {
        unsafe {
            let cls = objc_getClass(b"NSDate\0".as_ptr() as *const c_char);
            let sel = sel_registerName(
                b"dateWithTimeIntervalSinceNow:\0".as_ptr() as *const c_char,
            );
            let msg: extern "C" fn(Class, Sel, f64) -> Id =
                std::mem::transmute(objc_msgSend as *const ());
            msg(cls, sel, seconds)
        }
    }

    /// `[runloop runUntilDate:date]` — blocks until either `date` is reached
    /// or an input source fires; processes any queued main-thread events.
    pub fn run_until_date(runloop: Id, date: Id) {
        unsafe {
            let sel = sel_registerName(b"runUntilDate:\0".as_ptr() as *const c_char);
            let msg: extern "C" fn(Id, Sel, Id) =
                std::mem::transmute(objc_msgSend as *const ());
            msg(runloop, sel, date)
        }
    }

    /// `[NSApp run]` — enters NSApplication's main event loop. Blocks
    /// indefinitely; only returns if `[NSApp stop:]` is called.
    pub fn nsapp_run(app: Id) {
        unsafe {
            let sel = sel_registerName(b"run\0".as_ptr() as *const c_char);
            let msg: extern "C" fn(Id, Sel) =
                std::mem::transmute(objc_msgSend as *const ());
            msg(app, sel)
        }
    }

    /// `[[NSMenu alloc] init]` — returns a fresh empty NSMenu.
    pub fn nsmenu_alloc_init() -> Id {
        unsafe {
            let cls = objc_getClass(b"NSMenu\0".as_ptr() as *const c_char);
            let alloc_sel = sel_registerName(b"alloc\0".as_ptr() as *const c_char);
            let init_sel = sel_registerName(b"init\0".as_ptr() as *const c_char);
            let alloc_fn: extern "C" fn(Class, Sel) -> Id =
                std::mem::transmute(objc_msgSend as *const ());
            let menu = alloc_fn(cls, alloc_sel);
            let init_fn: extern "C" fn(Id, Sel) -> Id =
                std::mem::transmute(objc_msgSend as *const ());
            init_fn(menu, init_sel)
        }
    }

    /// `[app setMainMenu:menu]`
    pub fn set_main_menu(app: Id, menu: Id) {
        unsafe {
            let sel = sel_registerName(b"setMainMenu:\0".as_ptr() as *const c_char);
            let msg: extern "C" fn(Id, Sel, Id) =
                std::mem::transmute(objc_msgSend as *const ());
            msg(app, sel, menu)
        }
    }
}

type IncrementFn = unsafe extern "C" fn(i32) -> i32;

unsafe fn dlerror_str() -> String {
    let p = libc::dlerror();
    if p.is_null() {
        "(no dlerror)".to_string()
    } else {
        CStr::from_ptr(p).to_string_lossy().into_owned()
    }
}

/// Optional Cocoa pre-init: when env `NSAPP` is set, do progressively more
/// of a Cocoa main-thread setup before the LabVIEW framework is loaded.
/// Lets us A/B test whether each Cocoa step changes the behaviour of
/// LabVIEW root-loop / UI-thread primitives. Levels (each adds to the
/// previous):
///
/// * `NSAPP=load`      → `NSApplicationLoad()`
/// * `NSAPP=launching` → `+ sharedApplication + setActivationPolicy(Regular) + finishLaunching`
/// * `NSAPP=activate`  → `+ activateIgnoringOtherApps(true)`
/// * `NSAPP=pump`      → `+ runUntilDate(now + 100ms)` (drains main-thread runloop)
/// * `NSAPP=run`       → spawn a worker for the framework calls and enter
///                       `[NSApp run]` on the main thread (handled separately
///                       in `main`, not in `maybe_init_nsapp`).
#[cfg(target_os = "macos")]
fn maybe_init_nsapp() {
    let level = std::env::var("NSAPP").unwrap_or_default();
    if level.is_empty() {
        return;
    }
    let ok = cocoa::nsapplication_load();
    println!("NSApplicationLoad() -> {ok}");

    if level == "load" {
        return;
    }

    let app = cocoa::shared_application();
    println!("sharedApplication -> {app:?}");
    if app.is_null() {
        eprintln!("sharedApplication returned nil; aborting Cocoa init");
        return;
    }

    // 0 == NSApplicationActivationPolicyRegular
    cocoa::set_activation_policy(app, 0);
    println!("setActivationPolicy(Regular) returned");

    cocoa::finish_launching(app);
    println!("finishLaunching returned");

    // Orthogonal: NSMENU=1 attaches an empty NSMenu as the app's main
    // menu. Targets the BaseMenu::GetCommandItem null-this crash that
    // lldb identified during framework init.
    if std::env::var_os("NSMENU").is_some() {
        let menu = cocoa::nsmenu_alloc_init();
        cocoa::set_main_menu(app, menu);
        println!("setMainMenu(empty NSMenu @ {menu:?}) returned");
    }

    if level == "launching" {
        return;
    }

    cocoa::activate_ignoring_other_apps(app, true);
    println!("activateIgnoringOtherApps(true) returned");

    if level == "activate" {
        return;
    }

    let runloop = cocoa::current_run_loop();
    let date = cocoa::date_with_interval_since_now(0.1);
    cocoa::run_until_date(runloop, date);
    println!("runUntilDate(now + 100ms) returned");
}

#[cfg(not(target_os = "macos"))]
fn maybe_init_nsapp() {}

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

    // NSAPP=run takes a completely different shape: spawn a worker for the
    // framework calls and enter NSApp's event loop on the main thread.
    #[cfg(target_os = "macos")]
    if std::env::var("NSAPP").as_deref() == Ok("run") {
        return run_with_nsapp_main_loop(framework_path, workaround);
    }

    maybe_init_nsapp();

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

/// `NSAPP=run` path: do the full Cocoa pre-init, spawn a worker thread that
/// runs the normal framework load + calls (and then `_exit`s the process
/// when done), and enter `[NSApp run]` on the main thread to keep the
/// event loop pumping. This tests the hypothesis that LabVIEW's root-loop
/// primitives need an active main-thread event loop to dispatch into.
#[cfg(target_os = "macos")]
fn run_with_nsapp_main_loop(framework_path: PathBuf, workaround: String) -> ExitCode {
    let ok = cocoa::nsapplication_load();
    println!("NSApplicationLoad() -> {ok}");
    let app = cocoa::shared_application();
    println!("sharedApplication -> {app:?}");
    if app.is_null() {
        eprintln!("sharedApplication returned nil; aborting");
        return ExitCode::from(1);
    }
    cocoa::set_activation_policy(app, 0);
    println!("setActivationPolicy(Regular) returned");
    cocoa::finish_launching(app);
    println!("finishLaunching returned");
    if std::env::var_os("NSMENU").is_some() {
        let menu = cocoa::nsmenu_alloc_init();
        cocoa::set_main_menu(app, menu);
        println!("setMainMenu(empty NSMenu @ {menu:?}) returned");
    }
    cocoa::activate_ignoring_other_apps(app, true);
    println!("activateIgnoringOtherApps(true) returned");

    println!("main: spawning worker for framework load + calls");
    std::thread::spawn(move || {
        let ok = run_with_libloading(&framework_path, &workaround);
        println!("worker: run_with_libloading returned ok={ok}");
        // Main is blocked in NSApp.run, so we have to terminate explicitly.
        unsafe { libc::_exit(if ok { 0 } else { 1 }) };
    });

    println!("main: entering [NSApp run] (blocks until _exit)");
    cocoa::nsapp_run(app);

    // Unreachable in practice — NSApp.run only returns via [NSApp stop:].
    eprintln!("[NSApp run] returned unexpectedly");
    ExitCode::from(2)
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

    // If the loaded framework also exports open_app_ref / close_app_ref
    // (lv_build.framework does; SharedLib.framework currently doesn't),
    // exercise them in isolation so we can characterise heavier LV-runtime
    // FFI behaviour from a minimal host. Probes before AND after each call
    // so we can tell whether the call returned at all.
    type OpenAppRefFn = unsafe extern "C" fn(u16, i32) -> u32;
    type CloseAppRefFn = unsafe extern "C" fn(u32);
    let app_ref_fns: Option<(OpenAppRefFn, CloseAppRefFn)> = unsafe {
        match (
            library.get::<OpenAppRefFn>(b"open_app_ref"),
            library.get::<CloseAppRefFn>(b"close_app_ref"),
        ) {
            (Ok(o), Ok(c)) => Some((*o, *c)),
            _ => None,
        }
    };
    if let Some((open, close)) = app_ref_fns {
        let port: u16 = 3364;
        // LabVIEW's default timeout for open_application_reference is 60s.
        let timeout_ms: i32 = 60_000;
        let on_worker = std::env::var("CALL_THREAD")
            .map(|v| v == "worker")
            .unwrap_or(false);

        let refnum = if on_worker {
            println!(
                "about to call open_app_ref({port}, {timeout_ms}) FROM WORKER THREAD"
            );
            let handle = std::thread::spawn(move || unsafe { open(port, timeout_ms) });
            let r = handle.join().expect("worker thread panicked");
            println!("open_app_ref returned (worker): refnum={r}");
            r
        } else {
            println!("about to call open_app_ref({port}, {timeout_ms})");
            let r = unsafe { open(port, timeout_ms) };
            println!("open_app_ref returned: refnum={r}");
            r
        };

        println!("about to call close_app_ref({refnum})");
        unsafe { close(refnum) };
        println!("close_app_ref returned");
    } else {
        println!("(open_app_ref / close_app_ref not exported by this framework — skipping)");
    }

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
