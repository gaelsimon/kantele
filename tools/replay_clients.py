"""Replays the control exchanges a real client made, captured by `capture_dir`, against a server and
diffs each answer with what the client was given."""
import argparse
import html
import os
import re
import sys
import urllib.error
import urllib.request
import xml.etree.ElementTree as ET

SKIPPED_HEADERS = {"host", "content-length", "connection", "accept-encoding"}
PORTABLE_IDS = {"0"}


class Recorded:
    def __init__(self, request_path):
        self.request_path = request_path
        self.name = os.path.basename(request_path)[: -len(".request.txt")]
        with open(request_path, "rb") as f:
            raw = f.read()
        head, _, self.body = raw.partition(b"\n\n")
        lines = head.decode("utf-8", "replace").split("\n")
        self.method, self.path = lines[0].split(" ", 1)
        self.headers = {}
        for line in lines[1:]:
            name, _, value = line.partition(": ")
            self.headers[name] = value
        folder = os.path.dirname(request_path)
        answers = [f for f in os.listdir(folder) if f.startswith(self.name + ".") and f.endswith(".response.xml")]
        self.status = int(answers[0].split(".")[-3]) if answers else None
        self.response = open(os.path.join(folder, answers[0]), "rb").read() if answers else b""

    @property
    def action(self):
        return self.name.split("-", 1)[1]

    @property
    def object_id(self):
        m = re.search(r"<(?:ObjectID|ContainerID)>(.*?)</(?:ObjectID|ContainerID)>", self.body.decode("utf-8", "replace"))
        return html.unescape(m.group(1)) if m else None

    @property
    def portable(self):
        return self.object_id in PORTABLE_IDS or self.object_id is None


def send(base, recorded):
    headers = {k: v for k, v in recorded.headers.items() if k.lower() not in SKIPPED_HEADERS}
    req = urllib.request.Request(base.rstrip("/") + recorded.path, data=recorded.body, headers=headers, method=recorded.method)
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            return r.status, r.read()
    except urllib.error.HTTPError as e:
        return e.code, e.read()
    except Exception as e:
        return 0, f"<!-- {type(e).__name__}: {e} -->".encode()


def fields_of(resp_bytes):
    text = resp_bytes.decode("utf-8", "replace")
    out = {}
    for tag in ("NumberReturned", "TotalMatches", "errorCode", "SearchCaps", "SortCaps", "Source"):
        m = re.search(rf"<{tag}>(.*?)</{tag}>", text, re.S)
        if m:
            out[tag] = m.group(1)
    a, b = text.find("<Result>"), text.find("</Result>")
    didl = html.unescape(text[a + 8 : b]) if a >= 0 and b >= 0 else None
    return out, didl


HOST = re.compile(r"http://[^/\s\"']+")


def canonical(didl):
    try:
        root = ET.fromstring(didl)
    except ET.ParseError as e:
        return [f"<unparseable: {e}>"]
    lines = []

    def walk(el, path):
        tag = el.tag.split("}")[-1]
        here = f"{path}/{tag}"
        attrs = " ".join(f'{k.split("}")[-1]}="{HOST.sub("http://HOST", v)}"' for k, v in sorted(el.attrib.items()))
        text = HOST.sub("http://HOST", (el.text or "").strip())
        lines.append(f"{here} [{attrs}] {text}")
        for child in el:
            walk(child, here)

    walk(root, "")
    return lines


def first_difference(a, b):
    for i, (x, y) in enumerate(zip(a, b)):
        if x != y:
            return f"line {i}: recorded {x!r} / replayed {y!r}"
    if len(a) != len(b):
        return f"recorded has {len(a)} DIDL lines, replayed {len(b)}"
    return None


def compare(recorded, status, resp):
    problems = []
    if status != recorded.status:
        problems.append(f"HTTP {recorded.status} recorded, {status} replayed")
    was, was_didl = fields_of(recorded.response)
    now, now_didl = fields_of(resp)
    for tag in ("NumberReturned", "TotalMatches", "errorCode", "SearchCaps", "SortCaps", "Source"):
        if was.get(tag) != now.get(tag):
            problems.append(f"{tag}: {was.get(tag)!r} -> {now.get(tag)!r}")
    if (was_didl is None) != (now_didl is None):
        problems.append("one answer has a DIDL Result and the other has none")
    elif was_didl is not None:
        diff = first_difference(canonical(was_didl), canonical(now_didl))
        if diff:
            problems.append(diff)
    return problems


def titles_of(didl):
    if not didl:
        return []
    out = []
    for m in re.finditer(r"<(container|item)\b.*?</\1>", didl, re.S):
        block = m.group(0)
        title = re.search(r"<dc:title>(.*?)</dc:title>", block, re.S)
        klass = re.search(r"<upnp:class>(.*?)</upnp:class>", block, re.S)
        out.append(f"{klass.group(1) if klass else '?'}  {html.unescape(title.group(1)) if title else '?'}")
    return out


def recordings(capture_dir, peer):
    for folder in sorted(os.listdir(capture_dir)):
        if peer and folder != peer:
            continue
        full = os.path.join(capture_dir, folder)
        if not os.path.isdir(full):
            continue
        requests = [name for name in os.listdir(full) if name.endswith(".request.txt")]
        for name in sorted(requests, key=lambda name: int(name.split("-", 1)[0])):
            yield folder, Recorded(os.path.join(full, name))


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("capture_dir", help="the folder capture_dir / KANTELE_CAPTURE wrote")
    ap.add_argument("--target", default="http://127.0.0.1:8200", help="server to replay against")
    ap.add_argument("--reference", help="a second server, asked the portable requests only (root and Search)")
    ap.add_argument("--peer", help="one client folder, instead of all")
    args = ap.parse_args()

    failures = 0
    total = 0
    for peer, rec in recordings(args.capture_dir, args.peer):
        total += 1
        agent = rec.headers.get("user-agent", rec.headers.get("User-Agent", "?"))
        status, resp = send(args.target, rec)
        problems = compare(rec, status, resp)
        mark = "same" if not problems else "DIFF"
        oid = f" {rec.object_id!r}" if rec.object_id is not None else ""
        print(f"{peer} {rec.name}{oid}  [{agent}]  {mark}")
        for p in problems:
            print(f"    {p}")
        failures += bool(problems)

        if args.reference and rec.portable:
            ref_status, ref_resp = send(args.reference, rec)
            ref_fields, ref_didl = fields_of(ref_resp)
            now_fields, now_didl = fields_of(resp)
            print(f"    reference HTTP {ref_status} {ref_fields}  /  target {now_fields}")
            for label, didl in (("reference", ref_didl), ("target", now_didl)):
                for line in titles_of(didl)[:12]:
                    print(f"      {label}: {line}")

    print(f"\n{total - failures}/{total} exchanges answered as recorded")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
