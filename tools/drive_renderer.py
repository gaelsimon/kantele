"""Drives a real renderer end to end: takes a track from a server, hands it to an amplifier over
AVTransport, plays it, seeks, and reads the position back — the automated stand-in for a remote
control. It makes sound in the room."""
import argparse
import html
import http.server
import os
import re
import socket
import sys
import tempfile
import threading
import time
import urllib.parse
import urllib.request

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from probe_upnp import (  # noqa: E402
    browse,
    check,
    containers_of,
    esc,
    get,
    msearch,
    resolve_control_url,
    result_of,
    soap,
    summarise,
    write,
)

RENDERER_TARGET = "urn:schemas-upnp-org:device:MediaRenderer:1"
AVT_NS = "urn:schemas-upnp-org:service:AVTransport:1"
RC_NS = "urn:schemas-upnp-org:service:RenderingControl:1"
DIDL_OPEN = (
    '<DIDL-Lite xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/" '
    'xmlns:dc="http://purl.org/dc/elements/1.1/" '
    'xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/" '
    'xmlns:dlna="urn:schemas-dlna-org:metadata-1-0/">'
)


class Events:
    """A GENA listener. Several amplifiers, the HEOS ones among them, answer GetTransportInfo with
    empty fields and say what they are doing only in an event."""

    def __init__(self, event_url):
        self.event_url = event_url
        self.values = {}
        self.at = 0.0
        self.sid = None
        peer = urllib.parse.urlparse(event_url).hostname
        probe = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        probe.connect((peer, 80))
        self.host = probe.getsockname()[0]
        probe.close()
        keep, seen = self.values, [0.0]
        class Handler(http.server.BaseHTTPRequestHandler):
            def do_NOTIFY(self):  # noqa: N802
                body = self.rfile.read(int(self.headers.get("content-length", 0)))
                heard = read_last_change(body)
                keep.update(heard)
                if heard:
                    seen[0] = time.time()
                self.send_response(200)
                self.end_headers()

            def log_message(self, *_):
                pass
        self.seen = seen
        self.server = http.server.HTTPServer((self.host, 0), Handler)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def subscribe(self, seconds=300):
        callback = f"http://{self.host}:{self.server.server_port}/"
        req = urllib.request.Request(self.event_url, method="SUBSCRIBE", headers={
            "CALLBACK": f"<{callback}>", "NT": "upnp:event", "TIMEOUT": f"Second-{seconds}"})
        try:
            with urllib.request.urlopen(req, timeout=10) as r:
                self.sid = r.headers.get("SID")
        except OSError:
            self.sid = None
        return self.sid

    def wait_for(self, name, wanted, seconds):
        deadline = time.time() + seconds
        while time.time() < deadline:
            if self.values.get(name) in wanted:
                return self.values[name]
            time.sleep(0.3)
        return self.values.get(name)

    def close(self):
        if self.sid:
            req = urllib.request.Request(self.event_url, method="UNSUBSCRIBE", headers={"SID": self.sid})
            try:
                urllib.request.urlopen(req, timeout=5).close()
            except OSError:
                pass
        self.server.shutdown()


def read_last_change(body):
    text = body.decode("utf-8", "replace")
    m = re.search(r"<LastChange>(.*?)</LastChange>", text, re.S)
    inner = html.unescape(m.group(1)) if m else text
    return {name: html.unescape(value) for name, value in re.findall(r"<(\w+)\s+val=\"([^\"]*)\"", inner)}


def named(desc_bytes):
    m = re.search(r"<friendlyName>(.*?)</friendlyName>", desc_bytes.decode("utf-8", "replace"), re.S)
    return m.group(1) if m else ""


def base_of(url):
    parsed = urllib.parse.urlparse(url)
    return f"{parsed.scheme}://{parsed.netloc}"


def event_url(desc_bytes, base_url, service_type_substr="AVTransport"):
    text = desc_bytes.decode("utf-8", "replace")
    for m in re.finditer(r"<service>(.*?)</service>", text, re.S):
        block = m.group(1)
        if service_type_substr in block:
            found = re.search(r"<eventSubURL>(.*?)</eventSubURL>", block, re.S)
            if found:
                return urllib.parse.urljoin(base_url + "/", found.group(1).strip().lstrip("/"))
    return None


