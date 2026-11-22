# WinX 1.1.0 Windows validation

## Baseline and architecture

WinX starts from the complete NothingX Desktop [v1.1 tag](https://github.com/nipunyatawara-dev/NothingX-Desktop/tree/v1.1)
at `671a69e990213efa3c4699b07edae9845d5ad2fb`. Its Windows transport and
packaging descend from [v1.0](https://github.com/nipunyatawara-dev/NothingX-Desktop/tree/v1.0)
at `d45d249d9915317792561a536c0946001655efd3`. The audit also compared
[Nothing X Mac](https://github.com/yashraut362/nothing-x-mac), including its
Tauri IPC and Swift RFCOMM bridge, and the
[Radiance ear-web](https://github.com/radiance-project/ear-web) framing and device
references. The Windows build retains the upstream v1.1 Tao/Wry shell, WebView2
renderer, Rust protocol and model state, assets, and conditional macOS bridge.
See [THIRD_PARTY.md](../THIRD_PARTY.md) for attribution and notices.

The Windows worker enumerates connected, paired candidates and opens a WinSock
Bluetooth Classic RFCOMM socket for service
`AEAC4A03-DFF5-498F-843A-34487CF133EB`. Windows resolves the channel through
SDP. Socket events wake the worker for reads, writes and shutdown. The worker
posts connection state and received bytes to Tao. Rust handles stream framing,
capabilities, pending commands and confirmation. The existing panel receives
JSON snapshots through local WebView IPC. Reconnection retries the control
channel with a 3–30 second backoff. The app leaves Windows pairing and audio
routing under the operating system's control.

The Windows shell adds the Open, Start with Windows, About, and Quit tray
commands, per-user startup, work-area and DPI-aware placement, and the WinX
product name. In-app updates verify the installer SHA-256 from the upstream
release, wait for the old process to exit, run the per-user installer and reopen
the installed application. The original executable and registry identity allow
the package to use a registered v1.0 install directory.

## Feature coverage

| Area | Coverage |
| --- | --- |
| Battery | Automated tests cover percentages, missing values, charging flags and truncated packets. |
| CMF Buds Pro 2 | The upstream recorded-session fixture exercises payload CRC, no-CRC notifications, battery, ANC, EQ, bass, latency, in-ear detection and gestures. Live Windows device coverage is pending. |
| ANC, EQ, bass and settings | The v1.1 model profiles, codecs and command gating are present. Automated cases cover readback, confirmation, timeout, and disconnect clearing. |
| Gestures, multipoint and Find | Retained v1.1 paths and model gating. The ring safety timeout and multipoint connection guards have automated coverage. |
| Unlisted models | Tests cover probing, unsupported responses, fallback and gating of feature writes. |
| Updates | Release, filename, checksum, and updater UI states have automated coverage. A live newer upstream Windows release was unavailable for a full in-app upgrade. |
| WinX Windows shell | Work-area placement is tested with simulated taskbars at each edge and 100%, 125%, 150%, 175%, and 200% scales. |
| Windows hardware | The user reported the Nothing Ear (3) worked correctly. The automated runs did not independently certify its control paths or establish live results for other models. |

## Build and verification commands

The target is `x86_64-pc-windows-msvc`. Build host Windows 11 25H2, build
26200, with Rust 1.95.0, Visual C++ Build Tools 14.44.35207, Windows SDK
10.0.26100.4188, NSIS 3.11, and WebView2 Evergreen.

```powershell
cargo test --locked --target x86_64-pc-windows-msvc
cargo fmt --check
cargo clippy --locked --target x86_64-pc-windows-msvc --all-targets -- -D warnings
node scripts/check-ui.cjs
.\scripts\build-windows.ps1
python scripts/smoke-windows.py
```

Outputs in `dist/` include `WinX-Windows-Setup-x64.exe`,
`WinX-Windows-Portable-x64.zip`, `WinX-1.1.0-source.zip` and
`SHA256SUMS.txt`. The release binaries are unsigned. The application and
WebView2 loader are native x64 binaries. Setup uses NSIS's 32-bit Unicode
launcher to install the x64 payload and checks for 64-bit Windows before
installation. `WinX.ico` is supplied separately and embedded in both executable
resources.

On this host, the 37 Rust tests, formatting check, strict Clippy, and Node
renderer checks passed. NSIS setup installed WinX 1.1.0 in a temporary
per-user directory, registered it in Windows and created a Start menu
shortcut. The installed app started and exited cleanly. The uninstall
removed the program files, registration and test shortcut.

The portable ZIP was extracted to a temporary directory. Both smoke cycles
passed startup, reopening the existing instance, opening and closing the
popover, and timed exit. A duplicate `--startup` launch stayed silent. The
Windows updater handoff script also passed PowerShell parsing. Build-time
resource compiler lookup and the NSIS launcher needed explicit paths in this
workspace's toolchain. The direct x64 build and packaged installer subsequently
passed.

Live taskbar menu interaction, the startup registry toggle across a Windows
sign-in, light and dark visual checks, Explorer restart, real mixed-DPI
displays, and the WebView2 bootstrapper on a machine without WebView2 remain
unverified. The desktop inspection tool could not select the tray popover
as a targetable window. Windows 10 runtime testing remains open.

## Remaining real-device checks

1. Pair the test model in Windows Bluetooth settings and select it as the audio
   output. Close other companion apps that may occupy the RFCOMM service.
2. Open WinX. Check live left, right, case, and charging readings against the
   headset or its official companion. Some models report the case only while
   it is open. A missing measurement should appear as unavailable.
3. Play audio while switching ANC or transparency and make a small EQ or bass
   change. Confirm the reported values after closing and reopening WinX.
   Restore the original values after the test.
4. Exercise the model's supported gestures, quick settings and multipoint.
   Remove the earbuds before checking Find and verify both manual stop and
   the 30-second automatic stop.
5. Repeat after Bluetooth is switched off and on, after sleep and resume, and
   when the earbuds switch hosts. Verify that leaving WinX and reopening it
   preserves the existing audio connection.
6. On Windows 10 and any additional Windows 11 setup, check the tray at the
   installed display scales and monitor positions. Test a Windows account
   with WebView2 already installed and a clean account where setup must install it.

For an optional read-only, repeatable live battery check, run
`python scripts/smoke-windows.py --require-battery`. To record WinSock error
codes during investigation, run the executable with
`WINX_BLUETOOTH_DIAGNOSTICS=1` and redirect standard error to a local file.
