//! RFID Lab onboarding TUI (Rust + ratatui, Auburn-themed).
//!
//! Flow: Welcome (centered, logo) → Scan (installed vs. wanted) → Install
//! (streaming progress) → Summary (results + follow-up commands).
//!
//! The list of software lives entirely in `src/software.rs` — add or remove
//! entries there and every screen picks them up automatically.

mod detect;
mod software;
mod theme;

use std::io::{self, BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// How long a dry-run install step pauses so its progress is visible.
const DRY_RUN_STEP_PAUSE: Duration = Duration::from_millis(400);
/// Per-item pause when revealing the dry-run scan top-to-bottom.
const DRY_RUN_CHECK_PAUSE: Duration = Duration::from_millis(70);

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::layout::Flex;
use ratatui::prelude::*;
use ratatui::widgets::{
    Block, Borders, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation,
    ScrollbarState, Wrap,
};

use detect::{detect, Platform};
use software::{load, Location, Rule, SectionDef, Software};

/// Where a command runs: the local shell, inside a WSL distro (from a
/// Windows host), or on the Windows host itself (winget).
#[derive(Clone)]
enum Exec {
    Bash,
    Wsl(String),
    WinCmd,
}

fn build_command(exec: &Exec, cmd: &str) -> Command {
    match exec {
        Exec::Bash => {
            let mut c = Command::new("bash");
            c.arg("-lc").arg(cmd);
            c
        }
        Exec::Wsl(distro) => {
            let mut c = Command::new("wsl.exe");
            c.args(["-d", distro, "--", "bash", "-lc", cmd]);
            c
        }
        Exec::WinCmd => {
            let mut c = Command::new("cmd");
            c.args(["/C", cmd]);
            c
        }
    }
}

const LOGO: [&str; 6] = [
    "██████╗ ███████╗██╗██████╗   ██╗      █████╗ ██████╗ ",
    "██╔══██╗██╔════╝██║██╔══██╗  ██║     ██╔══██╗██╔══██╗",
    "██████╔╝█████╗  ██║██║  ██║  ██║     ███████║██████╔╝",
    "██╔══██╗██╔══╝  ██║██║  ██║  ██║     ██╔══██║██╔══██╗",
    "██║  ██║██║     ██║██████╔╝  ███████╗██║  ██║██████╔╝",
    "╚═╝  ╚═╝╚═╝     ╚═╝╚═════╝   ╚══════╝╚═╝  ╚═╝╚═════╝ ",
];

const UNIVERSITY: &str = "A U B U R N   U N I V E R S I T Y";

enum Screen {
    Welcome,
    /// Windows host only: pick the WSL distro dev tools install into.
    Distro,
    Scan,
    Install,
    Summary,
}

enum Msg {
    /// Scan result for item i: Some(version/detail) if installed.
    Check(usize, Option<String>),
    ScanDone,
    /// Install step `usize` started.
    StepStart(usize),
    Line(String),
    /// Install step `usize` finished; bool = success.
    StepDone(usize, bool),
    InstallDone,
}

#[derive(Clone)]
enum ItemState {
    Checking,
    Installed(String),
    Missing,
}

#[derive(Clone, Copy)]
enum StepState {
    Pending,
    Running,
    Done,
    Failed,
}

impl StepState {
    fn symbol(self) -> &'static str {
        match self {
            StepState::Pending => "·",
            StepState::Running => "…",
            StepState::Done => "✓",
            StepState::Failed => "✗",
        }
    }
    fn style(self) -> Style {
        match self {
            StepState::Pending => theme::dim(),
            StepState::Running => theme::title(),
            StepState::Done => theme::good(),
            StepState::Failed => theme::bad(),
        }
    }
}

struct StepStatus {
    title: String,
    state: StepState,
}

/// A navigable row on the scan screen: a collapsed section's header, or a
/// software item.
#[derive(Clone, Copy)]
enum Nav {
    Header(usize), // section index
    Item(usize),   // item index
}

struct App {
    dry_run: bool,
    platform: Platform,
    items: Vec<Software>,
    /// Section definitions (titles + rules), in display order.
    sections: Vec<SectionDef>,
    /// One box per non-empty section (by section index). Each holds the item
    /// indices shown in that box.
    boxes: Vec<(usize, Vec<usize>)>,
    /// Per-section collapsed state (indexed by section index). Only ever true
    /// for collapsible sections.
    collapsed: Vec<bool>,
    /// Cursor position over the nav rows (see `nav_rows`).
    cursor: usize,
    /// One-shot warning shown in the footer (e.g. "pick an editor").
    notice: Option<String>,
    /// WSL distros found on a Windows host, and the picker cursor.
    distros: Vec<String>,
    distro_cursor: usize,
    states: Vec<ItemState>,
    selected: Vec<bool>,
    screen: Screen,
    welcome_btn: usize, // 0 = Get Started, 1 = Exit
    scan_done: bool,
    steps: Vec<StepStatus>,
    log: Vec<String>,
    rx: Option<Receiver<Msg>>,
    follow_ups: Vec<String>,
}

