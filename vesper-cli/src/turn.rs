use std::sync::Arc;

use anyhow::Result;
use tokio::sync::mpsc;
use vesper_ai::{
    auditor::AuditorClient,
    client::{AnthropicClient, Message, StreamEvent},
    director::DirectorClient,
};
use vesper_core::{events::DirectorCall, rules, state::{GameState, NpcState, Phase}};
use vesper_db::{Db, EventLogRow};
use vesper_ui::{
    app::{Msg, NpcBrief, PlayerAction},
    audio::SoundCue,
};

const REMEMBERER_IDS: [&str; 2] = ["iris_calloway", "wren_adisa"];
const FRAGMENTS_NEEDED: i32 = 7;

const DIRECTOR_MODEL: &str = "claude-sonnet-4-6";
const NARRATOR_MODEL: &str = "claude-sonnet-4-6";

const NARRATOR_SYSTEM: &str = "\
You are the narrator of VESPER, a survival horror game set in Ash Hollow — a town nobody \
planned to visit and nobody can leave. Write in third-person past tense. Short sentences. \
Specific nouns. No metaphors for what is wrong; describe only what the player sees. \
End every passage on a still, concrete image. Never use the words epic, journey, or adventure. \
Three short paragraphs only.";

pub struct TurnEngine {
    db: Db,
    director: Arc<DirectorClient>,
    auditor: Arc<AuditorClient>,
    narrator: Arc<AnthropicClient>,
    state: GameState,
    action_rx: mpsc::UnboundedReceiver<PlayerAction>,
    msg_tx: mpsc::UnboundedSender<Msg>,
    current_summary: Option<String>,
}

impl TurnEngine {
    pub fn new(
        db: Db,
        director: Arc<DirectorClient>,
        auditor: Arc<AuditorClient>,
        narrator: Arc<AnthropicClient>,
        state: GameState,
        action_rx: mpsc::UnboundedReceiver<PlayerAction>,
        msg_tx: mpsc::UnboundedSender<Msg>,
    ) -> Self {
        Self {
            db,
            director,
            auditor,
            narrator,
            state,
            action_rx,
            msg_tx,
            current_summary: None,
        }
    }

    pub async fn run(mut self) {
        // Seed the latest summary from DB (e.g. resumed save)
        self.current_summary = self.db.latest_summary().ok().flatten();

        // Opening narration
        let opening = format!(
            "{} has just arrived in Ash Hollow. Describe their first moments: the road, \
             the diner visible through the early light, and one detail that is wrong in a \
             way they cannot quite name.",
            self.state.player_name
        );
        let _ = self.msg_tx.send(Msg::Sound(SoundCue::Phase(self.state.phase.as_str().into())));
        let _ = self.msg_tx.send(Msg::NarratorBegin);
        let narrator = Arc::clone(&self.narrator);
        let msg_tx = self.msg_tx.clone();
        if let Err(e) = Self::narrate_with(narrator, msg_tx, &opening).await {
            let _ = self.msg_tx.send(Msg::Error(e.to_string()));
        }
        self.push_menu();

        // Main turn loop
        while let Some(action) = self.action_rx.recv().await {
            match action {
                PlayerAction::Quit => break,
                PlayerAction::MenuChoice { label, .. } => {
                    let _ = self.msg_tx.send(Msg::Sound(SoundCue::Sting));
                    let _ = self.msg_tx.send(Msg::NarratorBegin);
                    match self.process_turn(&label).await {
                        Ok(Some((won, reason))) => {
                            let _ = self.msg_tx.send(Msg::GameOver { won, reason });
                            break;
                        }
                        Ok(None) => self.push_menu(),
                        Err(e) => {
                            let _ = self.msg_tx.send(Msg::Error(e.to_string()));
                            self.push_menu();
                        }
                    }
                }
            }
        }
    }

