"""Create reproducible source and dependency-notice artifacts, without build trees."""
import argparse
from pathlib import Path
import subprocess
import json
import zipfile

root = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--windows", action="store_true", help="Use the WinX distribution name")
args = parser.parse_args()
destination = root / "dist"
destination.mkdir(exist_ok=True)
metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--locked", "--format-version", "1"], cwd=root))
version = next(p["version"] for p in metadata["packages"] if p["name"] == "nothingx-desktop")
notices = []
for package in sorted(metadata["packages"], key=lambda p: (p["name"], p["version"])):
    if package["source"] is None:
        continue
    directory = Path(package["manifest_path"]).parent
    notices.append(f"\n{'=' * 70}\n{package['name']} {package['version']} — {package.get('license', '')}\n")
    files = [p for p in directory.iterdir() if p.is_file() and p.name.lower().startswith(("license", "copying", "notice", "copyright"))]
    for file in sorted(files):
        notices.append(f"\n{file.name}\n{file.read_text(encoding='utf-8', errors='replace')}\n")
(destination / "DEPENDENCY_LICENSES.txt").write_text("".join(notices), encoding="utf-8")
if (root / ".git").exists():
    files = subprocess.check_output(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"], cwd=root
    ).decode().split("\0")
else:
    # Keep the source archive buildable after extraction, without a Git checkout.
    folders = (".github", "assets", "docs", "native", "packaging", "scripts", "src", "ui")
    files = [p.name for p in root.iterdir() if p.is_file() and not p.name.startswith(".env")]
    files.extend(
        p.relative_to(root).as_posix()
        for folder in folders
        for p in (root / folder).rglob("*")
        if p.is_file() and "__pycache__" not in p.parts
    )
name_prefix = "WinX" if args.windows else "NothingX-Desktop"
archive_name = f"WinX-v{version}-Source.zip" if args.windows else f"{name_prefix}-{version}-source.zip"
with zipfile.ZipFile(destination / archive_name, "w", zipfile.ZIP_DEFLATED) as archive:
    for name in sorted(set(files)):
        if name and (root / name).is_file():
            archive.write(root / name, Path(f"{name_prefix}-{version}") / name)
