# RFC: Configurable Difficulty

**Status:** Draft
**Scope:** vesper-core, vesper-ai, vesper-db, vesper-cli (wizard + soak)

---

## Summary

Introduce a `DifficultyLevel` enum and a `DifficultyConfig` struct that parameterise every
meaningful gameplay lever. Difficulty is chosen during character creation, persisted in the save
file, and threaded through the Director system prompt and the rules guard. No new API calls;
no AI-side changes beyond prompt injection.

---

## Motivation

Current observed win rate is approximately **1 in 10** (10%). That is the target for Hard. The
full target table:

| Level        | Label       | Target win rate |
|--------------|-------------|----------------|
| `Playground` | Playground  | ~90 %          |
| `Easy`       | Easy        | ~60 %          |
| `Intermediate` | Intermediate | ~50 %        |
| `Hard`       | Hard        | ~30 %          |
| `Impossible` | Impossible  | ~5 %           |

Hard is the current (untuned) baseline. Everything else needs explicit parameter work.

---

## Detailed Design

### 1. `DifficultyLevel` enum — `vesper-core/src/difficulty.rs` (new file)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DifficultyLevel {
    Playground,
    Easy,
    Intermediate,
    Hard,
    Impossible,
}

impl DifficultyLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Playground  => "playground",
            Self::Easy        => "easy",
            Self::Intermediate => "intermediate",
            Self::Hard        => "hard",
            Self::Impossible  => "impossible",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "playground"   => Self::Playground,
            "easy"         => Self::Easy,
            "intermediate" => Self::Intermediate,
            "impossible"   => Self::Impossible,
            _              => Self::Hard,
        }
    }

    pub fn config(&self) -> DifficultyConfig {
        DifficultyConfig::for_level(*self)
    }
}
```

Expose from `vesper-core/src/lib.rs` as `pub mod difficulty;`.

---

### 2. `DifficultyConfig` struct — same file

```rust
pub struct DifficultyConfig {
    // ── Win condition ─────────────────────────────────────────────────────────
    /// Fragments each Rememberer needs to win. Default (Hard) = 7.
    pub fragments_needed: i32,

    // ── Rules guard (enforced in pure Rust, no AI involvement) ────────────────
    /// Maximum NPC kills allowed per turn across all causes.
    pub max_kills_per_turn: usize,
    /// Before this day, kill_npc on a Rememberer is rejected by the rules guard.
    pub rememberer_kill_immunity_days: u32,

    // ── Starting conditions ───────────────────────────────────────────────────
    /// Player starting sanity.
    pub player_sanity_start: i32,
    /// Flat bonus added to every NPC's starting trust value (can be negative).
    pub npc_trust_bonus: i32,

