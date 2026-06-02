"""
ui.py  –  Professional Black & White dashboard for EarbudsTracker
──────────────────────────────────────────────────────────────────
Layout
  ┌─────────────────────────────────┐
  │  Header: icon · name · status   │
  ├─────────────────────────────────┤
  │  Session ring + big time nums   │  ← Canvas arc animation
  ├─────────────────────────────────┤
  │  Today │ Week  (2-col grid)     │
  │  Month  │ Lifetime              │
  ├─────────────────────────────────┤
  │  [History]          [Reset]     │
  └─────────────────────────────────┘
"""

import tkinter as tk
from tkinter import ttk, messagebox
from typing import Callable

REFRESH_MS = 1000

# ── Palette  (strict black / white / grey) ────────────────────────────────────
BG          = "#000000"   # pure black
SURFACE     = "#111111"   # near-black cards / header
SURFACE2    = "#161616"   # slightly lighter card fill
SURFACE3    = "#222222"   # hover / active state
BORDER      = "#2a2a2a"   # subtle card border
BORDER_LIT  = "#444444"   # highlighted border (focused / hover)

# Functional status colours – kept minimal; only used for live-state feedback
WHITE       = "#ffffff"
GREEN       = "#4ade80"   # playback active  – muted sage green
GREEN_DIM   = "#1a3a27"   # ring track fill when playing
AMBER       = "#d4a017"   # idle-connected   – warm gold (not neon)
AMBER_DIM   = "#2e2000"
RED_DIM     = "#2a0a0a"   # ring track fill when disconnected

# Aliases used by the rest of the file
VIOLET      = WHITE       # connected arc  → white ring
VIOLET_DIM  = "#2a2a2a"   # ring track fill
CYAN        = WHITE       # connected time label → white
CYAN_DIM    = "#333333"   # dimmed (disconnected)

TEXT        = "#ffffff"   # primary text
TEXT2       = "#aaaaaa"   # secondary text
TEXT3       = "#555555"   # muted / captions

F           = "Segoe UI"
F_MONO      = "Consolas"

# Arc geometry
RING_SIZE   = 180
RING_W      = 12
RING_GAP    = 6


# ── Helpers ────────────────────────────────────────────────────────────────────

def fmt_h(secs: float) -> str:
    """Short format: 1h 23m or 45m 06s or 08s"""
    s = max(0, int(secs))
    h, rem = divmod(s, 3600)
    m, sec = divmod(rem, 60)
    if h:
        return f"{h}h {m:02d}m"
    if m:
        return f"{m}m {sec:02d}s"
    return f"{sec}s"


def fmt_full(secs: float) -> str:
    """HH:MM:SS monospace display"""
    s = max(0, int(secs))
    h, rem = divmod(s, 3600)
    m, sec = divmod(rem, 60)
    return f"{h:02d}:{m:02d}:{sec:02d}"


# ── Animated ring canvas ───────────────────────────────────────────────────────

class _RingCanvas(tk.Canvas):
    """
    Two concentric arcs:
      outer – connected duration  (white)
      inner – playback duration   (muted green)
    Track arcs are dark-grey to show the unfilled portion.
    """
    MAX_SECS = 24 * 3600

    def __init__(self, parent):
        sz = RING_SIZE
        super().__init__(parent, width=sz, height=sz,
                         bg=BG, highlightthickness=0)
        cx = cy = sz // 2
        r_out = sz // 2 - 4
        r_in  = r_out - RING_W - RING_GAP - RING_W

        # Track arcs (empty background)
        self._bg_out = self.create_arc(
            cx - r_out, cy - r_out, cx + r_out, cy + r_out,
            start=91, extent=-359, style="arc",
            outline=BORDER_LIT, width=RING_W
        )
        self._bg_in = self.create_arc(
            cx - r_in, cy - r_in, cx + r_in, cy + r_in,
            start=91, extent=-359, style="arc",
            outline=BORDER, width=RING_W
        )

        # Live arcs
        self._arc_out = self.create_arc(
            cx - r_out, cy - r_out, cx + r_out, cy + r_out,
            start=90, extent=0, style="arc",
            outline=WHITE, width=RING_W
        )
        self._arc_in = self.create_arc(
            cx - r_in, cy - r_in, cx + r_in, cy + r_in,
            start=90, extent=0, style="arc",
            outline=GREEN, width=RING_W
        )

    def update_ring(self, conn_secs: float, play_secs: float,
                    conn_color: str = WHITE, play_color: str = GREEN):
        def to_extent(secs):
            frac = min(1.0, secs / self.MAX_SECS)
            return -(frac * 359)

        self.itemconfig(self._arc_out, extent=to_extent(conn_secs),
                        outline=conn_color)
        self.itemconfig(self._arc_in,  extent=to_extent(play_secs),
                        outline=play_color)


