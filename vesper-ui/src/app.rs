use crate::audio::{SoundCue, SoundEngine};
use crossterm::event::{Event, EventStream, KeyCode, KeyEventKind};
use futures::StreamExt;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    DefaultTerminal, Frame,
};
use tokio::sync::mpsc;

pub struct NpcBrief {
    pub name: String,
    pub role: String,
}

pub enum PlayerAction {
    MenuChoice { index: usize, label: String },
    Quit,
}

pub enum Msg {
    /// Clear the narrative pane and start streaming.
    NarratorBegin,
    NarratorDelta(String),
    NarratorDone,
    Error(String),
    /// Engine finished processing; here are the next menu options.
    MenuReady(Vec<String>),
    SidebarUpdate { player_sanity: i32, alive_count: i64, nearby: Vec<NpcBrief> },
    PhaseLabel(String),
    Sound(SoundCue),
}

enum GameMode {
    Processing,
    AwaitingChoice,
}

pub struct App {
    player_name: String,
    nearby: Vec<NpcBrief>,
    alive_count: i64,
    player_sanity: i32,
    phase_label: String,
    narrative: String,
    streaming: bool,
    scroll: u16,
    status: String,
    mode: GameMode,
    menu_options: Vec<String>,
    quit: bool,
    action_tx: mpsc::UnboundedSender<PlayerAction>,
    sound: Option<SoundEngine>,
}

impl App {
    pub fn new(
        player_name: impl Into<String>,
        alive_count: i64,
        player_sanity: i32,
        phase_label: impl Into<String>,
        action_tx: mpsc::UnboundedSender<PlayerAction>,
    ) -> Self {
        let phase_label = phase_label.into();
        Self {
            player_name: player_name.into(),
            nearby: vec![],
            alive_count,
            player_sanity,
            status: "Entering Ash Hollow…".into(),
            phase_label,
            narrative: String::new(),
            streaming: true,
            scroll: 0,
            mode: GameMode::Processing,
            menu_options: vec![],
            quit: false,
            action_tx,
            sound: SoundEngine::try_init(),
        }
    }

    pub async fn run(
        mut self,
        terminal: &mut DefaultTerminal,
        mut msg_rx: mpsc::UnboundedReceiver<Msg>,
    ) -> anyhow::Result<()> {
        let mut events = EventStream::new();

        loop {
            terminal.draw(|f| self.render(f))?;

            tokio::select! {
                Some(Ok(ev)) = events.next() => {
                    if let Event::Key(key) = ev {
                        if key.kind == KeyEventKind::Press {
                            self.handle_key(key.code);
                        }
                    }
                }
                Some(msg) = msg_rx.recv() => self.handle_msg(msg),
            }

            if self.quit {
                break;
            }
        }

        Ok(())
    }

    fn handle_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('q') | KeyCode::Esc => {
                let _ = self.action_tx.send(PlayerAction::Quit);
                self.quit = true;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.scroll = self.scroll.saturating_add(1);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll = self.scroll.saturating_sub(1);
            }
            KeyCode::Char(c) => {
                if matches!(self.mode, GameMode::AwaitingChoice) {
                    if let Some(digit) = c.to_digit(10) {
                        let idx = digit as usize;
                        // 1-based
                        if idx > 0 && idx <= self.menu_options.len() {
                            let label = self.menu_options[idx - 1].clone();
                            let _ = self.action_tx.send(PlayerAction::MenuChoice {
                                index: idx - 1,
                                label,
                            });
                            self.mode = GameMode::Processing;
                            self.status = "Thinking…".into();
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_msg(&mut self, msg: Msg) {
        match msg {
            Msg::NarratorBegin => {
                self.narrative.clear();
                self.scroll = 0;
                self.streaming = true;
                self.status = "…".into();
            }
            Msg::NarratorDelta(t) => {
                self.narrative.push_str(&t);
            }
            Msg::NarratorDone => {
                self.streaming = false;
                self.status = self.phase_label.clone();
            }
            Msg::Error(e) => {
                self.narrative.push_str(&format!("\n\n[Error: {e}]"));
                self.streaming = false;
                self.status = "Error".into();
            }
            Msg::MenuReady(opts) => {
                self.menu_options = opts;
                self.mode = GameMode::AwaitingChoice;
                if !self.streaming {
                    self.status = self.phase_label.clone();
                }
            }
            Msg::SidebarUpdate { player_sanity, alive_count, nearby } => {
                self.player_sanity = player_sanity;
                self.alive_count = alive_count;
                self.nearby = nearby;
            }
            Msg::PhaseLabel(label) => {
                self.phase_label = label.clone();
                self.status = label;
            }
            Msg::Sound(cue) => {
                if let Some(s) = &self.sound {
                    s.play(cue);
                }
            }
        }
    }

    fn render(&self, frame: &mut Frame) {
        let area = frame.area();

        let outer = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(3)])
            .split(area);

        let inner = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
            .split(outer[0]);

        // Narrative pane
        let spinner = if self.streaming { " ▒" } else { "" };
        let narrative = Paragraph::new(self.narrative.as_str())
            .block(
                Block::default().borders(Borders::ALL).title(Span::styled(
                    format!(" VESPER{spinner} "),
                    Style::default().add_modifier(Modifier::BOLD),
                )),
            )
            .wrap(Wrap { trim: false })
            .scroll((self.scroll, 0))
            .style(Style::default().fg(Color::Gray));
        frame.render_widget(narrative, inner[0]);

        // Sidebar
        let sanity_bars = sanity_bar(self.player_sanity);
        let mut lines: Vec<Line> = vec![
            Line::from(format!(" {}", self.player_name)),
            Line::from(format!(" {}", self.phase_label)),
            Line::from(""),
            Line::from(format!(" Sanity  {} {:>3}", sanity_bars, self.player_sanity)),
            Line::from(format!(" Alive   {:>2} / 55", self.alive_count)),
        ];

        if !self.nearby.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(" Nearby"));
            for npc in &self.nearby {
                lines.push(Line::from(format!("  • {}", npc.name)));
            }
        }

        if matches!(self.mode, GameMode::AwaitingChoice) && !self.menu_options.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                " — choose —",
                Style::default().fg(Color::Yellow),
            )));
            for (i, opt) in self.menu_options.iter().enumerate() {
                lines.push(Line::from(Span::styled(
                    format!(" [{}] {}", i + 1, opt),
                    Style::default().fg(Color::White),
                )));
            }
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!(" {}", self.status),
            Style::default().fg(Color::DarkGray),
        )));

        let sidebar = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(Span::styled(" Status ", Style::default().fg(Color::DarkGray))),
            )
            .wrap(Wrap { trim: false })
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(sidebar, inner[1]);

        // Bottom bar
        let bottom_text = if matches!(self.mode, GameMode::AwaitingChoice) {
            "  [1–5] choose   [↑↓/jk] scroll   [Q/Esc] quit  "
        } else {
            "  Processing…    [↑↓/jk] scroll   [Q/Esc] quit  "
        };
        let menu_bar = Paragraph::new(bottom_text)
            .block(Block::default().borders(Borders::ALL))
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(menu_bar, outer[1]);
    }
}

fn sanity_bar(sanity: i32) -> String {
    let filled = (sanity.clamp(0, 100) / 12) as usize;
    let empty = 8usize.saturating_sub(filled);
    format!("{}{}", "▓".repeat(filled), "░".repeat(empty))
}
