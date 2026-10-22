# A704F macOS CLI

This is a native Rust CLI for Apple Silicon Macs, with an optional Qt 6 Widgets frontend in `gui/`. The GUI invokes the CLI for all HID and macro operations; it does not implement a second protocol transport.

## Verified Protocol Boundary

The repository README identifies USB `18f8:1286` as the A704F and says configuration is not stored persistently by the mouse. `SPECS.md` names interface 0 for configuration and interface 1 for keyboard/button input. Its documented setting payloads are:

| Command | Payload bytes after command | Documented values |
| --- | --- | --- |
| `09` | active DPI, packed DPI/profile, enabled-profile mask, 3 zero bytes | Four profiles, active profile 0-3, DPI nibble 1-f |
| `10` | physical button, action, 4 zero bytes | Seven documented buttons; actions 00-0b |
| `11` | scroll mode, debug action, 4 zero bytes | Modes 00-02; debug actions only when mode 02 |
| `12` | 00, fire rate, 4 zero bytes | Fire-rate byte 00-ff |
| `13` | 7f, RGB mode, 4 zero bytes | Modes 10-17 |

`src/hidhelper.cpp` constructs a 16-byte buffer: report ID `21`, bytes `09 07 03 01 00 08 00`, command prefix `07`, command, then six payload bytes. It passes the buffer to `hid_send_feature_report`. The new encoder reproduces that source buffer exactly; byte order is therefore the literal order above. The source does not check write results, read back settings, or establish descriptor-level report sizes. The new transport selects interface 0 based on `SPECS.md`, but that interface and report framing have not been validated on macOS hardware.

