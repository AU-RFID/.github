//! RFID Lab onboarding TUI (PoC 2: Rust + ratatui).
//!
//! Distributed as a prebuilt binary fetched by public-scripts/onboard-rust.sh.
//! Feature-parity target: public-scripts/onboard-gum.sh (PoC 1).

mod tasks;

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
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};

use tasks::{components, detect, Component, Platform};

enum Screen {
    Menu,
    Pick,
    Running,
    Doctor,
}

enum WorkerMsg {
    StepStart,
    Line(String),
    StepDone(bool),
    AllDone,
}

struct StepStatus {
    title: String,
    state: char, // '·' pending, '…' running, '✓' ok, '✗' failed
}

struct App {
    dry_run: bool,
    platform: Platform,
    components: Vec<Component>,
    screen: Screen,
    menu_state: ListState,
    pick_state: ListState,
    picked: Vec<bool>,
    steps: Vec<StepStatus>,
    current_step: usize,
    log: Vec<String>,
    rx: Option<Receiver<WorkerMsg>>,
    doctor: Vec<(String, bool, String)>, // label, ok, detail
    follow_ups: Vec<String>,
}

impl App {
    fn new(dry_run: bool) -> Self {
        let platform = detect();
        let components = components(&platform);
        let n = components.len();
        let mut menu_state = ListState::default();
        menu_state.select(Some(0));
        let mut pick_state = ListState::default();
        pick_state.select(Some(0));
        App {
            dry_run,
            platform,
            components,
            screen: Screen::Menu,
            menu_state,
            pick_state,
            picked: vec![true; n],
            steps: Vec::new(),
            current_step: 0,
            log: Vec::new(),
            rx: None,
            doctor: Vec::new(),
            follow_ups: Vec::new(),
        }
    }

    fn start_install(&mut self, all: bool) {
        let selected: Vec<usize> = (0..self.components.len())
            .filter(|&i| all || self.picked[i])
            .collect();

        self.steps.clear();
        self.follow_ups.clear();
        let mut work: Vec<(String, String)> = Vec::new();
        for &i in &selected {
            let c = &self.components[i];
            for s in &c.steps {
                self.steps.push(StepStatus { title: s.title.to_string(), state: '·' });
                work.push((s.title.to_string(), s.cmd.clone()));
            }
            for f in &c.follow_up {
                self.follow_ups.push((*f).to_string());
            }
        }
        self.current_step = 0;
        self.log.clear();

        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let dry_run = self.dry_run;
        thread::spawn(move || run_worker(work, tx, dry_run));
        self.screen = Screen::Running;
    }

    fn drain_worker(&mut self) {
        let Some(rx) = &self.rx else { return };
        let mut done = false;
        while let Ok(msg) = rx.try_recv() {
            match msg {
                WorkerMsg::StepStart => {
                    if let Some(s) = self.steps.get_mut(self.current_step) {
                        s.state = '…';
                    }
                }
                WorkerMsg::Line(l) => self.log.push(l),
                WorkerMsg::StepDone(ok) => {
                    if let Some(s) = self.steps.get_mut(self.current_step) {
                        s.state = if ok { '✓' } else { '✗' };
                    }
                    self.current_step += 1;
                }
                WorkerMsg::AllDone => done = true,
            }
        }
        if done {
            self.rx = None;
            self.run_doctor();
        }
    }

    fn run_doctor(&mut self) {
        self.doctor.clear();
        for c in &self.components {
            for chk in &c.checks {
                let out = Command::new("bash").arg("-lc").arg(&chk.cmd).output();
                match out {
                    Ok(o) if o.status.success() => {
                        let detail = String::from_utf8_lossy(&o.stdout).trim().to_string();
                        self.doctor.push((chk.label.to_string(), true, detail));
                    }
                    _ => self.doctor.push((chk.label.to_string(), false, "not found".into())),
                }
            }
        }
        self.screen = Screen::Doctor;
    }
}

/// Runs install steps sequentially on a background thread, streaming output.
/// In dry-run mode nothing is executed — each step's command is only printed.
fn run_worker(work: Vec<(String, String)>, tx: Sender<WorkerMsg>, dry_run: bool) {
    for (title, cmd) in work {
        let _ = tx.send(WorkerMsg::StepStart);
        if dry_run {
            let _ = tx.send(WorkerMsg::Line(format!("[dry-run] {title} — would run:")));
            let _ = tx.send(WorkerMsg::Line(format!("  $ {cmd}")));
            thread::sleep(Duration::from_millis(400)); // let the UI show progress
            let _ = tx.send(WorkerMsg::StepDone(true));
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
                        let _ = tx2.send(WorkerMsg::Line(line));
                    }
                });
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    let _ = tx.send(WorkerMsg::Line(line));
                }
                let _ = h.join();
                child.wait().map(|s| s.success()).unwrap_or(false)
            }
            Err(e) => {
                let _ = tx.send(WorkerMsg::Line(format!("spawn failed: {e}")));
                false
            }
        };
        let _ = tx.send(WorkerMsg::StepDone(ok));
    }
    let _ = tx.send(WorkerMsg::AllDone);
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
        app.drain_worker();
        terminal.draw(|f| draw(f, &mut app))?;

        if !event::poll(Duration::from_millis(100))? {
            continue;
        }
        let Event::Key(key) = event::read()? else { continue };
        if key.kind != KeyEventKind::Press {
            continue;
        }

        match app.screen {
            Screen::Menu => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                KeyCode::Up | KeyCode::Char('k') => move_sel(&mut app.menu_state, -1, 3),
                KeyCode::Down | KeyCode::Char('j') => move_sel(&mut app.menu_state, 1, 3),
                KeyCode::Enter => match app.menu_state.selected().unwrap_or(0) {
                    0 => app.start_install(true),
                    1 => app.screen = Screen::Pick,
                    _ => app.run_doctor(),
                },
                _ => {}
            },
            Screen::Pick => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => app.screen = Screen::Menu,
                KeyCode::Up | KeyCode::Char('k') => {
                    move_sel(&mut app.pick_state, -1, app.components.len())
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    move_sel(&mut app.pick_state, 1, app.components.len())
                }
                KeyCode::Char(' ') => {
                    if let Some(i) = app.pick_state.selected() {
                        app.picked[i] = !app.picked[i];
                    }
                }
                KeyCode::Enter => app.start_install(false),
                _ => {}
            },
            Screen::Running => {
                // installs are not cancellable in the PoC; ignore keys
            }
            Screen::Doctor => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                KeyCode::Enter => app.screen = Screen::Menu,
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

