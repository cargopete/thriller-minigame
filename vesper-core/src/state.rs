use serde_json::json;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    Dawn,
    Day,
    Dusk,
    Night,
}

impl Phase {
    pub fn from_str(s: &str) -> Self {
        match s {
            "day" => Self::Day,
            "dusk" => Self::Dusk,
            "night" => Self::Night,
            _ => Self::Dawn,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Dawn => "dawn",
            Self::Day => "day",
            Self::Dusk => "dusk",
            Self::Night => "night",
        }
    }

    pub fn label(&self, day: u32) -> String {
        let name = match self {
            Self::Dawn => "Dawn",
            Self::Day => "Day",
            Self::Dusk => "Dusk",
            Self::Night => "Night",
        };
        format!("Day {day}, {name}")
    }
}

#[derive(Debug, Clone)]
pub struct NpcState {
    pub id: String,
    pub name: String,
    pub role: String,
    pub residence: String,
    pub sanity: i32,
    pub trust: i32,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct GameState {
    pub day: u32,
    pub phase: Phase,
    pub player_name: String,
    pub player_sanity: i32,
    pub player_location: String,
    pub npcs: Vec<NpcState>,
}

impl GameState {
    /// Compact JSON representation sent to the Director each turn.
    pub fn compact_json(&self) -> serde_json::Value {
        let alive: Vec<_> = self
            .npcs
            .iter()
            .filter(|n| n.status == "alive")
            .map(|n| {
                json!({
                    "id": n.id,
                    "name": n.name,
                    "location": n.residence,
                    "sanity": n.sanity,
                    "trust": n.trust,
                })
            })
            .collect();

        json!({
            "day": self.day,
            "phase": self.phase.as_str(),
            "player": {
                "name": self.player_name,
                "sanity": self.player_sanity,
                "location": self.player_location,
            },
            "alive_npcs": alive,
        })
    }
}
