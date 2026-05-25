#[cfg(test)]
#[cfg(test)]
mod layout_math_tests {
    /// Verify that visible lines never exceed the area height.
    #[test]
    fn test_vis_clamp() {
        for area_h in 0..=100u16 {
            for total_lines in 0..=500usize {
                for scroll in [true, false] {
                    let vis = (area_h as usize).saturating_sub(2).max(1);
                    let start = if scroll {
                        total_lines.saturating_sub(vis)
                    } else {
                        0usize
                    };
                    let end = (start + vis).min(total_lines);
                    let displayed = end.saturating_sub(start);
                    // Displayed lines must never exceed vis
                    assert!(displayed <= vis,
                        "overflow: area_h={area_h}, total={total_lines}, vis={vis}, start={start}, end={end}, displayed={displayed}");
                }
            }
        }
    }

    /// Scroll position must stay within valid range.
    #[test]
    fn test_scroll_bounds() {
        for total in 0..=200usize {
            for vis in 1..=50usize {
                let max_scroll = total.saturating_sub(vis);
                // scroll must be in [0, max_scroll]
                for scroll in [0.0f32, max_scroll as f32 / 2.0, max_scroll as f32] {
                    let clamped = scroll.clamp(0.0, max_scroll.max(0) as f32);
                    assert!(clamped >= 0.0);
                    assert!(clamped <= max_scroll.max(0) as f32);
                }
            }
        }
    }

    /// Text area must be at least 1 column narrower than full area (scrollbar).
    #[test]
    fn test_text_area_width() {
        for full_w in 0..=120u16 {
            let text_w = full_w.saturating_sub(1);
            assert!(text_w < full_w || full_w == 0);
        }
    }

    /// Safety margin: bottom_h + 2 must not exceed terminal height.
    #[test]
    fn test_safety_margin() {
        for term_h in 0..=60u16 {
            for input_lines in 1..=5 {
                for hint_count in 0..=8 {
                    let input_h = (input_lines + 2) as u16;
                    let bottom_h = (input_h as usize + hint_count + 1).min(term_h.saturating_sub(2) as usize) as u16;
                    let out_h = term_h.saturating_sub(bottom_h + 2);
                    // output height must not exceed terminal - bottom
                    assert!(out_h + bottom_h + 2 <= term_h || term_h < 3,
                        "overflow: term={term_h} bottom={bottom_h} out={out_h}");
                }
            }
        }
    }

    /// Filled lines must equal vis after resize.
    #[test]
    fn test_filled_resize_truncates() {
        for vis in 1..=50 {
            for filled_len in 0..=100 {
                let mut filled = vec![1; filled_len];
                filled.resize(vis, 0);
                assert_eq!(filled.len(), vis,
                    "resize({vis}) from {filled_len} gave len {}", filled.len());
            }
        }
    }
}