    // ── Director prompt injection (see §4) ────────────────────────────────────
    /// Short paragraph appended to the Director's HARD RULES block.
    pub director_difficulty_block: &'static str,
}
```

---

### 3. Parameter table

| Field                          | Playground | Easy | Intermediate | Hard | Impossible |
|-------------------------------|------------|------|-------------|------|------------|
| `fragments_needed`            | 4          | 5    | 6           | 7    | 9          |
| `max_kills_per_turn`          | 1          | 1    | 2           | 2    | 3          |
| `rememberer_kill_immunity_days` | 14       | 10   | 7           | 4    | 0          |
| `player_sanity_start`         | 100        | 90   | 85          | 80   | 70         |
| `npc_trust_bonus`             | +20        | +10  | +5          | 0    | −10        |

Rationale:

- **`fragments_needed`** is the single most powerful lever. Dropping from 7 to 4 cuts the
  accumulation window almost in half. Raising to 9 in Impossible forces the Rememberers to
  survive longer in a harsher world.
- **`max_kills_per_turn`** is enforced hard in the rules guard (pure Rust, no prompt
  engineering required). Playground caps deaths at 1 per turn so a single bad night cannot
  wipe the Rememberers. Impossible allows 3.
- **`rememberer_kill_immunity_days`** gives the Rememberers a protected early window. On
  Playground they are effectively immortal until Day 14; on Impossible the monsters can take
  them on Day 1.
- **`npc_trust_bonus`** shifts the whole NPC trust distribution at seed time. High trust
  means more NPCs cooperate, which generates more safe opportunities for fragment discovery.
  Negative trust on Impossible means the community starts fractured.
- **`player_sanity_start`** affects the narrative flavour more than mechanics (currently no
  hard sanity-death rule), but it feeds into the Director's state context and influences its
  tone. Lower sanity → Director treats the player as more desperate and less trusted.

---

### 4. Director prompt injection

`DifficultyConfig::director_difficulty_block` is a static string appended to the
`DIRECTOR_SYSTEM` constant at runtime (not hard-coded). The Director sees it as part of its
HARD RULES block. One paragraph per level:

**Playground**
```
DIFFICULTY — PLAYGROUND:
Be generous. Grant fragments whenever any plausible opportunity arises — aim for at least
one grant per Rememberer every two turns. Monsters are present but hesitant; kill at most
one NPC per night and never a Rememberer while they remain cautious. The community
cooperates more than it fractures. The player should feel capable.
```

**Easy**
```
DIFFICULTY — EASY:
Grant fragments freely; natural-feeling opportunities should be taken. Aim for roughly one
grant per Rememberer every three turns. Monsters kill one NPC per night on average.
Rememberers are unlucky but rarely targeted directly.
```

**Intermediate**
```
DIFFICULTY — INTERMEDIATE:
Grant fragments when circumstances naturally allow — neither withheld nor forced. Monsters
kill one or two NPCs per night. The Rememberers face real risk but are not singled out.
```

**Hard** *(current behaviour, no change needed to existing prompt)*
```
DIFFICULTY — HARD:
Grant fragments only when circumstances genuinely allow discovery — solitude, unusual
locations, player-facilitated exploration. Monsters are aggressive. The Rememberers are not
protected by fate.
```

**Impossible**
```
DIFFICULTY — IMPOSSIBLE:
Fragments are rare; grant them only at genuine turning points with clear narrative
justification, at most once every four to five turns per Rememberer. Monsters are relentless.
The community fractures faster than it heals. Every choice has a cost.
```

The injection site in `director.rs`: append `config.director_difficulty_block` as a third
element in the `system` JSON array (after the World Bible and the main DIRECTOR_SYSTEM block),
without `cache_control` so it doesn't pollute the cached block.

```rust
let system = json!([
    { "type": "text", "text": WORLD_BIBLE,       "cache_control": {"type":"ephemeral"} },
    { "type": "text", "text": DIRECTOR_SYSTEM    },
    { "type": "text", "text": config.director_difficulty_block }
]);
```

`run_turn` gains a `config: &DifficultyConfig` parameter, or `DifficultyLevel` — caller's
choice.

---

### 5. Rules guard changes — `vesper-core/src/rules.rs`

Two new checks driven by `DifficultyConfig`, which means `validate` needs the config:

```rust
pub fn validate(call: &DirectorCall, state: &GameState, cfg: &DifficultyConfig) -> Result<()>
```

**New check A — kill cap per turn**

The rules guard receives the full slice of calls for a turn (not one at a time) so it can
count. Current API is per-call; that needs to change to:

```rust
pub fn validate_turn(calls: &[DirectorCall], state: &GameState, cfg: &DifficultyConfig) -> Vec<usize>
// returns indices of rejected calls
```

Or keep per-call but pass in a `kills_so_far: usize` counter. The latter is less disruptive
to existing call sites.

```rust
pub fn validate(
    call: &DirectorCall,
    state: &GameState,
    cfg: &DifficultyConfig,
    kills_this_turn: usize,
) -> Result<()>
```

Inside the `KillNpc` arm, add:

```rust
if kills_this_turn >= cfg.max_kills_per_turn {
    anyhow::bail!("kill_npc rejected: max_kills_per_turn ({}) reached", cfg.max_kills_per_turn);
}
```

**New check B — Rememberer kill immunity**

Inside the `KillNpc` arm:

```rust
const REMEMBERER_IDS: &[&str] = &["iris_calloway", "wren_adisa"];
if REMEMBERER_IDS.contains(&npc_id.as_str()) && state.day < cfg.rememberer_kill_immunity_days {
    anyhow::bail!(
        "kill_npc({npc_id}) rejected: Rememberer kill immunity active until day {}",
        cfg.rememberer_kill_immunity_days
    );
}
```

Existing 6 unit tests gain a `cfg: &DifficultyConfig` argument using
`DifficultyLevel::Hard.config()` as the default, so they continue to pass without change.

---

### 6. Win condition parameter

`check_outcome()` in both `turn.rs` and `soak.rs` currently hard-codes `FRAGMENTS_NEEDED: i32
= 7`. Replace with `cfg.fragments_needed` from a `DifficultyConfig` held on the engine.

---

### 7. DB persistence — `vesper-db`

**Schema** — add `difficulty TEXT NOT NULL DEFAULT 'hard'` column to the `save` table via a
migration:

```sql
ALTER TABLE save ADD COLUMN difficulty TEXT NOT NULL DEFAULT 'hard';
```

**`Db` API additions:**

```rust
impl Db {
    pub fn create_save_with_difficulty(&self, player: &Player, difficulty: DifficultyLevel) -> Result<()>;
    pub fn get_difficulty(&self) -> Result<DifficultyLevel>;
}
```

`create_save` (existing) remains unchanged and defaults to `'hard'` at the SQL level for
backwards compatibility with existing saves.

---

### 8. NPC trust bonus — `vesper-db`

`npc_trust_bonus` is applied at seed time in `create_save_with_difficulty`:

```rust
fn seed_npcs_with_bonus(&self, bonus: i32) -> Result<()> {
    // after the normal INSERT for each NPC, run:
    self.conn.execute(
        "UPDATE npcs SET trust = MIN(100, MAX(0, trust + ?1))",
        [bonus],
    )?;
    Ok(())
}
```

This keeps the NPC seed data (vesper-db/src/npcs.rs) unchanged.

---

### 9. Character creation wizard — `vesper-cli/src/main.rs`

Add a difficulty selection step at the end of `run_wizard`, after the backstory prompt:

```
DIFFICULTY

  How unforgiving should Ash Hollow be?

  [1] Playground    — 9 in 10 survive          (fragments needed: 4, monsters hesitant)
  [2] Easy          — 6 in 10 survive          (fragments needed: 5)
  [3] Intermediate  — 5 in 10 survive          (fragments needed: 6)
  [4] Hard          — 3 in 10 survive          (fragments needed: 7) ← default
  [5] Impossible    — 1 in 20 survive          (fragments needed: 9, community fractured)
