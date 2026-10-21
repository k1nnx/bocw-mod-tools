use super::{build_success_reply, DwService};
use tracing::debug;

pub struct BdObjectStore;

impl DwService for BdObjectStore {
    fn id(&self) -> u8 {
        193
    }

    fn name(&self) -> &str {
        "bdObjectStore"
    }

    fn handle_task(&self, task_id: u8, _data: &[u8]) -> Vec<u8> {
        debug!("[object_store] task {} — returning success", task_id);
        build_success_reply(task_id)
    }
}
