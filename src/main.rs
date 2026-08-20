use std::io;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Paragraph, Row, Table, TableState};
use ratatui::{DefaultTerminal, Frame};
use sysinfo::{Pid, ProcessesToUpdate, Signal, System};

struct ProcInfo {
    pid: Pid,
    name: String,
    cpu: f32,
    mem_mb: u64,
}

enum InputMode {
    Normal,
    Search,
}

/// Application state: everything draw() and the key handlers need.
struct AppState {
    processes: Vec<ProcInfo>,
    table_state: TableState,
    input_mode: InputMode,
    search_query: String,
    status: Option<String>,
    /// Tracks WHICH process is selected (by pid), not which row index — the list
    /// re-sorts by CPU every tick, so a row index would silently point at a
    /// different process after every refresh.
    selected_pid: Option<Pid>,
}

impl AppState {
    fn new() -> Self {
        Self {
            processes: Vec::new(),
            table_state: TableState::default(),
            input_mode: InputMode::Normal,
            search_query: String::new(),
            status: None,
            selected_pid: None,
        }
    }
}

/// Narrow borrow on purpose: only the two fields this needs, not all of AppState —
/// lets callers still mutate table_state/status while a filtered list is alive.
fn filtered_processes<'a>(processes: &'a [ProcInfo], query: &str) -> Vec<&'a ProcInfo> {
    let query = query.to_lowercase();
    processes
        .iter()
        .filter(|p| query.is_empty() || p.name.to_lowercase().contains(&query))
        .collect()
}

fn select_next(state: &mut AppState) {
    let filtered = filtered_processes(&state.processes, &state.search_query);
    if filtered.is_empty() {
        state.selected_pid = None;
        return;
    }
    let current = state
        .selected_pid
        .and_then(|pid| filtered.iter().position(|p| p.pid == pid));
    let next = match current {
        Some(i) => (i + 1).min(filtered.len() - 1),
        None => 0,
    };
    state.selected_pid = Some(filtered[next].pid);
}

fn select_prev(state: &mut AppState) {
    let filtered = filtered_processes(&state.processes, &state.search_query);
    if filtered.is_empty() {
        state.selected_pid = None;
        return;
    }
    let current = state
        .selected_pid
        .and_then(|pid| filtered.iter().position(|p| p.pid == pid));
    let prev = match current {
        Some(i) => i.saturating_sub(1),
        None => 0,
    };
    state.selected_pid = Some(filtered[prev].pid);
}

/// Looks up a single pid fresh (independent of the metrics thread's own `System`)
/// and sends it SIGKILL, turning the OS-level outcome into a message for the UI.
fn kill_process(pid: Pid) -> String {
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);

    match sys.process(pid) {
        Some(process) => match process.kill_with(Signal::Kill) {
            Some(true) => format!("killed pid {pid}"),
            Some(false) => format!("permission denied: could not kill pid {pid}"),
            None => "SIGKILL is not supported on this platform".to_string(),
        },
        None => format!("pid {pid} not found (already exited?)"),
    }
}

fn kill_selected(state: &mut AppState) {
    state.status = match state.selected_pid {
        Some(pid) => Some(kill_process(pid)),
        None => Some("no process selected".to_string()),
    };
}

fn spawn_metrics_thread(tx: mpsc::Sender<Vec<ProcInfo>>) {
    thread::spawn(move || {
        let mut sys = System::new_all();
        sys.refresh_all();

        loop {
            thread::sleep(Duration::from_secs(1));
            sys.refresh_all();

            let mut snapshot: Vec<ProcInfo> = sys
                .processes()
                .iter()
                .map(|(pid, process)| ProcInfo {
                    pid: *pid,
                    name: process.name().to_string_lossy().into_owned(),
                    cpu: process.cpu_usage(),
                    mem_mb: process.memory() / 1024 / 1024,
                })
                .collect();

            // Highest CPU first, like htop's default sort.
            snapshot.sort_by(|a, b| b.cpu.partial_cmp(&a.cpu).unwrap());

            if tx.send(snapshot).is_err() {
                break;
            }
        }
    });
}

