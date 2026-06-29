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
        _col: u16,
        output_top: u16,
        output_h: u16,
    ) {
        if self.welcome {
            return;
        }
        match kind {
            MouseEventKind::ScrollDown => {
                if !self.scroll_hovered_tool_result(1) {
                    self.scroll_up(1.0);
                }
            }
            MouseEventKind::ScrollUp => {
                if !self.scroll_hovered_tool_result(-1) {
                    self.scroll_down(1.0);
                }
            }
            MouseEventKind::Moved => {
                self.hovered_block = self.block_at_row(row, output_top, output_h);
            }
            MouseEventKind::Down(btn) => {
                if btn == MouseButton::Right {
                    return;
                }
                let on_scrollbar = _col >= self.output_width as u16;
                let total = self.rendered_total;
                let needs_scrollbar = total > self.output_vis;
                if on_scrollbar
                    && needs_scrollbar
                    && row > output_top
                    && row < output_top + output_h
                {
                    self.scrollbar_dragging = true;
                    self.set_scroll_from_thumb(row, output_top, output_h);
                    return;
                }
                if self.help_board.hit_border(
                    row,
                    _col,
                    Rect {
                        x: 0,
                        y: output_top,
                        width: 80,
                        height: output_h,
                    },
                ) {
                    self.help_board.start_drag(_col, row);
                    return;
                }
                if let Some(bi) = self.block_at_row(row, output_top, output_h) {
                    let now = Instant::now();
                    let is_double = self.last_click.is_some_and(|(t, r, c)| {
                        t.elapsed() < Duration::from_millis(DOUBLE_CLICK_MS)
                            && (r == row || r.abs_diff(row) <= 1)
                            && (c == _col || c.abs_diff(_col) <= 1)
                    });
                    if is_double {
                        self.toggle_tool_call(bi);
                        self.last_click = None;
                    } else {
                        self.last_click = Some((now, row, _col));
                    }
                } else {
                    self.last_click = Some((Instant::now(), row, _col));
                }
            }
            MouseEventKind::Drag(_) if self.scrollbar_dragging => {
                self.set_scroll_from_thumb(row, output_top, output_h);
            }
            MouseEventKind::Up(_) => {
                self.scrollbar_dragging = false;
            }
            _ => {}
        }
    }

    fn set_scroll_from_thumb(&mut self, row: u16, output_top: u16, output_h: u16) {
        let total = self.rendered_total;
        let vis = self.output_vis.max(1);
        if total <= vis {
            self.scroll = 0.0;
            self.stick_to_bottom = true;
            return;
        }
        let sb_h = output_h.saturating_sub(1) as f32;
        if sb_h <= 0.0 {
            return;
        }
        let rel = (row.saturating_sub(output_top).saturating_sub(1)) as f32;
        let progress = (rel / sb_h).clamp(0.0, 1.0);
        let max_scroll = (total - vis) as f32;
        self.scroll = (progress * max_scroll).round();
        self.stick_to_bottom = self.scroll >= max_scroll;
    }

    fn block_at_row(&self, screen_row: u16, output_top: u16, output_h: u16) -> Option<usize> {
        if screen_row < output_top || screen_row >= output_top + output_h {
            return None;
        }
        let blocks = &self.blocks;
        let vis = self.output_vis;
        let total: usize = blocks.iter().map(|b| b.height).sum();
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

    fn scroll_hovered_tool_result(&mut self, delta: i64) -> bool {
        const MAX_RESULT_VIS: usize = 10;
        let bi = match self.hovered_block {
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
        let content_w = self.output_width.saturating_sub(4 + 1).max(1);
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

pub fn tool_call_key(block: &crate::layout::Block) -> u64 {
    let mut hasher = DefaultHasher::new();
    std::mem::discriminant(&block.kind).hash(&mut hasher);
    block.source_lines.first().hash(&mut hasher);
    block.source_lines.get(1).hash(&mut hasher);
    hasher.finish()
}
