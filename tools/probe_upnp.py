"""Generic UPnP AV probe: discovers a MediaServer, walks ContentDirectory to a track, checks a
Range GET, and prints PASS/FAIL — the automated stand-in for pointing a real control point at it."""
import argparse
import contextlib
import html
import os
import re
import socket
import sys
import tempfile
import urllib.error
import urllib.parse
import urllib.request
import xml.dom.minidom

MCAST = ("239.255.255.250", 1900)
DEVICE_TARGET = "urn:schemas-upnp-org:device:MediaServer:1"
CD_NS = "urn:schemas-upnp-org:service:ContentDirectory:1"

RESULTS = []


def check(label, ok, detail=""):
    RESULTS.append((label, ok))
    mark = "PASS" if ok else "FAIL"
    print(f"  [{mark}] {label}" + (f" — {detail}" if detail else ""))
    return ok


def write(out_dir, name, data):
    path = os.path.join(out_dir, name)
    with open(path, "wb" if isinstance(data, bytes) else "w") as f:
        f.write(data)
    return path


def pretty(xml_text):
    try:
        return xml.dom.minidom.parseString(xml_text).toprettyxml(indent="  ")
    except Exception:
        return xml_text


def msearch(target=DEVICE_TARGET, timeout=4):
    req = (
        "M-SEARCH * HTTP/1.1\r\n"
        f"HOST: {MCAST[0]}:{MCAST[1]}\r\n"
        'MAN: "ssdp:discover"\r\n'
        "MX: 3\r\n"
        f"ST: {target}\r\n"
        "\r\n"
    )
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    s.setsockopt(socket.IPPROTO_IP, socket.IP_MULTICAST_TTL, 2)
    s.settimeout(timeout)
    s.sendto(req.encode(), MCAST)
    found = []
    try:
        while True:
            data, addr = s.recvfrom(65535)
            text = data.decode("utf-8", "replace")
            loc = next(
                (l.split(":", 1)[1].strip() for l in text.split("\r\n") if l.lower().startswith("location:")),
                None,
            )
            if loc:
                found.append((addr[0], loc))
    except socket.timeout:
        pass
    s.close()
    return found


def get(url, headers=None, method="GET", limit=None):
    """Status, body and headers; a body read up to `limit` bytes when the rest is not wanted.
    A server that cannot be reached is status 0 with the reason where the body would be."""
    req = urllib.request.Request(url, headers={"User-Agent": "kantele-probe/0", **(headers or {})}, method=method)
    try:
        with urllib.request.urlopen(req, timeout=15) as r:
            return r.status, r.read(limit), {k.lower(): v for k, v in r.headers.items()}
    except urllib.error.HTTPError as e:
        return e.code, e.read(), {k.lower(): v for k, v in e.headers.items()}
    except (urllib.error.URLError, OSError) as e:
        return 0, str(e).encode(), {}


