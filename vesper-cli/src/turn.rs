use std::{path::PathBuf, sync::Arc};

use chrono;

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
const FINALE_MODEL: &str = "claude-opus-4-7";

fn narrator_model(day: u32) -> &'static str {
    if day >= 17 { FINALE_MODEL } else { "claude-sonnet-4-6" }
}

const NARRATOR_SYSTEM: &str = "\
You are the Narrator of VESPER, a survival horror game set in Ash Hollow.\n\
\n\
VOICE\n\
Third-person past tense. Short, declarative sentences. Specific nouns over adjectives. \
Describe only what the player could see, hear, or smell — never interiority unless it \
arrives as a physical sensation. End every scene on a still, concrete image. \
Never use the words epic, journey, adventure, destiny, or chosen.\n\
\n\
LENGTH\n\
Two paragraphs. First (3-4 sentences) sets the scene. \
Second (2-3 sentences) lands one quiet human detail. 120-180 words total.\n\
\n\
ATMOSPHERE\n\
The dread lives in the ordinary: a cold cup of coffee, a door left ajar, \
the way someone laughs a half-beat too quickly. Earn the horror — never announce it. \
When cicadas go silent, name the silence. When the iron bell rings at dusk, end on it.\n\
\n\
HARD RULES\n\
- The player character's name must appear at least once.\n\
- Never hint at any character's hidden identity or special status.\n\
- Never use the words Rememberer, fragment, or memory fragment.\n\
- Never write player choices or next actions.";

