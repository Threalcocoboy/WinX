"""Exercise the packaged Windows app on an interactive Windows desktop."""

import argparse
import ctypes
from ctypes import wintypes
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import zipfile


def snapshot(path):
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return {}


def wait_for(process, path, condition, seconds=10):
    until = time.monotonic() + seconds
    while time.monotonic() < until:
        if process.poll() is not None:
            raise RuntimeError(f"Application stopped early (exit code {process.returncode})")
        state = snapshot(path)
        if condition(state):
            return state
        time.sleep(0.1)
    raise RuntimeError(f"Timed out waiting for application state: {snapshot(path).get('message')}")


def close_panel(pid):
    user32 = ctypes.WinDLL("user32", use_last_error=True)
    find = user32.FindWindowW
    find.argtypes = (wintypes.LPCWSTR, wintypes.LPCWSTR)
    find.restype = wintypes.HWND
    owner_pid = wintypes.DWORD()
    hwnd = find("NothingXDesktopWindow", "WinX")
    if not hwnd:
        raise RuntimeError("Could not find the native panel")
    owner = user32.GetWindowThreadProcessId
    owner.argtypes = (wintypes.HWND, ctypes.POINTER(wintypes.DWORD))
    owner(hwnd, ctypes.byref(owner_pid))
    if owner_pid.value != pid:
        raise RuntimeError("The native panel belongs to another process")
    post = user32.PostMessageW
    post.argtypes = (wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM)
    if not post(hwnd, 0x0010, 0, 0):  # WM_CLOSE
        raise ctypes.WinError(ctypes.get_last_error())


def has_battery(state):
    device = state.get("device", {})
    return device.get("connected") and any(device.get("battery", {}).values())


def run(binary, require_battery):
    if not binary.is_file():
        raise FileNotFoundError(binary)
    with tempfile.TemporaryDirectory(prefix="nothingx-smoke-") as directory:
        path = Path(directory) / "status.json"
        environment = os.environ | {"LOCALAPPDATA": directory}
        runtime = 45 if require_battery else 15
        command = [str(binary), "--status-file", str(path), "--quit-after", str(runtime)]
        for cycle in range(1, 3):
            path.unlink(missing_ok=True)
            process = subprocess.Popen(command, env=environment, cwd=binary.parent)
            try:
                wait_for(process, path, lambda s: s.get("popoverVisible") is False)
                if cycle == 1:
                    subprocess.run([str(binary), "--startup"], env=environment, check=True, timeout=5)
                    assert snapshot(path)["popoverOpen"] is False
                subprocess.run([str(binary)], env=environment, check=True, timeout=5)
                wait_for(process, path, lambda s: s.get("popoverVisible") and s.get("popoverOpen"))
                close_panel(process.pid)
                state = wait_for(process, path, lambda s: s.get("popoverVisible") is False)
                if require_battery:
                    state = wait_for(process, path, has_battery, seconds=30)
                    device = state["device"]
                    print(f"Cycle {cycle}: {device['name']} reported {device['battery']}", flush=True)
                if process.wait(timeout=runtime + 5):
                    raise RuntimeError(f"Application returned exit code {process.returncode}")
                print(f"Cycle {cycle}: launch, single instance, panel open/close, clean exit passed", flush=True)
            finally:
                if process.poll() is None:
                    process.terminate()
                    process.wait(timeout=5)


if __name__ == "__main__":
    root = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path, nargs="?")
    parser.add_argument("--require-battery", action="store_true")
    args = parser.parse_args()
    if args.binary:
        run(args.binary.resolve(), args.require_battery)
    else:
        with tempfile.TemporaryDirectory(prefix="winx-portable-") as directory:
            version = (root / "Cargo.toml").read_text(encoding="utf-8").split('version = "', 1)[1].split('"', 1)[0]
            with zipfile.ZipFile(root / f"dist/WinX-v{version}-Portable-x64.zip") as archive:
                archive.extractall(directory)
            binaries = list(Path(directory).glob("*/nothingx-desktop.exe"))
            if len(binaries) != 1:
                raise RuntimeError("Expected one WinX executable in the portable archive")
            run(binaries[0], args.require_battery)
