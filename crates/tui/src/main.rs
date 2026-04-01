mod app;
mod parser;
mod theme;
mod ui;

use std::io::BufWriter;
use std::os::unix::io::FromRawFd;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use tracing::info;

use app::{App, InputMode, Popup, View};

/// Terminal writer type: a buffered File wrapping a dup'd terminal fd.
type TermWriter = BufWriter<std::fs::File>;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing to file so it doesn't pollute the TUI
    let log_file = dirs::cache_dir()
        .unwrap_or_else(|| "/tmp".into())
        .join("legal-ko")
        .join("legal-ko.log");
    if let Some(parent) = log_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file)?;

    tracing_subscriber::fmt()
        .with_writer(file)
        .with_env_filter("legal_ko=debug")
        .init();

    info!("Starting legal-ko");

    // ── Permanent stdout/stderr suppression ──────────────────────
    //
    // ONNX Runtime (used by vibe-rust for TTS) spawns C++ background threads
    // that print to stdout/stderr.  A temporary dup2 redirect is not enough
    // because those threads can outlive the redirect.
    //
    // Strategy: save a private copy of the terminal fd, permanently point
    // fd 1/2 at /dev/null, and have ratatui write through the private copy.
    let (tty_fd, stdout_backup, stderr_backup) = unsafe {
        let tty = libc::dup(libc::STDOUT_FILENO);
        let out_bak = libc::dup(libc::STDOUT_FILENO);
        let err_bak = libc::dup(libc::STDERR_FILENO);
        assert!(tty >= 0, "failed to dup stdout for terminal writer");

        // Redirect stdout/stderr → /dev/null
        if let Ok(devnull) = std::fs::OpenOptions::new().write(true).open("/dev/null") {
            use std::os::unix::io::AsRawFd;
            let null_fd = devnull.as_raw_fd();
            libc::dup2(null_fd, libc::STDOUT_FILENO);
            libc::dup2(null_fd, libc::STDERR_FILENO);
            // devnull dropped — the dup2'd fds remain open
        }

        (tty, out_bak, err_bak)
    };

    // Wrap the dup'd fd as a BufWriter<File> for ratatui
    let tty_file = unsafe { std::fs::File::from_raw_fd(tty_fd) };
    let mut tty_write = BufWriter::new(tty_file);

    // Setup terminal via the private fd
    enable_raw_mode()?;
    execute!(tty_write, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(tty_write);
    let mut terminal = Terminal::new(backend)?;

    // Run app
    let result = run_app(&mut terminal).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    drop(terminal);

    // Restore stdout/stderr so post-exit error messages are visible
    unsafe {
        if stdout_backup >= 0 {
            libc::dup2(stdout_backup, libc::STDOUT_FILENO);
            libc::close(stdout_backup);
        }
        if stderr_backup >= 0 {
            libc::dup2(stderr_backup, libc::STDERR_FILENO);
            libc::close(stderr_backup);
        }
    }

    if let Err(ref e) = result {
        eprintln!("Error: {e:#}");
    }

    Ok(())
}