# ── Pulsing status dot ─────────────────────────────────────────────────────────

class _PulseDot(tk.Canvas):
    """Small circle that pulses to indicate live state."""
    D = 10

    def __init__(self, parent):
        super().__init__(parent, width=self.D, height=self.D,
                         bg=SURFACE, highlightthickness=0)
        self._dot   = self.create_oval(1, 1, self.D - 1, self.D - 1,
                                       fill=TEXT3, outline="")
        self._alpha = 0.0
        self._dir   = 1
        self._color = TEXT3
        self._animate()

    def set_color(self, color: str):
        self._color = color

    def _animate(self):
        self._alpha += self._dir * 0.07
        if self._alpha >= 1.0:
            self._alpha = 1.0
            self._dir = -1
        elif self._alpha <= 0.25:
            self._alpha = 0.25
            self._dir = 1
        self.itemconfig(self._dot, fill=self._color)
        self.after(60, self._animate)


# ── Stat card ─────────────────────────────────────────────────────────────────

class _StatCell(tk.Frame):
    def __init__(self, parent, title: str,
                 conn_color: str = TEXT2, play_color: str = GREEN):
        super().__init__(parent, bg=SURFACE2,
                         highlightthickness=1, highlightbackground=BORDER)
        self._cc = conn_color
        self._pc = play_color

        # Title row
        tk.Label(self, text=title, bg=SURFACE2, fg=TEXT3,
                 font=(F, 7, "bold")).pack(anchor="w", padx=10, pady=(8, 2))
        tk.Frame(self, bg=BORDER, height=1).pack(fill="x", padx=10)

        # Connected row
        r1 = tk.Frame(self, bg=SURFACE2)
        r1.pack(fill="x", padx=10, pady=(5, 1))
        tk.Label(r1, text="CONN", bg=SURFACE2, fg=TEXT3,
                 font=(F, 7)).pack(side="left")
        self._conn = tk.Label(r1, text="—", bg=SURFACE2, fg=conn_color,
                              font=(F_MONO, 10, "bold"))
        self._conn.pack(side="right")

        # Playback row
        r2 = tk.Frame(self, bg=SURFACE2)
        r2.pack(fill="x", padx=10, pady=(1, 8))
        tk.Label(r2, text="PLAY", bg=SURFACE2, fg=TEXT3,
                 font=(F, 7)).pack(side="left")
        self._play = tk.Label(r2, text="—", bg=SURFACE2, fg=play_color,
                              font=(F_MONO, 10, "bold"))
        self._play.pack(side="right")

    def update(self, conn_secs: float, play_secs: float):
        self._conn.config(text=fmt_h(conn_secs))
        self._play.config(text=fmt_h(play_secs))


# ── Section label helper ───────────────────────────────────────────────────────

def _section_label(parent, text: str):
    """Uppercase grey caption with a thin right-extending rule."""
    row = tk.Frame(parent, bg=BG)
    tk.Label(row, text=text, bg=BG, fg=TEXT3,
             font=(F, 7, "bold")).pack(side="left")
    tk.Frame(row, bg=BORDER, height=1).pack(
        side="left", fill="x", expand=True, padx=(10, 0), pady=1)
    return row


# ── Main window ───────────────────────────────────────────────────────────────

