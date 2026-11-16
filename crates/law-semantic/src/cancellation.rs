//! Opt-in cooperative cancellation; no process-wide signal handlers are installed.
use std::cell::RefCell;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

thread_local! {
    static FLAG: RefCell<Option<Arc<AtomicBool>>> = const { RefCell::new(None) };
}

/// Run with a cancellation flag on this thread. Nested scopes and unwinding
/// restore the previous flag. Spawned threads do not inherit this scope.
/// Git polls the flag before spawning and while waiting; other work may finish
/// before cancellation is observed. Set the flag to true and never reset it.
/// A failure to stop or reap git remains an environment error rather than a
/// cancellation error; cancellation does not guarantee successful cleanup.
pub fn with_flag<T>(flag: Arc<AtomicBool>, run: impl FnOnce() -> T) -> T {
    struct Restore(Option<Arc<AtomicBool>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            FLAG.with(|slot| {
                slot.replace(self.0.take());
            });
        }
    }
    let _restore = Restore(FLAG.with(|slot| slot.replace(Some(flag))));
    run()
}

/// Whether the current thread's explicit cancellation scope was cancelled.
#[must_use]
pub fn is_cancelled() -> bool {
    FLAG.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::SeqCst))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_scopes_are_thread_local_and_restore_after_unwind() {
        let cancelled = Arc::new(AtomicBool::new(true));
        assert!(!is_cancelled());
        with_flag(cancelled, || {
            assert!(is_cancelled());
            assert!(!std::thread::spawn(is_cancelled).join().expect("thread"));
            let result = std::panic::catch_unwind(|| {
                with_flag(Arc::new(AtomicBool::new(false)), || {
                    assert!(!is_cancelled());
                    panic!("test unwind");
                });
            });
            assert!(result.is_err());
            assert!(is_cancelled());
        });
        assert!(!is_cancelled());
    }
}
