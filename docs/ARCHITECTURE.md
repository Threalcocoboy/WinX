# WinX architecture

| Component | Responsibility |
| --- | --- |
| `src/main.rs`, `src/panel.rs` | Tao event loop, system tray, native windows, Wry and WebView2 setup, and IPC dispatch. |
| `ui/src/popover/`, `ui/bridge.js` | Embedded panel, Settings, onboarding, and the native bridge. |
| `src/transport_windows.rs`, `src/protocol.rs`, `src/device.rs` | WinSock Bluetooth Classic RFCOMM transport, validated framing and controls, capability discovery, and confirmed device state. |
| `src/awareness_windows.rs`, `src/media_*.rs`, `src/overlay_*.rs` | Connection monitoring, battery notifications, hardware media confirmation, and the native overlay. |
| `src/talk*.rs`, `src/preferences.rs` | Talk Mode transitions, Bluetooth and Core Audio recovery records, and local preferences. |
| `src/winx_update.rs`, `src/update_ui_windows.rs` | Scoped GitHub HTTPS requests, SemVer, cache and settings, streamed download validation, and the UI state machine. |
| `src/update-install.ps1`, `packaging/windows.nsi` | Graceful process handoff, interactive per-user installation, executable version verification, and startup acknowledgement. |

The Bluetooth worker owns the command socket. Settings commands and confirmed device changes meet in the native event loop. Native update requests and downloads run off that event loop and send progress or results back by message. The web UI receives only native-generated state and sends explicit commands. The Rust download path holds the trusted asset selection. The PowerShell handoff runs only after the event loop has confirmed a safe Talk Mode state and saved the profile.

Settings and update state live under `%LOCALAPPDATA%\NothingX Desktop` for profile continuity. The corresponding-source ZIP includes the build and package scripts. Conditional macOS code remains in the inherited source tree.
