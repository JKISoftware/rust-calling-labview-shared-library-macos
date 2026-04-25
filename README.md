# rust-calling-labview-shared-library-macos

Minimal reproducer demonstrating that on macOS, when a non-LabVIEW host
process loads a LabVIEW-built `.framework` and calls any LabVIEW runtime
function that uses the **root loop / UI thread** (e.g. `Open Application
Reference`), the LabVIEW runtime crashes the process with `SIGSEGV` at
address `0x8`. The framework loads cleanly, and lightweight VIs that
don't engage LabVIEW's UI thread (e.g. a simple `Increment` math VI)
execute correctly. Only paths that depend on `BaseMenu`-backed dispatch
crash.

The same code targeting the equivalent LabVIEW-built `.dll` on Windows
or `.so` on Linux runs end-to-end without these issues.

## What this reproducer does

`cargo run` does two things in order:

1. Calls `Increment(int32_t i) -> int32_t` against a spread of inputs
   (0, 1, 5, 41, -1) to confirm light FFI calls work.
2. Calls `open_app_ref(uint16_t port, int32_t timeout_ms) -> uint32_t`
   to attempt to open a LabVIEW application reference (then
   `close_app_ref` to release it). This is the call that crashes on
   macOS.

The host-side code is intentionally minimal: a single `src/main.rs`,
`libloading` for `dlopen`, no async runtime, no signal handlers, no
ctrlc handler — only what's needed to demonstrate the bug.

## Prerequisites

- macOS (Apple Silicon or Intel). Reproduced on macOS 15.x.
- LabVIEW 2025 or 2026 installed; for `open_app_ref` to have a real
  target, LabVIEW running and listening on the configured TCP port
  (3364 in the default `src/main.rs`).
- Rust toolchain (any recent stable, edition 2021).

## Build & run

1. Open `shared-library.lvproj` in LabVIEW.
2. Right-click the **Shared Library** build spec under **Build
   Specifications** and choose **Build**. This produces
   `target/labview/SharedLib.framework/Versions/A/SharedLib` as a
   universal (arm64 + x86_64) Mach-O bundle.
3. From a terminal at the repo root:

   ```
   cargo run
   ```

## Headline result

```
OS:           macos (aarch64)
Framework:    .../target/labview/SharedLib.framework/Versions/A/SharedLib
Loaded OK (libloading)

LabVIEW caught fatal signal
26.1f0 - Received SIGSEGV
Reason: invalid permissions for mapped object
Attempt to reference address: 0x8
[DLL] increment(i: 0)
[DLL] i++: 1
Increment(0) = 1 (expected 1) [OK]
... (5 increment calls, all returning input + 1)
about to call open_app_ref(3364, 60000)
[DLL] open_app_ref(port: 3364, timeout_ms = 60000)
[DLL] Open Application Reference (before)
zsh: segmentation fault  cargo run
```

Process exit: signal 11 (SIGSEGV), exit status `139`.

What this reveals:

- `dlopen` succeeds — `Library::new` returns Ok ("Loaded OK
  (libloading)").
- Almost immediately after load, the LabVIEW runtime emits the
  "LabVIEW caught fatal signal" banner. Despite this, the process
  keeps running — LabVIEW catches and swallows the signal internally.
- 5 sequential `Increment(i)` calls return correctly with `i + 1`.
  Light VIs that don't touch LabVIEW's UI thread or root loop work
  end-to-end.
- The first `open_app_ref` call enters the LabVIEW DLL wrapper
  successfully (instrumented `[DLL] open_app_ref(...)` print)
  and reaches the call to LabVIEW's "Open Application Reference"
  primitive (instrumented `[DLL] Open Application Reference (before)`
  print). The matching `(after)` print never appears — process killed.

## lldb investigation

Running under `lldb` to capture the (otherwise swallowed) signal:

```
WORKAROUND=exit lldb -b -s lldb-cmds.txt -- ./target/debug/rust-calling-labview-shared-library-macos
```

with `lldb-cmds.txt`:

```
settings set target.inherit-env true
process launch
bt
register read
bt all
quit
```

lldb stops on the first signal — which is the swallowed one fired
during framework startup, *before* our `open_app_ref` call:

```
* thread #5, stop reason = EXC_BAD_ACCESS (code=1, address=0x8)
    frame #0: 0x...df0 LabVIEW 26.1 Runtime`BaseMenu::GetCommandItem(long) const
->  0x...df0 <+0>:  ldr    x0, [x0, #0x8]
    0x...df4 <+4>:  cbz    x0, 0x...e04           ; <+20>
```

Backtrace of the crashing thread (LabVIEW's own UI thread, running as
a pthread):

```
thread #5 (SysUIThread, a worker pthread spawned by LV)
  #0  BaseMenu::GetCommandItem(long) const          ← x0 = 0 (null `this`)
  #1  LVMainEventProc(WEvent*, long) + 888
  #2  WSendEvent + 848
  #3  WProcessAppEvents + 60
  #4  BGAppTask_MG::HandleTasks(int) + 76
  #5  BGAppTask_MacUI::CheckAndHandleBGEvent() + 48
  #6  BGAppTasksRunLoopObserverCallback(...)        ← CFRunLoop observer
  #7  __CFRUNLOOP_IS_CALLING_OUT_TO_AN_OBSERVER_CALLBACK_FUNCTION__
  #8  __CFRunLoopDoObservers
  #9  __CFRunLoopRun
  #10 _CFRunLoopRunSpecificWithOptions
  #11 MainLoop_MacCocoaUI::Run() + 268               ← LV's main UI loop
  #12 MGMain(int, char const* const*) + 212
  #13 SysUIThread(void*) + 20                        ← LV's "UI thread"
  #14 ThreadCoverProc(void*) + 168
  #15 _pthread_start + 136
```

Register state at the crash:

```
x0 = 0x0           ← null `this`
pc = ...df0        ← in BaseMenu::GetCommandItem
lr = ...bcf4       ← caller is LVMainEventProc + 888
```

Instruction at `pc` is `ldr x0, [x0, #0x8]` — load from `*(x0 + 8)`.
With `x0 = 0`, that's the dereference of address `0x8` reported in the
LV crash banner.

Meanwhile, the macOS main thread (#1) is in our Rust code, mid-`Increment`:

```
thread #1, queue = 'com.apple.main-thread'
  #4  SharedLib`StaticInitTermProcs::PrepLVCall(...)
  #5  SharedLib`CallVIFromDll(epIdx=0, ...)
  #6  SharedLib`Increment + 124
  #7  rust_calling_labview_shared_library_macos::call_increments(...)
  #8  rust_calling_labview_shared_library_macos::run_with_libloading(...)
  #9  rust_calling_labview_shared_library_macos::main
```

So the runtime model is:

- **macOS main thread**: runs our Rust code. Calls into the LabVIEW
  framework synchronously when we invoke an exported VI.
- **`SysUIThread`**: a separate pthread that LabVIEW spawns at
  framework load. Runs LabVIEW's `MainLoop_MacCocoaUI::Run`, which
  drives `_CFRunLoopRunSpecificWithOptions` on its own thread, with
  a `BGAppTasksRunLoopObserverCallback` observer that periodically
  walks into `WProcessAppEvents` → `LVMainEventProc` → `BaseMenu::GetCommandItem`.
- That observer callback dereferences `0x8` (null `BaseMenu`) on
  every tick. LabVIEW's signal handler swallows each `SIGSEGV` and
  the loop continues — *repeatedly*, throughout the entire run.

Note that `SysUIThread` is **not** the macOS main thread. Apple
documentation says the *main* `CFRunLoop` must run on the *main*
thread; LabVIEW does its own thing here, running an unrelated
`CFRunLoop` instance on a worker pthread.

## Falsified host-side workarounds

The reproducer accepts several environment variables that try
progressively more aggressive host-side fixes. **None of them change
the `BaseMenu` mid-call crash signature.** They do change what gets
printed before / around the load and exit, which is useful for
ruling things out.

### Cocoa pre-init: `NSAPP=...`

Each level adds to the previous:

```
NSAPP=load        cargo run    # NSApplicationLoad()
NSAPP=launching   cargo run    # + sharedApplication + setActivationPolicy(Regular) + finishLaunching
NSAPP=activate    cargo run    # + activateIgnoringOtherApps(true)
NSAPP=pump        cargo run    # + runUntilDate(now + 100ms)
NSAPP=run         cargo run    # spawn worker for framework calls; enter [NSApp run] on main
```

Every Cocoa call returns successfully (each step prints its return
value). The crash is identical at every level. Cocoa main-thread
setup doesn't affect LabVIEW's worker-thread `CFRunLoop`.

### Empty `NSMenu`: `NSMENU=1`

Sets `[NSApp setMainMenu:[[NSMenu alloc] init]]` after
`sharedApplication`. Combined with any `NSAPP=` level, no change.
Confirms LabVIEW's `BaseMenu` is independent of Cocoa's `NSMenu` —
they are two unrelated menu systems.

### Worker-thread call site: `CALL_THREAD=worker`

```
CALL_THREAD=worker WORKAROUND=exit cargo run
```

Calls `open_app_ref` from `std::thread::spawn`-ed worker rather than
the main thread. No change.

### Process-exit teardown workarounds: `WORKAROUND=...`

This was the original investigation — *before* we identified the
mid-call `BaseMenu` crash. With the `open_app_ref` calls present,
the process dies before reaching the exit path, so these workarounds
no longer manifest a visible difference. They are kept in the code
for documentation of what *was* tried and what's been ruled out.

| `WORKAROUND=` | What it does |
|---|---|
| *(unset)* | Faithful default. `Library::drop` calls `dlclose` at scope exit. |
| `forget`   | `std::mem::forget(library)` so `Library::drop` doesn't run. |
| `nodelete` | Raw `libc::dlopen(..., RTLD_NOW \| RTLD_NODELETE)`. |
| `exit`     | Replace normal return with `libc::_exit(0)`. |

The reproducer also has a *separate* (older) bug where, when the run
completes only light calls and reaches normal process exit, a
`SIGSEGV` fires in `atexit` / destructor cleanup. `WORKAROUND=exit`
silences that teardown crash by skipping the destructor chain via
`libc::_exit`. That bug is now masked by the earlier `BaseMenu`
mid-call crash, but `_exit` remains a valid mitigation if the
mid-call path is avoided.

### LabVIEW build settings (verified by the maintainer)

The LabVIEW shared-library build spec **"Execute VIs in private
execution system"** has been toggled both ways with no change in
behaviour. The crash is independent of that setting.

### Demo script

```
./demo.sh
```

Runs the four `WORKAROUND` modes in sequence. With the
`open_app_ref` calls present, all four exit 139.

## Probable cause

The crash is unambiguously inside LabVIEW's `BaseMenu::GetCommandItem`
with `this = nullptr`. The function loads a field at offset `0x8` of
`this`, which is null, dereferencing address `0x8`.

`BaseMenu` appears to be uninitialized in this process because:

- In `LabVIEW.app`, menu / command system setup runs as part of normal
  application startup, populating `BaseMenu`.
- When `lv_build.framework` (or any other LabVIEW-built framework) is
  loaded by `dlopen` into a non-LabVIEW host, that startup code
  doesn't run, and `BaseMenu` is left null.

Once the framework is loaded, `MainLoop_MacCocoaUI::Run` is started on
`SysUIThread` and immediately begins servicing `CFRunLoopObserver`
callbacks. Each observer callback eventually calls into
`BaseMenu::GetCommandItem(this=null)` and crashes. LabVIEW's signal
handler catches the resulting `EXC_BAD_ACCESS` and resumes the loop.
This is the source of the "LabVIEW caught fatal signal" banner the
host sees right after `Loaded OK`.

When the host calls a heavyweight LabVIEW runtime function that uses
the same dispatch path (`open_application_reference` and presumably
any other root-loop primitive), the same `BaseMenu` is null, the
crash fires inside the call, and somewhere along the recovery path
the handler's in-process state diverges enough that the signal is no
longer recoverable — the process is killed.

The Windows `.dll` and Linux `.so` produced by the equivalent LabVIEW
build specs from the same project load and run cleanly from
non-LabVIEW host processes, including `open_application_reference`
calls. The bug is specific to the macOS `.framework` form of the
LabVIEW shared library.

## Possible fixes (require a change in LabVIEW itself)

The fix needs to live inside the LabVIEW shared-library runtime
shipped by NI. No host-side change can substitute for this.
Plausible fix shapes:

1. **Initialize `BaseMenu` defensively at framework startup** — the
   menu system needs to be in a non-null, valid state when loaded
   into a non-LabVIEW host, even if no menus will ever be shown.
   This is the most surgical fix and likely matches what
   `LabVIEW.app`'s normal startup does today.
2. **Make `BaseMenu::GetCommandItem` (and other entry points) null-
   safe** — return a sentinel / no-op when `this` is null rather
   than dereferencing it.
3. **Don't start `MainLoop_MacCocoaUI::Run` unconditionally on
   framework load** — only spin up the UI thread / `CFRunLoop` if
   the host actually needs root-loop services, and arrange for the
   necessary main-thread coordination at that point.

## Host-side mitigation

There is **no host-side mitigation** for the `BaseMenu` /
`open_app_ref` mid-call crash. It happens inside LabVIEW's own
runtime on a thread the host doesn't control, and recovery would
require fixing LabVIEW's internal state.

`WORKAROUND=exit` (`libc::_exit`) remains a valid mitigation for
the *separate* process-teardown crash, for code paths that don't
trigger the mid-call crash first. See the `WORKAROUND` table above.

## Files

- `src/main.rs` — the reproducer host: `argv[1]` framework path
  override, env-variable-gated host-side experiments, `Increment` /
  `open_app_ref` / `close_app_ref` calls.
- `shared-library.lvproj` + `increment.vi` — minimal LabVIEW project
  that produces `SharedLib.framework`. Exports `Increment`,
  `open_app_ref`, `close_app_ref` (the latter two added so the
  reproducer can exercise the root-loop crash without depending on
  any larger LabVIEW project).
- `demo.sh` — runs the four `WORKAROUND` modes in sequence (legacy
  from the teardown-crash investigation).
- `target/labview/SharedLib.framework` — LabVIEW build output
  (gitignored; rebuild via the .lvproj).

## Probing other LabVIEW-built frameworks

Pass an absolute path to any LabVIEW-built framework's binary as the
first argument to override the default lookup:

```
cargo run -- /path/to/Some.framework/Versions/A/Some
```

If the chosen framework doesn't export one of `Increment`,
`open_app_ref`, or `close_app_ref`, the reproducer prints a clean
"not exported by this framework — skipping" message for that part
and continues. The same `BaseMenu`-backed crash signature reproduces
against significantly larger production LabVIEW-built frameworks
loaded this way.