def value_of(resp_bytes, tag):
    m = re.search(rf"<{tag}>(.*?)</{tag}>", resp_bytes.decode("utf-8", "replace"), re.S)
    return html.unescape(m.group(1)) if m else None


def fault_of(resp_bytes):
    text = resp_bytes.decode("utf-8", "replace")
    m = re.search(r"<errorCode>(.*?)</errorCode>", text, re.S)
    if not m:
        return None
    says = re.search(r"<errorDescription>(.*?)</errorDescription>", text, re.S)
    return f"{m.group(1)} {says.group(1) if says else ''}".strip()


def seconds_of(clock):
    """A REL_TIME as the renderer writes it, in seconds; None where it writes NOT_IMPLEMENTED."""
    if not clock or not re.match(r"^\d+:\d{2}:\d{2}", clock):
        return None
    hours, minutes, rest = clock.split(":")[:3]
    return int(hours) * 3600 + int(minutes) * 60 + int(float(rest.replace(",", ".")))


def items_with_res(didl):
    """Every track of a listing: its URL and the element itself, which is the metadata to hand over."""
    found = []
    for m in re.finditer(r"<item\b.*?</item>", didl or "", re.S):
        block = m.group(0)
        res = re.search(r"<res\b[^>]*>(.*?)</res>", block, re.S)
        if res:
            found.append((html.unescape(res.group(1)).strip(), block))
    return found


def find_device(target, name, timeout):
    print(f"== looking for {'a renderer' if target == RENDERER_TARGET else 'a server'} ==")
    candidates = []
    for ip, loc in msearch(target=target, timeout=timeout):
        status, desc, _ = get(loc)
        if status != 200:
            continue
        candidates.append((loc, named(desc), desc))
        print(f"  {named(desc)!r} at {loc}")
    if name:
        candidates = [c for c in candidates if name.lower() in c[1].lower()]
    if not candidates:
        return None, None, None
    if len(candidates) > 1 and not name:
        print("  several answered; pass a name to pick one")
        return None, None, None
    return candidates[0]


def hunt_tracks(control_url, out, max_depth):
    """Walks down from the root to a listing of tracks, preferring one holding more than one so the
    queue has something to play next."""
    object_id, hops, fallback = "0", 0, []
    while hops < max_depth:
        _, resp, status = browse(control_url, object_id, count=20)
        if status != 200:
            break
        didl, _ = result_of(resp)
        here = items_with_res(didl)
        if len(here) > 1:
            return kept(out, here)
        fallback = fallback or here
        children = containers_of(didl)
        if not children:
            break
        for child in children[:6]:
            _, sub, sub_status = browse(control_url, child, count=20)
            if sub_status != 200:
                continue
            inside = items_with_res(result_of(sub)[0])
            if len(inside) > 1:
                return kept(out, inside)
            fallback = fallback or inside
        object_id = children[0]
        hops += 1
        print(f"  [{hops}] down into {object_id!r}")
    return kept(out, fallback)


def kept(out, found):
    if found:
        write(out, "track.didl.xml", found[0][1])
    return found


def act(control_url, action, fields, ns=AVT_NS, out=None, label=None):
    body = f'  <u:{action} xmlns:u="{ns}">\n'
    for name, value in fields.items():
        body += f"   <{name}>{esc(str(value))}</{name}>\n"
    body += f"  </u:{action}>"
    env, resp, status = soap(control_url, action, body, ns=ns)
    if out:
        write(out, f"{label or action}.request.xml", env)
        write(out, f"{label or action}.response.xml", resp)
    return resp, status


