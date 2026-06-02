"""
tray.py
───────
System-tray icon using QSystemTrayIcon.
Provides menu: Open Dashboard, Quit.
Icon is generated programmatically using PIL.
"""

from PyQt5.QtWidgets import QSystemTrayIcon, QMenu, QAction
from PyQt5.QtGui import QIcon, QPixmap, QImage, QFont
from PyQt5.QtCore import Qt
from PIL import Image, ImageDraw
import logging

logger = logging.getLogger("tray")


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


def get_qicon(connected: bool = False, playing: bool = False) -> QIcon:
    """Convert PIL generated icon image to QIcon."""
    pil_img = _make_icon_image(connected, playing)
    data = pil_img.tobytes("raw", "RGBA")
    qimg = QImage(data, pil_img.size[0], pil_img.size[1], QImage.Format_RGBA8888)
    pixmap = QPixmap.fromImage(qimg)
    return QIcon(pixmap)


class TrayIcon(QSystemTrayIcon):
    def __init__(self, parent, on_open, on_quit, tracker):
        super().__init__(parent)
        self.on_open = on_open
        self.on_quit = on_quit
        self.tracker = tracker

        # Context menu
        self.menu = QMenu(parent)
        
        self.open_action = QAction("Open Dashboard", self)
        self.open_action.triggered.connect(self.on_open)
        self.open_action.setFont(QFont("Segoe UI", 9, QFont.Bold))
        self.menu.addAction(self.open_action)
        
        self.menu.addSeparator()
        
        self.quit_action = QAction("Quit", self)
        self.quit_action.triggered.connect(self.on_quit)
        self.menu.addAction(self.quit_action)
        
        self.setContextMenu(self.menu)
        self.activated.connect(self._on_activated)

        # Initialise icon and tooltip
        self.update_icon()

        # Chain state change notifications
        _prev_notify = self.tracker.on_state_change
        def _chained():
            self.update_icon()
            if callable(_prev_notify):
                _prev_notify()
        self.tracker.on_state_change = _chained

    def _on_activated(self, reason):
        if reason in (QSystemTrayIcon.DoubleClick, QSystemTrayIcon.Trigger):
            self.on_open()

    def update_icon(self) -> None:
        snap = self.tracker.get_snapshot()
        self.setIcon(get_qicon(snap["connected"], snap["playing"]))
        
        tip = (
            f"CMF Buds 2a\n"
            f"{'🟢 Playing' if snap['playing'] else ('🟡 Connected' if snap['connected'] else '🔴 Disconnected')}\n"
            f"Session: {_fmt(snap['sess_conn'])} / {_fmt(snap['sess_play'])} play"
        )
        self.setToolTip(tip)


def _fmt(s: float) -> str:
    s = int(s)
    h, rem = divmod(s, 3600)
    m, sec = divmod(rem, 60)
    return f"{h}h{m:02d}m" if h else f"{m}m{sec:02d}s"
