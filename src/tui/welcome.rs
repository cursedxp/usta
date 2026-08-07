//! Açılış kutusu: veri toplama (saf) + render. Spec §5.
//! Tüm parse'lar best-effort — bozuk/eksik girdi alanı atlar, asla panik yok.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use unicode_width::UnicodeWidthStr;

const ORANGE: Color = Color::Indexed(208);

const LOGO: [&str; 4] = [
    "██  ██ ██████ ██████ ██████",
    "██  ██ ██       ██   ██  ██",
    "██  ██ ██████   ██   ██████",
    "██████     ██   ██   ██  ██",
];

/// Açılış kutusunun tüm verisi — render bu struct'tan çizer, IO yapmaz.
pub struct WelcomeData {
    pub version: &'static str,
    pub name: Option<String>,
    pub model: String,
    pub dir: String,
    pub topic: String,
    pub level: Option<String>,
    pub map_percent: Option<u8>,
    pub next_item: Option<String>,
    pub drill_count: usize,
    pub first_session: bool,
}

/// `## {header}` başlığından bir sonraki `## `e kadarki gövde.
fn section<'a>(md: &'a str, header: &str) -> Option<&'a str> {
    let needle = format!("## {header}");
    let start = md.find(&needle)? + needle.len();
    let rest = &md[start..];
    let end = rest.find("\n## ").unwrap_or(rest.len());
    Some(&rest[..end])
}

/// `# Öğrenci Profili — Ada` → `Ada` (em-dash veya tire sonrası).
pub fn extract_name(profile: &str) -> Option<String> {
    let h1 = profile.lines().find(|l| l.starts_with("# "))?;
    let name = h1.rsplit(['—', '-']).next()?.trim();
    if name.is_empty() || name.contains("Profil") || name.starts_with('#') { return None; }
    Some(name.to_string())
}

/// `## Seviye` bölümünün ilk dolu satırı, liste işareti soyulmuş.
pub fn extract_level(progress: &str) -> Option<String> {
    section(progress, "Seviye")?
        .lines()
        .map(|l| l.trim().trim_start_matches(['-', '*', ' ']).trim())
        .find(|l| !l.is_empty())
        .map(String::from)
}

const STATUSES: [&str; 4] = ["görülmedi", "görüldü", "oturdu", "derinleşildi"];

/// Durum içeren satır sayımından harita yüzdesi: görülmedi-olmayan / toplam.
pub fn curriculum_percent(curriculum: &str) -> Option<u8> {
    let (mut total, mut seen) = (0u32, 0u32);
    for line in curriculum.lines() {
        if line.contains("görülmedi") { total += 1; }
        else if STATUSES[1..].iter().any(|s| line.contains(s)) { total += 1; seen += 1; }
    }
    if total == 0 { return None; }
    Some(((seen * 100) / total) as u8)
}

/// İlk `görülmedi` maddesinin metni — liste işareti ve durum eki soyulur.
pub fn next_unseen(curriculum: &str) -> Option<String> {
    let line = curriculum.lines().find(|l| l.contains("görülmedi"))?;
    let text = line.split("görülmedi").next()?
        .trim()
        .trim_start_matches(['-', '*', ' '])
        .trim_end_matches([':', '—', '-', '·', '|', ' ']);
    if text.is_empty() { None } else { Some(text.to_string()) }
}

/// `## Geri çağırma soruları` bölümündeki madde sayısı.
pub fn drill_count(progress: &str) -> usize {
    section(progress, "Geri çağırma soruları")
        .map(|s| s.lines().filter(|l| l.trim().starts_with('-')).count())
        .unwrap_or(0)
}

