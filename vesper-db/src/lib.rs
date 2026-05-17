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
pub struct NpcSummary {
    pub id: String,
    pub name: String,
    pub role: String,
    pub sanity: i32,
    pub trust: i32,
    pub status: String,
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

    /// Residents of `location` who are alive, ordered by trust desc.
    pub fn nearby_npcs(&self, location: &str, limit: usize) -> Result<Vec<NpcSummary>> {
        let residence = if location == "colony_house" { "colony_house" } else { "town" };
        let mut stmt = self.conn.prepare(
            "SELECT id, name, role, sanity, trust, status \
             FROM npc WHERE residence = ?1 AND status = 'alive' \
             ORDER BY trust DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![residence, limit as i64], |row| {
            Ok(NpcSummary {
                id:     row.get(0)?,
                name:   row.get(1)?,
                role:   row.get(2)?,
                sanity: row.get(3)?,
                trust:  row.get(4)?,
                status: row.get(5)?,
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

    /// Clear all save data (new game).
    pub fn wipe(&self) -> Result<()> {
        self.conn
            .execute_batch("DELETE FROM npc; DELETE FROM player; DELETE FROM save;")?;
        Ok(())
    }
}