impl App {
    fn new(dry_run: bool) -> Self {
        let platform = detect();
        let (sections, items) = load(&platform);
        let n = items.len();

        // One box per non-empty section (software.json is the source of truth
        // for section order and membership); empty sections render nothing.
        let boxes: Vec<(usize, Vec<usize>)> = (0..sections.len())
            .filter_map(|s| {
                let order: Vec<usize> = (0..n).filter(|&i| items[i].section == s).collect();
                (!order.is_empty()).then_some((s, order))
            })
            .collect();

        // Collapsible sections start collapsed.
        let collapsed: Vec<bool> = sections.iter().map(|s| s.collapsible).collect();

        App {
            dry_run,
            platform,
            items,
            sections,
            boxes,
            collapsed,
            cursor: 0,
            notice: None,
            distros: Vec::new(),
            distro_cursor: 0,
            states: vec![ItemState::Checking; n],
            selected: vec![false; n],
            screen: Screen::Welcome,
            welcome_btn: 0,
            scan_done: false,
            steps: Vec::new(),
            log: Vec::new(),
            rx: None,
            follow_ups: Vec::new(),
        }
    }

    fn start_scan(&mut self) {
        self.states = vec![ItemState::Checking; self.items.len()];
        self.selected = vec![false; self.items.len()];
        self.scan_done = false;
        let jobs = self.check_jobs();
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);

