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
Loaded OK

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
  "Loaded OK". The framework's static initializers complete without
  crashing.
- The exported `Increment` call is invoked five times with a spread of
  inputs (zero, small positive, negative, larger positive); each call
  returns exactly `input + 1`. The function is genuinely executing — a
  no-op or garbage-return path could not produce this consistent
  `input + 1` mapping across five different inputs.
- The interleaved order between the LabVIEW crash banner and the
  `Increment` output lines is just stdio buffering; the calls did
  execute and return the correct answers.
- The fatal `SIGSEGV` at address `0x8` fires after all `Increment`
  calls return, during library unload (`dlclose`) or process exit.
  The framework loads and runs correctly; only its teardown is broken
  on macOS.
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
