use super::{App, InputMode, Popup, View};
use legal_ko_core::AGENTS;
use legal_ko_core::models;
use tracing::warn;

impl App {
    /// Apply search + category + department + bookmarks filters.
    ///
    /// When Meilisearch ranked results are available for the current query,
    /// they are used for ordering and filtering. Otherwise falls back to naive
    /// substring matching on the title.
    pub fn apply_filters(&mut self) {
        let query = &self.search_query;
        let use_meili = !query.is_empty()
            && self
                .meili_search_query
                .as_deref()
                .is_some_and(|q| q == query);

        if use_meili {
            // Build index lookup: id → position in all_laws
            let id_to_idx: std::collections::HashMap<&str, usize> = self
                .all_laws
                .iter()
                .enumerate()
                .map(|(i, e)| (e.id.as_str(), i))
                .collect();

            let meili_ids = self.meili_search_ids.as_deref().unwrap_or(&[]);

            self.filtered_indices = meili_ids
                .iter()
                .filter_map(|id| id_to_idx.get(id.as_str()).copied())
                .filter(|&i| {
                    let entry = &self.all_laws[i];
                    self.passes_non_search_filters(entry)
                })
                .collect();
        } else {
            let query_lower = query.to_lowercase();

            self.filtered_indices = self
                .all_laws
                .iter()
                .enumerate()
                .filter(|(_, entry)| {
                    // Search filter
                    if !query_lower.is_empty() && !entry.title.to_lowercase().contains(&query_lower)
                    {
                        return false;
                    }
                    self.passes_non_search_filters(entry)
                })
                .map(|(i, _)| i)
                .collect();

            // Dispatch async Meilisearch query if ready and query is non-empty
            if !query.is_empty() && self.meili_ready {
                self.dispatch_meili_search();
            }
        }

        // Clamp selection
        if self.filtered_indices.is_empty() {
            self.list_selected = 0;
        } else if self.list_selected >= self.filtered_indices.len() {
            self.list_selected = self.filtered_indices.len().saturating_sub(1);
        }
    }

    // ── Bookmarks ─────────────────────────────────────────────

    pub fn toggle_bookmark(&mut self) {
        let id = match self.view {
            View::List => self.selected_entry().map(|e| e.id.clone()),
            View::Detail => self.detail.as_ref().map(|d| d.entry.id.clone()),
            View::Loading => None,
        };

        if let Some(id) = id {
            let added = self.bookmarks.toggle(&id);
            let bookmarks_snapshot = self.bookmarks.clone();
            tokio::task::spawn_blocking(move || {
                if let Err(e) = bookmarks_snapshot.save() {
                    warn!(error = %e, "Failed to save bookmarks");
                }
            });
            self.status_message = Some(if added {
                "Bookmarked".to_string()
            } else {
                "Bookmark removed".to_string()
            });
        }
    }

    pub fn toggle_bookmarks_only(&mut self) {
        self.bookmarks_only = !self.bookmarks_only;
        self.apply_filters();
        self.status_message = Some(if self.bookmarks_only {
            "Showing bookmarks only".to_string()
        } else {
            "Showing all laws".to_string()
        });
    }

    // ── Search ────────────────────────────────────────────────

    pub fn start_search(&mut self) {
        self.input_mode = InputMode::Search;
    }

    pub fn search_push_char(&mut self, c: char) {
        self.search_query.push(c);
        self.apply_filters();
    }

    pub fn search_pop_char(&mut self) {
        self.search_query.pop();
        self.apply_filters();
    }

    pub fn finish_search(&mut self) {
        self.input_mode = InputMode::Normal;
    }

    pub fn clear_search(&mut self) {
        self.search_query.clear();
        self.input_mode = InputMode::Normal;
        self.apply_filters();
    }

    // ── Filter popups ─────────────────────────────────────────

    pub fn open_category_filter(&mut self) {
        self.popup = Popup::CategoryFilter;
        self.popup_selected = 0;
    }

    pub fn open_department_filter(&mut self) {
        self.popup = Popup::DepartmentFilter;
        self.popup_selected = 0;
    }

    pub fn open_article_list(&mut self) {
        if !self.detail_articles.is_empty() {
            self.popup = Popup::ArticleList;
            self.popup_selected = 0;
        }
    }

    /// Open the AI agent picker popup.
    ///
    /// If no agents are installed, shows a status message instead.
    /// If only one agent is installed, opens it directly (no popup).
    /// Pre-selects the last-used agent when available.
    pub fn open_agent_picker(&mut self) {
        if self.installed_agents.is_empty() {
            self.status_message = Some("No AI agents installed".to_string());
            return;
        }

        // Only one agent installed — skip the popup, open directly.
        if self.installed_agents.len() == 1 {
            let agent = self.installed_agents[0];
            self.open_agent_split(agent);
            return;
        }

        self.popup = Popup::AgentPicker;

        // Pre-select the last-used agent if it's in the installed list.
        self.popup_selected = self
            .last_agent_index
            .and_then(|idx| {
                let agent = &AGENTS[idx];
                self.installed_agents
                    .iter()
                    .position(|a| a.name == agent.name)
            })
            .unwrap_or(0);
    }

    pub fn close_popup(&mut self) {
        self.popup = Popup::None;
    }

    pub fn popup_move_down(&mut self) {
        let max = self.popup_items_count();
        if max > 0 && self.popup_selected < max.saturating_sub(1) {
            self.popup_selected += 1;
        }
    }

    pub fn popup_move_up(&mut self) {
        self.popup_selected = self.popup_selected.saturating_sub(1);
    }

    pub fn popup_select(&mut self) {
        match self.popup {
            Popup::CategoryFilter => {
                if self.popup_selected == 0 {
                    self.category_filter = None;
                } else {
                    self.category_filter = self
                        .categories
                        .get(self.popup_selected.saturating_sub(1))
                        .cloned();
                }
                self.apply_filters();
                self.close_popup();
            }
            Popup::DepartmentFilter => {
                if self.popup_selected == 0 {
                    self.department_filter = None;
                } else {
                    self.department_filter = self
                        .departments
                        .get(self.popup_selected.saturating_sub(1))
                        .cloned();
                }
                self.apply_filters();
                self.close_popup();
            }
            Popup::ArticleList => {
                self.jump_to_article(self.popup_selected);
                self.close_popup();
            }
            Popup::AgentPicker => {
                if let Some(&agent) = self.installed_agents.get(self.popup_selected) {
                    self.close_popup();
                    self.open_agent_split(agent);
                }
            }
            _ => {}
        }
    }

    // ── Sort ───────────────────────────────────────────────────

    pub fn toggle_sort(&mut self) {
        self.sort_order = self.sort_order.next();
        models::sort_entries(&mut self.all_laws, self.sort_order);
        self.apply_filters();
        self.status_message = Some(format!("Sort: {}", self.sort_order.label()));
    }

    fn popup_items_count(&self) -> usize {
        match self.popup {
            Popup::CategoryFilter => self.categories.len() + 1,
            Popup::DepartmentFilter => self.departments.len() + 1,
            Popup::ArticleList => self.detail_articles.len(),
            Popup::AgentPicker => self.installed_agents.len(),
            _ => 0,
        }
    }
}
