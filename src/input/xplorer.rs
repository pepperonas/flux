use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use rusb::{Context, UsbContext};

const VENDOR_ID: u16 = 0x1430;
const PRODUCT_ID: u16 = 0x4748;
const INTERFACE: u8 = 0;
const INPUT_ENDPOINT: u8 = 0x81;

const ABSENT: u8 = 0;
const CONNECTED: u8 = 1;
const ERROR: u8 = 2;

/// Compares successive reports while a controller control is moved. The
/// learner reports byte positions only; assigning positions to controls is
/// left to the setup flow and the physical device.
#[derive(Default)]
pub struct ReportLearner {
    previous: Option<[u8; 32]>,
}

impl ReportLearner {
    pub fn observe(&mut self, report: [u8; 32]) -> Vec<usize> {
        let changed = self
            .previous
            .map(|previous| {
                report
                    .iter()
                    .zip(previous)
                    .enumerate()
                    .filter_map(|(index, (current, old))| (current != &old).then_some(index))
                    .collect()
            })
            .unwrap_or_default();
        self.previous = Some(report);
        changed
    }
}

/// Raw-USB owner for the Xbox 360 X-plorer. It deliberately reports raw
/// activity only: its byte layout is learned from the physical controller and
/// must not be guessed from a similar guitar.
pub struct XplorerSource {
    state: Arc<AtomicU8>,
    reports: Arc<AtomicU64>,
    last_report: Arc<AtomicU64>,
    last_bytes: Arc<AtomicU64>,
    changed_mask: Arc<AtomicU64>,
    claim_denied: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl XplorerSource {
    pub fn start() -> XplorerSource {
        let state = Arc::new(AtomicU8::new(ABSENT));
        let reports = Arc::new(AtomicU64::new(0));
        let last_report = Arc::new(AtomicU64::new(0));
        let last_bytes = Arc::new(AtomicU64::new(0));
        let changed_mask = Arc::new(AtomicU64::new(0));
        let claim_denied = Arc::new(AtomicBool::new(false));
        let running = Arc::new(AtomicBool::new(true));
        let worker_state = Arc::clone(&state);
        let worker_reports = Arc::clone(&reports);
        let worker_last_report = Arc::clone(&last_report);
        let worker_last_bytes = Arc::clone(&last_bytes);
        let worker_changed_mask = Arc::clone(&changed_mask);
        let worker_claim_denied = Arc::clone(&claim_denied);
        let worker_running = Arc::clone(&running);
        let worker = thread::Builder::new()
            .name("flux-xplorer-usb".into())
            .spawn(move || {
                let context = match Context::new() {
                    Ok(context) => context,
                    Err(err) => {
                        log::warn!("X-plorer USB context unavailable: {err}");
                        worker_state.store(ERROR, Ordering::Relaxed);
                        return;
                    }
                };
                while worker_running.load(Ordering::Relaxed) {
                    let Some(handle) = context.open_device_with_vid_pid(VENDOR_ID, PRODUCT_ID)
                    else {
                        worker_state.store(ABSENT, Ordering::Relaxed);
                        thread::sleep(Duration::from_millis(250));
                        continue;
                    };
                    // macOS may attach a generic USB driver between discovery
                    // and claim. Try an explicit detach first, then enable
                    // libusb's automatic detach for reconnects. Some macOS
                    // versions report NotFound when no driver is attached;
                    // that is harmless and claim_interface remains the source
                    // of truth for access failures.
                    if rusb::supports_detach_kernel_driver() {
                        match handle.detach_kernel_driver(INTERFACE) {
                            Ok(()) | Err(rusb::Error::NotFound) => {}
                            Err(err) => {
                                log::debug!("could not detach X-plorer kernel driver: {err}")
                            }
                        }
                    }
                    let _ = handle.set_auto_detach_kernel_driver(true);
                    if let Err(err) = handle.claim_interface(INTERFACE) {
                        // Do not flood the log while macOS keeps ownership of
                        // the vendor interface. The state remains visible in
                        // Diagnostics and a new warning is emitted only after
                        // a successful reconnect or a different state.
                        if worker_state.swap(ERROR, Ordering::Relaxed) != ERROR {
                            log::warn!("could not claim X-plorer interface {INTERFACE}: {err}");
                        } else {
                            log::debug!("could not claim X-plorer interface {INTERFACE}: {err}");
                        }
                        worker_claim_denied
                            .store(matches!(err, rusb::Error::Access), Ordering::Relaxed);
                        thread::sleep(Duration::from_millis(500));
                        continue;
                    }
                    worker_state.store(CONNECTED, Ordering::Relaxed);
                    worker_claim_denied.store(false, Ordering::Relaxed);
                    worker_last_report.store(0, Ordering::Relaxed);
                    worker_last_bytes.store(0, Ordering::Relaxed);
                    worker_changed_mask.store(0, Ordering::Relaxed);
                    let mut report = [0u8; 32];
                    let mut previous_report = [0u8; 32];
                    let mut have_previous = false;
                    while worker_running.load(Ordering::Relaxed) {
                        match handle.read_interrupt(
                            INPUT_ENDPOINT,
                            &mut report,
                            Duration::from_millis(20),
                        ) {
                            Ok(_) => {
                                worker_reports.fetch_add(1, Ordering::Relaxed);
                                // Keep a compact fingerprint for diagnostics. The
                                // raw layout is intentionally not interpreted here.
                                let hash = report.iter().fold(0xcbf29ce484222325u64, |h, byte| {
                                    h.wrapping_mul(0x100000001b3).wrapping_add(u64::from(*byte))
                                });
                                worker_last_report.store(hash, Ordering::Relaxed);
                                let bytes = report[..8]
                                    .iter()
                                    .enumerate()
                                    .fold(0u64, |value, (i, byte)| {
                                        value | (u64::from(*byte) << (i * 8))
                                    });
                                worker_last_bytes.store(bytes, Ordering::Relaxed);
                                if have_previous {
                                    let mask = report.iter().zip(previous_report).enumerate().fold(
                                        0u64,
                                        |mask, (index, (current, previous))| {
                                            if current != &previous {
                                                mask | (1u64 << index)
                                            } else {
                                                mask
                                            }
                                        },
                                    );
                                    worker_changed_mask.store(mask, Ordering::Relaxed);
                                }
                                previous_report = report;
                                have_previous = true;
                            }
                            Err(rusb::Error::Timeout) => {}
                            Err(rusb::Error::NoDevice) => break,
                            Err(err) => {
                                log::warn!("X-plorer report read failed: {err}");
                                worker_state.store(ERROR, Ordering::Relaxed);
                                break;
                            }
                        }
                    }
                    let _ = handle.release_interface(INTERFACE);
                    worker_state.store(ABSENT, Ordering::Relaxed);
                }
            })
            .map_err(|err| log::warn!("could not start X-plorer reader: {err}"))
            .ok();
        XplorerSource {
            state,
            reports,
            last_report,
            last_bytes,
            changed_mask,
            claim_denied,
            running,
            worker,
        }
    }

    pub fn status(&self) -> &'static str {
        match self.state.load(Ordering::Relaxed) {
            CONNECTED => "connected; waiting for control mapping",
            ERROR if self.claim_denied.load(Ordering::Relaxed) => {
                "USB access denied by macOS; see Diagnostics"
            }
            ERROR => "USB error; see log",
            _ => "not connected",
        }
    }

    pub fn reports(&self) -> u64 {
        self.reports.load(Ordering::Relaxed)
    }

    pub fn last_report_fingerprint(&self) -> Option<u64> {
        let value = self.last_report.load(Ordering::Relaxed);
        (value != 0).then_some(value)
    }

    pub fn last_report_bytes(&self) -> Option<u64> {
        let value = self.last_bytes.load(Ordering::Relaxed);
        (self.last_report_fingerprint().is_some()).then_some(value)
    }

    pub fn changed_byte_mask(&self) -> Option<u64> {
        self.last_report_fingerprint()
            .map(|_| self.changed_mask.load(Ordering::Relaxed))
    }
}

impl Drop for XplorerSource {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ReportLearner;

    #[test]
    fn report_learner_returns_changed_byte_positions() {
        let mut learner = ReportLearner::default();
        assert!(learner.observe([0; 32]).is_empty());
        let mut next = [0; 32];
        next[3] = 1;
        next[17] = 255;
        assert_eq!(learner.observe(next), vec![3, 17]);
        assert!(learner.observe(next).is_empty());
    }
}
