use std::{io::empty, thread};

use chrono::Utc;
use cron::Schedule;
use tracing::debug;

use crate::{EvaluationBuilder, LamStore};

/// Schedule a script as a cron job
pub fn schedule_script<S>(name: S, script: S, store: LamStore, schedule: Schedule)
where
    S: AsRef<str>,
{
    let name = name.as_ref();
    loop {
        let now = Utc::now();
        if let Some(next) = schedule.upcoming(Utc).take(1).next() {
            debug!(%next, "next run");
            let elapsed = next - now;
            thread::sleep(elapsed.to_std().expect("failed to fetch next schedule"));
            // TODO: figure out how to pass standard input
            let e = EvaluationBuilder::new(script.as_ref(), empty())
                .with_name(name)
                .with_store(store.clone())
                .build();
            e.evaluate().expect("failed to evaludate the function");
        }
    }
}