class Amp:
    """What the renderer is doing, asked directly and read from its events where it answers nothing."""

    def __init__(self, control_url, events_url):
        self.control_url = control_url
        self.events_url = events_url
        self.events = None
        self.since = 0.0

    def listening(self):
        if self.events is None and self.events_url:
            self.events = Events(self.events_url)
            if self.events.subscribe():
                print(f"  it answers nothing when asked, so its events are read instead ({self.events.sid})")
        return self.events

    def heard(self, name):
        """What its events say, and nothing at all where they have said nothing since `since`."""
        listener = self.listening()
        if not listener or listener.seen[0] < self.since:
            return None
        return listener.values.get(name)

    def state(self):
        resp, _ = act(self.control_url, "GetTransportInfo", {"InstanceID": 0})
        return value_of(resp, "CurrentTransportState") or self.heard("TransportState")

    def position(self):
        resp, _ = act(self.control_url, "GetPositionInfo", {"InstanceID": 0})
        rel, uri = value_of(resp, "RelTime"), value_of(resp, "TrackURI")
        rel = rel or self.heard("RelativeTimePosition")
        uri = uri or self.heard("CurrentTrackURI") or self.heard("AVTransportURI")
        return rel, uri

    def started(self):
        """PLAYING, or, where it says nothing at all, a position that moves."""
        state = self.state()
        if state == "PLAYING":
            return True
        if state:
            return False
        return self.advancing(6)[1] is not None

    def wait_until_playing(self, seconds):
        deadline = time.time() + seconds
        while time.time() < deadline:
            if self.started():
                return True
            time.sleep(1)
        return False

    def wait_for_uri(self, url, seconds):
        """A renderer changing tracks names the one before for a while, so the change is waited for."""
        deadline, seen = time.time() + seconds, None
        while time.time() < deadline:
            _, seen = self.position()
            if (seen or "").strip() == url:
                return True, seen
            time.sleep(1)
        return False, seen

    def advancing(self, seconds, step=2.0):
        """Two positions, the second past the first. A renderer crossing a track boundary reports the
        end of the one before for a while, which is not a position standing still."""
        first, _ = self.position()
        deadline = time.time() + seconds
        while time.time() < deadline:
            time.sleep(step)
            later, _ = self.position()
            a, b = seconds_of(first), seconds_of(later)
            if a is not None and b is not None and b > a:
                return first, later
            first = later
        return first, None

    def wait_for(self, wanted, seconds):
        deadline = time.time() + seconds
        state = None
        while time.time() < deadline:
            state = self.state()
            if state in wanted:
                return state
            time.sleep(0.5)
        return state

    def close(self):
        if self.events:
            self.events.close()


