use std::env;
use std::fs::read_to_string;

use anyhow::Result;
use clap::Parser;
use futures::future::join_all;
use serde::{Deserialize, Serialize};
use sysinfo::{CpuExt, CpuRefreshKind, RefreshKind, System, SystemExt};
use tokio::process::Command;
use tokio::spawn;
use tokio::time::sleep;
use tokio::time::Duration;
use tracing::info;
use tracing::trace;
use tracing_subscriber::fmt;
use tracing_subscriber::prelude::__tracing_subscriber_SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
struct Args {
    /// config file path
    #[clap(short, long)]
    config_file: String,
}
#[derive(Serialize, Deserialize, Clone)]
struct Config {
    // 命令
    command: String,
    args: Vec<String>,
    // 同时执行几个php
    pool: i32,
    // 间隔
    interval: i32,
    // cpu 0-100
    threshold: f32,
}

#[tokio::main]
async fn main() -> Result<(), ()> {
    for (key, value) in env::vars() {
        println!("{key}: {value}");
    }
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();
    let args = Args::parse();
    let config = match read_to_string(&args.config_file) {
        Ok(config) => config,
        Err(e) => {
            println!("failed to read {} {}", args.config_file, e);
            return Err(());
        }
    };

    let tasks = match serde_json::from_str::<Config>(&config) {
        Ok(config) => {
            let mut task = Vec::with_capacity(config.pool as usize);
            for i in 0..config.pool {
                task.push(create_task(
                    i + 1,
                    config.command.clone(),
                    config.args.clone(),
                    config.threshold,
                    config.interval,
                ))
            }
            join_all(task)
        }
        Err(err) => {
            println!("failed to parse {} {}", &config, err);
            return Err(());
        }
    };
    tasks.await;
    Ok(())
}
async fn create_task(
    runner: i32,
    command: String,
    args: Vec<String>,
    threshold: f32,
    interval: i32,
) {
    let mut system =
        System::new_with_specifics(RefreshKind::new().with_cpu(CpuRefreshKind::everything()));
    loop {
        sleep(Duration::from_secs(1)).await;
        system.refresh_cpu();
        let cpu = system.global_cpu_info().cpu_usage();
        trace!("cur cpu: {}", cpu);
        let mut cmd = Command::new(&command);
        let cmd = cmd.args(&args);
        trace!("{:#?}", cmd);
        if cpu < threshold {
            match cmd.spawn() {
                Ok(child) => match child.wait_with_output().await {
                    Ok(output) => {
                        info!("{} succesed {:#?}", runner, output);
                    }
                    Err(e) => info!("{} failed to run: {}", runner, e),
                },
                Err(e) => info!("{} failed to start: {}", runner, e),
            };
        } else {
            info!("{}, sleep", cpu);
            sleep(Duration::from_secs(interval as u64)).await;
        }
        system.refresh_cpu();
    }
}