        if self.dry_run {
            // Preview a fresh machine: report everything missing, revealed
            // top-to-bottom so the scan animation is visible.
            thread::spawn(move || {
                for (i, _, _) in jobs {
                    thread::sleep(DRY_RUN_CHECK_PAUSE);
                    let _ = tx.send(Msg::Check(i, None));
                }
                let _ = tx.send(Msg::ScanDone);
            });
        } else {
            // Run checks across a bounded pool; results stream to the UI as
            // each finishes, then ScanDone once the pool drains.
            thread::spawn(move || {
                for (i, result) in spawn_check_pool(jobs) {
                    let _ = tx.send(Msg::Check(i, result));
                }
                let _ = tx.send(Msg::ScanDone);
            });
        }
        self.screen = Screen::Scan;
    }

    fn start_install(&mut self) {
        let picked: Vec<usize> = (0..self.items.len()).filter(|&i| self.selected[i]).collect();
        if picked.is_empty() {
            return;
        }
        self.steps.clear();
        self.follow_ups.clear();
        self.log.clear();

        let mut work: Vec<(String, String, Exec)> = Vec::new();
        for &i in &picked {
            let item = &self.items[i];
            for s in &item.install {
                self.steps.push(StepStatus {
                    title: s.title.clone(),
                    state: StepState::Pending,
                });
                work.push((s.title.clone(), s.cmd.clone(), self.exec_for(i)));
            }
            for f in &item.follow_up {
                self.follow_ups.push(f.clone());
            }
        }

        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let dry_run = self.dry_run;
        thread::spawn(move || run_installer(work, tx, dry_run));
        self.screen = Screen::Install;
    }

    fn drain_messages(&mut self) {
        let Some(rx) = &self.rx else { return };
        let mut install_done = false;
        while let Ok(msg) = rx.try_recv() {
            match msg {
                Msg::Check(i, detail) => {
                    self.states[i] = match detail {
                        Some(d) => ItemState::Installed(d),
                        None => ItemState::Missing,
                    };
                }
                Msg::ScanDone => {
                    self.scan_done = true;
                    // Required + missing → locked in. Optional/pick-one start unticked.
                    for i in 0..self.items.len() {
                        self.selected[i] = matches!(self.states[i], ItemState::Missing)
                            && self.rule_of(i) == Rule::Required;
                    }
                    self.rx = None;
                    return;
                }
                Msg::StepStart(i) => {
                    if let Some(s) = self.steps.get_mut(i) {
                        s.state = StepState::Running;
                    }
                }
                Msg::Line(l) => self.log.push(l),
                Msg::StepDone(i, ok) => {
                    if let Some(s) = self.steps.get_mut(i) {
                        s.state = if ok { StepState::Done } else { StepState::Failed };
                    }
                }
                Msg::InstallDone => install_done = true,
            }
        }
        if install_done {
            self.rx = None;
            if self.dry_run {
                // Preview the happy path: everything just "installed" succeeds.
                for i in 0..self.items.len() {
                    if self.selected[i] {
                        self.states[i] = ItemState::Installed("(dry-run)".into());
                    }
                }
            } else {
                // Re-check everything (same bounded pool) so the summary
                // shows the real post-install state.
                for (i, r) in spawn_check_pool(self.check_jobs()) {
                    self.states[i] = match r {
                        Some(d) => ItemState::Installed(d),
                        None => ItemState::Missing,
                    };
                }
            }
            self.screen = Screen::Summary;
        }
    }

    /// The (item index, check command, exec context) for every tool — the
    /// input to a parallel check run.
    fn check_jobs(&self) -> Vec<CheckJob> {
        (0..self.items.len())
            .map(|i| (i, self.items[i].check.clone(), self.exec_for(i)))
            .collect()
    }

    /// The scan screen's navigable rows, top to bottom: a collapsed section
    /// contributes a single header row; every other section contributes its
    /// item rows. This is the single source of truth for cursor movement.
    fn nav_rows(&self) -> Vec<Nav> {
        let mut rows = Vec::new();
        for (sec, order) in &self.boxes {
            if self.collapsed[*sec] {
                rows.push(Nav::Header(*sec));
            } else {
                rows.extend(order.iter().map(|&i| Nav::Item(i)));
            }
        }
        rows
    }

    /// The nav row under the cursor.
    fn cursor_nav(&self) -> Option<Nav> {
        self.nav_rows().get(self.cursor).copied()
    }

    /// Move the Scan-screen cursor across the nav rows (wrapping).
    fn move_row(&mut self, delta: i32) {
        let len = self.nav_rows().len();
        if len == 0 {
            return;
        }
        self.cursor = (self.cursor as i32 + delta).rem_euclid(len as i32) as usize;
    }

    /// The software item under the cursor, if the cursor is on an item row.
    fn cursor_item(&self) -> Option<usize> {
        match self.cursor_nav()? {
            Nav::Item(i) => Some(i),
            Nav::Header(_) => None,
        }
    }

    /// Expand/collapse the section under the cursor, keeping the cursor on that
    /// section (its header when collapsed, its first item when expanded).
    fn set_collapsed(&mut self, sec: usize, want: bool) {
        if !self.sections[sec].collapsible || self.collapsed[sec] == want {
            return;
        }
        self.collapsed[sec] = want;
        let rows = self.nav_rows();
        self.cursor = rows
            .iter()
            .position(|r| match r {
                Nav::Header(s) => *s == sec,
                Nav::Item(i) => self.items[*i].section == sec,
            })
            .unwrap_or(0);
    }

    /// Toggle the collapse state of the section under the cursor, if collapsible.
    fn toggle_collapse_at_cursor(&mut self) {
        if let Some(sec) = self.cursor_nav().map(|n| match n {
            Nav::Header(s) => s,
            Nav::Item(i) => self.items[i].section,
        }) {
            if self.sections[sec].collapsible {
                self.set_collapsed(sec, !self.collapsed[sec]);
            }
        }
    }

    /// Where item `i`'s commands run: locally, or (from a Windows host) inside
    /// the chosen WSL distro for Dev tools and on the host for GUI apps.
    fn exec_for(&self, i: usize) -> Exec {
        if self.platform.windows_host() {
            match self.items[i].location {
                Location::Host => Exec::WinCmd,
                Location::Dev => {
                    Exec::Wsl(self.platform.wsl_distro.clone().unwrap_or_default())
                }
            }
        } else {
            Exec::Bash
        }
    }

    /// The rule governing item `i`'s section.
    fn rule_of(&self, i: usize) -> Rule {
        self.sections[self.items[i].section].rule
    }

    /// The first `pick-one` section that has items available but none
    /// installed or selected. Returns its title for the footer warning.
    /// Sections with no items on this platform (e.g. GUI-only on a server)
    /// are treated as satisfied.
    fn unsatisfied_pick_one(&self) -> Option<&str> {
        for (si, sec) in self.sections.iter().enumerate() {
            if sec.rule != Rule::PickOne {
                continue;
            }
            let mut has_items = false;
            let mut satisfied = false;
            for (i, sw) in self.items.iter().enumerate() {
                if sw.section != si {
                    continue;
                }
                has_items = true;
                if matches!(self.states[i], ItemState::Installed(_)) || self.selected[i] {
                    satisfied = true;
                    break;
                }
            }
            if has_items && !satisfied {
                return Some(sec.title.trim());
            }
        }
        None
    }
}

