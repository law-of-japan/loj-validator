//! Entry point for the next specification: one input, a set of checks, one report.
use crate::common::{Check, Report};
use std::path::{Path, PathBuf};

/// CHG-01/03 repository layout supplied by the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepositoryKind {
    Law,
    Registry,
}

impl RepositoryKind {
    pub(crate) fn canonical(self, path: &str) -> bool {
        match self {
            Self::Law => {
                matches!(path, "history.yaml" | "rebuild.yaml") || path.starts_with("versions/")
            }
            Self::Registry => path == "space.yaml" || path.starts_with("records/"),
        }
    }
}

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
    /// Commits of a law or registry repository in a `git bundle` (CHG-02, CHG-03): those
    /// after `base` up to `head`, or, without `base`, every commit reachable
    /// from `head`, from the first commit (the commit without a parent) on.
    /// With `legacy_boundary`, it and its ancestors are legacy commits, not
    /// checked, and the canonical commits on it are first commits (CHG-03).
    Change {
        repository: RepositoryKind,
        bundle: PathBuf,
        base: Option<String>,
        head: String,
        legacy_boundary: Option<String>,
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
        Input::Package(p) => return run_package(p, checks).report,
        Input::Repository(p) => crate::repository::check(p, checks),
        Input::Registry(p) => crate::registry::check_with_space(p, space),
        Input::Records {
            registry,
            repositories,
        } => crate::xref::check_with_space(registry, repositories, checks, space),
        Input::Change {
            bundle,
            base,
            head,
            legacy_boundary,
            repository,
        } => crate::change::check_repository(
            bundle,
            base.as_deref(),
            head,
            legacy_boundary.as_deref(),
            *repository,
        ),
        Input::Reference {
            bundle,
            registry,
            reference,
        } => crate::fixref::check_with_space(bundle, registry, reference, space),
    };
    crate::cancellation::finish(&mut report, checks);
    select_results(&mut report, checks);
    report
}

/// Run package checks and retain AST and source data alongside the selected report.
#[must_use]
pub fn run_package(path: &Path, checks: &[Check]) -> crate::v1alpha2::PackageCheck {
    let mut package = crate::v1alpha2::check(path, checks);
    // Package cancellation finalizes the report and drops AST/source data together.
    select_results(&mut package.report, checks);
    package
}

fn select_results(report: &mut Report, checks: &[Check]) {
    for c in checks {
        if report.result(*c).is_none() {
            report.set(*c, crate::common::ResultValue::NotApplicable);
        }
    }
    report.select(checks);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_package_checks_finalize_every_selected_result() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../vendor/loj-spec/conformance/v1alpha2/valid/pkg-110");
        let checks = [Check::L0, Check::L1];
        let (package, _) =
            crate::cancellation::test_hook::run(Some(1), || run_package(&path, &checks));
        assert_eq!(package.report.exit_code(), 3);
        assert!(package.ast.is_none());
        assert!(package.positions.is_empty());
        assert!(package.source_map.is_empty());
        assert!(package.sources.is_empty());
        for check in checks {
            assert_eq!(
                package.report.result(check),
                Some(crate::common::ResultValue::Incomplete)
            );
        }
    }
}
