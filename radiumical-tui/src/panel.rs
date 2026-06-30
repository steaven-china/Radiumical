//! Panel manager — arranges floating panels side-by-side without overlap.
//!
//! Panels float over the output area. When multiple are open, they tile
//! horizontally so they never cover each other. Output area is unchanged.
//!
//! Each panel has:
//! - A draggable title bar (mouse down on header → drag → release)
//! - A close button (×) in the top-right corner
//! - An optional position override (from drag)

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, BorderType, Borders, Clear};
use ratatui::Frame;

// ── Panel identity ──

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PanelId {
    Dashboard,
    ProviderPicker,
    Settings,
    Help,
    SessionList,
    SubAgents,
    Mcp,
    Plan,
    Memory,
    Agents,
    Diagnostics,
    Outline,
    Perf,
    Confirm,
}

/// How a panel participates in layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelKind {
    /// Normal horizontal tiling with other Tiling panels.
    Tiling,
    /// Centered modal overlay, always rendered on top of everything.
    Overlay,
    /// Fixed at top-right corner, does not participate in tiling.
    FixedTopRight,
}

impl PanelId {
    pub fn title(&self) -> &'static str {
        match self {
            PanelId::Dashboard => " Dashboard ",
            PanelId::ProviderPicker => " Provider / Model ",
            PanelId::Settings => " Settings ",
            PanelId::Help => " Help ",
            PanelId::SessionList => " Sessions ",
            PanelId::SubAgents => " Sub-Agents ",
            PanelId::Mcp => " MCP Servers ",
            PanelId::Plan => " Plan ",
            PanelId::Memory => " Memory ",
            PanelId::Agents => " Agent Roles ",
            PanelId::Diagnostics => " Diagnostics ",
            PanelId::Outline => " Outline ",
            PanelId::Perf => " Perf ",
            PanelId::Confirm => " Confirm ",
        }
    }

    /// Close button text rendered in the top-right corner.
    pub fn close_btn(&self) -> &'static str {
        " × "
    }

    pub fn kind(&self) -> PanelKind {
        match self {
            PanelId::Confirm => PanelKind::Overlay,
            PanelId::Perf => PanelKind::FixedTopRight,
            _ => PanelKind::Tiling,
        }
    }

    /// Default size as (width_pct, height_pct) of the output area.
    pub fn default_size(&self) -> (f32, f32) {
        match self {
            PanelId::Outline => (0.35, 0.70),
            PanelId::Diagnostics => (0.50, 0.60),
            PanelId::Memory => (0.45, 0.50),
            _ => (0.50, 0.65),
        }
    }
}

// ── Layout slot ──

#[derive(Debug, Clone, Copy)]
pub struct PanelSlot {
    pub id: PanelId,
    pub rect: Rect,
}

// ── Drag state ──

#[derive(Debug, Clone, Copy, Default)]
pub struct DragState {
    /// The panel being dragged.
    pub panel: Option<PanelId>,
    /// Mouse position when drag started.
    pub start_mouse: Option<(u16, u16)>,
    /// Panel position when drag started.
    pub start_pos: Option<(i32, i32)>,
}

// ── Per-panel position override (from drag) ──

#[derive(Debug, Clone, Copy)]
struct PanelPos {
    x: i32,
    y: i32,
}

// ── Panel manager ──

pub struct PanelManager {
    open: Vec<PanelId>,
    /// Position overrides from dragging.
    positions: std::collections::HashMap<PanelId, PanelPos>,
    /// Current drag state.
    pub drag: DragState,
}

impl PanelManager {
    pub fn new() -> Self {
        Self {
            open: Vec::new(),
            positions: std::collections::HashMap::new(),
            drag: DragState::default(),
        }
    }

    pub fn toggle(&mut self, id: PanelId) -> bool {
        if let Some(pos) = self.open.iter().position(|p| *p == id) {
            self.open.remove(pos);
            self.positions.remove(&id);
            false
        } else {
            self.open.push(id);
            true
        }
    }

    pub fn open(&mut self, id: PanelId) {
        if !self.open.contains(&id) {
            self.open.push(id);
        }
    }

    pub fn close(&mut self, id: PanelId) {
        self.open.retain(|p| *p != id);
        self.positions.remove(&id);
    }

    #[allow(dead_code)]
    pub fn close_all(&mut self) {
        self.open.clear();
        self.positions.clear();
        self.drag = DragState::default();
    }

    pub fn is_open(&self, id: PanelId) -> bool {
        self.open.contains(&id)
    }

    #[allow(dead_code)]
    pub fn has_any(&self) -> bool {
        !self.open.is_empty()
    }

