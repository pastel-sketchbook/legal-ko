use ratatui::style::Color;

/// Semantic color theme for the legal-ko TUI.
///
/// The first group of slots is the Pastel Sketchbook standard superset
/// (shared across all ratatui apps: pastel-market, kube-log-viewer, yp).
/// The second group holds domain-specific slots for legal-ko's law/precedent
/// browsing UI.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub name: &'static str,

    // ── Pastel Sketchbook standard slots ──────────────────────
    /// Terminal background.
    pub bg: Color,
    /// Default foreground text.
    pub fg: Color,
    /// Accent for active elements, header borders.
    pub accent: Color,
    /// De-emphasized text (hints, separators).
    pub muted: Color,
    /// Panel border color.
    pub border: Color,
    /// Positive / gain indicator (green family).
    /// Reserved for Pastel Sketchbook compatibility; tested but not yet used in legal-ko UI.
    #[allow(dead_code)]
    pub gain: Color,
    /// Negative / loss indicator (red family).
    /// Reserved for Pastel Sketchbook compatibility; tested but not yet used in legal-ko UI.
    #[allow(dead_code)]
    pub loss: Color,
    /// Error / warning text.
    pub error: Color,
    /// Transient status bar foreground.
    pub status: Color,
    /// Selected row / item background.
    pub highlight_bg: Color,
    /// Selected row / item foreground.
    pub highlight_fg: Color,
    /// Alternating row stripe background.
    pub stripe_bg: Color,
    /// Keyboard shortcut badge background.
    pub key_bg: Color,
    /// Keyboard shortcut badge foreground.
    pub key_fg: Color,
    /// App title color.
    pub title: Color,
    /// Generic tag / label color.
    pub tag: Color,
    /// Panel interior background.
    pub panel_bg: Color,
    /// Chart / sparkline background — subtle shade off `bg`.
    /// Reserved for Pastel Sketchbook compatibility; not yet used in legal-ko.
    #[allow(dead_code)]
    pub chart_bg: Color,

    // ── Domain-specific slots (law browsing) ──────────────────
    /// Major heading — `#` (law title, 편).
    pub heading_major: Color,
    /// Chapter heading — `##` (장).
    pub heading_chapter: Color,
    /// Section heading — `###` (절).
    pub heading_section: Color,
    /// Article heading — `#####` (제X조).
    pub heading_article: Color,
    /// Bookmark star indicator.
    pub bookmark: Color,
    /// Category tag.
    pub category: Color,
    /// Department tag.
    pub department: Color,
    /// Date column (공포일자 / 시행일자).
    pub date: Color,
    /// Search input text / active search.
    pub search: Color,
}

