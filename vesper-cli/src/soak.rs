/// Headless soak test harness.
///
/// Runs N playthroughs with no TUI and no Narrator (saves ~60% API cost).
/// Prints a win-rate / fragment / kill-frequency summary when done.
///
/// Usage:
///   cargo run --bin soak -- [N] [concurrency]
///   cargo run --bin soak -- 10 3
use std::{
    collections::HashMap,
    sync::Arc,
};

use anyhow::Result;
use tokio::sync::Semaphore;
use vesper_ai::{auth::Auth, auditor::AuditorClient, director::DirectorClient};
use vesper_core::{
    events::DirectorCall,
    rules,
    state::{GameState, NpcState, Phase},
};
use vesper_db::{Db, Player};

const DIRECTOR_MODEL: &str = "claude-sonnet-4-6";
const REMEMBERER_IDS: [&str; 2] = ["iris_calloway", "wren_adisa"];
const FRAGMENTS_NEEDED: i32 = 7;
/// Safety cap — a game that runs this long has probably stalled.
const MAX_TURNS: u32 = 80;

// ── Player-choice simulation ─────────────────────────────────────────────────

fn phase_options(phase: &str) -> &'static [&'static str] {
    match phase {
        "dawn" => &[
            "Survey the town square",
            "Visit the diner",
            "Check on a neighbour",
            "Search for supplies",
            "Rest and tend to yourself",
        ],
        "day" => &[
            "Explore the town",
            "Talk to someone",
            "Investigate something strange",
            "Help with community tasks",
            "Follow a lead",
        ],
        "dusk" => &[
            "Gather people inside",
            "Barricade a building",
            "Share what you know",
            "Keep watch",
            "Find somewhere to hide",
        ],
        "night" => &[
            "Stay hidden and wait",
            "Move carefully through the dark",
            "Help someone in danger",
            "Investigate a sound",
            "Do nothing and endure",
        ],
        _ => &["Wait and see"],
    }
}

/// Cheap pseudorandom pick — no external crate needed.
fn pick(run_id: usize, turn: u32, n: usize) -> usize {
    let h = run_id
        .wrapping_mul(2_654_435_761)
        .wrapping_add((turn as usize).wrapping_mul(2_246_822_519));
    h % n
}

// ── Run result ───────────────────────────────────────────────────────────────

struct RunResult {
    run_id: usize,
    won: bool,
    turns: u32,
    final_day: u32,
    outcome: String,
    fragments: [i32; 2], // [iris, wren]
    kills: Vec<String>,
}

// ── SoakEngine ───────────────────────────────────────────────────────────────

struct SoakEngine {
    run_id: usize,
    db: Db,
    director: Arc<DirectorClient>,
    auditor: Arc<AuditorClient>,
    state: GameState,
    turn: u32,
    kills: Vec<String>,
    summary: Option<String>,
}

impl SoakEngine {
    fn new(
        run_id: usize,
        director: Arc<DirectorClient>,
        auditor: Arc<AuditorClient>,
    ) -> Result<Self> {
        let db = Db::open_memory()?;
        let player = Player {
            name: format!("Soak_{run_id}"),
            gender: None,
            age: None,
            interests: vec![],
            backstory: None,
            sanity: 80,
            location: "town".into(),
        };
        db.create_save(&player)?;

        let state = GameState {
            day: 1,
            phase: Phase::Dawn,
            player_name: player.name,
            player_sanity: 80,
            player_location: "town".into(),
            npcs: db.all_npcs()?.into_iter().map(|n| NpcState {
                id:        n.id,
                name:      n.name,
                role:      n.role,
                residence: n.residence,
                sanity:    n.sanity,
                trust:     n.trust,
                status:    n.status,
                fragments: n.fragments,
            }).collect(),
        };

        Ok(Self {
            run_id,
            db,
            director,
            auditor,
            state,
            turn: 0,
            kills: vec![],
            summary: None,
        })
    }

    async fn run(mut self) -> RunResult {
        loop {
            if self.turn >= MAX_TURNS {
                return self.finish(false, "turn_cap".into());
            }

            let opts = phase_options(self.state.phase.as_str());
            let choice = opts[pick(self.run_id, self.turn, opts.len())];

            match self.step(choice).await {
                Ok(Some((won, reason))) => return self.finish(won, reason),
                Ok(None) => {}
                Err(e) => {
                    // Transient API error — log and keep going
                    eprintln!("[run {}] turn {} error: {e}", self.run_id, self.turn);
                }
            }
            self.turn += 1;
        }
    }

