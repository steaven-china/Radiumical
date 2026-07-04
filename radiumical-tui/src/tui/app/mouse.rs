use crate::tui::app::App;
use crossterm::event::{MouseButton, MouseEventKind};
use ratatui::layout::Rect;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

const DOUBLE_CLICK_MS: u64 = 300;

impl App {
    pub fn handle_mouse(
        &mut self,
        kind: MouseEventKind,
        row: u16,
        col: u16,
        output_top: u16,
        output_h: u16,
    ) {
        if self.welcome {
            return;
        }

        // Compute current panel slots for hit-testing.
        let output_area = Rect {
            x: 0,
            y: output_top,
            width: self.viewport.width as u16,
            height: output_h,
        };
        let slots = self.panels.layout(output_area);

        match kind {
            // ── Scroll ──
            MouseEventKind::ScrollDown => {
                let bi = self.hovered_block
                    .or_else(|| self.block_at_row(row, output_top, output_h));
                if !self.scroll_tool_result(bi, 1) {
                    self.scroll_up(1.0);
                }
            }
            MouseEventKind::ScrollUp => {
                let bi = self.hovered_block
                    .or_else(|| self.block_at_row(row, output_top, output_h));
                if !self.scroll_tool_result(bi, -1) {
                    self.scroll_down(1.0);
                }
            }

            // ── Mouse move (hover tracking) ──
            MouseEventKind::Moved => {
                self.hovered_block = self.block_at_row(row, output_top, output_h);
            }

            // ── Mouse down ──
            MouseEventKind::Down(btn) => {
                if btn == MouseButton::Right {
                    return;
                }

                // 1. Panel close button click
                if let Some(panel_id) = self.panels.hit_close_button(col, row, &slots) {
                    self.close_panel_by_id(panel_id);
                    return;
                }

                // 2. Panel title bar drag start
                if let Some(panel_id) = self.panels.hit_title_bar(col, row, &slots) {
                    if let Some(slot) = slots.iter().find(|s| s.id == panel_id) {
                        self.panels.drag_start(panel_id, col, row, slot);
                    }
                    return;
                }

                // 3. If dragging is active, ignore other clicks
                if self.panels.is_dragging() {
                    return;
                }

                // 4. Scrollbar click
                let on_scrollbar = col >= self.viewport.width as u16;
                let total = self.viewport.rendered_total;
                let needs_scrollbar = total > self.viewport.visible_lines;
                if on_scrollbar
                    && needs_scrollbar
                    && row > output_top
                    && row < output_top + output_h
                {
                    self.viewport.scrollbar_dragging = true;
                    self.set_scroll_from_thumb(row, output_top, output_h);
                    return;
                }

                // 5. Help board drag
                if self.help_board.hit_border(
                    row,
                    col,
                    Rect {
                        x: 0,
                        y: output_top,
                        width: 80,
                        height: output_h,
                    },
                ) {
                    self.help_board.start_drag(col, row);
                    return;
                }

                // 6. Tool call double-click
                if let Some(bi) = self.block_at_row(row, output_top, output_h) {
                    let now = Instant::now();
                    let is_double = self.last_click.is_some_and(|(t, r, c)| {
                        t.elapsed() < Duration::from_millis(DOUBLE_CLICK_MS)
                            && (r == row || r.abs_diff(row) <= 1)
                            && (c == col || c.abs_diff(col) <= 1)
                    });
                    if is_double {
                        self.toggle_tool_call(bi);
                        self.last_click = None;
                    } else {
                        self.last_click = Some((now, row, col));
                    }
                } else {
                    self.last_click = Some((Instant::now(), row, col));
                }
            }

            // ── Drag ──
            MouseEventKind::Drag(_) => {
                // Panel drag takes priority
                if self.panels.is_dragging() {
                    self.panels.drag_move(col, row);
                    return;
                }
                if self.viewport.scrollbar_dragging {
                    self.set_scroll_from_thumb(row, output_top, output_h);
                }
            }

            // ── Mouse up ──
            MouseEventKind::Up(_) => {
                if self.panels.is_dragging() {
                    self.panels.drag_end();
                }
                self.viewport.scrollbar_dragging = false;
            }

            _ => {}
        }
    }

