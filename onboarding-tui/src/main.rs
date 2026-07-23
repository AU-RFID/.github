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
use std::thread;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::layout::Flex;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};

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
    StepStart,
    Line(String),
    StepDone(bool),
    InstallDone,
}

#[derive(Clone)]
enum ItemState {
    Checking,
    Installed(String),
    Missing,
}

struct StepStatus {
    title: String,
    state: char, // '·' pending, '…' running, '✓' ok, '✗' failed
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
    /// Cursor position over the combined display order across all boxes.
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
    current_step: usize,
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

        App {
            dry_run,
            platform,
            items,
            sections,
            boxes,
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
            current_step: 0,
            log: Vec::new(),
            rx: None,
            follow_ups: Vec::new(),
        }
    }

    fn start_scan(&mut self) {
        self.states = vec![ItemState::Checking; self.items.len()];
        self.selected = vec![false; self.items.len()];
        self.scan_done = false;
        let checks: Vec<(String, Exec)> = (0..self.items.len())
            .map(|i| (self.items[i].check.clone(), self.exec_for(i)))
            .collect();
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let dry_run = self.dry_run;
        thread::spawn(move || {
            for (i, (check, exec)) in checks.iter().enumerate() {
                // Dry run: pretend nothing is installed so the full first-day
                // flow (everything missing → install → summary) can be previewed.
                let result = if dry_run {
                    thread::sleep(Duration::from_millis(200)); // keep the checking… animation visible
                    None
                } else {
                    run_check(check, exec)
                };
                let _ = tx.send(Msg::Check(i, result));
            }
            let _ = tx.send(Msg::ScanDone);
        });
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
        self.current_step = 0;

        let mut work: Vec<(String, String, Exec)> = Vec::new();
        for &i in &picked {
            let item = &self.items[i];
            for s in &item.install {
                self.steps.push(StepStatus { title: s.title.to_string(), state: '·' });
                work.push((s.title.to_string(), s.cmd.clone(), self.exec_for(i)));
            }
            for f in &item.follow_up {
                self.follow_ups.push((*f).to_string());
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
                Msg::StepStart => {
                    if let Some(s) = self.steps.get_mut(self.current_step) {
                        s.state = '…';
                    }
                }
                Msg::Line(l) => self.log.push(l),
                Msg::StepDone(ok) => {
                    if let Some(s) = self.steps.get_mut(self.current_step) {
                        s.state = if ok { '✓' } else { '✗' };
                    }
                    self.current_step += 1;
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
                // Re-check everything so the summary shows the real post-install state.
                for i in 0..self.items.len() {
                    self.states[i] = match run_check(&self.items[i].check, &self.exec_for(i)) {
                        Some(d) => ItemState::Installed(d),
                        None => ItemState::Missing,
                    };
                }
            }
            self.screen = Screen::Summary;
        }
    }

    /// Total number of selectable rows across every box.
    fn total_rows(&self) -> usize {
        self.boxes.iter().map(|(_, o)| o.len()).sum()
    }

    /// Move the Scan-screen cursor across all boxes.
    fn move_row(&mut self, delta: i32) {
        let len = self.total_rows();
        if len == 0 {
            return;
        }
        self.cursor = (self.cursor as i32 + delta).rem_euclid(len as i32) as usize;
    }

    /// The software item under the Scan-screen cursor.
    fn cursor_item(&self) -> Option<usize> {
        let mut c = self.cursor;
        for (_, order) in &self.boxes {
            if c < order.len() {
                return order.get(c).copied();
            }
            c -= order.len();
        }
        None
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

    /// The first `pick-one` section with nothing installed or selected, if any.
    /// Returns its title for the footer warning.
    fn unsatisfied_pick_one(&self) -> Option<&str> {
        for (si, sec) in self.sections.iter().enumerate() {
            if sec.rule != Rule::PickOne {
                continue;
            }
            let satisfied = self.items.iter().enumerate().any(|(i, sw)| {
                sw.section == si
                    && (matches!(self.states[i], ItemState::Installed(_)) || self.selected[i])
            });
            if !satisfied {
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

/// Runs install steps sequentially on a background thread, streaming output.
/// In dry-run mode nothing is executed — each step's command is only printed.
fn run_installer(work: Vec<(String, String, Exec)>, tx: Sender<Msg>, dry_run: bool) {
    for (title, cmd, exec) in work {
        let _ = tx.send(Msg::StepStart);
        if dry_run {
            let where_note = match &exec {
                Exec::Bash => String::new(),
                Exec::Wsl(d) => format!(" (in WSL: {d})"),
                Exec::WinCmd => " (on Windows host)".to_string(),
            };
            let _ = tx.send(Msg::Line(format!("[dry-run] {title}{where_note} — would run:")));
            let _ = tx.send(Msg::Line(format!("  $ {cmd}")));
            thread::sleep(Duration::from_millis(400)); // let the UI show progress
            let _ = tx.send(Msg::StepDone(true));
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
        let _ = tx.send(Msg::StepDone(ok));
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
                KeyCode::Char(' ') => {
                    // Missing optional/pick-one tools can be toggled; required
                    // missing tools are locked in.
                    if app.scan_done {
                        app.notice = None;
                        if let Some(i) = app.cursor_item() {
                            if matches!(app.states[i], ItemState::Missing)
                                && app.rule_of(i) != Rule::Required
                            {
                                app.selected[i] = !app.selected[i];
                            }
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
    let (body, footer) = chrome(f, app, "Choose a WSL distro");

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
                .title(" Dev tools will be installed inside this distro "),
        )
        .highlight_style(theme::highlight())
        .highlight_symbol(" > ");
    let mut state = ListState::default();
    state.select(Some(app.distro_cursor));
    f.render_stateful_widget(list, body, &mut state);

    hint(f, footer, "↑/↓ move · enter select · esc back · q quit");
}

fn draw_welcome(f: &mut Frame, app: &App) {
    // Vertically center the whole block: logo + university + subtitle + buttons.
    let content_height = LOGO.len() as u16 + 8;
    let [area] = Layout::vertical([Constraint::Length(content_height)])
        .flex(Flex::Center)
        .areas(f.area());

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
        Paragraph::new(UNIVERSITY).style(theme::navy()).centered(),
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

    // Hint pinned near the bottom of the screen.
    let [hint_row] = Layout::vertical([Constraint::Length(1)])
        .flex(Flex::End)
        .areas(f.area());
    f.render_widget(
        Paragraph::new("←/→ switch · enter select · q quit")
            .style(theme::dim())
            .centered(),
        hint_row,
    );
}

fn button(f: &mut Frame, area: Rect, label: &str, active: bool) {
    let (block_style, text_style) = if active {
        (theme::border(), theme::highlight())
    } else {
        (theme::dim(), theme::dim())
    };
    let block = Block::default().borders(Borders::ALL).border_style(block_style);
    f.render_widget(Paragraph::new(label).style(text_style).centered().block(block), area);
}

/// Standard header/body/footer chrome for the non-welcome screens.
fn chrome(f: &mut Frame, app: &App, screen_title: &str) -> (Rect, Rect) {
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(5),
        Constraint::Length(1),
    ])
    .areas(f.area());

    let dry = if app.dry_run { "  [DRY RUN — nothing will be installed]" } else { "" };
    f.render_widget(
        Paragraph::new(format!(" RFID Lab Onboarding · {screen_title} — {}{dry}", app.platform.label()))
            .style(theme::title())
            .block(Block::default().borders(Borders::ALL).border_style(theme::border())),
        header,
    );
    (body, footer)
}

fn hint(f: &mut Frame, area: Rect, text: &str) {
    f.render_widget(Paragraph::new(text).style(theme::dim()), area);
}

/// Build the display line for one software item (Ubuntu-installer style:
/// `[X]` selected, `[ ]` unselected, locked `[X]` for required-missing).
fn scan_line<'a>(app: &'a App, i: usize) -> Line<'a> {
    let sw = &app.items[i];
    let (icon, style) = match &app.states[i] {
        ItemState::Checking => ("⏳ checking…".to_string(), theme::dim()),
        ItemState::Installed(d) => (format!("✓ {d}"), theme::good()),
        ItemState::Missing => ("✗ missing".to_string(), theme::bad()),
    };
    // Checkbox: optional missing tools toggle with space; required missing
    // tools are locked in; installed tools have nothing to pick.
    let mark = if !app.scan_done || !matches!(app.states[i], ItemState::Missing) {
        "    "
    } else if app.rule_of(i) == Rule::Required || app.selected[i] {
        "[X] " // required-missing is locked in; optional shows its toggle state
    } else {
        "[ ] "
    };
    let star = if sw.preferred { "★ preferred  " } else { "" };
    Line::from(vec![
        Span::styled(format!(" {mark}"), theme::border()),
        Span::styled(format!("{:<20}", sw.name), Style::new().bold()),
        Span::styled(star, theme::title()),
        Span::styled(icon, style),
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
                .title(title.to_string()),
        )
        .highlight_style(theme::highlight())
        .highlight_symbol(" > ");
    let mut state = ListState::default();
    state.select(cursor);
    f.render_stateful_widget(list, area, &mut state);
}

fn draw_scan(f: &mut Frame, app: &mut App) {
    let (body, footer) = chrome(f, app, "What you have vs. what we use");

    // Split: boxes region on top, fixed About pane at the bottom.
    let [boxes_area, detail_area] =
        Layout::vertical([Constraint::Min(3), Constraint::Length(4)]).areas(body);

    let heights: Vec<u16> = app.boxes.iter().map(|(_, o)| o.len() as u16 + 2).collect();

    // Which box holds the cursor, and its offset within that box.
    let (cur_box, cur_local) = {
        let mut c = app.cursor;
        let mut bi = 0;
        for (i, (_, order)) in app.boxes.iter().enumerate() {
            if c < order.len() {
                bi = i;
                break;
            }
            c -= order.len();
        }
        (bi, c)
    };

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
        let (section_idx, order) = &app.boxes[j];
        let local = (j == cur_box).then_some(cur_local);
        scan_box(f, app, areas[slot], &app.sections[*section_idx].title, order, local);
    }

    // Description of the highlighted item.
    let desc = app
        .cursor_item()
        .and_then(|i| app.items.get(i))
        .map(|sw| sw.description.as_str())
        .unwrap_or("");
    f.render_widget(
        Paragraph::new(desc).style(theme::dim()).wrap(Wrap { trim: true }).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme::dim())
                .title(" About "),
        ),
        detail_area,
    );

    let scroll = {
        let up = if start > 0 { "▲" } else { " " };
        let down = if end + 1 < app.boxes.len() { "▼" } else { " " };
        format!("{up}{down}")
    };
    if let Some(notice) = &app.notice {
        f.render_widget(Paragraph::new(format!(" ⚠ {notice}")).style(theme::bad()), footer);
    } else if app.scan_done {
        let n = app.selected.iter().filter(|s| **s).count();
        hint(f, footer, &format!("{scroll} space toggle · enter install {n} selected · r rescan · esc back · q quit"));
    } else {
        hint(f, footer, "scanning your system…");
    }
}

fn draw_install(f: &mut Frame, app: &App) {
    let (body, footer) = chrome(f, app, "Installing");

    let [left, right] =
        Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)]).areas(body);

    let items: Vec<ListItem> = app
        .steps
        .iter()
        .map(|s| {
            let style = match s.state {
                '✓' => theme::good(),
                '✗' => theme::bad(),
                '…' => theme::title(),
                _ => theme::dim(),
            };
            ListItem::new(format!("{} {}", s.state, s.title)).style(style)
        })
        .collect();
    f.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme::border())
                .title(" Steps "),
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
                .title(" Output "),
        ),
        right,
    );

    hint(f, footer, "installing… please wait");
}

fn draw_summary(f: &mut Frame, app: &App) {
    let (body, footer) = chrome(f, app, "Summary");

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
                .title(" Results "),
        ),
        body,
    );

    hint(f, footer, "enter rescan · q quit");
}
