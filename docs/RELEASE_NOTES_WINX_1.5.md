# WinX 1.5.0 · Controls and Awareness

WinX 1.5 adds a native Windows overlay, customizable global shortcuts, and
intelligent battery alerts. It builds on the WinX code in this workspace and
the upstream NothingX Desktop v1.1 tag. The upstream tag list contained
v1.0 and v1.1 at build time.

See [Talk Mode, onboarding, and media controls](WINX-TALK-MODE.md) for the latest WinX 1.5 update and hardware validation steps.

## Settings

- **Controls → Keyboard shortcuts** can enable, record, clear, and reset
  shortcuts. The default shortcuts are Ctrl+Alt+X for the panel, Ctrl+Alt+N
  to cycle noise control, Ctrl+Alt+T for transparency, and Ctrl+Alt+G for low
  latency. Find shortcuts are unassigned. Remove the earbuds before using
  any Find shortcut.
- **Notifications → Overlay** controls Connected and Disconnected popups,
  which are enabled by default, and the visual settings shared by all
  overlays. The position selector previews a change. The default is the
  bottom right of the foreground monitor, normal size, two seconds, and
  Windows appearance.
- **Notifications → Media Controls** has separate Play/Pause, track, and
  volume switches. All three default on when the profile has no saved media
  setting. The original saved switch is carried over to all three controls.
- **Notifications → Battery alerts** controls the 20%, 10% and 5% defaults,
  components, quiet states, combining, critical sound, and alert duration.
  Preview actions send no device commands and leave the battery history intact.

## Implementation

The existing WinSock RFCOMM reader continues to own device I/O and the upstream
decoders continue to validate and confirm device state. WinX observes the
connected-device list and Bluetooth adapter identities separately from the
control socket. The first valid observation after launch, radio changes or
resume sets a baseline. Genuine absence requires two samples spanning two
seconds. A connection requires a successful compatible protocol response
and waits up to 900 ms for the initial battery reading. A complete response
can show immediately. The overlay omits absent components.
Connection cards use the Windows device name, an accent-colored success mark,
and the WinX earbud or headphones glyph. The action catalog assigns distinct
artwork to ANC modes, Play and Pause, track directions, volume and mute,
battery severity, bass, low latency, EQ, and locating requests. It reuses
existing WinX glyphs and draws new marks as vectors.

WinX uses one native topmost layered window with a premultiplied GDI surface
and work-area positioning. The surface reads the panel's CSS colors,
typography, radius, spacing, and animation timing. It composites the panel
fill over the native acrylic tint. The layered window uses that tint and
transparency without the desktop blur of the main panel's acrylic backdrop.
Successive icons and labels crossfade in a fixed layout. The window does not
activate and its hit test is transparent. The manager replaces normal
messages with newer messages and protects high and critical battery
messages from connection notices. Queued connection notices expire and
are invalidated when the corresponding link state changes.

Settings and battery history are local to the WinX profile. Battery alerts
use fresh protocol reports, persist the last warned threshold across
sessions, and rearm after meaningful recovery. WinX uses Windows' shell
notification state and foreground full-screen detection where available.
Windows does not expose every modern Do Not Disturb configuration to
classic desktop applications through this API.

Global shortcuts use Windows registration with no keyboard hook. Earbud
setting overlays reflect confirmed state, suppress synchronization and
repeat packets, and do not claim a physical gesture when the protocol
only reports its result.

### Media source identification

WinX registers only for background Raw Input Consumer Control reports. It
requires a device handle and matching Plug and Play ancestry. Supported
sources are a Bluetooth HID collection whose Bluetooth address matches the
active compatible Nothing/CMF headset, or a removable USB HID collection
beneath a physical USB host controller. Ordinary keyboard input, injected
messages and unidentified or virtual sources are excluded. WinX requires a
live Windows Bluetooth link and verified protocol session both when it
accepts a report and when it offers a media overlay.

The HID usage supplies the command and direction. WinX checks a fresh
pre-command baseline, the same media session or audio endpoint, and the
corresponding outcome. Each command can confirm once and expires after 1.1
seconds. Disconnect, suspend/resume, and input topology changes discard
pending actions. WinX does not repost input or issue a second media command.

Windows Media Control identifies the media application and state without
identifying the action's source. Windows can process Bluetooth AVRCP without
exposing an identifiable HID report. Those controls stay silent even with a
compatible pair. Software controls, autoplay, natural track changes, system
volume sliders, and resume-driven state changes cannot trigger an overlay
on their own. Windows does not expose a shared transaction ID between HID
input and the resulting media state. If a genuine hardware command and an
independent action change the same session in the same direction during the
confirmation window, Windows cannot distinguish that coincidence.

