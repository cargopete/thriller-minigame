# VESPER

A survival horror game set in Ash Hollow — a town nobody planned to visit and nobody can leave.

Inspired by MGM+'s *From*. Driven by Claude. Played in your terminal.

---

## What it is

VESPER is a single-player TUI game built in Rust. You arrive in Ash Hollow and must survive 17 in-game days. The town has 55 residents. Every turn, a three-agent AI pipeline decides what happens, writes the scene, and checks its own work.

You won't win. But the two Rememberers might.

---

## How it works

```
Player action
     │
     ▼
 Director (Sonnet 4.6)          ← tool-use only, never prose
     │  proposes NPC actions, deaths, phase advances
     ▼
 Rules Guard (pure Rust)        ← no API calls, offline, fully tested
     │  validates phase transitions, kill timing, NPC liveness
     ▼
 Auditor (Haiku 4.5)            ← cheap second opinion
     │  vetoes bad calls, catches Rememberer leaks
     ▼
 apply() → GameState + SQLite
     │
     ▼
 Narrator (Sonnet 4.6, SSE)     ← streams prose directly to the TUI
     │  never told who the Rememberers are
     ▼
 rolling summary every 3 days (Haiku 4.5)
```

The Director's system prompt includes the World Bible — cached at the Anthropic layer for 1 hour, so repeated calls cost a fraction of a cold start.

---

## Mechanics

| Thing | Detail |
|---|---|
| Days | 17, each divided into dawn / day / dusk / night |
| NPCs | 55 residents across the town and the colony house |
| Phase transitions | Sequential only: dawn → day → dusk → night → dawn |
| Night rule | Monsters can only kill at night |
| Win condition | Both Rememberers alive with 7 memory fragments each by Day 17 |
| Save | Single slot, SQLite at `~/.local/share/vesper/vesper.db` |
| Permadeath | Yes. If a Rememberer dies, the run is over |

---

## Controls

| Key | Action |
|---|---|
| `1`–`5` | Choose from the current menu |
| `↑` / `k` | Scroll narrative up |
| `↓` / `j` | Scroll narrative down |
| `J` | Open/close the journal (all past turns, current session) |
| `Q` / `Esc` | Quit |

---

## Audio

Procedural drone audio via `rodio`. No sample files required.

| Phase | Sound |
|---|---|
| Dawn | 55 Hz + 56.2 Hz, slow LFO |
| Day | 70 Hz + 71.5 Hz |
| Dusk | 45 Hz + 46.1 Hz, faster wobble |
| Night | 28 Hz + 29.3 Hz, glacial LFO — deepest and most unsettling |

Player choices trigger a descending pitch sting. NPC deaths trigger a low-frequency impact. Audio fails gracefully if no device is available.

---

## Setup

```bash
export ANTHROPIC_API_KEY=sk-ant-...
cargo run --release
```

Requires a terminal at least 80×24. The game creates its data directory automatically.

---

## Workspace

```
vesper-cli    binary — entry point, turn engine, wires everything together
vesper-core   pure logic — GameState, DirectorCall, rules guard (6 tests, no API)
vesper-ai     Anthropic clients — Director, Auditor, Narrator (SSE streaming)
vesper-db     SQLite layer — save/resume, NPC state, event log, rolling summaries
vesper-ui     ratatui TUI — App, Journal, GameOver screen, procedural audio
```

---

## AI cost

Rough estimates per full 17-day playthrough:

| Agent | Model | Est. cost |
|---|---|---|
| Director | Sonnet 4.6 | ~$0.80 (World Bible cached after first call) |
| Auditor + summaries | Haiku 4.5 | ~$0.05 |
| Narrator | Sonnet 4.6 | ~$1.20 |
| **Total** | | **~$2.05** |

Cache hits on the World Bible (≈1 100 tokens, 1-hour TTL) reduce Director cost significantly on subsequent turns.

---

## Build status

```
P0 ✓  workspace, streaming client, ratatui skeleton
P1 ✓  dialoguer character creation, rusqlite migrations, save/resume
P2 ✓  55 NPC roster, sidebar, alive counter
P3 ✓  turn loop, Director tool-use, rules guard
P4 ✓  Narrator SSE → NarrativePane (completed in P3)
P5 ✓  Auditor, rolling summary, prompt caching (World Bible)
P6 ✓  Rememberer mechanic, memory fragments, win/lose detection
P7 ✓  ASCII title, phase headers, journal mode, narrative persistence
P8    soak test harness, ~5% win-rate tuning
```
