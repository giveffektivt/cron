use once_cell::sync::Lazy;
use prometheus_exporter::prometheus::{
    GaugeVec, HistogramOpts, HistogramVec, IntCounterVec, Opts, register_gauge_vec,
    register_histogram_vec, register_int_counter_vec,
};

pub static JOB_OUTCOME_COUNTER: Lazy<IntCounterVec> = Lazy::new(|| {
    register_int_counter_vec!(
        Opts::new("cron_job_runs_total", "Total cron job runs by outcome"),
        &["job", "status"]
    )
    .unwrap()
});

pub static JOB_DURATION_SECONDS: Lazy<HistogramVec> = Lazy::new(|| {
    let hist_opts = HistogramOpts::new(
        "cron_job_duration_seconds",
        "Duration of cron job runs in seconds",
    )
    .buckets(vec![
        0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
    ]);
    register_histogram_vec!(hist_opts, &["job"]).unwrap()
});

pub static JOB_LAST_RUN_TIMESTAMP: Lazy<GaugeVec> = Lazy::new(|| {
    register_gauge_vec!(
        Opts::new(
            "cron_job_last_run_timestamp_seconds",
            "Unix timestamp of last cron job run"
        ),
        &["job"]
    )
    .unwrap()
});

pub static JOB_LAST_SUCCESS_TIMESTAMP: Lazy<GaugeVec> = Lazy::new(|| {
    register_gauge_vec!(
        Opts::new(
            "cron_job_last_success_timestamp_seconds",
            "Unix timestamp of last successful run"
        ),
        &["job"]
    )
    .unwrap()
});

pub static JOB_LAST_FAILURE_TIMESTAMP: Lazy<GaugeVec> = Lazy::new(|| {
    register_gauge_vec!(
        Opts::new(
            "cron_job_last_failure_timestamp_seconds",
            "Unix timestamp of last failed run (error/panic/timeout/overrun)"
        ),
        &["job"]
    )
    .unwrap()
});
