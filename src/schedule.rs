use cron::Schedule;
use derive_builder::Builder;

use crate::Store;

/// Schedule options.
#[derive(Builder, Debug)]
pub struct ScheduleOptions {
    /// Fail after N retries.
    #[builder(default)]
    pub bail: usize,
    /// Run script immediately after it's scheduled.
    #[builder(default)]
    pub initial_run: bool,
    /// Cron expression.
    pub schedule: Schedule,
    /// Store.
    #[builder(default)]
    pub store: Option<Store>,
}