fn run_check(check: &str, exec: &Exec) -> Option<String> {
    let out = build_command(exec, check).output().ok()?;
    if out.status.success() {
        let detail = String::from_utf8_lossy(&out.stdout).trim().to_string();
        Some(if detail.is_empty() { "installed".into() } else { detail })
    } else {
        None
    }
}

/// A single check to run: (item index, check command, exec context).
type CheckJob = (usize, String, Exec);

/// Spawn a pool of worker threads (bounded by the CPU count) that run `jobs`
/// concurrently, so a slow tool never blocks the others without spawning an
/// unbounded number of processes. Returns a receiver that yields
/// (item index, result) as each check finishes and closes when all are done.
/// Callers can stream from it (scan) or drain it to completion (re-check).
fn spawn_check_pool(jobs: Vec<CheckJob>) -> Receiver<(usize, Option<String>)> {
    let (tx, rx) = mpsc::channel();
    if jobs.is_empty() {
        return rx; // already closed — no workers hold a sender
    }
    let workers = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(jobs.len());

    let queue = Arc::new(Mutex::new(jobs.into_iter()));
    for _ in 0..workers {
        let queue = Arc::clone(&queue);
        let tx = tx.clone();
        thread::spawn(move || loop {
            // Pop one job; release the lock before running the check.
            let Some((i, check, exec)) = queue.lock().unwrap().next() else {
                break;
            };
            let _ = tx.send((i, run_check(&check, &exec)));
        });
    }
    rx
}

/// Runs install steps sequentially on a background thread, streaming output.
/// In dry-run mode nothing is executed — each step's command is only printed.
fn run_installer(work: Vec<(String, String, Exec)>, tx: Sender<Msg>, dry_run: bool) {
    for (step, (title, cmd, exec)) in work.into_iter().enumerate() {
        let _ = tx.send(Msg::StepStart(step));
        if dry_run {
            let where_note = match &exec {
                Exec::Bash => String::new(),
                Exec::Wsl(d) => format!(" (in WSL: {d})"),
                Exec::WinCmd => " (on Windows host)".to_string(),
            };
            let _ = tx.send(Msg::Line(format!("[dry-run] {title}{where_note} — would run:")));
            let _ = tx.send(Msg::Line(format!("  $ {cmd}")));
            thread::sleep(DRY_RUN_STEP_PAUSE); // let the UI show progress
            let _ = tx.send(Msg::StepDone(step, true));
            continue;
        }
        let ok = match build_command(&exec, &cmd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(mut child) => {
                let stdout = child.stdout.take().unwrap();
                let stderr = child.stderr.take().unwrap();
                let tx2 = tx.clone();
                let h = thread::spawn(move || {
                    for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                        let _ = tx2.send(Msg::Line(line));
                    }
                });
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    let _ = tx.send(Msg::Line(line));
                }
                let _ = h.join();
                child.wait().map(|s| s.success()).unwrap_or(false)
            }
            Err(e) => {
                let _ = tx.send(Msg::Line(format!("spawn failed: {e}")));
                false
            }
        };
        let _ = tx.send(Msg::StepDone(step, ok));
    }
    let _ = tx.send(Msg::InstallDone);
}