    /// Close a panel by its PanelId, also resetting the associated legacy flag.
    fn close_panel_by_id(&mut self, id: crate::panel::PanelId) {
        use crate::panel::PanelId;
        self.panels.close(id);
        match id {
            PanelId::Dashboard => self.dashboard.visible = false,
            PanelId::ProviderPicker => {
                self.overlays.model_picker = false;
                self.provider_picker.close();
            }
            PanelId::Settings => {
                self.overlays.settings = false;
                self.settings_board.visible = false;
            }
            PanelId::Help => self.overlays.help = false,
            PanelId::Confirm => {
                self.confirm.visible = false;
            }
            PanelId::Perf => {
                self.overlays.perf = false;
            }
            PanelId::SessionList => {
                self.session_tui.close();
            }
            _ => {}
        }
    }

    fn set_scroll_from_thumb(&mut self, row: u16, output_top: u16, output_h: u16) {
        let total = self.viewport.rendered_total;
        let vis = self.viewport.visible_lines.max(1);
        if total <= vis {
            self.viewport.scroll = 0.0;
            self.viewport.stick_to_bottom = true;
            return;
        }
        let sb_h = output_h.saturating_sub(1) as f32;
        if sb_h <= 0.0 {
            return;
        }
        let rel = (row.saturating_sub(output_top).saturating_sub(1)) as f32;
        let progress = (rel / sb_h).clamp(0.0, 1.0);
        let max_scroll = (total - vis) as f32;
        self.viewport.scroll = (progress * max_scroll).round();
        self.viewport.stick_to_bottom = self.viewport.scroll >= max_scroll - 0.5;
    }

    fn block_at_row(&self, screen_row: u16, output_top: u16, output_h: u16) -> Option<usize> {
        if screen_row < output_top || screen_row >= output_top + output_h {
            return None;
        }
        let blocks = &self.blocks;
        let vis = self.viewport.visible_lines;
        let total = self.viewport.rendered_total;
        let start = self.scroll_start(total, vis);
        let rel = (screen_row - output_top) as usize + start;
        let mut off = 0usize;
        for (i, b) in blocks.iter().enumerate() {
            let end = off + b.height;
            if rel >= off && rel < end {
                return Some(i);
            }
            off = end;
        }
        None
    }

    fn scroll_tool_result(&mut self, bi: Option<usize>, delta: i64) -> bool {
        const MAX_RESULT_VIS: usize = 10;
        let bi = match bi {
            Some(bi) => bi,
            None => return false,
        };
        let block = match self.blocks.get(bi) {
            Some(b) => b,
            None => return false,
        };
        let (_name, _args, result, expanded) = match &block.kind {
            crate::layout::BlockKind::ToolCall {
                name,
                args,
                result,
                expanded,
                ..
            } => (name, args, result, *expanded),
            _ => return false,
        };
        if !expanded {
            return false;
        }
        let content_w = self.viewport.width.saturating_sub(4 + 1).max(1);
        let wrapped_count = crate::layout::wrapped_tool_result_lines(result, content_w).len();
        if wrapped_count <= MAX_RESULT_VIS {
            return false;
        }
        let key = tool_call_key(block);
        let current = self.tool_result_scroll.get(&key).copied().unwrap_or(0) as i64;
        let max_scroll = (wrapped_count - MAX_RESULT_VIS) as i64;
        let next = (current + delta).clamp(0, max_scroll) as usize;
        self.tool_result_scroll.insert(key, next);
        true
    }

    fn toggle_tool_call(&mut self, block_idx: usize) {
        let block = match self.blocks.get(block_idx) {
            Some(b) => b,
            None => return,
        };
        if !matches!(block.kind, crate::layout::BlockKind::ToolCall { .. }) {
            return;
        }
        let key = tool_call_key(block);
        let current = self.tool_expanded.get(&key).copied().unwrap_or(false);
        self.tool_expanded.insert(key, !current);
    }
}

pub fn tool_call_key(block: &crate::layout::Block) -> String {
    // Use the unique ID embedded after the box top line's trailing \x02.
    if let Some(first) = block.source_lines.first() {
        if let Some(id) = first.split('\x02').nth(1) {
            return id.to_string();
        }
    }
    // Fallback for legacy/tool-less blocks.
    let mut hasher = DefaultHasher::new();
    std::mem::discriminant(&block.kind).hash(&mut hasher);
    block.source_lines.first().hash(&mut hasher);
    block.source_lines.get(1).hash(&mut hasher);
    format!("fallback_{}", hasher.finish())
}
