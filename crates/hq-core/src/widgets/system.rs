//! "This Mac": CPU, memory, disk, battery, uptime and the busiest processes.
//!
//! `Sampler` keeps one `sysinfo::System` alive between ticks (CPU use is a difference
//! between two readings) and a short history for the sparklines. The app's job ticks it
//! every 5 s (`spawn`); if no job is running, asking for data ticks it on demand.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::{Map, Value};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

use crate::types::{
    Battery, ProcessUse, SystemData, SystemSample, WidgetBody, WidgetData, WidgetKind, WidgetStatus,
};

pub const TICK: Duration = Duration::from_secs(5);
/// Ten minutes of 5 s ticks.
const HISTORY: usize = 120;
/// pmset is a process spawn; the battery changes slowly.
const BATTERY_EVERY: Duration = Duration::from_secs(60);
/// A sample older than this is stale enough to retake when someone asks.
const STALE: Duration = Duration::from_secs(4);
const GB: f64 = 1024.0 * 1024.0 * 1024.0;

pub struct Sampler {
    sys: Option<System>,
    history: VecDeque<(f32, f32)>,
    last: Option<(Instant, SystemSample)>,
    battery: Option<(Instant, Option<Battery>)>,
}

pub type SharedSampler = Arc<Mutex<Sampler>>;

pub fn shared() -> SharedSampler {
    Arc::new(Mutex::new(Sampler::new()))
}

impl Default for Sampler {
    fn default() -> Self {
        Self::new()
    }
}

impl Sampler {
    pub fn new() -> Self {
        Sampler {
            sys: System::new().ok(),
            history: VecDeque::with_capacity(HISTORY),
            last: None,
            battery: None,
        }
    }

    /// Take a reading now and add it to the history.
    pub fn tick(&mut self) -> SystemSample {
        let mut s = SystemSample {
            at: chrono::Local::now().timestamp_millis(),
            ..Default::default()
        };
        if let Some(sys) = self.sys.as_mut() {
            sys.refresh_cpu_usage();
            sys.refresh_memory();
            sys.refresh_processes_specifics(
                ProcessesToUpdate::All,
                true,
                ProcessRefreshKind::nothing().with_cpu().with_memory(),
            );
            s.cpu_pct = sys.global_cpu_usage().clamp(0.0, 100.0);
            s.cpu_count = sys.cpus().len() as u32;
            s.mem_total = sys.total_memory();
            s.mem_used = sys.used_memory().min(s.mem_total);
            s.swap_used = sys.used_swap();
            s.top = top_processes(
                sys.processes().values().map(|p| {
                    (
                        p.name().to_string_lossy().into_owned(),
                        p.cpu_usage(),
                        p.memory(),
                    )
                }),
                3,
            );
        }
        s.uptime_secs = System::uptime().unwrap_or(0);
        s.mem_pressure = pressure_level();
        if let Some((free, total)) = disk_space() {
            s.disk_free = Some(free);
            s.disk_total = Some(total);
            // never below the free figure: purgeable space only adds to it
            s.disk_available = disk_available().map(|a| a.max(free));
        }
        s.battery = self.battery();

        if self.history.len() == HISTORY {
            self.history.pop_front();
        }
        self.history.push_back((s.cpu_pct, mem_pct(&s)));
        self.last = Some((Instant::now(), s.clone()));
        s
    }

    /// The latest reading, retaken if it is stale.
    pub fn current(&mut self) -> SystemSample {
        match &self.last {
            Some((when, s)) if when.elapsed() < STALE => s.clone(),
            _ => self.tick(),
        }
    }

    pub fn history(&self) -> (Vec<f32>, Vec<f32>) {
        self.history.iter().copied().unzip()
    }

    fn battery(&mut self) -> Option<Battery> {
        if let Some((when, b)) = &self.battery {
            if when.elapsed() < BATTERY_EVERY {
                return b.clone();
            }
        }
        let b = read_battery();
        self.battery = Some((Instant::now(), b.clone()));
        b
    }
}

