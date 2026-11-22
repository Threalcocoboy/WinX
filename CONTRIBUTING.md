# Contributing to WinX

Use [GitHub Issues](https://github.com/Threalcocoboy/WinX/issues) to report bugs, discuss changes, or share a device compatibility result. Describe what happened and what you expected, and include WinX, Windows, and WebView2 versions when relevant. For a hardware report, list the model, firmware, Bluetooth adapter, and individual controls confirmed on real hardware. State whether the Nothing X phone app was connected.

Review logs and screenshots before sharing. Remove Bluetooth addresses, Windows usernames, media titles, paths, and other personal details. Report security issues through the process in [SECURITY.md](SECURITY.md).

For a pull request, describe the change and include unit tests or manual verification steps where appropriate. Run the Windows checks from a Developer PowerShell. Preserve upstream notices and keep feature claims tied to the tests you performed.

```powershell
./scripts/check-windows.ps1
```

See [BUILDING.md](docs/BUILDING.md) for MSVC, WebView2, and packaging setup.
