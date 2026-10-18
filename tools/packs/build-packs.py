#!/usr/bin/env python3
"""Builds LIBRARY packs from packs.json.in and a content tree (laid out as
~/ZeroTracker), plus packs.json.

A pack with "source": {"type": "files", "base": URL} is downloaded file by
file from its original host: the catalog lists each file's URL (base plus the
file's path under the pack's folder), its install path, size and SHA-256, and
no zip is made. --check-remote fetches each such URL and compares the hash.
Other packs become one zip each, hosted with the catalog.

usage: build-packs.py CONTENT_DIR OUT_DIR [--base-url URL]
WAV files are stored, other files deflated. The songs, instruments and
samples the .deb carries are not in any pack.
"""
import argparse, fnmatch, hashlib, json, os, pathlib, sys, time, urllib.error, urllib.parse, urllib.request, zipfile

def fetch(url, tries=4):
    """The body at url; archive.org answers 5xx now and then."""
    for attempt in range(tries):
        try:
            with urllib.request.urlopen(url, timeout=60) as r:
                return r.read()
        except urllib.error.HTTPError as e:
            if e.code < 500 or attempt == tries - 1:
                sys.exit(f"{e.code} {url}")
            time.sleep(3 * (attempt + 1))

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("content")
    ap.add_argument("out")
    ap.add_argument("--base-url")
    ap.add_argument("--check-remote", action="store_true")
    args = ap.parse_args()
    here = pathlib.Path(__file__).resolve().parent
    spec = json.loads((here / "packs.json.in").read_text())
    content = pathlib.Path(args.content).resolve()
    out = pathlib.Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    base = args.base_url or spec["base_url"]
    catalog = {"version": 1, "packs": []}
    for pack in spec["packs"]:
        files = []
        for rel in pack["paths"]:
            root = content / rel
            if not root.exists():
                sys.exit(f"missing {root}")
            # "exclude": globs under the path, for files whose origin is in doubt.
            skip = [g for g in pack.get("exclude", [])]
            files += sorted(p for p in root.rglob("*") if p.is_file() and not p.is_symlink()
                            and not any(fnmatch.fnmatchcase(p.relative_to(root).as_posix(), g) for g in skip))
        entry = {k: pack[k] for k in ("id", "name", "kind", "description", "authors", "licence", "licence_url")}
        count = lambda ext: sum(1 for p in files if p.suffix.lower() == ext)
        entry.update(songs=count(".m8s"), instruments=count(".m8i"), samples=count(".wav"))
        source = pack.get("source", {"type": "zip"})
        if source["type"] == "files":
            listed = []
            for p in files:
                under = p.relative_to(content / pack["paths"][0]).as_posix()
                url = source["base"] + urllib.parse.quote(under)
                digest = hashlib.sha256(p.read_bytes()).hexdigest()
                if args.check_remote:
                    remote = fetch(url)
                    if hashlib.sha256(remote).hexdigest() != digest:
                        sys.exit(f"remote differs: {url}")
                listed.append({"url": url, "path": p.relative_to(content).as_posix(),
                               "bytes": p.stat().st_size, "sha256": digest})
            total = sum(f["bytes"] for f in listed)
            entry.update(source="files", files=listed, bytes=total, unpacked_bytes=total)
            catalog["packs"].append(entry)
            print(f"{pack['id']:24} {len(files):5} files {total/1e6:8.1f} MB  (from {source['base'].split('/')[2]})")
            continue
        zpath = out / f"{pack['id']}.zip"
        unpacked = 0
        with zipfile.ZipFile(zpath, "w") as z:
            for p in files:
                name = p.relative_to(content).as_posix()
                info = zipfile.ZipInfo.from_file(p, name)
                info.external_attr = 0o644 << 16
                kind = zipfile.ZIP_STORED if p.suffix.lower() == ".wav" else zipfile.ZIP_DEFLATED
                with open(p, "rb") as f:
                    z.writestr(info, f.read(), compress_type=kind)
                unpacked += p.stat().st_size
        digest = hashlib.sha256(zpath.read_bytes()).hexdigest()
        entry.update(source="zip", url=base + zpath.name, bytes=zpath.stat().st_size,
                     unpacked_bytes=unpacked, sha256=digest)
        catalog["packs"].append(entry)
        print(f"{pack['id']:24} {len(files):5} files {zpath.stat().st_size/1e6:8.1f} MB")
    (out / "packs.json").write_text(json.dumps(catalog, indent=2) + "\n")

if __name__ == "__main__":
    main()
