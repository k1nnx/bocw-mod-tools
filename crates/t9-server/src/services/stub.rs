use super::{build_success_reply, DwService};
use tracing::debug;

/// Generic stub service that returns success for all tasks.
pub struct StubService {
    service_id: u8,
    service_name: String,
}

impl StubService {
    pub fn new(id: u8, name: &str) -> Self {
        Self {
            service_id: id,
            service_name: name.to_string(),
        }
    }
}

impl DwService for StubService {
    fn id(&self) -> u8 {
        self.service_id
    }

    fn name(&self) -> &str {
        &self.service_name
    }

    fn handle_task(&self, task_id: u8, _data: &[u8]) -> Vec<u8> {
        debug!("[{}] task {} — stub success", self.service_name, task_id);
        build_success_reply(task_id)
    }
}
