# SysPulse


https://github.com/user-attachments/assets/085b20fa-1fa9-4acc-8ec7-b51672888133


A lightweight, terminal-based system monitor (TUI) for tracking CPU and memory usage of running processes — search, sort, and kill them without leaving the terminal.

Built in Rust as a learning project covering ownership across threads, non-blocking concurrency, and building an interactive TUI from scratch.

## Features

- **Real-time process listing** — PID, name, CPU%, RAM usage, refreshed every second.
- **Sorted by CPU usage**, highest first, like `htop`'s default view.
- **Live search** — press `/` and type to filter processes by name as you go.
- **Arrow-key navigation** — selection tracks a specific process even as the list re-sorts under it.
- **Kill a process** — `k` sends `SIGKILL` to the selected process, with the actual OS response (success, permission denied, or already exited) shown on screen.

## Screenshot

*(add a terminal screenshot or GIF here — e.g. recorded with [VHS](https://github.com/charmbracelet/vhs) or [asciinema](https://asciinema.org))*

## Installation

Requires [Rust](https://www.rust-lang.org/tools/install) (via `rustup` or your package manager).

```bash
git clone <this-repo-url>
cd syspulse
cargo build --release
```

## Usage

```bash
cargo run
```

or run the compiled binary directly:

```bash
./target/release/syspulse
```

### Keybindings

| Key         | Action                              |
|-------------|--------------------------------------|
| `↑` / `↓`   | Move selection up/down               |
| `/`         | Enter search mode                    |
| `Enter` / `Esc` | Exit search mode (back to normal) |
| `k`         | Kill the selected process (`SIGKILL`) |
| `q`         | Quit (Normal mode only)              |

> **Warning**: `k` sends a real `SIGKILL` — there is no confirmation prompt and no undo. Test it on a disposable process first.

## Tech stack

- [`sysinfo`](https://crates.io/crates/sysinfo) — cross-platform process, CPU, and memory data.
- [`ratatui`](https://crates.io/crates/ratatui) — terminal UI widgets and layout.
- [`crossterm`](https://crates.io/crates/crossterm) — raw-mode terminal control and keyboard input, cross-platform (Windows, macOS, Linux).

## How it works

- A background thread refreshes process data every second and sends snapshots to the main thread over an `mpsc` channel — no shared mutable state, no locks.
- The main thread polls the keyboard non-blockingly (`crossterm::event::poll`) so the UI stays responsive between refreshes.
- Selection is tracked by process ID, not row position, so it stays on the same process even as the list re-sorts under it.