fn draw(f: &mut Frame, app: &mut App) {
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(5),
        Constraint::Length(1),
    ])
    .areas(f.area());

    let title = Paragraph::new(format!(
        "RFID Lab Onboarding — {}{}",
        app.platform.label(),
        if app.dry_run { "  [DRY RUN — nothing will be installed]" } else { "" }
    ))
    .style(Style::new().bold().fg(Color::Magenta))
    .block(Block::default().borders(Borders::ALL));
    f.render_widget(title, header);

    match app.screen {
        Screen::Menu => {
            let items = ["Full setup (everything)", "Pick components", "Doctor (check my environment)"]
                .iter()
                .map(|s| ListItem::new(*s))
                .collect::<Vec<_>>();
            let list = List::new(items)
                .block(Block::default().borders(Borders::ALL).title(" What would you like to do? "))
                .highlight_style(Style::new().bold().bg(Color::Magenta).fg(Color::Black))
                .highlight_symbol(" > ");
            f.render_stateful_widget(list, body, &mut app.menu_state);
            hint(f, footer, "↑/↓ move · enter select · q quit");
        }
        Screen::Pick => {
            let items = app
                .components
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    let mark = if app.picked[i] { "[x]" } else { "[ ]" };
                    ListItem::new(format!("{mark} {}", c.name))
                })
                .collect::<Vec<_>>();
            let list = List::new(items)
                .block(Block::default().borders(Borders::ALL).title(" Components "))
                .highlight_style(Style::new().bold().bg(Color::Magenta).fg(Color::Black))
                .highlight_symbol(" > ");
            f.render_stateful_widget(list, body, &mut app.pick_state);
            hint(f, footer, "space toggle · enter install · esc back");
        }
        Screen::Running => {
            let [left, right] =
                Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)])
                    .areas(body);
            let items = app
                .steps
                .iter()
                .map(|s| {
                    let style = match s.state {
                        '✓' => Style::new().fg(Color::Green),
                        '✗' => Style::new().fg(Color::Red),
                        '…' => Style::new().fg(Color::Yellow).bold(),
                        _ => Style::new().fg(Color::DarkGray),
                    };
                    ListItem::new(format!("{} {}", s.state, s.title)).style(style)
                })
                .collect::<Vec<_>>();
            f.render_widget(
                List::new(items).block(Block::default().borders(Borders::ALL).title(" Steps ")),
                left,
            );

            let visible = right.height.saturating_sub(2) as usize;
            let start = app.log.len().saturating_sub(visible);
            let text = app.log[start..].join("\n");
            f.render_widget(
                Paragraph::new(text)
                    .wrap(Wrap { trim: false })
                    .block(Block::default().borders(Borders::ALL).title(" Output ")),
                right,
            );
            hint(f, footer, "installing… please wait");
        }
        Screen::Doctor => {
            let mut lines: Vec<Line> = app
                .doctor
                .iter()
                .map(|(label, ok, detail)| {
                    if *ok {
                        Line::styled(format!("  ✓ {label}: {detail}"), Style::new().fg(Color::Green))
                    } else {
                        Line::styled(format!("  ✗ {label}: {detail}"), Style::new().fg(Color::Red))
                    }
                })
                .collect();
            if !app.follow_ups.is_empty() {
                lines.push(Line::raw(""));
                lines.push(Line::styled(
                    "  Finish up by running these in a NEW terminal:",
                    Style::new().bold(),
                ));
                for fu in &app.follow_ups {
                    lines.push(Line::styled(format!("    $ {fu}"), Style::new().fg(Color::Cyan)));
                }
            }
            f.render_widget(
                Paragraph::new(lines)
                    .wrap(Wrap { trim: false })
                    .block(Block::default().borders(Borders::ALL).title(" Doctor ")),
                body,
            );
            hint(f, footer, "enter menu · q quit");
        }
    }
}

fn hint(f: &mut Frame, area: Rect, text: &str) {
    f.render_widget(
        Paragraph::new(text).style(Style::new().fg(Color::DarkGray)),
        area,
    );
}
