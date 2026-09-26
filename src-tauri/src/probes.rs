use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use tokio::runtime::Handle;
use tokio::task::JoinHandle;

use crate::config::{Config, OverlayConfig};
use crate::probe;
use crate::render::SamplePoint;

/// One graph tick is one second. Probe tasks keep their own cadence and are
/// never restarted just because the user saved a style or graph setting.
const TICK: Duration = Duration::from_secs(1);
const MAX_BUFFERED_SAMPLES: usize = 86_400;

#[derive(Default)]
pub struct SampleBuffer {
    pub values: VecDeque<SamplePoint>,
    pub generation: u64,
}

pub type SampleStore = Arc<Mutex<HashMap<String, SampleBuffer>>>;

/// Owns one long-lived task per enabled overlay.
///
/// Configuration is shared with the tasks instead of being captured at spawn
/// time. Saving a config therefore updates the next probe without interrupting
/// the current graph.
pub struct ProbeManager {
    handle: Handle,
    configs: Arc<RwLock<HashMap<String, OverlayConfig>>>,
    samples: SampleStore,
    tasks: HashMap<String, JoinHandle<()>>,
    running: Arc<AtomicBool>,
}

impl ProbeManager {
    pub fn new(handle: Handle) -> Self {
        Self {
            handle,
            configs: Arc::new(RwLock::new(HashMap::new())),
            samples: Arc::new(Mutex::new(HashMap::new())),
            tasks: HashMap::new(),
            running: Arc::new(AtomicBool::new(true)),
        }
    }

    pub fn samples(&self) -> &SampleStore {
        &self.samples
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    pub fn set_running(&self, running: bool) {
        self.running.store(running, Ordering::Relaxed);
    }

    /// Apply configuration without restarting tasks that already exist.
    ///
    /// Existing tasks read `configs` on every tick, so host, protocol, and
    /// timeout edits take effect without a Save-induced pause. Only deleted or
    /// disabled overlays are stopped, and newly enabled overlays are started.
    pub fn apply_config(&mut self, config: &Config) {
        let enabled: HashMap<String, OverlayConfig> = config
            .overlays
            .iter()
            .filter(|overlay| overlay.enabled)
            .map(|overlay| (overlay.id.clone(), overlay.clone()))
            .collect();

        *self.configs.write().unwrap() = enabled.clone();

        let wanted: std::collections::HashSet<String> = enabled.keys().cloned().collect();
        self.tasks.retain(|id, task| {
            if wanted.contains(id) {
                true
            } else {
                task.abort();
                false
            }
        });

        for (id, _overlay) in enabled {
            if self.tasks.contains_key(&id) {
                continue;
            }

            let configs = Arc::clone(&self.configs);
            let samples = Arc::clone(&self.samples);
            let running = Arc::clone(&self.running);
            let task_id = id.clone();
            let task = self.handle.spawn(async move {
                probe_loop(task_id, configs, samples, running).await;
            });
            self.tasks.insert(id, task);
        }
    }

    pub fn stop_all(&mut self) {
        for (_, task) in self.tasks.drain() {
            task.abort();
        }
    }
}

impl Drop for ProbeManager {
    fn drop(&mut self) {
        self.stop_all();
    }
}

async fn probe_loop(
    id: String,
    configs: Arc<RwLock<HashMap<String, OverlayConfig>>>,
    samples: SampleStore,
    running: Arc<AtomicBool>,
) {
    loop {
        let overlay = configs.read().unwrap().get(&id).cloned();
        if running.load(Ordering::Relaxed) {
            if let Some(overlay) = overlay.as_ref() {
                let latency = probe::measure(&overlay.probe, overlay.timeout_ms).await;
                let mut all_samples = samples.lock().unwrap();
                let buffer = all_samples.entry(id.clone()).or_default();
                buffer.values.push_back(SamplePoint {
                    value: latency,
                    timestamp: Instant::now(),
                    is_prefill: false,
                });
                buffer.generation = buffer.generation.wrapping_add(1);
                while buffer.values.len() > MAX_BUFFERED_SAMPLES {
                    buffer.values.pop_front();
                }
            }
        }
        tokio::time::sleep(TICK).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProbeConfig;

    #[test]
    fn style_only_config_update_keeps_existing_task() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let mut manager = ProbeManager::new(runtime.handle().clone());
        let mut overlay = OverlayConfig::new();
        overlay.probe = ProbeConfig::Tcp {
            host: "127.0.0.1".to_string(),
            port: 1,
        };
        overlay.timeout_ms = 1;
        let mut config = Config {
            overlays: vec![overlay.clone()],
            ..Config::default()
        };
        manager.apply_config(&config);
        let first = manager.tasks[&overlay.id].id();

        overlay.scale = 3;
        config.overlays[0] = overlay;
        manager.apply_config(&config);

        assert_eq!(manager.tasks[&config.overlays[0].id].id(), first);
        manager.stop_all();
        runtime.shutdown_background();
    }
}
