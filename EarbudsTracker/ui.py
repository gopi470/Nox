"""
ui.py  –  Native WinUI 3 Fluent Dashboard for EarbudsTracker
──────────────────────────────────────────────────────────
Uses PyQt5 and qfluentwidgets to deliver a modern Windows 11 native layout.
"""

import sys
from PyQt5.QtWidgets import QWidget, QVBoxLayout, QHBoxLayout, QGridLayout, QLabel, QFrame, QHeaderView, QTableWidgetItem
from PyQt5.QtCore import Qt, QTimer, pyqtSlot, QRectF
from PyQt5.QtGui import QFont, QColor, QPainter, QPen, QIcon
from qfluentwidgets import (
    FluentWindow, TitleLabel, SubtitleLabel, BodyLabel, CaptionLabel,
    CardWidget, TableWidget, PushButton, MessageBox, InfoBar, InfoBarPosition
)
from qfluentwidgets import FluentIcon as FIF
import qfluentwidgets as qfw


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


# ── Custom progress rings widget ───────────────────────────────────────────────

class SessionProgressRing(QWidget):
    """
    Custom widget that draws two concentric progress rings using QPainter:
      - Outer ring: Connection time (White / Gray when disconnected)
      - Inner ring: Playback time (Muted green)
    """
    def __init__(self, parent=None):
        super().__init__(parent)
        self.setMinimumSize(220, 220)
        self.conn_secs = 0.0
        self.play_secs = 0.0
        self.max_secs = 24 * 3600  # 24 hours = 100% rotation
        self.is_connected = False
        self.is_playing = False

    def update_durations(self, conn_secs: float, play_secs: float, is_connected: bool, is_playing: bool):
        self.conn_secs = conn_secs
        self.play_secs = play_secs
        self.is_connected = is_connected
        self.is_playing = is_playing
        self.update()

    def paintEvent(self, event):
        painter = QPainter(self)
        painter.setRenderHint(QPainter.Antialiasing)

        width = self.width()
        height = self.height()
        size = min(width, height)

        cx = width / 2.0
        cy = height / 2.0

        r_out = (size / 2.0) - 20.0
        r_in = r_out - 16.0  # spacing between concentric rings
        stroke_w = 10

        # Dynamic color states matching WinUI 3 guidelines
        if self.is_playing:
            conn_color = QColor("#ffffff")
            play_color = QColor("#4ade80")  # Fluent emerald green
            track_out_color = QColor("#2d2d2d")
            track_in_color = QColor("#1f3a22")
        elif self.is_connected:
            conn_color = QColor("#ffffff")
            play_color = QColor("#555555")
            track_out_color = QColor("#2d2d2d")
            track_in_color = QColor("#1e1e1e")
        else:
            conn_color = QColor("#444444")
            play_color = QColor("#333333")
            track_out_color = QColor("#222222")
            track_in_color = QColor("#1a1a1a")

        # 1. Draw outer track (Connection)
        pen_track_out = QPen(track_out_color, stroke_w, Qt.SolidLine, Qt.RoundCap)
        painter.setPen(pen_track_out)
        painter.drawEllipse(QRectF(cx - r_out, cy - r_out, r_out * 2, r_out * 2))

        # 2. Draw inner track (Playback)
        pen_track_in = QPen(track_in_color, stroke_w, Qt.SolidLine, Qt.RoundCap)
        painter.setPen(pen_track_in)
        painter.drawEllipse(QRectF(cx - r_in, cy - r_in, r_in * 2, r_in * 2))

        # 3. Draw outer connection progress arc
        if self.conn_secs > 0:
            pen_conn = QPen(conn_color, stroke_w, Qt.SolidLine, Qt.RoundCap)
            painter.setPen(pen_conn)
            span_conn = -min(360.0, (self.conn_secs / self.max_secs) * 360.0)
            painter.drawArc(QRectF(cx - r_out, cy - r_out, r_out * 2, r_out * 2), 90 * 16, int(span_conn * 16))

        # 4. Draw inner playback progress arc
        if self.play_secs > 0:
            pen_play = QPen(play_color, stroke_w, Qt.SolidLine, Qt.RoundCap)
            painter.setPen(pen_play)
            span_play = -min(360.0, (self.play_secs / self.max_secs) * 360.0)
            painter.drawArc(QRectF(cx - r_in, cy - r_in, r_in * 2, r_in * 2), 90 * 16, int(span_play * 16))