```

Use `dialoguer::Select` as elsewhere. Default index 3 (Hard).

Return `(Player, DifficultyLevel)` from `run_wizard`; pass to `create_save_with_difficulty`.

When resuming a save, load `DifficultyLevel` from DB and display it in the sidebar:
`── HARD ──` beneath the alive counter.

---

### 10. Soak harness — `vesper-cli/src/soak.rs`

Add `--difficulty <level>` as an optional third argument (default: `hard`):

```
cargo run --bin soak -- 20 4 hard
cargo run --bin soak -- 20 4 easy
```

`SoakEngine` gains a `cfg: DifficultyConfig` field. The `new()` constructor applies
`npc_trust_bonus` to the in-memory DB after seeding. `check_outcome()` uses
`cfg.fragments_needed`. `step()` passes `cfg` to `validate()` and `run_turn()`.

`print_summary` prints the difficulty level at the top:

```
  SOAK TEST RESULTS — 20 runs  [EASY]
```

This makes win-rate tuning mechanical: run soak at each level, adjust parameters, repeat.

---

### 11. Call-site thread — `TurnEngine`

`TurnEngine` in `vesper-cli/src/turn.rs` gains a `cfg: DifficultyConfig` field loaded from
DB on construction. It is passed to:

- `director.run_turn(model, &state, action, summary, &self.cfg)`
- `rules::validate(call, &state, &self.cfg, kills_this_turn)`
- `check_outcome()` uses `self.cfg.fragments_needed`

---

## Data flow summary

```
Wizard selects DifficultyLevel
        │
        ▼
