"""Package the Windows x64 executable, WebView2 loader and optional setup."""
import argparse
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parent.parent


def require_x64(path):
    data = path.read_bytes()
    if data[:2] != b"MZ" or len(data) < 64:
        raise SystemExit(f"Invalid Windows executable: {path}")
    offset = struct.unpack_from("<I", data, 0x3c)[0]
    if data[offset:offset + 6] != b"PE\0\0\x64\x86":
        raise SystemExit(f"Expected a Windows x64 image: {path}")


def verify_microsoft_signature(bootstrapper):
    powershell = Path(os.environ["SystemRoot"]) / "System32/WindowsPowerShell/v1.0/powershell.exe"
    environment = {key: value for key, value in os.environ.items() if key.lower() != "psmodulepath"}
    environment["WINX_BOOTSTRAPPER_PATH"] = str(bootstrapper)
    script = """
$signature = Get-AuthenticodeSignature -LiteralPath $env:WINX_BOOTSTRAPPER_PATH
if ($signature.Status -ne 'Valid' -or
    $signature.SignerCertificate.GetNameInfo('SimpleName', $false) -ne 'Microsoft Corporation') {
    throw "The WebView2 bootstrapper does not have a valid Microsoft signature."
}
"""
    subprocess.run(
        [powershell, "-NoProfile", "-NonInteractive", "-Command", script],
        env=environment,
        check=True,
    )


def webview_bootstrapper():
    downloads = ROOT / "build/downloads"
    downloads.mkdir(parents=True, exist_ok=True)
    bootstrapper = downloads / "MicrosoftEdgeWebView2Setup.exe"
    if not bootstrapper.exists():
        url = "https://go.microsoft.com/fwlink/p/?LinkId=2124703"
        with urllib.request.urlopen(url, timeout=60) as response:
            data = response.read(4 * 1024 * 1024 + 1)
            if not response.geturl().startswith("https://") or len(data) > 4 * 1024 * 1024:
                raise SystemExit("Unexpected WebView2 bootstrapper response")
        if data[:2] != b"MZ":
            raise SystemExit("Microsoft's WebView2 bootstrapper download was not an executable")
        with tempfile.NamedTemporaryFile(dir=downloads, suffix=".exe", delete=False) as output:
            output.write(data)
            temporary = Path(output.name)
        try:
            if os.name == "nt":
                verify_microsoft_signature(temporary)
            temporary.replace(bootstrapper)
        finally:
            temporary.unlink(missing_ok=True)
    elif os.name == "nt":
        verify_microsoft_signature(bootstrapper)
    return bootstrapper


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path, nargs="?", default=ROOT / "target/release/nothingx-desktop.exe")
    parser.add_argument("--installer", action="store_true")
    parser.add_argument("--local-names", action="store_true", help="Use WinX release asset filenames")
    args = parser.parse_args()
    metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--locked", "--format-version", "1"], cwd=ROOT))
    version = next(p["version"] for p in metadata["packages"] if p["name"] == "nothingx-desktop")
    package = next(p for p in metadata["packages"] if p["name"] == "webview2-com-sys")
    loader = Path(package["manifest_path"]).parent / "x64/WebView2Loader.dll"
    require_x64(args.binary)
    require_x64(loader)

    stem = f"WinX-v{version}" if args.local_names else f"NothingX-Desktop-{version}"
    folder_name = f"{stem}-windows-x64"
    portable_name = f"{stem}-Portable-x64.zip" if args.local_names else f"{folder_name}.zip"
    dist = ROOT / "dist"
    dist.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(dir=dist, prefix="winx-package-") as staging:
        folder = Path(staging) / folder_name
        folder.mkdir()
        shutil.copy2(args.binary, folder / "nothingx-desktop.exe")
        shutil.copy2(loader, folder / loader.name)
        for name in ["README.md", "LICENSE", "THIRD_PARTY.md"]:
            shutil.copy2(ROOT / name, folder / name)
        shutil.copy2(dist / "DEPENDENCY_LICENSES.txt", folder)
        docs = folder / "docs"
        docs.mkdir()
        shutil.copy2(ROOT / "docs/WINX-TALK-MODE.md", docs)
        archive_path = dist / portable_name
        with zipfile.ZipFile(archive_path, "w", zipfile.ZIP_DEFLATED) as archive:
            for file in sorted(folder.rglob("*")):
                if file.is_file():
                    archive.write(file, Path(folder.name) / file.relative_to(folder))
        print(archive_path.name, flush=True)
        if args.installer:
            compiler = shutil.which("makensis")
            if compiler is None:
                raise SystemExit("Install NSIS and put makensis on PATH to build the Windows installer")
            bootstrapper = webview_bootstrapper()
            setup = dist / (f"{stem}-Setup-x64.exe" if args.local_names else f"{folder_name}-setup.exe")
            file_version = version.split("-")[0].split("+")[0] + ".0"
            prefix = "/D" if os.name == "nt" else "-D"
            defines = {
                "APP_VERSION": version,
                "APP_FILE_VERSION": file_version,
                "APP_DIR": folder,
                "WEBVIEW2_BOOTSTRAPPER": bootstrapper,
                "OUTPUT_EXE": setup,
            }
            subprocess.run(
                [compiler, *(f"{prefix}{key}={value}" for key, value in defines.items()),
                 str(ROOT / "packaging/windows.nsi")],
                check=True, cwd=ROOT,
            )
            print(setup.name, flush=True)


if __name__ == "__main__":
    main()
