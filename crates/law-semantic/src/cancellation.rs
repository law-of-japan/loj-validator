//! Opt-in cooperative cancellation; no process-wide signal handlers are installed.
//!
//! Git polls the flag before spawning and while waiting. Other work polls it at
//! checkpoints in loops whose length grows with the input: per file, Revision,
//! Record, repository, commit and reference, and per Markdown token and tree
//! node. A single YAML or CommonMark parse, a single hash and a blocked
//! filesystem call are not interrupted; they finish before the next checkpoint.
//!
//! A check that observes cancellation stops early and holds only partial
//! results. The public entry points of the next specification
//! ([`crate::checks::run`], [`crate::checks::run_with_space`],
//! [`crate::v1alpha2::check`] and the checks of [`crate::fixref`]) therefore
//! check the flag once more before returning: when it is set, every selected
//! result is `incomplete` and partial diagnostics, references and ASTs are
//! discarded, so cancellation never yields `pass` or `fail` (VAL-04).
use crate::common::{Check, Class, Diagnostic, Report, ResultValue};
use std::cell::RefCell;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

thread_local! {
    static FLAG: RefCell<Option<Arc<AtomicBool>>> = const { RefCell::new(None) };
}

/// Message of the diagnostic left in a cancelled report.
pub const CANCELLED: &str = "validation cancelled before completion";

/// Run with a cancellation flag on this thread. Nested scopes and unwinding
/// restore the previous flag. Spawned threads do not inherit this scope.
/// Work may finish before cancellation is observed (see the module
/// documentation for the checkpoints). Set the flag to true and never reset it.
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

/// A checkpoint in a loop over input: true when the loop should stop. Outside
/// a cancellation scope it is always false, so results are unchanged.
pub(crate) fn checkpoint() -> bool {
    #[cfg(test)]
    test_hook::passed();
    is_cancelled()
}

/// When cancelled, make `report` a cancelled report: every result of `checks`,
/// of the report and of its Record reports becomes `incomplete`, and partial
/// findings are replaced by one undetermined VAL-04 diagnostic. Returns whether
/// the report was cancelled.
pub(crate) fn finish(report: &mut Report, checks: &[Check]) -> bool {
    if !is_cancelled() {
        return false;
    }
    cancel(report, checks);
    true
}

fn cancel(report: &mut Report, checks: &[Check]) {
    for (_, result) in &mut report.results {
        *result = ResultValue::Incomplete;
    }
    for check in checks {
        report.set(*check, ResultValue::Incomplete);
    }
    report.diagnostics = vec![
        Diagnostic::error("VAL-04", crate::common::COMMON, CANCELLED)
            .with_class(Class::Undetermined)
            .with_code("common/v1alpha1/cancelled"),
    ];
    report.references.clear();
    report.resolution = None;
    for (_, record) in &mut report.records {
        cancel(record, &[]);
    }
}

/// Test support: cancel the current scope at the n-th checkpoint and count
/// the checkpoints passed, so tests can show that work stops deterministically.
#[cfg(test)]
pub(crate) mod test_hook {
    use std::cell::Cell;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    thread_local! {
        static PASSED: Cell<u64> = const { Cell::new(0) };
        static TRIP: Cell<Option<u64>> = const { Cell::new(None) };
    }

    pub(super) fn passed() {
        let n = PASSED.with(|p| {
            p.set(p.get() + 1);
            p.get()
        });
        if TRIP.with(Cell::get) == Some(n) {
            super::FLAG.with(|slot| {
                if let Some(flag) = slot.borrow().as_ref() {
                    flag.store(true, Ordering::SeqCst);
                }
            });
        }
    }

    /// Run `f` in a scope that is cancelled at checkpoint `at` (`None`: never).
    /// Returns the result and the number of checkpoints passed.
    pub(crate) fn run<T>(at: Option<u64>, f: impl FnOnce() -> T) -> (T, u64) {
        PASSED.with(|p| p.set(0));
        TRIP.with(|t| t.set(at));
        let result = super::with_flag(Arc::new(AtomicBool::new(false)), f);
        TRIP.with(|t| t.set(None));
        (result, PASSED.with(Cell::get))
    }
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