/// Available themes, indexed by position. First entry is the default.
///
/// 16 themes: 8 dark + 8 light. Standard slots synced with pastel-market.
pub const THEMES: &[Theme] = &[
    // ── Dark themes ──────────────────────────────────────────
    // Default — dark, cyan accent, legal blues
    Theme {
        name: "Default",
        bg: Color::Reset,
        fg: Color::White,
        accent: Color::Rgb(0, 217, 255),
        muted: Color::DarkGray,
        border: Color::DarkGray,
        gain: Color::Rgb(0, 200, 80),
        loss: Color::Rgb(255, 80, 80),
        error: Color::Rgb(255, 80, 80),
        status: Color::Rgb(0, 217, 255),
        highlight_bg: Color::Rgb(40, 40, 60),
        highlight_fg: Color::Rgb(255, 220, 100),
        stripe_bg: Color::Rgb(28, 28, 34),
        key_bg: Color::DarkGray,
        key_fg: Color::Black,
        title: Color::Rgb(0, 217, 255),
        tag: Color::Rgb(180, 140, 255),
        panel_bg: Color::Rgb(24, 24, 30),
        chart_bg: Color::Rgb(18, 18, 24),
        heading_major: Color::Rgb(200, 120, 255),
        heading_chapter: Color::Rgb(0, 200, 80),
        heading_section: Color::Rgb(0, 190, 210),
        heading_article: Color::Rgb(240, 200, 60),
        bookmark: Color::Rgb(255, 200, 40),
        category: Color::Rgb(0, 200, 80),
        department: Color::Rgb(80, 160, 255),
        date: Color::Rgb(180, 140, 210),
        search: Color::Rgb(255, 200, 40),
    },
    // Gruvbox Dark — warm earthy tones
    Theme {
        name: "Gruvbox",
        bg: Color::Rgb(29, 32, 33),
        fg: Color::Rgb(235, 219, 178),
        accent: Color::Rgb(215, 153, 33),
        muted: Color::Rgb(146, 131, 116),
        border: Color::Rgb(62, 57, 54),
        gain: Color::Rgb(184, 187, 38),
        loss: Color::Rgb(251, 73, 52),
        error: Color::Rgb(251, 73, 52),
        status: Color::Rgb(184, 187, 38),
        highlight_bg: Color::Rgb(50, 48, 47),
        highlight_fg: Color::Rgb(250, 189, 47),
        stripe_bg: Color::Rgb(40, 40, 40),
        key_bg: Color::Rgb(80, 73, 69),
        key_fg: Color::Rgb(235, 219, 178),
        title: Color::Rgb(250, 189, 47),
        tag: Color::Rgb(131, 165, 152),
        panel_bg: Color::Rgb(37, 36, 36),
        chart_bg: Color::Rgb(22, 24, 25),
        heading_major: Color::Rgb(211, 134, 155),
        heading_chapter: Color::Rgb(184, 187, 38),
        heading_section: Color::Rgb(131, 165, 152),
        heading_article: Color::Rgb(250, 189, 47),
        bookmark: Color::Rgb(250, 189, 47),
        category: Color::Rgb(184, 187, 38),
        department: Color::Rgb(131, 165, 152),
        date: Color::Rgb(211, 134, 155),
        search: Color::Rgb(250, 189, 47),
    },
    // Solarized Dark — blue-cyan palette
    Theme {
        name: "Solarized",
        bg: Color::Rgb(0, 43, 54),
        fg: Color::Rgb(253, 246, 227),
        accent: Color::Rgb(42, 161, 152),
        muted: Color::Rgb(131, 148, 150),
        border: Color::Rgb(16, 58, 68),
        gain: Color::Rgb(133, 153, 0),
        loss: Color::Rgb(220, 50, 47),
        error: Color::Rgb(220, 50, 47),
        status: Color::Rgb(181, 137, 0),
        highlight_bg: Color::Rgb(7, 54, 66),
        highlight_fg: Color::Rgb(253, 246, 227),
        stripe_bg: Color::Rgb(3, 48, 58),
        key_bg: Color::Rgb(88, 110, 117),
        key_fg: Color::Rgb(253, 246, 227),
        title: Color::Rgb(181, 137, 0),
        tag: Color::Rgb(108, 113, 196),
        panel_bg: Color::Rgb(7, 54, 66),
        chart_bg: Color::Rgb(0, 36, 46),
        heading_major: Color::Rgb(211, 54, 130),
        heading_chapter: Color::Rgb(133, 153, 0),
        heading_section: Color::Rgb(42, 161, 152),
        heading_article: Color::Rgb(181, 137, 0),
        bookmark: Color::Rgb(181, 137, 0),
        category: Color::Rgb(133, 153, 0),
        department: Color::Rgb(38, 139, 210),
        date: Color::Rgb(108, 113, 196),
        search: Color::Rgb(181, 137, 0),
    },
    // Ayu Dark — deep blue with orange accents
    Theme {
        name: "Ayu",
        bg: Color::Rgb(10, 14, 20),
        fg: Color::Rgb(191, 191, 191),
        accent: Color::Rgb(255, 153, 64),
        muted: Color::Rgb(92, 103, 115),
        border: Color::Rgb(40, 44, 52),
        gain: Color::Rgb(125, 210, 80),
        loss: Color::Rgb(240, 113, 113),
        error: Color::Rgb(240, 113, 113),
        status: Color::Rgb(85, 180, 211),
        highlight_bg: Color::Rgb(20, 24, 32),
        highlight_fg: Color::Rgb(255, 180, 84),
        stripe_bg: Color::Rgb(15, 19, 26),
        key_bg: Color::Rgb(60, 66, 76),
        key_fg: Color::Rgb(191, 191, 191),
        title: Color::Rgb(255, 180, 84),
        tag: Color::Rgb(210, 154, 230),
        panel_bg: Color::Rgb(18, 22, 30),
        chart_bg: Color::Rgb(6, 9, 14),
        heading_major: Color::Rgb(210, 140, 240),
        heading_chapter: Color::Rgb(125, 210, 80),
        heading_section: Color::Rgb(92, 200, 220),
        heading_article: Color::Rgb(255, 180, 84),
        bookmark: Color::Rgb(255, 180, 84),
        category: Color::Rgb(125, 210, 80),
        department: Color::Rgb(92, 200, 220),
        date: Color::Rgb(210, 140, 240),
        search: Color::Rgb(255, 180, 84),
    },
    // Flexoki Dark — ink-and-paper warmth
    Theme {
        name: "Flexoki",
        bg: Color::Rgb(16, 15, 15),
        fg: Color::Rgb(206, 205, 195),
        accent: Color::Rgb(36, 131, 123),
        muted: Color::Rgb(135, 133, 128),
        border: Color::Rgb(40, 39, 38),
        gain: Color::Rgb(208, 162, 21),
        loss: Color::Rgb(209, 77, 65),
        error: Color::Rgb(209, 77, 65),
        status: Color::Rgb(208, 162, 21),
        highlight_bg: Color::Rgb(28, 27, 26),
        highlight_fg: Color::Rgb(208, 162, 21),
        stripe_bg: Color::Rgb(22, 21, 20),
        key_bg: Color::Rgb(52, 51, 49),
        key_fg: Color::Rgb(206, 205, 195),
        title: Color::Rgb(208, 162, 21),
        tag: Color::Rgb(142, 139, 206),
        panel_bg: Color::Rgb(24, 23, 22),
        chart_bg: Color::Rgb(10, 9, 9),
        heading_major: Color::Rgb(206, 93, 151),
        heading_chapter: Color::Rgb(102, 128, 11),
        heading_section: Color::Rgb(36, 131, 123),
        heading_article: Color::Rgb(208, 162, 21),
        bookmark: Color::Rgb(208, 162, 21),
        category: Color::Rgb(102, 128, 11),
        department: Color::Rgb(36, 131, 123),
        date: Color::Rgb(206, 93, 151),
        search: Color::Rgb(208, 162, 21),
    },
    // Zoegi Dark — muted monochrome with green accent
    Theme {
        name: "Zoegi",
        bg: Color::Rgb(20, 20, 20),
        fg: Color::Rgb(204, 204, 204),
        accent: Color::Rgb(64, 128, 104),
        muted: Color::Rgb(89, 89, 89),
        border: Color::Rgb(48, 48, 48),
        gain: Color::Rgb(92, 168, 112),
        loss: Color::Rgb(204, 92, 92),
        error: Color::Rgb(204, 92, 92),
        status: Color::Rgb(86, 139, 153),
        highlight_bg: Color::Rgb(34, 34, 34),
        highlight_fg: Color::Rgb(128, 200, 160),
        stripe_bg: Color::Rgb(27, 27, 27),
        key_bg: Color::Rgb(64, 64, 64),
        key_fg: Color::Rgb(204, 204, 204),
        title: Color::Rgb(128, 200, 160),
        tag: Color::Rgb(150, 180, 210),
        panel_bg: Color::Rgb(28, 28, 28),
        chart_bg: Color::Rgb(14, 14, 14),
        heading_major: Color::Rgb(180, 140, 200),
        heading_chapter: Color::Rgb(92, 168, 112),
        heading_section: Color::Rgb(100, 170, 180),
        heading_article: Color::Rgb(128, 200, 160),
        bookmark: Color::Rgb(128, 200, 160),
        category: Color::Rgb(92, 168, 112),
        department: Color::Rgb(100, 170, 180),
        date: Color::Rgb(180, 140, 200),
        search: Color::Rgb(128, 200, 160),
    },
    // FFE Dark — Nordic-inspired cool blues
    Theme {
        name: "FFE Dark",
        bg: Color::Rgb(30, 35, 43),
        fg: Color::Rgb(216, 222, 233),
        accent: Color::Rgb(79, 214, 190),
        muted: Color::Rgb(155, 162, 175),
        border: Color::Rgb(59, 66, 82),
        gain: Color::Rgb(161, 239, 211),
        loss: Color::Rgb(255, 117, 127),
        error: Color::Rgb(255, 117, 127),
        status: Color::Rgb(161, 239, 211),
        highlight_bg: Color::Rgb(46, 52, 64),
        highlight_fg: Color::Rgb(240, 169, 136),
        stripe_bg: Color::Rgb(26, 31, 39),
        key_bg: Color::Rgb(59, 66, 82),
        key_fg: Color::Rgb(216, 222, 233),
        title: Color::Rgb(240, 169, 136),
        tag: Color::Rgb(137, 220, 235),
        panel_bg: Color::Rgb(26, 31, 39),
        chart_bg: Color::Rgb(22, 26, 34),
        heading_major: Color::Rgb(200, 150, 230),
        heading_chapter: Color::Rgb(161, 239, 211),
        heading_section: Color::Rgb(129, 161, 193),
        heading_article: Color::Rgb(240, 169, 136),
        bookmark: Color::Rgb(240, 169, 136),
        category: Color::Rgb(161, 239, 211),
        department: Color::Rgb(129, 161, 193),
        date: Color::Rgb(200, 150, 230),
        search: Color::Rgb(240, 169, 136),
    },
    // Postrboard Dark — slate blue with lime/orange accents
    Theme {
        name: "Postrboard",
        bg: Color::Rgb(26, 27, 38),
        fg: Color::Rgb(226, 232, 240),
        accent: Color::Rgb(79, 182, 232),
        muted: Color::Rgb(124, 141, 163),
        border: Color::Rgb(42, 45, 61),
        gain: Color::Rgb(132, 204, 22),
        loss: Color::Rgb(248, 113, 113),
        error: Color::Rgb(248, 113, 113),
        status: Color::Rgb(132, 204, 22),
        highlight_bg: Color::Rgb(54, 58, 79),
        highlight_fg: Color::Rgb(251, 138, 77),
        stripe_bg: Color::Rgb(30, 31, 43),
        key_bg: Color::Rgb(54, 58, 79),
        key_fg: Color::Rgb(226, 232, 240),
        title: Color::Rgb(251, 138, 77),
        tag: Color::Rgb(96, 165, 250),
        panel_bg: Color::Rgb(22, 23, 31),
        chart_bg: Color::Rgb(18, 19, 28),
        heading_major: Color::Rgb(192, 132, 252),
        heading_chapter: Color::Rgb(132, 204, 22),
        heading_section: Color::Rgb(79, 182, 232),
        heading_article: Color::Rgb(251, 138, 77),
        bookmark: Color::Rgb(251, 138, 77),
        category: Color::Rgb(132, 204, 22),
        department: Color::Rgb(96, 165, 250),
        date: Color::Rgb(192, 132, 252),
        search: Color::Rgb(251, 138, 77),
    },
    // ── Light themes ─────────────────────────────────────────
    // Default Light — transparent bg, dark text
    Theme {
        name: "Default Light",
        bg: Color::Reset,
        fg: Color::Rgb(40, 40, 50),
        accent: Color::Rgb(0, 140, 180),
        muted: Color::Rgb(120, 120, 130),
        border: Color::Rgb(180, 180, 190),
        gain: Color::Rgb(0, 140, 50),
        loss: Color::Rgb(200, 40, 40),
        error: Color::Rgb(200, 40, 40),
        status: Color::Rgb(0, 140, 180),
        highlight_bg: Color::Rgb(220, 225, 235),
        highlight_fg: Color::Rgb(30, 30, 40),
        stripe_bg: Color::Rgb(240, 240, 245),
        key_bg: Color::Rgb(180, 180, 190),
        key_fg: Color::Rgb(40, 40, 50),
        title: Color::Rgb(0, 140, 180),
        tag: Color::Rgb(100, 80, 180),
        panel_bg: Color::Rgb(235, 235, 240),
        chart_bg: Color::Rgb(245, 245, 248),
        heading_major: Color::Rgb(140, 60, 180),
        heading_chapter: Color::Rgb(0, 140, 50),
        heading_section: Color::Rgb(0, 130, 160),
        heading_article: Color::Rgb(180, 120, 0),
        bookmark: Color::Rgb(180, 120, 0),
        category: Color::Rgb(0, 140, 50),
        department: Color::Rgb(0, 100, 200),
        date: Color::Rgb(140, 60, 180),
        search: Color::Rgb(180, 120, 0),
    },
    // Gruvbox Light — warm parchment tones
    Theme {
        name: "Gruvbox Light",
        bg: Color::Rgb(251, 241, 199),
        fg: Color::Rgb(60, 56, 54),
        accent: Color::Rgb(215, 153, 33),
        muted: Color::Rgb(146, 131, 116),
        border: Color::Rgb(213, 196, 161),
        gain: Color::Rgb(121, 116, 14),
        loss: Color::Rgb(204, 36, 29),
        error: Color::Rgb(204, 36, 29),
        status: Color::Rgb(121, 116, 14),
        highlight_bg: Color::Rgb(235, 219, 178),
        highlight_fg: Color::Rgb(60, 56, 54),
        stripe_bg: Color::Rgb(249, 236, 186),
        key_bg: Color::Rgb(213, 196, 161),
        key_fg: Color::Rgb(60, 56, 54),
        title: Color::Rgb(215, 153, 33),
        tag: Color::Rgb(69, 133, 136),
        panel_bg: Color::Rgb(242, 233, 185),
        chart_bg: Color::Rgb(245, 236, 192),
        heading_major: Color::Rgb(177, 98, 134),
        heading_chapter: Color::Rgb(121, 116, 14),
        heading_section: Color::Rgb(69, 133, 136),
        heading_article: Color::Rgb(215, 153, 33),
        bookmark: Color::Rgb(215, 153, 33),
        category: Color::Rgb(121, 116, 14),
        department: Color::Rgb(69, 133, 136),
        date: Color::Rgb(177, 98, 134),
        search: Color::Rgb(215, 153, 33),
    },
    // Solarized Light — bright blue-cyan
    Theme {
        name: "Solarized Light",
        bg: Color::Rgb(253, 246, 227),
        fg: Color::Rgb(88, 110, 117),
        accent: Color::Rgb(42, 161, 152),
        muted: Color::Rgb(147, 161, 161),
        border: Color::Rgb(220, 212, 188),
        gain: Color::Rgb(133, 153, 0),
        loss: Color::Rgb(220, 50, 47),
        error: Color::Rgb(220, 50, 47),
        status: Color::Rgb(133, 153, 0),
        highlight_bg: Color::Rgb(238, 232, 213),
        highlight_fg: Color::Rgb(7, 54, 66),
        stripe_bg: Color::Rgb(245, 239, 218),
        key_bg: Color::Rgb(220, 212, 188),
        key_fg: Color::Rgb(88, 110, 117),
        title: Color::Rgb(181, 137, 0),
        tag: Color::Rgb(108, 113, 196),
        panel_bg: Color::Rgb(238, 232, 213),
        chart_bg: Color::Rgb(247, 241, 222),
        heading_major: Color::Rgb(211, 54, 130),
        heading_chapter: Color::Rgb(133, 153, 0),
        heading_section: Color::Rgb(42, 161, 152),
        heading_article: Color::Rgb(181, 137, 0),
        bookmark: Color::Rgb(181, 137, 0),
        category: Color::Rgb(133, 153, 0),
        department: Color::Rgb(38, 139, 210),
        date: Color::Rgb(108, 113, 196),
        search: Color::Rgb(181, 137, 0),
    },
    // Flexoki Light — soft warm paper
    Theme {
        name: "Flexoki Light",
        bg: Color::Rgb(255, 252, 240),
        fg: Color::Rgb(16, 15, 15),
        accent: Color::Rgb(36, 131, 123),
        muted: Color::Rgb(111, 110, 105),
        border: Color::Rgb(230, 228, 217),
        gain: Color::Rgb(102, 128, 11),
        loss: Color::Rgb(209, 77, 65),
        error: Color::Rgb(209, 77, 65),
        status: Color::Rgb(102, 128, 11),
        highlight_bg: Color::Rgb(242, 240, 229),
        highlight_fg: Color::Rgb(16, 15, 15),
        stripe_bg: Color::Rgb(247, 245, 234),
        key_bg: Color::Rgb(230, 228, 217),
        key_fg: Color::Rgb(16, 15, 15),
        title: Color::Rgb(36, 131, 123),
        tag: Color::Rgb(100, 92, 187),
        panel_bg: Color::Rgb(244, 241, 230),
        chart_bg: Color::Rgb(250, 247, 235),
        heading_major: Color::Rgb(206, 93, 151),
        heading_chapter: Color::Rgb(102, 128, 11),
        heading_section: Color::Rgb(36, 131, 123),
        heading_article: Color::Rgb(188, 146, 0),
        bookmark: Color::Rgb(188, 146, 0),
        category: Color::Rgb(102, 128, 11),
        department: Color::Rgb(36, 131, 123),
        date: Color::Rgb(206, 93, 151),
        search: Color::Rgb(188, 146, 0),
    },
    // Ayu Light — bright with orange warmth
    Theme {
        name: "Ayu Light",
        bg: Color::Rgb(252, 252, 252),
        fg: Color::Rgb(92, 97, 102),
        accent: Color::Rgb(255, 153, 64),
        muted: Color::Rgb(153, 160, 166),
        border: Color::Rgb(207, 209, 210),
        gain: Color::Rgb(133, 179, 4),
        loss: Color::Rgb(240, 113, 113),
        error: Color::Rgb(240, 113, 113),
        status: Color::Rgb(133, 179, 4),
        highlight_bg: Color::Rgb(230, 230, 230),
        highlight_fg: Color::Rgb(92, 97, 102),
        stripe_bg: Color::Rgb(243, 244, 245),
        key_bg: Color::Rgb(207, 209, 210),
        key_fg: Color::Rgb(92, 97, 102),
        title: Color::Rgb(255, 153, 64),
        tag: Color::Rgb(163, 122, 204),
        panel_bg: Color::Rgb(242, 242, 242),
        chart_bg: Color::Rgb(246, 246, 246),
        heading_major: Color::Rgb(163, 80, 197),
        heading_chapter: Color::Rgb(133, 179, 4),
        heading_section: Color::Rgb(55, 152, 168),
        heading_article: Color::Rgb(255, 153, 64),
        bookmark: Color::Rgb(255, 153, 64),
        category: Color::Rgb(133, 179, 4),
        department: Color::Rgb(55, 152, 168),
        date: Color::Rgb(163, 80, 197),
        search: Color::Rgb(255, 153, 64),
    },
    // Zoegi Light — clean minimal green
    Theme {
        name: "Zoegi Light",
        bg: Color::Rgb(255, 255, 255),
        fg: Color::Rgb(51, 51, 51),
        accent: Color::Rgb(55, 121, 97),
        muted: Color::Rgb(89, 89, 89),
        border: Color::Rgb(230, 230, 230),
        gain: Color::Rgb(55, 121, 97),
        loss: Color::Rgb(204, 92, 92),
        error: Color::Rgb(204, 92, 92),
        status: Color::Rgb(55, 121, 97),
        highlight_bg: Color::Rgb(235, 235, 235),
        highlight_fg: Color::Rgb(51, 51, 51),
        stripe_bg: Color::Rgb(247, 247, 247),
        key_bg: Color::Rgb(230, 230, 230),
        key_fg: Color::Rgb(51, 51, 51),
        title: Color::Rgb(55, 121, 97),
        tag: Color::Rgb(80, 120, 160),
        panel_bg: Color::Rgb(245, 245, 245),
        chart_bg: Color::Rgb(250, 250, 250),
        heading_major: Color::Rgb(130, 90, 160),
        heading_chapter: Color::Rgb(55, 121, 97),
        heading_section: Color::Rgb(70, 140, 150),
        heading_article: Color::Rgb(160, 120, 30),
        bookmark: Color::Rgb(160, 120, 30),
        category: Color::Rgb(55, 121, 97),
        department: Color::Rgb(70, 140, 150),
        date: Color::Rgb(130, 90, 160),
        search: Color::Rgb(160, 120, 30),
    },
    // FFE Light — soft Nordic daylight
    Theme {
        name: "FFE Light",
        bg: Color::Rgb(232, 236, 240),
        fg: Color::Rgb(30, 35, 43),
        accent: Color::Rgb(42, 157, 132),
        muted: Color::Rgb(74, 80, 96),
        border: Color::Rgb(201, 205, 214),
        gain: Color::Rgb(26, 138, 110),
        loss: Color::Rgb(201, 67, 78),
        error: Color::Rgb(201, 67, 78),
        status: Color::Rgb(26, 138, 110),
        highlight_bg: Color::Rgb(221, 225, 232),
        highlight_fg: Color::Rgb(192, 121, 32),
        stripe_bg: Color::Rgb(245, 247, 250),
        key_bg: Color::Rgb(201, 205, 214),
        key_fg: Color::Rgb(30, 35, 43),
        title: Color::Rgb(192, 121, 32),
        tag: Color::Rgb(58, 142, 164),
        panel_bg: Color::Rgb(245, 247, 250),
        chart_bg: Color::Rgb(227, 231, 236),
        heading_major: Color::Rgb(160, 100, 200),
        heading_chapter: Color::Rgb(26, 138, 110),
        heading_section: Color::Rgb(42, 157, 132),
        heading_article: Color::Rgb(192, 121, 32),
        bookmark: Color::Rgb(192, 121, 32),
        category: Color::Rgb(26, 138, 110),
        department: Color::Rgb(42, 157, 132),
        date: Color::Rgb(160, 100, 200),
        search: Color::Rgb(192, 121, 32),
    },
    // Postrboard Light — clean slate with warm orange accents
    Theme {
        name: "Postrboard Light",
        bg: Color::Rgb(250, 250, 250),
        fg: Color::Rgb(17, 24, 39),
        accent: Color::Rgb(2, 132, 199),
        muted: Color::Rgb(100, 116, 139),
        border: Color::Rgb(203, 213, 225),
        gain: Color::Rgb(77, 124, 15),
        loss: Color::Rgb(220, 38, 38),
        error: Color::Rgb(220, 38, 38),
        status: Color::Rgb(77, 124, 15),
        highlight_bg: Color::Rgb(226, 232, 240),
        highlight_fg: Color::Rgb(194, 65, 12),
        stripe_bg: Color::Rgb(248, 250, 252),
        key_bg: Color::Rgb(203, 213, 225),
        key_fg: Color::Rgb(17, 24, 39),
        title: Color::Rgb(194, 65, 12),
        tag: Color::Rgb(12, 74, 110),
        panel_bg: Color::Rgb(241, 245, 249),
        chart_bg: Color::Rgb(244, 244, 244),
        heading_major: Color::Rgb(147, 51, 234),
        heading_chapter: Color::Rgb(77, 124, 15),
        heading_section: Color::Rgb(2, 132, 199),
        heading_article: Color::Rgb(194, 65, 12),
        bookmark: Color::Rgb(194, 65, 12),
        category: Color::Rgb(77, 124, 15),
        department: Color::Rgb(12, 74, 110),
        date: Color::Rgb(147, 51, 234),
        search: Color::Rgb(194, 65, 12),
    },
];

