use super::*;
use serde::Deserialize;
use std::io::Read;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

const MAX_BYTES: u64 = 256 * 1024;
const HOSTS: [(&str, &str); 3] = [
    ("sid-vm", "Sid VM"),
    ("helsinki", "VPS Helsinki"),
    ("singapore", "VPS Singapore"),
];

#[derive(Debug, Deserialize)]
pub(super) struct Snapshot {
    version: u32,
    generated_at: String,
    poll_seconds: u64,
    hosts: Vec<Host>,
    temporary: Temporary,
}
#[derive(Debug, Deserialize)]
struct Host {
    id: String,
    status: String,
    sampled_at: Option<String>,
    cpu_percent: Option<f64>,
    memory_used_bytes: Option<u64>,
    memory_total_bytes: Option<u64>,
    load1: Option<f64>,
    cpu_count: Option<u32>,
}
#[derive(Debug, Deserialize)]
struct Temporary {
    status: String,
    sampled_at: Option<String>,
    providers: Vec<Provider>,
    resources: Vec<Resource>,
}
#[derive(Debug, Deserialize)]
struct Provider {
    id: String,
    status: String,
}
#[derive(Debug, Deserialize)]
struct Resource {
    label: String,
    kind: String,
    state: String,
    elapsed_seconds: Option<u64>,
    elapsed_basis: String,
    cpu_percent: Option<f64>,
    gpu_percent: Option<f64>,
}

#[derive(Debug, PartialEq)]
enum Line {
    Text(String, bool),
    Bar(&'static str, Option<f64>),
}

pub(super) struct MonitorPanel {
    pub(super) expanded: bool,
    pub(super) scroll: usize,
    snapshot: Option<Snapshot>,
    error: Option<String>,
    reader: Option<Receiver<Result<Snapshot, String>>>,
    last_tick: Option<Instant>,
    now: i64,
}
impl Default for MonitorPanel {
    fn default() -> Self {
        Self {
            expanded: true,
            scroll: 0,
            snapshot: None,
            error: None,
            reader: None,
            last_tick: None,
            now: OffsetDateTime::now_utc().unix_timestamp(),
        }
    }
}

fn snapshot_path() -> PathBuf {
    std::env::var_os("HERDR_MONITOR_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("LOCALAPPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(crate::config::state_dir)
                .join("herdr")
                .join("monitor")
                .join("snapshot.json")
        })
}
fn read_snapshot(path: &std::path::Path) -> Result<Snapshot, String> {
    let file = std::fs::File::open(path).map_err(|_| "Monitor data unavailable".to_owned())?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Monitor read failed".to_owned())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("Monitor file too large".to_owned());
    }
    decode(&bytes)
}
fn decode(bytes: &[u8]) -> Result<Snapshot, String> {
    let snapshot: Snapshot =
        serde_json::from_slice(bytes).map_err(|_| "Invalid monitor data".to_owned())?;
    if snapshot.version != 1
        || snapshot.hosts.len() > 16
        || snapshot.temporary.resources.len() > 32
        || snapshot.temporary.providers.len() > 8
        || timestamp(&snapshot.generated_at).is_none()
    {
        return Err("Unsupported monitor data".to_owned());
    }
    Ok(snapshot)
}
fn timestamp(value: &str) -> Option<i64> {
    OffsetDateTime::parse(value, &Rfc3339)
        .ok()
        .map(|t| t.unix_timestamp())
}
fn fresh(value: Option<&str>, now: i64, max_age: i64) -> bool {
    value
        .and_then(timestamp)
        .is_some_and(|t| t <= now + 5 && now.saturating_sub(t) <= max_age)
}
fn percent(value: Option<f64>) -> Option<f64> {
    value.filter(|n| n.is_finite() && (0.0..=100.0).contains(n))
}
fn clean(value: &str) -> String {
    value.chars().filter(|c| !c.is_control()).take(48).collect()
}
fn elapsed(resource: &Resource, snapshot: &Snapshot, now: i64) -> Option<u64> {
    let seconds = resource
        .elapsed_seconds
        .filter(|_| matches!(resource.elapsed_basis.as_str(), "created" | "uptime"))?;
    let delta = if resource.state == "running" || resource.elapsed_basis == "created" {
        now.saturating_sub(timestamp(&snapshot.generated_at).unwrap_or(now)).max(0) as u64
    } else {
        0
    };
    Some(seconds.saturating_add(delta))
}
fn format_elapsed(seconds: Option<u64>) -> String {
    seconds
        .map(|seconds| format!("{:02}:{:02}:{:02}", seconds / 3600, seconds / 60 % 60, seconds % 60))
        .unwrap_or_else(|| "unknown".into())
}

