//! `loj-validator check` for the next specification (loj/v1alpha2 and the
//! loj-history, loj-glyphs, loj-registry, loj-rebuild kinds). The JSON output
//! is a report of `schemas/common/v1alpha1/report.schema.json`.
use law_semantic::checks::{run, Input};
use law_semantic::common::{Check, Report};
use std::path::PathBuf;

pub const USAGE: &str = "next specification (loj/v1alpha2 and history, glyphs, registry, rebuild):
  loj-validator check PATH [--level L0|L1|L3|L4]... [--category NAME]... [--check NAME]...
                     [--format json|text] [--ast FILE]
  loj-validator check --registry DIR --repository DIR... [--level L4] [--category NAME]...
  loj-validator check BUNDLE --base COMMIT --head COMMIT        (change check, CHG-02/03)
  loj-validator check BUNDLE --registry DIR --reference REF     (reference resolution)

  PATH is law.yaml or its directory (package), a law repository (history.yaml),
  or a registry (records/). Categories: historyStructure, evidence (L3);
  relationResolution, relationConsistency, timeConsistency (L4).
  Checks: L0, L1, L3[/category], L4[/category], registry, rebuild, change, reference.
  L2, L3/sourceMatch and L4/clauseReference are reserved and cannot be selected.
  Without a selection, every check that applies to the input runs.";

pub struct Args {
    pub input: Input,
    pub checks: Vec<Check>,
    pub json: bool,
    pub ast: Option<PathBuf>,
}

pub fn parse(args: &[String]) -> Result<Args, String> {
    let mut path = None;
    let mut levels = Vec::new();
    let mut categories: Vec<String> = Vec::new();
    let mut named = Vec::new();
    let mut registry = None;
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
            "--repository" => repositories.push(PathBuf::from(value)),
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
    let input = match (path, registry, base, head, reference) {
        (Some(bundle), None, Some(base), Some(head), None) if repositories.is_empty() => {
            Input::Change { bundle, base, head }
        }
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
    })
}

/// Run and render. Returns the output text and the exit code.
pub fn execute(args: &Args) -> (String, i32) {
    let report: Report = if let (Some(out), Input::Package(p)) = (&args.ast, &args.input) {
        let r = law_semantic::v1alpha2::check(p, &args.checks);
        if let Some(ast) = &r.ast {
            if let Err(e) = std::fs::write(out, serde_json::to_vec_pretty(ast).unwrap()) {
                return (format!("error: cannot write AST: {e}"), 2);
            }
        }
        let mut report = r.report;
        report.select(&args.checks);
        report
    } else {
        run(&args.input, &args.checks)
    };
    let code = report.exit_code();
    if args.json {
        return (
            serde_json::to_string_pretty(&report.to_json()).unwrap(),
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
                _ => format!(" {}", l["path"].as_str().unwrap_or("")),
            }
        });
        s.push_str(&format!(
            "{} {} ({}){at}: {}\n",
            d.class, d.rule, d.schema_version, d.message
        ));
    }
    (s, code)
}
