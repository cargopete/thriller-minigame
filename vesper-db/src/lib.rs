mod npcs;

use std::path::Path;

use anyhow::Result;
use rusqlite::{params, Connection};

pub struct Db {
    conn: Connection,
}

#[derive(Debug, Clone)]
pub struct Player {
    pub name: String,
    pub gender: Option<String>,
    pub age: Option<i32>,
    pub interests: Vec<String>,
    pub backstory: Option<String>,
    pub sanity: i32,
    pub location: String,
}

#[derive(Debug, Clone)]
pub struct EventLogRow {
    pub day: u32,
    pub phase: String,
    pub kind: String,
    pub payload_json: String,
    pub narrative_md: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NpcSummary {
    pub id: String,
    pub name: String,
    pub role: String,
    pub sanity: i32,
    pub trust: i32,
    pub status: String,
    pub residence: String,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(include_str!("schema.sql"))?;
        Ok(())
    }

    pub fn has_save(&self) -> Result<bool> {
        let n: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM save", [], |row| row.get(0))?;
        Ok(n > 0)
    }

    pub fn get_save(&self) -> Result<(u32, String)> {
        self.conn
            .query_row(
                "SELECT day, phase FROM save WHERE id = 1",
                [],
                |row| Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?)),
            )
            .map_err(Into::into)
    }

    pub fn create_save(&self, player: &Player) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT OR REPLACE INTO save (id, seed, day, phase, created_at, updated_at) \
             VALUES (1, randomblob(16), 1, 'dawn', ?1, ?2)",
            params![now, now],
        )?;
        self.conn.execute(
            "INSERT OR REPLACE INTO player \
             (id, name, gender, age, interests, backstory, sanity, location) \
             VALUES (1, ?1, ?2, ?3, ?4, ?5, 80, 'town')",
            params![
                player.name,
                player.gender,
                player.age,
                player.interests.join(","),
                player.backstory,
            ],
        )?;
        self.seed_npcs()?;
        Ok(())
    }

    fn seed_npcs(&self) -> Result<()> {
        for n in npcs::NPC_SEEDS {
            self.conn.execute(
                "INSERT OR IGNORE INTO npc \
                 (id, name, age, gender, archetype, residence, role, \
                  sanity, trust, is_rememberer, secret, hook) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                params![
                    n.id, n.name, n.age, n.gender,
                    n.archetype, n.residence, n.role,
                    n.sanity, n.trust, n.is_rememberer,
                    n.secret, n.hook,
                ],
            )?;
        }
        Ok(())
    }

    pub fn load_player(&self) -> Result<Player> {
        let row = self.conn.query_row(
            "SELECT name, gender, age, interests, backstory, sanity, location \
             FROM player WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<i32>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, i32>(5)?,
                    row.get::<_, String>(6)?,
                ))
            },
        )?;

        Ok(Player {
            name: row.0,
            gender: row.1,
            age: row.2,
            interests: row
                .3
                .unwrap_or_default()
                .split(',')
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect(),
            backstory: row.4,
            sanity: row.5,
            location: row.6,
        })
    }

    /// All alive NPCs — used to build GameState at startup.
    pub fn all_npcs(&self) -> Result<Vec<NpcSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, role, sanity, trust, status, residence FROM npc",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(NpcSummary {
                id:        row.get(0)?,
                name:      row.get(1)?,
                role:      row.get(2)?,
                sanity:    row.get(3)?,
                trust:     row.get(4)?,
                status:    row.get(5)?,
                residence: row.get(6)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    /// Residents of `location` who are alive, ordered by trust desc.
    pub fn nearby_npcs(&self, location: &str, limit: usize) -> Result<Vec<NpcSummary>> {
        let residence = if location == "colony_house" { "colony_house" } else { "town" };
        let mut stmt = self.conn.prepare(
            "SELECT id, name, role, sanity, trust, status, residence \
             FROM npc WHERE residence = ?1 AND status = 'alive' \
             ORDER BY trust DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![residence, limit as i64], |row| {
            Ok(NpcSummary {
                id:        row.get(0)?,
                name:      row.get(1)?,
                role:      row.get(2)?,
                sanity:    row.get(3)?,
                trust:     row.get(4)?,
                status:    row.get(5)?,
                residence: row.get(6)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    /// Total alive NPC count.
    pub fn alive_count(&self) -> Result<i64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM npc WHERE status = 'alive'", [], |r| r.get(0))
            .map_err(Into::into)
    }

    /// Advance the save's day and phase.
    pub fn advance_phase(&self, day: u32, phase: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE save SET day = ?1, phase = ?2, updated_at = datetime('now') WHERE id = 1",
            params![day, phase],
        )?;
        Ok(())
    }

    /// Apply stat deltas to an NPC, clamped to 0–100.
    pub fn apply_npc_delta(&self, id: &str, sanity_delta: i32, trust_delta: i32) -> Result<()> {
        self.conn.execute(
            "UPDATE npc SET \
             sanity = MAX(0, MIN(100, sanity + ?1)), \
             trust  = MAX(0, MIN(100, trust  + ?2)) \
             WHERE id = ?3",
            params![sanity_delta, trust_delta, id],
        )?;
        Ok(())
    }

    /// Mark an NPC dead (or turned).
    pub fn set_npc_status(&self, id: &str, status: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE npc SET status = ?1 WHERE id = ?2",
            params![status, id],
        )?;
        Ok(())
    }

    /// Append an event to the log.
    pub fn log_event(
        &self,
        day: u32,
        phase: &str,
        kind: &str,
        payload_json: &str,
        narrative_md: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO event_log (day, phase, kind, payload_json, narrative_md) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![day, phase, kind, payload_json, narrative_md],
        )?;
        Ok(())
    }

    /// Event log rows since (and including) `from_day`.
    pub fn event_log_since(&self, from_day: u32) -> Result<Vec<EventLogRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT day, phase, kind, payload_json, narrative_md \
             FROM event_log WHERE day >= ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map(params![from_day], |row| {
            Ok(EventLogRow {
                day:          row.get(0)?,
                phase:        row.get(1)?,
                kind:         row.get(2)?,
                payload_json: row.get(3)?,
                narrative_md: row.get(4)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    /// Persist a rolling narrative summary.
    pub fn save_summary(&self, through_day: u32, text: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO summary (through_day, text) VALUES (?1, ?2)",
            params![through_day, text],
        )?;
        Ok(())
    }

    /// Fetch the most recent rolling summary, if any.
    pub fn latest_summary(&self) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT text FROM summary ORDER BY id DESC LIMIT 1",
        )?;
        let mut rows = stmt.query([])?;
        Ok(rows.next()?.map(|r| r.get::<_, String>(0)).transpose()?)
    }

    /// Clear all save data (new game).
    pub fn wipe(&self) -> Result<()> {
        self.conn
            .execute_batch("DELETE FROM npc; DELETE FROM player; DELETE FROM save;")?;
        Ok(())
    }
}