/// Dosya içeriklerinden WelcomeData kur — hepsi Option, eksik = alan atlanır.
pub fn gather(
    profile: Option<&str>, progress: Option<&str>, curriculum: Option<&str>,
    topic: &str, model: &str, dir: &str,
) -> WelcomeData {
    WelcomeData {
        version: env!("CARGO_PKG_VERSION"),
        name: profile.and_then(extract_name),
        model: model.to_string(),
        dir: dir.to_string(),
        topic: topic.to_string(),
        level: progress.and_then(extract_level),
        map_percent: curriculum.and_then(curriculum_percent),
        next_item: curriculum.and_then(next_unseen),
        drill_count: progress.map(drill_count).unwrap_or(0),
        first_session: progress.is_none(),
    }
}

/// Görünür genişliğe göre kırp, taşarsa `…` ekle. Padding hesapları da
/// unicode-width ile — Türkçe karakterlerde byte sayımı yanlış hizalar.
pub fn fit(s: &str, max: usize) -> String {
    if s.width() <= max { return s.to_string(); }
    let mut out = String::new();
    let mut w = 0usize;
    for ch in s.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if w + cw > max.saturating_sub(1) { break; }
        out.push(ch);
        w += cw;
    }
    out.push('…');
    out
}

/// Görünür genişliğe tamamla — sağa boşluk ekler (unicode-width'e göre).
fn pad(s: &str, w: usize) -> String {
    format!("{s}{}", " ".repeat(w.saturating_sub(s.width())))
}

/// Çift kolonlu açılış kutusu. Genişlik `min(width, 100)`; sol kolon logo +
/// selamlama + model + dizin, sağ kolon Öğrenme Durumu (spec §5).
pub fn render_welcome(d: &WelcomeData, width: u16) -> Text<'static> {
    let total = (width as usize).clamp(60, 100);
    let inner = total - 2;                      // kenarlar
    let left_w = 34usize;
    let right_w = inner - left_w - 3;           // " │ " ayracı

    let greet = match &d.name {
        Some(n) => format!("Tekrar hoş geldin, {n}!"),
        None => "Tekrar hoş geldin!".to_string(),
    };
    let mut left: Vec<(String, bool)> = vec![(String::new(), false)];
    for l in LOGO { left.push((format!("  {l}"), true)); }
    left.push((String::new(), false));
    left.push((format!("  {}", fit(&greet, left_w - 2)), false));
    left.push((format!("  {}", fit(&d.model, left_w - 2)), false));
    left.push((format!("  {}", fit(&d.dir, left_w - 2)), false));

    let mut right: Vec<(String, Style)> = Vec::new();
    if d.first_session {
        right.push(("Öğrenme Durumu".to_string(), Style::default()));
        right.push((String::new(), Style::default()));
        right.push((fit("İlk oturum — tanışmayla başlarız.", right_w), Style::default()));
    } else {
        right.push(("Öğrenme Durumu".to_string(), Style::default()));
        let konu = match &d.level {
            Some(l) => format!("Konu: {} · {}", d.topic, l),
            None => format!("Konu: {}", d.topic),
        };
        right.push((fit(&konu, right_w), Style::default()));
        if let Some(p) = d.map_percent { right.push((format!("Harita: %{p}"), Style::default())); }
        right.push(("─".repeat(right_w), Style::default()));
        right.push(("Sırada".to_string(), Style::default()));
        if let Some(n) = &d.next_item { right.push((fit(n, right_w), Style::default())); }
        if d.drill_count > 0 { right.push((format!("Drill: {} soru hazır", d.drill_count), Style::default())); }
    }

    render_box(d.version, left, right, width)
}