/// Tick `sampler` every `every` on its own thread until `stop` is set. This is the
/// "widget sampling" job of SPEC 2 (`src-tauri/src/jobs.rs` starts it).
/// If the thread can't start, sampling on demand still works; the caller may log it.
pub fn spawn(
    sampler: SharedSampler,
    every: Duration,
    stop: Arc<AtomicBool>,
) -> std::io::Result<JoinHandle<()>> {
    std::thread::Builder::new()
        .name("hq-widgets-sampler".into())
        .spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                if let Ok(mut s) = sampler.lock() {
                    s.tick();
                }
                std::thread::sleep(every);
            }
        })
}

pub fn mem_pct(s: &SystemSample) -> f32 {
    if s.mem_total == 0 {
        0.0
    } else {
        (s.mem_used as f64 / s.mem_total as f64 * 100.0) as f32
    }
}

/// The `n` busiest processes; ties go to the one using more memory.
pub fn top_processes(procs: impl Iterator<Item = (String, f32, u64)>, n: usize) -> Vec<ProcessUse> {
    let mut all: Vec<ProcessUse> = procs
        .filter(|(name, cpu, _)| !name.is_empty() && cpu.is_finite())
        .map(|(name, cpu_pct, mem)| ProcessUse {
            name,
            cpu_pct: (cpu_pct * 10.0).round() / 10.0,
            mem,
        })
        .collect();
    all.sort_by(|a, b| b.cpu_pct.total_cmp(&a.cpu_pct).then(b.mem.cmp(&a.mem)));
    all.truncate(n);
    all
}

/// macOS's own memory pressure (what Activity Monitor's graph colours): the kernel's
/// level 1 = normal, 2 = warn, 4 = critical.
#[cfg(target_os = "macos")]
fn pressure_level() -> Option<String> {
    let mut level: libc::c_int = 0;
    let mut len = std::mem::size_of::<libc::c_int>();
    // SAFETY: a fixed, NUL-terminated name; `level` and `len` are valid for the call.
    let rc = unsafe {
        libc::sysctlbyname(
            c"kern.memorystatus_vm_pressure_level".as_ptr(),
            (&mut level as *mut libc::c_int).cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    (rc == 0).then(|| pressure_name(level).to_string())
}

#[cfg(not(target_os = "macos"))]
fn pressure_level() -> Option<String> {
    None
}

pub fn pressure_name(level: i32) -> &'static str {
    match level {
        4 => "critical",
        2 => "warn",
        _ => "normal",
    }
}

/// Free and total bytes on the volume that holds the user's files. On APFS that is the
/// Data volume; `/` is the sealed, read-only system snapshot.
fn disk_space() -> Option<(u64, u64)> {
    let path = ["/System/Volumes/Data", "/"]
        .into_iter()
        .find(|p| Path::new(p).exists())?;
    let c = std::ffi::CString::new(path).ok()?;
    // SAFETY: zeroed is a valid statvfs; statvfs fills it for a NUL-terminated path.
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::statvfs(c.as_ptr(), &mut st) };
    if rc != 0 {
        return None;
    }
    let frsize = st.f_frsize as u64;
    Some((st.f_bavail as u64 * frsize, st.f_blocks as u64 * frsize))
}

/// Finder's figure: free plus what macOS would purge for an important write. `statvfs`
/// counts only space that is free now, so the two can differ by tens of GB when iCloud
/// keeps local copies.
#[cfg(target_os = "macos")]
fn disk_available() -> Option<u64> {
    use objc2_foundation::{
        NSNumber, NSString, NSURLVolumeAvailableCapacityForImportantUsageKey, NSURL,
    };
    let url = NSURL::fileURLWithPath(&NSString::from_str("/"));
    let mut value = None;
    // SAFETY: the key is a Foundation constant, and its documented value is an NSNumber.
    unsafe {
        url.getResourceValue_forKey_error(
            &mut value,
            NSURLVolumeAvailableCapacityForImportantUsageKey,
        )
    }
    .ok()?;
    let n = value?.downcast::<NSNumber>().ok()?;
    u64::try_from(n.longLongValue()).ok()
}

#[cfg(not(target_os = "macos"))]
fn disk_available() -> Option<u64> {
    None
}