# ── Dashboard Page ────────────────────────────────────────────────────────────

class HomeInterface(QWidget):
    def __init__(self, tracker, parent=None):
        super().__init__(parent=parent)
        self.setObjectName("HomeInterface")
        self.tracker = tracker

        layout = QVBoxLayout(self)
        layout.setContentsMargins(36, 24, 36, 24)
        layout.setSpacing(20)

        # Header
        self.title_label = TitleLabel("Dashboard", self)
        self.subtitle_label = CaptionLabel("Real-time connection monitoring for CMF Buds 2a", self)
        self.subtitle_label.setStyleSheet("color: #888888;")
        layout.addWidget(self.title_label)
        layout.addWidget(self.subtitle_label)

        # Body Layout (Ring Left / Details Right)
        body_layout = QHBoxLayout()
        body_layout.setSpacing(24)

        # Left side: Ring Card
        self.ring_card = CardWidget(self)
        ring_card_layout = QVBoxLayout(self.ring_card)
        ring_card_layout.setContentsMargins(24, 24, 24, 24)
        ring_card_layout.setAlignment(Qt.AlignCenter)

        self.ring = SessionProgressRing(self.ring_card)
        ring_card_layout.addWidget(self.ring)

        # Legend
        legend_layout = QHBoxLayout()
        legend_layout.setAlignment(Qt.AlignCenter)
        legend_layout.setSpacing(16)

        self.conn_dot = QLabel("●", self.ring_card)
        self.conn_dot.setStyleSheet("color: #ffffff;")
        dot_font = self.conn_dot.font()
        dot_font.setPointSize(12)
        self.conn_dot.setFont(dot_font)
        
        self.conn_lbl = CaptionLabel("Connected", self.ring_card)
        self.conn_lbl.setStyleSheet("color: #cccccc;")

        self.play_dot = QLabel("●", self.ring_card)
        self.play_dot.setStyleSheet("color: #4ade80;")
        self.play_dot.setFont(dot_font)
        self.play_lbl = CaptionLabel("Playback", self.ring_card)
        self.play_lbl.setStyleSheet("color: #cccccc;")

        legend_layout.addWidget(self.conn_dot)
        legend_layout.addWidget(self.conn_lbl)
        legend_layout.addWidget(self.play_dot)
        legend_layout.addWidget(self.play_lbl)
        ring_card_layout.addLayout(legend_layout)

        body_layout.addWidget(self.ring_card, 1)

        # Right side: Detail Cards
        right_layout = QVBoxLayout()
        right_layout.setSpacing(16)

        # Connection Status Card
        self.status_card = CardWidget(self)
        status_card_layout = QHBoxLayout(self.status_card)
        status_card_layout.setContentsMargins(20, 20, 20, 20)

        self.status_icon = QLabel("🎧", self.status_card)
        self.status_icon.setStyleSheet("margin-right: 12px;")
        status_icon_font = self.status_icon.font()
        status_icon_font.setPointSize(20)
        self.status_icon.setFont(status_icon_font)

        status_text_layout = QVBoxLayout()
        status_text_layout.setSpacing(4)
        self.status_title = SubtitleLabel("Disconnected", self.status_card)
        self.status_desc = CaptionLabel("Earbuds are not detected", self.status_card)
        self.status_desc.setStyleSheet("color: #888888;")
        status_text_layout.addWidget(self.status_title)
        status_text_layout.addWidget(self.status_desc)

        status_card_layout.addWidget(self.status_icon)
        status_card_layout.addLayout(status_text_layout)
        status_card_layout.addStretch()
        right_layout.addWidget(self.status_card)

        # Connected Time Card
        self.conn_card = CardWidget(self)
        conn_card_layout = QVBoxLayout(self.conn_card)
        conn_card_layout.setContentsMargins(20, 20, 20, 20)
        conn_card_layout.setSpacing(6)
        conn_title = CaptionLabel("CONNECTION TIME", self.conn_card)
        conn_title.setStyleSheet("color: #888888; font-weight: bold; letter-spacing: 0.5px;")
        
        self.conn_time = TitleLabel("00:00:00", self.conn_card)
        time_font = self.conn_time.font()
        time_font.setFamily("Consolas")
        time_font.setBold(True)
        time_font.setPointSize(22)
        self.conn_time.setFont(time_font)
        
        conn_card_layout.addWidget(conn_title)
        conn_card_layout.addWidget(self.conn_time)
        right_layout.addWidget(self.conn_card)

        # Playback Time Card
        self.play_card = CardWidget(self)
        play_card_layout = QVBoxLayout(self.play_card)
        play_card_layout.setContentsMargins(20, 20, 20, 20)
        play_card_layout.setSpacing(6)
        play_title = CaptionLabel("PLAYBACK TIME", self.play_card)
        play_title.setStyleSheet("color: #4ade80; font-weight: bold; letter-spacing: 0.5px;")
        
        self.play_time = TitleLabel("00:00:00", self.play_card)
        self.play_time.setFont(time_font)
        self.play_time.setStyleSheet("color: #4ade80;")
        play_card_layout.addWidget(play_title)
        play_card_layout.addWidget(self.play_time)
        right_layout.addWidget(self.play_card)

        body_layout.addLayout(right_layout, 1)
        layout.addLayout(body_layout)
        layout.addStretch()

    def update_data(self, snap):
        connected = snap["connected"]
        playing = snap["playing"]
        c_secs = snap["sess_conn"]
        p_secs = snap["sess_play"]

        # Update visual rings
        self.ring.update_durations(c_secs, p_secs, connected, playing)

        # Update times
        self.conn_time.setText(fmt_full(c_secs))
        self.play_time.setText(fmt_full(p_secs))

        # Update Status details
        if playing:
            self.status_icon.setText("🟢")
            self.status_title.setText("Playing")
            self.status_title.setStyleSheet("color: #4ade80;")
            self.status_desc.setText("Streaming live audio channel")
            self.play_time.setStyleSheet("color: #4ade80;")
        elif connected:
            self.status_icon.setText("🟡")
            self.status_title.setText("Connected (Idle)")
            self.status_title.setStyleSheet("color: #d4a017;")
            self.status_desc.setText("Link active but no media playing")
            self.play_time.setStyleSheet("color: #888888;")
        else:
            self.status_icon.setText("🔴")
            self.status_title.setText("Disconnected")
            self.status_title.setStyleSheet("color: #888888;")
            self.status_desc.setText("Earbuds are out of range or off")
            self.play_time.setStyleSheet("color: #555555;")


