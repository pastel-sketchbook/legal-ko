mod app;
mod parser;
mod theme;
mod ui;

use std::io::BufWriter;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use tracing::info;

use app::{App, InputMode, Popup, SuspendRequest, View};

/// Terminal writer type when TTS is enabled: a buffered File wrapping a dup'd terminal fd.
/// This lets us permanently redirect stdout/stderr to /dev/null (to suppress ONNX
/// Runtime noise) while ratatui writes through a private copy of the terminal fd.
#[cfg(feature = "tts")]
type TermWriter = BufWriter<std::fs::File>;

/// Terminal writer type when TTS is disabled: standard buffered stdout.
#[cfg(not(feature = "tts"))]
type TermWriter = BufWriter<std::io::Stdout>;

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
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("legal_ko=debug")),
        )
        .init();

    info!("Starting legal-ko");

    // ── TTS: Permanent stdout/stderr suppression ────────────────
    //
    // ONNX Runtime (used by vibe-rust for TTS) spawns C++ background threads
    // that print to stdout/stderr.  A temporary dup2 redirect is not enough
    // because those threads can outlive the redirect.
    //
    // Strategy: save a private copy of the terminal fd, permanently point
    // fd 1/2 at /dev/null, and have ratatui write through the private copy.
    #[cfg(feature = "tts")]
    // SAFETY: `dup` and `dup2` are well-defined POSIX calls on valid standard
    // file descriptors.  We save private copies of stdout/stderr before
    // redirecting them to /dev/null.  The saved fds are restored in the cleanup
    // block at the end of `main`.  All raw fds obtained here are either
    // consumed by `File::from_raw_fd` (tty_fd) or explicitly closed after
    // restoration (stdout_backup, stderr_backup).
    let (tty_fd, stdout_backup, stderr_backup) = unsafe {
        use std::os::unix::io::AsRawFd;
        let tty = libc::dup(libc::STDOUT_FILENO);
        let out_bak = libc::dup(libc::STDOUT_FILENO);
        let err_bak = libc::dup(libc::STDERR_FILENO);
        anyhow::ensure!(tty >= 0, "failed to dup stdout for terminal writer");

        // Redirect stdout/stderr → /dev/null
        if let Ok(devnull) = std::fs::OpenOptions::new().write(true).open("/dev/null") {
            let null_fd = devnull.as_raw_fd();
            libc::dup2(null_fd, libc::STDOUT_FILENO);
            libc::dup2(null_fd, libc::STDERR_FILENO);
            // devnull dropped — the dup2'd fds remain open
        }

        (tty, out_bak, err_bak)
    };

    // Build the terminal writer.
    #[cfg(feature = "tts")]
    let tty_write = {
        use std::os::unix::io::FromRawFd;
        // SAFETY: `tty_fd` is a valid file descriptor obtained from `libc::dup` above.
        // `from_raw_fd` takes ownership; we never use `tty_fd` again after this point.
        let tty_file = unsafe { std::fs::File::from_raw_fd(tty_fd) };
        BufWriter::new(tty_file)
    };

    #[cfg(not(feature = "tts"))]
    let tty_write = BufWriter::new(std::io::stdout());

    // Setup terminal
    enable_raw_mode()?;
    install_panic_hook();
    let mut tty_write = tty_write;
    execute!(tty_write, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(tty_write);
    let mut terminal = Terminal::new(backend)?;

    // Run app
    #[cfg(feature = "tts")]
    let result = run_app(&mut terminal, (stdout_backup, stderr_backup)).await;
    #[cfg(not(feature = "tts"))]
    let result = run_app(&mut terminal, (-1, -1)).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    drop(terminal);

    // TTS: Restore stdout/stderr from backup fds
    #[cfg(feature = "tts")]
    // SAFETY: `stdout_backup` and `stderr_backup` are valid fds obtained from
    // `libc::dup` at the start of `main`.  We restore the original stdout/stderr
    // via `dup2` and then close the backup fds to avoid leaking them.
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

#[allow(clippy::unused_async)]
async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<TermWriter>>,
    fd_backup: (i32, i32),
) -> Result<()> {
    let mut app = App::new();
    app.start_loading();

    loop {
        // Draw
        terminal.draw(|f| ui::render(f, &app))?;

        // Check for background messages (non-blocking)
        while let Ok(msg) = app.msg_rx.try_recv() {
            app.handle_message(msg);
        }

        // Check for TTS playback finished
        #[cfg(feature = "tts")]
        app.check_tts_playback();

        // Check for external commands (e.g. navigate from OpenCode)
        if app.poll_command() {
            app.sync_context();
        }

        // Advance animation tick
        app.tick = app.tick.wrapping_add(1);

        // Poll for input events with timeout
        if event::poll(Duration::from_millis(50))?
            && let Event::Key(key) = event::read()?
        {
            handle_key_event(&mut app, key, terminal.size()?.height as usize);
        }

        // ── Suspend-and-resume fallback ──────────────────────
        //
        // When the user opens an AI agent in a terminal without split
        // support, open_agent_split() sets suspend_agent instead of
        // spawning a split pane.  We leave the alternate screen, run
        // the agent as a blocking child, and resume the TUI on exit.
        if let Some(req) = app.suspend_agent.take() {
            suspend_and_run(terminal, &req, fd_backup)?;
            app.status_message = Some(format!("{} exited — TUI resumed", req.agent_name));
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

/// Temporarily leave the TUI, run an agent binary in the foreground, then resume.
fn suspend_and_run(
    terminal: &mut Terminal<CrosstermBackend<TermWriter>>,
    req: &SuspendRequest,
    _fd_backup: (i32, i32),
) -> Result<()> {
    // Leave TUI mode
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    // TTS: temporarily restore real stdout/stderr so the agent can use them
    #[cfg(feature = "tts")]
    // SAFETY: _fd_backup fds are valid copies saved at the start of main().
    // We temporarily restore them so the child process inherits usable
    // stdout/stderr, then redirect back to /dev/null after the child exits.
    unsafe {
        if _fd_backup.0 >= 0 {
            libc::dup2(_fd_backup.0, libc::STDOUT_FILENO);
        }
        if _fd_backup.1 >= 0 {
            libc::dup2(_fd_backup.1, libc::STDERR_FILENO);
        }
    }

    // Run the agent as a blocking child process
    let status = std::process::Command::new(&req.binary_path).status();
    match &status {
        Ok(s) => info!(agent = req.agent_name, exit_code = ?s.code(), "Agent exited"),
        Err(e) => info!(agent = req.agent_name, error = %e, "Failed to run agent"),
    }

    // TTS: redirect stdout/stderr back to /dev/null
    #[cfg(feature = "tts")]
    // SAFETY: We re-redirect stdout/stderr to /dev/null so ONNX Runtime
    // background threads don't pollute the restored TUI.
    unsafe {
        if let Ok(devnull) = std::fs::OpenOptions::new().write(true).open("/dev/null") {
            use std::os::unix::io::AsRawFd;
            let null_fd = devnull.as_raw_fd();
            libc::dup2(null_fd, libc::STDOUT_FILENO);
            libc::dup2(null_fd, libc::STDERR_FILENO);
        }
    }

    // Re-enter TUI mode
    enable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        EnterAlternateScreen,
        EnableMouseCapture
    )?;
    terminal.hide_cursor()?;
    terminal.clear()?;

    Ok(())
}

fn handle_key_event(app: &mut App, key: KeyEvent, terminal_height: usize) {
    // Ctrl+C always quits immediately, regardless of view or input mode.
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        app.should_quit = true;
        return;
    }

    // Popups have priority
    if app.popup != Popup::None {
        handle_popup_key(app, key);
        app.sync_context();
        return;
    }

    // Search mode has priority
    if app.input_mode == InputMode::Search {
        handle_search_key(app, key);
        app.sync_context();
        return;
    }

    match app.view {
        View::Loading => handle_loading_key(app, key),
        View::List => handle_list_key(app, key, terminal_height),
        View::Detail => handle_detail_key(app, key, terminal_height),
    }
    app.sync_context();
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
            app.list_page_down(page_size);
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.list_page_up(page_size);
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
        #[cfg(feature = "tts")]
        KeyCode::Char('T') => app.toggle_tts_profile(),
        KeyCode::Char('o') => app.open_agent_picker(),
        KeyCode::Char('?') => app.popup = Popup::Help,
        KeyCode::Esc => {
            if app.search_query.is_empty() {
                app.go_back();
            } else {
                app.clear_search();
            }
        }
        _ => {}
    }
}

