use std::collections::{HashMap, HashSet, VecDeque};
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::runtime::Handle;
use tokio::task::JoinHandle;

use crate::config::{Config, ProbeConfig, TargetConfig};
use crate::probe;
use crate::render::{SamplePoint, SAMPLE_INTERVAL};

/// Probe tasks keep their own cadence — `SAMPLE_INTERVAL`, the graph's one
/// second tick — and are never restarted just because the user saved a style or
/// graph setting.
const MAX_BUFFERED_SAMPLES: usize = 86_400;

/// How long a target's resolved address may be used before a background
/// refresh replaces it.
///
/// Nothing about a lookup may sit on the sampling path (see `AddressCache`),
/// so this is only about staying current with a rotating record: the game
/// endpoints this exists for are Cloudflare names whose A records live for
/// well under a minute.
const DNS_REFRESH_INTERVAL: Duration = Duration::from_secs(60);

/// Consecutive failed probes that force an early refresh.
///
/// A record can rotate to an edge that no longer answers before the periodic
/// refresh arrives. Three misses at the one-second cadence is a few seconds of
/// red markers, not a minute.
const DNS_REFRESH_AFTER_FAILURES: u32 = 3;

/// A lookup slower than this is written to the log, when logging is enabled.
///
/// It is exactly the stall the cached address exists to absorb, so the line is
/// the evidence that the cache is earning its keep.
const SLOW_LOOKUP: Duration = Duration::from_millis(500);

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
///
/// It also travels on the wire, inside a `SetConfig`'s `retire` list: the
/// removals a Save has committed, which are exactly the probes the renderer
/// must stop keeping. Serialising it here rather than with a parallel
/// `(String, String)` shape is what keeps the two ends naming the same target.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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

/// Every enabled target with its key: exactly the set `ProbeManager` runs a
/// task for.
///
/// The one definition of "enabled target", shared by `apply_config` and by
/// `enabled_target_keys`, so the set the manager probes and the set the window
/// diffs a save against cannot disagree.
fn enabled_target_configs(config: &Config) -> impl Iterator<Item = (TaskKey, TargetConfig)> + '_ {
    config
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
}

/// The keys of a config's enabled targets.
///
/// The window diffs a save against this: the keys the renderer was told to
/// keep before the save, minus these, are exactly the probes a Save removes.
pub fn enabled_target_keys(config: &Config) -> HashSet<TaskKey> {
    enabled_target_configs(config).map(|(key, _)| key).collect()
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
    /// Targets that left the active config while background tracking was on.
    ///
    /// Their tasks keep running because `probe_loop` reads `configs`, which is
    /// the active targets merged with this map. A profile switch moves the whole
    /// profile it left behind in here; a removal the user saved arrives in
    /// `retire` and leaves for good.
    background: HashMap<TaskKey, TargetConfig>,
    /// The enabled targets of the last applied config.
    ///
    /// Kept so an apply can tell a departure from a newcomer: only a key that
    /// was here and is not in the new config is a departure worth backgrounding.
    active: HashMap<TaskKey, TargetConfig>,
}