fn main() -> io::Result<()> {
    let mut dry_run = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--dry-run" => dry_run = true,
            other => {
                eprintln!("Unknown option: {other} (supported: --dry-run)");
                std::process::exit(2);
            }
        }
    }

    // Restore the terminal even if we panic — otherwise the user's shell is
    // left in raw mode with the alternate screen still active.
    let orig_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = io::stdout().execute(LeaveAlternateScreen);
        orig_hook(info);
    }));

    enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    terminal.clear()?;

    let res = run_app(&mut terminal, dry_run);

    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;
    res
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, dry_run: bool) -> io::Result<()> {
    let mut app = App::new(dry_run);

    loop {
        app.drain_messages();
        terminal.draw(|f| draw(f, &mut app))?;

        if !event::poll(Duration::from_millis(100))? {
            continue;
        }
        let Event::Key(key) = event::read()? else { continue };
        if key.kind != KeyEventKind::Press {
            continue;
        }

        match app.screen {
            Screen::Welcome => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::Char('h') | KeyCode::Char('l') => {
                    app.welcome_btn = 1 - app.welcome_btn;
                }
                KeyCode::Enter => {
                    if app.welcome_btn == 0 {
                        // On a Windows host, pick the WSL distro first.
                        if app.platform.windows_host() && app.platform.wsl_distro.is_none() {
                            app.distros = Platform::wsl_distros();
                            app.distro_cursor = 0;
                            app.screen = Screen::Distro;
                        } else {
                            app.start_scan();
                        }
                    } else {
                        return Ok(());
                    }
                }
                _ => {}
            },
            Screen::Distro => match key.code {
                KeyCode::Char('q') => return Ok(()),
                KeyCode::Esc => app.screen = Screen::Welcome,
                KeyCode::Up | KeyCode::Char('k') => {
                    if !app.distros.is_empty() {
                        app.distro_cursor = (app.distro_cursor + app.distros.len() - 1)
                            % app.distros.len();
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if !app.distros.is_empty() {
                        app.distro_cursor = (app.distro_cursor + 1) % app.distros.len();
                    }
                }
                KeyCode::Enter => {
                    if let Some(d) = app.distros.get(app.distro_cursor) {
                        app.platform.wsl_distro = Some(d.clone());
                        app.start_scan();
                    }
                }
                _ => {}
            },
            Screen::Scan => match key.code {
                KeyCode::Char('q') => return Ok(()),
                KeyCode::Esc => app.screen = Screen::Welcome,
                KeyCode::Up | KeyCode::Char('k') => app.move_row(-1),
                KeyCode::Down | KeyCode::Char('j') => app.move_row(1),
                // →/l expands a collapsed section; ←/h collapses.
                KeyCode::Right | KeyCode::Char('l') => {
                    if let Some(Nav::Header(sec)) = app.cursor_nav() {
                        app.set_collapsed(sec, false);
                    }
                }
                KeyCode::Left | KeyCode::Char('h') => {
                    if let Some(i) = app.cursor_item() {
                        app.set_collapsed(app.items[i].section, true);
                    }
                }
                KeyCode::Char(' ') => {
                    if !app.scan_done {
                        // ignore
                    } else if matches!(app.cursor_nav(), Some(Nav::Header(_))) {
                        // Space on a collapsed section header expands it.
                        app.toggle_collapse_at_cursor();
                    } else if let Some(i) = app.cursor_item() {
                        // Missing optional/pick-one tools can be toggled;
                        // required missing tools are locked in.
                        app.notice = None;
                        if matches!(app.states[i], ItemState::Missing)
                            && app.rule_of(i) != Rule::Required
                        {
                            app.selected[i] = !app.selected[i];
                        }
                    }
                }
                KeyCode::Char('r') => {
                    if app.scan_done {
                        app.notice = None;
                        app.start_scan();
                    }
                }
                KeyCode::Enter if app.scan_done => {
                    match app.unsatisfied_pick_one() {
                        None => app.start_install(),
                        Some(title) => {
                            app.notice = Some(format!("Pick at least one from:{title}"));
                        }
                    }
                }
                _ => {}
            },
            Screen::Install => {
                // installs are not cancellable; ignore keys
            }
            Screen::Summary => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                KeyCode::Enter => app.start_scan(),
                _ => {}
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

fn draw(f: &mut Frame, app: &mut App) {
    match app.screen {
        Screen::Welcome => draw_welcome(f, app),
        Screen::Distro => draw_distro(f, app),
        Screen::Scan => draw_scan(f, app),
        Screen::Install => draw_install(f, app),
        Screen::Summary => draw_summary(f, app),
    }
}

fn draw_distro(f: &mut Frame, app: &App) {
    let (body, footer) = chrome(f, app);

    if app.distros.is_empty() {
        f.render_widget(
            Paragraph::new(
                "No WSL distros found.\n\nInstall one first (in PowerShell):\n  wsl --install -d Ubuntu\n\nthen restart this tool.",
            )
            .style(theme::bad())
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).border_style(theme::border())),
            body,
        );
        hint(f, footer, "esc back · q quit");
        return;
    }

    let items: Vec<ListItem> = app
        .distros
        .iter()
        .map(|d| ListItem::new(format!("  {d}")))
        .collect();
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme::border())
                .title(" Dev tools will be installed inside this distro ")
                .title_style(theme::title()),
        )
        .highlight_style(theme::highlight())
        .highlight_symbol(" > ");
    let mut state = ListState::default();
    state.select(Some(app.distro_cursor));
    f.render_stateful_widget(list, body, &mut state);

    hint(f, footer, "↑/↓ move · enter select · esc back · q quit");
}

