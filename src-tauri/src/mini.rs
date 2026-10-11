use crate::prefs::{self, SharedPrefs};
use crate::state_machine::SharedState;
use crate::util::MutexExt;
use crate::windows::{self, get_pet_bounds, MonitorArea, WindowBounds};
use crate::{emit_state, sync_hit};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Manager, PhysicalPosition};

/// Monotonic generation counter — each new animation bumps this; if the counter
/// advances while an animation is running, the old animation stops.
pub type AnimationGeneration = Arc<AtomicU64>;

/// Epoch-millis deadline: peek detection is suppressed until this instant passes.
/// Set when entering mini mode so the entry animation isn't hijacked by peek_in.
/// Newtype wrapper to avoid Tauri managed-state collision with AnimationGeneration.
#[derive(Clone)]
pub struct PeekSuppressDeadline(Arc<AtomicU64>);

impl PeekSuppressDeadline {
    pub fn new() -> Self {
        Self(Arc::new(AtomicU64::new(0)))
    }
    pub fn load(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
    pub fn store(&self, val: u64) {
        self.0.store(val, Ordering::SeqCst);
    }
}

/// Logical-pixel constants — must be scaled by DPI before use in physical coords.
const SNAP_TOLERANCE_LP: f64 = 40.0;
const PEEK_OFFSET_LP: f64 = 25.0;
pub const MINI_OFFSET_RATIO: f64 = 0.486;

/// Sentinel value: suppress peek until mouse exits the pet vicinity.
const PEEK_WAIT_FOR_EXIT: u64 = u64::MAX;

/// Check if peek detection is currently suppressed.
/// Returns `true` if suppression is active (time-based or waiting for mouse exit).
pub fn is_peek_suppressed(app: &AppHandle) -> bool {
    app.try_state::<PeekSuppressDeadline>()
        .map(|d| {
            let deadline = d.load();
            if deadline == PEEK_WAIT_FOR_EXIT {
                return true;
            }
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            now < deadline
        })
        .unwrap_or(false)
}

/// Called by the tick loop when mouse is NOT near the pet and suppression is
/// in "wait for exit" mode — clears the suppression so peek can trigger on
/// the next approach.
pub fn clear_peek_suppression_if_mouse_away(app: &AppHandle) {
    if let Some(d) = app.try_state::<PeekSuppressDeadline>() {
        if d.load() == PEEK_WAIT_FOR_EXIT {
            d.store(0);
        }
    }
}

fn suppress_peek_until_mouse_exit(app: &AppHandle) {
    if let Some(d) = app.try_state::<PeekSuppressDeadline>() {
        d.store(PEEK_WAIT_FOR_EXIT);
    }
}

/// Snap tolerance in physical pixels for the current DPI.
pub fn snap_tolerance(app: &AppHandle) -> i32 {
    (SNAP_TOLERANCE_LP * windows::pet_scale_factor(app)).round() as i32
}

/// If `x` places a `pet_w`-wide pet inside either edge's snap band, push it
/// just outside the band (inward). Used when restoring from mini mode so the
/// pet can't instantly re-snap. Monitors too small for the band are left alone.
pub(crate) fn nudge_out_of_snap_band(x: i32, pet_w: u32, monitor: &MonitorArea, band: i32) -> i32 {
    let left_limit = monitor.x + band;
    let right_limit = monitor.x + monitor.width as i32 - band - pet_w as i32;
    if left_limit > right_limit {
        return x;
    }
    x.clamp(left_limit, right_limit)
}

/// Peek offset in physical pixels for the current DPI.
fn peek_offset(app: &AppHandle) -> i32 {
    (PEEK_OFFSET_LP * windows::pet_scale_factor(app)).round() as i32
}

/// Which screen edge the pet is snapping to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SnapSide {
    Left,
    Right,
}

pub fn should_snap_to_edge(app: &AppHandle) -> Option<EdgeSnap> {
    let bounds = get_pet_bounds(app)?;
    edge_snap_for_bounds(app, &bounds)
}

pub fn edge_snap_for_bounds(app: &AppHandle, bounds: &WindowBounds) -> Option<EdgeSnap> {
    let monitor = windows::monitor_for_bounds(app, bounds)?;
    let tolerance = snap_tolerance(app);
    let screen_left = monitor.x;
    let screen_right = monitor.x + monitor.width as i32;
    let pet_right = bounds.x + bounds.width as i32;

    if screen_right - pet_right <= tolerance {
        Some(EdgeSnap {
            monitor,
            width: bounds.width,
            side: SnapSide::Right,
        })
    } else if bounds.x - screen_left <= tolerance {
        Some(EdgeSnap {
            monitor,
            width: bounds.width,
            side: SnapSide::Left,
        })
    } else {
        None
    }
}

