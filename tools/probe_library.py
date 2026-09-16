"""What the first probe leaves out, asked of a whole library: every menu, the far end of the longest
list, the A-Z index, playlists, cover art, each search capability the server declares, and several
clients at once."""
import argparse
import concurrent.futures
import html
import os
import re
import sys
import time
import urllib.parse

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from probe_upnp import (  # noqa: E402
    browse,
    check,
    discover,
    get,
    resolve_control_url,
    result_of,
    search,
    soap,
    summarise,
)

CD_NS = "urn:schemas-upnp-org:service:ContentDirectory:1"
AT_ONCE = 8


def titles(didl):
    return [html.unescape(t) for t in re.findall(r"<dc:title>(.*?)</dc:title>", didl or "", re.S)]


def named_containers(didl):
    out = []
    for m in re.finditer(r"<container\b[^>]*\bid=\"([^\"]+)\".*?</container>", didl or "", re.S):
        title = re.search(r"<dc:title>(.*?)</dc:title>", m.group(0), re.S)
        out.append((m.group(1), html.unescape(title.group(1)) if title else "?"))
    return out


def ask(control_url, object_id, start=0, count=20, label=None):
    began = time.time()
    _, resp, status = browse(control_url, object_id, start=start, count=count)
    didl, fields = result_of(resp)
    took = time.time() - began
    if label:
        ok = status == 200 and "errorCode" not in fields
        check(label, ok,
              f"{fields.get('NumberReturned', '?')} of {fields.get('TotalMatches', '?')} in {took:.2f}s"
              if ok else f"HTTP {status} {fields}")
    return didl, fields, took


def every_menu(cd):
    print("== every menu on the root ==")
    didl, _, _ = ask(cd, "0", count=50, label="the root lists its menus")
    menus = named_containers(didl)
    print(f"  {[name for _, name in menus]}")
    slowest = (0.0, "")
    for object_id, name in menus:
        _, _, took = ask(cd, object_id, count=20, label=f"menu {name!r} answers")
        slowest = max(slowest, (took, name))
    print(f"  slowest: {slowest[1]!r} at {slowest[0]:.2f}s")
    return menus