fn draw_welcome(f: &mut Frame, app: &App) {
    let frame = frame_area(f.area());
    // Vertically center the whole block: logo + university + subtitle + buttons.
    let content_height = LOGO.len() as u16 + 8;
    let [area] = Layout::vertical([Constraint::Length(content_height)])
        .flex(Flex::Center)
        .areas(frame);

    let mut rows = Layout::vertical([
        Constraint::Length(LOGO.len() as u16), // logo
        Constraint::Length(1),                 // spacer
        Constraint::Length(1),                 // AUBURN UNIVERSITY
        Constraint::Length(1),                 // spacer
        Constraint::Length(1),                 // subtitle
        Constraint::Length(1),                 // spacer
        Constraint::Length(3),                 // buttons
    ])
    .split(area)
    .to_vec();
    let buttons_row = rows.pop().unwrap();

    let logo_lines: Vec<Line> = LOGO.iter().map(|l| Line::styled(*l, theme::title())).collect();
    f.render_widget(Paragraph::new(logo_lines).centered(), rows[0]);

    f.render_widget(
        Paragraph::new(UNIVERSITY)
            .style(Style::new().fg(theme::GRAY).add_modifier(Modifier::BOLD))
            .centered(),
        rows[2],
    );

    let dry = if app.dry_run { "  ·  DRY RUN" } else { "" };
    f.render_widget(
        Paragraph::new(format!(
            "Onboarding — set up your dev environment  ·  {}{dry}",
            app.platform.label()
        ))
        .style(theme::dim())
        .centered(),
        rows[4],
    );

    // Two centered buttons side by side.
    let [btns] = Layout::horizontal([Constraint::Length(17 + 3 + 10)])
        .flex(Flex::Center)
        .areas(buttons_row);
    let [b1, _, b2] = Layout::horizontal([
        Constraint::Length(17),
        Constraint::Length(3),
        Constraint::Length(10),
    ])
    .areas(btns);
    button(f, b1, "  Get Started  ", app.welcome_btn == 0);
    button(f, b2, "  Exit  ", app.welcome_btn == 1);

    // Hint pinned near the bottom of the framed area.
    let [hint_row] = Layout::vertical([Constraint::Length(1)])
        .flex(Flex::End)
        .areas(frame);
    f.render_widget(
        Paragraph::new("←/→ switch · enter select · q quit")
            .style(theme::dim())
            .centered(),
        hint_row,
    );
}

fn button(f: &mut Frame, area: Rect, label: &str, active: bool) {
    let block = Block::default().borders(Borders::ALL);
    let (block, text_style) = if active {
        // Solid orange fill — the whole button is the accent, no gray outline.
        (block.border_style(theme::accent()).style(theme::highlight()), theme::highlight())
    } else {
        (block.border_style(theme::border()), theme::dim())
    };
    f.render_widget(Paragraph::new(label).style(text_style).centered().block(block), area);
}

/// A centered content frame. Terminals vary wildly in size — on a big window
/// (e.g. a 1440p monitor) a full-width TUI leaves a lot of dead space on the
/// right, so we cap the content to a comfortable size and center it with a
/// margin on every side. On a small terminal it simply fills the space (minus
/// a thin margin), so nothing is ever clipped.
fn frame_area(full: Rect) -> Rect {
    const MAX_W: u16 = 88;
    const MAX_H: u16 = 38;
    // Always leave a margin: 2 cols each side, 1 row top/bottom.
    let w = full.width.saturating_sub(4).clamp(1, MAX_W);
    let h = full.height.saturating_sub(2).clamp(1, MAX_H);
    let x = full.x + full.width.saturating_sub(w) / 2;
    let y = full.y + full.height.saturating_sub(h) / 2;
    Rect { x, y, width: w, height: h }
}

/// Total onboarding steps. Tool installation is step 1 of the flow; more steps
/// (SSH keys, cloning repos, …) may be added later, so the header shows "1/N".
const ONBOARDING_STEP: usize = 1;
const ONBOARDING_STEPS: usize = 1;

/// Standard header/body/footer chrome for the non-welcome screens. The title
/// bar is centered and reads "Tool Installation  <step>/<total>".
fn chrome(f: &mut Frame, app: &App) -> (Rect, Rect) {
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(5),
        Constraint::Length(1),
    ])
    .areas(frame_area(f.area()));

    let dry = if app.dry_run { "   ·   DRY RUN — nothing will be installed" } else { "" };
    f.render_widget(
        Paragraph::new(format!("Tool Installation  {ONBOARDING_STEP}/{ONBOARDING_STEPS}{dry}"))
            .style(theme::title())
            .centered()
            .block(Block::default().borders(Borders::ALL).border_style(theme::border())),
        header,
    );
    (body, footer)
}

fn hint(f: &mut Frame, area: Rect, text: &str) {
    f.render_widget(Paragraph::new(text).style(theme::dim()), area);
}

