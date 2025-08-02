use anyhow::Result;
use futures::FutureExt;
use futures::future::BoxFuture;
use metrics::{counter, gauge, histogram};
use panic_message::get_panic_message;
use std::panic::AssertUnwindSafe;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::task::JoinHandle;
use tokio::time::{MissedTickBehavior, interval, timeout};

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
            gauge!("cron_job_last_run_timestamp_seconds", "name" => name).set(now);

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
                            counter!("cron_job_runs_total", "name" => name, "status" => "success").increment(1);
                            histogram!("cron_job_duration_seconds", "name" => name).record(elapsed);
                            gauge!("cron_job_last_success_timestamp_seconds", "name" => name).set(ts);
                            log::info!("[{name}] ok");
                        },
                        Ok(Ok(Err(e))) => {
                            counter!("cron_job_runs_total", "name" => name, "status" => "error").increment(1);
                            histogram!("cron_job_duration_seconds", "name" => name).record(elapsed);
                            gauge!("cron_job_last_failure_timestamp_seconds", "name" => name).set(ts);
                            log::error!("[{name}] error: {e:?}");
                        },
                        Ok(Err(e)) => {
                            counter!("cron_job_runs_total", "name" => name, "status" => "panic").increment(1);
                            histogram!("cron_job_duration_seconds", "name" => name).record(elapsed);
                            gauge!("cron_job_last_failure_timestamp_seconds", "name" => name).set(ts);
                            let msg = get_panic_message(&e).unwrap_or("non-string panic payload");
                            log::error!("[{name}] panicked: {msg}");
                        },
                        Err(_) => {
                            counter!("cron_job_runs_total", "name" => name, "status" => "timeout").increment(1);
                            gauge!("cron_job_last_failure_timestamp_seconds", "name" => name).set(ts);
                            log::warn!("[{name}] timed out");
                        },
                    }
                }
                _ = ticker.tick() => {
                    let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64();
                    counter!("cron_job_runs_total", "name" => name, "status" => "overrun").increment(1);
                    gauge!("cron_job_last_failure_timestamp_seconds", "name" => name).set(ts);
                    log::warn!("[{name}] overrun; cancelled");
                }
            }
        }
    })
}
