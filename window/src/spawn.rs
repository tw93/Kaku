#[cfg(target_os = "macos")]
use core_foundation::runloop::*;
use promise::spawn::{Runnable, SpawnFunc};
use std::collections::VecDeque;
#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

lazy_static::lazy_static! {
    pub(crate) static ref SPAWN_QUEUE: Arc<SpawnQueue> = Arc::new(SpawnQueue::new().expect("failed to create SpawnQueue"));
}

pub(crate) fn run_spawn_queue_burst(max_funcs: usize) -> bool {
    SPAWN_QUEUE.run_burst(max_funcs)
}

struct InstrumentedSpawnFunc {
    func: SpawnFunc,
    at: Instant,
}

pub(crate) struct SpawnQueue {
    spawned_funcs: Mutex<VecDeque<InstrumentedSpawnFunc>>,
    spawned_funcs_low_pri: Mutex<VecDeque<InstrumentedSpawnFunc>>,
}

fn schedule_with_pri(runnable: Runnable, high_pri: bool) {
    SPAWN_QUEUE.spawn_impl(
        Box::new(move || {
            runnable.run();
        }),
        high_pri,
    );
}

impl SpawnQueue {
    fn lock_recover<'a, T>(mutex: &'a Mutex<T>, name: &str) -> MutexGuard<'a, T> {
        match mutex.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                log::error!(
                    "spawn queue: mutex `{}` poisoned; recovering and continuing",
                    name
                );
                poisoned.into_inner()
            }
        }
    }

    pub fn new() -> anyhow::Result<Self> {
        Self::new_impl()
    }

    pub fn register_promise_schedulers(&self) {
        promise::spawn::set_schedulers(
            Box::new(|runnable| {
                schedule_with_pri(runnable, true);
            }),
            Box::new(|runnable| {
                schedule_with_pri(runnable, false);
            }),
        );
    }

    pub fn run(&self) -> bool {
        self.run_impl()
    }

    pub fn run_burst(&self, max_funcs: usize) -> bool {
        self.run_impl_burst(max_funcs)
    }

    // This needs to be a separate function from the loop in `run`
    // in order for the lock to be released before we call the
    // returned function. Used by Linux/Windows run_impl; macOS uses batch drain.
    #[allow(dead_code)]
    fn pop_func(&self) -> Option<SpawnFunc> {
        if let Some(func) = Self::lock_recover(&self.spawned_funcs, "spawned_funcs").pop_front() {
            metrics::histogram!("executor.spawn_delay").record(func.at.elapsed());
            Some(func.func)
        } else if let Some(func) =
            Self::lock_recover(&self.spawned_funcs_low_pri, "spawned_funcs_low_pri").pop_front()
        {
            metrics::histogram!("executor.spawn_delay.low_pri").record(func.at.elapsed());
            Some(func.func)
        } else {
            None
        }
    }

    fn queue_func(&self, f: SpawnFunc, high_pri: bool) {
        let f = InstrumentedSpawnFunc {
            func: f,
            at: Instant::now(),
        };
        let mut queue = if high_pri {
            Self::lock_recover(&self.spawned_funcs, "spawned_funcs")
        } else {
            Self::lock_recover(&self.spawned_funcs_low_pri, "spawned_funcs_low_pri")
        };
        queue.push_back(f);
    }

    fn has_any_queued(&self) -> bool {
        !Self::lock_recover(&self.spawned_funcs, "spawned_funcs").is_empty()
            || !Self::lock_recover(&self.spawned_funcs_low_pri, "spawned_funcs_low_pri").is_empty()
    }
}

#[cfg(target_os = "macos")]
impl SpawnQueue {
    fn new_impl() -> anyhow::Result<Self> {
        let spawned_funcs = Mutex::new(VecDeque::new());
        let spawned_funcs_low_pri = Mutex::new(VecDeque::new());

        // Fire on AfterWaiting (runloop just woke up) so spawn callbacks triggered
        // by keyboard/timer events are processed immediately in the same turn,
        // keeping input latency low. BeforeWaiting clears any remaining items
        // before the runloop sleeps again. Using both is cheaper than
        // kCFRunLoopAllActivities while still being responsive.
        let observer = unsafe {
            CFRunLoopObserverCreate(
                std::ptr::null(),
                kCFRunLoopBeforeWaiting | kCFRunLoopAfterWaiting,
                1,
                0,
                SpawnQueue::trigger,
                std::ptr::null_mut(),
            )
        };
        unsafe {
            CFRunLoopAddObserver(CFRunLoopGetMain(), observer, kCFRunLoopCommonModes);
        }

        Ok(Self {
            spawned_funcs,
            spawned_funcs_low_pri,
        })
    }