fn draw(frame: &mut Frame, state: &mut AppState) {
    let filtered = filtered_processes(&state.processes, &state.search_query);

    // Re-locate the selected process by pid every frame — its row index may have
    // shifted (re-sorted by CPU) or vanished (exited, or filtered out by search).
    let index = state
        .selected_pid
        .and_then(|pid| filtered.iter().position(|p| p.pid == pid));

    match (index, filtered.first()) {
        (Some(i), _) => state.table_state.select(Some(i)),
        (None, Some(first)) => {
            // Nothing selected yet, or the selected process is gone: default to the top row.
            state.selected_pid = Some(first.pid);
            state.table_state.select(Some(0));
        }
        (None, None) => state.table_state.select(None),
    }

    let header = Row::new(["PID", "NAME", "CPU%", "RAM (MB)"])
        .style(Style::default().add_modifier(Modifier::BOLD));

    let rows = filtered.iter().map(|p| {
        Row::new([
            p.pid.to_string(),
            p.name.clone(),
            format!("{:.1}%", p.cpu),
            p.mem_mb.to_string(),
        ])
    });

    let widths = [
        Constraint::Length(10),
        Constraint::Percentage(50),
        Constraint::Length(8),
        Constraint::Length(12),
    ];

    let title = match state.input_mode {
        InputMode::Normal => " SysPulse — \u{2191}/\u{2193} select \u{00b7} / search \u{00b7} k kill \u{00b7} q quit ".to_string(),
        InputMode::Search => format!(" Search: {}_  (Enter/Esc to exit) ", state.search_query),
    };

    let table = Table::new(rows, widths)
        .header(header)
        .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("> ")
        .block(Block::default().title(title).borders(Borders::ALL));

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(frame.area());

    frame.render_stateful_widget(table, chunks[0], &mut state.table_state);

    let status_line = state.status.clone().unwrap_or_default();
    frame.render_widget(Paragraph::new(status_line), chunks[1]);
}

fn run(terminal: &mut DefaultTerminal, rx: mpsc::Receiver<Vec<ProcInfo>>) -> io::Result<()> {
    let mut state = AppState::new();

    loop {
        // Drain: only the latest snapshot matters if more than one piled up.
        let mut latest = None;
        while let Ok(snapshot) = rx.try_recv() {
            latest = Some(snapshot);
        }
        if let Some(snapshot) = latest {
            state.processes = snapshot;
        }

        terminal.draw(|frame| draw(frame, &mut state))?;

        // Non-blocking key check: returns after 50ms even with no input,
        // so the loop keeps polling the channel instead of stalling on stdin.
        if event::poll(Duration::from_millis(50)).unwrap_or(false)
            && let Ok(Event::Key(key)) = event::read()
        {
            match state.input_mode {
                InputMode::Normal => match key.code {
                    KeyCode::Char('q') => return Ok(()),
                    KeyCode::Char('/') => {
                        state.input_mode = InputMode::Search;
                        state.status = None;
                    }
                    KeyCode::Down => select_next(&mut state),
                    KeyCode::Up => select_prev(&mut state),
                    KeyCode::Char('k') => kill_selected(&mut state),
                    _ => {}
                },
                InputMode::Search => match key.code {
                    KeyCode::Enter | KeyCode::Esc => state.input_mode = InputMode::Normal,
                    KeyCode::Backspace => {
                        state.search_query.pop();
                    }
                    KeyCode::Char(c) => state.search_query.push(c),
                    _ => {}
                },
            }
        }
    }
}

fn main() -> io::Result<()> {
    let (tx, rx) = mpsc::channel::<Vec<ProcInfo>>();
    spawn_metrics_thread(tx);

    // Raw mode + alternate screen: on, then guaranteed back off via restore().
    let mut terminal = ratatui::init();
    let result = run(&mut terminal, rx);
    ratatui::restore();

    result
}
