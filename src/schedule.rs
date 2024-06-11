use std::{
    io::{stdin, BufReader},
    sync::Arc,
    thread,
};

use chrono::Utc;
use cron::Schedule;
use parking_lot::Mutex;
use tracing::debug;

use crate::{EvaluationBuilder, Store};

/// Schedule options.
#[derive(Debug)]
pub struct ScheduleOptions {
    /// Run the script immediately after startup.
    pub initial_run: bool,
    /// Name.
    pub name: String,
    /// Schedule.
    pub schedule: Schedule,
    /// Script.
    pub script: String,
    /// Store.
    pub store: Store,
}

/// Schedule a script as a cron job.
pub fn schedule_script(opts: ScheduleOptions) {
    let input = Arc::new(Mutex::new(BufReader::new(stdin())));
    let name = &opts.name;
    let run_task = || {
        let e = EvaluationBuilder::with_reader(&opts.script, input.clone())
            .name(name)
            .store(opts.store.clone())
            .build();
        e.evaluate().expect("failed to evaludate the function");
    };
    if opts.initial_run {
        debug!("initial run");
        run_task();
    }
    loop {
        let now = Utc::now();
        if let Some(next) = opts.schedule.upcoming(Utc).take(1).next() {
            debug!(%next, "next run");
            let elapsed = next - now;
            thread::sleep(elapsed.to_std().expect("failed to fetch next schedule"));
            run_task();
        }
    }
}
