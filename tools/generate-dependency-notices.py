#!/usr/bin/env python3
"""Generate reproducible Rust and npm dependency license inventories.

The generator is intentionally dependency-free.  Rust metadata is read from
Cargo's resolved graph and npm metadata from package-lock.json.  License and
notice files are copied verbatim from the locally resolved package directories.
Run dependency restoration first so every package is available locally.
"""

from __future__ import annotations

import csv
import json
import os
import re
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "licenses" / "dependencies"
LICENSE_NAME = re.compile(r"^(licen[cs]e|copying|notice|copyright)([._-].*)?$", re.I)


def run_cargo_metadata() -> dict:
    command = [
        "powershell.exe",
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        str(ROOT / "tools" / "cargo.ps1"),
        "metadata",
        "--format-version",
        "1",
        "--locked",
        "--offline",
    ]
    completed = subprocess.run(
        command,
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    return json.loads(completed.stdout)


def license_files(package_dir: Path) -> list[Path]:
    if not package_dir.is_dir():
        return []
    return sorted(
        (path for path in package_dir.iterdir() if path.is_file() and LICENSE_NAME.match(path.name)),
        key=lambda path: path.name.casefold(),
    )


def read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8", errors="replace").strip()


def write_csv(path: Path, rows: list[dict], fields: list[str]) -> None:
    with path.open("w", encoding="utf-8", newline="") as stream:
        writer = csv.DictWriter(stream, fieldnames=fields, lineterminator="\n")
        writer.writeheader()
        writer.writerows(rows)


def rust_inventory() -> tuple[list[dict], str]:
    metadata = run_cargo_metadata()
    workspace = Path(metadata["workspace_root"]).resolve()
    rows: list[dict] = []
    notices = [
        "# Rust dependency notices",
        "",
        "Generated from `Cargo.lock` via `cargo metadata --locked --offline`.",
        "The inventory includes every external package in the resolved lockfile graph,",
        "including target-specific and build dependencies.",
        "",
    ]
    for package in sorted(metadata["packages"], key=lambda item: (item["name"].casefold(), item["version"])):
        manifest = Path(package["manifest_path"]).resolve()
        try:
            manifest.relative_to(workspace)
            is_workspace = True
        except ValueError:
            is_workspace = False
        if is_workspace:
            continue
        package_dir = manifest.parent
        files = license_files(package_dir)
        rows.append(
            {
                "name": package["name"],
                "version": package["version"],
                "license_expression": package.get("license") or "",
                "license_file_metadata": package.get("license_file") or "",
                "license_files_found": ";".join(path.name for path in files),
                "authors": "; ".join(package.get("authors") or []),
                "repository": package.get("repository") or "",
                "source": package.get("source") or "",
            }
        )
        notices.extend([f"## {package['name']} {package['version']}", ""])
        notices.append(f"Declared license: `{package.get('license') or package.get('license_file') or 'UNSPECIFIED'}`")
        if package.get("authors"):
            notices.append(f"Declared authors: {', '.join(package['authors'])}")
        notices.append("")
        if files:
            for path in files:
                notices.extend([f"### {path.name}", "", "```text", read_text(path), "```", ""])
        else:
            notices.extend(["No top-level license or notice file was present in the resolved package directory.", ""])
    return rows, "\n".join(notices).rstrip() + "\n"


def npm_inventory() -> tuple[list[dict], str]:
    lock_path = ROOT / "apps" / "desktop" / "package-lock.json"
    lock = json.loads(lock_path.read_text(encoding="utf-8"))
    rows: list[dict] = []
    notices = [
        "# npm dependency notices",
        "",
        "Generated from `apps/desktop/package-lock.json` and the corresponding",
        "locally installed package directories. Both runtime and build dependencies",
        "are listed because build tools may contribute code to distributed assets.",
        "",
    ]
    for relative, package in sorted(lock["packages"].items(), key=lambda item: item[0].casefold()):
        if not relative:
            continue
        package_dir = ROOT / "apps" / "desktop" / Path(relative.replace("/", os.sep))
        files = license_files(package_dir)
        name = package.get("name") or relative.rsplit("node_modules/", 1)[-1]
        rows.append(
            {
                "name": name,
                "version": package.get("version", ""),
                "license_expression": package.get("license", ""),
                "development_only": str(bool(package.get("dev", False))).lower(),
                "package_present": str(package_dir.is_dir()).lower(),
                "license_files_found": ";".join(path.name for path in files),
                "resolved": package.get("resolved", ""),
                "integrity": package.get("integrity", ""),
            }
        )
        notices.extend([f"## {name} {package.get('version', '')}", ""])
        notices.append(f"Declared license: `{package.get('license') or 'UNSPECIFIED'}`")
        notices.append("")
        if files:
            for path in files:
                notices.extend([f"### {path.name}", "", "```text", read_text(path), "```", ""])
        else:
            notices.extend(["No top-level license or notice file was present in the installed package directory.", ""])
    return rows, "\n".join(notices).rstrip() + "\n"


def main() -> None:
    OUTPUT.mkdir(parents=True, exist_ok=True)
    rust_rows, rust_notices = rust_inventory()
    npm_rows, npm_notices = npm_inventory()
    write_csv(
        OUTPUT / "rust-dependencies.csv",
        rust_rows,
        ["name", "version", "license_expression", "license_file_metadata", "license_files_found", "authors", "repository", "source"],
    )
    write_csv(
        OUTPUT / "npm-dependencies.csv",
        npm_rows,
        ["name", "version", "license_expression", "development_only", "package_present", "license_files_found", "resolved", "integrity"],
    )
    (OUTPUT / "RUST-NOTICES.md").write_text(rust_notices, encoding="utf-8", newline="\n")
    (OUTPUT / "NPM-NOTICES.md").write_text(npm_notices, encoding="utf-8", newline="\n")
    summary = {
        "rust_packages": len(rust_rows),
        "rust_missing_declared_license": sum(
            not row["license_expression"] and not row["license_file_metadata"] for row in rust_rows
        ),
        "rust_missing_local_notice": sum(not row["license_files_found"] for row in rust_rows),
        "npm_packages": len(npm_rows),
        "npm_missing_declared_license": sum(not row["license_expression"] for row in npm_rows),
        "npm_missing_local_notice": sum(not row["license_files_found"] for row in npm_rows),
        "npm_missing_local_notice_for_present_package": sum(
            row["package_present"] == "true" and not row["license_files_found"] for row in npm_rows
        ),
    }
    (OUTPUT / "summary.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8", newline="\n")
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
