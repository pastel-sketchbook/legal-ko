use ratatui::style::Color;

/// Semantic color theme for the legal-ko TUI.
///
/// Each field maps to a UI purpose — widgets reference `theme.accent` or
/// `theme.border` rather than a raw `Color::Cyan` or `Color::DarkGray`.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub name: &'static str,
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
    /// Selected row / item background.
    pub highlight_bg: Color,
    /// Selected row / item foreground.
    pub highlight_fg: Color,
    /// Keyboard shortcut badge background.
    pub key_bg: Color,
    /// Keyboard shortcut badge foreground.
    pub key_fg: Color,
    /// App title color.
    pub title: Color,
    /// Panel interior background.
    pub panel_bg: Color,
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
    /// Search input text / active search.
    pub search: Color,
    /// Error / warning text.
    pub error: Color,
}

/// Available themes, indexed by position. First entry is the default.
pub const THEMES: &[Theme] = &[
    // Default — dark, cyan accent, legal blues
    Theme {
        name: "Default",
        bg: Color::Reset,
        fg: Color::White,
        accent: Color::Rgb(0, 217, 255),
        muted: Color::DarkGray,
        border: Color::DarkGray,
        highlight_bg: Color::Rgb(40, 40, 60),
        highlight_fg: Color::Rgb(255, 220, 100),
        key_bg: Color::DarkGray,
        key_fg: Color::Black,
        title: Color::Rgb(0, 217, 255),
        panel_bg: Color::Rgb(24, 24, 30),
        heading_major: Color::Rgb(200, 120, 255),
        heading_chapter: Color::Rgb(0, 200, 80),
        heading_section: Color::Rgb(0, 190, 210),
        heading_article: Color::Rgb(240, 200, 60),
        bookmark: Color::Rgb(255, 200, 40),
        category: Color::Rgb(0, 200, 80),
        department: Color::Rgb(80, 160, 255),
        search: Color::Rgb(255, 200, 40),
        error: Color::Rgb(255, 80, 80),
    },
    // Gruvbox Dark — warm earthy tones
    Theme {
        name: "Gruvbox",
        bg: Color::Rgb(29, 32, 33),
        fg: Color::Rgb(235, 219, 178),
        accent: Color::Rgb(215, 153, 33),
        muted: Color::Rgb(146, 131, 116),
        border: Color::Rgb(62, 57, 54),
        highlight_bg: Color::Rgb(50, 48, 47),
        highlight_fg: Color::Rgb(250, 189, 47),
        key_bg: Color::Rgb(80, 73, 69),
        key_fg: Color::Rgb(235, 219, 178),
        title: Color::Rgb(250, 189, 47),
        panel_bg: Color::Rgb(37, 36, 36),
        heading_major: Color::Rgb(211, 134, 155),
        heading_chapter: Color::Rgb(184, 187, 38),
        heading_section: Color::Rgb(131, 165, 152),
        heading_article: Color::Rgb(250, 189, 47),
        bookmark: Color::Rgb(250, 189, 47),
        category: Color::Rgb(184, 187, 38),
        department: Color::Rgb(131, 165, 152),
        search: Color::Rgb(250, 189, 47),
        error: Color::Rgb(251, 73, 52),
    },
    // Solarized Dark — blue-cyan palette
    Theme {
        name: "Solarized",
        bg: Color::Rgb(0, 43, 54),
        fg: Color::Rgb(253, 246, 227),
        accent: Color::Rgb(42, 161, 152),
        muted: Color::Rgb(131, 148, 150),
        border: Color::Rgb(16, 58, 68),
        highlight_bg: Color::Rgb(7, 54, 66),
        highlight_fg: Color::Rgb(253, 246, 227),
        key_bg: Color::Rgb(88, 110, 117),
        key_fg: Color::Rgb(253, 246, 227),
        title: Color::Rgb(181, 137, 0),
        panel_bg: Color::Rgb(7, 54, 66),
        heading_major: Color::Rgb(211, 54, 130),
        heading_chapter: Color::Rgb(133, 153, 0),
        heading_section: Color::Rgb(42, 161, 152),
        heading_article: Color::Rgb(181, 137, 0),
        bookmark: Color::Rgb(181, 137, 0),
        category: Color::Rgb(133, 153, 0),
        department: Color::Rgb(38, 139, 210),
        search: Color::Rgb(181, 137, 0),
        error: Color::Rgb(220, 50, 47),
    },
    // Ayu Dark — deep blue with orange accents
    Theme {
        name: "Ayu",
        bg: Color::Rgb(10, 14, 20),
        fg: Color::Rgb(191, 191, 191),
        accent: Color::Rgb(255, 153, 64),
        muted: Color::Rgb(92, 103, 115),
        border: Color::Rgb(40, 44, 52),
        highlight_bg: Color::Rgb(20, 24, 32),
        highlight_fg: Color::Rgb(255, 180, 84),
        key_bg: Color::Rgb(60, 66, 76),
        key_fg: Color::Rgb(191, 191, 191),
        title: Color::Rgb(255, 180, 84),
        panel_bg: Color::Rgb(18, 22, 30),
        heading_major: Color::Rgb(210, 140, 240),
        heading_chapter: Color::Rgb(125, 210, 80),
        heading_section: Color::Rgb(92, 200, 220),
        heading_article: Color::Rgb(255, 180, 84),
        bookmark: Color::Rgb(255, 180, 84),
        category: Color::Rgb(125, 210, 80),
        department: Color::Rgb(92, 200, 220),
        search: Color::Rgb(255, 180, 84),
        error: Color::Rgb(240, 113, 113),
    },
    // Flexoki Dark — ink-and-paper warmth
    Theme {
        name: "Flexoki",
        bg: Color::Rgb(16, 15, 15),
        fg: Color::Rgb(206, 205, 195),
        accent: Color::Rgb(36, 131, 123),
        muted: Color::Rgb(135, 133, 128),
        border: Color::Rgb(40, 39, 38),
        highlight_bg: Color::Rgb(28, 27, 26),
        highlight_fg: Color::Rgb(208, 162, 21),
        key_bg: Color::Rgb(52, 51, 49),
        key_fg: Color::Rgb(206, 205, 195),
        title: Color::Rgb(208, 162, 21),
        panel_bg: Color::Rgb(24, 23, 22),
        heading_major: Color::Rgb(206, 93, 151),
        heading_chapter: Color::Rgb(102, 128, 11),
        heading_section: Color::Rgb(36, 131, 123),
        heading_article: Color::Rgb(208, 162, 21),
        bookmark: Color::Rgb(208, 162, 21),
        category: Color::Rgb(102, 128, 11),
        department: Color::Rgb(36, 131, 123),
        search: Color::Rgb(208, 162, 21),
        error: Color::Rgb(209, 77, 65),
    },
    // Zoegi Dark — muted monochrome with green accent
    Theme {
        name: "Zoegi",
        bg: Color::Rgb(20, 20, 20),
        fg: Color::Rgb(204, 204, 204),
        accent: Color::Rgb(64, 128, 104),
        muted: Color::Rgb(89, 89, 89),
        border: Color::Rgb(48, 48, 48),
        highlight_bg: Color::Rgb(34, 34, 34),
        highlight_fg: Color::Rgb(128, 200, 160),
        key_bg: Color::Rgb(64, 64, 64),
        key_fg: Color::Rgb(204, 204, 204),
        title: Color::Rgb(128, 200, 160),
        panel_bg: Color::Rgb(28, 28, 28),
        heading_major: Color::Rgb(180, 140, 200),
        heading_chapter: Color::Rgb(92, 168, 112),
        heading_section: Color::Rgb(100, 170, 180),
        heading_article: Color::Rgb(128, 200, 160),
        bookmark: Color::Rgb(128, 200, 160),
        category: Color::Rgb(92, 168, 112),
        department: Color::Rgb(100, 170, 180),
        search: Color::Rgb(128, 200, 160),
        error: Color::Rgb(204, 92, 92),
    },
    // FFE Dark — Nordic-inspired cool blues
    Theme {
        name: "FFE Dark",
        bg: Color::Rgb(30, 35, 43),
        fg: Color::Rgb(216, 222, 233),
        accent: Color::Rgb(79, 214, 190),
        muted: Color::Rgb(155, 162, 175),
        border: Color::Rgb(59, 66, 82),
        highlight_bg: Color::Rgb(46, 52, 64),
        highlight_fg: Color::Rgb(240, 169, 136),
        key_bg: Color::Rgb(59, 66, 82),
        key_fg: Color::Rgb(216, 222, 233),
        title: Color::Rgb(240, 169, 136),
        panel_bg: Color::Rgb(26, 31, 39),
        heading_major: Color::Rgb(200, 150, 230),
        heading_chapter: Color::Rgb(161, 239, 211),
        heading_section: Color::Rgb(129, 161, 193),
        heading_article: Color::Rgb(240, 169, 136),
        bookmark: Color::Rgb(240, 169, 136),
        category: Color::Rgb(161, 239, 211),
        department: Color::Rgb(129, 161, 193),
        search: Color::Rgb(240, 169, 136),
        error: Color::Rgb(255, 117, 127),
    },
    // --- Light themes ---
    // Default Light — transparent bg, dark text
    Theme {
        name: "Default Light",
        bg: Color::Reset,
        fg: Color::Rgb(40, 40, 50),
        accent: Color::Rgb(0, 140, 180),
        muted: Color::Rgb(120, 120, 130),
        border: Color::Rgb(180, 180, 190),
        highlight_bg: Color::Rgb(220, 225, 235),
        highlight_fg: Color::Rgb(30, 30, 40),
        key_bg: Color::Rgb(180, 180, 190),
        key_fg: Color::Rgb(40, 40, 50),
        title: Color::Rgb(0, 140, 180),
        panel_bg: Color::Rgb(235, 235, 240),
        heading_major: Color::Rgb(140, 60, 180),
        heading_chapter: Color::Rgb(0, 140, 50),
        heading_section: Color::Rgb(0, 130, 160),
        heading_article: Color::Rgb(180, 120, 0),
        bookmark: Color::Rgb(180, 120, 0),
        category: Color::Rgb(0, 140, 50),
        department: Color::Rgb(0, 100, 200),
        search: Color::Rgb(180, 120, 0),
        error: Color::Rgb(200, 40, 40),
    },
    // Gruvbox Light — warm parchment tones
    Theme {
        name: "Gruvbox Light",
        bg: Color::Rgb(251, 241, 199),
        fg: Color::Rgb(60, 56, 54),
        accent: Color::Rgb(215, 153, 33),
        muted: Color::Rgb(146, 131, 116),
        border: Color::Rgb(213, 196, 161),
        highlight_bg: Color::Rgb(235, 219, 178),
        highlight_fg: Color::Rgb(60, 56, 54),
        key_bg: Color::Rgb(213, 196, 161),
        key_fg: Color::Rgb(60, 56, 54),
        title: Color::Rgb(215, 153, 33),
        panel_bg: Color::Rgb(242, 233, 185),
        heading_major: Color::Rgb(177, 98, 134),
        heading_chapter: Color::Rgb(121, 116, 14),
        heading_section: Color::Rgb(69, 133, 136),
        heading_article: Color::Rgb(215, 153, 33),
        bookmark: Color::Rgb(215, 153, 33),
        category: Color::Rgb(121, 116, 14),
        department: Color::Rgb(69, 133, 136),
        search: Color::Rgb(215, 153, 33),
        error: Color::Rgb(204, 36, 29),
    },
    // Solarized Light — bright blue-cyan
    Theme {
        name: "Solarized Light",
        bg: Color::Rgb(253, 246, 227),
        fg: Color::Rgb(88, 110, 117),
        accent: Color::Rgb(42, 161, 152),
        muted: Color::Rgb(147, 161, 161),
        border: Color::Rgb(220, 212, 188),
        highlight_bg: Color::Rgb(238, 232, 213),
        highlight_fg: Color::Rgb(7, 54, 66),
        key_bg: Color::Rgb(220, 212, 188),
        key_fg: Color::Rgb(88, 110, 117),
        title: Color::Rgb(181, 137, 0),
        panel_bg: Color::Rgb(238, 232, 213),
        heading_major: Color::Rgb(211, 54, 130),
        heading_chapter: Color::Rgb(133, 153, 0),
        heading_section: Color::Rgb(42, 161, 152),
        heading_article: Color::Rgb(181, 137, 0),
        bookmark: Color::Rgb(181, 137, 0),
        category: Color::Rgb(133, 153, 0),
        department: Color::Rgb(38, 139, 210),
        search: Color::Rgb(181, 137, 0),
        error: Color::Rgb(220, 50, 47),
    },
    // Flexoki Light — soft warm paper
    Theme {
        name: "Flexoki Light",
        bg: Color::Rgb(255, 252, 240),
        fg: Color::Rgb(16, 15, 15),
        accent: Color::Rgb(36, 131, 123),
        muted: Color::Rgb(111, 110, 105),
        border: Color::Rgb(230, 228, 217),
        highlight_bg: Color::Rgb(242, 240, 229),
        highlight_fg: Color::Rgb(16, 15, 15),
        key_bg: Color::Rgb(230, 228, 217),
        key_fg: Color::Rgb(16, 15, 15),
        title: Color::Rgb(36, 131, 123),
        panel_bg: Color::Rgb(244, 241, 230),
        heading_major: Color::Rgb(206, 93, 151),
        heading_chapter: Color::Rgb(102, 128, 11),
        heading_section: Color::Rgb(36, 131, 123),
        heading_article: Color::Rgb(188, 146, 0),
        bookmark: Color::Rgb(188, 146, 0),
        category: Color::Rgb(102, 128, 11),
        department: Color::Rgb(36, 131, 123),
        search: Color::Rgb(188, 146, 0),
        error: Color::Rgb(209, 77, 65),
    },
    // Ayu Light — bright with orange warmth
    Theme {
        name: "Ayu Light",
        bg: Color::Rgb(252, 252, 252),
        fg: Color::Rgb(92, 97, 102),
        accent: Color::Rgb(255, 153, 64),
        muted: Color::Rgb(153, 160, 166),
        border: Color::Rgb(207, 209, 210),
        highlight_bg: Color::Rgb(230, 230, 230),
        highlight_fg: Color::Rgb(92, 97, 102),
        key_bg: Color::Rgb(207, 209, 210),
        key_fg: Color::Rgb(92, 97, 102),
        title: Color::Rgb(255, 153, 64),
        panel_bg: Color::Rgb(242, 242, 242),
        heading_major: Color::Rgb(163, 80, 197),
        heading_chapter: Color::Rgb(133, 179, 4),
        heading_section: Color::Rgb(55, 152, 168),
        heading_article: Color::Rgb(255, 153, 64),
        bookmark: Color::Rgb(255, 153, 64),
        category: Color::Rgb(133, 179, 4),
        department: Color::Rgb(55, 152, 168),
        search: Color::Rgb(255, 153, 64),
        error: Color::Rgb(240, 113, 113),
    },
    // Zoegi Light — clean minimal green
    Theme {
        name: "Zoegi Light",
        bg: Color::Rgb(255, 255, 255),
        fg: Color::Rgb(51, 51, 51),
        accent: Color::Rgb(55, 121, 97),
        muted: Color::Rgb(89, 89, 89),
        border: Color::Rgb(230, 230, 230),
        highlight_bg: Color::Rgb(235, 235, 235),
        highlight_fg: Color::Rgb(51, 51, 51),
        key_bg: Color::Rgb(230, 230, 230),
        key_fg: Color::Rgb(51, 51, 51),
        title: Color::Rgb(55, 121, 97),
        panel_bg: Color::Rgb(245, 245, 245),
        heading_major: Color::Rgb(130, 90, 160),
        heading_chapter: Color::Rgb(55, 121, 97),
        heading_section: Color::Rgb(70, 140, 150),
        heading_article: Color::Rgb(160, 120, 30),
        bookmark: Color::Rgb(160, 120, 30),
        category: Color::Rgb(55, 121, 97),
        department: Color::Rgb(70, 140, 150),
        search: Color::Rgb(160, 120, 30),
        error: Color::Rgb(204, 92, 92),
    },
    // FFE Light — soft Nordic daylight
    Theme {
        name: "FFE Light",
        bg: Color::Rgb(232, 236, 240),
        fg: Color::Rgb(30, 35, 43),
        accent: Color::Rgb(42, 157, 132),
        muted: Color::Rgb(74, 80, 96),
        border: Color::Rgb(201, 205, 214),
        highlight_bg: Color::Rgb(221, 225, 232),
        highlight_fg: Color::Rgb(192, 121, 32),
        key_bg: Color::Rgb(201, 205, 214),
        key_fg: Color::Rgb(30, 35, 43),
        title: Color::Rgb(192, 121, 32),
        panel_bg: Color::Rgb(245, 247, 250),
        heading_major: Color::Rgb(160, 100, 200),
        heading_chapter: Color::Rgb(26, 138, 110),
        heading_section: Color::Rgb(42, 157, 132),
        heading_article: Color::Rgb(192, 121, 32),
        bookmark: Color::Rgb(192, 121, 32),
        category: Color::Rgb(26, 138, 110),
        department: Color::Rgb(42, 157, 132),
        search: Color::Rgb(192, 121, 32),
        error: Color::Rgb(201, 67, 78),
    },
];

/// Look up a theme index by name (case-sensitive). Returns 0 (Default)
/// if no theme matches.
#[must_use]
#[allow(dead_code)]
pub fn theme_index_by_name(name: &str) -> usize {
    THEMES.iter().position(|t| t.name == name).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_count_matches_expected() {
        assert_eq!(THEMES.len(), 14);
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
}