def unanswered(label, why):
    print(f"  [    ] {label} — {why}")


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--renderer", help="friendlyName substring of the amplifier to drive")
    ap.add_argument("--renderer-desc", help="skip discovery, fetch this renderer description directly")
    ap.add_argument("--server", help="friendlyName substring of the server to take a track from")
    ap.add_argument("--server-desc", help="skip discovery, take the track from this server description")
    ap.add_argument("--track-url", help="play this URL instead of hunting for a track")
    ap.add_argument("--seek", default="00:01:00", help="where to jump once it plays (default 00:01:00)")
    ap.add_argument("--play-seconds", type=float, default=6.0, help="how long to let it play before stopping")
    ap.add_argument("--patience", type=float, default=60.0,
                    help="how long to wait for it to start, since one woken from standby is slow")
    ap.add_argument("--max-volume", type=int, default=45,
                    help="refuse to play above this, unless --force (default 45)")
    ap.add_argument("--force", action="store_true", help="drive it even if it is playing or loud")
    ap.add_argument("--dry-run", action="store_true", help="say what it would play, and make no sound")
    ap.add_argument("--leave-playing", action="store_true", help="do not stop it at the end")
    ap.add_argument("--no-queue", action="store_true", help="skip the track after this one")
    ap.add_argument("--tail-seconds", type=float, default=8.0,
                    help="how close to the end to jump when waiting for the next track (default 8)")
    ap.add_argument("--out", default=os.path.join(tempfile.gettempdir(), "drive-renderer"))
    ap.add_argument("--timeout", type=float, default=4.0)
    ap.add_argument("--max-depth", type=int, default=6)
    args = ap.parse_args()
    os.makedirs(args.out, exist_ok=True)

    if args.renderer_desc:
        status, desc, _ = get(args.renderer_desc)
        loc = args.renderer_desc if status == 200 else None
        name = named(desc) if status == 200 else None
    else:
        loc, name, desc = find_device(RENDERER_TARGET, args.renderer, args.timeout)
    if not loc:
        print("no renderer to drive", file=sys.stderr)
        sys.exit(2)

    write(args.out, "renderer-description.xml", desc)
    avt = resolve_control_url(desc, base_of(loc), "AVTransport")
    rc = resolve_control_url(desc, base_of(loc), "RenderingControl")
    print(f"\n== {name} ==")
    print(f"  AVTransport: {avt}")
    if not check("renderer advertises AVTransport", avt is not None):
        summarise(args.out)
        return

    amp = Amp(avt, event_url(desc, base_of(loc)))
    print("\n== what it is doing now ==")
    was = amp.state()
    print(f"  transport: {was or 'it says nothing'}")
    volume = None
    if rc:
        resp, _ = act(rc, "GetVolume", {"InstanceID": 0, "Channel": "Master"}, ns=RC_NS)
        volume = value_of(resp, "CurrentVolume")
        print(f"  volume: {volume}")
    refusal = None
    if was == "PLAYING" and not args.force:
        refusal = "it is playing something; pass --force to take it over"
    if volume and volume.isdigit() and int(volume) > args.max_volume and not args.force:
        refusal = f"volume is {volume}, above --max-volume {args.max_volume}; pass --force to play anyway"
    if refusal:
        amp.close()
        sys.exit(refusal)

    metadata, next_url, next_metadata = "", None, ""
    if args.track_url:
        track_url = args.track_url
    else:
        if args.server_desc:
            status, sdesc, _ = get(args.server_desc)
            sloc = args.server_desc if status == 200 else None
        else:
            sloc, sname, sdesc = find_device("urn:schemas-upnp-org:device:MediaServer:1", args.server, args.timeout)
        if not sloc:
            print("no server to take a track from", file=sys.stderr)
            sys.exit(2)
        cd = resolve_control_url(sdesc, base_of(sloc), "ContentDirectory")
        print(f"\n== hunting a track on {named(sdesc)} ==")
        if not check("server advertises ContentDirectory", cd is not None):
            summarise(args.out)
            return
        found = hunt_tracks(cd, args.out, args.max_depth)
        if not check("found a track to play", bool(found)):
            amp.close()
            summarise(args.out)
            return
        track_url, metadata = found[0]
        if len(found) > 1:
            next_url, next_metadata = found[1]
    print(f"  {track_url}")
    if next_url:
        print(f"  next: {next_url}")

    if args.dry_run:
        print("\n--dry-run, so nothing is played")
        amp.close()
        summarise(args.out)
        return

    print("\n== handing it to the renderer ==")
    didl = f"{DIDL_OPEN}{metadata}</DIDL-Lite>" if metadata else ""
    resp, status = act(avt, "SetAVTransportURI",
                       {"InstanceID": 0, "CurrentURI": track_url, "CurrentURIMetaData": didl},
                       out=args.out)
    check("SetAVTransportURI accepted", status == 200 and not fault_of(resp), fault_of(resp) or f"HTTP {status}")

    amp.since = time.time()
    resp, status = act(avt, "Play", {"InstanceID": 0, "Speed": "1"}, out=args.out)
    check("Play accepted", status == 200 and not fault_of(resp), fault_of(resp) or f"HTTP {status}")

    began = time.time()
    if not check("it starts playing", amp.wait_until_playing(args.patience),
                 f"it says {amp.state() or 'nothing'} after {args.patience:.0f}s"):
        stop(amp, args)
        summarise(args.out)
        return
    print(f"  it took {time.time() - began:.0f}s to start")

    arrived, uri = amp.wait_for_uri(track_url, args.patience)
    check("it is playing the URL it was given", arrived, uri or "no TrackURI")
    first, later = amp.advancing(args.play_seconds + 10)
    a, b = seconds_of(first), seconds_of(later)
    if a is None and b is None:
        unanswered("the position advances", "it gives no position")
        check("it is still playing", amp.state() == "PLAYING")
    else:
        check("the position advances", a is not None and b is not None and b > a, f"{first} then {later}")

    print("\n== seeking ==")
    resp, status = act(avt, "Seek", {"InstanceID": 0, "Unit": "REL_TIME", "Target": args.seek}, out=args.out)
    if check("Seek accepted", status == 200 and not fault_of(resp), fault_of(resp) or f"HTTP {status}"):
        time.sleep(2)
        after, _ = amp.position()
        wanted, reached = seconds_of(args.seek), seconds_of(after)
        if reached is None:
            unanswered("it plays from where it was told", "it gives no position")
        else:
            check("it plays from where it was told", abs(reached - wanted) <= 10, f"asked {args.seek}, at {after}")
        check("it is still playing after the jump", amp.state() == "PLAYING")

    print("\n== pause and resume ==")
    amp.since = time.time()
    resp, status = act(avt, "Pause", {"InstanceID": 0}, out=args.out)
    if check("Pause accepted", status == 200 and not fault_of(resp), fault_of(resp) or f"HTTP {status}"):
        paused = amp.wait_for({"PAUSED_PLAYBACK", "PAUSED"}, 10)
        check("it reports itself paused", paused in ("PAUSED_PLAYBACK", "PAUSED"), paused or "it says nothing")
    time.sleep(1)
    amp.since = time.time()
    resp, status = act(avt, "Play", {"InstanceID": 0, "Speed": "1"}, out=args.out)
    check("it plays again", amp.wait_until_playing(args.patience))

    if next_url and not args.no_queue:
        the_next_track(amp, avt, args, next_url, next_metadata)

    stop(amp, args)
    summarise(args.out)


