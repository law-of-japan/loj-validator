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
    #[must_use]
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
    #[must_use]
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
                Check::DISPLAY_CONSISTENCY,
            ],
            Input::Change { .. } => vec![Check::CHANGE],
            Input::Reference { .. } => vec![Check::REFERENCE],
        }
    }
}

/// Run the selected checks. Checks that do not apply to the input are
/// reported as `notApplicable`.
#[must_use]
pub fn run(input: &Input, checks: &[Check]) -> Report {
    run_with_space(input, checks, None)
}

/// Run checks with the explicit registration-space identifier (VAL-03).
#[must_use]
pub fn run_with_space(input: &Input, checks: &[Check], space: Option<&str>) -> Report {
    let mut report = match input {
        Input::Package(p) => crate::v1alpha2::check(p, checks).report,
        Input::Repository(p) => crate::repository::check(p, checks),
        Input::Registry(p) => crate::registry::check_with_space(p, space),
        Input::Records {
            registry,
            repositories,
        } => crate::xref::check_with_space(registry, repositories, checks, space),
        Input::Change { bundle, base, head } => crate::change::check(bundle, base, head),
        Input::Reference {
            bundle,
            registry,
            reference,
        } => crate::fixref::check_with_space(bundle, registry, reference, space),
    };
    // A cancelled check holds partial results only (see `cancellation`).
    crate::cancellation::finish(&mut report, checks);
    for c in checks {
        if report.result(*c).is_none() {
            report.set(*c, crate::common::ResultValue::NotApplicable);
        }
    }
    report.select(checks);
    report
}
