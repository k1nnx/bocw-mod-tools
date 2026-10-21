use super::{build_success_reply, DwService};
use tracing::debug;

pub struct BdDml;

impl DwService for BdDml {
    fn id(&self) -> u8 {
        27
    }

    fn name(&self) -> &str {
        "bdDML"
    }

    fn handle_task(&self, task_id: u8, _data: &[u8]) -> Vec<u8> {
        debug!("[dml] task {} — returning success", task_id);
        build_success_reply(task_id)
    }
}
