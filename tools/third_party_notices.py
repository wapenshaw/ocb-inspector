"""Regenerate THIRD-PARTY-NOTICES.txt: the licences of everything shipped inside `ocbi` and
`ocb-inspector` (the Windows and Linux release builds).

Covers:
  - the EDK2 reference used by crates/core/src/firmware/tiano.rs (BSD-2-Clause-Patent);
  - every Rust crate in the normal (non-dev, non-build) dependency graph of ocbi-cli and
    ocbi-desktop for every release target in TARGETS (proc-macros are included, conservatively);
  - every npm package whose code or files end up in the UI bundle (found from a sourcemapped
    `vite build` into a temporary directory, plus emitted assets such as the bundled fonts,
    so devDependencies that only run at build time are left out).

Licence texts are taken from the files each package ships; identical texts are printed once
with the list of packages that use them. Run from the repository root after changing
dependencies:

    python -I tools/third_party_notices.py            # writes THIRD-PARTY-NOTICES.txt
    python -I tools/third_party_notices.py --check    # exits 1 if the file is out of date
"""

from __future__ import annotations

import hashlib
import json
import re
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "THIRD-PARTY-NOTICES.txt"
# The release targets: Windows, and Linux (static CLI on musl, desktop on glibc). One file covers
# both, so a crate used on only one of them is listed too.
TARGETS = ["x86_64-pc-windows-msvc", "x86_64-unknown-linux-gnu", "x86_64-unknown-linux-musl"]
BINARIES = ["ocbi-cli", "ocbi-desktop"]
OWN = {"ocbi-cli", "ocb-core", "ocbi-desktop"}
LICENSE_FILE = re.compile(r"^(licen[cs]e|copying|notice|unlicense|copyright)", re.I)
# Crates published without a licence file whose sibling crate from the same repository ships it.
SAME_REPO = {"alloc-stdlib": "alloc-no-stdlib"}
EDK2 = ROOT / "crates/core/src/firmware/LICENSE-EDK2.txt"


def run(cmd: list[str], cwd: Path = ROOT) -> str:
    return subprocess.run(
        cmd, cwd=cwd, check=True, capture_output=True, text=True, encoding="utf-8",
        shell=sys.platform == "win32" and cmd[0] in ("pnpm", "npx"),
    ).stdout


def license_texts(pkg_dir: Path, declared: str | None) -> list[tuple[str, str]]:
    files = []
    if declared:
        files.append(pkg_dir / declared)
    files += sorted(
        p for p in pkg_dir.iterdir() if p.is_file() and LICENSE_FILE.match(p.name)
    )
    seen, out = set(), []
    for f in files:
        f = f.resolve()
        if f in seen or not f.is_file():
            continue
        seen.add(f)
        out.append((f.name, f.read_text(encoding="utf-8", errors="replace").strip()))
    return out


def rust_packages() -> list[dict]:
    names = set()
    for target in TARGETS:
        for b in BINARIES:
            tree = run(["cargo", "tree", "-q", "-e", "normal", "--target", target, "-p", b,
                        "--prefix", "none", "-f", "{p}"])
            for line in tree.splitlines():
                parts = line.split()
                if len(parts) >= 2:
                    names.add((parts[0], parts[1].lstrip("v")))
    packages = {}
    for target in TARGETS:
        meta = json.loads(run(["cargo", "metadata", "--format-version", "1", "--locked",
                               "--filter-platform", target]))
        for p in meta["packages"]:
            packages[(p["name"], p["version"])] = p
    out = []
    for p in packages.values():
        if (p["name"], p["version"]) not in names or p["name"] in OWN:
            continue
        d = Path(p["manifest_path"]).parent
        out.append({
            "name": p["name"], "version": p["version"],
            "license": p.get("license") or "(see licence file)",
            "source": f"https://crates.io/crates/{p['name']}/{p['version']}",
            "texts": license_texts(d, p.get("license_file")),
        })
    by_name = {p["name"]: p for p in out}
    for p in out:
        sib = by_name.get(SAME_REPO.get(p["name"], ""))
        if not p["texts"] and sib:
            p["texts"] = [(f"{n}, from {sib['name']} in the same repository", t)
                          for n, t in sib["texts"]]
    return sorted(out, key=lambda p: (p["name"], p["version"]))


