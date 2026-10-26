//! Scrollable chat history for the fullscreen TUI. Entries keep their
//! un-wrapped source so every width change re-wraps from scratch; the scroll
//! position is `offset` = lines hidden BELOW the pane (0 = at the bottom).

use ratatui::text::{Line, Span, Text};
use unicode_width::UnicodeWidthChar;

use crate::tui::paint::{reply_text, user_echo_text};

/// What was pushed, kept in a form that can be re-rendered at any width.
pub enum Source {
    /// Soft-wrapped (char-split) on width change — notices, warnings, errors.
    Text(Text<'static>),
    /// Clipped, never re-wrapped — welcome panels (box borders break if wrapped).
    Fixed(Text<'static>),
    /// Assistant markdown, re-rendered for the width.
    Reply(String),
    /// The user's submitted line, re-wrapped for the width.
    Echo(String),
}

pub struct ChatBuffer {
    entries: Vec<Source>,
    /// Lines hidden below the pane. 0 = showing the newest line.
    offset: usize,
    follow: bool,
    unread: bool,
    /// Width / pane height seen by the last `visible()` call (used by scroll math).
    width: u16,
    pane_h: u16,
    cache: Option<(u16, Vec<Line<'static>>)>,
}

impl Default for ChatBuffer {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            offset: 0,
            follow: true,
            unread: false,
            width: 80,
            pane_h: 1,
            cache: None,
        }
    }
}

/// Greedy char-split of one line at `w` display columns, keeping span styles.
fn wrap_line(line: &Line<'static>, w: usize) -> Vec<Line<'static>> {
    let w = w.max(1);
    let mut out: Vec<Line<'static>> = Vec::new();
    let mut cur: Vec<Span<'static>> = Vec::new();
    let mut used = 0usize;
    for span in &line.spans {
        let mut buf = String::new();
        for ch in span.content.chars() {
            let cw = ch.width().unwrap_or(0);
            if used + cw > w && used > 0 {
                if !buf.is_empty() {
                    cur.push(Span::styled(std::mem::take(&mut buf), span.style));
                }
                out.push(Line::from(std::mem::take(&mut cur)).style(line.style));
                used = 0;
            }
            buf.push(ch);
            used += cw;
        }
        if !buf.is_empty() {
            cur.push(Span::styled(buf, span.style));
        }
    }
    out.push(Line::from(cur).style(line.style));
    out
}

/// First visual row of `wrap_line` — a line cut at the pane edge.
fn clip_line(line: &Line<'static>, w: usize) -> Line<'static> {
    wrap_line(line, w).swap_remove(0)
}

fn wrap_text(t: &Text<'static>, w: u16) -> Vec<Line<'static>> {
    t.lines
        .iter()
        .flat_map(|l| wrap_line(l, w as usize))
        .collect()
}

fn render(src: &Source, w: u16) -> Vec<Line<'static>> {
    match src {
        Source::Text(t) => wrap_text(t, w),
        Source::Fixed(t) => t.lines.iter().map(|l| clip_line(l, w as usize)).collect(),
        Source::Reply(s) => wrap_text(&reply_text(s, w), w),
        Source::Echo(s) => wrap_text(&user_echo_text(s, w), w),
    }
}

impl ChatBuffer {
    fn flat(&mut self, width: u16) -> &[Line<'static>] {
        if self.cache.as_ref().map(|c| c.0) != Some(width) {
            let lines = self.entries.iter().flat_map(|e| render(e, width)).collect();
            self.cache = Some((width, lines));
        }
        &self.cache.as_ref().expect("cache just filled").1
    }

    /// Total wrapped lines at `width`.
    pub fn line_count(&mut self, width: u16) -> usize {
        self.flat(width).len()
    }

    pub fn push(&mut self, s: Source) {
        if self.follow {
            self.entries.push(s);
            self.cache = None;
            return;
        }
        // Scrolled up: keep the user's view still and flag the new content.
        let w = self.width;
        let before = self.flat(w).len();
        self.entries.push(s);
        self.cache = None;
        let after = self.flat(w).len();
        self.offset += after.saturating_sub(before);
        self.unread = true;
    }

    /// The lines to paint into a `width` x `height` pane (top-anchored when short).
    /// Also records the pane size for the scroll helpers and clamps `offset`.
    pub fn visible(&mut self, width: u16, height: u16) -> Vec<Line<'static>> {
        if width == 0 || height == 0 {
            return Vec::new();
        }
        self.width = width;
        self.pane_h = height;
        let h = height as usize;
        let total = self.flat(width).len();
        self.offset = self.offset.min(total.saturating_sub(h));
        if self.offset == 0 {
            self.follow = true;
            self.unread = false;
        }
        let end = total - self.offset;
        let start = end.saturating_sub(h);
        self.flat(width)[start..end].to_vec()
    }

    pub fn scroll_up(&mut self, n: usize) {
        let w = self.width;
        let max = self.flat(w).len().saturating_sub(self.pane_h as usize);
        if max == 0 {
            return;
        }
        self.offset = (self.offset + n).min(max);
        self.follow = false;
    }

    pub fn scroll_down(&mut self, n: usize) {
        self.offset = self.offset.saturating_sub(n);
        if self.offset == 0 {
            self.follow = true;
            self.unread = false;
        }
    }

    fn page_step(&self) -> usize {
        (self.pane_h as usize).saturating_sub(1).max(1)
    }

    pub fn page_up(&mut self) {
        self.scroll_up(self.page_step());
    }

    pub fn page_down(&mut self) {
        self.scroll_down(self.page_step());
    }

    pub fn jump_bottom(&mut self) {
        self.offset = 0;
        self.follow = true;
        self.unread = false;
    }

    pub fn unread(&self) -> bool {
        self.unread
    }

    pub fn is_following(&self) -> bool {
        self.follow
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::{Color, Style};

    fn plain(lines: &[Line]) -> Vec<String> {
        lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    /// `n` single-line entries "l0".."l{n-1}".
    fn filled(n: usize) -> ChatBuffer {
        let mut c = ChatBuffer::default();
        for i in 0..n {
            c.push(Source::Text(Text::raw(format!("l{i}"))));
        }
        c
    }

    #[test]
    fn wrap_line_splits_by_display_width_and_keeps_style() {
        let red = Style::default().fg(Color::Red);
        let l = Line::from(Span::styled("abcdef", red));
        let w = wrap_line(&l, 3);
        assert_eq!(plain(&w), vec!["abc", "def"]);
        assert!(w.iter().all(|r| r.spans[0].style == red));
        // wide (2-column) chars are never split in half
        let w = wrap_line(&Line::raw("日本語"), 4);
        assert_eq!(plain(&w), vec!["日本", "語"]);
        // empty line stays one empty row
        assert_eq!(wrap_line(&Line::raw(""), 10).len(), 1);
    }

    #[test]
    fn clip_line_truncates_to_width() {
        assert_eq!(plain(&[clip_line(&Line::raw("abcdef"), 4)]), vec!["abcd"]);
    }

    #[test]
    fn short_content_is_top_anchored() {
        let mut c = filled(3);
        assert_eq!(plain(&c.visible(80, 10)), vec!["l0", "l1", "l2"]);
    }

    #[test]
    fn follows_the_bottom_by_default() {
        let mut c = filled(20);
        assert_eq!(
            plain(&c.visible(80, 5)),
            vec!["l15", "l16", "l17", "l18", "l19"]
        );
        assert!(c.is_following());
        c.push(Source::Text(Text::raw("new")));
        assert_eq!(plain(&c.visible(80, 5)).last().unwrap(), "new");
        assert!(!c.unread());
    }

    #[test]
    fn scroll_up_moves_the_window_and_clamps_at_the_top() {
        let mut c = filled(20);
        c.visible(80, 5);
        c.scroll_up(3);
        assert_eq!(
            plain(&c.visible(80, 5)),
            vec!["l12", "l13", "l14", "l15", "l16"]
        );
        assert!(!c.is_following());
        c.scroll_up(1000);
        assert_eq!(plain(&c.visible(80, 5)), vec!["l0", "l1", "l2", "l3", "l4"]);
    }

    #[test]
    fn scroll_up_is_a_noop_when_everything_fits() {
        let mut c = filled(3);
        c.visible(80, 10);
        c.scroll_up(3);
        assert!(c.is_following());
    }

    #[test]
    fn scrolling_back_to_the_bottom_refollows_and_clears_unread() {
        let mut c = filled(20);
        c.visible(80, 5);
        c.scroll_up(3);
        c.push(Source::Text(Text::raw("new")));
        assert!(c.unread());
        c.scroll_down(2);
        assert!(!c.is_following() && c.unread());
        c.scroll_down(100);
        assert!(c.is_following() && !c.unread());
        assert_eq!(plain(&c.visible(80, 5)).last().unwrap(), "new");
    }

    #[test]
    fn new_content_while_scrolled_keeps_the_view_stable() {
        let mut c = filled(20);
        c.visible(80, 5);
        c.scroll_up(3);
        let before = plain(&c.visible(80, 5));
        c.push(Source::Text(Text::raw("a\nb")));
        assert_eq!(plain(&c.visible(80, 5)), before);
        assert!(c.unread());
    }

    #[test]
    fn page_up_and_down_move_pane_height_minus_one() {
        let mut c = filled(40);
        c.visible(80, 10);
        c.page_up();
        assert_eq!(c.offset, 9);
        assert_eq!(plain(&c.visible(80, 10))[0], "l21");
        c.page_down();
        assert_eq!(c.offset, 0);
        assert!(c.is_following());
    }

    #[test]
    fn jump_bottom_resets_everything() {
        let mut c = filled(20);
        c.visible(80, 5);
        c.scroll_up(3);
        c.push(Source::Text(Text::raw("new")));
        c.jump_bottom();
        assert!(c.is_following() && !c.unread() && c.offset == 0);
    }

    #[test]
    fn echo_and_reply_rewrap_when_the_width_changes() {
        let mut c = ChatBuffer::default();
        c.push(Source::Echo("x ".repeat(30)));
        let wide = c.line_count(80);
        let narrow = c.line_count(20);
        assert!(narrow > wide, "narrow {narrow} should exceed wide {wide}");
        assert_eq!(c.line_count(80), wide, "width round-trip is stable");
    }

    #[test]
    fn offset_is_clamped_after_the_pane_grows() {
        let mut c = filled(20);
        c.visible(80, 5);
        c.scroll_up(8);
        assert_eq!(c.offset, 8);
        c.visible(80, 18); // taller pane: max offset is now 2
        assert_eq!(c.offset, 2);
        c.visible(80, 25); // everything fits
        assert_eq!(c.offset, 0);
        assert!(c.is_following());
    }

    #[test]
    fn fixed_entries_are_clipped_not_wrapped() {
        let mut c = ChatBuffer::default();
        c.push(Source::Fixed(Text::raw("0123456789")));
        assert_eq!(plain(&c.visible(4, 5)), vec!["0123"]);
        c.push(Source::Text(Text::raw("0123456789")));
        assert_eq!(c.line_count(4), 1 + 3);
    }

    #[test]
    fn zero_sized_pane_returns_nothing() {
        let mut c = filled(3);
        assert!(c.visible(0, 5).is_empty());
        assert!(c.visible(80, 0).is_empty());
    }
}