fn handle_detail_key(app: &mut App, key: KeyEvent, terminal_height: usize) {
    let page_size = terminal_height.saturating_sub(2);

    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => app.go_back(),
        KeyCode::Char('j') | KeyCode::Down => app.detail_scroll_down(1),
        KeyCode::Char('k') | KeyCode::Up => app.detail_scroll_up(1),
        KeyCode::Char('g') | KeyCode::Home => app.detail_top(),
        KeyCode::Char('G') | KeyCode::End => app.detail_bottom(),
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.detail_scroll_down(page_size);
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.detail_scroll_up(page_size);
        }
        KeyCode::PageDown => app.detail_scroll_down(page_size),
        KeyCode::PageUp => app.detail_scroll_up(page_size),
        KeyCode::Char('n') => app.next_article(),
        KeyCode::Char('p') => app.prev_article(),
        KeyCode::Char('a') => app.open_article_list(),
        KeyCode::Char('B') => app.toggle_bookmark(),
        KeyCode::Char('t') => app.next_theme(),
        #[cfg(feature = "tts")]
        KeyCode::Char('T') => app.toggle_tts_profile(),
        #[cfg(feature = "tts")]
        KeyCode::Char('r') => app.speak_article(),
        #[cfg(feature = "tts")]
        KeyCode::Char('R') => app.speak_full(),
        #[cfg(feature = "tts")]
        KeyCode::Char('s') => app.stop_tts(),
        KeyCode::Char('E') => app.export_law(),
        KeyCode::Char('o') => app.open_agent_picker(),
        KeyCode::Char('?') => app.popup = Popup::Help,
        _ => {}
    }
}

fn handle_search_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => app.clear_search(),
        KeyCode::Enter => app.finish_search(),
        KeyCode::Backspace => app.search_pop_char(),
        KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.list_move_down();
        }
        KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.list_move_up();
        }
        KeyCode::Down => app.list_move_down(),
        KeyCode::Up => app.list_move_up(),
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

/// Install a panic hook that restores the terminal before printing the panic.
///
/// Without this, a panic while raw mode is active leaves the terminal in a
/// corrupted state (no echo, no line buffering, alternate screen still active).
/// Writing directly to `/dev/tty` ensures the escape sequences reach the real
/// terminal even when stdout/stderr are redirected (e.g. TTS `/dev/null` suppression).
fn install_panic_hook() {
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        // Best-effort terminal restoration — ignore errors since we're already panicking.
        let _ = disable_raw_mode();
        if let Ok(mut tty) = std::fs::OpenOptions::new().write(true).open("/dev/tty") {
            let _ = execute!(
                tty,
                crossterm::cursor::Show,
                LeaveAlternateScreen,
                DisableMouseCapture
            );
        }
        original_hook(panic_info);
    }));
}