    #[test]
    fn checkpoints_outside_a_scope_never_cancel() {
        assert!(!checkpoint());
        let mut report = Report::default();
        report.set(Check::L0, ResultValue::Pass);
        assert!(!finish(&mut report, &[Check::L0]));
        assert_eq!(report.result(Check::L0), Some(ResultValue::Pass));
    }

    /// Synthetic input generators: numbered articles with one paragraph each.
    mod synthetic {
        use serde_json::json;
        use std::path::{Path, PathBuf};

        pub(super) struct Dir(pub(super) PathBuf);
        impl Dir {
            pub(super) fn new(name: &str) -> Self {
                let path = std::env::temp_dir()
                    .join(format!("loj-cancellation-{name}-{}", std::process::id()));
                let _ = std::fs::remove_dir_all(&path);
                std::fs::create_dir_all(&path).expect("scratch directory");
                Self(path)
            }
        }
        impl Drop for Dir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        fn kanji(mut n: usize) -> String {
            const DIGITS: [&str; 10] = ["", "一", "二", "三", "四", "五", "六", "七", "八", "九"];
            let mut out = String::new();
            for (unit, name) in [(1000, "千"), (100, "百"), (10, "十")] {
                let d = n / unit;
                if d > 0 {
                    out.push_str(if d > 1 { DIGITS[d] } else { "" });
                    out.push_str(name);
                }
                n %= unit;
            }
            out + DIGITS[n]
        }

        pub(super) fn package(dir: &Path, articles: usize) {
            std::fs::create_dir_all(dir).expect("package directory");
            let manifest = json!({"schemaVersion":"loj/v1alpha2","law":{"id":"synthetic","title":"Synthetic law","type":"act","number":null,"promulgatedAt":null,"language":"ja"},"documents":[{"path":"main.md","parent":"main"}]});
            std::fs::write(dir.join("law.yaml"), manifest.to_string()).expect("law.yaml");
            let text: String = (1..=articles)
                .map(|i| {
                    format!(
                        "# 第{}条
<!-- loj:paragraph number=1 -->
本文。
<!-- loj:end-paragraph -->

",
                        kanji(i)
                    )
                })
                .collect();
            std::fs::write(dir.join("main.md"), text).expect("main.md");
        }

        /// Revision IDs are eight characters of the Revision ID alphabet.
        pub(super) fn repository(dir: &Path, revisions: usize, articles: usize) {
            let ids: Vec<String> = (0..revisions).map(|i| format!("r{i:07}")).collect();
            let history = json!({"schemaVersion":"loj-history/v1alpha1","record":{"id":"synthetic"},
                "revisions": ids.iter().map(|id| json!({"id":id,"text":{"status":"complete"}})).collect::<Vec<_>>()});
            std::fs::write(dir.join("history.yaml"), history.to_string()).expect("history.yaml");
            for id in &ids {
                package(&dir.join("versions").join(id), articles);
            }
        }

        pub(super) fn registry(dir: &Path, records: usize) {
            const ALPHABET: &[u8] = b"0123456789abcdefghjkmnpqrstvwxyz";
            std::fs::create_dir_all(dir.join("records")).expect("records directory");
            for i in 0..records {
                let tail: String = (0..4)
                    .map(|k| ALPHABET[(i >> (5 * k)) & 31] as char)
                    .collect();
                let id = format!("a7k2m9{tail}");
                let record = json!({"schemaVersion":"loj-registry/v1alpha1","id":id,"label":"Synthetic record",
                    "status":"active","identity":{"evidence":[{"source":"s","locator":"1"}],"verification":{"state":"confirmed"}},
                    "collection":{"state":"notCollected"},"sources":[{"id":"s","kind":"other","title":"Synthetic source"}],"numbers":[],"externalIds":[]});
                std::fs::write(
                    dir.join("records").join(format!("{id}.yaml")),
                    record.to_string(),
                )
                .expect("record");
            }
        }
    }

