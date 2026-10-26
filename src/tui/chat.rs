//! Scrollable chat history for the fullscreen TUI. Entries keep their
//! un-wrapped source so every width change re-wraps from scratch; the scroll
//! position is `offset` = lines hidden BELOW the pane (0 = at the bottom).

use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use unicode_width::UnicodeWidthChar;

use crate::image_attach::PastedImage;
use crate::tui::paint::{error_line, notice_line, reply_text, user_echo_text, warn_line};
use crate::tui::theme;

/// What was pushed, kept in a form that can be re-rendered at any width.
pub enum Source {
    /// Word-wrapped on width change — notices, warnings, errors.
    Text(Text<'static>),
    /// Pre-sized panel (welcome box), rebuilt at the current width on every render and
    /// clipped (never wrapped) when the pane is narrower than the panel's minimum.
    Panel(Box<dyn Fn(u16) -> Text<'static>>),
    /// Assistant markdown, re-rendered for the width.
    Reply(String),
    /// The user's submitted line, re-wrapped for the width.
    Echo(String),
}

/// Bound on kept entries; past it the oldest `TRIM_ENTRIES` are dropped.
pub const MAX_ENTRIES: usize = 1000;
const TRIM_ENTRIES: usize = 100;

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
    /// Count of per-entry `render` calls (tests assert on incremental work).
    #[cfg(test)]
    renders: usize,
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
            #[cfg(test)]
            renders: 0,
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
                    if used + cw > w && used > 0 {
                        rows.push(std::mem::take(&mut cur));
                        used = 0;
                    }
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

/// A line cut at the pane edge: as many whole cells as fit in `w` columns.
fn clip_line(line: &Line<'static>, w: usize) -> Line<'static> {
    let w = w.max(1);
    let mut used = 0usize;
    let mut cells: Vec<(char, Style)> = Vec::new();
    for (c, st) in line
        .spans
        .iter()
        .flat_map(|s| s.content.chars().map(move |c| (c, s.style)))
    {
        let cw = c.width().unwrap_or(0);
        if used + cw > w {
            break;
        }
        used += cw;
        cells.push((c, st));
    }
    to_line(cells, line.style)
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
        Source::Panel(f) => {
            let lines: Vec<_> = f(w)
                .lines
                .iter()
                .map(|l| clip_line(l, w as usize))
                .collect();
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
            #[cfg(test)]
            {
                self.renders += self.entries.len();
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
        if !self.follow {
            let w = self.width;
            self.flat(w); // cache at the current width, so the new rows can be counted
        }
        let mut added = 0;
        if let Some((w, lines, soft)) = self.cache.as_mut() {
            let (l, f) = render(&s, *w);
            added = l.len();
            lines.extend(l);
            soft.extend(f);
            #[cfg(test)]
            {
                self.renders += 1;
            }
        }
        self.entries.push(s);
        if !self.follow {
            // Scrolled up: keep the user's view still and flag the new content.
            self.offset += added;
            self.unread = true;
        }
        self.trim();
    }

    /// Bound memory: past `MAX_ENTRIES`, drop the oldest `TRIM_ENTRIES` (amortized).
    /// `offset` counts from the bottom, so the view does not jump.
    fn trim(&mut self) {
        if self.entries.len() > MAX_ENTRIES {
            self.entries.drain(..TRIM_ENTRIES);
            self.cache = None;
        }
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

    /// Persistent content, soft-wrapped on resize.
    pub fn text(&mut self, t: Text<'static>) {
        self.push(Source::Text(t));
    }

    pub fn notice(&mut self, msg: &str) {
        self.text(notice_line(msg));
    }

    pub fn warn(&mut self, msg: &str) {
        self.text(warn_line(msg));
    }

    pub fn error(&mut self, msg: &str) {
        self.text(error_line(msg));
    }

    /// The user's submitted line (wrapped at render time); jumps to the bottom.
    pub fn user_echo(&mut self, line: &str) {
        self.jump_bottom();
        self.push(Source::Echo(line.to_string()));
    }

    /// One `image N (W×H)` line per attached image, right under the user's echo.
    pub fn image_echo(&mut self, images: &[PastedImage]) {
        for (i, img) in images.iter().enumerate() {
            self.notice(&img.echo_label(i + 1));
        }
    }

    /// Usta's reply; the markdown is rendered for the CURRENT width at draw time.
    pub fn reply(&mut self, md: &str) {
        self.push(Source::Reply(md.to_string()));
    }

    /// A pre-sized panel: `f` builds it for the current width on every render.
    pub fn panel(&mut self, f: impl Fn(u16) -> Text<'static> + 'static) {
        self.push(Source::Panel(Box::new(f)));
    }

    /// Route buffered ui notices: a leading `⚠ ` renders as the amber warning
    /// layer; everything else is a dim info line.
    pub fn flush_notices(&mut self, msgs: Vec<String>) {
        for m in msgs {
            match m.strip_prefix(&format!("{} ", theme::G_WARN)) {
                Some(rest) => self.warn(rest),
                None => self.notice(&m),
            }
        }
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
    fn wrap_line_counts_the_carried_rest_after_a_split() {
        let (r, soft) = rows(&Line::raw("aa bb cc dd"), 5);
        assert_eq!(r, vec!["aa bb ", "cc dd"]);
        assert_eq!(soft, vec![false, true]);
    }

    #[test]
    fn wrap_line_never_overflows_when_a_wide_char_follows_a_carry() {
        let (r, _) = rows(&Line::raw(" abc日"), 4);
        assert_eq!(r, vec![" ", "abc", "日"]);
        assert_eq!(r.concat(), " abc日");
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
    fn clip_line_cuts_mid_word_and_by_display_width() {
        assert_eq!(
            plain(&[clip_line(&Line::raw("│ ab cd │"), 6)]),
            vec!["│ ab c"]
        );
        assert_eq!(plain(&[clip_line(&Line::raw("日本語"), 3)]), vec!["日"]);
    }

    #[test]
    fn incremental_cache_equals_a_full_rebuild() {
        let mut c = ChatBuffer::default();
        c.visible(12, 5); // warm the cache
        c.push(Source::Text(Text::raw(
            "one two three four
five six seven",
        )));
        c.push(Source::Echo("hello there my friend, how are you".into()));
        c.push(Source::Reply(
            "some **bold** reply text that is long".into(),
        ));
        c.push(Source::Panel(Box::new(|w| {
            Text::raw(format!("│ panel {w} │"))
        })));
        c.push(Source::Text(Text::raw("last notice wraps too")));
        let (_, lines, soft) = c.cache.clone().expect("cache warm");
        let snap = (plain(&lines), soft);
        c.cache = None;
        c.visible(12, 5);
        let (_, lines2, soft2) = c.cache.clone().expect("cache rebuilt");
        assert_eq!(snap, (plain(&lines2), soft2.clone()));
        assert_eq!(soft2.len(), lines2.len());
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
    fn panels_rerender_at_the_current_width_and_clip_below_it() {
        let mut c = ChatBuffer::default();
        c.push(Source::Panel(Box::new(|w| {
            Text::raw(format!("{}|", "-".repeat(w as usize - 1)))
        })));
        assert_eq!(
            plain(&c.visible(100, 5)),
            vec![format!("{}|", "-".repeat(99))]
        );
        let rows = plain(&c.visible(60, 5));
        assert_eq!(rows, vec![format!("{}|", "-".repeat(59))]);
        assert!(
            rows[0].ends_with('|'),
            "border on the last column at the new width"
        );
        // a panel wider than the pane is clipped, never wrapped
        c.push(Source::Panel(Box::new(|_| Text::raw("0123456789"))));
        assert_eq!(plain(&c.visible(4, 5)).last().unwrap(), "0123");
    }

    #[test]
    fn zero_sized_pane_returns_nothing() {
        let mut c = filled(3);
        assert!(c.visible(0, 5).is_empty());
        assert!(c.visible(80, 0).is_empty());
    }

    #[test]
    fn push_while_scrolled_renders_only_the_new_entry() {
        let mut c = filled(20);
        c.visible(80, 5);
        c.scroll_up(3);
        let before = c.renders;
        let offset = c.offset;
        c.push(Source::Text(Text::raw("a\nb")));
        assert_eq!(c.renders - before, 1);
        assert_eq!(c.offset, offset + 2);
    }

    #[test]
    fn push_while_following_extends_the_cache() {
        let mut c = filled(5);
        c.visible(80, 5);
        let before = c.renders;
        c.push(Source::Text(Text::raw("x")));
        c.visible(80, 5);
        assert_eq!(c.renders - before, 1, "no full re-render after a push");
    }

    #[test]
    fn entries_are_capped_and_the_view_survives() {
        let mut c = filled(1100);
        assert!(c.entries.len() <= MAX_ENTRIES);
        assert_eq!(plain(&c.visible(80, 3)).last().unwrap(), "l1099");
        c.scroll_up(10);
        let before = plain(&c.visible(80, 3));
        for i in 0..150 {
            c.push(Source::Text(Text::raw(format!("n{i}"))));
        }
        assert!(c.entries.len() <= MAX_ENTRIES);
        assert_eq!(
            plain(&c.visible(80, 3)),
            before,
            "scrolled view keeps its text"
        );
    }

    fn styles(c: &mut ChatBuffer) -> Vec<Vec<Style>> {
        c.visible(80, 50)
            .iter()
            .map(|l| l.spans.iter().map(|s| s.style).collect())
            .collect()
    }

    #[test]
    fn user_echo_jumps_to_the_bottom() {
        let mut c = filled(20);
        c.visible(80, 5);
        c.scroll_up(5);
        c.user_echo("hello");
        assert!(c.is_following());
        assert!(plain(&c.visible(80, 5)).iter().any(|r| r.contains("hello")));
    }

    #[test]
    fn image_echo_adds_one_labelled_line_per_image() {
        let img = |w, h| crate::image_attach::PastedImage {
            media_type: "image/png",
            data: vec![],
            width: w,
            height: h,
        };
        let mut c = ChatBuffer::default();
        c.image_echo(&[img(4, 3), img(10, 20)]);
        let rows = plain(&c.visible(80, 10));
        assert_eq!(rows.len(), 2);
        assert!(rows[0].contains("image 1 (4×3)") && rows[1].contains("image 2 (10×20)"));
    }

    #[test]
    fn flush_routes_warn_prefixed_messages_to_the_warn_layer() {
        let mut c = ChatBuffer::default();
        c.flush_notices(vec![
            format!("{} careful", crate::tui::theme::G_WARN),
            "fyi".into(),
        ]);
        let mut w = ChatBuffer::default();
        w.warn("careful");
        let mut n = ChatBuffer::default();
        n.notice("fyi");
        let s = styles(&mut c);
        assert_eq!(s[0], styles(&mut w)[0]);
        assert_eq!(s[1], styles(&mut n)[0]);
    }

    #[test]
    fn error_line_uses_the_error_style() {
        let mut c = ChatBuffer::default();
        c.error("boom");
        let expect: Vec<Style> = crate::tui::paint::error_line("boom").lines[0]
            .spans
            .iter()
            .map(|s| s.style)
            .collect();
        assert_eq!(styles(&mut c)[0], expect);
    }
}
