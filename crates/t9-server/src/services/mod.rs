mod anticheat;
mod dml;
mod profiles;
mod stats;
mod title_utilities;
mod object_store;
mod stub;

use std::collections::HashMap;
use t9_common::ByteBuffer;
use tracing::{debug, warn};

pub trait DwService: Send + Sync {
    fn id(&self) -> u8;
    fn name(&self) -> &str;
    fn handle_task(&self, task_id: u8, data: &[u8]) -> Vec<u8>;
}

pub struct ServiceRegistry {
    services: HashMap<u8, Box<dyn DwService>>,
}

impl ServiceRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            services: HashMap::new(),
        };

        registry.register(Box::new(stats::BdStats));
        registry.register(Box::new(profiles::BdProfiles));
        registry.register(Box::new(title_utilities::BdTitleUtilities));
        registry.register(Box::new(stub::StubService::new(15, "bdKeyArchive")));
        registry.register(Box::new(stub::StubService::new(18, "bdBandwidthTest")));
        registry.register(Box::new(stub::StubService::new(23, "bdCounter")));
        registry.register(Box::new(dml::BdDml));
        registry.register(Box::new(stub::StubService::new(28, "bdGroup")));
        registry.register(Box::new(anticheat::BdAnticheat));
        registry.register(Box::new(stub::StubService::new(52, "bdTags")));
        registry.register(Box::new(stub::StubService::new(58, "bdPooledStorage")));
        registry.register(Box::new(stub::StubService::new(67, "bdEventLog")));
        registry.register(Box::new(stub::StubService::new(68, "bdRichPresence")));
        registry.register(Box::new(stub::StubService::new(80, "bdMarketplace")));
        registry.register(Box::new(stub::StubService::new(95, "bdPublisherVariables")));
        registry.register(Box::new(stub::StubService::new(104, "bdMarketingComms")));
        registry.register(Box::new(stub::StubService::new(125, "bdUNK125")));
        registry.register(Box::new(object_store::BdObjectStore));
        registry.register(Box::new(stub::StubService::new(195, "bdLootGeneration")));

        registry
    }

    fn register(&mut self, service: Box<dyn DwService>) {
        let id = service.id();
        debug!("[services] Registered service {} (id={})", service.name(), id);
        self.services.insert(id, service);
    }

    pub fn dispatch(&self, service_id: u8, data: &[u8]) -> Vec<u8> {
        // Parse task_id from the data
        let task_id = if !data.is_empty() { data[0] } else { 0 };

        if let Some(service) = self.services.get(&service_id) {
            debug!(
                "[services] Dispatching to {} (id={}, task={})",
                service.name(),
                service_id,
                task_id
            );
            service.handle_task(task_id, if data.len() > 1 { &data[1..] } else { &[] })
        } else {
            warn!(
                "[services] Unknown service id={}, task={} — sending empty reply",
                service_id, task_id
            );
            build_success_reply(task_id)
        }
    }
}

/// Build a minimal success reply for a service task.
pub fn build_success_reply(task_id: u8) -> Vec<u8> {
    let mut inner = ByteBuffer::new();
    inner.set_use_data_types(false);
    inner.write_uint32(0); // transaction ID
    inner.write_ubyte(task_id);
    inner.write_ubyte(0); // success

    let inner_data = inner.into_buffer();
    let mut reply = ByteBuffer::new();
    reply.set_use_data_types(false);
    reply.write_int32(inner_data.len() as i32);
    reply.write_raw(&inner_data);

    reply.into_buffer()
}