def soap(control_url, action, body_xml, ns=CD_NS):
    envelope = (
        '<?xml version="1.0" encoding="utf-8"?>\n'
        '<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" '
        's:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">\n'
        f" <s:Body>\n{body_xml}\n </s:Body>\n</s:Envelope>\n"
    )
    req = urllib.request.Request(
        control_url,
        data=envelope.encode(),
        headers={
            "Content-Type": 'text/xml; charset="utf-8"',
            "SOAPACTION": f'"{ns}#{action}"',
            "User-Agent": "kantele-probe/0",
            "Connection": "close",
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=25) as r:
            return envelope, r.read(), r.status
    except urllib.error.HTTPError as e:
        return envelope, e.read(), e.code
    except (urllib.error.URLError, OSError) as e:
        return envelope, str(e).encode(), 0


def esc(s):
    return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def browse(control_url, object_id, flag="BrowseDirectChildren", count=20, filt="*", start=0):
    body = (
        f'  <u:Browse xmlns:u="{CD_NS}">\n'
        f"   <ObjectID>{esc(object_id)}</ObjectID>\n"
        f"   <BrowseFlag>{flag}</BrowseFlag>\n"
        f"   <Filter>{esc(filt)}</Filter>\n"
        f"   <StartingIndex>{start}</StartingIndex>\n"
        f"   <RequestedCount>{count}</RequestedCount>\n"
        "   <SortCriteria></SortCriteria>\n  </u:Browse>"
    )
    return soap(control_url, "Browse", body)


def search(control_url, cid, criteria, count=10, filt="*"):
    body = (
        f'  <u:Search xmlns:u="{CD_NS}">\n'
        f"   <ContainerID>{esc(cid)}</ContainerID>\n"
        f"   <SearchCriteria>{esc(criteria)}</SearchCriteria>\n"
        f"   <Filter>{esc(filt)}</Filter>\n"
        "   <StartingIndex>0</StartingIndex>\n"
        f"   <RequestedCount>{count}</RequestedCount>\n"
        "   <SortCriteria></SortCriteria>\n  </u:Search>"
    )
    return soap(control_url, "Search", body)


def result_of(resp_bytes):
    text = resp_bytes.decode("utf-8", "replace")
    fields = {}
    for tag in ("NumberReturned", "TotalMatches", "UpdateID", "errorCode", "errorDescription"):
        m = re.search(rf"<{tag}>(.*?)</{tag}>", text, re.S)
        if m:
            fields[tag] = m.group(1)
    a, b = text.find("<Result>"), text.find("</Result>")
    didl = html.unescape(text[a + 8 : b]) if a >= 0 and b >= 0 else None
    return didl, fields


def containers_of(didl):
    return re.findall(r'<container\b[^>]*\bid="([^"]+)"', didl or "")


def first_item_with_res(didl):
    for m in re.finditer(r"<item\b.*?</item>", didl or "", re.S):
        block = m.group(0)
        iid = re.search(r'\bid="([^"]+)"', block)
        res = re.search(r"<res\b[^>]*>(.*?)</res>", block, re.S)
        if iid and res:
            return iid.group(1), html.unescape(res.group(1)).strip()
    return None, None


def safe(name):
    return re.sub(r"[^A-Za-z0-9._-]+", "_", name)[:80]


def resolve_control_url(desc_bytes, base_url, service_type_substr="ContentDirectory"):
    text = desc_bytes.decode("utf-8", "replace")
    for m in re.finditer(r"<service>(.*?)</service>", text, re.S):
        block = m.group(1)
        if service_type_substr in block:
            cm = re.search(r"<controlURL>(.*?)</controlURL>", block, re.S)
            if cm:
                return urllib.parse.urljoin(base_url + "/", cm.group(1).strip().lstrip("/"))
    return None


def discover(args):
    if args.desc_url:
        return args.desc_url
    print("== SSDP discovery ==")
    found = msearch(timeout=args.timeout)
    for ip, loc in found:
        print(f"  {ip} -> {loc}")
    if not found:
        return None

    candidates = []
    for ip, loc in found:
        try:
            _, desc, _ = get(loc)
        except Exception:
            continue
        fn = re.search(r"<friendlyName>(.*?)</friendlyName>", desc.decode("utf-8", "replace"), re.S)
        name = fn.group(1) if fn else ""
        candidates.append((loc, name))
        print(f"    {name!r} at {loc}")

    if args.name:
        matches = [loc for loc, name in candidates if args.name.lower() in name.lower()]
        if not matches:
            print(f"no MediaServer with friendlyName containing {args.name!r}")
            return None
        return matches[0]

    if len(candidates) == 1:
        return candidates[0][0]
    if candidates:
        print("several MediaServers answered; pass --name to pick one")
    return None


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--name", help="friendlyName substring to pick among SSDP responders")
    ap.add_argument("--desc-url", help="skip SSDP, fetch this description.xml directly")
    # Outside the tree: the documented command is run from the root of a clone.
    ap.add_argument("--out", default=os.path.join(tempfile.gettempdir(), "probe-upnp"),
                    help="where the exchanges are written (default: under the temporary folder)")
    ap.add_argument("--timeout", type=float, default=4.0)
    ap.add_argument("--max-depth", type=int, default=6, help="container hops while hunting a track")
    ap.add_argument("--print-control", action="store_true",
                    help="print the ContentDirectory control URL and nothing else, for $(…)")
    args = ap.parse_args()

    if not args.print_control:
        os.makedirs(args.out, exist_ok=True)

    # Everything discovery says is commentary, and in --print-control the only thing on stdout
    # has to be the URL, since the documented use is $(…).
    if args.print_control:
        with contextlib.redirect_stdout(sys.stderr):
            loc = discover(args)
    else:
        loc = discover(args)
    if not loc:
        print("no device to probe", file=sys.stderr)
        sys.exit(2)

    if args.print_control:
        status, desc, _ = get(loc)
        if status != 200:
            sys.exit(f"the description at {loc} answered {status}")
        parsed = urllib.parse.urlparse(loc)
        found = resolve_control_url(desc, f"{parsed.scheme}://{parsed.netloc}")
        if not found:
            sys.exit(f"{loc} advertises no ContentDirectory")
        print(found)
        return

    print(f"\n== device description: {loc} ==")
    status, desc, _ = get(loc)
    write(args.out, "device-description.xml", desc)
    check("device description reachable", status == 200, f"HTTP {status}" if status else desc.decode("utf-8", "replace"))

    text = desc.decode("utf-8", "replace")
    fn = re.search(r"<friendlyName>(.*?)</friendlyName>", text, re.S)
    udn = re.search(r"<UDN>(.*?)</UDN>", text, re.S)
    print(f"  friendlyName: {fn.group(1) if fn else '?'}")
    print(f"  UDN: {udn.group(1) if udn else '?'}")

    base = f"{urllib.parse.urlparse(loc).scheme}://{urllib.parse.urlparse(loc).netloc}"
    control_url = resolve_control_url(desc, base)
    print(f"  ContentDirectory controlURL: {control_url}")
    if not check("ContentDirectory service advertised", control_url is not None):
        summarise(args.out)
        return

    print("\n== Browse root (BrowseDirectChildren) ==")
    env, resp, status = browse(control_url, "0", count=50)
    write(args.out, "browse-root.request.xml", env)
    write(args.out, "browse-root.response.xml", resp)
    check("Browse root: HTTP 200", status == 200, f"HTTP {status}")
    didl, fields = result_of(resp)
    print(f"  {fields}")
    if didl:
        write(args.out, "browse-root.didl.xml", pretty(didl))
    check("Browse root: DIDL present", didl is not None)
    if fields.get("NumberReturned") and fields.get("TotalMatches"):
        check(
            "Browse root: NumberReturned <= TotalMatches",
            int(fields["NumberReturned"]) <= int(fields["TotalMatches"]),
            str(fields),
        )

    print("\n== BrowseMetadata root ==")
    env, resp, status = browse(control_url, "0", flag="BrowseMetadata", count=1)
    write(args.out, "browsemetadata-root.request.xml", env)
    write(args.out, "browsemetadata-root.response.xml", resp)
    _, meta_fields = result_of(resp)
    check("BrowseMetadata root: HTTP 200", status == 200, f"HTTP {status}")
    check("BrowseMetadata root: NumberReturned == 1", meta_fields.get("NumberReturned") == "1", str(meta_fields))

    if fields.get("TotalMatches") and int(fields["TotalMatches"]) > 1:
        print("\n== Browse root, page 2 (pagination) ==")
        env, resp, status = browse(control_url, "0", count=1, start=1)
        write(args.out, "browse-root-page2.request.xml", env)
        write(args.out, "browse-root-page2.response.xml", resp)
        _, page_fields = result_of(resp)
        check("Browse root page 2: HTTP 200", status == 200, f"HTTP {status}")
        check("Browse root page 2: NumberReturned == 1", page_fields.get("NumberReturned") == "1", str(page_fields))

    print("\n== Walking to a track ==")
    item_id, res_url, hops = walk_to_a_track(control_url, didl, args)
    check("found a track with a <res> URL", item_id is not None, f"after {hops} browse(s)")

    if res_url:
        print(f"\n== GET {res_url} ==")
        # The first bytes are enough to know the file is served; the whole of a DSD is not wanted.
        status, _, headers = get(res_url, limit=1)
        check("plain GET: HTTP 200", status == 200, f"HTTP {status}")
        check("plain GET: Content-Length present", "content-length" in headers)

        status, body, headers = get(res_url, headers={"Range": "bytes=0-65535"})
        check("Range GET: HTTP 206", status == 206, f"HTTP {status}")
        check("Range GET: Content-Range present", "content-range" in headers, str(headers.get("content-range")))
        check("Range GET: body truncated to the range", len(body) <= 65536, f"{len(body)} bytes")

    print("\n== GetSearchCapabilities / Search ==")
    env, resp, status = soap(control_url, "GetSearchCapabilities", f'  <u:GetSearchCapabilities xmlns:u="{CD_NS}"/>')
    write(args.out, "getsearchcapabilities.request.xml", env)
    write(args.out, "getsearchcapabilities.response.xml", resp)
    caps_m = re.search(r"<SearchCaps>(.*?)</SearchCaps>", resp.decode("utf-8", "replace"), re.S)
    caps = caps_m.group(1) if caps_m else ""
    print(f"  SearchCaps: {caps!r}")

    if caps.strip():
        env, resp, status = search(control_url, "0", 'upnp:class derivedfrom "object.item.audioItem"')
        write(args.out, "search-audioitem.request.xml", env)
        write(args.out, "search-audioitem.response.xml", resp)
        _, s_fields = result_of(resp)
        check("Search audioItem: HTTP 200", status == 200, f"HTTP {status}")
        check("Search audioItem: no errorCode", "errorCode" not in s_fields, str(s_fields))
    else:
        print("  Search not advertised, skipped")

    summarise(args.out)


MAX_BROWSES = 40


def walk_to_a_track(control_url, root_didl, args):
    """Depth first through the containers, every branch in turn, until an item carries a <res>.
    A generic server may open on a branch with no tracks in it, so the first is not the only one tried."""
    item_id, res_url = first_item_with_res(root_didl)
    if item_id:
        return item_id, res_url, 0
    hops = 0
    pending = [(cid, 1) for cid in reversed(containers_of(root_didl))]
    while pending and hops < MAX_BROWSES:
        cid, depth = pending.pop()
        hops += 1
        print(f"  [{depth}] Browse {cid!r}")
        env, resp, status = browse(control_url, cid, count=50)
        write(args.out, f"walk-{hops:02d}-{safe(cid)}.request.xml", env)
        write(args.out, f"walk-{hops:02d}-{safe(cid)}.response.xml", resp)
        cur_didl, _ = result_of(resp)
        if cur_didl:
            write(args.out, f"walk-{hops:02d}-{safe(cid)}.didl.xml", pretty(cur_didl))
        item_id, res_url = first_item_with_res(cur_didl)
        if item_id:
            return item_id, res_url, hops
        if depth < args.max_depth:
            pending.extend((child, depth + 1) for child in reversed(containers_of(cur_didl)))
    return None, None, hops


def summarise(out=None):
    passed = sum(1 for _, ok in RESULTS if ok)
    failed = [label for label, ok in RESULTS if not ok]
    if out:
        print(f"\nevery exchange is under {out}")
    print(f"\n{passed}/{len(RESULTS)} checks passed")
    if failed:
        print("failed:")
        for label in failed:
            print(f"  - {label}")
        sys.exit(1)


if __name__ == "__main__":
    main()