pub struct EdgeSnap {
    pub monitor: MonitorArea,
    pub width: u32,
    pub side: SnapSide,
}

impl EdgeSnap {
    /// The X position where the pet hides at the edge (partially off-screen).
    /// Uses monitor-relative coordinates so it works on any monitor.
    pub fn hidden_x(&self) -> i32 {
        match self.side {
            SnapSide::Right => {
                self.monitor.x + self.monitor.width as i32
                    - (self.width as f64 * MINI_OFFSET_RATIO).round() as i32
            }
            SnapSide::Left => {
                self.monitor.x - (self.width as f64 * (1.0 - MINI_OFFSET_RATIO)).round() as i32
            }
        }
    }
}

pub fn snap_side_key(side: SnapSide) -> &'static str {
    match side {
        SnapSide::Left => "left",
        SnapSide::Right => "right",
    }
}

pub fn snap_side_from_key(side: &str) -> Option<SnapSide> {
    match side {
        "left" => Some(SnapSide::Left),
        "right" => Some(SnapSide::Right),
        _ => None,
    }
}

pub fn remember_snap_for_current_monitor(app: &AppHandle) {
    let Some(snap) = should_snap_to_edge(app) else {
        return;
    };
    let Some(bounds) = get_pet_bounds(app) else {
        return;
    };
    let Some(prefs_state) = app.try_state::<SharedPrefs>() else {
        return;
    };
    let mut prefs = prefs_state.lock_or_recover();
    let placement = prefs
        .monitor_positions
        .entry(snap.monitor.key.clone())
        .or_default();
    placement.x = bounds.x;
    placement.y = bounds.y;
    placement.mini_side = Some(snap_side_key(snap.side).into());
    prefs::save(app, &prefs);
}

fn preferred_snap_side(
    monitor: &MonitorArea,
    bounds: &WindowBounds,
    prefs: &prefs::Prefs,
) -> SnapSide {
    if let Some(saved) = prefs
        .monitor_positions
        .get(&monitor.key)
        .and_then(|placement| placement.mini_side.as_deref())
        .and_then(snap_side_from_key)
    {
        return saved;
    }

    let monitor_mid = monitor.x + monitor.width as i32 / 2;
    let pet_mid = bounds.x + bounds.width as i32 / 2;
    if pet_mid >= monitor_mid {
        SnapSide::Right
    } else {
        SnapSide::Left
    }
}

/// Exit mini mode: restore position, emit idle state, sync hit window.
/// Returns true if the pet was in mini mode and was restored.
pub fn do_exit_mini(app: &AppHandle) -> bool {
    let Some(prefs_state) = app.try_state::<SharedPrefs>() else {
        return false;
    };
    let monitor = get_pet_bounds(app)
        .and_then(|bounds| windows::monitor_for_bounds(app, &bounds));
    let monitor_key = monitor.as_ref().map(|m| m.key.clone());
    let (was_mini, mut restore_x, restore_y) = {
        let mut p = prefs_state.lock_or_recover();
        if !p.mini_mode {
            return false;
        }
        p.mini_mode = false;
        let restore = monitor_key
            .as_ref()
            .and_then(|key| p.monitor_positions.get(key))
            .map(|placement| (placement.x, placement.y))
            .unwrap_or((p.pre_mini_x, p.pre_mini_y));
        prefs::save(app, &p);
        (true, restore.0, restore.1)
    };
    if !was_mini {
        return false;
    }
    // Keep the restored position out of either edge snap band — restoring to
    // a position inside the band re-triggers mini mode on the next tiny drag.
    if let Some(m) = monitor.as_ref() {
        let pet_w = get_pet_bounds(app)
            .map(|b| b.width)
            .unwrap_or(crate::prefs::DEFAULT_PET_DIMENSION);
        restore_x = nudge_out_of_snap_band(restore_x, pet_w, m, snap_tolerance(app) + 20);
    }
    if let Some(pet) = app.get_webview_window("pet") {
        let _ = pet.set_position(PhysicalPosition::new(restore_x, restore_y));
    }
    // Restore the real state from the state machine instead of hardcoding idle
    let (resolved, svg) = if let Some(state) = app.try_state::<SharedState>() {
        let sm = state.lock_or_recover();
        let r = sm.resolve_display_state();
        let s = sm.svg_for_state(&r);
        (r, s)
    } else {
        ("idle".into(), "clyde-idle-follow.svg".into())
    };
    emit_state(app, &resolved, &svg);
    sync_hit(app);
    // Mini mode changed from a non-tray path (drag, exit_mini_mode command) —
    // refresh the tray so its ✓ Mini Mode checkmark stays truthful.
    rebuild_tray(app);
    true
}

