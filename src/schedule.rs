use cron::Schedule;

use crate::Store;

/// Schedule options.
#[derive(Debug)]
pub struct ScheduleOptions {
    bail: usize,
    initial_run: bool,
    schedule: Schedule,
    store: Option<Store>,
}

impl ScheduleOptions {
    /// Create a new instance of schedule options.
    pub fn new(schedule: Schedule) -> Self {
        Self {
            bail: 0,
            initial_run: false,
            schedule,
            store: None,
        }
    }

    /// Get bail.
    pub fn bail(&self) -> usize {
        self.bail
    }

    /// Get schedule.
    pub fn schedule(&self) -> &Schedule {
        &self.schedule
    }

    /// Set bail. 0 to disable.
    pub fn set_bail(&mut self, bail: usize) -> &mut Self {
        self.bail = bail;
        self
    }

    /// Set initial run.
    pub fn set_initial_run(&mut self, initial_run: bool) -> &mut Self {
        self.initial_run = initial_run;
        self
    }

    /// Set or unset store.
    pub fn set_store(&mut self, store: Option<Store>) -> &mut Self {
        self.store = store;
        self
    }
}