impl ProbeManager {
    pub fn new(handle: Handle) -> Self {
        Self {
            handle,
            configs: Arc::new(RwLock::new(HashMap::new())),
            samples: Arc::new(Mutex::new(HashMap::new())),
            tasks: HashMap::new(),
            running: Arc::new(AtomicBool::new(true)),
            background: HashMap::new(),
            active: HashMap::new(),
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
    ///
    /// With `background_tracking` on, a target that leaves the config keeps its
    /// task and its samples instead of being stopped — the window may still
    /// discard the edit that removed it, and a profile left behind by a switch
    /// is the case the setting exists for. `retire` names the removals a Save
    /// has committed, and those are let go for good. With the setting off,
    /// nothing is kept and anything kept before is dropped.
    pub fn apply_config(&mut self, config: &Config, background_tracking: bool, retire: &[TaskKey]) {
        let enabled: HashMap<TaskKey, TargetConfig> = enabled_target_configs(config).collect();

        // What leaves this config stays probed only when the setting is on.
        if background_tracking {
            for (key, target) in &self.active {
                if !enabled.contains_key(key) {
                    self.background
                        .entry(key.clone())
                        .or_insert_with(|| target.clone());
                }
            }
        } else {
            self.background.clear();
        }
        for key in retire {
            self.background.remove(key);
        }
        for key in enabled.keys() {
            self.background.remove(key);
        }
        self.active = enabled.clone();

        // The tasks read this map per tick, so it is the merged view: kept
        // targets keep measuring while they are out of the active config.
        let mut running_configs = self.background.clone();
        running_configs.extend(enabled);
        *self.configs.write().unwrap() = running_configs;

        let wanted: HashSet<TaskKey> = self.configs.read().unwrap().keys().cloned().collect();
        self.tasks.retain(|key, task| {
            if wanted.contains(key) {
                true
            } else {
                task.abort();
                false
            }
        });

        for key in wanted {
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

/// The IPv4 address a probe task is probing — ICMP pings it, TCP connects to
/// it — and the background lookup that keeps it current.
///
/// `to_socket_addrs` can block for seconds on a cold cache, and a blocked
/// sampling loop writes no sample — a silent break in the graph with no
/// timeout marker to explain it. So the address is resolved once, up front,
/// and refreshed from a detached task; the loop only ever reads what is
/// already there.
struct AddressCache {
    host: String,
    addr: Option<IpAddr>,
    looked_up_at: Instant,
    refreshing: Option<Receiver<Option<IpAddr>>>,
    failures: u32,
}

impl Default for AddressCache {
    fn default() -> Self {
        Self {
            host: String::new(),
            addr: None,
            looked_up_at: Instant::now(),
            refreshing: None,
            failures: 0,
        }
    }
}

impl AddressCache {
    /// Point the cache at `host`, dropping the address if the host changed.
    ///
    /// A host edit invalidates the address the same tick it arrives; keeping it
    /// would keep pinging a name the user replaced.
    fn rehost(&mut self, host: &str) {
        if self.host != host {
            self.host = host.to_owned();
            self.addr = None;
            self.looked_up_at = Instant::now();
            self.refreshing = None;
            self.failures = 0;
        }
    }

    /// Take a finished background lookup's result, if it has finished.
    fn take_refresh(&mut self, now: Instant) {
        let Some(pending) = self.refreshing.as_ref() else {
            return;
        };
        let result = pending.try_recv();
        match result {
            Ok(Some(addr)) => {
                self.addr = Some(addr);
                self.looked_up_at = now;
                self.failures = 0;
                self.refreshing = None;
            }
            Ok(None) => {
                // The lookup failed. Keep pinging the address that worked and
                // try again on the normal clock; clearing the failure run is
                // what stops the next tick from asking for another refresh
                // immediately.
                self.looked_up_at = now;
                self.failures = 0;
                self.refreshing = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                // The refresh task is gone without an answer; ask again.
                self.refreshing = None;
            }
        }
    }
}

/// What a probe task should do about its cached address on this tick.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum AddressAction {
    /// No address yet: resolve it now and use the result.
    Resolve,
    /// Keep using the cached address and start a background refresh.
    Refresh,
    /// Use the cached address as it is.
    Use,
}

/// The address decision, split out so the full table is testable: a refresh
/// already in flight is never doubled up, and a short failure run does not
/// force one.
fn address_action(
    has_address: bool,
    refreshing: bool,
    overdue: bool,
    failures: u32,
) -> AddressAction {
    if !has_address {
        AddressAction::Resolve
    } else if !refreshing && (overdue || failures >= DNS_REFRESH_AFTER_FAILURES) {
        AddressAction::Refresh
    } else {
        AddressAction::Use
    }
}

/// One probe tick through the address cache.
///
/// Nothing here can delay the sample on a lookup except the first one, before
/// there is an address to fall back on; every later refresh is a detached task
/// whose result the next tick picks up. Both protocols probe the cached
/// address directly — ICMP pings it, TCP connects to it.
async fn measure_cached(
    cache: &mut AddressCache,
    probe: &ProbeConfig,
    timeout_ms: u32,
) -> Option<u32> {
    let host = probe.host();
    cache.rehost(host);
    let now = Instant::now();
    cache.take_refresh(now);

    match address_action(
        cache.addr.is_some(),
        cache.refreshing.is_some(),
        now.saturating_duration_since(cache.looked_up_at) > DNS_REFRESH_INTERVAL,
        cache.failures,
    ) {
        AddressAction::Resolve => {
            cache.addr = probe::lookup_ipv4(host).await;
            cache.looked_up_at = Instant::now();
            cache.failures = 0;
        }
        AddressAction::Refresh => {
            let (sender, receiver) = mpsc::channel();
            let host = host.to_owned();
            tokio::spawn(async move {
                let started = Instant::now();
                let resolved = probe::lookup_ipv4(&host).await;
                let elapsed = started.elapsed();
                if elapsed > SLOW_LOOKUP {
                    crate::diagnostics::log_line(
                        "renderer",
                        &format!("lookup for {host} took {elapsed:?}"),
                    );
                }
                let _ = sender.send(resolved);
            });
            cache.refreshing = Some(receiver);
        }
        AddressAction::Use => {}
    }

    let latency = match cache.addr {
        Some(addr) => match probe {
            ProbeConfig::Icmp { .. } => probe::ping_ipv4(addr, timeout_ms).await,
            ProbeConfig::Tcp { port, .. } => probe::connect_ipv4(addr, *port, timeout_ms).await,
        },
        None => None,
    };
    if latency.is_none() {
        cache.failures = cache.failures.saturating_add(1);
    } else {
        cache.failures = 0;
    }
    latency
}

async fn probe_loop(
    key: TaskKey,
    configs: Arc<RwLock<HashMap<TaskKey, TargetConfig>>>,
    samples: SampleStore,
    running: Arc<AtomicBool>,
) {
    let mut address = AddressCache::default();
    loop {
        let target = configs.read().unwrap().get(&key).cloned();
        if running.load(Ordering::Relaxed) {
            if let Some(target) = target.as_ref() {
                let latency = measure_cached(&mut address, &target.probe, target.timeout_ms).await;
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
        tokio::time::sleep(SAMPLE_INTERVAL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{OverlayConfig, ProbeConfig};
    use std::net::Ipv4Addr;
    use std::time::Duration;

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
        manager.apply_config(&config, false, &[]);

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
        manager.apply_config(&config, false, &[]);
        let first: Vec<_> = (0..2)
            .map(|index| {
                let key = key_of(&overlay, index);
                manager.tasks[&key].id()
            })
            .collect();

        overlay.scale = 3;
        config.overlays[0] = overlay.clone();
        manager.apply_config(&config, false, &[]);

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
        manager.apply_config(&config, false, &[]);
        let survivor = manager.tasks[&key_of(&overlay, 0)].id();

        overlay.targets[1].enabled = false;
        config.overlays[0] = overlay.clone();
        manager.apply_config(&config, false, &[]);

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
        manager.apply_config(&config, false, &[]);

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
        manager.apply_config(&config, false, &[]);

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

    /// A profile switch leaves the old profile's targets probed when the
    /// setting is on: the tasks survive and the merged map still names them,
    /// which is what `probe_loop` reads every tick.
    #[test]
    fn a_departure_keeps_its_task_when_background_tracking_is_on() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let mut manager = ProbeManager::new(runtime.handle().clone());
        let left = overlay_with_targets(2);
        let first_config = Config {
            overlays: vec![left.clone()],
            ..Config::default()
        };
        manager.apply_config(&first_config, true, &[]);
        let original: Vec<_> = (0..2)
            .map(|index| manager.tasks[&key_of(&left, index)].id())
            .collect();

        let other = overlay_with_targets(1);
        let second_config = Config {
            overlays: vec![other.clone()],
            ..Config::default()
        };
        manager.apply_config(&second_config, true, &[]);

        for (index, task) in original.iter().enumerate() {
            let key = key_of(&left, index);
            assert_eq!(
                manager.tasks[&key].id(),
                *task,
                "the profile that was switched away from lost target {index}"
            );
            assert!(
                manager.configs.read().unwrap().contains_key(&key),
                "the kept target left the map the probe loop reads, so it \
                 stopped measuring"
            );
        }
        assert!(
            manager.tasks.contains_key(&key_of(&other, 0)),
            "the new profile was not probed"
        );
        manager.stop_all();
        runtime.shutdown_background();
    }

    /// A saved removal is the one departure that does not stay: the key arrives
    /// in `retire` and its task goes away for good.
    #[test]
    fn a_saved_removal_retires_the_kept_target() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let mut manager = ProbeManager::new(runtime.handle().clone());
        let mut overlay = overlay_with_targets(2);
        let config = Config {
            overlays: vec![overlay.clone()],
            ..Config::default()
        };
        manager.apply_config(&config, true, &[]);

        // The user unticks host 1. The edit is unsaved, so it keeps probing.
        overlay.targets[1].enabled = false;
        let trimmed = Config {
            overlays: vec![overlay.clone()],
            ..Config::default()
        };
        manager.apply_config(&trimmed, true, &[]);
        let kept = key_of(&overlay, 1);
        assert!(
            manager.tasks.contains_key(&kept),
            "an unsaved removal stopped the probe"
        );

        // Then saves: the removal is committed, so it is retired.
        manager.apply_config(&trimmed, true, std::slice::from_ref(&kept));
        assert!(
            !manager.tasks.contains_key(&kept),
            "a saved removal was still being probed"
        );
        assert!(
            manager.tasks.contains_key(&key_of(&overlay, 0)),
            "the target that stayed was stopped too"
        );
        manager.stop_all();
        runtime.shutdown_background();
    }

    /// Turning the setting off drops everything that was being kept, which is
    /// the whole of the "off" promise: departures stop immediately.
    #[test]
    fn background_tracking_off_drops_what_was_kept() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let mut manager = ProbeManager::new(runtime.handle().clone());
        let left = overlay_with_targets(1);
        let first_config = Config {
            overlays: vec![left.clone()],
            ..Config::default()
        };
        manager.apply_config(&first_config, true, &[]);

        let other = overlay_with_targets(1);
        let second_config = Config {
            overlays: vec![other.clone()],
            ..Config::default()
        };
        manager.apply_config(&second_config, true, &[]);

        manager.apply_config(&second_config, false, &[]);
        assert!(
            !manager.tasks.contains_key(&key_of(&left, 0)),
            "a kept target outlived the setting being switched off"
        );
        assert!(
            manager.tasks.contains_key(&key_of(&other, 0)),
            "switching the setting off stopped the active profile too"
        );
        manager.stop_all();
        runtime.shutdown_background();
    }

    /// Coming back is a return, not a restart: the task that kept probing is
    /// reused, so the samples it collected land in the same buffer.
    #[test]
    fn a_returning_target_reuses_its_kept_task() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let mut manager = ProbeManager::new(runtime.handle().clone());
        let profile = overlay_with_targets(1);
        let config = Config {
            overlays: vec![profile.clone()],
            ..Config::default()
        };
        manager.apply_config(&config, true, &[]);
        let id = manager.tasks[&key_of(&profile, 0)].id();

        let other = overlay_with_targets(1);
        let other_config = Config {
            overlays: vec![other.clone()],
            ..Config::default()
        };
        manager.apply_config(&other_config, true, &[]);
        manager.apply_config(&config, true, &[]);

        assert_eq!(
            manager.tasks[&key_of(&profile, 0)].id(),
            id,
            "the returning target was restarted instead of reusing the task \
             that kept probing"
        );
        manager.stop_all();
        runtime.shutdown_background();
    }

    /// The refresh decision, as a table. A refresh already in flight is never
    /// doubled up, and a short failure run does not force one.
    #[test]
    fn the_address_action_resolves_refreshes_or_reuses() {
        let cases = [
            (false, false, false, 0, AddressAction::Resolve),
            (true, false, false, 0, AddressAction::Use),
            (true, false, true, 0, AddressAction::Refresh),
            (true, true, true, 0, AddressAction::Use),
            (
                true,
                false,
                false,
                DNS_REFRESH_AFTER_FAILURES,
                AddressAction::Refresh,
            ),
            (
                true,
                false,
                false,
                DNS_REFRESH_AFTER_FAILURES - 1,
                AddressAction::Use,
            ),
            (
                true,
                true,
                false,
                DNS_REFRESH_AFTER_FAILURES,
                AddressAction::Use,
            ),
        ];
        for (has_address, refreshing, overdue, failures, expected) in cases {
            assert_eq!(
                address_action(has_address, refreshing, overdue, failures),
                expected,
                "has={has_address} refreshing={refreshing} overdue={overdue} failures={failures}"
            );
        }
    }

    /// A background lookup's result lands in the cache; a failed one keeps the
    /// address that is still being pinged, and clears the pending slot or the
    /// next tick would never ask again.
    #[test]
    fn a_finished_lookup_lands_in_the_cache() {
        let first = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7));
        let second = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 8));
        let mut cache = AddressCache {
            host: "example.invalid".to_owned(),
            addr: Some(first),
            ..AddressCache::default()
        };

        let (sender, receiver) = mpsc::channel();
        cache.refreshing = Some(receiver);
        sender.send(Some(second)).expect("send");
        cache.take_refresh(Instant::now());
        assert_eq!(cache.addr, Some(second));
        assert!(
            cache.refreshing.is_none(),
            "a finished refresh stayed pending"
        );

        let (sender, receiver) = mpsc::channel();
        cache.refreshing = Some(receiver);
        cache.failures = 2;
        sender.send(None).expect("send");
        cache.take_refresh(Instant::now());
        assert_eq!(
            cache.addr,
            Some(second),
            "a failed lookup dropped the address"
        );
        assert!(
            cache.refreshing.is_none(),
            "a failed refresh stayed pending"
        );
        assert_eq!(cache.failures, 0);
    }