/// Enter mini mode: save current position, animate to edge, emit mini state.
/// Returns true if mini mode was entered.
pub fn do_enter_mini(app: &AppHandle) -> bool {
    let Some(prefs_state) = app.try_state::<SharedPrefs>() else {
        return false;
    };
    let bounds = match get_pet_bounds(app) {
        Some(bounds) => bounds,
        None => return false,
    };
    let monitor = match windows::monitor_for_bounds(app, &bounds) {
        Some(monitor) => monitor,
        None => return false,
    };

    // Determine target X: from edge snap (left or right), or default to right edge
    let (hidden_x, side) = if let Some(snap) = edge_snap_for_bounds(app, &bounds) {
        (snap.hidden_x(), snap.side)
    } else {
        // Not near any edge (triggered from tray/context menu) — use preferred side or default to right
        let side = {
            let prefs = prefs_state.lock_or_recover();
            preferred_snap_side(&monitor, &bounds, &prefs)
        };
        let snap = EdgeSnap {
            monitor: monitor.clone(),
            width: bounds.width,
            side,
        };
        (snap.hidden_x(), side)
    };

    {
        let mut p = prefs_state.lock_or_recover();
        p.pre_mini_x = bounds.x;
        p.pre_mini_y = bounds.y;
        p.mini_mode = true;
        // Keep the stored on-screen position for restore: overwriting it with
        // the near-edge release position made exit-mini drop the pet right
        // back into the snap band (re-snap loop). Only fresh entries get coords.
        let is_new = !p.monitor_positions.contains_key(&monitor.key);
        let placement = p.monitor_positions.entry(monitor.key.clone()).or_default();
        if is_new {
            placement.x = bounds.x;
            placement.y = bounds.y;
        }
        placement.mini_side = Some(snap_side_key(side).into());
        prefs::save(app, &p);
    }

    emit_state(app, "mini-idle", "clyde-mini-enter.svg");
    // Suppress peek until the mouse leaves the pet vicinity. Without this,
    // the mouse (still at the drag-release position) triggers peek_in right
    // after the entry animation, pulling the pet 25px away from the edge.
    suppress_peek_until_mouse_exit(app);
    // animate_to_x automatically syncs hit window when animation completes
    animate_to_x(app, hidden_x, 300);
    // Entered via drag or command — refresh tray checkmark (idempotent if the
    // tray handler already rebuilds).
    rebuild_tray(app);
    true
}

/// Rebuild the tray menu with the current language. Reads prefs in a scoped
/// lock and releases it before rebuilding (rebuild takes the same locks).
fn rebuild_tray(app: &AppHandle) {
    let Some(lang) = app
        .try_state::<SharedPrefs>()
        .map(|p| p.lock_or_recover().lang.clone())
    else {
        return;
    };
    crate::tray::rebuild_menu(app, &lang);
}

/// Animate window with parabolic arc (jump transition).
#[allow(dead_code)]
pub fn animate_parabola(
    app: &AppHandle,
    target_x: i32,
    target_y: i32,
    peak_height: i32,
    duration_ms: u64,
) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(pet) = app.get_webview_window("pet") {
            let start_pos = pet.outer_position().unwrap_or_default();
            let sx = start_pos.x as f64;
            let sy = start_pos.y as f64;
            let tx = target_x as f64;
            let ty = target_y as f64;
            let steps = (duration_ms / 16).max(1);
            let mut tick_interval = tokio::time::interval(Duration::from_millis(16));
            for i in 1..=steps {
                tick_interval.tick().await;
                let t = i as f64 / steps as f64;
                let eased = t * (2.0 - t);
                let x = sx + (tx - sx) * eased;
                // Parabolic arc: -4 * peak * t * (t - 1)
                let arc = -4.0 * peak_height as f64 * t * (t - 1.0);
                let y = sy + (ty - sy) * eased - arc;
                let _ = pet.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
            }
        }
        sync_hit(&app);
    });
}

/// Peek in: slide to peek position (absolute, not relative — prevents drift)
pub fn peek_in(app: &AppHandle) {
    if let Some(snap) = should_snap_to_edge(app) {
        let offset = peek_offset(app);
        let target_x = match snap.side {
            SnapSide::Right => snap.hidden_x() - offset,
            SnapSide::Left => snap.hidden_x() + offset,
        };
        animate_to_x(app, target_x, 200);
    }
}

