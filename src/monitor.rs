//! Cursor-aware monitor detection via X11/RandR.
//!
//! Returns the geometry of the monitor containing the mouse cursor.
//! Returns None on Wayland or if the X11 query fails.

use x11rb::connection::Connection;
use x11rb::protocol::randr;
use x11rb::protocol::xproto::ConnectionExt;

/// Physical pixel geometry of a monitor.
pub struct MonitorRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Return the monitor containing the mouse cursor.
pub fn cursor_monitor() -> Option<MonitorRect> {
    let (conn, _) = x11rb::connect(None).ok()?;
    let root = conn.setup().roots.first()?.root;

    let pointer = conn.query_pointer(root).ok()?.reply().ok()?;
    let (cx, cy) = (pointer.root_x as i32, pointer.root_y as i32);

    let monitors = randr::get_monitors(&conn, root, true).ok()?.reply().ok()?;

    monitors
        .monitors
        .iter()
        .find(|m| {
            let (mx, my, mw, mh) = (m.x as i32, m.y as i32, m.width as i32, m.height as i32);
            cx >= mx && cx < mx + mw && cy >= my && cy < my + mh
        })
        .or_else(|| monitors.monitors.iter().find(|m| m.primary))
        .or(monitors.monitors.first())
        .map(|m| MonitorRect {
            x: m.x as i32,
            y: m.y as i32,
            width: m.width as u32,
            height: m.height as u32,
        })
}