def npm_packages() -> list[dict]:
    ui = ROOT / "ui"
    with tempfile.TemporaryDirectory() as tmp:
        run(["pnpm", "exec", "vite", "build", "--sourcemap", "--outDir", tmp, "--emptyOutDir"], ui)
        bundled = set()
        # Static assets (fonts, images) are copied into the bundle without a sourcemap entry:
        # match each emitted file, minus Vite's content hash, to the package that ships it.
        for a in Path(tmp, "assets").iterdir():
            if a.suffix in (".js", ".css", ".map"):
                continue
            stem = re.sub(r"-[A-Za-z0-9_-]{8}$", "", a.stem) + a.suffix
            for hit in (ui / "node_modules/.pnpm").glob(f"*/node_modules/**/{stem}"):
                pkg = re.search(r"node_modules/\.pnpm/[^/]+/node_modules/((?:@[^/]+/)?[^/]+)", hit.as_posix())
                if pkg:
                    bundled.add(pkg.group(1))
                    break
        for m in Path(tmp, "assets").glob("*.map"):
            for src in json.loads(m.read_text(encoding="utf-8"))["sources"]:
                hit = re.search(r"node_modules/(?:\.pnpm/[^/]+/node_modules/)?((?:@[^/]+/)?[^/]+)", src)
                if hit:
                    bundled.add(hit.group(1))
    out = []
    for name in sorted(bundled):
        # Resolve through svelte's own node_modules too (pnpm keeps transitive deps nested).
        candidates = [ui / "node_modules" / name]
        candidates += list((ui / "node_modules/.pnpm").glob(f"*/node_modules/{name}"))
        d = next(c for c in candidates if (c / "package.json").is_file())
        pj = json.loads((d / "package.json").read_text(encoding="utf-8"))
        out.append({
            "name": name, "version": pj["version"],
            "license": pj.get("license") or "(see licence file)",
            "source": f"https://www.npmjs.com/package/{name}/v/{pj['version']}",
            "texts": license_texts(d.resolve(), None),
        })
    return out


def render(rust: list[dict], npm: list[dict]) -> str:
    rule = "=" * 78
    lines = [
        "THIRD-PARTY NOTICES for OCB Inspector (ocbi and ocb-inspector, Windows and Linux)",
        "",
        "Generated by tools/third_party_notices.py; do not edit by hand.",
        "Ship this file next to the executables.",
        "",
        "Some components are offered under a choice of licences (for example",
        "\"MIT OR Apache-2.0\"); every text the package ships is reproduced below.",
        "Components under MPL-2.0 are used unmodified; their source code is available",
        "at the crates.io address listed for each.",
        "",
        rule,
        "1. EDK2 (TianoCore) BaseUefiDecompressLib",
        "   Used as the reference for crates/core/src/firmware/tiano.rs.",
        "   SPDX-License-Identifier: BSD-2-Clause-Patent",
        rule,
        "",
        EDK2.read_text(encoding="utf-8").strip(),
        "",
    ]
    for title, pkgs in (("2. Rust crates", rust), ("3. JavaScript packages and fonts in the UI bundle", npm)):
        lines += [rule, title, rule, ""]
        width = max(len(f"{p['name']} {p['version']}") for p in pkgs)
        for p in pkgs:
            lines.append(f"{p['name'] + ' ' + p['version']:<{width}}  {p['license']}")
        lines.append("")
    # Licence texts, each distinct text once.
    groups: dict[str, dict] = {}
    missing = []
    for p in rust + npm:
        if not p["texts"]:
            missing.append(p)
        for fname, text in p["texts"]:
            key = hashlib.sha256(re.sub(r"\s+", " ", text).encode()).hexdigest()
            g = groups.setdefault(key, {"text": text, "users": []})
            g["users"].append(f"{p['name']} {p['version']} ({fname})")
    lines += [rule, "4. Licence texts", rule, ""]
    for g in sorted(groups.values(), key=lambda g: g["users"][0]):
        lines.append("-" * 78)
        lines.append("Used by: " + ", ".join(g["users"]))
        lines.append("-" * 78)
        lines += ["", g["text"], ""]
    if missing:
        lines += [rule, "5. Packages that ship no licence file", rule, ""]
        lines.append("These declare their licence only in package metadata; the standard text")
        lines.append("of that licence applies (see section 4 for copies of each licence named).")
        lines.append("")
        for p in missing:
            lines.append(f"{p['name']} {p['version']}  {p['license']}  {p['source']}")
        lines.append("")
    return "\n".join(lines).rstrip() + "\n"


def main() -> int:
    text = render(rust_packages(), npm_packages())
    if "--check" in sys.argv:
        current = OUT.read_text(encoding="utf-8") if OUT.exists() else ""
        if current != text:
            print(f"{OUT.name} is out of date; run python -I tools/third_party_notices.py")
            return 1
        print(f"{OUT.name} is up to date")
        return 0
    OUT.write_text(text, encoding="utf-8", newline="\n")
    print(f"wrote {OUT.name} ({len(text) // 1024} KiB)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
