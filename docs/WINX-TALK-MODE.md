# WinX Talk Mode, onboarding, and media controls

## Talk Mode

Use **Ctrl+Alt+H** or open **Settings → Talk Mode**. The shortcut is configurable in the existing global hotkey list. The default target is **5%** and the default fade is **600 ms**. Transparency, EQ reduction, feedback, and the shortcut can be configured separately. Settings from earlier releases are retained, including shortcut assignments that already occupy Ctrl+Alt+H.

On activation, WinX snapshots confirmed Bluetooth settings and the exact Core Audio scalar of the default playback output when Windows Plug and Play associates that output with the selected earbuds. WinX starts fading the original output while applying the supported speech EQ (bass minimum, mid maximum, treble minimum). It requests Transparency only after the fade and EQ stage have resolved. The EQ change requires readable original bands and preset, a known writable range, and a three-filter report that WinX can restore losslessly. It leaves bass enhancement and mute unchanged. Output selection uses the physical device container rather than matching a display name. If the default output belongs to another device, Windows cannot establish the mapping, or the earbud uses an unsupported EQ format, WinX skips that operation and describes the partial result.

On deactivation, WinX rereads the affected earbud settings and restores the original values after confirmation. It restores volume by reopening the exact endpoint ID, even if the default changed. WinX preserves values changed by the user after activation when it can detect the change. Additional presses are ignored while a transition is in progress. Closing normally starts restoration. If WinX or Windows terminates, local recovery files let WinX restore on its next launch. If a target is unavailable, WinX parks the original record and stops retrying. Retry Recovery in Settings checks saved targets again. Selecting the same endpoint for a new session first attempts to resolve its existing record. A different verified endpoint can start an independent session. Corrupt or inaccessible records may be archived only through explicit confirmation. The archived records are kept locally for inspection. Windows values can only be compared at the resolution exposed by the endpoint, and changes that return to an identical value between checks are indistinguishable.

## Onboarding

WinX opens the six-scene setup automatically for new installations. Existing installations can open **Settings → General → Replay Onboarding**. The tour uses the original Nothing X icon, geometric vector transitions, and the supplied WinX artwork. It uses Windows Bluetooth Settings for initial pairing, can select paired or previously connected compatible devices, displays reported battery values, and saves the device choice and preferences. The position selector and preview use the native overlay.

The Talk Mode demonstration plays the bundled demo track only after an explicit click. In the live demo, Core Audio fades the sound through the same Talk Mode service used by the shortcut. With no usable hardware, **Use a visual preview** simulates the interface and can play the same clip with its own gain. The preview never writes earbud settings or system volume. The chosen shortcut drives the keycaps and adapts to Ctrl, Alt, and Shift. The tour can be used with Windows reduced motion. WinX saves completion independently of other settings.

## Media-origin detection

WinX still requires a verified compatible Bluetooth link and a hardware command before checking the resulting playback, track or output change. Windows Raw Input can identify an eligible consumer-control device by its handle. The WinX classifier uses a matching Bluetooth device address or Plug and Play container on Bluetooth ancestry, and supports removable USB consumer controls on a physical USB controller. The newer detector handles arrival races and keeps playback and volume observers independent of asynchronous metadata requests. In the old observer, a slow metadata request lengthened the entire sample interval and could prevent a fresh pre-command baseline even for a valid source-identified Play/Pause or volume command.

Windows' in-box Bluetooth stack may handle AVRCP directly. A Bluetooth AVRCP transport device in Windows does not on its own expose a source-identified consumer-control report. Windows' media session API identifies the playback app, and the WinX RFCOMM protocol handlers read gesture assignments and resultant settings. Neither is evidence that an earbud issued an individual media command. WinX leaves source-less AVRCP, standard software playback changes, native volume slider changes, and ambiguous events silent. An identified hardware command still requires a matching transition on the same current session or output. A failed, late, or ambiguous outcome is suppressed.

