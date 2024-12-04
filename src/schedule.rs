use bon::Builder;
use cron::Schedule;

use crate::Store;

/// Schedule options.
#[derive(Builder, Debug)]
pub struct ScheduleOptions {
    /// Fail after N retries.
    pub bail: usize,
    /// Run script immediately after it's scheduled.
    pub initial_run: bool,
    /// Cron expression.
    pub schedule: Schedule,
    /// Store.
    pub store: Option<Store>,
}