/// Kimlik modu: konu YOK. Sol kolon logo + selam + model + dizin; sağ kolon
/// "Ne öğrenmek istiyorsun?" + devam edilebilecek yerel konular (veya
/// ilk-oturum mesajı). Konu seçilmeden gösterilir (Claude tarzı: welcome
/// üstte, soru altta). run.rs konu girişinde (`ask_topic`) bağlı.
///
/// `local`: bu projede kayıtlı konular — boş değilse `Enter → <ilk>'e devam`
/// satırı ve numaralı liste (≤6) gösterilir. `other`: başka projelerde
/// kayıtlı konular — sadece bilgi amaçlı, seçilemez, soluk bir satırda özetlenir.
pub fn render_welcome_identity(
    name: Option<&str>,
    model: &str,
    dir: &str,
    local: &[String],
    other: &[String],
    width: u16,
) -> Text<'static> {
    let total = (width as usize).clamp(60, 100);
    let inner = total - 2;
    let left_w = 34usize;
    let right_w = inner - left_w - 3;

    let greet = match name {
        Some(n) => format!("Merhaba, {n}!"),
        None => "Merhaba!".to_string(),
    };
    let mut left: Vec<(String, bool)> = vec![(String::new(), false)];
    for l in LOGO { left.push((format!("  {l}"), true)); }
    left.push((String::new(), false));
    left.push((format!("  {}", fit(&greet, left_w - 2)), false));
    left.push((format!("  {}", fit(model, left_w - 2)), false));
    left.push((format!("  {}", fit(dir, left_w - 2)), false));

    // Diğer projelerdeki konular sadece bilgi amaçlı — soluk (DIM) gösterilir.
    let dim = Style::default().add_modifier(Modifier::DIM);
    let mut right: Vec<(String, Style)> = vec![
        ("Ne öğrenmek istiyorsun?".to_string(), Style::default()),
        (String::new(), Style::default()),
    ];
    if let Some(first) = local.first() {
        right.push((fit(&format!("Enter → {first}'e devam"), right_w), Style::default()));
        for (i, t) in local.iter().take(6).enumerate() {
            right.push((fit(&format!("{}) {t}", i + 1), right_w), Style::default()));
        }
        right.push((String::new(), Style::default()));
        right.push((fit("Yeni konu için yaz.", right_w), Style::default()));
        if !other.is_empty() {
            right.push((fit(&format!("Diğer projelerde: {}", other.join(", ")), right_w), dim));
        }
    } else {
        // Spec §3: yerel konu yokken ilk-oturum mesajı AYNEN korunur.
        right.push((fit("İlk oturum — bir konu yaz.", right_w), Style::default()));
        // mevcut "Kayıtlı:" satırı KALKAR — yerine other bilgi satırı (varsa).
        if !other.is_empty() {
            right.push((String::new(), Style::default()));
            right.push((fit(&format!("Diğer projelerde: {}", other.join(", ")), right_w), dim));
        }
    }

    render_box(env!("CARGO_PKG_VERSION"), left, right, width)
}

