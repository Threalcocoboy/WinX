# Updating WinX

**Settings → Updates** displays the running version, the latest suitable release, the last successful check, and release notes. Automatic checks and update notices start enabled. The preview channel is opt-in. **Check for Updates** requests current data immediately, subject to GitHub's rate-limit cooldown.

WinX queries the public GitHub Releases API in a background thread. It checks shortly after startup and about once per day, using the cached timestamp and ETag to reduce traffic. The stable channel uses the latest published release endpoint. With previews enabled, WinX considers the newest compatible stable and prerelease versions in GitHub's 100 most recent releases. WinX compares versions by SemVer precedence.

## Download and install

1. Select **Update Now** to download the Windows x64 setup package. Progress reflects bytes received. WinX checks the official repository and asset URLs, the expected asset name and size, HTTPS redirects to GitHub's asset hosts, the executable header, and SHA-256. It compares the digest exposed by GitHub when available.
2. Read the release notes and installer disclosure, then choose **Close WinX and open setup** or **Cancel**. The official installer is unsigned. The checksum confirms file integrity relative to GitHub's metadata. Independent publisher verification requires signing.
3. WinX settles any active Talk Mode transition and attempts restoration. If a recovery record is unresolved or settings cannot be saved, it keeps the recovery data and stops the installation. Reconnect the original target, open **Settings → Talk Mode → Retry Recovery**, and retry.
4. The helper waits for WinX to exit, rechecks the file hash, and opens interactive per-user Setup. Follow Setup's prompts. The helper checks the installed executable version and waits for the new application to confirm startup. If installation fails or is cancelled, it reports the error and tries to reopen the previous executable.

Temporary downloads are kept under the WinX local profile. Failed downloads and a cancelled in-app confirmation are cleaned up. Setup leaves the existing profile in place, including onboarding, shortcuts, device choice, and battery and overlay preferences.

## Manual update

Download the installer from [WinX Releases](https://github.com/Threalcocoboy/WinX/releases/latest), compare its hash with `SHA256SUMS.txt`, then exit WinX normally before opening Setup. With the portable build, exit WinX and extract the new ZIP to a fresh folder, then start it. An in-app update from a portable build opens the per-user installer.

```powershell
Get-FileHash -Algorithm SHA256 -LiteralPath .\WinX-vVERSION-Setup-x64.exe
```

## Manual release validation

- In a disposable Windows profile, install the previous build from the same release channel. Change a shortcut, theme, device choice, or battery preference, then verify those values after an upgrade.
- With the official feed serving a later test release, check that a single update notification offers all three actions. Restart WinX, manually check again, and check that the same version stays quiet. Repeat in both themes at 100%, 150%, and 200% scale.
- Cancel once at WinX's confirmation and once inside Setup. Repeat with a complete installation, then confirm the version in About and Updates. Verify error reporting with interrupted connectivity or simulated low disk space on the test machine.
- Repeat during Talk Mode activation and with the original output or earbuds disconnected. Verify that WinX waits for transitions, restores the original listening state where possible, and leaves unresolved records available in **Retry Recovery**.