DB: save.difficulty = "easy"
NPC trust values shifted by +10
        │
        ▼
main.rs loads DifficultyLevel → DifficultyConfig
        │
        ├─▶ TurnEngine.cfg
        │       │
        │       ├─▶ Director.run_turn(…, cfg)
        │       │       └─▶ system prompt gets director_difficulty_block injected
        │       │
        │       ├─▶ rules::validate(…, cfg, kills_so_far)
        │       │       └─▶ kill cap + Rememberer immunity enforced in Rust
        │       │
        │       └─▶ check_outcome() uses cfg.fragments_needed
        │
        └─▶ Sidebar shows difficulty label
```

---

## Drawbacks

- `validate()` signature changes break all existing call sites and all 6 existing tests.
  Low risk (all internal) but not zero work.
- Adding `config` to `run_turn` means both `director.rs` and `soak.rs` need updating
  consistently.
- The Director's response to the difficulty block is probabilistic. Actual win rates will
  need soak-testing to confirm targets are met; the parameter values in §3 are starting
  estimates, not guarantees.

---

## Alternatives considered

**A. Difficulty as a multiplier on fragment grant probability**
Rejected. The Director doesn't expose a probability knob; all we can do is prompt-engineer.
The numeric levers (fragments_needed, kill cap, immunity days) are the reliable layer; the
prompt injection is the AI-layer tuning on top.

**B. Separate model per difficulty**
Rejected. Overkill. The same Sonnet 4.6 Director responds well to tone instructions.

**C. Post-hoc win rate adjustment via Auditor**
Rejected. Making the Auditor difficulty-aware would require it to know the Rememberer
identities, which conflicts with the concealment design.

**D. Single `difficulty_factor: f32` float instead of named levels**
Rejected. Named levels map to player expectation language ("Hard", "Easy") and are
serialisable to a fixed string set. A float would be harder to document and present in UI.

---

## Open questions

1. Should difficulty be changeable mid-run, or locked at save creation? Recommendation: locked
   (changing mid-run invalidates the win-rate contract).

2. The Auditor currently hard-codes its veto threshold at 2 kills/turn. Should it also receive
   `max_kills_per_turn` from the config? Probably yes — the Auditor's own kill-count check
   should match the rules guard rather than being a separate constant.

3. `npc_trust_bonus` at seed time shifts all NPCs uniformly. A more granular approach would
   shift only non-Rememberer NPCs. Does it matter? Probably not — the Rememberers' trust
   values affect Director decisions about who to help them, not the rules guard.

4. Impossible mode at `fragments_needed = 9` with `rememberer_kill_immunity_days = 0` may be
   effectively unwinnable in practice (< 1%). Is 5% really achievable or should the
   parameter be `fragments_needed = 8`? Needs soak data.

---

## Implementation order

1. `vesper-core/src/difficulty.rs` — enum + config struct + parameter table
2. `vesper-core/src/rules.rs` — update `validate` signature, add two new checks, fix tests
3. `vesper-db` — migration, `create_save_with_difficulty`, `get_difficulty`, trust bonus
4. `vesper-ai/src/director.rs` — `run_turn` accepts `&DifficultyConfig`, injects block
5. `vesper-cli/src/turn.rs` — `TurnEngine` holds `cfg`, threads it through
6. `vesper-cli/src/main.rs` — wizard difficulty step, sidebar label
7. `vesper-cli/src/soak.rs` — `--difficulty` arg, print level in summary
8. Soak runs: 20 runs × 5 levels to calibrate parameters