    #[allow(dead_code)]
    pub fn count(&self) -> usize {
        self.open.len()
    }

    #[allow(dead_code)]
    pub fn open_ids(&self) -> &[PanelId] {
        &self.open
    }

    // ── Drag handling ──

    /// Start dragging a panel. Call on mouse down on the title bar.
    pub fn drag_start(&mut self, id: PanelId, mouse_x: u16, mouse_y: u16, slot: &PanelSlot) {
        self.drag = DragState {
            panel: Some(id),
            start_mouse: Some((mouse_x, mouse_y)),
            start_pos: Some((slot.rect.x as i32, slot.rect.y as i32)),
        };
    }

    /// Update drag position. Call on mouse move while dragging.
    pub fn drag_move(&mut self, mouse_x: u16, mouse_y: u16) {
        if let (Some((sx, sy)), Some((px, py)), Some(panel)) = (
            self.drag.start_mouse,
            self.drag.start_pos,
            self.drag.panel,
        ) {
            let dx = mouse_x as i32 - sx as i32;
            let dy = mouse_y as i32 - sy as i32;
            self.positions.insert(panel, PanelPos {
                x: px + dx,
                y: py + dy,
            });
        }
    }

    /// End dragging. Call on mouse up.
    pub fn drag_end(&mut self) {
        self.drag = DragState::default();
    }

    /// Is a drag in progress?
    pub fn is_dragging(&self) -> bool {
        self.drag.panel.is_some()
    }

    /// Check if a mouse position is on a close button of any panel.
    /// Returns the PanelId if clicked.
    pub fn hit_close_button(&self, mouse_x: u16, mouse_y: u16, slots: &[PanelSlot]) -> Option<PanelId> {
        for slot in slots {
            let r = slot.rect;
            // Close button is at top-right: " × " = 3 chars, inside border
            let btn_x = r.x + r.width.saturating_sub(4);
            let btn_y = r.y; // top border row
            if mouse_x >= btn_x && mouse_x < btn_x + 3 && mouse_y == btn_y {
                return Some(slot.id);
            }
        }
        None
    }

    /// Check if a mouse position is on a title bar of any panel.
    /// Returns the PanelId if on the title bar row (for drag initiation).
    pub fn hit_title_bar(&self, mouse_x: u16, mouse_y: u16, slots: &[PanelSlot]) -> Option<PanelId> {
        for slot in slots {
            let r = slot.rect;
            // Title bar is the top border row
            if mouse_y == r.y && mouse_x >= r.x && mouse_x < r.x + r.width {
                // Exclude the close button area
                let btn_x = r.x + r.width.saturating_sub(4);
                if mouse_x >= btn_x {
                    continue;
                }
                return Some(slot.id);
            }
        }
        None
    }

    // ── Layout ──

    /// Compute layout for all open panels within the given area.
    /// Tiling panels tile horizontally, centered vertically. No overlap.
    /// Overlay panels are centered modals (rendered last = on top).
    /// FixedTopRight panels are pinned to the top-right corner.
    pub fn layout(&self, area: Rect) -> Vec<PanelSlot> {
        if self.open.is_empty() {
            return Vec::new();
        }

        let mut slots = Vec::with_capacity(self.open.len());

        // ── Tiling panels ──
        let tiling: Vec<&PanelId> = self
            .open
            .iter()
            .filter(|id| id.kind() == PanelKind::Tiling)
            .collect();

        if !tiling.is_empty() {
            let n = tiling.len() as u16;
            let gap = 1u16;
            let total_gap = gap * n.saturating_sub(1);

            // Use max requested width across panels, but cap so they fit.
            let max_requested_w: u16 = tiling
                .iter()
                .map(|id| {
                    let (wp, _) = id.default_size();
                    (area.width as f32 * wp) as u16
                })
                .max()
                .unwrap_or(20)
                .max(20);
            let panel_w = max_requested_w
                .min((area.width.saturating_sub(total_gap)) / n)
                .max(20);

            let total_w = panel_w * n + total_gap;
            let start_x = area.x + area.width.saturating_sub(total_w) / 2;

            for (i, &id) in tiling.iter().enumerate() {
                let (_, hp) = id.default_size();
                let panel_h = (area.height as f32 * hp) as u16;
                let default_x = start_x + (panel_w + gap) * i as u16;
                let default_y = area.y + area.height.saturating_sub(panel_h) / 2;

                let (x, y) = if let Some(pos) = self.positions.get(id) {
                    let cx = pos.x.max(area.x as i32)
                        .min((area.x + area.width).saturating_sub(panel_w) as i32);
                    let cy = pos.y.max(area.y as i32)
                        .min((area.y + area.height).saturating_sub(panel_h) as i32);
                    (cx as u16, cy as u16)
                } else {
                    (default_x, default_y)
                };

                slots.push(PanelSlot {
                    id: *id,
                    rect: Rect {
                        x,
                        y,
                        width: panel_w,
                        height: panel_h,
                    },
                });
            }
        }

        // ── FixedTopRight panels ──
        for &id in self
            .open
            .iter()
            .filter(|id| id.kind() == PanelKind::FixedTopRight)
        {
            let w = 32u16.min(area.width.saturating_sub(2));
            let h = 1u16;
            let x = area.x + area.width.saturating_sub(w + 2);
            let y = area.y + 1;
            slots.push(PanelSlot {
                id,
                rect: Rect {
                    x,
                    y,
                    width: w,
                    height: h,
                },
            });
        }

        // ── Overlay panels (always last = rendered on top) ──
        for &id in self
            .open
            .iter()
            .filter(|id| id.kind() == PanelKind::Overlay)
        {
            let (w, h) = match id {
                PanelId::Confirm => {
                    let w = (area.width as f32 * 0.4) as u16;
                    let h = 5u16;
                    (w.max(20).min(area.width.saturating_sub(2)), h)
                }
                _ => {
                    let w = (area.width as f32 * 0.5) as u16;
                    let h = (area.height as f32 * 0.5) as u16;
                    (w.max(20), h.max(5))
                }
            };
            let x = area.x + area.width.saturating_sub(w) / 2;
            let y = area.y + area.height.saturating_sub(h) / 2;
            slots.push(PanelSlot {
                id,
                rect: Rect {
                    x,
                    y,
                    width: w,
                    height: h,
                },
            });
        }

        slots
    }

