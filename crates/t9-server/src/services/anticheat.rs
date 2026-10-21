use super::{build_success_reply, DwService};
use tracing::debug;

pub struct BdAnticheat;

impl DwService for BdAnticheat {
    fn id(&self) -> u8 {
        38
    }

    fn name(&self) -> &str {
        "bdAnticheat"
    }

    fn handle_task(&self, task_id: u8, _data: &[u8]) -> Vec<u8> {
        // All anti-cheat tasks get empty success replies.
        // Task IDs: 2=answerChallenges, 3=reportConsoleID, 4=reportConsoleDetails,
        //           5=answerTOTPChallenge, 6=reportExtendedAuthInfo
        let task_name = match task_id {
            2 => "answerChallenges",
            3 => "reportConsoleID",
            4 => "reportConsoleDetails",
            5 => "answerTOTPChallenge",
            6 => "reportExtendedAuthInfo",
            _ => "unknown",
        };
        debug!("[anticheat] {} (task {}) — ACK", task_name, task_id);
        build_success_reply(task_id)
    }
}
