"""Drops a file into a served folder and waits for the server to notice: the library grows, the
update id moves, subscribers are told, the new track plays, and all of it comes back when the file
goes. Run it where the folder is, which for a NAS means on the NAS."""
import argparse
import html
import http.server
import os
import re
import shutil
import socket
import sys
import threading
import time
import urllib.parse
import urllib.request

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from probe_upnp import (  # noqa: E402
    browse,
    check,
    get,
    resolve_control_url,
    result_of,
    search,
    summarise,
)


class Told:
    """A subscriber of the server's own making, to see what a device would be told."""

    def __init__(self, event_url):
        self.event_url = event_url
        self.updates = []
        self.sid = None
        probe = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        probe.connect((urllib.parse.urlparse(event_url).hostname, 80))
        self.host = probe.getsockname()[0]
        probe.close()
        keep = self.updates

        class Handler(http.server.BaseHTTPRequestHandler):
            def do_NOTIFY(self):  # noqa: N802
                body = self.rfile.read(int(self.headers.get("content-length", 0))).decode("utf-8", "replace")
                for value in re.findall(r"<SystemUpdateID>(?:<!\[CDATA\[)?(\d+)", html.unescape(body)):
                    keep.append((time.time(), int(value)))
                self.send_response(200)
                self.end_headers()

            def log_message(self, *_):
                pass

        self.server = http.server.HTTPServer((self.host, 0), Handler)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def subscribe(self, seconds=300):
        req = urllib.request.Request(self.event_url, method="SUBSCRIBE", headers={
            "CALLBACK": f"<http://{self.host}:{self.server.server_port}/>",
            "NT": "upnp:event", "TIMEOUT": f"Second-{seconds}"})
        with urllib.request.urlopen(req, timeout=10) as r:
            self.sid = r.headers.get("SID")
        if self.sid:
            threading.Thread(target=self.renewing, args=(seconds,), daemon=True).start()
        return self.sid

    def renewing(self, seconds):
        """A wait for a timed check outlives the subscription, and a dropped one is told nothing."""
        while self.sid:
            time.sleep(max(seconds / 2, 15))
            if not self.sid:
                return
            req = urllib.request.Request(self.event_url, method="SUBSCRIBE",
                                         headers={"SID": self.sid, "TIMEOUT": f"Second-{seconds}"})
            try:
                urllib.request.urlopen(req, timeout=10).close()
            except OSError:
                self.sid = None

    def after(self, when, seconds):
        deadline = time.time() + seconds
        while time.time() < deadline:
            told = [u for at, u in self.updates if at > when]
            if told:
                return told[-1]
            time.sleep(0.3)
        return None

    def close(self):
        sid, self.sid = self.sid, None
        if sid:
            req = urllib.request.Request(self.event_url, method="UNSUBSCRIBE", headers={"SID": sid})
            try:
                urllib.request.urlopen(req, timeout=5).close()
            except OSError:
                pass
        self.server.shutdown()


def status(base):
    _, body, _ = get(f"{base}/api/status")
    out = {}
    for line in body.decode("utf-8", "replace").splitlines():
        name, _, value = line.partition(" = ")
        out[name.strip()] = value.strip()
    return out


def tracks(base):
    return int(status(base).get("tracks", "-1"))


def update_id(cd):
    _, resp, _ = browse(cd, "0", count=1)
    _, fields = result_of(resp)
    return fields.get("UpdateID")


def wait_for_tracks(base, wanted, seconds, every):
    deadline = time.time() + seconds
    held = None
    while time.time() < deadline:
        held = tracks(base)
        if held == wanted:
            return held, time.time()
        time.sleep(every)
    return held, None


def likely_title(path):
    """A file named for its track number is not named for its title."""
    stem = os.path.splitext(os.path.basename(path))[0]
    return re.sub(r"^[\d\-. ]+", "", stem).strip()[:24]


def first_url(didl):
    found = re.search(r"<res\b[^>]*>(.*?)</res>", didl or "", re.S)
    return html.unescape(found.group(1)).strip() if found else None


def reachable(cd, path):
    """The dropped track, found by its title or, where the tags name it otherwise, as the newest."""
    name = likely_title(path)
    _, resp, _ = search(cd, "0", f'dc:title contains "{name}"', count=5)
    didl, fields = result_of(resp)
    found = fields.get("TotalMatches", "0") != "0"
    check("it can be searched for", found, f"{fields.get('TotalMatches')} for {name!r}")
    if not found:
        _, resp, _ = browse(cd, "0", count=50)
        root, _ = result_of(resp)
        recent = next((m.group(1) for m in re.finditer(r"<container\b[^>]*\bid=\"([^\"]+)\".*?</container>",
                                                       root or "", re.S) if "recent" in m.group(0).lower()), None)
        if not check("the newest can be reached another way", recent is not None):
            return
        _, resp, _ = browse(cd, recent, count=5)
        didl, _ = result_of(resp)
    url = first_url(didl)
    if check("it carries a URL", url is not None):
        status, _, headers = get(url, headers={"Range": "bytes=0-1023"}, limit=2048)
        check("it plays", status == 206, f"HTTP {status} {headers.get('content-range', '')}")


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--base", default="http://127.0.0.1:8200", help="where the server answers")
    ap.add_argument("--folder", required=True, help="a folder the server is serving, written into")
    ap.add_argument("--file", required=True, help="the audio file to drop in, copied from where it is")
    ap.add_argument("--patience", type=float, default=90.0, help="how long the server may take to notice")
    ap.add_argument("--subscription", type=float, default=300.0,
                    help="seconds a subscription is asked for, renewed at half of it while it waits")
    ap.add_argument("--every", type=float, default=0.5,
                    help="seconds between asks, which a wait for a timed check wants longer")
    args = ap.parse_args()

    _, desc, _ = get(f"{args.base}/description.xml")
    cd = resolve_control_url(desc, args.base)
    event_url = urllib.parse.urljoin(args.base + "/", "event/ContentDirectory")
    told = Told(event_url)
    if not check("the server takes a subscription", told.subscribe(args.subscription) is not None):
        summarise()
        return

    before, before_id = tracks(args.base), update_id(cd)
    print(f"  {before} tracks, update id {before_id}")

    dropped = os.path.join(args.folder, f"_kantele-probe-{os.getpid()}")
    os.makedirs(dropped, exist_ok=True)
    landed = os.path.join(dropped, os.path.basename(args.file))
    print(f"\n== dropping {os.path.basename(args.file)} into {dropped} ==")
    began = time.time()
    shutil.copy2(args.file, landed)

    held, at = wait_for_tracks(args.base, before + 1, args.patience, args.every)
    if check("the new file is in the library", at is not None, f"{held} tracks, was {before}"):
        print(f"  it took {at - began:.1f}s")
    after_id = update_id(cd)
    check("the update id moved", after_id != before_id, f"{before_id} then {after_id}")
    check("a subscriber is told", told.after(began, 10) is not None,
          f"{len(told.updates)} notifications in all")

    reachable(cd, args.file)

    print("\n== taking it away again ==")
    went = time.time()
    shutil.rmtree(dropped)
    held, at = wait_for_tracks(args.base, before, args.patience, args.every)
    if check("the library goes back to what it was", at is not None, f"{held} tracks, was {before}"):
        print(f"  it took {at - went:.1f}s")
    check("a subscriber is told again", told.after(went, 10) is not None,
          f"{len(told.updates)} notifications in all")

    told.close()
    summarise()


if __name__ == "__main__":
    main()