#[cfg(target_os = "macos")]
fn read_battery() -> Option<Battery> {
    let out = std::process::Command::new("/usr/bin/pmset")
        .args(["-g", "batt"])
        .output()
        .ok()?;
    parse_pmset(&String::from_utf8_lossy(&out.stdout))
}

#[cfg(not(target_os = "macos"))]
fn read_battery() -> Option<Battery> {
    None
}

/// `pmset -g batt` output. A Mac without a battery (mini, Studio) has no
/// `InternalBattery` line and gives None.
///
/// ```text
/// Now drawing from 'AC Power'
///  -InternalBattery-0 (id=1234)  95%; charging; 0:40 remaining present: true
/// ```
pub fn parse_pmset(text: &str) -> Option<Battery> {
    let plugged = text.lines().next().is_some_and(|l| l.contains("AC Power"));
    let line = text.lines().find(|l| l.contains("InternalBattery"))?;
    let pct_at = line.find('%')?;
    let digits: String = line[..pct_at]
        .chars()
        .rev()
        .take_while(char::is_ascii_digit)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let pct: u8 = digits.parse().ok()?;
    let state = line[pct_at + 1..]
        .split(';')
        .nth(1)
        .map(|s| s.trim().to_lowercase())
        .unwrap_or_default();
    let charging = state == "charging" || state == "finishing charge";
    Some(Battery {
        pct: pct.min(100),
        charging,
        plugged: plugged || charging,
    })
}

/// Thresholds from the widget's config.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub cpu_warn: f32,
    pub disk_warn_gb: f64,
    pub battery_warn: u8,
}

impl Limits {
    pub fn from_config(cfg: &Map<String, Value>) -> Self {
        let n = |k: &str, d: f64| cfg.get(k).and_then(Value::as_f64).unwrap_or(d);
        Limits {
            cpu_warn: n("cpuWarn", 85.0) as f32,
            disk_warn_gb: n("diskWarnGb", 15.0),
            battery_warn: n("batteryWarn", 20.0).clamp(0.0, 100.0) as u8,
        }
    }
}

/// The widget's status and the reasons for it. CPU is judged on the last few samples so
/// one busy tick doesn't tint the widget; everything else on the latest reading.
pub fn evaluate(s: &SystemSample, cpu_recent: &[f32], lim: &Limits) -> (WidgetStatus, Vec<String>) {
    let mut status = WidgetStatus::Ok;
    let mut notes = Vec::new();
    let mut raise = |to: WidgetStatus, note: String| {
        status = status.max(to);
        notes.push(note);
    };

    let recent = &cpu_recent[cpu_recent.len().saturating_sub(3)..];
    let cpu = if recent.is_empty() {
        s.cpu_pct
    } else {
        recent.iter().sum::<f32>() / recent.len() as f32
    };
    if cpu >= lim.cpu_warn {
        raise(WidgetStatus::Warn, format!("cpu {cpu:.0}%"));
    }

    match s.mem_pressure.as_deref() {
        Some("critical") => raise(WidgetStatus::Bad, "memory pressure critical".into()),
        Some("warn") => raise(WidgetStatus::Warn, "memory pressure high".into()),
        Some(_) => {}
        // no kernel reading: fall back to how full memory is
        None if mem_pct(s) >= 92.0 => raise(
            WidgetStatus::Warn,
            format!("memory {:.0}% used", mem_pct(s)),
        ),
        None => {}
    }

    if let Some(free) = s.disk_free {
        let gb = free as f64 / GB;
        if gb < lim.disk_warn_gb / 4.0 {
            raise(WidgetStatus::Bad, format!("disk {gb:.1} GB free"));
        } else if gb < lim.disk_warn_gb {
            raise(WidgetStatus::Warn, format!("disk {gb:.0} GB free"));
        }
    }

    if let Some(b) = &s.battery {
        if !b.plugged && b.pct <= lim.battery_warn / 2 {
            raise(WidgetStatus::Bad, format!("battery {}%", b.pct));
        } else if !b.plugged && b.pct <= lim.battery_warn {
            raise(WidgetStatus::Warn, format!("battery {}%", b.pct));
        }
    }
    (status, notes)
}

