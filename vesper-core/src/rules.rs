use thiserror::Error;

use crate::events::DirectorCall;
use crate::state::{GameState, Phase};

#[derive(Debug, Error)]
pub enum RuleViolation {
    #[error("kill_npc outside night without valid cause (phase={phase}, cause={cause})")]
    KillOutsideNight { phase: String, cause: String },
    #[error("invalid phase transition: {from} → {to}")]
    InvalidPhaseTransition { from: String, to: String },
    #[error("NPC not found or already dead: {id}")]
    NpcNotFound { id: String },
}

pub fn validate(call: &DirectorCall, state: &GameState) -> Result<(), RuleViolation> {
    match call {
        DirectorCall::KillNpc { npc_id, cause, .. } => {
            let at_night = state.phase == Phase::Night;
            let valid_cause = cause == "voices_arc" || cause == "faction_war";
            if !at_night && !valid_cause {
                return Err(RuleViolation::KillOutsideNight {
                    phase: state.phase.as_str().into(),
                    cause: cause.clone(),
                });
            }
            if !state.npcs.iter().any(|n| n.id == *npc_id && n.status == "alive") {
                return Err(RuleViolation::NpcNotFound { id: npc_id.clone() });
            }
        }
        DirectorCall::AdvancePhase { from, to, .. } => {
            if !is_valid_transition(from, to) {
                return Err(RuleViolation::InvalidPhaseTransition {
                    from: from.clone(),
                    to: to.clone(),
                });
            }
        }
        DirectorCall::NpcAction { npc_id, .. } => {
            if !state.npcs.iter().any(|n| n.id == *npc_id && n.status == "alive") {
                return Err(RuleViolation::NpcNotFound { id: npc_id.clone() });
            }
        }
        DirectorCall::EndTurnNarrative { .. } => {}
        DirectorCall::GrantFragment { npc_id, .. } => {
            if !state.npcs.iter().any(|n| n.id == *npc_id && n.status == "alive") {
                return Err(RuleViolation::NpcNotFound { id: npc_id.clone() });
            }
        }
    }
    Ok(())
}

fn is_valid_transition(from: &str, to: &str) -> bool {
    matches!(
        (from, to),
        ("dawn", "day") | ("day", "dusk") | ("dusk", "night") | ("night", "dawn")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::DirectorCall;
    use crate::state::{GameState, NpcState, Phase};

    fn state(phase: Phase) -> GameState {
        GameState {
            day: 1,
            phase,
            player_name: "Test".into(),
            player_sanity: 80,
            player_location: "town".into(),
            npcs: vec![NpcState {
                id: "lloyd_becker".into(),
                name: "Lloyd Becker".into(),
                role: "mechanic".into(),
                residence: "town".into(),
                sanity: 70,
                trust: 60,
                status: "alive".into(),
                fragments: 0,
            }],
        }
    }

    #[test]
    fn kill_at_night_ok() {
        assert!(validate(
            &DirectorCall::KillNpc {
                npc_id: "lloyd_becker".into(),
                cause: "monster".into(),
                witness_ids: vec![],
            },
            &state(Phase::Night)
        )
        .is_ok());
    }

    #[test]
    fn kill_by_day_rejected() {
        assert!(validate(
            &DirectorCall::KillNpc {
                npc_id: "lloyd_becker".into(),
                cause: "monster".into(),
                witness_ids: vec![],
            },
            &state(Phase::Day)
        )
        .is_err());
    }

    #[test]
    fn kill_voices_arc_any_phase() {
        for phase in [Phase::Dawn, Phase::Day, Phase::Dusk] {
            assert!(
                validate(
                    &DirectorCall::KillNpc {
                        npc_id: "lloyd_becker".into(),
                        cause: "voices_arc".into(),
                        witness_ids: vec![],
                    },
                    &state(phase)
                )
                .is_ok()
            );
        }
    }

    #[test]
    fn valid_phase_transitions() {
        let s = state(Phase::Dawn);
        for (from, to) in [("dawn", "day"), ("day", "dusk"), ("dusk", "night"), ("night", "dawn")] {
            assert!(
                validate(
                    &DirectorCall::AdvancePhase { from: from.into(), to: to.into(), day: 1 },
                    &s
                )
                .is_ok(),
                "should be valid: {from} → {to}"
            );
        }
    }

    #[test]
    fn invalid_phase_transition() {
        assert!(validate(
            &DirectorCall::AdvancePhase { from: "dawn".into(), to: "night".into(), day: 1 },
            &state(Phase::Dawn)
        )
        .is_err());
    }

    #[test]
    fn npc_action_on_dead_npc_rejected() {
        let mut s = state(Phase::Day);
        s.npcs[0].status = "dead".into();
        assert!(validate(
            &DirectorCall::NpcAction {
                npc_id: "lloyd_becker".into(),
                action_type: "dialogue".into(),
                target: None,
                sanity_delta: 0,
                trust_delta: 5,
                summary: "test".into(),
            },
            &s
        )
        .is_err());
    }
}
