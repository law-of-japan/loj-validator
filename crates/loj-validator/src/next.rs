//! `loj-validator check` for the next specification (loj/v1alpha2 and the
//! loj-history, loj-glyphs, loj-registry, loj-rebuild kinds). The JSON output
//! is a report of `schemas/common/v1alpha1/report.schema.json`.
use law_semantic::checks::{run_with_space, Input, RepositoryKind};
use law_semantic::common::{Check, Report};
use std::path::PathBuf;

pub const USAGE: &str = "next specification (loj/v1alpha2 and history, glyphs, registry, rebuild):
  loj-validator check PATH [--level L0|L1|L3|L4]... [--category NAME]... [--check NAME]...
                     [--format json|text] [--ast FILE] [--registration-space SPACE] [--record ID]
  loj-validator check --registry DIR --repository DIR... [--level L4] [--category NAME]...
  loj-validator check BUNDLE [--repository-kind law|registry] [--base COMMIT] --head COMMIT      (change check, CHG-02/03)
  loj-validator check BUNDLE --registry DIR --reference REF     (reference resolution)

  PATH is law.yaml or its directory (package), a law repository (history.yaml),
  or a registry (records/). Categories: historyStructure, evidence (L3);
  relationResolution, relationConsistency, timeConsistency, displayConsistency (L4).
  The default repository kind for change checks is law.
  Without --base, the change check covers every commit reachable from --head,
  from the first commit (the commit without a parent) on.
  --registration-space SPACE supplies the registry space ([a-z][a-z0-9-]*);
  no default is inferred from space.yaml. --record ID selects a repository/records report.
  --ast is package-only; --registration-space requires a registry input.
  Checks: L0, L1, L3[/category], L4[/category], registry, rebuild, change, reference.
  L2, L3/sourceMatch and L4/clauseReference are reserved and cannot be selected.
  Without a selection, every check that applies to the input runs.
  Unix CLI: SIGINT/SIGTERM stop git groups and clean temporary directories, then
  exit 3; JSON reports error.kind=incomplete. Other checks stop at per-file,
  revision, record, commit and Markdown token checkpoints; a single parse,
  hash or blocked filesystem call finishes first.
  Signals after result completion do not change output; repeated signals do not
  force exit. SIGKILL/abort/host failure cannot guarantee cleanup.";

pub struct Args {
    pub input: Input,
    pub checks: Vec<Check>,
    pub json: bool,
    pub ast: Option<PathBuf>,
    pub registration_space: Option<String>,
    pub record: Option<String>,
}