    async fn step(&mut self, player_action: &str) -> Result<Option<(bool, String)>> {
        let director = Arc::clone(&self.director);
        let auditor  = Arc::clone(&self.auditor);

        // 1. Director
        let calls = director
            .run_turn(DIRECTOR_MODEL, &self.state, player_action, self.summary.as_deref())
            .await?;

        // 2. Auditor (fails open)
        let approvals = auditor
            .review(&calls, &self.state)
            .await
            .unwrap_or_else(|_| vec![true; calls.len()]);

        // 3. Apply
        let mut prose_seed = String::new();
        for (i, call) in calls.iter().enumerate() {
            if !approvals.get(i).copied().unwrap_or(true) {
                continue;
            }
            if rules::validate(call, &self.state).is_ok() {
                self.apply(call)?;
            }
            if let DirectorCall::EndTurnNarrative { prose_seed: ps, .. } = call {
                prose_seed = ps.clone();
            }
        }

        // 4. Rolling summary every 3 days
        if self.state.phase == Phase::Dawn && self.state.day % 3 == 0 {
            self.maybe_summarise(&auditor).await;
        }

        // 5. Log
        let narrative = if prose_seed.is_empty() { None } else { Some(prose_seed.as_str()) };
        let _ = self.db.log_event(
            self.state.day,
            self.state.phase.as_str(),
            "turn",
            &format!("{{\"action\":{player_action:?}}}"),
            narrative,
        );

        Ok(self.check_outcome())
    }

    async fn maybe_summarise(&mut self, auditor: &AuditorClient) {
        let from = self.state.day.saturating_sub(3);
        let events = match self.db.event_log_since(from) {
            Ok(e) if !e.is_empty() => e,
            _ => return,
        };
        let text = events
            .iter()
            .map(|e| format!("Day {}, {}: {}", e.day, e.phase, e.payload_json))
            .collect::<Vec<_>>()
            .join("\n");
        if let Ok(s) = auditor.summarise(&text, self.summary.as_deref()).await {
            if !s.is_empty() {
                let _ = self.db.save_summary(self.state.day, &s);
                self.summary = Some(s);
            }
        }
    }

    fn apply(&mut self, call: &DirectorCall) -> Result<()> {
        match call {
            DirectorCall::AdvancePhase { to, day, .. } => {
                self.state.phase = Phase::from_str(to);
                self.state.day = *day;
                self.db.advance_phase(*day, to)?;
            }
            DirectorCall::NpcAction { npc_id, sanity_delta, trust_delta, .. } => {
                if let Some(n) = self.state.npcs.iter_mut().find(|n| &n.id == npc_id) {
                    n.sanity = (n.sanity + sanity_delta).clamp(0, 100);
                    n.trust  = (n.trust  + trust_delta).clamp(0, 100);
                }
                self.db.apply_npc_delta(npc_id, *sanity_delta, *trust_delta)?;
            }
            DirectorCall::KillNpc { npc_id, .. } => {
                if let Some(n) = self.state.npcs.iter_mut().find(|n| &n.id == npc_id) {
                    n.status = "dead".into();
                    self.kills.push(n.name.clone());
                }
                self.db.set_npc_status(npc_id, "dead")?;
            }
            DirectorCall::GrantFragment { npc_id, .. } => {
                let total = self.db.grant_fragment(npc_id)?;
                if let Some(n) = self.state.npcs.iter_mut().find(|n| &n.id == npc_id) {
                    n.fragments = total;
                }
            }
            DirectorCall::EndTurnNarrative { .. } => {}
        }
        Ok(())
    }

    fn check_outcome(&self) -> Option<(bool, String)> {
        let rememberers: Vec<_> = self
            .state
            .npcs
            .iter()
            .filter(|n| REMEMBERER_IDS.contains(&n.id.as_str()))
            .collect();

        if rememberers.len() == 2
            && rememberers.iter().all(|n| n.status == "alive" && n.fragments >= FRAGMENTS_NEEDED)
        {
            return Some((true, "both_rememberers_complete".into()));
        }

        if let Some(dead) = rememberers.iter().find(|n| n.status != "alive") {
            return Some((false, format!("rememberer_dead:{}", dead.id)));
        }

        if self.state.day > 17
            || (self.state.day == 17 && self.state.phase == Phase::Night)
        {
            return Some((false, "time_expired".into()));
        }

        None
    }

    fn frag(&self, id: &str) -> i32 {
        self.state.npcs.iter().find(|n| n.id == id).map(|n| n.fragments).unwrap_or(0)
    }

    fn finish(self, won: bool, outcome: String) -> RunResult {
        RunResult {
            run_id: self.run_id,
            won,
            turns: self.turn,
            final_day: self.state.day,
            outcome,
            fragments: [self.frag("iris_calloway"), self.frag("wren_adisa")],
            kills: self.kills,
        }
    }
}

// ── Summary printer ──────────────────────────────────────────────────────────