    async fn process_turn(&mut self, player_action: &str) -> Result<Option<(bool, String)>> {
        // Clone Arcs for use across await points
        let director = Arc::clone(&self.director);
        let auditor  = Arc::clone(&self.auditor);
        let narrator = Arc::clone(&self.narrator);
        let msg_tx   = self.msg_tx.clone();

        // 1. Director decides
        let calls = director
            .run_turn(DIRECTOR_MODEL, &self.state, player_action, self.current_summary.as_deref())
            .await?;

        // 2. Auditor reviews (fails open)
        let approvals = auditor
            .review(&calls, &self.state)
            .await
            .unwrap_or_else(|e| {
                eprintln!("Auditor error (approving all): {e}");
                vec![true; calls.len()]
            });

        // 3. Validate approved calls and apply
        let mut prose_seed = String::new();
        let mut mood = "quiet".to_string();
        for (i, call) in calls.iter().enumerate() {
            if !approvals.get(i).copied().unwrap_or(true) {
                eprintln!("Auditor vetoed call {i}");
                continue;
            }
            match rules::validate(call, &self.state) {
                Ok(()) => self.apply(call)?,
                Err(e) => eprintln!("Rule violation (skipped): {e}"),
            }
            if let DirectorCall::EndTurnNarrative { prose_seed: ps, mood: m } = call {
                prose_seed = ps.clone();
                mood = m.clone();
            }
        }

        // Log the turn
        let payload = format!(
            "{{\"action\":{:?},\"calls\":{},\"approved\":{}}}",
            player_action,
            calls.len(),
            approvals.iter().filter(|&&a| a).count()
        );
        let _ = self.db.log_event(
            self.state.day,
            self.state.phase.as_str(),
            "turn",
            &payload,
            None,
        );

        // 4. Narrator writes the scene
        let prompt = if prose_seed.is_empty() {
            format!(
                "Day {}, {}. The player chose: {}. Describe what happens.",
                self.state.day, self.state.phase.as_str(), player_action
            )
        } else {
            format!(
                "Day {}, {}. Mood: {}. {}  Player action: {}.",
                self.state.day, self.state.phase.as_str(), mood, prose_seed, player_action
            )
        };
        Self::narrate_with(narrator, msg_tx.clone(), &prompt).await?;

        // 5. Rolling summary every 3 days (after night→dawn transition)
        if self.state.phase == Phase::Dawn && self.state.day % 3 == 0 {
            self.try_generate_summary().await;
        }

        // 6. Update sidebar
        let alive = self.db.alive_count().unwrap_or(0);
        let nearby: Vec<NpcBrief> = self
            .state
            .npcs
            .iter()
            .filter(|n| n.status == "alive" && n.residence == self.state.player_location)
            .take(6)
            .map(|n| NpcBrief { name: n.name.clone(), role: n.role.clone() })
            .collect();
        let _ = msg_tx.send(Msg::SidebarUpdate {
            player_sanity: self.state.player_sanity,
            alive_count: alive,
            nearby,
        });
        let _ = msg_tx.send(Msg::PhaseLabel(self.state.phase.label(self.state.day)));

        Ok(self.check_outcome())
    }

    async fn try_generate_summary(&mut self) {
        let from_day = self.state.day.saturating_sub(3);
        let events = match self.db.event_log_since(from_day) {
            Ok(e) => e,
            Err(e) => { eprintln!("event_log_since error: {e}"); return; }
        };
        if events.is_empty() {
            return;
        }
        let events_text = format_events(&events);
        let prev = self.current_summary.as_deref();

        // Auditor doubles as the summariser (both are Haiku)
        match self.auditor.summarise(&events_text, prev).await {
            Ok(summary) if !summary.is_empty() => {
                let _ = self.db.save_summary(self.state.day, &summary);
                self.current_summary = Some(summary);
            }
            Ok(_) => {}
            Err(e) => eprintln!("Summary generation failed: {e}"),
        }
    }

    async fn narrate_with(
        narrator: Arc<AnthropicClient>,
        msg_tx: mpsc::UnboundedSender<Msg>,
        prompt: &str,
    ) -> Result<()> {
        let (stx, mut srx) = mpsc::unbounded_channel::<StreamEvent>();
        let prompt = prompt.to_string();
        tokio::spawn(async move {
            let _ = narrator
                .stream(NARRATOR_MODEL, Some(NARRATOR_SYSTEM), &[Message::user(prompt)], 600, stx)
                .await;
        });
        while let Some(ev) = srx.recv().await {
            match ev {
                StreamEvent::Delta(t)  => { let _ = msg_tx.send(Msg::NarratorDelta(t)); }
                StreamEvent::Done      => { let _ = msg_tx.send(Msg::NarratorDone); break; }
                StreamEvent::Error(e)  => {
                    let _ = msg_tx.send(Msg::Error(e.clone()));
                    anyhow::bail!(e);
                }
            }
        }
        Ok(())
    }

