"""Verify the WinX release assets, PE architecture, metadata and ZIP integrity."""
import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import os
import subprocess
from tempfile import TemporaryDirectory
import time
import zipfile

ROOT = Path(__file__).resolve().parent.parent
DIST = ROOT / "dist"


def require(condition, message):
    if not condition:
        raise SystemExit(message)


def machine(data, name):
    require(data[:2] == b"MZ" and len(data) >= 64, f"Invalid PE header in {name}")
    address = int.from_bytes(data[60:64], "little")
    require(data[address:address + 4] == b"PE\0\0", f"Invalid PE image in {name}")
    return int.from_bytes(data[address + 4:address + 6], "little")


def checked_zip(path):
    archive = zipfile.ZipFile(path)
    require(archive.testzip() is None, f"Corrupt archive: {path.name}")
    names = archive.namelist()
    require(len(set(names)) == len(names), f"Duplicate entries in {path.name}")
    for name in names:
        entry = PurePosixPath(name)
        require(not entry.is_absolute() and ".." not in entry.parts and "\\" not in name,
                f"Unexpected path in {path.name}")
    return archive


def verify_version(path, version):
    if os.name != "nt":
        return
    powershell = Path(os.environ["SystemRoot"]) / "System32/WindowsPowerShell/v1.0/powershell.exe"
    environment = {key: value for key, value in os.environ.items() if key.lower() != "psmodulepath"}
    environment["WINX_VERIFY_PE"] = str(path)
    actual = subprocess.check_output(
        [powershell, "-NoProfile", "-NonInteractive", "-Command",
         "(Get-Item -LiteralPath $env:WINX_VERIFY_PE).VersionInfo.ProductVersion"],
        env=environment, text=True,
    ).strip()
    require(actual == version, f"{path.name} reports {actual}, expected {version}")


def smoke_test(executable, temporary):
    """Run the packaged app with a fresh profile on a Windows runner."""
    status = temporary / "status.json"
    profile = temporary / "profile"
    data = profile / "NothingX Desktop"
    data.mkdir(parents=True)
    (data / "winx-preferences.json").write_text('{"hotkeys":{"enabled":false}}', encoding="utf-8")
    process = subprocess.Popen(
        [executable, "--status-file", status, "--quit-after", "60", "--no-bluetooth"],
        env={**os.environ, "LOCALAPPDATA": str(profile)}, cwd=executable.parent,
    )
    try:
        until = time.monotonic() + 50
        ready = False
        snapshot = {}
        while process.poll() is None and time.monotonic() < until:
            try:
                snapshot = json.loads(status.read_text(encoding="utf-8"))
                ready = snapshot.get("rendererReady", False)
            except (OSError, json.JSONDecodeError, KeyError):
                pass
            if ready:
                break
            time.sleep(0.1)
        require(ready, f"The WinX WebView did not confirm startup (exit={process.poll()}, "
                f"native panel initialized={bool(snapshot)})")
        require(process.wait(timeout=65) == 0, "WinX did not exit cleanly")
    finally:
        if process.poll() is None:
            process.terminate()
            process.wait(timeout=5)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version")
    parser.add_argument("--smoke", action="store_true", help="Launch the app from the ZIP on an isolated Windows runner")
    args = parser.parse_args()
    stem = f"WinX-v{args.version}"
    setup, portable, source = (DIST / f"{stem}{suffix}" for suffix in ("-Setup-x64.exe", "-Portable-x64.zip", "-Source.zip"))
    expected = [(hashlib.sha256(file.read_bytes()).hexdigest(), file.name) for file in (setup, portable, source)]
    manifest = (DIST / "SHA256SUMS.txt").read_text(encoding="ascii").splitlines()
    require(manifest == [f"{digest}  {name}" for digest, name in expected],
            "Release manifest does not match the exact packaged files")
    require(machine(setup.read_bytes(), setup.name) in (0x14c, 0x8664), "Unexpected NSIS setup architecture")
    verify_version(setup, args.version)
    with checked_zip(portable) as archive, TemporaryDirectory(ignore_cleanup_errors=True) as scratch:
        folder = f"{stem}-windows-x64"
        executable = f"{folder}/nothingx-desktop.exe"
        for name in (executable, f"{folder}/WebView2Loader.dll"):
            require(machine(archive.read(name), name) == 0x8664, f"{name} is not x64")
        for name in ("LICENSE", "THIRD_PARTY.md", "DEPENDENCY_LICENSES.txt", "docs/WINX-TALK-MODE.md"):
            require(f"{folder}/{name}" in archive.namelist(), f"Missing {name} from portable package")
        temporary = Path(scratch)
        app = Path(archive.extract(executable, temporary))
        verify_version(app, args.version)
        if args.smoke:
            archive.extract(f"{folder}/WebView2Loader.dll", temporary)
            smoke_test(app, temporary)
    with checked_zip(source) as archive:
        for name in ("Cargo.toml", "Cargo.lock", "LICENSE", "src/winx_update.rs",
                     "src/update-install.ps1", "scripts/build-windows.ps1", "packaging/windows.nsi"):
            require(f"WinX-{args.version}/{name}" in archive.namelist(), f"Missing corresponding source: {name}")
    print(f"WinX {args.version}: installer, x64 payload, archives, version metadata and SHA-256 verified")


if __name__ == "__main__":
    main()
