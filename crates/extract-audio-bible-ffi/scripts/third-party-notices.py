"""Writes THIRD-PARTY-NOTICES: every Rust crate built into a binary, with its license and the
license files it ships.

The npm package (WebAssembly) and the Python wheels contain extract-audio-bible-ffi compiled with all of its
dependencies, so they carry those crates' notices. This lists the crates the binary links for one
target (normal dependencies only: build scripts, proc macros' own dependencies, and dev
dependencies don't end up in it), reading `cargo metadata`, which needs nothing installed beyond
cargo itself.

    python scripts/third-party-notices.py --target wasm32-unknown-unknown -o dist/wasm/pkg/THIRD-PARTY-NOTICES
    python scripts/third-party-notices.py -o dist/python/THIRD-PARTY-NOTICES   # the host's target

Run from crates/extract-audio-bible-ffi, or pass --manifest-path and --package for another crate.
"""

import argparse
import json
import subprocess
import sys
from pathlib import Path

LICENSE_NAMES = ("LICENSE", "LICENCE", "COPYING", "NOTICE", "UNLICENSE", "COPYRIGHT")


def host_target() -> str:
    out = subprocess.run(["rustc", "-vV"], check=True, capture_output=True, text=True).stdout
    for line in out.splitlines():
        if line.startswith("host: "):
            return line.removeprefix("host: ").strip()
    sys.exit("rustc -vV gave no host")


def is_proc_macro(package: dict) -> bool:
    """Proc macros run while compiling; they (and what only they use) aren't in the binary"""
    return any("proc-macro" in target["kind"] for target in package["targets"])


def license_files(package: dict) -> list[Path]:
    folder = Path(package["manifest_path"]).parent
    files = []
    if package.get("license_file"):
        files.append(folder / package["license_file"])
    for path in sorted(folder.iterdir()):
        if path.is_file() and path.name.upper().startswith(LICENSE_NAMES) and path not in files:
            files.append(path)
    return [f for f in files if f.is_file()]


def main() -> None:
    root = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--manifest-path", default=str(root / "Cargo.toml"))
    parser.add_argument("--package", default="extract-audio-bible-ffi", help="the crate built into the binary")
    parser.add_argument("--target", help="the target triple (default: the host's)")
    parser.add_argument("--features", default="", help="features of --package turned on")
    parser.add_argument("-o", "--output", required=True)
    args = parser.parse_args()

    target = args.target or host_target()
    command = ["cargo", "metadata", "--format-version", "1", "--manifest-path", args.manifest_path, "--filter-platform", target]
    if args.features:
        command += ["--features", args.features]
    metadata = json.loads(subprocess.run(command, check=True, capture_output=True, text=True).stdout)

    packages = {p["id"]: p for p in metadata["packages"]}
    nodes = {n["id"]: n for n in metadata["resolve"]["nodes"]}
    roots = [p["id"] for p in metadata["packages"] if p["name"] == args.package]
    if not roots:
        sys.exit(f"no package {args.package} in {args.manifest_path}")

    # The crates linked into the binary: normal dependencies, followed from the root
    seen = set()
    todo = list(roots)
    while todo:
        id = todo.pop()
        if id in seen or is_proc_macro(packages[id]):
            continue
        seen.add(id)
        for dep in nodes[id]["deps"]:
            if any(kind["kind"] is None for kind in dep["dep_kinds"]):
                todo.append(dep["pkg"])
    seen -= set(roots)

    crates = sorted((packages[id] for id in seen), key=lambda p: (p["name"], p["version"]))
    out = [
        "THIRD-PARTY NOTICES",
        "",
        f"This package contains {args.package} compiled for {target} with the following Rust crates.",
        "Each is listed with its license (an SPDX expression, as its Cargo.toml declares it) and",
        "the license and notice files it is distributed with. Where a crate offers a choice of",
        "licenses, it is used here under any of them.",
        "",
    ]
    for p in crates:
        out.append("=" * 78)
        out.append(f"{p['name']} {p['version']}")
        out.append(f"License: {p.get('license') or 'see the files below'}")
        if p.get("repository"):
            out.append(f"Source: {p['repository']}")
        if p.get("authors"):
            out.append(f"Authors: {', '.join(p['authors'])}")
        files = license_files(p)
        if not files:
            out.append("(no license file is distributed with this crate)")
        for f in files:
            out.append("")
            out.append(f"--- {f.name} ---")
            out.append(f.read_text(encoding="utf-8", errors="replace").rstrip())
        out.append("")

    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text("\n".join(out) + "\n", encoding="utf-8")
    print(f"wrote {output}: {len(crates)} crates for {target}")


if __name__ == "__main__":
    main()
