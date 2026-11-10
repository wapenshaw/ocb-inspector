"""Summarize a profile-code survey JSONL: group images by save/load routine fingerprints.

Usage: python -I summarize_survey.py <survey.jsonl>
"""
import collections
import json
import sys


def role(fn):
    s = " ".join(fn["strings"])
    if "OcInfoPassToSetup" in s:
        return "save"
    if "FixedBoot" in s:
        return "load"
    return "other"


def main(path):
    rows = [json.loads(l) for l in open(path, encoding="utf-8")]
    groups = collections.defaultdict(list)
    for r in rows:
        if "error" in r:
            groups[("ERROR", r["error"][:60])].append(r["path"])
            continue
        fns = {}
        for m in r["profile_modules"]:
            for f in m["mos_functions"]:
                fns.setdefault(role(f), []).append(f)
        save = tuple(sorted(f["fingerprint"] for f in fns.get("save", [])))
        load = tuple(sorted(f["fingerprint"] for f in fns.get("load", [])))
        other = len(fns.get("other", []))
        masks = tuple(sorted({m for fl in fns.values() for f in fl for m in f["imm64"] if m.startswith("0x4") or m.startswith("0x18")}))
        spf = tuple(s.get("count") for s in r["spf"])
        trailer = tuple(sorted(f["fingerprint"] for m in r["trailer_modules"] for f in m["oci_functions"]))
        # $OCI$ code lives in OcProfileInfo, in the setup browser itself (AM5, Intel 800), or nowhere.
        in_browser = any(m["oci_functions"] for m in r["profile_modules"])
        oci = "oci:module" if trailer else "oci:setup-browser" if in_browser else "oci:none"
        key = (save, load, other, masks, spf and "spf" or "no-spf", oci)
        groups[key].append((r["bios_id"], spf))
    for k, v in sorted(groups.items(), key=lambda kv: -len(kv[1])):
        print(len(v), k)
        print("    ", ", ".join(f"{i}{list(s) if s else ''}" for i, s in v[:40] if True) if not isinstance(v[0], str) else v[:5])


if __name__ == "__main__":
    main(sys.argv[1])
