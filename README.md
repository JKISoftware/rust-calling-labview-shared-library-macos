# rust-calling-labview-shared-library-macos

Minimal reproducer demonstrating that loading a LabVIEW-built `.framework` from
a non-LabVIEW host process crashes on macOS.

The same Rust code (loading via `libloading`, then calling a single
`Increment(int32_t) -> int32_t` export) works correctly when targeting the
equivalent LabVIEW-built `.dll` on Windows and `.so` on Linux.

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

## Expected vs. observed

**Expected** output (and what we see when the equivalent `.dll` / `.so` is
loaded on Windows / Linux):

```
OS:           macos (arm64)
Framework:    .../target/labview/SharedLib.framework/Versions/A/SharedLib
Loaded OK
Increment(5) = 6
```

**Observed** on macOS, with both LabVIEW 2025 and 2026, on both arm64 and
x86_64 (under Rosetta) host processes:

```
OS:           macos (arm64)
Framework:    .../target/labview/SharedLib.framework/Versions/A/SharedLib
LabVIEW caught fatal signal
26.x ...
Received SIGSEGV
Attempt to reference address: 0x8
```

The process exits with code 139 (`128 + SIGSEGV`). The crash happens inside
`dlopen`, before `libloading::Library::new` returns — i.e. before any of this
reproducer's code after that line gets a chance to run.

## What we have already tried

These approaches were attempted and did **not** change the crash:

- Pre-init Cocoa from the host: `NSApplicationLoad`,
  `[NSApplication sharedApplication]`, setting an activation policy,
  attaching an empty main menu.
- Wrapping the host binary inside a `.app` bundle and launching it through
  the bundle.
- Exporting dummy `gLVRTVersion` and `EnableBackwardCompatibleLoad` symbols
  from the host binary.
- Toggling LabVIEW build settings on the shared library: compatibility-with-
  future-RTE, private execution system, delay OS messages.
- Rebuilding the shared library against LabVIEW 2025 vs. 2026 — the framework
  appears to load the same `NILVRuntimeManager` regardless.

The Windows `.dll` and Linux `.so` produced by the equivalent build specs
from the same `.lvproj` load and call cleanly from non-LabVIEW host
processes, so the host-side approach itself is sound on those platforms.