fn print_summary(results: &[RunResult]) {
    let n = results.len();
    if n == 0 {
        println!("No results.");
        return;
    }

    let wins  = results.iter().filter(|r| r.won).count();
    let avg_turns = results.iter().map(|r| r.turns as f64).sum::<f64>() / n as f64;
    let avg_day   = results.iter().map(|r| r.final_day as f64).sum::<f64>() / n as f64;

    let avg_iris  = results.iter().map(|r| r.fragments[0] as f64).sum::<f64>() / n as f64;
    let avg_wren  = results.iter().map(|r| r.fragments[1] as f64).sum::<f64>() / n as f64;
    let max_iris  = results.iter().map(|r| r.fragments[0]).max().unwrap_or(0);
    let max_wren  = results.iter().map(|r| r.fragments[1]).max().unwrap_or(0);

    println!();
    println!("  SOAK TEST RESULTS — {n} runs");
    println!("  {}", "━".repeat(52));
    println!("  Win rate        {wins:>3} / {n:<3}  ({:.1}%)", wins as f64 / n as f64 * 100.0);
    println!("  Avg turns       {avg_turns:.1}");
    println!("  Avg final day   {avg_day:.1}");
    println!();
    println!("  Fragments at end (need {FRAGMENTS_NEEDED} each):");
    println!("    iris_calloway   avg {avg_iris:.1}   max {max_iris}");
    println!("    wren_adisa      avg {avg_wren:.1}   max {max_wren}");
    println!();

    // Kill frequency
    let mut kill_freq: HashMap<&str, usize> = HashMap::new();
    for r in results {
        for name in &r.kills {
            *kill_freq.entry(name.as_str()).or_default() += 1;
        }
    }
    if !kill_freq.is_empty() {
        let mut kills: Vec<_> = kill_freq.into_iter().collect();
        kills.sort_by(|a, b| b.1.cmp(&a.1));
        println!("  Most killed NPCs (top 10):");
        for (name, count) in kills.iter().take(10) {
            println!("    {name:<28}  {count}/{n}");
        }
        println!();
    }

    // Outcome breakdown
    let mut outcomes: HashMap<&str, usize> = HashMap::new();
    for r in results {
        *outcomes.entry(r.outcome.as_str()).or_default() += 1;
    }
    let mut outcomes: Vec<_> = outcomes.into_iter().collect();
    outcomes.sort_by(|a, b| b.1.cmp(&a.1));
    println!("  Outcomes:");
    for (reason, count) in &outcomes {
        println!("    {reason:<34}  {count}");
    }
    println!();

    // Per-run log
    println!("  Run log:");
    println!("    {:>4}  {:4}  {:>5}  {:>5}  {:>6}  {}",
        "run", "W/L", "turns", "day", "frags", "outcome");
    for r in results {
        println!("    {:>4}  {:4}  {:>5}  {:>5}  [{},{}]  {}",
            r.run_id,
            if r.won { "WIN" } else { "LOSS" },
            r.turns,
            r.final_day,
            r.fragments[0], r.fragments[1],
            r.outcome);
    }
    println!();
}

// ── Entry point ──────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let auth = Auth::resolve()
        .expect("No authentication found. Set ANTHROPIC_API_KEY or sign in with Claude Code.");

    let mut args = std::env::args().skip(1);
    let n:           usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(5);
    let concurrency: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(2);

    println!("Soak: {n} run(s), concurrency {concurrency}");
    println!("Model: {DIRECTOR_MODEL}  (Narrator skipped)");
    println!();

    // Shared clients — reqwest::Client is arc-internally, safe to clone across tasks
    let director = Arc::new(DirectorClient::new(auth.clone()));
    let auditor  = Arc::new(AuditorClient::new(auth));
    let sem      = Arc::new(Semaphore::new(concurrency));

    let mut tasks = tokio::task::JoinSet::new();

    for run_id in 0..n {
        let dir = Arc::clone(&director);
        let aud = Arc::clone(&auditor);
        let sem = Arc::clone(&sem);
        tasks.spawn(async move {
            let _permit = sem.acquire().await.unwrap();
            println!("→ run {run_id} starting");
            match SoakEngine::new(run_id, dir, aud) {
                Ok(engine) => {
                    let r = engine.run().await;
                    println!("✓ run {:>2}  {}  day {}  {} turns  [{},{}]",
                        r.run_id,
                        if r.won { "WIN " } else { "LOSS" },
                        r.final_day, r.turns,
                        r.fragments[0], r.fragments[1]);
                    r
                }
                Err(e) => {
                    eprintln!("✗ run {run_id} init error: {e}");
                    RunResult {
                        run_id,
                        won: false,
                        turns: 0,
                        final_day: 1,
                        outcome: format!("init_error"),
                        fragments: [0, 0],
                        kills: vec![],
                    }
                }
            }
        });
    }

    let mut results: Vec<RunResult> = Vec::new();
    while let Some(res) = tasks.join_next().await {
        if let Ok(r) = res {
            results.push(r);
        }
    }

    results.sort_by_key(|r| r.run_id);
    print_summary(&results);

    Ok(())
}
