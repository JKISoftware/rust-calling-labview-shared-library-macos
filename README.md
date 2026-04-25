# rust-calling-labview-shared-library-macos

Minimal reproducer demonstrating that a non-LabVIEW host process which loads
a LabVIEW-built `.framework` on macOS exits with a fatal `SIGSEGV` during
process teardown, even though the framework itself loads cleanly and its
exports run correctly.

The same Rust code (loading via `libloading`, then calling a single
`Increment(int32_t) -> int32_t` export) runs cleanly to completion when
targeting the equivalent LabVIEW-built `.dll` on Windows or `.so` on Linux.

## Prerequisites

- macOS (Apple Silicon or Intel)
- LabVIEW 2025 or 2026 — both have been observed to reproduce the crash
- Rust toolchain (any recent stable; `edition = "2021"`)

## Build & run

1. Open `shared-library.lvproj` in LabVIEW.
2. Right-click the **Shared Library** build spec under **Build Specifications**
   and choose **Build**. This produces
   `target/labview/SharedLib.framework/Versions/A/SharedLib` as a universal
   (arm64 + x86_64) Mach-O bundle.
3. From a terminal at the repo root:

   ```
   cargo run
   ```

## What we observe

Running `cargo run` on macOS produces:

```
OS:           macos (aarch64)
Framework:    .../target/labview/SharedLib.framework/Versions/A/SharedLib
Loaded OK (libloading)

LabVIEW caught fatal signal
26.1f0 - Received SIGSEGV
Reason: invalid permissions for mapped object
Attempt to reference address: 0x8
Increment(0) = 1 (expected 1) [OK]
Increment(1) = 2 (expected 2) [OK]
Increment(5) = 6 (expected 6) [OK]
Increment(41) = 42 (expected 42) [OK]
Increment(-1) = 0 (expected 0) [OK]
```

Process exit code: `139` (`128 + SIGSEGV`).

What this tells us:

- `libloading::Library::new` (which calls `dlopen`) returns `Ok` — see
  "Loaded OK (libloading)". The framework's static initializers complete
  without crashing.
- The exported `Increment` call is invoked five times with a spread of
  inputs (zero, small positive, negative, larger positive); each call
  returns exactly `input + 1`. The function is genuinely executing — a
  no-op or garbage-return path could not produce this consistent
  `input + 1` mapping across five different inputs.
- The interleaved order between the LabVIEW crash banner and the
  `Increment` output lines is just stdio buffering; the calls did
  execute and return the correct answers.
- The fatal `SIGSEGV` at address `0x8` fires after all `Increment`
  calls return — specifically, after our `main` returns, during
  process-exit cleanup (`atexit` handlers / `__cxa_finalize`
  destructors / dyld terminator routines). The framework loads and
  runs correctly; only its teardown is broken on macOS. See
  "Workarounds investigated" below for the empirical narrowing.
- The same teardown crash reproduces with frameworks built from both
  LabVIEW 2025 and LabVIEW 2026.

This reproducer intentionally uses the smallest possible framework (one
VI, one export) to rule out project-specific causes. The same teardown
crash signature also reproduces against significantly larger production
LabVIEW-built frameworks loaded through the same Rust host.

## Probing other frameworks

Pass an absolute path to any LabVIEW-built framework's binary as the
first argument to override the default lookup:

```
cargo run -- /path/to/Some.framework/Versions/A/Some
```

If the chosen framework has no `Increment` export, the binary will
print a clean `dlsym failed: ...` error and then still exit with the
same teardown `SIGSEGV` — confirming that the crash is in the
framework's runtime teardown rather than in the call site.

## Workarounds investigated

The reproducer accepts an optional `WORKAROUND` environment variable to
exercise the candidate workarounds we have tried. The headline result
is that **only `_exit` actually silences the crash** on macOS.