    extern "C" fn trigger(
        _observer: *mut __CFRunLoopObserver,
        _: CFRunLoopActivity,
        _: *mut std::ffi::c_void,
    ) {
        OBSERVER_LAST_FIRED.store(monotonic_ms(), Ordering::Relaxed);
        if SPAWN_QUEUE.run() {
            Self::queue_wakeup();
        }
    }

    fn queue_wakeup() {
        unsafe {
            CFRunLoopWakeUp(CFRunLoopGetMain());
        }
        if DISPATCH_FALLBACK.load(Ordering::Relaxed) {
            dispatch_drain();
        }
    }

    fn spawn_impl(&self, f: SpawnFunc, high_pri: bool) {
        self.queue_func(f, high_pri);
        Self::queue_wakeup();
    }

    fn queued_counts(&self) -> (usize, usize) {
        (
            Self::lock_recover(&self.spawned_funcs, "spawned_funcs").len(),
            Self::lock_recover(&self.spawned_funcs_low_pri, "spawned_funcs_low_pri").len(),
        )
    }

    fn run_impl(&self) -> bool {
        // Process a burst each wakeup so chained high-priority GUI work
        // (SpawnWindow -> reconcile -> new_window) doesn't get stretched
        // across multiple runloop turns.
        self.run_impl_burst(64)
    }

    fn run_impl_burst(&self, max_funcs: usize) -> bool {
        // Drain both queues under a single lock each so the burst loop
        // does 2 lock acquisitions instead of up to 2*max_funcs.
        let mut batch: Vec<SpawnFunc> = {
            let mut q = Self::lock_recover(&self.spawned_funcs, "spawned_funcs");
            let n = max_funcs.min(q.len());
            q.drain(..n)
                .map(|f| {
                    metrics::histogram!("executor.spawn_delay").record(f.at.elapsed());
                    f.func
                })
                .collect()
        };
        let remaining = max_funcs.saturating_sub(batch.len());
        if remaining > 0 {
            let mut q = Self::lock_recover(&self.spawned_funcs_low_pri, "spawned_funcs_low_pri");
            let n = remaining.min(q.len());
            batch.extend(q.drain(..n).map(|f| {
                metrics::histogram!("executor.spawn_delay.low_pri").record(f.at.elapsed());
                f.func
            }));
        }
        for func in batch {
            func();
        }
        self.has_any_queued()
    }
}

// In the field (V0.25.0 and V0.27.0) the GUI froze after an update
// notification click: the main thread sat idle in AppKit's event loop while
// tasks queued here were never run, so keys, scrolling and tab switches did
// nothing until a force quit, though menus still opened. A V0.27.0 capture
// traced this to show() clearing a native fullscreen style: AppKit threw
// through the observer callback and it stopped firing (K-18/K-22). Keep the
// dispatch rescue for stranded tasks: it wakes the run loop through its own
// port; if the observer then stays silent while the main thread is free,
// every later wakeup also goes through the dispatch queue.
#[cfg(target_os = "macos")]
static OBSERVER_LAST_FIRED: AtomicU64 = AtomicU64::new(0);
#[cfg(target_os = "macos")]
static DISPATCH_FALLBACK: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "macos")]
fn monotonic_ms() -> u64 {
    lazy_static::lazy_static! {
        static ref EPOCH: Instant = Instant::now();
    }
    // Start at 1 so that 0 keeps meaning "never fired".
    EPOCH.elapsed().as_millis() as u64 + 1
}