# ── Statistics Card Widget ───────────────────────────────────────────────────

class StatsCard(CardWidget):
    def __init__(self, title, parent=None):
        super().__init__(parent)
        layout = QVBoxLayout(self)
        layout.setContentsMargins(20, 20, 20, 20)
        layout.setSpacing(14)

        self.title_lbl = SubtitleLabel(title, self)
        layout.addWidget(self.title_lbl)

        # Subtle card separator
        divider = QFrame(self)
        divider.setFrameShape(QFrame.HLine)
        divider.setStyleSheet("background-color: #2a2a2a; max-height: 1px;")
        layout.addWidget(divider)

        # Connection detail row
        conn_layout = QHBoxLayout()
        conn_title = CaptionLabel("Connection time:", self)
        conn_title.setStyleSheet("color: #888888;")
        
        self.conn_val = BodyLabel("—", self)
        conn_val_font = self.conn_val.font()
        conn_val_font.setFamily("Consolas")
        conn_val_font.setBold(True)
        conn_val_font.setPointSize(10)
        self.conn_val.setFont(conn_val_font)
        
        conn_layout.addWidget(conn_title)
        conn_layout.addStretch()
        conn_layout.addWidget(self.conn_val)
        layout.addLayout(conn_layout)

        # Playback detail row
        play_layout = QHBoxLayout()
        play_title = CaptionLabel("Playback time:", self)
        play_title.setStyleSheet("color: #888888;")
        
        self.play_val = BodyLabel("—", self)
        self.play_val.setFont(conn_val_font)
        self.play_val.setStyleSheet("color: #4ade80;")
        
        play_layout.addWidget(play_title)
        play_layout.addStretch()
        play_layout.addWidget(self.play_val)
        layout.addLayout(play_layout)

    def update_stats(self, conn_secs, play_secs):
        self.conn_val.setText(fmt_h(conn_secs))
        self.play_val.setText(fmt_h(play_secs))


# ── Statistics Page ───────────────────────────────────────────────────────────

