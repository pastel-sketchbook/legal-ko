use super::{App, View};

impl App {
    // ── List navigation ───────────────────────────────────────

    pub fn list_move_down(&mut self) {
        if !self.filtered_indices.is_empty()
            && self.list_selected < self.filtered_indices.len().saturating_sub(1)
        {
            self.list_selected += 1;
        }
    }

    pub fn list_move_up(&mut self) {
        self.list_selected = self.list_selected.saturating_sub(1);
    }

    pub fn list_page_down(&mut self, page_size: usize) {
        if self.filtered_indices.is_empty() {
            return;
        }
        self.list_selected =
            (self.list_selected + page_size).min(self.filtered_indices.len().saturating_sub(1));
    }

    pub fn list_page_up(&mut self, page_size: usize) {
        self.list_selected = self.list_selected.saturating_sub(page_size);
    }

    pub fn list_top(&mut self) {
        self.list_selected = 0;
    }

    pub fn list_bottom(&mut self) {
        if !self.filtered_indices.is_empty() {
            self.list_selected = self.filtered_indices.len().saturating_sub(1);
        }
    }

    // ── Detail navigation ─────────────────────────────────────

    pub fn detail_scroll_down(&mut self, amount: usize) {
        self.detail_scroll =
            (self.detail_scroll + amount).min(self.detail_lines_count.saturating_sub(1));
    }

    pub fn detail_scroll_up(&mut self, amount: usize) {
        self.detail_scroll = self.detail_scroll.saturating_sub(amount);
    }

    pub fn detail_top(&mut self) {
        self.detail_scroll = 0;
    }

    pub fn detail_bottom(&mut self) {
        self.detail_scroll = self.detail_lines_count.saturating_sub(1);
    }

    pub fn next_article(&mut self) {
        if self.detail_articles.is_empty() {
            return;
        }
        for art in &self.detail_articles {
            if art.line_index > self.detail_scroll {
                self.detail_scroll = art.line_index;
                return;
            }
        }
        self.detail_scroll = self.detail_articles[0].line_index;
    }

    pub fn prev_article(&mut self) {
        if self.detail_articles.is_empty() {
            return;
        }
        for art in self.detail_articles.iter().rev() {
            if art.line_index < self.detail_scroll {
                self.detail_scroll = art.line_index;
                return;
            }
        }
        // Invariant: `detail_articles` is non-empty (checked by early return at line 75).
        self.detail_scroll = self
            .detail_articles
            .last()
            .expect("detail_articles is non-empty (checked above)")
            .line_index;
    }

    /// Jump to a specific article by index in the articles list
    pub fn jump_to_article(&mut self, article_index: usize) {
        if let Some(art) = self.detail_articles.get(article_index) {
            self.detail_scroll = art.line_index;
        }
    }

    // ── Precedent list navigation ─────────────────────────────

    pub fn precedent_list_move_down(&mut self) {
        let count = self.precedent_visible_count();
        let cursor = self.precedent_cursor();
        if count > 0 && cursor < count.saturating_sub(1) {
            self.set_precedent_cursor(cursor + 1);
        }
    }

    pub fn precedent_list_move_up(&mut self) {
        let cursor = self.precedent_cursor();
        self.set_precedent_cursor(cursor.saturating_sub(1));
    }

    pub fn precedent_list_page_down(&mut self, page_size: usize) {
        let count = self.precedent_visible_count();
        if count == 0 {
            return;
        }
        let cursor = self.precedent_cursor();
        self.set_precedent_cursor((cursor + page_size).min(count.saturating_sub(1)));
    }

    pub fn precedent_list_page_up(&mut self, page_size: usize) {
        let cursor = self.precedent_cursor();
        self.set_precedent_cursor(cursor.saturating_sub(page_size));
    }

    pub fn precedent_list_top(&mut self) {
        self.set_precedent_cursor(0);
    }

    pub fn precedent_list_bottom(&mut self) {
        let count = self.precedent_visible_count();
        if count > 0 {
            self.set_precedent_cursor(count.saturating_sub(1));
        }
    }

    // ── Precedent detail navigation ───────────────────────────

    pub fn precedent_detail_scroll_down(&mut self, amount: usize) {
        self.precedent_detail_scroll = (self.precedent_detail_scroll + amount)
            .min(self.precedent_detail_lines_count.saturating_sub(1));
    }

    pub fn precedent_detail_scroll_up(&mut self, amount: usize) {
        self.precedent_detail_scroll = self.precedent_detail_scroll.saturating_sub(amount);
    }

    pub fn precedent_detail_top(&mut self) {
        self.precedent_detail_scroll = 0;
    }

    pub fn precedent_detail_bottom(&mut self) {
        self.precedent_detail_scroll = self.precedent_detail_lines_count.saturating_sub(1);
    }

    pub fn next_section(&mut self) {
        if self.precedent_detail_sections.is_empty() {
            return;
        }
        for sec in &self.precedent_detail_sections {
            if sec.line_index > self.precedent_detail_scroll {
                self.precedent_detail_scroll = sec.line_index;
                return;
            }
        }
        self.precedent_detail_scroll = self.precedent_detail_sections[0].line_index;
    }

    pub fn prev_section(&mut self) {
        if self.precedent_detail_sections.is_empty() {
            return;
        }
        for sec in self.precedent_detail_sections.iter().rev() {
            if sec.line_index < self.precedent_detail_scroll {
                self.precedent_detail_scroll = sec.line_index;
                return;
            }
        }
        // Invariant: `precedent_detail_sections` is non-empty (checked by early return at line 170).
        self.precedent_detail_scroll = self
            .precedent_detail_sections
            .last()
            .expect("precedent_detail_sections is non-empty (checked above)")
            .line_index;
    }

    /// Jump to a specific section by index
    pub fn jump_to_section(&mut self, section_index: usize) {
        if let Some(sec) = self.precedent_detail_sections.get(section_index) {
            self.precedent_detail_scroll = sec.line_index;
        }
    }

    // ── Back navigation ───────────────────────────────────────

    pub fn go_back(&mut self) {
        match self.view {
            View::Detail => {
                #[cfg(feature = "tts")]
                self.stop_tts();
                self.view = View::List;
                self.detail = None;
                self.detail_scroll = 0;
                self.detail_rendered_lines.clear();
            }
            View::PrecedentDetail => {
                self.view = View::PrecedentList;
                self.precedent_detail = None;
                self.precedent_detail_scroll = 0;
                self.precedent_detail_rendered_lines.clear();
            }
            View::List | View::PrecedentList | View::Loading => {
                self.should_quit = true;
            }
        }
    }
}