    fn apply(&mut self, call: &DirectorCall) -> Result<()> {
        match call {
            DirectorCall::AdvancePhase { to, day, .. } => {
                self.state.phase = Phase::from_str(to);
                self.state.day = *day;
                self.db.advance_phase(*day, to)?;
                let _ = self.msg_tx.send(Msg::Sound(SoundCue::Phase(to.clone())));
            }
            DirectorCall::NpcAction { npc_id, sanity_delta, trust_delta, .. } => {
                if let Some(npc) = self.state.npcs.iter_mut().find(|n| &n.id == npc_id) {
                    npc.sanity = (npc.sanity + sanity_delta).clamp(0, 100);
                    npc.trust  = (npc.trust  + trust_delta).clamp(0, 100);
                }
                self.db.apply_npc_delta(npc_id, *sanity_delta, *trust_delta)?;
            }
            DirectorCall::KillNpc { npc_id, .. } => {
                if let Some(npc) = self.state.npcs.iter_mut().find(|n| &n.id == npc_id) {
                    npc.status = "dead".into();
                }
                self.db.set_npc_status(npc_id, "dead")?;
                let _ = self.msg_tx.send(Msg::Sound(SoundCue::Death));
            }
            DirectorCall::EndTurnNarrative { .. } => {}
            DirectorCall::GrantFragment { npc_id, .. } => {
                let new_total = self.db.grant_fragment(npc_id)?;
                if let Some(npc) = self.state.npcs.iter_mut().find(|n| &n.id == npc_id) {
                    npc.fragments = new_total;
                }
            }
        }
        Ok(())
    }

    fn check_outcome(&self) -> Option<(bool, String)> {
        let rememberers: Vec<&NpcState> = self
            .state
            .npcs
            .iter()
            .filter(|n| REMEMBERER_IDS.contains(&n.id.as_str()))
            .collect();

        // Win: both alive with enough fragments
        if rememberers.len() == 2
            && rememberers.iter().all(|n| n.status == "alive" && n.fragments >= FRAGMENTS_NEEDED)
        {
            return Some((
                true,
                "The road that brought you here has opened again.".into(),
            ));
        }

        // Lose: a Rememberer died
        if let Some(dead) = rememberers.iter().find(|n| n.status != "alive") {
            return Some((false, format!("{} did not survive.", dead.name)));
        }

        // Lose: time ran out (past Day 17 night)
        if self.state.day > 17
            || (self.state.day == 17 && self.state.phase == Phase::Night)
        {
            return Some((false, "Day 17 passed. Ash Hollow kept its secret — and kept you.".into()));
        }

        None
    }

    fn push_menu(&self) {
        let opts = phase_menu_options(self.state.phase.as_str());
        let _ = self.msg_tx.send(Msg::MenuReady(opts));
    }
}

fn format_events(events: &[EventLogRow]) -> String {
    events
        .iter()
        .map(|e| {
            let detail = e.narrative_md.as_deref().unwrap_or(&e.payload_json);
            format!("Day {}, {}: {}", e.day, e.phase, detail)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn phase_menu_options(phase: &str) -> Vec<String> {
    match phase {
        "dawn" => vec![
            "Survey the town square".into(),
            "Visit the diner".into(),
            "Check on a neighbour".into(),
            "Search for supplies".into(),
            "Rest and tend to yourself".into(),
        ],
        "day" => vec![
            "Explore the town".into(),
            "Talk to someone".into(),
            "Investigate something strange".into(),
            "Help with community tasks".into(),
            "Follow a lead".into(),
        ],
        "dusk" => vec![
            "Gather people inside".into(),
            "Barricade a building".into(),
            "Share what you know".into(),
            "Keep watch".into(),
            "Find somewhere to hide".into(),
        ],
        "night" => vec![
            "Stay hidden and wait".into(),
            "Move carefully through the dark".into(),
            "Help someone in danger".into(),
            "Investigate a sound".into(),
            "Do nothing and endure".into(),
        ],
        _ => vec!["Wait and see".into()],
    }
}
