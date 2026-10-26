//! Scrollable chat history for the fullscreen TUI. Entries keep their
//! un-wrapped source so every width change re-wraps from scratch; the scroll
//! position is `offset` = lines hidden BELOW the pane (0 = at the bottom).

use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use unicode_width::UnicodeWidthChar;

use crate::tui::paint::{reply_text, user_echo_text};

/// What was pushed, kept in a form that can be re-rendered at any width.
pub enum Source {
    /// Word-wrapped on width change — notices, warnings, errors.
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
    cache: Option<(u16, Vec<Line<'static>>, Vec<bool>)>,
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

/// Word-aware wrap of one line at `w` display columns, keeping span styles.
/// Returns the rows and, per row, whether it continues the previous one (soft).
fn wrap_line(line: &Line<'static>, w: usize) -> (Vec<Line<'static>>, Vec<bool>) {
    let w = w.max(1);
    let width = |r: &[(char, Style)]| r.iter().map(|(c, _)| c.width().unwrap_or(0)).sum::<usize>();
    let mut rows: Vec<Vec<(char, Style)>> = Vec::new();
    let mut cur: Vec<(char, Style)> = Vec::new();
    let mut used = 0usize;
    let cells = line
        .spans
        .iter()
        .flat_map(|s| s.content.chars().map(move |c| (c, s.style)));
    for (ch, st) in cells {
        let cw = ch.width().unwrap_or(0);
        if used + cw > w && used > 0 {
            if ch == ' ' {
                cur.push((ch, st));
                rows.push(std::mem::take(&mut cur));
                used = 0;
                continue;
            }
            match cur.iter().rposition(|(c, _)| *c == ' ') {
                Some(i) if i + 1 < cur.len() => {
                    let rest = cur.split_off(i + 1);
                    rows.push(std::mem::replace(&mut cur, rest));
                    used = width(&cur);
                }
                _ => {
                    rows.push(std::mem::take(&mut cur));
                    used = 0;
                }
            }
        }
        cur.push((ch, st));
        used += cw;
    }
    rows.push(cur);
    let soft = (0..rows.len()).map(|i| i > 0).collect();
    let lines = rows.into_iter().map(|r| to_line(r, line.style)).collect();
    (lines, soft)
}

/// Regroup (char, style) cells into spans of equal style.
fn to_line(cells: Vec<(char, Style)>, style: Style) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    for (c, st) in cells {
        match spans.last_mut() {
            Some(s) if s.style == st => s.content.to_mut().push(c),
            _ => spans.push(Span::styled(c.to_string(), st)),
        }
    }
    Line::from(spans).style(style)
}

/// First visual row of `wrap_line` — a line cut at the pane edge.
fn clip_line(line: &Line<'static>, w: usize) -> Line<'static> {
    wrap_line(line, w).0.swap_remove(0)
}

fn wrap_text(t: &Text<'static>, w: u16) -> (Vec<Line<'static>>, Vec<bool>) {
    let (mut lines, mut soft) = (Vec::new(), Vec::new());
    for l in &t.lines {
        let (rows, flags) = wrap_line(l, w as usize);
        lines.extend(rows);
        soft.extend(flags);
    }
    (lines, soft)
}

fn render(src: &Source, w: u16) -> (Vec<Line<'static>>, Vec<bool>) {
    match src {
        Source::Text(t) => wrap_text(t, w),
        Source::Fixed(t) => {
            let lines: Vec<_> = t.lines.iter().map(|l| clip_line(l, w as usize)).collect();
            let soft = vec![false; lines.len()];
            (lines, soft)
        }
        Source::Reply(s) => wrap_text(&reply_text(s, w), w),
        Source::Echo(s) => wrap_text(&user_echo_text(s, w), w),
    }
}

impl ChatBuffer {
    fn flat(&mut self, width: u16) -> &[Line<'static>] {
        if self.cache.as_ref().map(|c| c.0) != Some(width) {
            let (mut lines, mut soft) = (Vec::new(), Vec::new());
            for (l, f) in self.entries.iter().map(|e| render(e, width)) {
                lines.extend(l);
                soft.extend(f);
            }
            self.cache = Some((width, lines, soft));
        }
        &self.cache.as_ref().expect("cache just filled").1
    }

    /// Total wrapped lines at `width`.
    #[cfg(test)]
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

    #[cfg(test)]
    pub fn is_following(&self) -> bool {
        self.follow
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

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

    fn rows(l: &Line<'static>, w: usize) -> (Vec<String>, Vec<bool>) {
        let (lines, soft) = wrap_line(l, w);
        (plain(&lines), soft)
    }

    #[test]
    fn wrap_line_char_splits_a_word_wider_than_the_row() {
        let red = Style::default().fg(Color::Red);
        let (lines, soft) = wrap_line(&Line::from(Span::styled("abcdef", red)), 3);
        assert_eq!(plain(&lines), vec!["abc", "def"]);
        assert_eq!(soft, vec![false, true]);
        assert!(lines.iter().all(|r| r.spans.iter().all(|s| s.style == red)));
    }

    #[test]
    fn wrap_line_breaks_after_the_last_space() {
        let (r, soft) = rows(&Line::raw("the quick brown fox"), 10);
        assert_eq!(r, vec!["the quick ", "brown fox"]);
        assert_eq!(soft, vec![false, true]);
    }

    #[test]
    fn wrap_line_hangs_an_overflowing_space_on_the_upper_row() {
        let (r, _) = rows(&Line::raw("abc def"), 3);
        assert_eq!(r, vec!["abc ", "def"]);
    }

    #[test]
    fn wrap_line_keeps_wide_chars_whole_and_styles_across_breaks() {
        assert_eq!(rows(&Line::raw("日本語"), 4).0, vec!["日本", "語"]);
        let b = Style::default().fg(Color::Blue);
        let l = Line::from(vec![Span::raw("aa "), Span::styled("bbbb", b)]);
        let (lines, _) = wrap_line(&l, 5);
        assert_eq!(plain(&lines), vec!["aa ", "bbbb"]);
        assert_eq!(lines[1].spans[0].style, b);
        assert_eq!(wrap_line(&Line::raw(""), 10).0.len(), 1);
    }

    #[test]
    fn multi_line_text_marks_only_continuations_soft() {
        let mut c = ChatBuffer::default();
        c.push(Source::Text(Text::raw("one two three\nfour")));
        c.visible(8, 10);
        let (_, _, soft) = c.cache.as_ref().unwrap();
        assert_eq!(soft, &vec![false, true, false]);
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
