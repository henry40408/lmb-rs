use std::{
    fmt::Display,
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
    initial_run: bool,
    name: String,
    schedule: Schedule,
    script: String,
    store: Option<Store>,
}

impl ScheduleOptions {
    /// Create a new instance of schedule options.
    pub fn new<S>(name: S, script: S, schedule: Schedule) -> Self
    where
        S: Display,
    {
        Self {
            initial_run: false,
            name: name.to_string(),
            script: script.to_string(),
            schedule,
            store: None,
        }
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

/// Schedule a script as a cron job.
pub fn schedule_script(opts: ScheduleOptions) {
    let input = Arc::new(Mutex::new(BufReader::new(stdin())));
    let name = &opts.name;
    let run_task = || {
        let mut e = EvaluationBuilder::with_reader(&opts.script, input.clone());
        e.name(name);
        if let Some(store) = opts.store.clone() {
            e.store(store.clone());
        }
        let e = e.build();
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
