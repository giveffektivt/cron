use anyhow::Result;
use futures::FutureExt;
use futures::future::BoxFuture;
use panic_message::get_panic_message;
use std::panic::AssertUnwindSafe;
use std::time::Duration;
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
            let run = AssertUnwindSafe(job.run_once()).catch_unwind();
            let timed = timeout(hard_timeout, run);
            tokio::pin!(timed);

            tokio::select! {
                r = &mut timed => {
                    match r {
                        Ok(Ok(Ok(()))) => log::info!("[{name}] ok"),
                        Ok(Ok(Err(e))) => log::error!("[{name}] error: {e:?}"),
                        Ok(Err(e)) => {
                            let msg = get_panic_message(&e).unwrap_or("non-string panic payload");
                            log::error!("[{name}] panicked: {msg}");
                        },
                        Err(_) => log::warn!("[{name}] timed out"),
                    }
                }
                _ = ticker.tick() => {
                    log::warn!("[{name}] overrun; cancelled");
                }
            }
        }
    })
}