/// Build the display line for one software item — fixed-width columns so the
/// checkbox, name, preferred tag, and status all line up down the list.
/// Status is `✓ installed` (green) when present, blank otherwise.
fn scan_line<'a>(app: &'a App, i: usize) -> Line<'a> {
    let sw = &app.items[i];
    let (status_text, status_style) = match &app.states[i] {
        ItemState::Checking => ("…".to_string(), theme::dim()),
        ItemState::Installed(_) => ("✓ installed".to_string(), theme::good()),
        ItemState::Missing => (String::new(), theme::dim()),
    };
    // Checkbox: optional/pick-one missing tools toggle with space; required
    // missing tools are locked in; installed tools have nothing to pick.
    let mark = if !app.scan_done || !matches!(app.states[i], ItemState::Missing) {
        "    "
    } else if app.rule_of(i) == Rule::Required || app.selected[i] {
        "[X] " // required-missing is locked in; optional shows its toggle state
    } else {
        "[ ] "
    };
    let star = if sw.preferred { "★ preferred" } else { "" };
    Line::from(vec![
        Span::styled(format!(" {mark}"), theme::border()),
        Span::styled(format!("{:<22}", sw.name), Style::new().bold()),
        Span::styled(format!("{star:<12}"), theme::title()),
        Span::styled(status_text, status_style),
    ])
}

/// Render one section box (AI Tools / Required) with its own border and
/// title, highlighting the row under the global cursor when it's inside.
fn scan_box(f: &mut Frame, app: &App, area: Rect, title: &str, order: &[usize], cursor: Option<usize>) {
    let items: Vec<ListItem> = order.iter().map(|&i| ListItem::new(scan_line(app, i))).collect();
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme::border())
                .title(title.to_string())
                .title_style(theme::title()),
        )
        .highlight_style(theme::highlight())
        .highlight_symbol(" > ");
    let mut state = ListState::default();
    state.select(cursor);
    f.render_stateful_widget(list, area, &mut state);
}

/// A collapsed section's box title carries a ▸; an expanded collapsible one a ▾.
fn box_title(app: &App, sec: usize) -> String {
    let t = &app.sections[sec].title;
    if app.sections[sec].collapsible {
        let arrow = if app.collapsed[sec] { "▸" } else { "▾" };
        format!(" {arrow}{t}")
    } else {
        t.clone()
    }
}

/// Render a collapsed section as a single line: just its header plus a hint to
/// expand. Highlighted (selected) when the cursor is on it.
fn render_collapsed_box(f: &mut Frame, app: &App, area: Rect, sec: usize, count: usize, selected: bool) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::border())
        .title(box_title(app, sec))
        .title_style(theme::title());
    let (text, style) = if selected {
        (format!(" > {count} tools — →/space to expand "), theme::highlight())
    } else {
        (format!("   {count} tools — →/space to expand"), theme::dim())
    };
    f.render_widget(Paragraph::new(text).style(style).block(block), area);
}

