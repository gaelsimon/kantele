"""Writes the catalogue a Synology package source serves, read off the built .spk so the two can
never disagree. Package Center reads it, offers the package, and offers every later version as an
update."""
import argparse
import hashlib
import json
import os
import sys
import tarfile


def info_of(spk):
    """The INFO file inside the package, as the keys DSM reads."""
    with tarfile.open(spk) as package:
        held = package.extractfile("INFO")
        if held is None:
            sys.exit(f"{spk} carries no INFO")
        fields = {}
        for line in held.read().decode("utf-8").splitlines():
            name, _, value = line.partition("=")
            fields[name.strip()] = value.strip().strip('"')
    return fields


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("spk", help="the package the catalogue offers")
    ap.add_argument("--link", required=True, help="where that file can be downloaded")
    ap.add_argument("--thumbnail", help="an icon Package Center shows beside the name")
    ap.add_argument("--out", default="index.json")
    args = ap.parse_args()

    info = info_of(args.spk)
    with open(args.spk, "rb") as file:
        raw = file.read()

    entry = {
        "package": info["package"],
        "version": info["version"],
        "dname": info.get("displayname", info["package"]),
        "desc": info.get("description", ""),
        "price": 0,
        "download_count": 0,
        "recent_download_count": 0,
        "link": args.link,
        "size": len(raw),
        "md5": hashlib.md5(raw).hexdigest(),
        "maintainer": info.get("maintainer", ""),
        "distributor": info.get("distributor", ""),
        "thirdparty": True,
        "category": 0,
        "subcategory": 0,
        "type": 0,
        "start": True,
        # No wizard to answer, so Package Center may install, start and upgrade without stopping.
        "qinst": True,
        "qstart": True,
        "qupgrade": True,
        "beta": False,
        "depsers": "",
        "deppkgs": "",
        "conflictpkgs": "",
    }
    if args.thumbnail:
        entry["thumbnail"] = [args.thumbnail]

    with open(args.out, "w") as out:
        json.dump({"packages": [entry]}, out, indent=2)
        out.write("\n")
    print(f"== wrote {args.out}: {entry['package']} {entry['version']}, "
          f"{entry['size']} bytes, md5 {entry['md5']}")
    print(f"   {os.path.basename(args.spk)} is offered at {args.link}")


if __name__ == "__main__":
    main()
