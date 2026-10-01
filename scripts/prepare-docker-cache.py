#!/usr/bin/env python3
"""Prepare only public registry archives and indexes from the locked dependency graph."""
import json
from pathlib import Path
import shutil
import subprocess

root = Path(__file__).resolve().parent.parent
metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--locked", "--all-features", "--format-version", "1"], cwd=root))
output = root / "target/docker-cargo-cache"
output.mkdir(parents=True, exist_ok=True)
registry_names = set()
for package in metadata["packages"]:
    if not package["source"]:
        continue
    directory = Path(package["manifest_path"]).parent
    registry_root = directory.parents[2]
    registry_name = directory.parent.name
    registry_names.add(registry_name)
    archive = registry_root / "cache" / registry_name / f"{package['name']}-{package['version']}.crate"
    target = output / "registry/cache" / registry_name / archive.name
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(archive, target)
    name = package["name"]
    entry = (f"1/{name}" if len(name) == 1 else f"2/{name}" if len(name) == 2
             else f"3/{name[0]}/{name}" if len(name) == 3 else f"{name[:2]}/{name[2:4]}/{name}")
    index = registry_root / "index" / registry_name
    destination = output / "registry/index" / registry_name
    (destination / ".cache" / entry).parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(index / ".cache" / entry, destination / ".cache" / entry)
    shutil.copy2(index / "config.json", destination / "config.json")
if len(registry_names) != 1:
    raise ValueError("Expected one registry source")
registry_name = next(iter(registry_names))
if registry_name.startswith("rsproxy.cn-"):
    (output / "config.toml").write_text('[source.crates-io]\nreplace-with = "cached-registry"\n[source.cached-registry]\nregistry = "sparse+https://rsproxy.cn/index/"\n')
elif not registry_name.startswith("index.crates.io-"):
    raise ValueError("Unsupported local registry; use the normal online Docker build")
print("Public dependency cache prepared at target/docker-cargo-cache")
