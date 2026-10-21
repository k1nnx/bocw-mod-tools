use super::{build_success_reply, DwService};
use tracing::debug;

pub struct BdTitleUtilities;

impl DwService for BdTitleUtilities {
    fn id(&self) -> u8 {
        12
    }

    fn name(&self) -> &str {
        "bdTitleUtilities"
    }

    fn handle_task(&self, task_id: u8, _data: &[u8]) -> Vec<u8> {
        debug!("[title_utilities] task {} — returning success", task_id);
        build_success_reply(task_id)
    }
}