impl MonitorPanel {
    pub(super) fn new(expanded: bool) -> Self {
        Self {
            expanded,
            ..Self::default()
        }
    }
    pub(super) fn tick(&mut self, now: Instant) -> bool {
        if self
            .last_tick
            .is_some_and(|last| now.saturating_duration_since(last) < Duration::from_secs(1))
        {
            return false;
        }
        let previous = self.lines();
        self.last_tick = Some(now);
        self.now = OffsetDateTime::now_utc().unix_timestamp();
        if self.reader.is_none() {
            let (tx, rx) = mpsc::sync_channel(1);
            let path = snapshot_path();
            match std::thread::Builder::new()
                .name("herdr-monitor".into())
                .spawn(move || loop {
                    match tx.try_send(read_snapshot(&path)) {
                        Err(mpsc::TrySendError::Disconnected(_)) => break,
                        _ => {}
                    }
                    std::thread::sleep(Duration::from_secs(2));
                }) {
                Ok(_) => self.reader = Some(rx),
                Err(_) => self.error = Some("Monitor reader unavailable".into()),
            }
        }
        if let Some(reader) = &self.reader {
            match reader.try_recv() {
                Ok(Ok(snapshot)) => {
                    self.snapshot = Some(snapshot);
                    self.error = None;
                }
                Ok(Err(error)) => {
                    self.snapshot = None;
                    self.error = Some(error);
                }
                Err(TryRecvError::Disconnected) => {
                    self.reader = None;
                    self.error = Some("Monitor reader stopped".into());
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        self.expanded && previous != self.lines()
    }

    fn lines(&self) -> Vec<Line> {
        let mut lines = Vec::new();
        let Some(snapshot) = self.snapshot.as_ref() else {
            lines.push(Line::Text(
                self.error
                    .clone()
                    .unwrap_or_else(|| "Waiting for monitor data".into()),
                false,
            ));
            return lines;
        };
        let age_limit = snapshot
            .poll_seconds
            .clamp(15, 60)
            .saturating_mul(3)
            .max(45) as i64;
        let current = fresh(Some(&snapshot.generated_at), self.now, age_limit);
        if !current {
            lines.push(Line::Text("STALE - last known data".into(), true));
        }
        for (id, label) in HOSTS {
            lines.push(Line::Text(label.into(), true));
            let host = snapshot.hosts.iter().find(|host| host.id == id);
            let Some(host) = host.filter(|h| {
                h.status == "ok" && fresh(h.sampled_at.as_deref(), self.now, age_limit) && current
            }) else {
                lines.push(Line::Text(
                    if current {
                        "  unavailable / stale"
                    } else {
                        "  stale"
                    }
                    .into(),
                    false,
                ));
                continue;
            };
            lines.push(Line::Bar("CPU", percent(host.cpu_percent)));
            let memory = host
                .memory_used_bytes
                .zip(host.memory_total_bytes)
                .filter(|(used, total)| *total > 0 && used <= total);
            lines.push(Line::Bar(
                "RAM",
                memory.map(|(used, total)| used as f64 / total as f64 * 100.0),
            ));
            if let Some((used, total)) = memory {
                lines.push(Line::Text(
                    format!(
                        "  {:.1}/{:.1} GiB",
                        used as f64 / 1_073_741_824.0,
                        total as f64 / 1_073_741_824.0
                    ),
                    false,
                ));
            }
            if id != "sid-vm" {
                let load = host
                    .load1
                    .filter(|n| n.is_finite() && *n >= 0.0)
                    .map(|n| format!("{n:.2}"))
                    .unwrap_or_else(|| "?".into());
                let cores = host
                    .cpu_count
                    .filter(|n| *n > 0)
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "?".into());
                lines.push(Line::Text(format!("  Load {load} / {cores} CPU"), false));
            }
        }
        let temporary = &snapshot.temporary;
        lines.push(Line::Text("Temporary compute".into(), true));
        let temp_fresh = current && fresh(temporary.sampled_at.as_deref(), self.now, 180);
        if !temp_fresh {
            lines.push(Line::Text("  resource data stale".into(), false));
            return lines;
        }
        let inventory_complete = ["hetzner", "runpod"].iter().all(|id| {
            temporary
                .providers
                .iter()
                .any(|provider| provider.id == *id && provider.status == "ok")
        });
        for id in ["hetzner", "runpod"] {
            if !temporary
                .providers
                .iter()
                .any(|provider| provider.id == id && provider.status == "ok")
            {
                lines.push(Line::Text(format!("  {id} unavailable"), false));
            }
        }
        let active: Vec<_> = temporary
            .resources
            .iter()
            .filter(|r| r.kind == "rented_cpu" || r.state != "stopped")
            .collect();
        if active.is_empty() {
            lines.push(Line::Text(
                if temporary.status == "ok" && inventory_complete {
                    "  none active"
                } else {
                    "  active resources unknown"
                }
                .into(),
                false,
            ));
        }
        let rented: Vec<_> = active
            .iter()
            .copied()
            .filter(|resource| resource.kind == "rented_cpu")
            .collect();
        let rented_known = temporary.providers.iter().any(|provider| {
            provider.id == "hetzner" && provider.status == "ok"
        });
        if rented_known {
            let longest = rented.iter().try_fold(0, |longest, resource| {
                elapsed(resource, snapshot, self.now).map(|seconds| longest.max(seconds))
            });
            lines.push(Line::Text(format!("Rented CPU: {}", rented.len()), true));
            lines.push(Line::Text(
                format!("Longest: {}", if rented.is_empty() {
                    "--".into()
                } else {
                    format_elapsed(longest)
                }),
                false,
            ));
        } else {
            lines.push(Line::Text("Rented CPU: unknown".into(), true));
            lines.push(Line::Text("Longest: unknown".into(), false));
        }
        for resource in active {
            if resource.kind == "rented_cpu" {
                continue;
            }
            let kind = if resource.kind == "runpod" {
                "RunPod"
            } else {
                "Rented CPU"
            };
            lines.push(Line::Text(
                format!("{kind}: {}", clean(&resource.label)),
                true,
            ));
            let elapsed = format_elapsed(elapsed(resource, snapshot, self.now));
            let basis = if resource.elapsed_basis == "created" {
                "age"
            } else {
                "up"
            };
            lines.push(Line::Text(
                format!("  {} {basis} {elapsed}", clean(&resource.state)),
                false,
            ));
            lines.push(Line::Bar(
                if resource.kind == "runpod" {
                    "GPU"
                } else {
                    "CPU"
                },
                percent(if resource.kind == "runpod" {
                    resource.gpu_percent
                } else {
                    resource.cpu_percent
                }),
            ));
        }
        lines
    }

    pub(super) fn height(&self, available: u16) -> u16 {
        if !self.expanded {
            return 0;
        }
        (self.lines().len().saturating_add(1).min(u16::MAX as usize) as u16)
            .min(available.saturating_sub(7))
    }
}

pub(super) fn render(
    buffer: &mut Buffer,
    area: Rect,
    panel: &MonitorPanel,
    palette: &Palette,
    hits: &mut ShellHitMap,
) {
    if area.is_empty() {
        return;
    }
    hits.monitor_body = area;
    let lines = panel.lines();
    let rows = usize::from(area.height.saturating_sub(1));
    hits.monitor_max_scroll = lines.len().saturating_sub(rows);
    let offset = panel.scroll.min(hits.monitor_max_scroll);
    let title = if lines.len() > rows {
        " Monitor (scroll)"
    } else {
        " Machine monitor"
    };
    render::put_text(
        buffer,
        area.x,
        area.y,
        area.width,
        title,
        Style::default()
            .fg(palette.accent)
            .add_modifier(Modifier::BOLD),
    );
    for (index, line) in lines.iter().skip(offset).take(rows).enumerate() {
        let y = area.y + 1 + index as u16;
        match line {
            Line::Text(text, bold) => {
                let style = Style::default().fg(if *bold {
                    palette.text
                } else {
                    palette.overlay0
                });
                render::put_text(
                    buffer,
                    area.x,
                    y,
                    area.width,
                    text,
                    if *bold {
                        style.add_modifier(Modifier::BOLD)
                    } else {
                        style
                    },
                );
            }
            Line::Bar(label, value) => {
                let width = area.width.saturating_sub(10);
                render::put_text(
                    buffer,
                    area.x,
                    y,
                    area.width.min(4),
                    label,
                    Style::default().fg(palette.overlay0),
                );
                let color = match value {
                    Some(v) if *v >= 90.0 => palette.red,
                    Some(v) if *v >= 70.0 => palette.yellow,
                    _ => palette.green,
                };
                let fill = value
                    .map(|v| (v / 100.0 * f64::from(width)).round() as u16)
                    .unwrap_or(0);
                for column in 0..width {
                    buffer[(area.x + 4 + column, y)]
                        .set_symbol(if column < fill { "█" } else { "░" })
                        .set_style(Style::default().fg(if column < fill {
                            color
                        } else {
                            palette.surface_dim
                        }));
                }
                let value = value
                    .map(|v| format!("{v:.0}%"))
                    .unwrap_or_else(|| "?".into());
                render::put_right_text(buffer, area, y, &value, Style::default().fg(color));
            }
        }
    }
}

impl ClientShellState {
    pub(crate) fn tick_machine_monitor(&mut self, now: Instant) -> bool {
        self.machine_monitor.tick(now) && !self.sidebar_collapsed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> serde_json::Value {
        serde_json::json!({"version":1,"generated_at":"2026-11-13T12:00:00Z","poll_seconds":15,
            "hosts":[{"id":"sid-vm","status":"ok","sampled_at":"2026-11-13T12:00:00Z","cpu_percent":34.0,"memory_used_bytes":5368709120u64,"memory_total_bytes":8589934592u64,"load1":0.4,"cpu_count":4}],
            "temporary":{"status":"ok","sampled_at":"2026-11-13T12:00:00Z","providers":[{"id":"hetzner","status":"ok"},{"id":"runpod","status":"ok"}],"resources":[]}})
    }
    fn panel(value: serde_json::Value) -> MonitorPanel {
        MonitorPanel {
            snapshot: Some(decode(&serde_json::to_vec(&value).unwrap()).unwrap()),
            now: timestamp("2026-11-13T12:00:10Z").unwrap(),
            ..Default::default()
        }
    }
    fn texts(panel: &MonitorPanel) -> String {
        panel
            .lines()
            .iter()
            .filter_map(|line| match line {
                Line::Text(text, _) => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
    #[test]
    fn monitor_known_hosts_and_null_values_never_become_zero() {
        let mut value = sample();
        value["hosts"][0]["cpu_percent"] = serde_json::Value::Null;
        let panel = panel(value);
        assert!(panel.lines().contains(&Line::Bar("CPU", None)));
        let text = texts(&panel);
        assert!(text.contains("VPS Helsinki"));
        assert!(text.contains("VPS Singapore"));
        assert!(!text.contains("Local"));
        assert!(text.contains("none active"));
    }
    #[test]
    fn monitor_stale_and_failed_sources_suppress_healthy_bars() {
        let mut p = panel(sample());
        p.now += 46;
        assert!(texts(&p).contains("STALE"));
        assert!(!p.lines().iter().any(|l| matches!(l, Line::Bar(_, Some(_)))));
        let mut value = sample();
        value["hosts"][0]["status"] = "unavailable".into();
        assert!(!panel(value)
            .lines()
            .iter()
            .any(|l| matches!(l, Line::Bar(_, Some(_)))));
    }
    #[test]
    fn monitor_provider_failure_is_not_empty_inventory() {
        let mut value = sample();
        value["temporary"]["status"] = "partial".into();
        value["temporary"]["providers"][1]["status"] = "unavailable".into();
        let text = texts(&panel(value));
        assert!(text.contains("runpod unavailable"));
        assert!(text.contains("resources unknown"));
        assert!(!text.contains("none active"));
    }
    #[test]
    fn monitor_elapsed_uses_snapshot_age_and_valid_basis() {
        let mut value = sample();
        value["temporary"]["resources"] = serde_json::json!([{"label":"work","kind":"runpod","state":"running","elapsed_seconds":65,"elapsed_basis":"uptime","cpu_percent":null,"gpu_percent":null}]);
        assert!(texts(&panel(value.clone())).contains("00:01:15"));
        value["temporary"]["resources"][0]["elapsed_basis"] = "unknown".into();
        assert!(texts(&panel(value)).contains("up unknown"));
    }
    #[test]
    fn monitor_summarizes_rented_cpu_count_and_longest_elapsed() {
        let mut value = sample();
        value["temporary"]["resources"] = serde_json::json!([
            {"label":"one","kind":"rented_cpu","state":"running","elapsed_seconds":65,"elapsed_basis":"uptime","cpu_percent":null,"gpu_percent":null},
            {"label":"two","kind":"rented_cpu","state":"running","elapsed_seconds":3725,"elapsed_basis":"created","cpu_percent":null,"gpu_percent":null},
            {"label":"gpu","kind":"runpod","state":"running","elapsed_seconds":20,"elapsed_basis":"uptime","cpu_percent":null,"gpu_percent":null}
        ]);
        let text = texts(&panel(value));
        assert!(text.contains("Rented CPU: 2"));
        assert!(text.contains("Longest: 01:02:15"));
        assert!(!text.contains("Rented CPU: one"));
        assert!(text.contains("RunPod: gpu"));
    }
    #[test]
    fn monitor_rented_count_handles_zero_one_and_three() {
        for count in [0, 1, 3] {
            let mut value = sample();
            value["temporary"]["resources"] = (0..count)
                .map(|index| serde_json::json!({"label":format!("worker-{index}"),"kind":"rented_cpu","state":"running","elapsed_seconds":65,"elapsed_basis":"created"}))
                .collect::<Vec<_>>().into();
            let text = texts(&panel(value));
            assert!(text.contains(&format!("Rented CPU: {count}")));
            assert!(text.contains(if count == 0 { "Longest: --" } else { "Longest: 00:01:15" }));
        }
    }
    #[test]
    fn monitor_unknown_worker_age_or_inventory_is_not_a_known_maximum() {
        let mut value = sample();
        value["temporary"]["resources"] = serde_json::json!([
            {"label":"known","kind":"rented_cpu","state":"running","elapsed_seconds":65,"elapsed_basis":"created"},
            {"label":"unknown","kind":"rented_cpu","state":"running","elapsed_seconds":null,"elapsed_basis":"unknown"}
        ]);
        let text = texts(&panel(value.clone()));
        assert!(text.contains("Rented CPU: 2"));
        assert!(text.contains("Longest: unknown"));
        value["temporary"]["providers"][0]["status"] = "unavailable".into();
        let text = texts(&panel(value));
        assert!(text.contains("Rented CPU: unknown"));
        assert!(text.contains("Longest: unknown"));
    }
    #[test]
    fn monitor_rented_summary_renders_complete_elapsed_in_narrow_sidebar() {
        let mut value = sample();
        value["temporary"]["resources"] = serde_json::json!([
            {"label":"worker","kind":"rented_cpu","state":"running","elapsed_seconds":4572,"elapsed_basis":"created"}
        ]);
        let p = panel(value);
        let area = Rect::new(0, 0, 24, 40);
        let mut buffer = Buffer::empty(area);
        render(&mut buffer, area, &p, &Palette::catppuccin(), &mut ShellHitMap::default());
        let rows = (0..area.height).map(|row| {
            (0..area.width).map(|column| buffer[(column, row)].symbol()).collect::<String>()
        }).collect::<Vec<_>>();
        assert!(rows.iter().any(|row| row.contains("Rented CPU: 1")));
        assert!(rows.iter().any(|row| row.contains("Longest: 01:16:22")));
    }
    #[test]
    fn monitor_decode_bounds_and_clock_validation() {
        let mut value = sample();
        value["version"] = 2.into();
        assert!(decode(&serde_json::to_vec(&value).unwrap()).is_err());
        assert!(!fresh(Some("bad"), 0, 45));
        assert!(!fresh(Some("2026-11-13T12:00:00Z"), 0, 45));
        assert_eq!(percent(Some(-1.0)), None);
        assert_eq!(percent(Some(101.0)), None);
        assert_eq!(clean("a\n\x1bb"), "ab");
    }
    #[test]
    fn monitor_layout_leaves_sessions_and_clips_safely() {
        let mut p = panel(sample());
        assert!(p.height(20) <= 13);
        p.expanded = false;
        assert_eq!(p.height(20), 0);
        p.expanded = true;
        for width in 1..28 {
            for height in 1..20 {
                let area = Rect::new(0, 0, width, height);
                let mut b = Buffer::empty(area);
                let mut hits = ShellHitMap::default();
                render(&mut b, area, &p, &Palette::catppuccin(), &mut hits);
                assert_eq!(hits.monitor_body, area);
            }
        }
    }
}