| `WORKAROUND=` | What it does | Exit code |
|---|---|---|
| *(unset)* | Faithful default. Loads via `libloading`, lets `Library::drop` call `dlclose` at scope exit, then returns from `main` normally. | **139** (SIGSEGV) |
| `forget` | Same load path, but `std::mem::forget(library)` is called so `Library::drop` never runs and `dlclose` is never called. | **139** (SIGSEGV) |
| `nodelete` | Loads via raw `libc::dlopen` with `RTLD_NOW \| RTLD_NODELETE`, calls `dlclose` explicitly (which is a no-op for unload purposes with this flag), then returns from `main`. | **139** (SIGSEGV) |
| `exit` | Same default load path, but instead of returning from `main` the binary calls `libc::_exit(0)` to terminate the process directly, bypassing `atexit` handlers, C++ static destructors, and dyld terminator routines. | **0** (clean) |

Run any one of:

```
WORKAROUND=forget   cargo run
WORKAROUND=nodelete cargo run
WORKAROUND=exit     cargo run
```

Or run all four modes in sequence with the bundled demo script:

```
./demo.sh
```

Sample output from one such run (absolute paths abbreviated):

```
========================================================================
=== default — no workaround (expect SIGSEGV, exit 139)
========================================================================
OS:           macos (aarch64)
Framework:    .../target/labview/SharedLib.framework/Versions/A/SharedLib
Loaded OK (libloading)

LabVIEW caught fatal signal
26.1f0 - Received SIGSEGV
Reason: invalid permissions for mapped object
Attempt to reference address: 0x8
Increment(0) = 1 (expected 1) [OK]
Increment(1) = 2 (expected 2) [OK]
Increment(5) = 6 (expected 6) [OK]
Increment(41) = 42 (expected 42) [OK]
Increment(-1) = 0 (expected 0) [OK]
./demo.sh: line 13: 11797 Segmentation fault: 11  "$bin"
EXIT CODE: 139

========================================================================
=== WORKAROUND=forget — skip Library::drop dlclose (expect 139)
========================================================================
OS:           macos (aarch64)
Framework:    .../target/labview/SharedLib.framework/Versions/A/SharedLib
Workaround:   forget
Loaded OK (libloading)

LabVIEW caught fatal signal
26.1f0 - Received SIGSEGV
Reason: invalid permissions for mapped object
Attempt to reference address: 0x8
Increment(0) = 1 (expected 1) [OK]
Increment(1) = 2 (expected 2) [OK]
Increment(5) = 6 (expected 6) [OK]
Increment(41) = 42 (expected 42) [OK]
Increment(-1) = 0 (expected 0) [OK]
std::mem::forget(library) — skipping Library::drop
./demo.sh: line 13: 11808 Segmentation fault: 11  WORKAROUND="$mode" "$bin"
EXIT CODE: 139

========================================================================
=== WORKAROUND=nodelete — RTLD_NODELETE (expect 139)
========================================================================
OS:           macos (aarch64)
Framework:    .../target/labview/SharedLib.framework/Versions/A/SharedLib
Workaround:   nodelete
dlopen flags: RTLD_NOW | RTLD_NODELETE (0x82)
Loaded OK (libc::dlopen)

LabVIEW caught fatal signal
26.1f0 - Received SIGSEGV
Reason: invalid permissions for mapped object
Attempt to reference address: 0x8
Increment(0) = 1 (expected 1) [OK]
Increment(1) = 2 (expected 2) [OK]
Increment(5) = 6 (expected 6) [OK]
Increment(41) = 42 (expected 42) [OK]
Increment(-1) = 0 (expected 0) [OK]
dlclose returned
./demo.sh: line 13: 11812 Segmentation fault: 11  WORKAROUND="$mode" "$bin"
EXIT CODE: 139

========================================================================
=== WORKAROUND=exit — libc::_exit, skip destructors (expect 0)
========================================================================
OS:           macos (aarch64)
Framework:    .../target/labview/SharedLib.framework/Versions/A/SharedLib
Workaround:   exit
Loaded OK (libloading)

LabVIEW caught fatal signal
26.1f0 - Received SIGSEGV
Reason: invalid permissions for mapped object
Attempt to reference address: 0x8
Increment(0) = 1 (expected 1) [OK]
Increment(1) = 2 (expected 2) [OK]
Increment(5) = 6 (expected 6) [OK]
Increment(41) = 42 (expected 42) [OK]
Increment(-1) = 0 (expected 0) [OK]
Exiting via libc::_exit(0)
EXIT CODE: 0
```