Microsoft references [Raw Input](https://learn.microsoft.com/en-us/windows/win32/inputdev/raw-input), [Bluetooth profiles](https://learn.microsoft.com/en-us/windows-hardware/drivers/bluetooth/general-bluetooth-support-in-windows), [Windows media sessions](https://learn.microsoft.com/en-us/uwp/api/windows.media.control.globalsystemmediatransportcontrolssession), and [stable Core Audio endpoint identity](https://learn.microsoft.com/en-us/windows/win32/api/mmdeviceapi/nf-mmdeviceapi-immdevice-getid).

### Optional local diagnostics

Launch a fresh WinX process with the environment variable **WINX_MEDIA_DIAGNOSTICS=1**. The log is written to **%LOCALAPPDATA%\NothingX Desktop\winx-media-diagnostics.log** and is capped at 128 KiB per run. It records registration and source classifications, playback state names, correlation outcomes, and suppression reasons. It does not log Bluetooth addresses, media titles, typing, audio, or telemetry. **nothingx-desktop.exe --media-inventory PATH** writes aggregate enumeration counts and exits without starting the app, installing hooks, registering Raw Input, or changing the device or media state.

## Windows validation

Rust and Node tests cover stored settings and migration, known and unknown media origins, foreground app and endpoint matching, notification priority, exact supported EQ snapshotting, Talk Mode confirmation and original-device recovery, shortcut conflicts, all overlay positions and themes, and isolated onboarding previews. The installer and portable ZIP can be inspected without running the application.

A read-only check on this Windows host found three eligible USB consumer-control collections. Windows listed Nothing Ear (3) AVRCP transport entries, and the WinX inventory found no linked compatible headset at the time of the check. Earbud-button behavior, live fades, restored listening settings, and the WebView presentation were not exercised on the desktop.

The following are manual Windows tests with compatible hardware. Use the system and app controls directly.

1. With an earlier preference file, confirm there is no automatic tour. Replay it from Settings. In a fresh profile confirm the tour opens, then confirm completion survives restarts. Check resizing, light and dark themes, Windows reduced motion, and 100%, 125%, 150%, and 200% display scaling.
2. Pair in Windows, select the device in WinX, check the reported battery, and test rescan with the device disconnected. Try all six preview positions. Record a custom Talk Mode shortcut, then clear it, reset it, and try an already-assigned combination.
3. Set a recognizable ANC strength, custom EQ, and output volume. Start the live demo, use the configured shortcut twice, check the three confirmed changes, and verify restoration. Change the default output or volume during the demo and confirm WinX targets only the original endpoint and keeps a newer user change. Repeat with an unsupported EQ profile, Bluetooth disconnect, sleep/resume, and normal exit and restart. Check that the simulated preview makes no device or system volume changes.
4. With diagnostics enabled, try earbud and independently identifiable hardware controls while a compatible device is connected. Compare with Spotify, YouTube, Windows volume slider changes, natural track completion, and resume from sleep. An earbud action that reaches Windows only through source-less AVRCP should stay silent. Keep the log local or review it before sharing.


## Implementation files

- Talk Mode and recovery use `src/talk.rs`, `src/talk_windows.rs`, `src/talk_audio_windows.rs`, and `src/pnp_windows.rs`. The existing device state, EQ profiles, preferences, hotkey registry, overlay actions, and icon catalog were extended.
- Media detection uses `src/media_input_windows.rs`, `src/media_windows.rs`, and `src/media_diagnostics.rs`. The awareness controller remains the connection gate and overlay dispatcher.
- Onboarding uses `ui/src/popover/onboarding.js`, `onboarding.css`, `shortcut-keys.js`, and the existing panel markup, settings, and bridge. `src/panel.rs`, `src/main.rs`, and the existing Windows Bluetooth transport provide the resizable window, device selection, and service integration.
- The demo audio is in `ui/src/assets/talk-demo.mp3`. `scripts/check-onboarding.cjs` runs the UI logic tests without a desktop. The Windows packager includes this guide.


## Recovery regression and Settings window

Previous WinX versions used one active audio journal for all endpoints. A failed `IMMDeviceEnumerator::GetDevice` or volume interface activation left that file in place. WinX retried the unavailable endpoint every 15 seconds, and the existence of the file also prevented preparation of any new endpoint. A corrupt journal followed the same blocking path.

WinX now parks failed records atomically, matches original targets by identity before sending any write, and attempts startup recovery once. A verified earbud reconnect can retry that earbud record. Explicit Retry checks retained records, and Reset archives them after a second user action. A missing setting or Windows audio call times out of the live state machine. Core Audio operations remain serialized, and a late result is ignored by generation. A stuck Windows audio worker can temporarily make volume unavailable until Windows releases the call or WinX restarts. The device stages can still be used as a partial result when it is safe.

Settings uses one separately titled, resizable Tao window with an embedded WebView and shared preferences. Closing it hides that window while the tray and Bluetooth connection remain active. Its separate IPC dispatch keeps replies and focus specific to the requesting window. The Settings theme applies to that window. Overlay appearance remains separately configurable.

Manual validation additions include opening Settings twice, closing and reopening it, using its categories at minimum size and at mixed monitor DPI, and checking Settings theme and keyboard focus. Test unmapped endpoints, repeated recovery, restoration after reconnect, explicit archive, and a different verified headset while the old endpoint remains absent.
