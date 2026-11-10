"""Run the patched native engine tests without unrelated platform dependencies."""
import json
from pathlib import Path
import shutil
import subprocess
import tomllib

root = Path(__file__).resolve().parent.parent
sdk = root / ".deps/pocketjs"
workspace = root / "target/native-tests"
workspace.mkdir(parents=True, exist_ok=True)
source_workspace = tomllib.loads((sdk / "Cargo.toml").read_text())["workspace"]
folders = ("core", "drivers", "spec", "render", "raster", "package", "modules/media", "runtime")
names = {}
external = set()
for folder in folders:
    source = sdk / "engine" / folder
    destination = workspace / folder
    destination.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source / "Cargo.toml", destination / "Cargo.toml")
    package = tomllib.loads((source / "Cargo.toml").read_text())
    names[package["package"]["name"]] = folder
    for section in ("dependencies", "dev-dependencies", "build-dependencies"):
        external.update(name for name, spec in package.get(section, {}).items()
                        if isinstance(spec, dict) and spec.get("workspace") and not name.startswith("pocket"))
    for directory in ("src", "tests"):
        link = destination / directory
        if (source / directory).exists():
            if link.is_symlink():
                link.unlink()
            link.symlink_to(source / directory, target_is_directory=True)

def toml_value(value):
    if isinstance(value, dict):
        return "{ " + ", ".join(f"{key} = {toml_value(item)}" for key, item in value.items()) + " }"
    return json.dumps(value)

manifest = '[workspace]\nresolver = "2"\nmembers = ' + json.dumps(folders) + '\n[workspace.package]\n'
manifest += ''.join(f'{key} = {toml_value(value)}\n' for key, value in source_workspace['package'].items())
manifest += '\n[workspace.dependencies]\n'
manifest += ''.join(f'{name} = {{ path = "{folder}", version = "0.16.0", default-features = false }}\n'
                    for name, folder in names.items())
manifest += ''.join(f'{name} = {toml_value(source_workspace["dependencies"][name])}\n' for name in sorted(external))
(workspace / "Cargo.toml").write_text(manifest)
subprocess.run(['cargo', 'test', '--release', '--offline', '--manifest-path', str(workspace / 'Cargo.toml'),
                '-p', 'pocketjs-core', '-p', 'pocket-runtime', '--features', 'pocketjs-core/counters', '-q'], check=True, cwd=root)