def the_far_end(cd, menus):
    print("\n== the far end of the longest list ==")
    longest, total = None, 0
    for object_id, name in menus:
        _, fields, _ = ask(cd, object_id, count=1)
        held = int(fields.get("TotalMatches", "0") or 0)
        if held > total:
            longest, total = (object_id, name), held
    if not check("a list long enough to page through", total > 100, f"the longest holds {total}"):
        return
    object_id, name = longest
    print(f"  {name!r} holds {total}")
    _, last, _ = ask(cd, object_id, start=max(0, total - 5), count=20, label="the last page answers")
    check("the last page holds what is left", last.get("NumberReturned") not in (None, "0"), str(last))
    _, _, took = ask(cd, object_id, start=total // 2, count=100, label="a hundred from the middle answer")
    check("a hundred rows are not slower than a page", took < 1.0, f"{took:.2f}s")


def the_alphabet(cd, menus):
    print("\n== the A-Z index ==")
    for object_id, name in menus:
        didl, _, _ = ask(cd, object_id, count=60)
        groups = [t for t in titles(didl) if re.fullmatch(r"[A-Z0-9#][-–—]?[A-Z0-9#]?", t.strip())]
        if len(groups) >= 3:
            print(f"  {name!r} carries {groups[:8]}")
            check("a long list carries an A-Z index", True)
            ask(cd, named_containers(didl)[0][0], count=10, label=f"a group of {name!r} opens")
            return
    check("a long list carries an A-Z index", False, "no menu offered one")


def the_playlists(cd, menus):
    print("\n== playlists ==")
    where = next((i for i, name in menus if "playlist" in name.lower()), None)
    if not check("the library offers playlists", where is not None):
        return
    didl, fields, _ = ask(cd, where, count=20, label="the playlist menu answers")
    print(f"  {fields.get('TotalMatches')} playlists")
    first = named_containers(didl)
    if not check("a playlist can be opened", bool(first)):
        return
    inside, held, _ = ask(cd, first[0][0], count=20, label=f"playlist {first[0][1]!r} lists its tracks")
    urls = [html.unescape(u).strip() for u in re.findall(r"<res\b[^>]*>(.*?)</res>", inside or "", re.S)]
    check("its tracks carry a URL", bool(urls), f"{len(urls)} of {held.get('TotalMatches')}")
    if urls:
        status, _, headers = get(urls[0], headers={"Range": "bytes=0-1023"}, limit=2048)
        check("a track of that playlist plays", status == 206, f"HTTP {status} {headers.get('content-range', '')}")


def the_cover_art(cd, menus):
    print("\n== cover art ==")
    art, looked = None, 0
    for object_id, _ in menus:
        didl, _, _ = ask(cd, object_id, count=40)
        for cid, _ in named_containers(didl)[:6]:
            inner, _, _ = ask(cd, cid, count=20)
            looked += 1
            found = re.search(r"<upnp:albumArtURI[^>]*>(.*?)</upnp:albumArtURI>", inner or "", re.S)
            if found:
                art = html.unescape(found.group(1)).strip()
                break
        if art:
            break
    if not check("an album offers a cover", art is not None, art or f"none in {looked} listings"):
        return
    status, _, headers = get(art, limit=4096)
    check("the cover is served", status == 200, f"HTTP {status}")
    check("it is an image", headers.get("content-type", "").startswith("image/"),
          headers.get("content-type", "none"))


def every_capability(cd):
    print("\n== every search capability the server declares ==")
    _, answer, _ = soap(cd, "GetSearchCapabilities", f'  <u:GetSearchCapabilities xmlns:u="{CD_NS}" />')
    declared = re.search(r"<SearchCaps>(.*?)</SearchCaps>", answer.decode("utf-8", "replace"), re.S)
    if not check("the server declares what it can search", declared is not None):
        return
    for capability in html.unescape(declared.group(1)).split(","):
        capability = capability.strip()
        if not capability or capability == "@refID":
            continue
        criteria = ('upnp:class derivedfrom "object.item.audioItem"' if capability == "upnp:class"
                    else f'{capability} contains "a"')
        _, resp, status = search(cd, "0", criteria, count=5)
        _, fields = result_of(resp)
        check(f"search on {capability}", status == 200 and "errorCode" not in fields,
              f"{fields.get('TotalMatches', '?')} matches" if "errorCode" not in fields else str(fields))


def all_at_once(cd, menus):
    print(f"\n== {AT_ONCE} clients at once ==")
    began = time.time()
    with concurrent.futures.ThreadPoolExecutor(max_workers=AT_ONCE) as pool:
        answers = list(pool.map(lambda i: browse(cd, menus[i % len(menus)][0], count=20), range(AT_ONCE)))
    answered = sum(1 for a in answers if a[2] == 200)
    check(f"{AT_ONCE} browses at once all answer", answered == AT_ONCE,
          f"{answered} of {AT_ONCE} in {time.time() - began:.2f}s")


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--name", help="friendlyName substring to pick among SSDP responders")
    ap.add_argument("--desc-url", help="skip SSDP, fetch this description.xml directly")
    ap.add_argument("--timeout", type=float, default=4.0)
    args = ap.parse_args()

    loc = discover(args)
    if not loc:
        print("no device to probe", file=sys.stderr)
        sys.exit(2)
    status, desc, _ = get(loc)
    if status != 200:
        sys.exit(f"the description at {loc} answered {status}")
    parsed = urllib.parse.urlparse(loc)
    cd = resolve_control_url(desc, f"{parsed.scheme}://{parsed.netloc}")
    if not check("ContentDirectory service advertised", cd is not None):
        summarise()
        return
    print(f"\n== {loc} ==")

    menus = every_menu(cd)
    if not menus:
        summarise()
        return
    the_far_end(cd, menus)
    the_alphabet(cd, menus)
    the_playlists(cd, menus)
    the_cover_art(cd, menus)
    every_capability(cd)
    all_at_once(cd, menus)
    summarise()


if __name__ == "__main__":
    main()