    fn only_cancellation(report: &Report) {
        assert!(
            report
                .results
                .iter()
                .all(|(_, r)| *r == ResultValue::Incomplete),
            "{:?}",
            report.results
        );
        assert_eq!(report.exit_code(), 3);
        assert_eq!(report.diagnostics.len(), 1, "{:?}", report.diagnostics);
        assert_eq!(report.diagnostics[0].message, CANCELLED);
    }

    /// Cancel at several checkpoints of a run and show that at most `slack`
    /// further checkpoints pass before the check returns. `run` must pass
    /// when it is not cancelled.
    fn stops_promptly(slack: u64, run: impl Fn() -> Report) {
        let (report, total) = test_hook::run(None, &run);
        assert!(
            report.results.iter().all(|(_, r)| *r == ResultValue::Pass),
            "{:?} {:?}",
            report.results,
            report.diagnostics
        );
        assert!(total > 100, "too few checkpoints for the test: {total}");
        for at in [1, total / 3, total / 2, total - 1] {
            let (report, passed) = test_hook::run(Some(at), &run);
            only_cancellation(&report);
            assert!(
                passed <= at + slack,
                "cancelled at {at} of {total}, but {passed} checkpoints passed"
            );
        }
    }

    #[test]
    fn large_package_check_stops_at_markdown_checkpoints() {
        let dir = synthetic::Dir::new("package");
        synthetic::package(&dir.0, 1500);
        let levels = [Check::L0, Check::L1];
        stops_promptly(2, || {
            let checked = crate::v1alpha2::check(&dir.0, &levels);
            if is_cancelled() {
                assert!(checked.ast.is_none() && checked.positions.is_empty());
                assert!(checked.source_map.is_empty() && checked.sources.is_empty());
            }
            checked.report
        });
    }

    #[test]
    fn large_repository_check_stops_between_and_within_revisions() {
        let dir = synthetic::Dir::new("repository");
        synthetic::repository(&dir.0, 30, 20);
        let input = crate::checks::Input::Repository(dir.0.clone());
        stops_promptly(3, || crate::checks::run(&input, &[Check::L0, Check::L1]));
    }

    #[test]
    fn large_registry_check_stops_between_records() {
        let dir = synthetic::Dir::new("registry");
        synthetic::registry(&dir.0, 300);
        let input = crate::checks::Input::Registry(dir.0.clone());
        stops_promptly(1, || {
            crate::checks::run_with_space(&input, &[Check::REGISTRY], Some("alpha"))
        });
    }

    #[test]
    fn finish_discards_partial_findings_of_every_record() {
        let mut record = Report::default();
        record.set(Check::EVIDENCE, ResultValue::Fail);
        record
            .diagnostics
            .push(Diagnostic::error("HIST-02", "x", "partial"));
        let mut report = Report::default();
        report.set(Check::L1, ResultValue::Fail);
        report
            .diagnostics
            .push(Diagnostic::error("BASIC-01", "x", "partial"));
        report.references.push(serde_json::json!({}));
        report.resolution = Some(serde_json::json!({"result": "resolved"}));
        report.records.push(("r".into(), record));
        let cancelled = with_flag(Arc::new(AtomicBool::new(true)), || {
            finish(&mut report, &[Check::L1, Check::REBUILD])
        });
        assert!(cancelled);
        assert_eq!(report.result(Check::L1), Some(ResultValue::Incomplete));
        assert_eq!(report.result(Check::REBUILD), Some(ResultValue::Incomplete));
        assert_eq!(report.exit_code(), 3);
        assert!(report.references.is_empty() && report.resolution.is_none());
        for r in [&report, &report.records[0].1] {
            assert!(r.results.iter().all(|(_, v)| *v == ResultValue::Incomplete));
            assert_eq!(r.diagnostics.len(), 1);
            assert_eq!(r.diagnostics[0].class, Class::Undetermined);
            assert_eq!(r.diagnostics[0].rule, "VAL-04");
        }
    }
}