/// Çift kolonlu kutuyu çiz — kenar + " │ " ayracı + eşit-genişlik padding.
/// `left`: (metin, logo-mu). `right`: (metin, stil) — 0. satır ayrıca
/// otomatik kalın+turuncu başlık stiline büründürülür (satırın kendi stili
/// boşsa bile), diğer satırlar taşıdıkları stille (ör. DIM) basılır.
fn render_box(version: &str, left: Vec<(String, bool)>, right: Vec<(String, Style)>, width: u16) -> Text<'static> {
    let total = (width as usize).clamp(60, 100);
    let inner = total - 2;                      // kenarlar
    let left_w = 34usize;
    let right_w = inner - left_w - 3;           // " │ " ayracı

    let rows = left.len().max(right.len());
    let title = format!(" Usta v{version} ");
    // NOT: dashes = inner - (4 + title_genişliği) olmalı — "╭─── " öneki 5 char,
    // kapanış "╮" 1 char, toplam sabit 6; inner = total-2 olduğundan 6-2=4 kalır.
    // Brifingdeki "5 +" formülü satırı 1 char kısa bırakıyordu (equal-width testini kırıyordu).
    let top = format!("╭─── {}{}╮", title.trim(), "─".repeat(inner.saturating_sub(4 + title.trim().width())));
    let bottom = format!("╰{}╯", "─".repeat(inner));

    let mut lines: Vec<Line> = vec![Line::from(top)];
    for i in 0..rows {
        let (ltxt, is_logo) = left.get(i).cloned().unwrap_or_default();
        let (rtxt, rtxt_style) = right.get(i).cloned().unwrap_or_default();
        let lspan = Span::styled(
            pad(&ltxt, left_w),
            if is_logo { Style::default().fg(ORANGE) } else { Style::default() },
        );
        let rstyle = if i == 0 && !rtxt.is_empty() {
            Style::default().add_modifier(Modifier::BOLD).fg(ORANGE)
        } else { rtxt_style };
        lines.push(Line::from(vec![
            Span::raw("│"),
            lspan,
            Span::raw(" │ "),
            Span::styled(pad(&rtxt, right_w), rstyle),
            Span::raw("│"),
        ]));
    }
    lines.push(Line::from(bottom));
    Text::from(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::text::Text;

    fn plain_lines(t: &Text) -> Vec<String> {
        t.lines.iter().map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect()).collect()
    }

    const PROFILE: &str = "# Öğrenci Profili — Ada\n\n## Kim\n- test";
    const PROGRESS: &str = "# rust — İlerleme\n## Seviye\n- Orta: ownership oturdu\n## Geri çağırma soruları\n- Soru 1? — cevap\n- Soru 2? — cevap\n- Soru 3? — cevap\n";
    const CURRICULUM: &str = "# rust haritası\n- Ownership: oturdu\n- Borrowing: görüldü\n- Lifetimes: görülmedi\n- Traits: görülmedi\n";

    #[test]
    fn extract_name_reads_h1_after_dash() {
        assert_eq!(extract_name(PROFILE), Some("Ada".to_string()));
        assert_eq!(extract_name("# Başlıksız"), None);
        assert_eq!(extract_name(""), None);
    }

    #[test]
    fn extract_level_reads_first_line_of_section() {
        assert_eq!(extract_level(PROGRESS), Some("Orta: ownership oturdu".to_string()));
        assert_eq!(extract_level("# boş"), None);
    }

    #[test]
    fn curriculum_percent_counts_non_unseen() {
        // 4 durumlu madde, 2'si görülmedi → %50
        assert_eq!(curriculum_percent(CURRICULUM), Some(50));
        assert_eq!(curriculum_percent("# durum yok"), None);
    }

    #[test]
    fn next_unseen_returns_first_unseen_item_text() {
        assert_eq!(next_unseen(CURRICULUM), Some("Lifetimes".to_string()));
        assert_eq!(next_unseen("- Hepsi: oturdu"), None);
    }

    #[test]
    fn drill_count_counts_section_bullets() {
        assert_eq!(drill_count(PROGRESS), 3);
        assert_eq!(drill_count("# soru yok"), 0);
    }

    #[test]
    fn gather_full_and_first_session() {
        let d = gather(Some(PROFILE), Some(PROGRESS), Some(CURRICULUM), "rust", "opus · cli", "~/x");
        assert!(!d.first_session);
        assert_eq!(d.name.as_deref(), Some("Ada"));
        assert_eq!(d.map_percent, Some(50));
        let d2 = gather(None, None, None, "gtm", "opus · cli", "~/x");
        assert!(d2.first_session);
        assert_eq!(d2.drill_count, 0);
    }

    #[test]
    fn render_welcome_lines_have_equal_display_width() {
        use unicode_width::UnicodeWidthStr;
        let d = gather(Some(PROFILE), Some(PROGRESS), Some(CURRICULUM), "rust", "opus · cli", "~/proje");
        let t = render_welcome(&d, 80);
        let lines = plain_lines(&t);
        assert!(lines.len() >= 8);
        let w = lines[0].width();
        assert!(lines.iter().all(|l| l.width() == w), "hizasız satır: {lines:#?}");
        assert!(lines[0].starts_with('╭') && lines.last().unwrap().starts_with('╰'));
    }

    #[test]
    fn render_welcome_first_session_shows_intro_message() {
        let d = gather(None, None, None, "gtm", "opus · cli", "~/p");
        let joined = plain_lines(&render_welcome(&d, 80)).join("\n");
        assert!(joined.contains("İlk oturum"));
        assert!(joined.contains("Tekrar hoş geldin"));
    }

    #[test]
    fn fit_truncates_by_display_width_with_ellipsis() {
        assert_eq!(fit("çğşöü-uzun-metin", 8), "çğşöü-u…");
        assert_eq!(fit("kısa", 10), "kısa");
    }

    #[test]
    fn render_identity_with_topics_lists_them_and_equal_width() {
        use unicode_width::UnicodeWidthStr;
        let local = vec!["rust".to_string(), "gtm".to_string()];
        let t = render_welcome_identity(Some("Ada"), "opus · cli", "~/p", &local, &[], 80);
        let lines = plain_lines(&t);
        let w = lines[0].width();
        assert!(lines.iter().all(|l| l.width() == w), "hizasız: {lines:#?}");
        let joined = lines.join("\n");
        assert!(joined.contains("Ne öğrenmek istiyorsun?"));
        assert!(joined.contains("rust"));
        assert!(joined.contains("Merhaba, Ada!"));
        assert!(lines[0].starts_with('╭') && lines.last().unwrap().starts_with('╰'));
    }

    #[test]
    fn render_identity_no_topics_shows_first_session_and_no_name() {
        let t = render_welcome_identity(None, "opus · cli", "~/p", &[], &[], 80);
        let joined = plain_lines(&t).join("\n");
        assert!(joined.contains("Ne öğrenmek istiyorsun?"));
        assert!(joined.contains("Merhaba!"));       // isim yok → jenerik
        assert!(!joined.contains("Merhaba,"));      // "Merhaba, X!" biçimi yok
        assert!(!joined.contains("Enter →"));       // konu yok → devam satırı yok
    }

    #[test]
    fn identity_welcome_lists_local_topics_with_enter_hint() {
        let local = vec!["brainstorm-ilk-adim".to_string(), "linux-guvenlik".to_string()];
        let other = vec!["rust".to_string()];
        let t = render_welcome_identity(Some("Anil"), "opus · cli", "~/x", &local, &other, 80);
        let joined = plain_lines(&t).join("\n");
        assert!(joined.contains("Enter"));
        assert!(joined.contains("brainstorm-ilk-adim"));
        assert!(joined.contains("1)"));
        assert!(joined.contains("2)"));
        assert!(joined.contains("Diğer projelerde"));
        // Hizalama korunur.
        use unicode_width::UnicodeWidthStr;
        let lines = plain_lines(&t);
        let w = lines[0].width();
        assert!(lines.iter().all(|l| l.width() == w), "hizasız: {lines:#?}");
    }

    #[test]
    fn identity_welcome_without_local_topics_keeps_first_run_look() {
        let t = render_welcome_identity(None, "opus · cli", "~/x", &[], &[], 80);
        let joined = plain_lines(&t).join("\n");
        assert!(joined.contains("Ne öğrenmek istiyorsun"));
        assert!(joined.contains("İlk oturum"));
        assert!(!joined.contains("Enter →"));
    }

    #[test]
    fn identity_welcome_other_projects_line_is_dim() {
        let local = vec!["rust".to_string()];
        let other = vec!["gtm".to_string()];
        let t = render_welcome_identity(Some("Ada"), "opus · cli", "~/p", &local, &other, 80);
        let span = t
            .lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .find(|s| s.content.contains("Diğer projelerde"))
            .expect("Diğer projelerde satırı bulunamalı");
        assert!(span.style.add_modifier.contains(Modifier::DIM), "stil DIM içermiyor: {:?}", span.style);
    }
}