The fact that `forget` and `nodelete` both still produce SIGSEGV 139
and `exit` produces a clean 0 localises the crash precisely: it lives
in the post-`dlclose` process-exit cleanup path that runs `atexit`
handlers and shared-library destructors. Suppressing or short-circuiting
`dlclose` is not enough; only skipping the destructor chain entirely
prevents the crash.

## Probable cause

The behaviour is consistent with the framework's process-exit cleanup
chain (`atexit` handlers and `__cxa_finalize` destructors registered
by the framework's static initializers) racing with LabVIEW runtime
threads that the framework spawned at `dlopen` time and that are still
live when `main` returns.

The Rust documentation for `std::process::exit` is explicit about the
underlying contract:

> "As of C23, the C standard does not permit multiple threads to call
> `exit` concurrently. Rust requires that all exit handlers are safe
> to execute at any time. In particular, if an exit handler cleans up
> some state that might be concurrently accessed by other threads, it
> is required that the exit handler performs suitable synchronization
> with those threads."
>
> — [`std::process::exit` docs](https://doc.rust-lang.org/nightly/std/process/fn.exit.html)

Both Rust returning from `main` and macOS dyld running registered
destructors on shared-library teardown are doing what the C standard
and Apple's loader specify. The break in the contract is on the
framework side: its destructors touch state that its own runtime
threads are still mutating, with no synchronization between the two.
The early "LabVIEW caught fatal signal" banner that prints between
the load-success message and the first `Increment` output is itself
a side effect of the same framework-internal threading behaviour —
one of the runtime threads catches an internal signal which the
framework's signal handler swallows, but the destructor race on exit
is not catchable.

The Windows `.dll` and Linux `.so` produced by the equivalent LabVIEW
build specs from the same project load and run cleanly from
non-LabVIEW host processes through process exit, which indicates that
the macOS `.framework` build is the platform-specific outlier rather
than a fundamental constraint of dlopen-loading a LabVIEW shared
library.

For background on why `dlclose` and shared-library destructors are
particularly difficult on macOS in the presence of TLS and live
threads, see
[rust-lang/rust#47974 (comment)](https://github.com/rust-lang/rust/issues/47974#issuecomment-1255726678),
which notes: *"the fact that `dlclose` works anywhere is kind of a
bug, it failing to unload is actually macOS doing the right thing …
`dlclose` is just not really coherent in programs which have thread
local storage."*

### Possible fixes (framework-side)

The fix needs to live inside the LabVIEW framework. Plausible shapes,
in decreasing order of robustness:

1. **Make the destructors thread-safe** — have each destructor first
   quiesce or join the LabVIEW runtime threads it shares state with,
   before tearing that state down.
2. **Don't register destructors that touch live runtime state** — let
   the OS reap the process. The Windows and Linux builds appear to
   take roughly this approach.
3. **Stop registering `atexit` handlers from the framework on macOS**
   entirely.

### Host-side mitigation (not a fix)

`libc::_exit(0)` (the `WORKAROUND=exit` mode above) silences the
SIGSEGV by skipping the entire destructor chain. It is acceptable for
short-lived CLI hosts that have nothing else needing clean atexit
teardown, but it is not a general fix — any other library or host
code that legitimately needs `atexit` handlers to run will lose them
under this workaround.