Windows references: [Raw Input](https://learn.microsoft.com/en-us/windows/win32/inputdev/about-raw-input),
[SetupAPI device ownership](https://learn.microsoft.com/en-us/windows/win32/api/setupapi/nf-setupapi-setupdigetdeviceinterfacedetailw),
[HID button usages](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/hidpi/nf-hidpi-hidp_getusages),
[media session API](https://learn.microsoft.com/en-us/uwp/api/windows.media.control.globalsystemmediatransportcontrolssession),
and [Bluetooth audio controls](https://learn.microsoft.com/en-us/windows-hardware/design/accessory-guidelines/bluetooth-accessory-guidelines/bluetooth-accessory-guidelines-classic-audio).

## Validation and manual checks

Windows x64 build commands from a Visual Studio Developer PowerShell:

    cargo test --locked --target x86_64-pc-windows-msvc
    cargo fmt --check
    cargo clippy --locked --all-targets --target x86_64-pc-windows-msvc -- -D warnings
    node scripts/check-ui.cjs
    node --check ui/src/popover/winx-settings.js
    .\scripts\build-windows.ps1

The automated lifecycle cases simulate native link observations, delayed and
missing battery readings, internal socket reconnection, Bluetooth adapter
changes, and sleep/resume. They check duplicate suppression and critical
battery priority. Native tests exercise a hidden window's styles and hit
testing and render GDI bitmap surfaces off-screen. Geometry tests cover six
positions, three sizes, negative multi-monitor coordinates, and common DPI
scales. Media tests cover unidentified state changes, source classification,
incompatible connections, session changes, expiry, duplicates, matching
outcomes, and preference migration. Icon tests check distinct, nonempty
artwork at five sizes. Off-screen surface tests check both themes, fixed
layout, and content interpolation.

On Windows 11 x64, 65 Rust tests passed. The Windows-specific tests covered
native shortcut registration, conflicts, reassignment and release, and
the hidden overlay window and GDI renderer. A read-only native test resolved
a present Raw Input HID interface to its SetupAPI device node. That check
used no button presses or media changes. Strict Clippy, rustfmt, the
existing JavaScript renderer tests and the new Settings script syntax
check passed. The restricted shell denied one test's replacement of an
existing settings file. The same replacement passed on Windows with the
terminal validation run in a workspace-local test directory.

The following checks require a person at the interactive Windows desktop:

1. Start WinX with Nothing or CMF earbuds already connected. No Connected
   popup should appear. Disconnect the earbuds through Windows and reconnect.
   Check the real device name, the available battery values, and exactly one
   popup per stable transition. Put the buds in their closed case to check
   disconnection when no WinX command is in progress.
2. Try a quick link fluctuation, change the Bluetooth adapter state, and
   suspend/resume Windows. Check that control-socket recovery and startup or
   resume baselines are silent. Repeat on Windows 10 and on a second model.
3. Open Settings → Notifications → Overlay and try the previews. Check all six positions,
   system/light/dark appearance, different monitors and scaling, and reduced
   motion. While an overlay is displayed, click through it in another
   application and confirm the foreground window keeps keyboard focus.
4. Change a shortcut while WinX is in the tray, then trigger it from another
   application. Verify an unavailable chord preserves the old binding and
   unsupported device actions issue no write. Trigger a supported ANC change
   and verify the overlay reflects its confirmed result.
5. Confirm the overlay settings and alert history survive a restart. Observe
   a low battery threshold on real hardware and verify that routine readings
   don't repeat it. Test the installer on a clean Windows user profile.
6. With compatible earbuds connected, try a removable USB HID media
   device. Exercise Play/Pause, skips, volume, and mute in an application
   supporting those commands. Check that clicking software controls, autoplay,
   and moving the Windows volume slider stay silent. Repeat after disconnecting
   the earbuds. Check the earbuds' buttons separately since AVRCP may remain silent.

Implementation files include `src/media.rs`, `src/media_input_windows.rs` and
`src/media_windows.rs` for command and outcome detection, `src/awareness*.rs`
and `src/preferences.rs` for lifecycle and settings, and
`src/overlay_*.rs` plus `ui/src/popover/` for rendering and previews.

These live hardware and desktop-interaction checks were not performed for
this build. No UI automation, simulated input, or Bluetooth control commands
were used to validate it. A previous WinX release's Ear (3) result does not
verify the WinX 1.5 overlay.

Local output names are `WinX-1.5.0-Setup-x64.exe`,
`WinX-1.5.0-Portable-x64.zip`, and `WinX-1.5.0-source.zip`.
Windows release files are unsigned. The installer and portable app retain
the WebView2 requirement. Nothing was published to GitHub.
