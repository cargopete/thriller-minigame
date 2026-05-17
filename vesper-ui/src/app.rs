use std::sync::Arc;

use crossterm::event::{Event, EventStream, KeyCode, KeyEventKind};
use futures::StreamExt;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, Borders, Paragraph, Wrap},
    DefaultTerminal, Frame,
};
use tokio::sync::mpsc;

use vesper_ai::client::{AnthropicClient, Message, StreamEvent};

const MODEL: &str = "claude-sonnet-4-6";

const SYSTEM: &str = "\
You are the narrator of VESPER, a survival horror game set in Ash Hollow — a town nobody \
planned to visit and nobody can leave. Write in third-person past tense. Short sentences. \
Specific nouns. No metaphors for what is wrong; describe only what the player sees. \
End every passage on a still, concrete image. Never use the words epic, journey, or adventure. \
Three short paragraphs only.";

pub struct NpcBrief {
    pub name: String,
    pub role: String,
}

pub enum Msg {
    NarratorDelta(String),
    NarratorDone,
    Error(String),
}

pub struct App {
    player_name: String,
    nearby: Vec<NpcBrief>,
    alive_count: i64,
    narrative: String,
    streaming: bool,
    scroll: u16,
    status: String,
    quit: bool,
}

impl App {
    pub fn new(
        player_name: impl Into<String>,
        nearby: Vec<NpcBrief>,
        alive_count: i64,
    ) -> Self {
        Self {
            player_name: player_name.into(),
            nearby,
            alive_count,
            narrative: String::new(),
            streaming: true,
            scroll: 0,
            status: "Entering Ash Hollow…".into(),
            quit: false,
        }
    }

    pub async fn run(
        mut self,
        terminal: &mut DefaultTerminal,
        client: Arc<AnthropicClient>,
    ) -> anyhow::Result<()> {
        let (app_tx, mut app_rx) = mpsc::unbounded_channel::<Msg>();

        let opening = format!(
            "{} has just arrived in Ash Hollow. Describe their first moments: the road, \
             the diner visible through the early light, and one detail that is wrong in a \
             way they cannot quite name.",
            self.player_name
        );

        {
            let c = client.clone();
            let tx = app_tx.clone();
            tokio::spawn(async move {
                let (stx, mut srx) = mpsc::unbounded_channel::<StreamEvent>();
                tokio::spawn(async move {
                    let _ = c
                        .stream(MODEL, Some(SYSTEM), &[Message::user(opening)], 600, stx)
                        .await;
                });
                while let Some(ev) = srx.recv().await {
                    let msg = match ev {
                        StreamEvent::Delta(t) => Msg::NarratorDelta(t),
                        StreamEvent::Done => Msg::NarratorDone,
                        StreamEvent::Error(e) => Msg::Error(e),
                    };
                    let done = matches!(msg, Msg::NarratorDone | Msg::Error(_));
                    let _ = tx.send(msg);
                    if done {
                        break;
                    }
                }
            });
        }

        let mut events = EventStream::new();

        loop {
            terminal.draw(|f| self.render(f))?;

            tokio::select! {
                Some(Ok(ev)) = events.next() => {
                    if let Event::Key(key) = ev {
                        if key.kind == KeyEventKind::Press {
                            match key.code {
                                KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
                                KeyCode::Down | KeyCode::Char('j') => {
                                    self.scroll = self.scroll.saturating_add(1);
                                }
                                KeyCode::Up | KeyCode::Char('k') => {
                                    self.scroll = self.scroll.saturating_sub(1);
                                }
                                _ => {}
                            }
                        }
                    }
                }
                Some(msg) = app_rx.recv() => self.handle(msg),
            }

            if self.quit {
                break;
            }
        }

        Ok(())
    }

    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::NarratorDelta(t) => self.narrative.push_str(&t),
            Msg::NarratorDone => {
                self.streaming = false;
                self.status = "Day 1, Dawn  —  Ash Hollow".into();
            }
            Msg::Error(e) => {
                self.narrative.push_str(&format!("\n\n[{}]", e));
                self.streaming = false;
                self.status = "Error".into();
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

        // Status sidebar
        let mut sidebar = format!(
            " {}\n Day 1 / 18\n\n Sanity  ▓▓▓▓▓▓▓▓ {:>2}\n Alive   {:>2} / 55\n",
            self.player_name, 80, self.alive_count
        );

        if !self.nearby.is_empty() {
            sidebar.push_str("\n Nearby\n");
            for npc in &self.nearby {
                sidebar.push_str(&format!("  • {}\n", npc.name));
            }
        }

        sidebar.push_str(&format!(
            "\n {}\n\n [↑↓/jk] scroll\n [Q/Esc] quit",
            self.status
        ));

        let status_pane = Paragraph::new(sidebar.as_str())
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(Span::styled(" Status ", Style::default().fg(Color::DarkGray))),
            )
            .wrap(Wrap { trim: false })
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(status_pane, inner[1]);

        // Menu bar
        let menu = Paragraph::new("  [Q / Esc] quit    [↑↓ / jk] scroll  ")
            .block(Block::default().borders(Borders::ALL))
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(menu, outer[1]);
    }
}