class StatsInterface(QWidget):
    def __init__(self, tracker, parent=None):
        super().__init__(parent=parent)
        self.setObjectName("StatsInterface")
        self.tracker = tracker

        layout = QVBoxLayout(self)
        layout.setContentsMargins(36, 24, 36, 24)
        layout.setSpacing(20)

        # Header
        self.title_label = TitleLabel("Statistics", self)
        self.subtitle_label = CaptionLabel("Historical and current interval statistics", self)
        self.subtitle_label.setStyleSheet("color: #888888;")
        layout.addWidget(self.title_label)
        layout.addWidget(self.subtitle_label)

        # Grid view
        grid = QGridLayout()
        grid.setSpacing(20)

        self.today_card = StatsCard("Today", self)
        self.week_card = StatsCard("This Week", self)
        self.month_card = StatsCard("This Month", self)
        self.lifetime_card = StatsCard("Lifetime", self)

        grid.addWidget(self.today_card, 0, 0)
        grid.addWidget(self.week_card, 0, 1)
        grid.addWidget(self.month_card, 1, 0)
        grid.addWidget(self.lifetime_card, 1, 1)

        layout.addLayout(grid)
        layout.addStretch()

    def update_data(self, snap):
        self.today_card.update_stats(snap["today"]["connected"], snap["today"]["playback"])
        self.week_card.update_stats(snap["week"]["connected"], snap["week"]["playback"])
        self.month_card.update_stats(snap["month"]["connected"], snap["month"]["playback"])
        self.lifetime_card.update_stats(snap["lifetime"]["connected"], snap["lifetime"]["playback"])


# ── Session History Page ──────────────────────────────────────────────────────

class HistoryInterface(QWidget):
    def __init__(self, tracker, parent=None):
        super().__init__(parent=parent)
        self.setObjectName("HistoryInterface")
        self.tracker = tracker

        layout = QVBoxLayout(self)
        layout.setContentsMargins(36, 24, 36, 24)
        layout.setSpacing(20)

        # Header
        self.title_label = TitleLabel("Session History", self)
        self.subtitle_label = CaptionLabel("Browse recent audio and connection intervals", self)
        self.subtitle_label.setStyleSheet("color: #888888;")
        layout.addWidget(self.title_label)
        layout.addWidget(self.subtitle_label)

        # Table widget
        self.table = TableWidget(self)
        self.table.setColumnCount(4)
        self.table.setHorizontalHeaderLabels(["Start Time", "End Time", "Connected", "Playback"])

        header = self.table.horizontalHeader()
        header.setSectionResizeMode(0, QHeaderView.Stretch)
        header.setSectionResizeMode(1, QHeaderView.Stretch)
        header.setSectionResizeMode(2, QHeaderView.ResizeToContents)
        header.setSectionResizeMode(3, QHeaderView.ResizeToContents)
        self.table.setEditTriggers(TableWidget.NoEditTriggers)

        layout.addWidget(self.table)

        # Action bar
        actions_layout = QHBoxLayout()
        self.refresh_btn = PushButton("Refresh History", self)
        self.refresh_btn.clicked.connect(self.refresh_history)
        actions_layout.addWidget(self.refresh_btn)
        actions_layout.addStretch()
        layout.addLayout(actions_layout)

        self.refresh_history()

    def showEvent(self, event):
        super().showEvent(event)
        self.refresh_history()

    def refresh_history(self):
        import database as db
        sessions = db.get_recent_sessions(200)

        self.table.setRowCount(0)
        for row_idx, s in enumerate(sessions):
            self.table.insertRow(row_idx)

            start = (s["session_start"] or "")[:19].replace("T", "  ")
            end = (s["session_end"] or "—")[:19].replace("T", "  ")
            c_str = fmt_h(s["connected_secs"])
            p_str = fmt_h(s["playback_secs"])

            self.table.setItem(row_idx, 0, QTableWidgetItem(start))
            self.table.setItem(row_idx, 1, QTableWidgetItem(end))
            self.table.setItem(row_idx, 2, QTableWidgetItem(c_str))

            p_item = QTableWidgetItem(p_str)
            p_item.setForeground(QColor("#4ade80"))
            self.table.setItem(row_idx, 3, p_item)


# ── Settings Page ─────────────────────────────────────────────────────────────

