//! Device hot-plug watcher via polling.
//!
//! The `start()` constructor and thread-based fields are only available on non-wasm32 targets.
//! On `wasm32`, only `current_device_names()` is available (used by device enumeration).

#[cfg(not(target_arch = "wasm32"))]
use cpal::traits::{DeviceTrait, HostTrait};

#[cfg(not(target_arch = "wasm32"))]
use oxisound_core::{DeviceEvent, DeviceInfo, OxiSoundError};

#[cfg(not(target_arch = "wasm32"))]
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Monitors device changes by polling the device list periodically (~500ms intervals).
///
/// On `wasm32`, this struct exists but `start()` is not available — use Web Audio API events
/// for device notifications instead.
pub struct CpalDeviceWatcher {
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) stop_flag: Arc<AtomicBool>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) receiver: std::sync::mpsc::Receiver<DeviceEvent>,
    #[cfg(all(feature = "tokio", not(target_arch = "wasm32")))]
    pub(crate) broadcast_tx: Arc<tokio::sync::broadcast::Sender<DeviceEvent>>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) thread: Option<std::thread::JoinHandle<()>>,
}

impl CpalDeviceWatcher {
    /// Starts polling for device changes at ~500ms intervals.
    ///
    /// Not available on `wasm32`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn start() -> Result<Self, OxiSoundError> {
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop_flag);
        let (tx, rx) = std::sync::mpsc::channel::<DeviceEvent>();

        #[cfg(feature = "tokio")]
        let (broadcast_tx_inner, _) = tokio::sync::broadcast::channel::<DeviceEvent>(64);
        #[cfg(feature = "tokio")]
        let broadcast_tx = Arc::new(broadcast_tx_inner);
        #[cfg(feature = "tokio")]
        let broadcast_tx_clone = Arc::clone(&broadcast_tx);

        let thread = std::thread::spawn(move || {
            let mut prev_names = Self::current_device_names();
            while !stop_clone.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(500));
                let current = Self::current_device_names();
                // Detect added devices
                for name in current.difference(&prev_names) {
                    let info = DeviceInfo::builder(name.clone()).build();
                    let event = DeviceEvent::DeviceAdded(info);
                    if tx.send(event.clone()).is_err() {
                        break;
                    }
                    #[cfg(feature = "tokio")]
                    let _ = broadcast_tx_clone.send(event);
                }
                // Detect removed devices
                for name in prev_names.difference(&current) {
                    let event = DeviceEvent::DeviceRemoved(name.clone());
                    if tx.send(event.clone()).is_err() {
                        break;
                    }
                    #[cfg(feature = "tokio")]
                    let _ = broadcast_tx_clone.send(event);
                }
                prev_names = current;
            }
        });

        Ok(Self {
            stop_flag,
            receiver: rx,
            #[cfg(feature = "tokio")]
            broadcast_tx,
            thread: Some(thread),
        })
    }

    /// Returns a broadcast receiver that delivers device events asynchronously.
    ///
    /// Multiple subscribers may each call `subscribe()` to get independent receivers.
    /// Not available on `wasm32`.
    #[cfg(all(feature = "tokio", not(target_arch = "wasm32")))]
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<DeviceEvent> {
        self.broadcast_tx.subscribe()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn current_device_names() -> std::collections::HashSet<String> {
        cpal::default_host()
            .devices()
            .map(|devs| {
                devs.filter_map(|d| d.description().ok().map(|desc| desc.name().to_owned()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Tries to receive a pending device event without blocking.
    ///
    /// Not available on `wasm32`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn try_recv(&self) -> Option<DeviceEvent> {
        self.receiver.try_recv().ok()
    }
}

impl Drop for CpalDeviceWatcher {
    fn drop(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.stop_flag.store(true, Ordering::Relaxed);
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }
}
