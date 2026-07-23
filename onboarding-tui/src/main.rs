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
use software::{registry, Software};

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
    states: Vec<ItemState>,
    selected: Vec<bool>,
    screen: Screen,
    welcome_btn: usize, // 0 = Get Started, 1 = Exit
    list_state: ListState,
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
        let items = registry(&platform);
        let n = items.len();
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        App {
            dry_run,
            platform,
            items,
            states: vec![ItemState::Checking; n],
            selected: vec![false; n],
            screen: Screen::Welcome,
            welcome_btn: 0,
            list_state,
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
        let checks: Vec<String> = self.items.iter().map(|s| s.check.clone()).collect();
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        thread::spawn(move || {
            for (i, check) in checks.iter().enumerate() {
                let _ = tx.send(Msg::Check(i, run_check(check)));
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

        let mut work: Vec<(String, String)> = Vec::new();
        for &i in &picked {
            let item = &self.items[i];
            for s in &item.install {
                self.steps.push(StepStatus { title: s.title.to_string(), state: '·' });
                work.push((s.title.to_string(), s.cmd.clone()));
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
                    // Preselect everything that's missing.
                    for (i, st) in self.states.iter().enumerate() {
                        self.selected[i] = matches!(st, ItemState::Missing);
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
            // Re-check everything so the summary shows the real post-install state.
            for i in 0..self.items.len() {
                self.states[i] = match run_check(&self.items[i].check) {
                    Some(d) => ItemState::Installed(d),
                    None => ItemState::Missing,
                };
            }
            self.screen = Screen::Summary;
        }
    }
}

fn run_check(check: &str) -> Option<String> {
    let out = Command::new("bash").arg("-lc").arg(check).output().ok()?;
    if out.status.success() {
        let detail = String::from_utf8_lossy(&out.stdout).trim().to_string();
        Some(if detail.is_empty() { "installed".into() } else { detail })
    } else {
        None
    }
}

/// Runs install steps sequentially on a background thread, streaming output.
/// In dry-run mode nothing is executed — each step's command is only printed.
fn run_installer(work: Vec<(String, String)>, tx: Sender<Msg>, dry_run: bool) {
    for (title, cmd) in work {
        let _ = tx.send(Msg::StepStart);
        if dry_run {
            let _ = tx.send(Msg::Line(format!("[dry-run] {title} — would run:")));
            let _ = tx.send(Msg::Line(format!("  $ {cmd}")));
            thread::sleep(Duration::from_millis(400)); // let the UI show progress
            let _ = tx.send(Msg::StepDone(true));
            continue;
        }
        let ok = match Command::new("bash")
            .arg("-lc")
            .arg(&cmd)
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

    enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;

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
                        app.start_scan();
                    } else {
                        return Ok(());
                    }
                }
                _ => {}
            },
            Screen::Scan => match key.code {
                KeyCode::Char('q') => return Ok(()),
                KeyCode::Esc => app.screen = Screen::Welcome,
                KeyCode::Up | KeyCode::Char('k') => move_sel(&mut app.list_state, -1, app.items.len()),
                KeyCode::Down | KeyCode::Char('j') => move_sel(&mut app.list_state, 1, app.items.len()),
                KeyCode::Char(' ') => {
                    if app.scan_done {
                        if let Some(i) = app.list_state.selected() {
                            app.selected[i] = !app.selected[i];
                        }
                    }
                }
                KeyCode::Char('r') => {
                    if app.scan_done {
                        app.start_scan();
                    }
                }
                KeyCode::Enter
                    if app.scan_done => {
                        app.start_install();
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

fn move_sel(state: &mut ListState, delta: i32, len: usize) {
    if len == 0 {
        return;
    }
    let cur = state.selected().unwrap_or(0) as i32;
    let next = (cur + delta).rem_euclid(len as i32) as usize;
    state.select(Some(next));
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

fn draw(f: &mut Frame, app: &mut App) {
    match app.screen {
        Screen::Welcome => draw_welcome(f, app),
        Screen::Scan => draw_scan(f, app),
        Screen::Install => draw_install(f, app),
        Screen::Summary => draw_summary(f, app),
    }
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

fn draw_scan(f: &mut Frame, app: &mut App) {
    let (body, footer) = chrome(f, app, "What you have vs. what we use");

    let items: Vec<ListItem> = app
        .items
        .iter()
        .enumerate()
        .map(|(i, sw)| {
            let (icon, style) = match &app.states[i] {
                ItemState::Checking => ("⏳ checking…".to_string(), theme::dim()),
                ItemState::Installed(d) => (format!("✓ {d}"), theme::good()),
                ItemState::Missing => ("✗ missing".to_string(), theme::bad()),
            };
            let mark = if !app.scan_done {
                "   "
            } else if app.selected[i] {
                "[x]"
            } else {
                "[ ]"
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!(" {mark} "), theme::border()),
                Span::styled(format!("{:<22}", sw.name), Style::new().bold()),
                Span::styled(icon, style),
            ]))
        })
        .collect();

    let [list_area, detail_area] =
        Layout::vertical([Constraint::Min(4), Constraint::Length(4)]).areas(body);

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme::border())
                .title(" Software "),
        )
        .highlight_style(theme::highlight())
        .highlight_symbol(" > ");
    f.render_stateful_widget(list, list_area, &mut app.list_state);

    // Description of the highlighted item.
    let desc = app
        .list_state
        .selected()
        .and_then(|i| app.items.get(i))
        .map(|sw| sw.description)
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

    if app.scan_done {
        let n = app.selected.iter().filter(|s| **s).count();
        hint(f, footer, &format!("space toggle · enter install {n} selected · r rescan · esc back · q quit"));
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