    /// Return the y-offset below which top-area panels end.
    /// Used by toasts to avoid overlapping top panels.
    pub fn top_occupied_bottom(&self, area: Rect) -> u16 {
        let mut max_bottom = area.y;
        for &id in &self.open {
            if id.kind() == PanelKind::FixedTopRight {
                // Perf panel at top-right: occupies area.y + 1 row
                let bottom = area.y + 2;
                if bottom > max_bottom {
                    max_bottom = bottom;
                }
            }
        }
        max_bottom
    }

    /// Render a panel frame with draggable title bar and close button.
    /// This is the reusable frame builder for all panels.
    pub fn render_panel_frame(
        f: &mut Frame,
        slot: &PanelSlot,
        title: &str,
        border_color: Color,
        bg_color: Color,
    ) {
        let r = slot.rect;
        f.render_widget(Clear, r);

        // Build title with close button: " Title ──── × "
        let close_btn = slot.id.close_btn();
        let title_w = ratatui::text::Line::from(title).width();
        let fill_w = (r.width as usize).saturating_sub(title_w + close_btn.len() + 6);
        let title_line = format!(" {}{}{}", title, "─".repeat(fill_w.max(1)), close_btn);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(title_line)
            .title_style(Style::default().fg(Color::Rgb(180, 180, 190)).add_modifier(Modifier::BOLD))
            .border_style(Style::default().fg(border_color))
            .style(Style::default().bg(bg_color));
        f.render_widget(block, r);
    }

    /// Check if a click is on the close button of a specific panel.
    #[allow(dead_code)]
    pub fn is_close_click(&self, mouse_x: u16, mouse_y: u16, slot: &PanelSlot) -> bool {
        let r = slot.rect;
        let btn_x = r.x + r.width.saturating_sub(4);
        let btn_y = r.y;
        mouse_x >= btn_x && mouse_x < btn_x + 3 && mouse_y == btn_y
    }

    /// Check if a click is on the title bar of a specific panel (for drag).
    #[allow(dead_code)]
    pub fn is_title_bar_click(&self, mouse_x: u16, mouse_y: u16, slot: &PanelSlot) -> bool {
        let r = slot.rect;
        let btn_x = r.x + r.width.saturating_sub(4);
        mouse_y == r.y && mouse_x >= r.x && mouse_x < btn_x
    }
}