The upstream README says MIT and the Arch package metadata says `MIT`, but no license file was present in this checkout. The root `LICENSE` records that stated license and attributes the original project to AmberFrost. Original reference: [AmberIsFrozen/Instant-A704F-Mouse-Utilities](https://github.com/AmberIsFrozen/Instant-A704F-Mouse-Utilities).

No firmware-stored macro support is claimed. The macro engine runs on the Mac while `macro run` is active.

## Build

Requirements: Rust stable, Xcode Command Line Tools, and macOS on Apple Silicon. Rust dependencies are fetched by Cargo; the native `hidapi` backend uses macOS IOKit. No Homebrew HID package, root access, kernel extension, or privileged daemon is required by this project.

```sh
cd macos
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo build --release
```

The native device and input runtime is macOS-only. Linux is supported for protocol, configuration, mock-transport, and macro-engine tests; `device inspect`, hardware writes, and event injection intentionally return a platform error there.

Rust is required for the backend CLI. CMake and Qt 6 are additionally required for the GUI; the GUI invokes the CLI rather than implementing HID itself. CLI-only users do not need CMake or Qt. The [macOS Apple Silicon workflow](../.github/workflows/macos-arm64.yml) builds both and uploads an unsigned `.app` artifact.

### Downloading the app from GitHub Actions

The Actions artifact download is a ZIP archive, not an `.app` file. Download the `a704f-configurator-macos-arm64` artifact from a successful workflow run and extract the ZIP with Finder. The extracted `a704f-configurator.app` is the app to open; do not rename the downloaded ZIP to `.app`. If it was already renamed, change its extension back to `.zip` and extract it.

This build is unsigned. If macOS warns that it cannot verify the developer, only if you trust the build, Control-click the extracted app, choose **Open**, then confirm **Open** in the dialog. The workflow currently builds Apple Silicon (`arm64`) only.

## Qt 6 GUI

The GUI requires Qt 6.2 or later, CMake 3.21 or later, Rust stable, and Xcode Command Line Tools. With Homebrew Qt installed:

```sh
brew install qt cmake
cd macos
cargo build --release
cmake -S gui -B gui/build -DCMAKE_PREFIX_PATH="$(brew --prefix qt)"
cmake --build gui/build --config Release
open gui/build/a704f-configurator.app
```

The CMake target bundles the release CLI when it exists at `macos/target/release/a704f-macos`. The GUI also allows selecting the CLI executable manually or setting `A704F_CLI_PATH`. To prepare a Qt deployment bundle, run `$(brew --prefix qt)/bin/macdeployqt gui/build/a704f-configurator.app` after building. The GUI supports read-only device inspection, settings import/export/preview, explicit-confirmation apply, and host macro import/export/validation/run/stop. Macro execution requires the same macOS permissions described below.

## Commands

```sh
cargo run -- --help
cargo run -- device list
cargo run -- device inspect
cargo run -- settings export --file settings.json
cargo run -- settings plan --file settings.json
cargo run -- settings apply --file settings.json
cargo run -- settings apply --file settings.json --write
cargo run -- macro export --file macro.json
cargo run -- macro validate --file macro.json
cargo run -- macro run --file macro.json
```

`device list` and `device inspect` are read-only. Inspection reports matching device IDs, interfaces, manufacturer/product strings, and HID usage values; it cannot report current settings because the reference protocol defines no readback command.

Settings `apply` is a dry run unless `--write` is supplied. The write path checks for exactly `18f8:1286`, validates the whole configuration, opens interface 0, and sends only the documented settings reports. It reports how many calls succeeded but cannot verify that firmware applied them. The known `30fa:1701` A704 is discoverable but not writable by this utility.

Settings JSON fields are `rgb_mode`, `scroll_mode`, `scroll_debug`, `fire_rate`, `active_dpi`, `dpi_profiles`, and `bindings`. `settings export` writes a valid example. Unknown fields, invalid enum ranges, duplicate/missing physical buttons, and malformed values are rejected.

Macro JSON is versioned and contains one `trigger` plus ordered `steps`. Triggers support a standard mouse button (`1` left, `2` right, `3` middle) or a hotkey. Step kinds are `key`, `mouse`, and `delay`; mouse actions are `left`, `right`, or `middle`. Key names use rdev identifiers such as `KeyA`, `KeyF`, `ControlLeft`, `ShiftLeft`, `MetaLeft`, `Space`, and `F1`-`F12`.

```json
{
  "version": 1,
  "trigger": { "type": "hotkey", "key": "KeyF", "modifiers": ["ControlLeft", "ShiftLeft"] },
  "steps": [
    { "type": "key", "key": "KeyA", "pressed": true },
    { "type": "delay", "milliseconds": 75 },
    { "type": "key", "key": "KeyA", "pressed": false }
  ]
}
```

Macros never run at launch. `macro run` starts monitoring only after an explicit command. F12 is the default stop-all key; use `--stop-key Escape` to change it. Ctrl+C also cancels. Held keys and buttons are released on normal completion, cancellation, or an input error. Only one macro runs at a time. The OS may deliver the trigger button to its normal target as well; this app does not intercept or suppress input.

## macOS Permissions

- **Accessibility:** required by the input library to observe global events and to post synthetic keyboard/mouse events. When launched from Terminal, macOS may attribute the request to Terminal; grant access only to the app/process you intentionally run.
- **Input Monitoring:** macOS may request this separately for global keyboard/mouse observation, depending on OS version and launch context. It is used only while `macro run` is active.
- **HID device access:** configuration needs access to the mouse's HID interface. This utility does not ask for administrator privileges, install a helper, alter system security settings, or use `sudo`. If access is denied, inspect macOS privacy prompts and the application signing/launch context.

Do not grant Full Disk Access. Stop the listener and revoke permissions in System Settings when no longer needed.

## WebHID

Chrome WebHID is a possible read/write path, but it is not implemented and is not a prerequisite. It requires a secure context (HTTPS; localhost is treated as secure), a user gesture for `navigator.hid.requestDevice()`, and a user permission choice. The page must use matching filters and then select a device/interface/report exposed by the browser. Browsers restrict protected keyboard/mouse HID collections, and macOS/browser behavior depends on the device's actual HID descriptors.

This repository does not include the mouse's report descriptor, so it cannot establish whether the configuration collection is exposed to WebHID or whether report ID `0x21` maps to the spec's 16-byte HIDAPI buffer in Chrome. Do not send the native HIDAPI frame through WebHID without descriptor inspection and hardware validation. The native CLI remains the fallback.

## Troubleshooting and Limits

- `device inspect` reports no matching interface: reconnect the mouse, check the USB ID in System Information, and retry without running as root.
- `macro run` receives no events: grant Accessibility and any requested Input Monitoring permission to the actual launching app, stop the command, and start it again.
- Synthetic input is rejected: grant Accessibility to the process that posts events. No permission bypass is attempted.
- Settings writes are not persistent and cannot be read back through the documented protocol; apply them again after reconnect/reboot if required.
- Extra side-button host triggers are not mapped because macOS reports extra mouse buttons with OS-specific codes. Use a hotkey or validate mapping locally before extending it.
- No rapid-fire, keyboard firmware output, multimedia output, or onboard macro feature is exposed by this CLI. The reference documents additional protocol commands, but this implementation intentionally limits writes to the verified settings subset.

## Verification Status

Verified in Codespaces: Rust build of platform-independent CLI/core, 11 unit tests for report encoding, setting validation, unsupported devices, explicit-write gating, JSON validation/round trips, macro ordering/timing/cancellation, and fake HID writes. No physical device is attached.

Requires a local Apple Silicon Mac and the actual `18f8:1286` mouse: macOS target compilation, HID interface selection, descriptor/report compatibility, successful settings writes, device behavior after each setting, privacy prompts, and live macro event timing/trigger behavior. No physical-device compatibility is claimed yet.