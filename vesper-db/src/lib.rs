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

    /// Clear all save data (new game).
    pub fn wipe(&self) -> Result<()> {
        self.conn
            .execute_batch("DELETE FROM player; DELETE FROM save;")?;
        Ok(())
    }
}