impl Default for PanelManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area() -> Rect {
        Rect { x: 0, y: 0, width: 100, height: 40 }
    }

    #[test]
    fn test_toggle() {
        let mut pm = PanelManager::new();
        assert!(!pm.is_open(PanelId::Dashboard));
        pm.toggle(PanelId::Dashboard);
        assert!(pm.is_open(PanelId::Dashboard));
        pm.toggle(PanelId::Dashboard);
        assert!(!pm.is_open(PanelId::Dashboard));
    }

    #[test]
    fn test_close_all() {
        let mut pm = PanelManager::new();
        pm.open(PanelId::Dashboard);
        pm.open(PanelId::Settings);
        assert_eq!(pm.count(), 2);
        pm.close_all();
        assert_eq!(pm.count(), 0);
    }

    #[test]
    fn test_layout_two_panels_no_overlap() {
        let mut pm = PanelManager::new();
        pm.open(PanelId::Dashboard);
        pm.open(PanelId::ProviderPicker);
        let slots = pm.layout(area());
        assert_eq!(slots.len(), 2);
        let a = slots[0].rect;
        let b = slots[1].rect;
        assert!(a.x + a.width <= b.x || b.x + b.width <= a.x);
    }

    #[test]
    fn test_layout_three_panels_no_overlap() {
        let mut pm = PanelManager::new();
        pm.open(PanelId::Dashboard);
        pm.open(PanelId::ProviderPicker);
        pm.open(PanelId::Settings);
        let slots = pm.layout(area());
        assert_eq!(slots.len(), 3);
        for i in 0..slots.len() {
            for j in i + 1..slots.len() {
                let a = slots[i].rect;
                let b = slots[j].rect;
                let no_overlap = a.x + a.width <= b.x
                    || b.x + b.width <= a.x
                    || a.y + a.height <= b.y
                    || b.y + b.height <= a.y;
                assert!(no_overlap, "panels {} and {} overlap", i, j);
            }
        }
    }

    #[test]
    fn test_single_panel_centered() {
        let mut pm = PanelManager::new();
        pm.open(PanelId::Dashboard);
        let slots = pm.layout(area());
        assert_eq!(slots.len(), 1);
        let r = slots[0].rect;
        assert!(r.x > 0, "should be centered horizontally");
        assert!(r.y > 0, "should be centered vertically");
    }

    #[test]
    fn test_drag_move() {
        let mut pm = PanelManager::new();
        pm.open(PanelId::Dashboard);
        let slots = pm.layout(area());
        let slot = &slots[0];

        // Start drag at (50, 20)
        pm.drag_start(PanelId::Dashboard, 50, 20, slot);
        assert!(pm.is_dragging());

        // Move to (55, 22)
        pm.drag_move(55, 22);
        let new_slots = pm.layout(area());
        let new_r = new_slots[0].rect;
        // Position should have shifted by (5, 2)
        assert_eq!(new_r.x, slot.rect.x + 5);
        assert_eq!(new_r.y, slot.rect.y + 2);

        // End drag
        pm.drag_end();
        assert!(!pm.is_dragging());
    }

    #[test]
    fn test_close_button_hit() {
        let mut pm = PanelManager::new();
        pm.open(PanelId::Dashboard);
        let slots = pm.layout(area());
        let slot = &slots[0];

        // Close button is at top-right: x + width - 4, y
        let btn_x = slot.rect.x + slot.rect.width - 4;
        let btn_y = slot.rect.y;
        assert!(pm.is_close_click(btn_x + 1, btn_y, slot));
        assert!(!pm.is_close_click(0, 0, slot));
    }

    #[test]
    fn test_title_bar_hit() {
        let mut pm = PanelManager::new();
        pm.open(PanelId::Dashboard);
        let slots = pm.layout(area());
        let slot = &slots[0];

        // Title bar is top row, excluding close button
        let mid_x = slot.rect.x + slot.rect.width / 2;
        let top_y = slot.rect.y;
        assert!(pm.is_title_bar_click(mid_x, top_y, slot));

        // Close button area should NOT be a title bar hit
        let btn_x = slot.rect.x + slot.rect.width - 3;
        assert!(!pm.is_title_bar_click(btn_x, top_y, slot));
    }

    #[test]
    fn test_drag_clamped_to_area() {
        let mut pm = PanelManager::new();
        pm.open(PanelId::Dashboard);
        let slots = pm.layout(area());
        let slot = &slots[0];

        // Drag way off-screen
        pm.drag_start(PanelId::Dashboard, 50, 20, slot);
        pm.drag_move(999, 999);
        let new_slots = pm.layout(area());
        let new_r = new_slots[0].rect;
        // Should be clamped to area bounds
        assert!(new_r.x + new_r.width <= area().x + area().width);
        assert!(new_r.y + new_r.height <= area().y + area().height);
    }

    #[test]
    fn test_panel_id_title() {
        assert_eq!(PanelId::Dashboard.title(), " Dashboard ");
        assert_eq!(PanelId::Settings.title(), " Settings ");
        assert_eq!(PanelId::Confirm.title(), " Confirm ");
    }
}
