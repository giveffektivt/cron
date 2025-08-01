use anyhow::Result;
use futures::FutureExt;
use futures::future::BoxFuture;
use panic_message::get_panic_message;
use std::panic::AssertUnwindSafe;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::task::JoinHandle;
use tokio::time::{MissedTickBehavior, interval, timeout};

use crate::metrics::{
    JOB_DURATION_SECONDS, JOB_LAST_FAILURE_TIMESTAMP, JOB_LAST_RUN_TIMESTAMP,
    JOB_LAST_SUCCESS_TIMESTAMP, JOB_OUTCOME_COUNTER,
};

pub trait CronJob: Send + 'static {
    fn run_once<'a>(&'a mut self) -> BoxFuture<'a, Result<()>>;
}

pub fn spawn_cron<T: CronJob>(name: &'static str, every: Duration, mut job: T) -> JoinHandle<()> {
    let hard_timeout = Duration::from_secs(60);

    tokio::spawn(async move {
        let mut ticker = interval(every);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

        loop {
            ticker.tick().await;

            log::info!("[{name}] start");

            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs_f64();
            JOB_LAST_RUN_TIMESTAMP.with_label_values(&[name]).set(now);

            let run = AssertUnwindSafe(job.run_once()).catch_unwind();
            let timed = timeout(hard_timeout, run);
            tokio::pin!(timed);

            let start = std::time::Instant::now();
            tokio::select! {
                r = &mut timed => {
                    let elapsed = start.elapsed().as_secs_f64();
                    let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64();
                    match r {
                        Ok(Ok(Ok(()))) => {
                            JOB_OUTCOME_COUNTER.with_label_values(&[name, "success"]).inc();
                            JOB_DURATION_SECONDS.with_label_values(&[name]).observe(elapsed);
                            JOB_LAST_SUCCESS_TIMESTAMP.with_label_values(&[name]).set(ts);
                            log::info!("[{name}] ok");
                        },
                        Ok(Ok(Err(e))) => {
                            JOB_OUTCOME_COUNTER.with_label_values(&[name, "error"]).inc();
                            JOB_DURATION_SECONDS.with_label_values(&[name]).observe(elapsed);
                            JOB_LAST_FAILURE_TIMESTAMP.with_label_values(&[name]).set(ts);
                            log::error!("[{name}] error: {e:?}");
                        },
                        Ok(Err(e)) => {
                            JOB_OUTCOME_COUNTER.with_label_values(&[name, "panic"]).inc();
                            JOB_DURATION_SECONDS.with_label_values(&[name]).observe(elapsed);
                            JOB_LAST_FAILURE_TIMESTAMP.with_label_values(&[name]).set(ts);
                            let msg = get_panic_message(&e).unwrap_or("non-string panic payload");
                            log::error!("[{name}] panicked: {msg}");
                        },
                        Err(_) => {
                            JOB_OUTCOME_COUNTER.with_label_values(&[name, "timeout"]).inc();
                            JOB_LAST_FAILURE_TIMESTAMP.with_label_values(&[name]).set(ts);
                            log::warn!("[{name}] timed out");
                        },
                    }
                }
                _ = ticker.tick() => {
                    let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64();
                    JOB_OUTCOME_COUNTER.with_label_values(&[name, "overrun"]).inc();
                    JOB_LAST_FAILURE_TIMESTAMP.with_label_values(&[name]).set(ts);
                    log::warn!("[{name}] overrun; cancelled");
                }
            }
        }
    })
}
