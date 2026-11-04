//! What each source costs on this machine, as the monitor samples them.
//!
//! `cargo run --release -p pulsar-monitor-core --example source_costs -- [config.toml] [seconds]`
//!
//! Uses the given config, or the user's own, and samples once a second for
//! `seconds` (default 65, enough to include the 30 s and 60 s refreshes).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use pulsar_core::config;
use pulsar_core::sampler::Sampler;
use windows::Win32::Foundation::FILETIME;
use windows::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetProcessHandleCount, GetProcessTimes,
};

struct Usage {
    cpu: Duration,
    private_bytes: usize,
    handles: u32,
}

fn usage() -> Usage {
    let process = unsafe { GetCurrentProcess() };
    let (mut created, mut exited, mut kernel, mut user) = Default::default();
    let _ = unsafe { GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) };
    let ticks = |t: FILETIME| (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
    let mut memory = PROCESS_MEMORY_COUNTERS_EX {
        cb: size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        ..Default::default()
    };
    let _ = unsafe { K32GetProcessMemoryInfo(process, (&raw mut memory).cast(), memory.cb) };
    let mut handles = 0;
    let _ = unsafe { GetProcessHandleCount(process, &mut handles) };
    Usage {
        cpu: Duration::from_nanos((ticks(kernel) + ticks(user)) * 100),
        private_bytes: memory.PrivateUsage,
        handles,
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().map(PathBuf::from).or_else(config::default_path);
    let seconds: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(65);
    let config = path
        .as_deref()
        .and_then(|p| config::read(p).ok())
        .unwrap_or_default();
    println!(
        "config: {}",
        path.as_deref()
            .filter(|p| p.exists())
            .map_or("default".into(), |p| p.display().to_string())
    );

    let start = usage();
    let built = Instant::now();
    let mut sampler = Sampler::from_config(&config);
    let construction = built.elapsed();
    sampler.sample();
    std::thread::sleep(Duration::from_secs(1));
    let warm = usage();

    let timings = sampler.source_timings(seconds, Duration::from_secs(1));
    let end = usage();

    println!("construction {:.2} ms", ms(construction));
    println!(
        "{:<16} {:>8} {:>8} {:>8} {:>8}",
        "source", "median", "p95", "max", "mean"
    );
    let mut total_mean = Duration::ZERO;
    for (id, mut times) in timings {
        times.sort();
        let at = |q: f64| times[((times.len() - 1) as f64 * q).round() as usize];
        let mean = times.iter().sum::<Duration>() / times.len() as u32;
        total_mean += mean;
        println!(
            "{:<16} {:>8.3} {:>8.3} {:>8.3} {:>8.3}",
            format!("{id:?}"),
            ms(at(0.5)),
            ms(at(0.95)),
            ms(at(1.0)),
            ms(mean)
        );
    }
    println!("sum of means {:.3} ms per tick", ms(total_mean));
    println!(
        "process cpu {:.3} ms per tick (construction and warm-up {:.1} ms)",
        ms((end.cpu - warm.cpu) / seconds.max(1)),
        ms(warm.cpu - start.cpu)
    );
    println!(
        "private {:.1} MB -> {:.1} MB, handles {} -> {}",
        warm.private_bytes as f64 / 1048576.0,
        end.private_bytes as f64 / 1048576.0,
        warm.handles,
        end.handles
    );
}
