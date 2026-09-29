use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use tokio::runtime::Handle;
use tokio::task::JoinHandle;

use crate::config::{Config, TargetConfig};
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

/// Samples for one overlay's targets, keyed by target id.
///
/// Nested rather than flat on purpose. The renderer walks one overlay's targets
/// in order on every frame, and a flat map would make it re-acquire the lock
/// per target, so the cost of a group would grow with the thing the lock is
/// there to protect.
pub type OverlaySamples = HashMap<String, SampleBuffer>;

pub type SampleStore = Arc<Mutex<HashMap<String, OverlaySamples>>>;

/// One probe task, identified by the target it measures.
///
/// A target id is only unique within its overlay, so the pair is what keys a
/// task and a buffer. `TaskKey` is that pair, named because `HashMap` needs it
/// to be one value and building the tuple at every lookup is how the two ends
/// of it drift apart.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TaskKey {
    pub overlay_id: String,
    pub target_id: String,
}

impl TaskKey {
    pub fn new(overlay_id: &str, target_id: &str) -> Self {
        Self {
            overlay_id: overlay_id.to_string(),
            target_id: target_id.to_string(),
        }
    }
}

/// Owns one long-lived task per enabled target.
///
/// Configuration is shared with the tasks instead of being captured at spawn
/// time. Saving a config therefore updates the next probe without interrupting
/// the current graph.
///
/// One task per *target* rather than one per overlay, and the reason is the
/// timeout: a probe measures for as long as its target's timeout allows, so a
/// task probing four targets in turn takes four timeouts to complete a tick
/// when the hosts are all down. Separate tasks overlap instead, so a group
/// costs the same wall-clock as a single target.
pub struct ProbeManager {
    handle: Handle,
    configs: Arc<RwLock<HashMap<TaskKey, TargetConfig>>>,
    samples: SampleStore,
    tasks: HashMap<TaskKey, JoinHandle<()>>,
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

    pub fn set_running(&mut self, running: bool) {
        self.running.store(running, Ordering::Relaxed);
    }

