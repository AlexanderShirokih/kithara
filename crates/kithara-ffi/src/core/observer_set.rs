use kithara::platform::sync::{Arc, Mutex};

use crate::{observer::ItemObserver, types::FfiItemEvent};

pub(crate) type ObserverId = u64;

#[derive(Default)]
struct Registrations {
    next_id: ObserverId,
    entries: Vec<(ObserverId, Arc<dyn ItemObserver>)>,
}

#[derive(Default)]
pub(crate) struct ObserverSet {
    registrations: Mutex<Registrations>,
}

impl ObserverSet {
    pub(crate) fn add(&self, observer: Arc<dyn ItemObserver>) -> ObserverId {
        let mut registrations = self.registrations.lock();
        let id = registrations.next_id;
        registrations.next_id += 1;
        registrations.entries.push((id, observer));
        id
    }

    pub(crate) fn remove(&self, id: ObserverId) {
        self.registrations
            .lock()
            .entries
            .retain(|(known, _)| *known != id);
    }

    fn snapshot(&self) -> Vec<Arc<dyn ItemObserver>> {
        self.registrations
            .lock()
            .entries
            .iter()
            .map(|(_, observer)| Arc::clone(observer))
            .collect()
    }
}

impl ItemObserver for ObserverSet {
    fn on_event(&self, event: FfiItemEvent) {
        for observer in self.snapshot() {
            observer.on_event(event.clone());
        }
    }
}