async fn run_app(terminal: &mut Terminal<CrosstermBackend<TermWriter>>) -> Result<()> {
    let mut app = App::new();
    app.start_loading();

    loop {
        // Draw
        terminal.draw(|f| ui::render(f, &app))?;

        // Check for background messages (non-blocking)
        while let Ok(msg) = app.msg_rx.try_recv() {
            app.handle_message(msg);
        }

        // Check if TTS playback finished
        app.check_tts_playback();

        // Advance animation tick
        app.tick = app.tick.wrapping_add(1);

        // Poll for input events with timeout
        if event::poll(Duration::from_millis(50))?
            && let Event::Key(key) = event::read()?
        {
            handle_key_event(&mut app, key, terminal.size()?.height as usize);
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

fn handle_key_event(app: &mut App, key: KeyEvent, terminal_height: usize) {
    // Popups have priority
    if app.popup != Popup::None {
        handle_popup_key(app, key);
        return;
    }

    // Search mode has priority
    if app.input_mode == InputMode::Search {
        handle_search_key(app, key);
        return;
    }

    match app.view {
        View::Loading => handle_loading_key(app, key),
        View::List => handle_list_key(app, key, terminal_height),
        View::Detail => handle_detail_key(app, key, terminal_height),
    }
}

fn handle_loading_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
        _ => {}
    }
}

fn handle_list_key(app: &mut App, key: KeyEvent, terminal_height: usize) {
    let page_size = terminal_height.saturating_sub(4); // account for bars

    match key.code {
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Char('j') | KeyCode::Down => app.list_move_down(),
        KeyCode::Char('k') | KeyCode::Up => app.list_move_up(),
        KeyCode::Char('g') | KeyCode::Home => app.list_top(),
        KeyCode::Char('G') | KeyCode::End => app.list_bottom(),
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.list_page_down(page_size)
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.list_page_up(page_size)
        }
        KeyCode::PageDown => app.list_page_down(page_size),
        KeyCode::PageUp => app.list_page_up(page_size),
        KeyCode::Enter => app.open_selected(),
        KeyCode::Char('/') => app.start_search(),
        KeyCode::Char('c') => app.open_category_filter(),
        KeyCode::Char('d') => app.open_department_filter(),
        KeyCode::Char('B') => app.toggle_bookmark(),
        KeyCode::Char('b') => app.toggle_bookmarks_only(),
        KeyCode::Char('t') => app.next_theme(),
        KeyCode::Char('T') => app.toggle_tts_profile(),
        KeyCode::Char('?') => app.popup = Popup::Help,
        KeyCode::Esc => {
            if !app.search_query.is_empty() {
                app.clear_search();
            } else {
                app.go_back();
            }
        }
        _ => {}
    }
}

fn handle_detail_key(app: &mut App, key: KeyEvent, terminal_height: usize) {
    let page_size = terminal_height.saturating_sub(2);

    match key.code {
        KeyCode::Char('q') => app.go_back(),
        KeyCode::Esc => app.go_back(),
        KeyCode::Char('j') | KeyCode::Down => app.detail_scroll_down(1),
        KeyCode::Char('k') | KeyCode::Up => app.detail_scroll_up(1),
        KeyCode::Char('g') | KeyCode::Home => app.detail_top(),
        KeyCode::Char('G') | KeyCode::End => app.detail_bottom(),
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.detail_scroll_down(page_size)
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.detail_scroll_up(page_size)
        }
        KeyCode::PageDown => app.detail_scroll_down(page_size),
        KeyCode::PageUp => app.detail_scroll_up(page_size),
        KeyCode::Char('n') => app.next_article(),
        KeyCode::Char('p') => app.prev_article(),
        KeyCode::Char('a') => app.open_article_list(),
        KeyCode::Char('B') => app.toggle_bookmark(),
        KeyCode::Char('t') => app.next_theme(),
        KeyCode::Char('T') => app.toggle_tts_profile(),
        KeyCode::Char('r') => app.speak_article(),
        KeyCode::Char('R') => app.speak_full(),
        KeyCode::Char('s') => app.stop_tts(),
        KeyCode::Char('?') => app.popup = Popup::Help,
        _ => {}
    }
}

fn handle_search_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => app.clear_search(),
        KeyCode::Enter => app.finish_search(),
        KeyCode::Backspace => app.search_pop_char(),
        KeyCode::Char(c) => app.search_push_char(c),
        _ => {}
    }
}

fn handle_popup_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => app.close_popup(),
        KeyCode::Char('j') | KeyCode::Down => app.popup_move_down(),
        KeyCode::Char('k') | KeyCode::Up => app.popup_move_up(),
        KeyCode::Enter => app.popup_select(),
        _ => {}
    }
}