/// Look up a theme index by name (case-sensitive). Returns 0 (Default)
/// if no theme matches.
#[must_use]
pub fn theme_index_by_name(name: &str) -> usize {
    THEMES.iter().position(|t| t.name == name).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_count_matches_expected() {
        assert_eq!(THEMES.len(), 16);
    }

    #[test]
    fn default_theme_is_first() {
        assert_eq!(THEMES[0].name, "Default");
    }

    #[test]
    fn theme_index_by_name_found() {
        assert_eq!(theme_index_by_name("Gruvbox"), 1);
        assert_eq!(theme_index_by_name("Ayu"), 3);
    }

    #[test]
    fn theme_index_by_name_not_found_returns_zero() {
        assert_eq!(theme_index_by_name("Nonexistent"), 0);
    }

    #[test]
    fn all_themes_have_unique_names() {
        let names: Vec<&str> = THEMES.iter().map(|t| t.name).collect();
        for (i, name) in names.iter().enumerate() {
            assert!(!names[..i].contains(name), "Duplicate theme name: {name}");
        }
    }

    #[test]
    fn all_themes_have_non_empty_names() {
        for theme in THEMES {
            assert!(!theme.name.is_empty(), "Theme name must not be empty");
        }
    }

    #[test]
    fn default_theme_gain_is_green_family() {
        if let Color::Rgb(r, g, b) = THEMES[0].gain {
            assert!(g > r, "gain green channel should dominate red");
            assert!(g > b, "gain green channel should dominate blue");
        } else {
            panic!("Default gain should be Color::Rgb");
        }
    }

    #[test]
    fn default_theme_loss_is_red_family() {
        if let Color::Rgb(r, g, b) = THEMES[0].loss {
            assert!(r > g, "loss red channel should dominate green");
            assert!(r > b, "loss red channel should dominate blue");
        } else {
            panic!("Default loss should be Color::Rgb");
        }
    }

    #[test]
    fn dark_light_pairs_exist() {
        // Every dark theme (first 8) should have a matching light variant
        let dark_names: Vec<&str> = THEMES[..8].iter().map(|t| t.name).collect();
        let light_names: Vec<&str> = THEMES[8..].iter().map(|t| t.name).collect();
        assert_eq!(
            dark_names.len(),
            light_names.len(),
            "dark/light count mismatch"
        );
        // "X" → "X Light", but "X Dark" → "X Light" (strip " Dark" suffix)
        for dark in &dark_names[1..] {
            let base = dark.strip_suffix(" Dark").unwrap_or(dark);
            let expected_light = format!("{base} Light");
            assert!(
                light_names.contains(&expected_light.as_str()),
                "Missing light variant for {dark}"
            );
        }
    }
}