pub struct TurnEngine {
    db: Db,
    director: Arc<DirectorClient>,
    auditor: Arc<AuditorClient>,
    narrator: Arc<AnthropicClient>,
    state: GameState,
    action_rx: mpsc::UnboundedReceiver<PlayerAction>,
    msg_tx: mpsc::UnboundedSender<Msg>,
    current_summary: Option<String>,
    runs_dir: PathBuf,
    is_resume: bool,
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
        runs_dir: PathBuf,
        is_resume: bool,
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
            runs_dir,
            is_resume,
        }
    }

    pub async fn run(mut self) {
        // Seed the latest summary from DB (e.g. resumed save)
        self.current_summary = self.db.latest_summary().ok().flatten();

        // Opening narration
        let opening = if self.is_resume {
            format!(
                "{} is back in Ash Hollow. It is {}. \
                 Describe the scene as they pick up where they left off — \
                 one detail of the place, one detail of the light, one thing that feels off.",
                self.state.player_name,
                self.state.phase.label(self.state.day),
            )
        } else {
            format!(
                "{} has just arrived in Ash Hollow. Describe their first moments: the road, \
                 the diner visible through the early light, and one detail that is wrong in a \
                 way they cannot quite name.",
                self.state.player_name
            )
        };
        // Populate sidebar immediately so nearby NPCs show from the start
        let alive = self.db.alive_count().unwrap_or(0);
        let nearby: Vec<NpcBrief> = self
            .state.npcs.iter()
            .filter(|n| n.status == "alive" && n.residence == self.state.player_location)
            .take(6)
            .map(|n| NpcBrief { name: n.name.clone(), role: n.role.clone() })
            .collect();
        let _ = self.msg_tx.send(Msg::SidebarUpdate {
            player_sanity: self.state.player_sanity,
            alive_count: alive,
            nearby,
        });

        let _ = self.msg_tx.send(Msg::Sound(SoundCue::Phase(self.state.phase.as_str().into())));
        let _ = self.msg_tx.send(Msg::NarratorBegin);
        let narrator = Arc::clone(&self.narrator);
        let msg_tx = self.msg_tx.clone();
        if let Err(e) = Self::narrate_with(narrator, msg_tx, narrator_model(self.state.day), &opening).await.map(|_| ()) {
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
                            self.export_run_log(won, &reason);
                            let _ = self.msg_tx.send(Msg::GameOver { won, reason });
                            break;
                        }
                        Ok(None) => {}
                        Err(e) => {
                            let _ = self.msg_tx.send(Msg::Error(e.to_string()));
                            self.push_menu(); // fallback on error
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
        let mut next_actions: Vec<String> = vec![];
        let mut player_sanity_delta: i32 = 0;
        for (i, call) in calls.iter().enumerate() {
            if !approvals.get(i).copied().unwrap_or(true) {
                eprintln!("Auditor vetoed call {i}");
                continue;
            }
            match rules::validate(call, &self.state) {
                Ok(()) => self.apply(call)?,
                Err(e) => eprintln!("Rule violation (skipped): {e}"),
            }
            if let DirectorCall::EndTurnNarrative { prose_seed: ps, mood: m, next_actions: na, location: loc, player_sanity_delta: psd } = call {
                prose_seed = ps.clone();
                mood = m.clone();
                next_actions = na.clone();
                player_sanity_delta = *psd;
                if let Some(new_loc) = loc {
                    self.state.player_location = new_loc.clone();
                    let _ = self.db.update_player_location(new_loc);
                }
            }
        }

        // Apply player sanity delta
        if player_sanity_delta != 0 {
            match self.db.apply_player_sanity_delta(player_sanity_delta) {
                Ok(new_sanity) => self.state.player_sanity = new_sanity,
                Err(e) => eprintln!("[sanity] apply delta failed: {e}"),
            }
        }

        // 4. Narrator writes the scene
        let player = &self.state.player_name;
        let prompt = if prose_seed.is_empty() {
            format!(
                "Player name: {player}. Day {}, {}. They chose: {}. Describe what happens.",
                self.state.day, self.state.phase.as_str(), player_action
            )
        } else {
            format!(
                "Player name: {player}. Day {}, {}. Mood: {}. {}  Player action: {}.",
                self.state.day, self.state.phase.as_str(), mood, prose_seed, player_action
            )
        };
        let narrative_text = Self::narrate_with(narrator, msg_tx.clone(), narrator_model(self.state.day), &prompt).await?;

        // Log the turn with narrative text
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
            Some(&narrative_text),
        );

        // 5. Deterministic phase advance — one player action = one phase step
        let (next_phase_str, next_day): (&str, u32) = match self.state.phase {
            Phase::Dawn  => ("day",   self.state.day),
            Phase::Day   => ("dusk",  self.state.day),
            Phase::Dusk  => ("night", self.state.day),
            Phase::Night => ("dawn",  self.state.day + 1),
        };
        self.state.phase = Phase::from_str(next_phase_str);
        self.state.day = next_day;
        self.db.advance_phase(next_day, next_phase_str)?;
        let _ = self.msg_tx.send(Msg::Sound(SoundCue::Phase(next_phase_str.to_string())));

        // 6. Rolling summary every 3 days (after night→dawn transition)
        if self.state.phase == Phase::Dawn && self.state.day % 3 == 0 {
            self.try_generate_summary().await;
        }

        // 8. Update sidebar
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

        // 9. Send contextual menu options — Director's next_actions, then Haiku fallback, then static
        let menu = if !next_actions.is_empty() {
            next_actions
        } else {
            auditor
                .generate_options(&narrative_text, self.state.phase.as_str(), &self.state.player_location)
                .await
                .unwrap_or_else(|e| {
                    eprintln!("[options] generate_options failed ({e}), using phase defaults");
                    phase_menu_options(self.state.phase.as_str())
                })
        };
        let _ = msg_tx.send(Msg::MenuReady(menu));

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
        model: &'static str,
        prompt: &str,
    ) -> Result<String> {
        let (stx, mut srx) = mpsc::unbounded_channel::<StreamEvent>();
        let prompt = prompt.to_string();
        tokio::spawn(async move {
            let _ = narrator
                .stream(model, Some(NARRATOR_SYSTEM), &[Message::user(prompt)], 1000, stx)
                .await;
        });
        let mut text = String::new();
        while let Some(ev) = srx.recv().await {
            match ev {
                StreamEvent::Delta(t) => {
                    text.push_str(&t);
                    let _ = msg_tx.send(Msg::NarratorDelta(t));
                }
                StreamEvent::Done => { let _ = msg_tx.send(Msg::NarratorDone); break; }
                StreamEvent::Error(e) => {
                    let _ = msg_tx.send(Msg::Error(e.clone()));
                    anyhow::bail!(e);
                }
            }
        }
        Ok(text)
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

        // Win: both Rememberers alive with enough fragments, community still standing
        let alive_count = self.state.npcs.iter().filter(|n| n.status == "alive").count();
        if rememberers.len() == 2
            && rememberers.iter().all(|n| n.status == "alive" && n.fragments >= FRAGMENTS_NEEDED)
            && alive_count >= 25
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

        // Lose: player sanity gone
        if self.state.player_sanity <= 0 {
            return Some((false, "Ash Hollow took what was left of you. You are still here.".into()));
        }

        // Lose: community collapsed (hard threshold)
        if alive_count < 12 {
            return Some((false, format!("The community broke. {alive_count} people remained — not enough.")));
        }

        // Lose: time ran out (past Day 17 night)
        if self.state.day > 17
            || (self.state.day == 17 && self.state.phase == Phase::Night)
        {
            return Some((false, "Day 17 passed. Ash Hollow kept its secret — and kept you.".into()));
        }

        None
    }

    fn export_run_log(&self, won: bool, reason: &str) {
        let md = match self.db.generate_run_markdown(
            &self.state.player_name,
            won,
            reason,
            self.state.day,
            self.state.phase.as_str(),
        ) {
            Ok(s) => s,
            Err(e) => { eprintln!("[run log] generate failed: {e}"); return; }
        };

        if let Err(e) = std::fs::create_dir_all(&self.runs_dir) {
            eprintln!("[run log] could not create runs dir: {e}");
            return;
        }

        let timestamp = chrono::Utc::now().format("%Y-%m-%d_%H-%M");
        let outcome = if won { "win" } else { "loss" };
        let filename = format!("{timestamp}_{outcome}.md");
        let path = self.runs_dir.join(&filename);

        if let Err(e) = std::fs::write(&path, &md) {
            eprintln!("[run log] write failed: {e}");
        } else {
            eprintln!("[run log] saved → {}", path.display());
        }
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
