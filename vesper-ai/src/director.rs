use anyhow::Result;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use vesper_core::{events::DirectorCall, state::GameState, world::WORLD_BIBLE};

const DIRECTOR_SYSTEM: &str = "\
You are the Director of VESPER, a survival horror game set in Ash Hollow. \
You control the narrative exclusively through tool calls — never write prose. \
The Narrator writes the actual text; your job is mechanics and consequences.\n\
\n\
HARD RULES:\n\
- Monsters can only kill at night (kill_npc with cause=monster is ONLY valid when phase=night).\n\
- Dead NPCs stay dead; do not call npc_action or kill_npc on a dead NPC.\n\
- Phase transitions must be sequential: dawn→day→dusk→night→dawn.\n\
- Call end_turn_narrative before advance_phase.\n\
- Call advance_phase exactly once, as your absolute final tool call.\n\
- Never name or hint at the identity of the two Rememberers in prose seeds.\n\
\n\
TONE: Road-to-hell choices. Every act of kindness has a cost. \
Slow dread, community fracture, small human moments against the dark.";

fn tool_schemas() -> Value {
    json!([
        {
            "name": "npc_action",
            "description": "An NPC takes an action affecting another NPC, the player, or the community. \
                            Call this to shift moods, build or destroy trust, create conflict or connection.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "npc_id":      {"type": "string"},
                    "action_type": {"enum": ["dialogue","aid","betray","breakdown","leave_house","gift","reveal_secret"]},
                    "target":      {"type": "string"},
                    "sanity_delta": {"type": "integer", "minimum": -50, "maximum": 20},
                    "trust_delta":  {"type": "integer", "minimum": -50, "maximum": 20},
                    "summary":     {"type": "string", "maxLength": 240}
                },
                "required": ["npc_id","action_type","summary"],
                "additionalProperties": false
            }
        },
        {
            "name": "kill_npc",
            "description": "Kill or 'turn' an NPC. ONLY valid when phase=night OR cause=voices_arc OR cause=faction_war.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "npc_id": {"type": "string"},
                    "cause":  {"enum": ["monster","betrayal","accident","voices_arc","faction_war","disease"]},
                    "witness_ids": {"type": "array", "items": {"type": "string"}}
                },
                "required": ["npc_id","cause"],
                "additionalProperties": false
            }
        },
        {
            "name": "end_turn_narrative",
            "description": "Required: emit a prose seed for the Narrator. Call this BEFORE advance_phase.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "prose_seed": {"type": "string", "maxLength": 500},
                    "mood": {"enum": ["tense","quiet","grief","dread","relief","confusion"]}
                },
                "required": ["prose_seed","mood"],
                "additionalProperties": false
            }
        },
        {
            "name": "advance_phase",
            "description": "Advance time. Call this exactly once, as your FINAL tool call.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "from": {"enum": ["dawn","day","dusk","night"]},
                    "to":   {"enum": ["dawn","day","dusk","night"]},
                    "day":  {"type": "integer", "minimum": 1, "maximum": 20}
                },
                "required": ["from","to","day"],
                "additionalProperties": false
            }
        }
    ])
}

pub struct DirectorClient {
    http: Client,
    api_key: String,
}

#[derive(Deserialize)]
struct ApiResponse {
    content: Vec<ContentBlock>,
}

#[derive(Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    kind: String,
    name: Option<String>,
    input: Option<Value>,
}

impl DirectorClient {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self { http: Client::new(), api_key: api_key.into() }
    }

    pub async fn run_turn(
        &self,
        model: &str,
        state: &GameState,
        player_action: &str,
        summary: Option<&str>,
    ) -> Result<Vec<DirectorCall>> {
        let state_json = serde_json::to_string_pretty(&state.compact_json())?;
        let user_msg = if let Some(s) = summary {
            format!(
                "CURRENT STATE:\n{state_json}\n\nRECENT EVENTS:\n{s}\n\nPLAYER ACTION: {player_action}"
            )
        } else {
            format!("CURRENT STATE:\n{state_json}\n\nPLAYER ACTION: {player_action}")
        };

        // System is an array so the World Bible block can be prompt-cached.
        let system = json!([
            {
                "type": "text",
                "text": WORLD_BIBLE,
                "cache_control": {"type": "ephemeral"}
            },
            {
                "type": "text",
                "text": DIRECTOR_SYSTEM
            }
        ]);

        let body = json!({
            "model": model,
            "max_tokens": 1024,
            "system": system,
            "tools": tool_schemas(),
            "tool_choice": {"type": "any"},
            "messages": [{"role": "user", "content": user_msg}]
        });

        let resp = self
            .http
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("anthropic-beta", "prompt-caching-2024-07-31")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Director API {status}: {text}");
        }

        let api_resp: ApiResponse = resp.json().await?;
        Ok(api_resp
            .content
            .into_iter()
            .filter(|b| b.kind == "tool_use")
            .filter_map(|b| parse_tool_call(b.name?.as_str(), b.input?))
            .collect())
    }
}

fn parse_tool_call(name: &str, input: Value) -> Option<DirectorCall> {
    match name {
        "advance_phase" => {
            #[derive(Deserialize)]
            struct I { from: String, to: String, day: u32 }
            let i: I = serde_json::from_value(input).ok()?;
            Some(DirectorCall::AdvancePhase { from: i.from, to: i.to, day: i.day })
        }
        "npc_action" => {
            #[derive(Deserialize)]
            struct I {
                npc_id: String,
                action_type: String,
                target: Option<String>,
                #[serde(default)] sanity_delta: i32,
                #[serde(default)] trust_delta: i32,
                summary: String,
            }
            let i: I = serde_json::from_value(input).ok()?;
            Some(DirectorCall::NpcAction {
                npc_id: i.npc_id,
                action_type: i.action_type,
                target: i.target,
                sanity_delta: i.sanity_delta,
                trust_delta: i.trust_delta,
                summary: i.summary,
            })
        }
        "kill_npc" => {
            #[derive(Deserialize)]
            struct I { npc_id: String, cause: String, #[serde(default)] witness_ids: Vec<String> }
            let i: I = serde_json::from_value(input).ok()?;
            Some(DirectorCall::KillNpc { npc_id: i.npc_id, cause: i.cause, witness_ids: i.witness_ids })
        }
        "end_turn_narrative" => {
            #[derive(Deserialize)]
            struct I { prose_seed: String, mood: String }
            let i: I = serde_json::from_value(input).ok()?;
            Some(DirectorCall::EndTurnNarrative { prose_seed: i.prose_seed, mood: i.mood })
        }
        _ => None,
    }
}
