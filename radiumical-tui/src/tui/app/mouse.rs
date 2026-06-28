use crate::tui::app::App;
use crossterm::event::{MouseButton, MouseEventKind};
use ratatui::layout::Rect;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

const DOUBLE_CLICK_MS: u64 = 300;

impl App {
    pub fn handle_mouse(&mut self, kind: MouseEventKind, row: u16, _col: u16, output_top: u16) {
        if self.welcome {
            return;
        }
        match kind {
            MouseEventKind::ScrollDown => self.scroll_down(1.0),
            MouseEventKind::ScrollUp => self.scroll_up(1.0),
            MouseEventKind::Moved => {
                self.hovered_block = self.block_at_row(row, output_top);
            }
            MouseEventKind::Down(btn) => {
                if btn == MouseButton::Right {
                    return;
                }
                let on_scrollbar = _col + 1 >= self.output_vis as u16;
                let total = self.output.len();
                let needs_scrollbar = total > self.output_vis;
                if on_scrollbar && needs_scrollbar && row > output_top {
                    self.scrollbar_dragging = true;
                    self.set_scroll_from_thumb(row, output_top);
                    return;
                }
                if self.help_board.hit_border(
                    row,
                    _col,
                    Rect {
                        x: 0,
                        y: output_top,
                        width: 80,
                        height: 24,
                    },
                ) {
                    self.help_board.start_drag(_col, row);
                    return;
                }
                if let Some(bi) = self.block_at_row(row, output_top) {
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
                self.set_scroll_from_thumb(row, output_top);
            }
            MouseEventKind::Up(_) => {
                self.scrollbar_dragging = false;
            }
            _ => {}
        }
    }

    fn set_scroll_from_thumb(&mut self, row: u16, output_top: u16) {
        let total = self.rendered_total;
        let vis = self.output_vis.max(1);
        if total <= vis {
            self.scroll = 0.0;
            self.stick_to_bottom = true;
            return;
        }
        let sb_h = (self.output_vis as u16).saturating_sub(1) as f32;
        if sb_h <= 0.0 {
            return;
        }
        let rel = (row.saturating_sub(output_top).saturating_sub(1)) as f32;
        let progress = (rel / sb_h).clamp(0.0, 1.0);
        let max_scroll = (total - vis) as f32;
        self.scroll = (progress * max_scroll).round();
        self.stick_to_bottom = self.scroll <= 0.0;
    }

    fn block_at_row(&self, screen_row: u16, output_top: u16) -> Option<usize> {
        if screen_row < output_top {
            return None;
        }
        let blocks = &self.blocks;
        let vis = self.output_vis;
        let total: usize = blocks.iter().map(|b| b.height).sum();
        let start = if self.stick_to_bottom {
            total.saturating_sub(vis)
        } else {
            (self.scroll as usize).min(total.saturating_sub(vis))
        };
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