class TrackerWindow:
    def __init__(self, tracker, on_quit: Callable):
        self._tracker = tracker

        self.root = tk.Tk()
        self.root.title("EarbudsTracker")
        self.root.geometry("400x672")
        self.root.minsize(380, 620)
        self.root.configure(bg=BG)
        self.root.resizable(False, False)
        self.root.protocol("WM_DELETE_WINDOW", self._hide)
        try:
            self.root.iconbitmap(default="")
        except Exception:
            pass

        self._build_ui()
        self._schedule_refresh()
        tracker.on_state_change = self._request_refresh

    # ── Build ──────────────────────────────────────────────────────────────────

    def _build_ui(self):
        root = self.root

        # ── Header bar ────────────────────────────────────────────────────────
        header = tk.Frame(root, bg=SURFACE,
                          highlightthickness=1, highlightbackground=BORDER)
        header.pack(fill="x")

        inner = tk.Frame(header, bg=SURFACE)
        inner.pack(fill="x", padx=20, pady=13)

        left = tk.Frame(inner, bg=SURFACE)
        left.pack(side="left")
        tk.Label(left, text="🎧", bg=SURFACE, font=(F, 16)
                 ).pack(side="left", padx=(0, 10))

        meta = tk.Frame(left, bg=SURFACE)
        meta.pack(side="left")
        tk.Label(meta, text="EarbudsTracker", bg=SURFACE, fg=TEXT,
                 font=(F, 12, "bold")).pack(anchor="w")
        tk.Label(meta, text="CMF Buds 2a", bg=SURFACE, fg=TEXT3,
                 font=(F, 8)).pack(anchor="w")

        right = tk.Frame(inner, bg=SURFACE)
        right.pack(side="right")
        self._pulse = _PulseDot(right)
        self._pulse.pack(side="left", padx=(0, 7))
        self._status_lbl = tk.Label(right, text="Disconnected",
                                    bg=SURFACE, fg=TEXT3, font=(F, 9))
        self._status_lbl.pack(side="left")

        # ── Current Session ───────────────────────────────────────────────────
        sec_sess = tk.Frame(root, bg=BG)
        sec_sess.pack(fill="x", padx=20, pady=(16, 0))

        lbl_row = _section_label(sec_sess, "CURRENT SESSION")
        lbl_row.pack(fill="x", pady=(0, 10))

        body = tk.Frame(sec_sess, bg=BG)
        body.pack(fill="x")

        # Left: ring
        ring_col = tk.Frame(body, bg=BG)
        ring_col.pack(side="left")
        self._ring = _RingCanvas(ring_col)
        self._ring.pack()

        # Ring legend
        legend = tk.Frame(ring_col, bg=BG)
        legend.pack(pady=(5, 0))
        for color, label in ((WHITE, "Connected"), (GREEN, "Playback")):
            leg_row = tk.Frame(legend, bg=BG)
            leg_row.pack(side="left", padx=7)
            tk.Label(leg_row, text="●", bg=BG, fg=color,
                     font=(F, 7)).pack(side="left")
            tk.Label(leg_row, text=label, bg=BG, fg=TEXT3,
                     font=(F, 7)).pack(side="left", padx=(2, 0))

        # Right: big time blocks
        times = tk.Frame(body, bg=BG)
        times.pack(side="left", padx=(14, 0), fill="both", expand=True)

        conn_card = tk.Frame(times, bg=SURFACE,
                             highlightthickness=1,
                             highlightbackground=BORDER)
        conn_card.pack(fill="x", pady=(0, 6))
        tk.Label(conn_card, text="CONNECTED", bg=SURFACE, fg=TEXT3,
                 font=(F, 7, "bold")).pack(anchor="w", padx=10, pady=(8, 0))
        self._sess_conn_lbl = tk.Label(conn_card, text="00:00:00",
                                       bg=SURFACE, fg=WHITE,
                                       font=(F_MONO, 21, "bold"))
        self._sess_conn_lbl.pack(anchor="w", padx=10, pady=(0, 8))

        play_card = tk.Frame(times, bg=SURFACE,
                             highlightthickness=1,
                             highlightbackground=BORDER)
        play_card.pack(fill="x")
        tk.Label(play_card, text="PLAYBACK", bg=SURFACE, fg=TEXT3,
                 font=(F, 7, "bold")).pack(anchor="w", padx=10, pady=(8, 0))
        self._sess_play_lbl = tk.Label(play_card, text="00:00:00",
                                       bg=SURFACE, fg=TEXT3,
                                       font=(F_MONO, 21, "bold"))
        self._sess_play_lbl.pack(anchor="w", padx=10, pady=(0, 8))

        self._audio_badge = tk.Label(times, text="— No signal",
                                     bg=BG, fg=TEXT3, font=(F, 8))
        self._audio_badge.pack(anchor="w", pady=(6, 0))

        # ── Statistics ────────────────────────────────────────────────────────
        sec_stats = tk.Frame(root, bg=BG)
        sec_stats.pack(fill="x", padx=20, pady=(18, 0))

        lbl_row2 = _section_label(sec_stats, "STATISTICS")
        lbl_row2.pack(fill="x", pady=(0, 8))

        grid = tk.Frame(sec_stats, bg=BG)
        grid.pack(fill="x")
        grid.columnconfigure(0, weight=1)
        grid.columnconfigure(1, weight=1)

        self._today = _StatCell(grid, "TODAY",      TEXT2, GREEN)
        self._week  = _StatCell(grid, "THIS WEEK",  TEXT2, GREEN)
        self._month = _StatCell(grid, "THIS MONTH", TEXT2, GREEN)
        self._life  = _StatCell(grid, "LIFETIME",   WHITE, GREEN)

        self._today.grid(row=0, column=0, padx=(0, 3), pady=(0, 3), sticky="nsew")
        self._week .grid(row=0, column=1, padx=(3, 0), pady=(0, 3), sticky="nsew")
        self._month.grid(row=1, column=0, padx=(0, 3), pady=(3, 0), sticky="nsew")
        self._life .grid(row=1, column=1, padx=(3, 0), pady=(3, 0), sticky="nsew")

        # ── Action buttons ────────────────────────────────────────────────────
        btns = tk.Frame(root, bg=BG)
        btns.pack(fill="x", padx=20, pady=16)

        self._btn_hist  = self._make_btn(btns, "Session History",
                                         self._open_sessions_window,
                                         fg=WHITE, border_color=BORDER_LIT,
                                         hover_bg=SURFACE3)
        self._btn_reset = self._make_btn(btns, "Reset Data",
                                         self._confirm_reset,
                                         fg=TEXT2, border_color=BORDER,
                                         hover_bg=SURFACE3)
        self._btn_hist .pack(side="left",  expand=True, fill="x", padx=(0, 4))
        self._btn_reset.pack(side="right", expand=True, fill="x", padx=(4, 0))

        # ── Footer ────────────────────────────────────────────────────────────
        tk.Label(root,
                 text="Minimise to tray  ·  data in %APPDATA%\\EarbudsTracker",
                 bg=BG, fg=TEXT3, font=(F, 7)).pack(side="bottom", pady=(0, 9))

    # ── Button factory ─────────────────────────────────────────────────────────

    def _make_btn(self, parent, text, cmd, fg, border_color, hover_bg):
        btn = tk.Button(
            parent, text=text, command=cmd,
            bg=SURFACE2, fg=fg,
            activebackground=hover_bg, activeforeground=fg,
            relief="flat", bd=0, pady=9, padx=12, cursor="hand2",
            font=(F, 9, "bold"),
            highlightthickness=1, highlightbackground=border_color,
        )
        btn.bind("<Enter>", lambda e: btn.config(bg=hover_bg))
        btn.bind("<Leave>", lambda e: btn.config(bg=SURFACE2))
        return btn

    # ── Refresh ────────────────────────────────────────────────────────────────

    def _schedule_refresh(self):
        self.root.after(REFRESH_MS, self._refresh)

    def _request_refresh(self):
        try:
            self.root.after(0, self._refresh)
        except RuntimeError:
            pass

    def _refresh(self):
        try:
            snap = self._tracker.get_snapshot()
            self._apply(snap)
        except Exception:
            pass
        self._schedule_refresh()

    def _apply(self, snap: dict):
        connected = snap["connected"]
        playing   = snap["playing"]
        c_secs    = snap["sess_conn"]
        p_secs    = snap["sess_play"]

        # ── Status dot + label ────────────────────────────────────────────────
        if playing:
            dot_color  = GREEN
            status_txt = "▶  Playing"
            conn_arc   = WHITE
            play_arc   = GREEN
            conn_fg    = WHITE
            play_fg    = GREEN
            badge_txt  = "▶  Audio active"
            badge_fg   = GREEN
        elif connected:
            dot_color  = AMBER
            status_txt = "Connected  –  idle"
            conn_arc   = WHITE
            play_arc   = TEXT3
            conn_fg    = WHITE
            play_fg    = TEXT3
            badge_txt  = "⏸  Paused / silent"
            badge_fg   = AMBER
        else:
            dot_color  = TEXT3
            status_txt = "Disconnected"
            conn_arc   = BORDER_LIT
            play_arc   = BORDER
            conn_fg    = TEXT3
            play_fg    = TEXT3
            badge_txt  = "—  No device"
            badge_fg   = TEXT3

        self._pulse.set_color(dot_color)
        self._status_lbl.config(text=status_txt, fg=dot_color)

        # ── Ring arcs ─────────────────────────────────────────────────────────
        self._ring.update_ring(c_secs, p_secs, conn_arc, play_arc)

        # ── Big time labels ───────────────────────────────────────────────────
        self._sess_conn_lbl.config(text=fmt_full(c_secs), fg=conn_fg)
        self._sess_play_lbl.config(text=fmt_full(p_secs), fg=play_fg)

        # ── Audio badge ───────────────────────────────────────────────────────
        self._audio_badge.config(text=badge_txt, fg=badge_fg)

        # ── Stat cells ────────────────────────────────────────────────────────
        self._today.update(snap["today"]["connected"],    snap["today"]["playback"])
        self._week .update(snap["week"]["connected"],     snap["week"]["playback"])
        self._month.update(snap["month"]["connected"],    snap["month"]["playback"])
        self._life .update(snap["lifetime"]["connected"], snap["lifetime"]["playback"])

    # ── Visibility ─────────────────────────────────────────────────────────────

    def show(self):
        self.root.deiconify()
        self.root.lift()
        self.root.focus_force()

    def _hide(self):
        self.root.withdraw()

    # ── Session history popup ──────────────────────────────────────────────────

    def _open_sessions_window(self):
        win = tk.Toplevel(self.root)
        win.title("Session History")
        win.geometry("740x440")
        win.configure(bg=BG)
        win.resizable(True, True)

        # Header
        hdr = tk.Frame(win, bg=SURFACE,
                       highlightthickness=1, highlightbackground=BORDER)
        hdr.pack(fill="x")
        tk.Label(hdr, text="Session History", bg=SURFACE, fg=TEXT,
                 font=(F, 12, "bold")).pack(side="left", padx=16, pady=11)

        # Treeview styling
        style = ttk.Style(win)
        style.theme_use("clam")
        style.configure("H.Treeview",
                        background=SURFACE, foreground=TEXT2,
                        fieldbackground=SURFACE, rowheight=30,
                        borderwidth=0, font=(F, 9))
        style.configure("H.Treeview.Heading",
                        background=SURFACE2, foreground=WHITE,
                        relief="flat", font=(F, 9, "bold"),
                        borderwidth=0)
        style.map("H.Treeview",
                  background=[("selected", SURFACE3)],
                  foreground=[("selected", TEXT)])

        cols = ("start", "end", "connected", "playback")
        hdrs = ("Start", "End", "Connected", "Playback")
        tree = ttk.Treeview(win, columns=cols, show="headings",
                            style="H.Treeview")
        widths = (190, 190, 150, 150)
        for col, h, w in zip(cols, hdrs, widths):
            tree.heading(col, text=h)
            tree.column(col, width=w, anchor="center")

        tree.tag_configure("odd",  background=SURFACE)
        tree.tag_configure("even", background=SURFACE2)

        import database as db
        for i, s in enumerate(db.get_recent_sessions(200)):
            start = (s["session_start"] or "")[:19].replace("T", "  ")
            end   = (s["session_end"]   or "—")[:19].replace("T", "  ")
            tag   = "odd" if i % 2 == 0 else "even"
            tree.insert("", "end", tags=(tag,), values=(
                start, end,
                fmt_h(s["connected_secs"]),
                fmt_h(s["playback_secs"]),
            ))

        sb = ttk.Scrollbar(win, orient="vertical", command=tree.yview)
        tree.configure(yscrollcommand=sb.set)
        tree.pack(side="left", fill="both", expand=True, padx=(12, 0), pady=10)
        sb.pack(side="right", fill="y", pady=10, padx=(0, 6))

    # ── Reset ──────────────────────────────────────────────────────────────────

    def _confirm_reset(self):
        if messagebox.askyesno(
            "Reset All Data",
            "Permanently delete all session history and statistics?\n\nThis cannot be undone.",
            parent=self.root,
        ):
            self._tracker.reset_all()
            messagebox.showinfo("Done", "All data cleared.", parent=self.root)

    def run(self):
        self.root.mainloop()