    /// A host edit drops the address with the name it belonged to.
    #[test]
    fn changing_the_host_drops_the_cached_address() {
        let mut cache = AddressCache {
            host: "old.invalid".to_owned(),
            addr: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
            failures: 4,
            ..AddressCache::default()
        };

        cache.rehost("new.invalid");
        assert_eq!(cache.addr, None);
        assert_eq!(cache.failures, 0);

        cache.addr = Some(IpAddr::V4(Ipv4Addr::LOCALHOST));
        cache.rehost("new.invalid");
        assert_eq!(
            cache.addr,
            Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
            "re-pointing at the same host must not drop the address"
        );
    }

    /// The first tick of a literal-IP host fills the cache without a lookup,
    /// so an address is in hand before any refresh machinery runs.
    #[test]
    fn a_literal_ip_host_caches_its_address() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        runtime.block_on(async {
            let mut cache = AddressCache::default();
            let probe = ProbeConfig::Icmp {
                host: "127.0.0.1".to_owned(),
            };
            let _ = measure_cached(&mut cache, &probe, 1).await;
            assert_eq!(cache.addr, Some(IpAddr::V4(Ipv4Addr::LOCALHOST)));
            assert!(cache.refreshing.is_none());
        });
        runtime.shutdown_background();
    }

    /// The TCP arm connects to the cached address directly; a literal-IP host
    /// caches without a lookup and the handshake completes against a real
    /// listener.
    #[test]
    fn a_tcp_target_connects_to_its_cached_address() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("listener");
        let port = listener.local_addr().expect("local address").port();
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        runtime.block_on(async {
            let mut cache = AddressCache::default();
            let probe = ProbeConfig::Tcp {
                host: "127.0.0.1".to_owned(),
                port,
            };
            let latency = measure_cached(&mut cache, &probe, 1_000).await;
            assert!(
                latency.is_some(),
                "connecting to a listening socket must measure"
            );
            assert_eq!(cache.addr, Some(IpAddr::V4(Ipv4Addr::LOCALHOST)));
            assert!(cache.refreshing.is_none());
        });
        runtime.shutdown_background();
    }
}