/// Read the arguments of a next-specification check.
///
/// # Errors
///
/// An option is unknown, repeated, lacks its value or contradicts another
/// one, or no input is given. The message is shown with the usage (exit
/// code 2).
pub fn parse(args: &[String]) -> Result<Args, String> {
    let mut path = None;
    let mut levels = Vec::new();
    let mut categories: Vec<String> = Vec::new();
    let mut named = Vec::new();
    let mut registry = None;
    let mut repository_kind = None;
    let mut registration_space = None;
    let mut record = None;
    let mut repositories = Vec::new();
    let (mut base, mut head, mut reference) = (None, None, None);
    let mut json = false;
    let mut ast = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let (name, inline) = match arg.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n, Some(v.to_string())),
            _ => (arg.as_str(), None),
        };
        if !name.starts_with("--") {
            if path.replace(PathBuf::from(arg)).is_some() {
                return Err(format!("unexpected argument: {arg}"));
            }
            continue;
        }
        let value = match inline {
            Some(v) => v,
            None => rest
                .next()
                .cloned()
                .ok_or_else(|| format!("{name} requires a value"))?,
        };
        let once = |slot: &mut Option<String>, v: String| {
            if slot.replace(v).is_some() {
                Err(format!("{name} given more than once"))
            } else {
                Ok(())
            }
        };
        match name {
            "--level" => levels.extend(value.split(',').map(str::to_owned)),
            "--category" => categories.extend(value.split(',').map(str::to_owned)),
            "--check" => named.extend(value.split(',').map(str::to_owned)),
            "--registry" => once(&mut registry, value)?,
            "--record" => once(&mut record, value)?,
            "--registration-space" => once(&mut registration_space, value)?,
            "--repository" => repositories.push(PathBuf::from(value)),
            "--repository-kind" => once(&mut repository_kind, value)?,
            "--base" => once(&mut base, value)?,
            "--head" => once(&mut head, value)?,
            "--reference" => once(&mut reference, value)?,
            "--format" => match value.as_str() {
                "json" => json = true,
                "text" => json = false,
                _ => return Err(format!("unsupported format: {value}")),
            },
            "--ast" => {
                if ast.replace(PathBuf::from(value)).is_some() {
                    return Err("--ast given more than once".into());
                }
            }
            _ => return Err(format!("unknown option: {name}")),
        }
    }
    if registration_space
        .as_deref()
        .is_some_and(|s| !law_semantic::common::valid_registration_space(s))
    {
        return Err("--registration-space must match [a-z][a-z0-9-]*".into());
    }
    if repository_kind.is_some() && head.is_none() {
        return Err("--repository-kind requires a change check".into());
    }
    let input = match (path, registry, base, head, reference) {
        (Some(bundle), None, base, Some(head), None) if repositories.is_empty() => Input::Change {
            bundle,
            base,
            head,
            repository: match repository_kind.as_deref() {
                None | Some("law") => RepositoryKind::Law,
                Some("registry") => RepositoryKind::Registry,
                _ => return Err("--repository-kind must be law or registry".into()),
            },
        },
        (Some(bundle), Some(registry), None, None, Some(reference)) if repositories.is_empty() => {
            Input::Reference {
                bundle: bundle.clone(),
                registry: registry.into(),
                reference,
            }
        }
        (None, Some(registry), None, None, None) if !repositories.is_empty() => Input::Records {
            registry: registry.into(),
            repositories,
        },
        (Some(p), None, None, None, None) if repositories.is_empty() => Input::detect(&p),
        (None, None, None, None, None) => return Err("missing input path".into()),
        _ => return Err("inconsistent input options (see usage)".into()),
    };
    if registration_space.is_some()
        && !matches!(
            &input,
            Input::Registry(_) | Input::Records { .. } | Input::Reference { .. }
        )
    {
        return Err("--registration-space requires a registry input".into());
    }
    if record.is_some() && !matches!(&input, Input::Repository(_) | Input::Records { .. }) {
        return Err("--record requires a repository or records input".into());
    }
    if ast.is_some() && !matches!(&input, Input::Package(_)) {
        return Err("--ast requires a package input".into());
    }
    let mut checks = Vec::new();
    for l in &levels {
        if !matches!(l.as_str(), "L0" | "L1" | "L3" | "L4") {
            return Err(match l.as_str() {
                "L2" => "L2 is reserved and not implemented".into(),
                _ => format!("unknown level: {l}"),
            });
        }
    }
    for n in levels.iter().chain(&named) {
        for c in Check::parse(n)? {
            if !checks.contains(&c) {
                checks.push(c);
            }
        }
    }
    if !categories.is_empty() {
        let mut picked = Vec::new();
        for k in &categories {
            let found: Vec<Check> = ["L3", "L4"]
                .iter()
                .filter_map(|l| Check::parse(&format!("{l}/{k}")).ok())
                .flatten()
                .collect();
            if found.is_empty() {
                return Err(Check::parse(&format!("L3/{k}"))
                    .and_then(|_| Check::parse(&format!("L4/{k}")))
                    .err()
                    .unwrap_or_else(|| format!("unknown category: {k}")));
            }
            picked.extend(found);
        }
        // A category narrows its level; other selected checks stay.
        checks.retain(|c| c.category.is_none());
        for c in picked {
            if !checks.contains(&c) {
                checks.push(c);
            }
        }
    }
    if checks.is_empty() {
        checks = input.default_checks();
    }
    Ok(Args {
        input,
        checks,
        json,
        ast,
        registration_space,
        record,
    })
}

/// Run and render. Returns the output text and the exit code.
#[must_use]
pub fn execute(args: &Args) -> (String, i32) {
    let report: Report = if let (Some(out), Input::Package(p)) = (&args.ast, &args.input) {
        let r = law_semantic::checks::run_package(p, &args.checks);
        if let Some(ast) = &r.ast {
            if let Err(e) = std::fs::write(
                out,
                serde_json::to_vec_pretty(ast).expect("a JSON value serializes"),
            ) {
                return (
                    if args.json {
                        serde_json::json!({"error":{"kind":"output","message":format!("cannot write AST: {e}")}}).to_string()
                    } else {
                        format!("error: cannot write AST: {e}")
                    },
                    2,
                );
            }
        }
        r.report
    } else {
        run_with_space(
            &args.input,
            &args.checks,
            args.registration_space.as_deref(),
        )
    };
    let report = if let Some(id) = &args.record {
        match report.records.iter().find(|(record, _)| record == id) {
            Some((_, record)) => record.clone(),
            None => {
                return (
                    if args.json {
                        serde_json::json!({"error":{"kind":"argument","message":format!("Record {id} is not among the checked inputs")}}).to_string()
                    } else {
                        format!("Record {id} is not among the checked inputs")
                    },
                    2,
                )
            }
        }
    } else {
        report
    };
    let code = report.exit_code();
    if args.json {
        return (
            serde_json::to_string_pretty(&report.to_json()).expect("a JSON value serializes"),
            code,
        );
    }
    let mut s = String::new();
    for (c, r) in &report.results {
        s.push_str(&format!("{}: {}\n", c.label(), r.as_str()));
    }
    for d in &report.diagnostics {
        let at = d.location.as_ref().map_or(String::new(), |l| {
            match (l["start"].as_u64(), l["end"].as_u64()) {
                (Some(a), Some(b)) => format!(" {} [{a}..{b}]", l["path"].as_str().unwrap_or("")),
                _ => format!(" {}", l["file"].as_str().unwrap_or("")),
            }
        });
        s.push_str(&format!(
            "{} {} ({}){at}: {}\n",
            d.class, d.rule, d.schema_version, d.message
        ));
    }
    (s, code)
}