/// Peek out: slide back to hidden position
pub fn peek_out(app: &AppHandle) {
    if let Some(snap) = should_snap_to_edge(app) {
        animate_to_x(app, snap.hidden_x(), 200);
    }
}

/// Animate window to target X in steps (ease-out quadratic).
/// Bumps the generation counter so any in-flight animation stops.
/// Syncs hit window only if this animation was not superseded.
pub fn animate_to_x(app: &AppHandle, target_x: i32, duration_ms: u64) {
    // Bump generation — any older animation will see its gen is stale and stop
    let gen = app
        .try_state::<AnimationGeneration>()
        .map(|g| {
            let new = g.fetch_add(1, Ordering::SeqCst) + 1;
            (g.inner().clone(), new)
        });

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let (gen_arc, my_gen) = match gen {
            Some(pair) => pair,
            None => {
                sync_hit(&app);
                return;
            }
        };

        if let Some(pet) = app.get_webview_window("pet") {
            let start_pos = pet.outer_position().unwrap_or_default();
            let start_x = start_pos.x;
            let start_y = start_pos.y;
            if start_x == target_x {
                sync_hit(&app);
                return;
            }

            let steps = (duration_ms / 16).max(1);
            let mut interval = tokio::time::interval(Duration::from_millis(16));
            for i in 1..=steps {
                interval.tick().await;
                if gen_arc.load(Ordering::SeqCst) != my_gen {
                    return; // superseded by a newer animation
                }
                let t = i as f64 / steps as f64;
                let eased = t * (2.0 - t); // ease-out quad
                let x = (start_x as f64 + (target_x - start_x) as f64 * eased).round() as i32;
                let _ = pet.set_position(PhysicalPosition::new(x, start_y));
            }
        }
        if gen_arc.load(Ordering::SeqCst) == my_gen {
            sync_hit(&app);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(x: i32, width: u32) -> MonitorArea {
        MonitorArea {
            key: "test".into(),
            x,
            y: 0,
            width,
            height: 1040,
        }
    }

    #[test]
    fn test_nudge_pushes_out_of_left_band() {
        // Left snap triggers when x <= monitor.x + band; restore must clear it.
        let m = monitor(0, 1920);
        assert_eq!(nudge_out_of_snap_band(20, 96, &m, 60), 60);
    }

    #[test]
    fn test_nudge_pushes_out_of_right_band() {
        // Right snap triggers when pet_right >= monitor_right - band.
        let m = monitor(0, 1920);
        assert_eq!(nudge_out_of_snap_band(1820, 96, &m, 60), 1764);
    }

    #[test]
    fn test_nudge_keeps_mid_screen_position() {
        let m = monitor(0, 1920);
        assert_eq!(nudge_out_of_snap_band(960, 96, &m, 60), 960);
    }

    #[test]
    fn test_nudge_uses_monitor_origin() {
        // Work area starts at x=60 (taskbar on the left) — limits are relative.
        let m = monitor(60, 1860);
        assert_eq!(nudge_out_of_snap_band(70, 96, &m, 60), 120);
    }

    #[test]
    fn test_nudge_tiny_monitor_left_alone() {
        // Band wider than the monitor: don't fight it.
        let m = monitor(0, 120);
        assert_eq!(nudge_out_of_snap_band(10, 96, &m, 60), 10);
    }

    #[test]
    fn test_hidden_x_left_side_visible_strip_sits_beside_taskbar() {
        // Work area x=60 (60px taskbar on the physical left edge), pet 96 wide.
        let m = monitor(60, 1860);
        let snap = EdgeSnap {
            monitor: m,
            width: 96,
            side: SnapSide::Left,
        };
        let hidden = snap.hidden_x();
        // Pet partially off the work area's left edge, but the visible strip
        // (work_area_right - pet_right equivalent) lies INSIDE the work area,
        // i.e. beside the taskbar, not underneath it.
        assert!(hidden < 60, "pet must be partially off-screen: {hidden}");
        assert!(hidden + 96 > 60, "visible strip must start at the taskbar edge: {hidden}");
    }

    #[test]
    fn test_hidden_x_right_side_visible_strip_sits_beside_taskbar() {
        let m = monitor(60, 1860);
        let snap = EdgeSnap {
            monitor: m,
            width: 96,
            side: SnapSide::Right,
        };
        let hidden = snap.hidden_x();
        let work_right = 60 + 1860;
        assert!(hidden + 96 > work_right, "pet must be partially off-screen: {hidden}");
        assert!(hidden < work_right, "visible strip must sit inside the work area: {hidden}");
    }
}