class SettingsInterface(QWidget):
    def __init__(self, tracker, parent=None):
        super().__init__(parent=parent)
        self.setObjectName("SettingsInterface")
        self.tracker = tracker

        layout = QVBoxLayout(self)
        layout.setContentsMargins(36, 24, 36, 24)
        layout.setSpacing(20)

        # Header
        self.title_label = TitleLabel("Settings", self)
        self.subtitle_label = CaptionLabel("Wipe cache databases and view version information", self)
        self.subtitle_label.setStyleSheet("color: #888888;")
        layout.addWidget(self.title_label)
        layout.addWidget(self.subtitle_label)

        # Database Reset Card
        self.db_card = CardWidget(self)
        db_layout = QHBoxLayout(self.db_card)
        db_layout.setContentsMargins(20, 20, 20, 20)

        db_text = QVBoxLayout()
        db_text.setSpacing(4)
        db_title = SubtitleLabel("Clear Session Database", self.db_card)
        db_desc = CaptionLabel("Permanently wipe all session histories and statistics. This cannot be undone.", self.db_card)
        db_desc.setStyleSheet("color: #888888;")
        db_text.addWidget(db_title)
        db_text.addWidget(db_desc)

        self.reset_btn = PushButton("Reset All Data", self.db_card)
        self.reset_btn.clicked.connect(self.confirm_reset)

        db_layout.addLayout(db_text)
        db_layout.addStretch()
        db_layout.addWidget(self.reset_btn)
        layout.addWidget(self.db_card)

        # Application Info Card
        self.info_card = CardWidget(self)
        info_layout = QHBoxLayout(self.info_card)
        info_layout.setContentsMargins(20, 20, 20, 20)

        info_text = QVBoxLayout()
        info_text.setSpacing(4)
        info_title = SubtitleLabel("About EarbudsTracker", self.info_card)
        info_desc = CaptionLabel(
            "Version 2.0 (Native WinUI 3 Fluent Design)\n"
            "Tracks Bluetooth connection state and peak audio rendering times.\n"
            "Data is safely persisted in %APPDATA%\\EarbudsTracker.",
            self.info_card
        )
        info_desc.setStyleSheet("color: #888888;")
        info_text.addWidget(info_title)
        info_text.addWidget(info_desc)

        info_layout.addLayout(info_text)
        info_layout.addStretch()
        layout.addWidget(self.info_card)

        layout.addStretch()

    def confirm_reset(self):
        title = "Reset All Data"
        content = "Are you sure you want to permanently delete all session history and statistics?\n\nThis action cannot be undone."

        w = MessageBox(title, content, self.window())
        if w.exec():
            self.tracker.reset_all()
            InfoBar.success(
                title="Reset Completed",
                content="All database histories and stats have been wiped.",
                orient=Qt.Horizontal,
                isClosable=True,
                position=InfoBarPosition.TOP,
                duration=4000,
                parent=self.window()
            )


# ── MainWindow Container ──────────────────────────────────────────────────────

class TrackerWindow(FluentWindow):
    def __init__(self, tracker):
        super().__init__()
        self.tracker = tracker

        self.setWindowTitle("EarbudsTracker")
        self.resize(800, 580)
        self.setMinimumSize(780, 520)

        # Force native window handle creation so AutoHotkey launcher can detect the hidden window
        self.winId()

        # Initialize subpages
        self.home_interface = HomeInterface(tracker, self)
        self.stats_interface = StatsInterface(tracker, self)
        self.history_interface = HistoryInterface(tracker, self)
        self.settings_interface = SettingsInterface(tracker, self)

        # Add pages to sidebar navigation
        self.addSubInterface(self.home_interface, FIF.HOME, "Dashboard")
        self.addSubInterface(self.stats_interface, FIF.CALENDAR, "Statistics")
        self.addSubInterface(self.history_interface, FIF.HISTORY, "History")
        self.addSubInterface(self.settings_interface, FIF.SETTING, "Settings")

        # Set theme to Dark Mode (Native WinUI 3 aesthetics)
        qfw.setTheme(qfw.Theme.DARK)

        # Schedule UI updates (every 1 second)
        self.timer = QTimer(self)
        self.timer.timeout.connect(self.refresh_data)
        self.timer.start(1000)

        # Hook immediate state notifications
        self.tracker.on_state_change = self.request_refresh

    def refresh_data(self):
        snap = self.tracker.get_snapshot()
        self.home_interface.update_data(snap)
        self.stats_interface.update_data(snap)

    def request_refresh(self):
        QTimer.singleShot(0, self.refresh_data)

    @pyqtSlot()
    def show_window(self):
        self.show()
        self.raise_()
        self.activateWindow()

    def closeEvent(self, event):
        # Override to close window to tray instead of quitting
        event.ignore()
        self.hide()