/// The check runs on the main thread `SILENCE_CHECK_DELAY_NS` after a rescue
/// drain. A free main thread goes to sleep, and fires the observer, many
/// times in that window, so no fire since the request means it is dead. The
/// drain itself cannot decide: CFRunLoop serves a pending dispatch message
/// without the waiting observers, so it can run ahead of a healthy observer.
#[cfg(target_os = "macos")]
fn observer_went_silent(last_fired: u64, requested: u64) -> bool {
    last_fired < requested
}

#[cfg(target_os = "macos")]
const SILENCE_CHECK_DELAY_NS: i64 = 500_000_000;

#[cfg(target_os = "macos")]
mod dispatch {
    use std::ffi::c_void;

    #[repr(C)]
    pub struct DispatchQueue {
        _private: [u8; 0],
    }

    pub const DISPATCH_TIME_NOW: u64 = 0;

    extern "C" {
        #[link_name = "_dispatch_main_q"]
        pub static MAIN_QUEUE: DispatchQueue;
        pub fn dispatch_async_f(
            queue: *const DispatchQueue,
            context: *mut c_void,
            work: extern "C" fn(*mut c_void),
        );
        pub fn dispatch_after_f(
            when: u64,
            queue: *const DispatchQueue,
            context: *mut c_void,
            work: extern "C" fn(*mut c_void),
        );
        pub fn dispatch_time(when: u64, delta: i64) -> u64;
    }
}

#[cfg(target_os = "macos")]
fn dispatch_drain() {
    unsafe {
        dispatch::dispatch_async_f(
            &dispatch::MAIN_QUEUE,
            std::ptr::null_mut(),
            drain_on_main_queue,
        );
    }
}

#[cfg(target_os = "macos")]
extern "C" fn drain_on_main_queue(_: *mut std::ffi::c_void) {
    if SPAWN_QUEUE.run() {
        dispatch_drain();
    }
}

#[cfg(target_os = "macos")]
extern "C" fn rescue_on_main_queue(context: *mut std::ffi::c_void) {
    drain_on_main_queue(std::ptr::null_mut());
    unsafe {
        dispatch::dispatch_after_f(
            dispatch::dispatch_time(dispatch::DISPATCH_TIME_NOW, SILENCE_CHECK_DELAY_NS),
            &dispatch::MAIN_QUEUE,
            context,
            check_observer_silence,
        );
    }
}

#[cfg(target_os = "macos")]
extern "C" fn check_observer_silence(context: *mut std::ffi::c_void) {
    let requested = context as usize as u64;
    let last_fired = OBSERVER_LAST_FIRED.load(Ordering::Relaxed);
    if observer_went_silent(last_fired, requested)
        && !DISPATCH_FALLBACK.swap(true, Ordering::Relaxed)
    {
        log::error!(
            "main thread queue: run loop observer silent for {}ms, \
             draining through the main dispatch queue from now on",
            monotonic_ms().saturating_sub(last_fired)
        );
        drain_on_main_queue(std::ptr::null_mut());
    }
}

/// Called by the stall watchdog while a task it posted is still waiting:
/// drain once through the main dispatch queue, then check whether the
/// observer is still firing.
#[cfg(target_os = "macos")]
pub fn rescue_main_thread_queue() {
    unsafe {
        dispatch::dispatch_async_f(
            &dispatch::MAIN_QUEUE,
            monotonic_ms() as usize as *mut std::ffi::c_void,
            rescue_on_main_queue,
        );
    }
}

/// One line for the stall log: how long since the observer drained, and how
/// much is waiting.
#[cfg(target_os = "macos")]
pub fn main_thread_queue_state() -> String {
    let last_fired = OBSERVER_LAST_FIRED.load(Ordering::Relaxed);
    let (high, low) = SPAWN_QUEUE.queued_counts();
    format!(
        "observer last fired {}ms ago, {high}+{low} tasks queued, dispatch fallback {}",
        monotonic_ms().saturating_sub(last_fired),
        if DISPATCH_FALLBACK.load(Ordering::Relaxed) {
            "on"
        } else {
            "off"
        }
    )
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn a_drain_with_no_observer_fire_since_the_request_means_silence() {
        assert!(observer_went_silent(100, 5000));
        assert!(!observer_went_silent(5000, 5000));
        assert!(!observer_went_silent(5001, 5000));
        // Never fired at all.
        assert!(observer_went_silent(0, 1));
    }
}
