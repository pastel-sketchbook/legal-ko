use super::{App, InputMode, Message, Popup, View};
use crate::hangul;
use legal_ko_core::AGENTS;
use legal_ko_core::models;
use legal_ko_core::parser;
use tracing::{info, warn};

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
            // NFC-normalize so NFD source data (e.g. Korean text from macOS
            // file systems) still matches an IME-produced NFC query.
            let query_norm = hangul::nfc(&query.to_lowercase());
            // Also try interpreting the query as English-keyboard Hangul (영타→한타)
            let hangul_query = hangul::eng_to_hangul(query).map(|h| hangul::nfc(&h.to_lowercase()));

            self.filtered_indices = self
                .all_laws
                .iter()
                .enumerate()
                .filter(|(_, entry)| {
                    // Search filter
                    if !query_norm.is_empty() {
                        let title = hangul::nfc(&entry.title.to_lowercase());
                        let matches = title.contains(&query_norm)
                            || hangul_query
                                .as_ref()
                                .is_some_and(|hq| title.contains(hq.as_str()));
                        if !matches {
                            return false;
                        }
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

    /// Apply search + case type + court filters for precedents.
    pub fn apply_precedent_filters(&mut self) {
        // Cancel any in-flight person search when filters change.
        self.person_search_active = false;
        self.person_search_results.clear();

        // NFC-normalize so NFD source data (e.g. Korean text from macOS
        // file systems) still matches an IME-produced NFC query.
        let query_norm = hangul::nfc(&self.precedent_search_query.to_lowercase());
        let hangul_query = hangul::eng_to_hangul(&self.precedent_search_query)
            .map(|h| hangul::nfc(&h.to_lowercase()));

        self.precedent_filtered_indices = self
            .all_precedents
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                // Search filter (case name or case number)
                if !query_norm.is_empty() {
                    let name = hangul::nfc(&entry.case_name.to_lowercase());
                    let number = hangul::nfc(&entry.case_number.to_lowercase());
                    let matches = name.contains(&query_norm)
                        || number.contains(&query_norm)
                        || hangul_query.as_ref().is_some_and(|hq| {
                            name.contains(hq.as_str()) || number.contains(hq.as_str())
                        });
                    if !matches {
                        return false;
                    }
                }
                // Case type filter
                if let Some(ref ct) = self.precedent_case_type_filter
                    && &entry.case_type != ct
                {
                    return false;
                }
                // Court filter
                if let Some(ref court) = self.precedent_court_filter
                    && &entry.court_name != court
                {
                    return false;
                }
                true
            })
            .map(|(i, _)| i)
            .collect();

        // Clamp selection
        if self.precedent_filtered_indices.is_empty() {
            self.precedent_list_selected = 0;
        } else if self.precedent_list_selected >= self.precedent_filtered_indices.len() {
            self.precedent_list_selected = self.precedent_filtered_indices.len().saturating_sub(1);
        }

        // If the query looks like a Korean name, trigger 법조인 (legal
        // professional) search. This runs alongside the normal metadata filter
        // — results from both are available to the renderer.
        if !query_norm.is_empty() && parser::is_korean_name(&self.precedent_search_query) {
            self.start_person_search();
        }
    }

    /// Spawn a background task that searches for a 법조인 name using the
    /// cached person index. If no index exists, builds one concurrently
    /// first (sending progress messages to the UI).
    fn start_person_search(&mut self) {
        self.person_search_seq = self.person_search_seq.wrapping_add(1);
        self.person_search_active = true;
        self.person_search_results.clear();

        let seq = self.person_search_seq;
        let name = self.precedent_search_query.clone();
        let entries: Vec<_> = self.all_precedents.clone();
        let tx = self.msg_tx.clone();
        let http = self.client.clone();

        info!(seq, name = %name, entries = entries.len(), "Starting person search (indexed)");

        tokio::spawn(async move {
            let results = legal_ko_core::person_index::search_persons(
                &http,
                &name,
                None,
                &entries,
                |_scanned, _total| {
                    // Progress is implicit via the animated indicator in the UI
                },
            )
            .await;

            for r in results {
                let _ = tx.send(Message::PersonSearchHit {
                    seq,
                    entry: r.entry,
                });
            }
            let _ = tx.send(Message::PersonSearchDone { seq });
        });
    }

    // ── Bookmarks ─────────────────────────────────────────────

    pub fn toggle_bookmark(&mut self) {
        let id = match self.view {
            View::List => self.selected_entry().map(|e| e.id.clone()),
            View::Detail => self.detail.as_ref().map(|d| d.entry.id.clone()),
            View::Loading
            | View::PrecedentList
            | View::PrecedentDetail
            | View::AdmruleList
            | View::AdmruleDetail
            | View::OrdinanceList
            | View::OrdinanceDetail => None,
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
        match self.view {
            View::PrecedentList => {
                self.precedent_search_query.push(c);
                self.apply_precedent_filters();
            }
            View::AdmruleList => {
                self.admrule_search_query.push(c);
                self.apply_admrule_filters();
            }
            View::OrdinanceList => {
                self.ordinance_search_query.push(c);
                self.apply_ordinance_filters();
            }
            _ => {
                self.search_query.push(c);
                self.apply_filters();
            }
        }
    }

    pub fn search_pop_char(&mut self) {
        match self.view {
            View::PrecedentList => {
                hangul::pop_jamo(&mut self.precedent_search_query);
                self.apply_precedent_filters();
            }
            View::AdmruleList => {
                hangul::pop_jamo(&mut self.admrule_search_query);
                self.apply_admrule_filters();
            }
            View::OrdinanceList => {
                hangul::pop_jamo(&mut self.ordinance_search_query);
                self.apply_ordinance_filters();
            }
            _ => {
                hangul::pop_jamo(&mut self.search_query);
                self.apply_filters();
            }
        }
    }

    pub fn finish_search(&mut self) {
        self.input_mode = InputMode::Normal;
    }

    pub fn clear_search(&mut self) {
        match self.view {
            View::PrecedentList => {
                self.precedent_search_query.clear();
                self.input_mode = InputMode::Normal;
                self.apply_precedent_filters();
            }
            View::AdmruleList => {
                self.admrule_search_query.clear();
                self.input_mode = InputMode::Normal;
                self.apply_admrule_filters();
            }
            View::OrdinanceList => {
                self.ordinance_search_query.clear();
                self.input_mode = InputMode::Normal;
                self.apply_ordinance_filters();
            }
            _ => {
                self.search_query.clear();
                self.input_mode = InputMode::Normal;
                self.apply_filters();
            }
        }
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

    pub fn open_section_list(&mut self) {
        if !self.precedent_detail_sections.is_empty() {
            self.popup = Popup::SectionList;
            self.popup_selected = 0;
        }
    }

    pub fn open_case_type_filter(&mut self) {
        self.popup = Popup::CaseTypeFilter;
        self.popup_selected = 0;
    }

    pub fn open_court_filter(&mut self) {
        self.popup = Popup::CourtFilter;
        self.popup_selected = 0;
    }

    pub fn open_crossref_list(&mut self) {
        if self.precedent_crossref_matches.is_empty() {
            self.status_message = Some("No 참조조문 found in this precedent".to_string());
        } else {
            self.popup = Popup::CrossRefList;
            self.popup_selected = 0;
        }
    }

    pub fn open_admrule_type_filter(&mut self) {
        self.popup = Popup::AdmruleTypeFilter;
        self.popup_selected = 0;
    }

    pub fn open_admrule_agency_filter(&mut self) {
        self.popup = Popup::AdmruleAgencyFilter;
        self.popup_selected = 0;
    }

    pub fn open_ordinance_type_filter(&mut self) {
        self.popup = Popup::OrdinanceTypeFilter;
        self.popup_selected = 0;
    }

    pub fn open_ordinance_region_filter(&mut self) {
        self.popup = Popup::OrdinanceRegionFilter;
        self.popup_selected = 0;
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
                self.category_filter = self.pick_filter_value(&self.categories.clone());
                self.apply_filters();
                self.close_popup();
            }
            Popup::DepartmentFilter => {
                self.department_filter = self.pick_filter_value(&self.departments.clone());
                self.apply_filters();
                self.close_popup();
            }
            Popup::ArticleList => {
                self.jump_to_article(self.popup_selected);
                self.close_popup();
            }
            Popup::SectionList => {
                self.jump_to_section(self.popup_selected);
                self.close_popup();
            }
            Popup::CaseTypeFilter => {
                self.precedent_case_type_filter =
                    self.pick_filter_value(&self.precedent_case_types.clone());
                self.apply_precedent_filters();
                self.close_popup();
            }
            Popup::CourtFilter => {
                self.precedent_court_filter =
                    self.pick_filter_value(&self.precedent_courts.clone());
                self.apply_precedent_filters();
                self.close_popup();
            }
            Popup::CrossRefList => {
                if let Some(law_match) = self
                    .precedent_crossref_matches
                    .get(self.popup_selected)
                    .cloned()
                {
                    self.close_popup();
                    self.jump_to_crossref_law(&law_match);
                }
            }
            Popup::AgentPicker => {
                if let Some(&agent) = self.installed_agents.get(self.popup_selected) {
                    self.close_popup();
                    self.open_agent_split(agent);
                }
            }
            Popup::AdmruleTypeFilter => {
                self.admrule_type_filter = self.pick_filter_value(&self.admrule_types.clone());
                self.apply_admrule_filters();
                self.close_popup();
            }
            Popup::AdmruleAgencyFilter => {
                self.admrule_agency_filter =
                    self.pick_filter_value(&self.admrule_agencies.clone());
                self.apply_admrule_filters();
                self.close_popup();
            }
            Popup::OrdinanceTypeFilter => {
                self.ordinance_type_filter =
                    self.pick_filter_value(&self.ordinance_types.clone());
                self.apply_ordinance_filters();
                self.close_popup();
            }
            Popup::OrdinanceRegionFilter => {
                self.ordinance_region_filter =
                    self.pick_filter_value(&self.ordinance_regions.clone());
                self.apply_ordinance_filters();
                self.close_popup();
            }
            Popup::ExportFormat => self.handle_export_select(),
            _ => {}
        }
    }

    /// Convert `popup_selected` into an `Option<String>` filter value.
    ///
    /// Index 0 means "all" (returns `None`); any other index picks from the
    /// supplied list (offset by 1).
    fn pick_filter_value(&self, items: &[String]) -> Option<String> {
        if self.popup_selected == 0 {
            None
        } else {
            items.get(self.popup_selected - 1).cloned()
        }
    }

    /// Handle selection inside the `ExportFormat` popup.
    ///
    /// Index 0 → Markdown export, index 1 → PDF export (when `pdf` feature is
    /// enabled). Dispatches to the correct method based on current view.
    fn handle_export_select(&mut self) {
        self.close_popup();
        match self.popup_selected {
            0 => {
                // Markdown export
                match self.view {
                    View::Detail => self.export_law(),
                    View::PrecedentDetail => self.export_precedent(),
                    View::AdmruleDetail => self.export_admrule(),
                    View::OrdinanceDetail => self.export_ordinance(),
                    _ => {}
                }
            }
            #[cfg(feature = "pdf")]
            1 => {
                // PDF export
                match self.view {
                    View::Detail => self.export_law_pdf(),
                    View::PrecedentDetail => self.export_precedent_pdf(),
                    View::AdmruleDetail => self.export_admrule_pdf(),
                    View::OrdinanceDetail => self.export_ordinance_pdf(),
                    _ => {}
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

    pub fn toggle_precedent_sort(&mut self) {
        self.precedent_sort_order = self.precedent_sort_order.next();
        models::sort_precedent_entries(&mut self.all_precedents, self.precedent_sort_order);
        self.apply_precedent_filters();
        self.status_message = Some(format!("Sort: {}", self.precedent_sort_order.label()));
    }

    pub fn toggle_admrule_sort(&mut self) {
        self.admrule_sort_order = self.admrule_sort_order.next();
        models::sort_admrule_entries(&mut self.all_admrules, self.admrule_sort_order);
        self.apply_admrule_filters();
        self.status_message = Some(format!("Sort: {}", self.admrule_sort_order.label()));
    }

    pub fn toggle_ordinance_sort(&mut self) {
        self.ordinance_sort_order = self.ordinance_sort_order.next();
        models::sort_ordinance_entries(&mut self.all_ordinances, self.ordinance_sort_order);
        self.apply_ordinance_filters();
        self.status_message = Some(format!("Sort: {}", self.ordinance_sort_order.label()));
    }

    fn popup_items_count(&self) -> usize {
        match self.popup {
            Popup::CategoryFilter => self.categories.len() + 1,
            Popup::DepartmentFilter => self.departments.len() + 1,
            Popup::ArticleList => self.detail_articles.len(),
            Popup::SectionList => self.precedent_detail_sections.len(),
            Popup::CaseTypeFilter => self.precedent_case_types.len() + 1,
            Popup::CourtFilter => self.precedent_courts.len() + 1,
            Popup::CrossRefList => self.precedent_crossref_matches.len(),
            Popup::AgentPicker => self.installed_agents.len(),
            Popup::AdmruleTypeFilter => self.admrule_types.len() + 1,
            Popup::AdmruleAgencyFilter => self.admrule_agencies.len() + 1,
            Popup::OrdinanceTypeFilter => self.ordinance_types.len() + 1,
            Popup::OrdinanceRegionFilter => self.ordinance_regions.len() + 1,
            Popup::ExportFormat => App::export_format_labels().len(),
            _ => 0,
        }
    }

    // ── Admrule filters ───────────────────────────────────────

    pub fn apply_admrule_filters(&mut self) {
        let query_norm = hangul::nfc(&self.admrule_search_query.to_lowercase());
        let hangul_query = hangul::eng_to_hangul(&self.admrule_search_query)
            .map(|h| hangul::nfc(&h.to_lowercase()));

        self.admrule_filtered_indices = self
            .all_admrules
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                if !query_norm.is_empty() {
                    let title = hangul::nfc(&entry.title.to_lowercase());
                    let matches = title.contains(&query_norm)
                        || hangul_query
                            .as_ref()
                            .is_some_and(|hq| title.contains(hq.as_str()));
                    if !matches {
                        return false;
                    }
                }
                if let Some(ref rt) = self.admrule_type_filter
                    && &entry.rule_type != rt
                {
                    return false;
                }
                if let Some(ref agency) = self.admrule_agency_filter
                    && &entry.agency != agency
                {
                    return false;
                }
                true
            })
            .map(|(i, _)| i)
            .collect();

        if self.admrule_filtered_indices.is_empty() {
            self.admrule_list_selected = 0;
        } else if self.admrule_list_selected >= self.admrule_filtered_indices.len() {
            self.admrule_list_selected = self.admrule_filtered_indices.len().saturating_sub(1);
        }
    }

    // ── Ordinance filters ─────────────────────────────────────

    pub fn apply_ordinance_filters(&mut self) {
        let query_norm = hangul::nfc(&self.ordinance_search_query.to_lowercase());
        let hangul_query = hangul::eng_to_hangul(&self.ordinance_search_query)
            .map(|h| hangul::nfc(&h.to_lowercase()));

        self.ordinance_filtered_indices = self
            .all_ordinances
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                if !query_norm.is_empty() {
                    let title = hangul::nfc(&entry.title.to_lowercase());
                    let matches = title.contains(&query_norm)
                        || hangul_query
                            .as_ref()
                            .is_some_and(|hq| title.contains(hq.as_str()));
                    if !matches {
                        return false;
                    }
                }
                if let Some(ref rt) = self.ordinance_type_filter
                    && &entry.rule_type != rt
                {
                    return false;
                }
                if let Some(ref region) = self.ordinance_region_filter
                    && &entry.region != region
                {
                    return false;
                }
                true
            })
            .map(|(i, _)| i)
            .collect();

        if self.ordinance_filtered_indices.is_empty() {
            self.ordinance_list_selected = 0;
        } else if self.ordinance_list_selected >= self.ordinance_filtered_indices.len() {
            self.ordinance_list_selected = self.ordinance_filtered_indices.len().saturating_sub(1);
        }
    }
}
