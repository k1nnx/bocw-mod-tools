use super::{build_success_reply, DwService};
use tracing::debug;

pub struct BdStats;

impl DwService for BdStats {
    fn id(&self) -> u8 {
        4
    }

    fn name(&self) -> &str {
        "bdStats"
    }

    fn handle_task(&self, task_id: u8, _data: &[u8]) -> Vec<u8> {
        debug!("[stats] task {} — returning success", task_id);
        build_success_reply(task_id)
    }
}
