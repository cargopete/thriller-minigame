/// Tool calls the Director emits. The rules guard validates these before they
/// are applied to the DB and in-memory GameState.
#[derive(Debug, Clone)]
pub enum DirectorCall {
    AdvancePhase {
        from: String,
        to: String,
        day: u32,
    },
    NpcAction {
        npc_id: String,
        action_type: String,
        target: Option<String>,
        sanity_delta: i32,
        trust_delta: i32,
        summary: String,
    },
    KillNpc {
        npc_id: String,
        cause: String,
        witness_ids: Vec<String>,
    },
    EndTurnNarrative {
        prose_seed: String,
        mood: String,
    },
}
