use serde_json::{json, Value};
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender};
use std::thread;
use std::time::SystemTime;

static NEXT_REQ_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
pub struct JsonLogger {
    tx: SyncSender<Value>,
}

fn iso_timestamp() -> String {
    let now = SystemTime::now();
    let duration = now
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();
    let millis = duration.subsec_millis();

    // Convert seconds to UTC date-time string
    let days = (secs / 86400) as i64;
    let rem_secs = secs % 86400;
    let hours = rem_secs / 3600;
    let minutes = (rem_secs % 3600) / 60;
    let seconds = rem_secs % 60;

    // Date computation (standard Gregorian cycle)
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        year, m, d, hours, minutes, seconds, millis
    )
}

impl JsonLogger {
    pub fn init() -> Self {
        // Use a bounded channel with a dedicated background thread for fast non-blocking writes
        let (tx, rx) = sync_channel::<Value>(10000);

        thread::spawn(move || {
            let log_file = "requests.log.jsonl";
            let mut file = match OpenOptions::new().create(true).append(true).open(log_file) {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("Failed to open {} for logging: {}", log_file, e);
                    return;
                }
            };

            while let Ok(val) = rx.recv() {
                if let Ok(mut json_str) = serde_json::to_string(&val) {
                    json_str.push('\n');
                    if let Err(e) = file.write_all(json_str.as_bytes()) {
                        eprintln!("Failed to write log entry: {}", e);
                    }
                    let _ = file.flush();
                }
            }
        });

        JsonLogger { tx }
    }

    pub fn next_id(&self) -> u64 {
        NEXT_REQ_ID.fetch_add(1, Ordering::Relaxed)
    }

    pub fn log_raw(&self, mut entry: Value) {
        if let Some(map) = entry.as_object_mut() {
            if !map.contains_key("timestamp") {
                map.insert("timestamp".to_string(), json!(iso_timestamp()));
            }
        }
        let _ = self.tx.try_send(entry);
    }
}