pub fn uptime_text(secs: u64) -> String {
    let (d, h, m) = (secs / 86_400, secs % 86_400 / 3600, secs % 3600 / 60);
    match (d, h) {
        (0, 0) => format!("{m}m"),
        (0, _) => format!("{h}h {m}m"),
        _ => format!("{d}d {h}h"),
    }
}

pub fn widget(sampler: &SharedSampler, cfg: &Map<String, Value>) -> WidgetData {
    let (now, (cpu_history, mem_history)) = {
        let mut s = sampler.lock().unwrap_or_else(|p| p.into_inner());
        let now = s.current();
        (now, s.history())
    };
    let (status, notes) = evaluate(&now, &cpu_history, &Limits::from_config(cfg));
    let mut parts = vec![
        format!("cpu {:.0}%", now.cpu_pct),
        format!("mem {:.0}%", mem_pct(&now)),
    ];
    if let Some(free) = now.disk_free {
        parts.push(format!("{:.0} GB free", free as f64 / GB));
    }
    if let Some(b) = &now.battery {
        parts.push(format!(
            "bat {}%{}",
            b.pct,
            if b.charging { "+" } else { "" }
        ));
    }
    let mut now = now;
    if !cfg.get("showTop").and_then(Value::as_bool).unwrap_or(true) {
        now.top.clear();
    }
    WidgetData {
        kind: WidgetKind::System,
        status,
        summary: parts.join(" · "),
        at: now.at,
        body: WidgetBody::System(SystemData {
            now,
            cpu_history,
            mem_history,
            notes,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sampler_shape() {
        let mut s = Sampler::new();
        let a = s.tick();
        let b = s.tick();
        assert!(b.at >= a.at);
        assert!((0.0..=100.0).contains(&b.cpu_pct));
        assert!(b.mem_total > 0 && b.mem_used <= b.mem_total);
        assert!(b.cpu_count >= 1);
        assert!(b.uptime_secs > 0);
        assert!(b.top.len() <= 3);
        assert!(b.top.windows(2).all(|w| w[0].cpu_pct >= w[1].cpu_pct));
        if let (Some(free), Some(total)) = (b.disk_free, b.disk_total) {
            assert!(free <= total && total > 0);
        }
        let (cpu, mem) = s.history();
        assert_eq!((cpu.len(), mem.len()), (2, 2));
        if cfg!(target_os = "macos") {
            assert!(b.mem_pressure.is_some());
        }
    }

    #[test]
    fn history_is_bounded() {
        let mut s = Sampler {
            sys: None,
            history: VecDeque::new(),
            last: None,
            battery: Some((Instant::now(), None)),
        };
        for _ in 0..HISTORY + 5 {
            s.tick();
        }
        assert_eq!(s.history().0.len(), HISTORY);
    }

    #[test]
    fn widget_from_shared_sampler() {
        let sampler = shared();
        let cfg = crate::widgets::entry(WidgetKind::System).default_config;
        let d = widget(&sampler, &cfg);
        assert!(d.summary.starts_with("cpu "));
        let WidgetBody::System(sys) = d.body else {
            panic!("system body")
        };
        assert!(!sys.cpu_history.is_empty());
    }

    #[test]
    fn spawned_job_ticks_and_stops() {
        let sampler = shared();
        let stop = Arc::new(AtomicBool::new(false));
        let h = spawn(sampler.clone(), Duration::from_millis(20), stop.clone()).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        stop.store(true, Ordering::Relaxed);
        h.join().unwrap();
        assert!(sampler.lock().unwrap().history().0.len() >= 2);
    }

    #[test]
    fn pmset() {
        let on_ac = "Now drawing from 'AC Power'\n -InternalBattery-0 (id=4653155)\t95%; charging; 0:40 remaining present: true\n";
        assert_eq!(
            parse_pmset(on_ac),
            Some(Battery {
                pct: 95,
                charging: true,
                plugged: true
            })
        );
        let full = "Now drawing from 'AC Power'\n -InternalBattery-0 (id=1)\t100%; charged; 0:00 remaining present: true\n";
        assert_eq!(
            parse_pmset(full),
            Some(Battery {
                pct: 100,
                charging: false,
                plugged: true
            })
        );
        let on_batt = "Now drawing from 'Battery Power'\n -InternalBattery-0 (id=1)\t7%; discharging; 0:20 remaining present: true\n";
        assert_eq!(
            parse_pmset(on_batt),
            Some(Battery {
                pct: 7,
                charging: false,
                plugged: false
            })
        );
        assert_eq!(parse_pmset("Now drawing from 'AC Power'\n"), None);
        assert_eq!(parse_pmset(""), None);
    }

    fn sample() -> SystemSample {
        SystemSample {
            cpu_pct: 10.0,
            mem_used: 8 * GB as u64,
            mem_total: 16 * GB as u64,
            mem_pressure: Some("normal".into()),
            disk_free: Some(200 * GB as u64),
            disk_total: Some(500 * GB as u64),
            disk_available: None,
            battery: Some(Battery {
                pct: 80,
                charging: false,
                plugged: false,
            }),
            ..Default::default()
        }
    }

    const LIM: Limits = Limits {
        cpu_warn: 85.0,
        disk_warn_gb: 20.0,
        battery_warn: 20,
    };

    #[test]
    fn status_thresholds() {
        assert_eq!(evaluate(&sample(), &[], &LIM), (WidgetStatus::Ok, vec![]));

        // one spike is averaged away; a sustained run is not
        assert_eq!(
            evaluate(&sample(), &[10.0, 10.0, 100.0], &LIM).0,
            WidgetStatus::Ok
        );
        let (st, notes) = evaluate(&sample(), &[90.0, 95.0, 99.0], &LIM);
        assert_eq!(st, WidgetStatus::Warn);
        assert_eq!(notes, vec!["cpu 95%".to_string()]);

        let mut s = sample();
        s.mem_pressure = Some("warn".into());
        assert_eq!(evaluate(&s, &[], &LIM).0, WidgetStatus::Warn);
        s.mem_pressure = Some("critical".into());
        assert_eq!(evaluate(&s, &[], &LIM).0, WidgetStatus::Bad);
        s.mem_pressure = None;
        s.mem_used = s.mem_total;
        assert_eq!(evaluate(&s, &[], &LIM).0, WidgetStatus::Warn);

        let mut s = sample();
        s.disk_free = Some(10 * GB as u64);
        assert_eq!(evaluate(&s, &[], &LIM).0, WidgetStatus::Warn);
        s.disk_free = Some(GB as u64);
        assert_eq!(evaluate(&s, &[], &LIM).0, WidgetStatus::Bad);

        let mut s = sample();
        s.battery = Some(Battery {
            pct: 15,
            charging: false,
            plugged: false,
        });
        assert_eq!(evaluate(&s, &[], &LIM).0, WidgetStatus::Warn);
        s.battery = Some(Battery {
            pct: 9,
            charging: false,
            plugged: false,
        });
        assert_eq!(evaluate(&s, &[], &LIM).0, WidgetStatus::Bad);
        s.battery = Some(Battery {
            pct: 9,
            charging: true,
            plugged: true,
        });
        assert_eq!(
            evaluate(&s, &[], &LIM).0,
            WidgetStatus::Ok,
            "plugged in is fine"
        );
    }

    #[test]
    fn top_sorted_and_cut() {
        let t = top_processes(
            vec![
                ("a".to_string(), 1.0, 5),
                ("b".to_string(), 50.04, 1),
                (String::new(), 99.0, 1),
                ("c".to_string(), 50.04, 9),
                ("d".to_string(), f32::NAN, 1),
            ]
            .into_iter(),
            3,
        );
        let names: Vec<&str> = t.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["c", "b", "a"]);
        assert_eq!(t[0].cpu_pct, 50.0);
    }

    #[test]
    fn uptimes() {
        assert_eq!(uptime_text(59), "0m");
        assert_eq!(uptime_text(3 * 3600 + 120), "3h 2m");
        assert_eq!(uptime_text(2 * 86_400 + 5 * 3600), "2d 5h");
        assert_eq!(pressure_name(4), "critical");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn finder_available_is_at_least_free() {
        let (free, _) = disk_space().unwrap();
        let avail = disk_available().unwrap();
        // allow for a write between the two reads
        assert!(avail + GB as u64 >= free, "available {avail} < free {free}");
    }
}
