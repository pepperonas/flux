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

/// Raw-USB owner for the Xbox 360 X-plorer. It deliberately reports raw
/// activity only: its byte layout is learned from the physical controller and
/// must not be guessed from a similar guitar.
pub struct XplorerSource {
    state: Arc<AtomicU8>,
    reports: Arc<AtomicU64>,
    running: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl XplorerSource {
    pub fn start() -> XplorerSource {
        let state = Arc::new(AtomicU8::new(ABSENT));
        let reports = Arc::new(AtomicU64::new(0));
        let running = Arc::new(AtomicBool::new(true));
        let worker_state = Arc::clone(&state);
        let worker_reports = Arc::clone(&reports);
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
                let Some(handle) = context.open_device_with_vid_pid(VENDOR_ID, PRODUCT_ID) else {
                    return;
                };
                if let Err(err) = handle.claim_interface(INTERFACE) {
                    log::warn!("could not claim X-plorer interface {INTERFACE}: {err}");
                    worker_state.store(ERROR, Ordering::Relaxed);
                    return;
                }
                worker_state.store(CONNECTED, Ordering::Relaxed);
                let mut report = [0u8; 32];
                while worker_running.load(Ordering::Relaxed) {
                    match handle.read_interrupt(
                        INPUT_ENDPOINT,
                        &mut report,
                        Duration::from_millis(20),
                    ) {
                        Ok(_) => {
                            worker_reports.fetch_add(1, Ordering::Relaxed);
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
                if worker_state.load(Ordering::Relaxed) == CONNECTED {
                    worker_state.store(ABSENT, Ordering::Relaxed);
                }
            })
            .map_err(|err| log::warn!("could not start X-plorer reader: {err}"))
            .ok();
        XplorerSource {
            state,
            reports,
            running,
            worker,
        }
    }

    pub fn status(&self) -> &'static str {
        match self.state.load(Ordering::Relaxed) {
            CONNECTED => "connected; waiting for control mapping",
            ERROR => "USB error; see log",
            _ => "not connected",
        }
    }

    pub fn reports(&self) -> u64 {
        self.reports.load(Ordering::Relaxed)
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
