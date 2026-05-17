use anyhow::Result;
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use vesper_core::{events::DirectorCall, state::GameState};

const AUDITOR_MODEL: &str = "claude-haiku-4-5-20251001";

const AUDITOR_SYSTEM: &str = "\
You are the Auditor for VESPER, a survival horror game. Your job is to review a list of \
proposed Director actions and veto any that violate rules or narrative integrity.\n\
\n\
Return ONLY valid JSON on a single line: {\"vetoed\":[<0-based indices>]}\n\
\n\
VETO a call if:\n\
- kill_npc appears more than twice in one turn\n\
- npc_action targets an NPC who is also being killed this same turn\n\
- end_turn_narrative is absent or appears more than once\n\
- advance_phase is absent, appears more than once, or is not the final call\n\
- prose_seed in end_turn_narrative hints at a Rememberer identity or special status\n\
- Any action targets an NPC listed as dead in the state\n\
\n\
If everything looks correct, return {\"vetoed\":[]}\n\
When in doubt, approve.";

const SUMMARISE_SYSTEM: &str = "\
You are a chronicler for VESPER, a survival horror game. Given a list of recent game events, \
write a single prose paragraph (100–200 words) summarising what has happened in the community \
of Ash Hollow. Third person, past tense. Concrete, specific, no melodrama. \
Focus on deaths, faction shifts, NPC emotional states, and the player's key choices. \
Never name or hint at the two Rememberers.";

pub struct AuditorClient {
    http: Client,
    api_key: String,
}

#[derive(Deserialize)]
struct TextResponse {
    content: Vec<TextBlock>,
}

#[derive(Deserialize)]
struct TextBlock {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: String,
}

#[derive(Deserialize)]
struct VetoResult {
    vetoed: Vec<usize>,
}

impl AuditorClient {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self { http: Client::new(), api_key: api_key.into() }
    }

    /// Review proposed Director calls. Returns a bool per call: true = approved.
    /// Fails open — any error returns all-approved so the game continues.
    pub async fn review(
        &self,
        calls: &[DirectorCall],
        state: &GameState,
    ) -> Result<Vec<bool>> {
        let state_json = serde_json::to_string_pretty(&state.compact_json())?;
        let calls_text = describe_calls(calls);

        let user_msg = format!(
            "CURRENT STATE:\n{state_json}\n\nPROPOSED CALLS:\n{calls_text}"
        );

        let body = json!({
            "model": AUDITOR_MODEL,
            "max_tokens": 128,
            "system": AUDITOR_SYSTEM,
            "messages": [{"role": "user", "content": user_msg}]
        });

        let resp = self
            .http
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Auditor API {status}: {text}");
        }

        let api_resp: TextResponse = resp.json().await?;
        let text = api_resp
            .content
            .into_iter()
            .find(|b| b.kind == "text")
            .map(|b| b.text)
            .unwrap_or_default();

        // Parse {"vetoed": [...]}; on any parse failure, approve all.
        let vetoed: Vec<usize> = serde_json::from_str::<VetoResult>(&text)
            .map(|v| v.vetoed)
            .unwrap_or_default();

        let mut approvals = vec![true; calls.len()];
        for i in vetoed {
            if i < approvals.len() {
                approvals[i] = false;
            }
        }
        Ok(approvals)
    }

    /// Summarise recent events into a rolling paragraph.
    pub async fn summarise(
        &self,
        events_text: &str,
        prev_summary: Option<&str>,
    ) -> Result<String> {
        let user_msg = if let Some(prev) = prev_summary {
            format!("PREVIOUS SUMMARY:\n{prev}\n\nNEW EVENTS:\n{events_text}")
        } else {
            format!("EVENTS:\n{events_text}")
        };

        let body = json!({
            "model": AUDITOR_MODEL,
            "max_tokens": 300,
            "system": SUMMARISE_SYSTEM,
            "messages": [{"role": "user", "content": user_msg}]
        });

        let resp = self
            .http
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Auditor/summarise API {status}: {text}");
        }

        let api_resp: TextResponse = resp.json().await?;
        Ok(api_resp
            .content
            .into_iter()
            .find(|b| b.kind == "text")
            .map(|b| b.text)
            .unwrap_or_default())
    }
}

fn describe_calls(calls: &[DirectorCall]) -> String {
    calls
        .iter()
        .enumerate()
        .map(|(i, call)| match call {
            DirectorCall::AdvancePhase { from, to, day } => {
                format!("{i}: advance_phase(from={from}, to={to}, day={day})")
            }
            DirectorCall::NpcAction { npc_id, action_type, sanity_delta, trust_delta, summary, .. } => {
                format!(
                    "{i}: npc_action(npc={npc_id}, action={action_type}, \
                     sanity_delta={sanity_delta}, trust_delta={trust_delta}, \
                     summary={summary:?})"
                )
            }
            DirectorCall::KillNpc { npc_id, cause, witness_ids } => {
                format!("{i}: kill_npc(npc={npc_id}, cause={cause}, witnesses=[{}])",
                    witness_ids.join(","))
            }
            DirectorCall::EndTurnNarrative { prose_seed, mood } => {
                format!("{i}: end_turn_narrative(mood={mood}, prose_seed={prose_seed:?})")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