fn draw_scan(f: &mut Frame, app: &mut App) {
    let (body, footer) = chrome(f, app);

    // Split: boxes region on top, fixed About pane at the bottom.
    let [boxes_area, detail_area] =
        Layout::vertical([Constraint::Min(3), Constraint::Length(4)]).areas(body);
    // Reserve a 1-col gutter on the right for the scrollbar (stable width
    // whether or not it's showing).
    let [boxes_area, scrollbar_col] =
        Layout::horizontal([Constraint::Min(1), Constraint::Length(1)]).areas(boxes_area);

    // A collapsed box is one line tall (plus borders); otherwise it sizes to
    // its items.
    let heights: Vec<u16> = app
        .boxes
        .iter()
        .map(|(sec, o)| if app.collapsed[*sec] { 3 } else { o.len() as u16 + 2 })
        .collect();

    // Locate the cursor: which section it's in (→ which box) and, if on an
    // item, that item's index.
    let cur = app.cursor_nav();
    let cur_section = cur.map(|n| match n {
        Nav::Header(s) => s,
        Nav::Item(i) => app.items[i].section,
    });
    let cur_box = cur_section
        .and_then(|cs| app.boxes.iter().position(|(s, _)| *s == cs))
        .unwrap_or(0);

    // Scroll by whole boxes so the cursor's box is always fully visible.
    let avail = boxes_area.height;
    let last_fitting = |start: usize| -> usize {
        let mut used = 0u16;
        let mut last = start;
        for (j, &h) in heights.iter().enumerate().skip(start) {
            if used + h <= avail {
                used += h;
                last = j;
            } else {
                break;
            }
        }
        last.max(start)
    };
    let mut start = 0usize;
    while cur_box > last_fitting(start) {
        start += 1;
    }
    let end = last_fitting(start); // inclusive

    // Lay out the visible boxes plus a filler, then render each.
    let mut constraints: Vec<Constraint> =
        (start..=end).map(|j| Constraint::Length(heights[j])).collect();
    constraints.push(Constraint::Min(0));
    let areas = Layout::vertical(constraints).split(boxes_area);

    for (slot, j) in (start..=end).enumerate() {
        let (sec, order) = &app.boxes[j];
        let (sec, area) = (*sec, areas[slot]);
        if app.collapsed[sec] {
            let selected = matches!(cur, Some(Nav::Header(s)) if s == sec);
            render_collapsed_box(f, app, area, sec, order.len(), selected);
        } else {
            let local = match cur {
                Some(Nav::Item(i)) if app.items[i].section == sec => {
                    order.iter().position(|&x| x == i)
                }
                _ => None,
            };
            scan_box(f, app, area, &box_title(app, sec), order, local);
        }
    }

    // Scrollbar in the gutter — the standard cue for "more above/below". Shown
    // only when the boxes don't all fit; the ▲/▼ end-caps and thumb position
    // tell the user which way there's more.
    let visible = end - start + 1;
    if visible < app.boxes.len() {
        let mut sb_state = ScrollbarState::new(app.boxes.len())
            .position(start)
            .viewport_content_length(visible);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▲"))
            .end_symbol(Some("▼"))
            .track_symbol(Some("│"))
            .thumb_symbol("█")
            .style(theme::dim())
            .thumb_style(theme::accent());
        f.render_stateful_widget(scrollbar, scrollbar_col, &mut sb_state);
    }

    // About pane: item description, or a hint on a collapsed header.
    let desc = match cur {
        Some(Nav::Item(i)) => app.items[i].description.clone(),
        Some(Nav::Header(_)) => "Optional tooling — expand to see what's inside.".to_string(),
        None => String::new(),
    };
    f.render_widget(
        Paragraph::new(desc).style(theme::dim()).wrap(Wrap { trim: true }).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme::border())
                .title(" About ")
                .title_style(theme::title()),
        ),
        detail_area,
    );

    if let Some(notice) = &app.notice {
        f.render_widget(Paragraph::new(format!(" ⚠ {notice}")).style(theme::bad()), footer);
    } else if app.scan_done {
        let n = app.selected.iter().filter(|s| **s).count();
        let hint_text = match cur {
            Some(Nav::Header(_)) => {
                format!(" ↑/↓ move · →/space expand · enter install {n} selected · r rescan · q quit")
            }
            _ => format!(
                " ↑/↓ move · space toggle · ←/→ collapse/expand · enter install {n} selected · r rescan · q quit"
            ),
        };
        hint(f, footer, &hint_text);
    } else {
        hint(f, footer, "scanning your system…");
    }
}

fn draw_install(f: &mut Frame, app: &App) {
    let (body, footer) = chrome(f, app);

    let [left, right] =
        Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)]).areas(body);

    let items: Vec<ListItem> = app
        .steps
        .iter()
        .map(|s| {
            ListItem::new(format!("{} {}", s.state.symbol(), s.title)).style(s.state.style())
        })
        .collect();
    f.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme::border())
                .title(" Steps ")
                .title_style(theme::title()),
        ),
        left,
    );

    let visible = right.height.saturating_sub(2) as usize;
    let start = app.log.len().saturating_sub(visible);
    let text = app.log[start..].join("\n");
    f.render_widget(
        Paragraph::new(text).wrap(Wrap { trim: false }).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme::border())
                .title(" Output ")
                .title_style(theme::title()),
        ),
        right,
    );

    hint(f, footer, "installing… please wait");
}

fn draw_summary(f: &mut Frame, app: &App) {
    let (body, footer) = chrome(f, app);

    let mut lines: Vec<Line> = app
        .items
        .iter()
        .enumerate()
        .map(|(i, sw)| match &app.states[i] {
            ItemState::Installed(d) => {
                Line::from(vec![
                    Span::styled(format!("  ✓ {:<22}", sw.name), theme::good()),
                    Span::styled(d.clone(), theme::dim()),
                ])
            }
            _ => Line::styled(format!("  ✗ {:<22}still missing", sw.name), theme::bad()),
        })
        .collect();

    if !app.follow_ups.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::styled("  Finish up by running these in a NEW terminal:", Style::new().bold().fg(theme::ORANGE)));
        for fu in &app.follow_ups {
            lines.push(Line::styled(format!("    $ {fu}"), theme::dim()));
        }
    }

    f.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme::border())
                .title(" Results ")
                .title_style(theme::title()),
        ),
        body,
    );

    hint(f, footer, "enter rescan · q quit");
}