    /// Apply configuration without restarting tasks that already exist.
    ///
    /// Existing tasks read `configs` on every tick, so host, protocol, and
    /// timeout edits take effect without a Save-induced pause. Only targets that
    /// were deleted, disabled, or moved to another overlay are stopped, and
    /// newly enabled targets are started.
    pub fn apply_config(&mut self, config: &Config) {
        let enabled: HashMap<TaskKey, TargetConfig> = config
            .overlays
            .iter()
            .filter(|overlay| overlay.enabled)
            .flat_map(|overlay| {
                overlay
                    .targets
                    .iter()
                    .filter(|target| target.enabled)
                    .map(|target| (TaskKey::new(&overlay.id, &target.id), target.clone()))
            })
            .collect();

        *self.configs.write().unwrap() = enabled.clone();

        let wanted: std::collections::HashSet<TaskKey> = enabled.keys().cloned().collect();
        self.tasks.retain(|key, task| {
            if wanted.contains(key) {
                true
            } else {
                task.abort();
                false
            }
        });

        for (key, _target) in enabled {
            if self.tasks.contains_key(&key) {
                continue;
            }

            let configs = Arc::clone(&self.configs);
            let samples = Arc::clone(&self.samples);
            let running = Arc::clone(&self.running);
            let task_key = key.clone();
            let task = self.handle.spawn(async move {
                probe_loop(task_key, configs, samples, running).await;
            });
            self.tasks.insert(key, task);
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
    key: TaskKey,
    configs: Arc<RwLock<HashMap<TaskKey, TargetConfig>>>,
    samples: SampleStore,
    running: Arc<AtomicBool>,
) {
    loop {
        let target = configs.read().unwrap().get(&key).cloned();
        if running.load(Ordering::Relaxed) {
            if let Some(target) = target.as_ref() {
                let latency = probe::measure(&target.probe, target.timeout_ms).await;
                let mut all_samples = samples.lock().unwrap();
                let overlay = all_samples.entry(key.overlay_id.clone()).or_default();
                let buffer = overlay.entry(key.target_id.clone()).or_default();
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
    use crate::config::{OverlayConfig, ProbeConfig};

    fn overlay_with_targets(count: usize) -> OverlayConfig {
        let mut overlay = OverlayConfig::new();
        overlay.targets.truncate(1);
        for index in 1..count {
            let mut target = overlay.targets[0].clone();
            target.id = format!("{}-target-{index}", overlay.id);
            target.probe = ProbeConfig::Tcp {
                host: format!("127.0.0.{index}"),
                port: 1,
            };
            target.timeout_ms = 1;
            overlay.targets.push(target);
        }
        overlay
    }

    fn key_of(overlay: &OverlayConfig, index: usize) -> TaskKey {
        TaskKey::new(&overlay.id, &overlay.targets[index].id)
    }

    /// Every enabled target is probed, so a group does not silently graph only
    /// its first host.
    #[test]
    fn every_enabled_target_gets_its_own_task() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let mut manager = ProbeManager::new(runtime.handle().clone());
        let overlay = overlay_with_targets(3);
        let config = Config {
            overlays: vec![overlay.clone()],
            ..Config::default()
        };
        manager.apply_config(&config);

        assert_eq!(manager.tasks.len(), 3);
        for index in 0..3 {
            assert!(
                manager.tasks.contains_key(&key_of(&overlay, index)),
                "target {index} has no task"
            );
        }
        manager.stop_all();
        runtime.shutdown_background();
    }

    /// The AGENTS.md rule, extended to a group: a style-only Save must not
    /// interrupt a measurement. Compared by task id, because two `JoinHandle`s
    /// that are equal in every other way are the same task.
    #[test]
    fn style_only_config_update_keeps_existing_task() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let mut manager = ProbeManager::new(runtime.handle().clone());
        let mut overlay = overlay_with_targets(2);
        let mut config = Config {
            overlays: vec![overlay.clone()],
            ..Config::default()
        };
        manager.apply_config(&config);
        let first: Vec<_> = (0..2)
            .map(|index| {
                let key = key_of(&overlay, index);
                manager.tasks[&key].id()
            })
            .collect();

        overlay.scale = 3;
        config.overlays[0] = overlay.clone();
        manager.apply_config(&config);

        for (index, original) in first.iter().enumerate() {
            let key = key_of(&overlay, index);
            assert_eq!(
                manager.tasks[&key].id(),
                *original,
                "target {index} was restarted by a style-only change"
            );
        }
        manager.stop_all();
        runtime.shutdown_background();
    }

    /// Disabling one target of a group stops that target's task and leaves the
    /// others running. This is the case a single-target overlay could not have.
    #[test]
    fn disabling_one_target_stops_only_its_task() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let mut manager = ProbeManager::new(runtime.handle().clone());
        let mut overlay = overlay_with_targets(3);
        let mut config = Config {
            overlays: vec![overlay.clone()],
            ..Config::default()
        };
        manager.apply_config(&config);
        let survivor = manager.tasks[&key_of(&overlay, 0)].id();

        overlay.targets[1].enabled = false;
        config.overlays[0] = overlay.clone();
        manager.apply_config(&config);

        assert!(!manager.tasks.contains_key(&key_of(&overlay, 1)));
        assert_eq!(manager.tasks[&key_of(&overlay, 0)].id(), survivor);
        assert!(manager.tasks.contains_key(&key_of(&overlay, 2)));
        manager.stop_all();
        runtime.shutdown_background();
    }

    /// Two overlays may each have a target with the same id, because the key is
    /// the pair. Keying on the target id alone would make one of them look
    /// like the other and stop it.
    #[test]
    fn the_same_target_id_in_two_overlays_is_two_tasks() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let mut manager = ProbeManager::new(runtime.handle().clone());
        let first = overlay_with_targets(1);
        let mut second = overlay_with_targets(1);
        second.targets[0].id = first.targets[0].id.clone();
        let config = Config {
            overlays: vec![first.clone(), second.clone()],
            ..Config::default()
        };
        manager.apply_config(&config);

        assert_eq!(manager.tasks.len(), 2);
        assert_ne!(
            manager.tasks[&key_of(&first, 0)].id(),
            manager.tasks[&key_of(&second, 0)].id()
        );
        manager.stop_all();
        runtime.shutdown_background();
    }

    /// Samples land under the overlay and then the target, so the renderer can
    /// read a whole group under one lock.
    #[test]
    fn a_probe_reaches_the_nested_buffer() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let mut manager = ProbeManager::new(runtime.handle().clone());
        let overlay = overlay_with_targets(1);
        let key = key_of(&overlay, 0);
        let config = Config {
            overlays: vec![overlay.clone()],
            ..Config::default()
        };
        manager.apply_config(&config);

        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let store = manager.samples().lock().unwrap();
            let generation = store
                .get(&overlay.id)
                .and_then(|targets| targets.get(&key.target_id))
                .map(|buffer| buffer.generation)
                .unwrap_or(0);
            drop(store);
            if generation > 0 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "no sample reached {overlay_id}/{target_id}",
                overlay_id = overlay.id,
                target_id = key.target_id
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        manager.stop_all();
        runtime.shutdown_background();
    }
}