def the_next_track(amp, avt, args, next_url, next_metadata):
    """Queues the track after this one, jumps to the end of the one playing, and waits for the change."""
    print("\n== the track after this one ==")
    didl = f"{DIDL_OPEN}{next_metadata}</DIDL-Lite>" if next_metadata else ""
    resp, status = act(avt, "SetNextAVTransportURI",
                       {"InstanceID": 0, "NextURI": next_url, "NextURIMetaData": didl}, out=args.out)
    if not check("SetNextAVTransportURI accepted", status == 200 and not fault_of(resp),
                 fault_of(resp) or f"HTTP {status}"):
        unanswered("the next track starts on its own", "this renderer keeps no queue")
        return

    amp.advancing(15)
    resp, _ = act(avt, "GetPositionInfo", {"InstanceID": 0})
    duration = seconds_of(value_of(resp, "TrackDuration")) or seconds_of(amp.heard("CurrentTrackDuration"))
    if duration is None or duration <= args.tail_seconds:
        unanswered("the next track starts on its own", "it gives no length to jump to the end of")
        return

    target = time.strftime("%H:%M:%S", time.gmtime(int(duration - args.tail_seconds)))
    print(f"  the track lasts {duration}s, jumping to {target}")
    amp.since = time.time()
    act(avt, "Seek", {"InstanceID": 0, "Unit": "REL_TIME", "Target": target}, out=args.out, label="Seek-to-end")

    moved, playing = amp.wait_for_uri(next_url, args.tail_seconds + args.patience)
    if moved:
        check("the next track starts on its own", True)
        check("it is still playing on the other side", amp.started())
        return
    resp, _ = act(avt, "GetMediaInfo", {"InstanceID": 0})
    queued = (value_of(resp, "NextURI") or "").strip() == next_url
    state, (rel, _) = amp.state(), amp.position()
    if queued and state in ("STOPPED", "NO_MEDIA_PRESENT"):
        unanswered("the next track starts on its own", "the renderer kept it queued and never asked for it")
    else:
        check("the next track starts on its own", False,
              f"it is {state or 'silent'} at {rel or 'nowhere'}, on {playing or 'nothing it names'}")


def stop(amp, args):
    if args.leave_playing:
        amp.close()
        return
    print("\n== stopping ==")
    amp.since = time.time()
    resp, status = act(amp.control_url, "Stop", {"InstanceID": 0}, out=args.out)
    check("Stop accepted", status == 200 and not fault_of(resp), fault_of(resp) or f"HTTP {status}")
    stopped = amp.wait_for({"STOPPED", "NO_MEDIA_PRESENT"}, 5)
    check("it stopped", stopped in ("STOPPED", "NO_MEDIA_PRESENT"), stopped or "it says nothing")
    amp.close()


if __name__ == "__main__":
    main()
