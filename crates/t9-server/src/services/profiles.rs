use super::{build_success_reply, DwService};
use tracing::debug;

pub struct BdProfiles;

impl DwService for BdProfiles {
    fn id(&self) -> u8 {
        8
    }

    fn name(&self) -> &str {
        "bdProfiles"
    }

    fn handle_task(&self, task_id: u8, _data: &[u8]) -> Vec<u8> {
        debug!("[profiles] task {} — returning success", task_id);
        build_success_reply(task_id)
    }
}
