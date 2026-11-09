//! Entry point for the next specification: one input, a set of checks, one report.
use crate::common::{Check, Report};
use std::path::{Path, PathBuf};

/// Inputs of docs/implementation-guide.md「検査と入力」 (conformance input kinds).
#[derive(Clone, Debug)]
pub enum Input {
    /// One version package (`law.yaml` or its directory).
    Package(PathBuf),
    /// One law repository (`history.yaml`, `versions/`, `rebuild.yaml`).
    Repository(PathBuf),
    /// The registry repository (`records/`).
    Registry(PathBuf),
    /// The registry and several law repositories (L4).
    Records {
        registry: PathBuf,
        repositories: Vec<PathBuf>,
    },
    /// Two commits of a law repository in a `git bundle` (CHG-02, CHG-03).
    Change {
        bundle: PathBuf,
        base: String,
        head: String,
    },
    /// A fixed or Revision reference resolved against a bundle and the registry.
    Reference {
        bundle: PathBuf,
        registry: PathBuf,
        reference: String,
    },
}

impl Input {
    /// Guess the kind of a path: `law.yaml`, a repository, or a registry.
    pub fn detect(path: &Path) -> Input {
        if path.is_dir() {
            if path.join("history.yaml").exists() {
                return Input::Repository(path.into());
            }
            if path.join("records").is_dir() {
                return Input::Registry(path.into());
            }
        }
        Input::Package(path.into())
    }
    /// Checks run when none are selected.
    pub fn default_checks(&self) -> Vec<Check> {
        match self {
            Input::Package(_) => vec![Check::L0, Check::L1],
            Input::Repository(_) => vec![
                Check::L0,
                Check::L1,
                Check::HISTORY_STRUCTURE,
                Check::EVIDENCE,
                Check::REBUILD,
            ],
            Input::Registry(_) => vec![Check::REGISTRY],
            Input::Records { .. } => vec![
                Check::RELATION_RESOLUTION,
                Check::RELATION_CONSISTENCY,
                Check::TIME_CONSISTENCY,
            ],
            Input::Change { .. } => vec![Check::CHANGE],
            Input::Reference { .. } => vec![Check::REFERENCE],
        }
    }
}

/// Run the selected checks. Checks that do not apply to the input are
/// reported as `notApplicable`; unimplemented ones as `incomplete`.
pub fn run(input: &Input, checks: &[Check]) -> Report {
    let mut report = match input {
        Input::Package(p) => crate::v1alpha2::check(p, checks).report,
        Input::Repository(p) => crate::repository::check(p, checks),
        Input::Registry(p) => crate::registry::check(p),
        Input::Records {
            registry,
            repositories,
        } => crate::xref::check(registry, repositories, checks),
        _ => {
            let mut r = Report::default();
            for c in checks {
                r.set(*c, crate::common::ResultValue::Incomplete);
            }
            r
        }
    };
    for c in checks {
        if report.result(*c).is_none() {
            report.set(*c, crate::common::ResultValue::NotApplicable);
        }
    }
    report.select(checks);
    report
}
