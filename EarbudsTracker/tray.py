"""
tray.py
───────
System-tray icon using pystray.
Provides menu: Open, Quit.
Icon is generated programmatically (no external image needed).
"""

import threading
from PIL import Image, ImageDraw
import pystray


def _make_icon_image(connected: bool = False, playing: bool = False) -> Image.Image:
    """Generate a 64×64 icon: headphone silhouette with status colour."""
    size  = 64
    img   = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    draw  = ImageDraw.Draw(img)

    # Background circle
    if playing:
        bg = (16, 185, 129, 230)       # emerald green
    elif connected:
        bg = (99, 102, 241, 230)       # indigo
    else:
        bg = (71, 85, 105, 200)        # slate grey

    draw.ellipse([2, 2, 61, 61], fill=bg)

    # Headphone arc
    arc_col  = (220, 220, 240, 255)
    draw.arc([12, 10, 52, 40], start=200, end=340, fill=arc_col, width=5)

    # Ear cups
    draw.ellipse([10, 30, 22, 48], fill=arc_col)
    draw.ellipse([42, 30, 54, 48], fill=arc_col)

    return img


class TrayIcon:
    def __init__(self, on_open, on_quit, tracker):
        self._on_open  = on_open
        self._on_quit  = on_quit
        self._tracker  = tracker
        self._icon     = None
        self._thread   = None

    def start(self) -> None:
        menu = pystray.Menu(
            pystray.MenuItem("Open Dashboard", self._do_open, default=True),
            pystray.Menu.SEPARATOR,
            pystray.MenuItem("Quit",           self._do_quit),
        )
        self._icon = pystray.Icon(
            "EarbudsTracker",
            _make_icon_image(),
            "EarbudsTracker – CMF Buds 2a",
            menu,
        )
        self._thread = threading.Thread(
            target=self._icon.run, name="TrayIcon", daemon=True
        )
        self._thread.start()

        # Hook tracker updates to refresh icon
        _prev_notify = self._tracker.on_state_change
        def _chained():
            self._update_icon()
            if callable(_prev_notify):
                _prev_notify()
        self._tracker.on_state_change = _chained

    def stop(self) -> None:
        if self._icon:
            self._icon.stop()

    def _update_icon(self) -> None:
        if self._icon is None:
            return
        snap = self._tracker.get_snapshot()
        img  = _make_icon_image(snap["connected"], snap["playing"])
        tip  = (
            f"CMF Buds 2a\n"
            f"{'🟢 Playing' if snap['playing'] else ('🟡 Connected' if snap['connected'] else '🔴 Disconnected')}\n"
            f"Session: {_fmt(snap['sess_conn'])} / {_fmt(snap['sess_play'])} play"
        )
        self._icon.icon = img
        self._icon.title = tip

    def _do_open(self, *_):
        self._on_open()

    def _do_quit(self, *_):
        self._on_quit()


def _fmt(s: float) -> str:
    s = int(s)
    h, rem = divmod(s, 3600)
    m, sec = divmod(rem, 60)
    return f"{h}h{m:02d}m" if h else f"{m}m{sec:02d}s"
