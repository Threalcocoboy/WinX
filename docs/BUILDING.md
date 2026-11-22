# Build WinX on Windows

Install Visual Studio Build Tools with **Desktop development with C++** and a Windows 10 or 11 SDK. Install Rust via rustup with the **x86_64-pc-windows-msvc** host, Node.js, Python 3.11 or later, and NSIS 3.10 or later. Put `makensis.exe` on PATH. WinX uses the system WebView2 Evergreen Runtime.

Open **Developer PowerShell for Visual Studio** at the repository root.

```powershell
rustup toolchain install 1.95.0 --profile minimal --component rustfmt --component clippy
rustup override set 1.95.0
rustup target add x86_64-pc-windows-msvc
cargo build --locked --target x86_64-pc-windows-msvc
./scripts/check-windows.ps1
```

The development executable is `target/x86_64-pc-windows-msvc/debug/nothingx-desktop.exe`. The check script runs rustfmt, strict Clippy, a Windows build, Rust tests, JavaScript checks, and a parser check for the installation helper.

## Package the release

```powershell
./scripts/build-windows.ps1
```

The script builds the release profile, stages the x64 application and the WebView2 loader, validates the Microsoft signature on the Evergreen bootstrapper used by Setup, and runs NSIS. It verifies the Windows binaries, archive contents, version metadata and SHA-256 manifest. Outputs in `dist/` are `WinX-vVERSION-Setup-x64.exe`, `WinX-vVERSION-Portable-x64.zip`, `WinX-vVERSION-Source.zip`, and `SHA256SUMS.txt`.

Application and packaging metadata come from the Cargo package version. Use a SemVer version with no build metadata. Add `docs/releases/vVERSION.md` and update the changelog before creating the matching `vVERSION` tag. The Windows release workflow tests and packages the tag, launches the portable build with a fresh profile on its Windows runner, then compares GitHub's uploaded asset sizes and digests before publishing the draft. A SemVer prerelease is published as a GitHub prerelease. Run live device and interactive Setup checks separately.

The editable hero animation is in [assets/branding](../assets/branding/). Install Pillow 11.3.0 and imageio-ffmpeg 0.6.0 into a Python environment and run `python assets/branding/render-logo-morph.py`. It uses the Segoe UI fonts from Windows by default.

As an alternative to pushing a tag, create a `release/vVERSION` branch at the intended commit after this workflow is on the default branch. The publication job creates the matching tag after the package checks pass.

## GitHub project settings

The distribution repository is **Threalcocoboy/WinX**. Use public visibility with Issues and Actions enabled. Set the description to “An unofficial Nothing & CMF companion app for Windows. Lightweight, customizable, and built for a better desktop experience.” Enable private vulnerability reporting in the repository's security settings.

Use these topics: `nothing`, `nothing-tech`, `cmf`, `windows`, `rust`, `bluetooth`, `earbuds`, `headphones`, `desktop`, `companion-app`, and `winx`.
