//! Resolution of fixed and Revision references (common/v1alpha1 FIXREF-02..07),
//! reported apart from the conformance levels.
use crate::common::findings::Findings;
use crate::common::{Check, Class, Report, COMMON};
use crate::git::{Bundle, ExportError};
use crate::registry::{self, Resolution};
use serde_json::{json, Map, Value};
use std::path::Path;

const RF: Check = Check::REFERENCE;

/// FIXREF-02 string form.
#[derive(Debug, PartialEq, Eq)]
pub struct Parsed {
    pub space: String,
    pub record: String,
    /// Empty for a Record reference.
    pub revision: String,
    pub commit: Option<String>,
    pub node: Option<String>,
}

/// Split a fixed or Revision reference
/// (`loj:<space>/<record>/<revision>[@<commit>][#<node>]`, FIXREF-02) into its
/// parts, without resolving it.
///
/// # Errors
///
/// The text does not have that form, or a part of it is empty. The message
/// is the text of the finding.
pub fn parse(s: &str) -> Result<Parsed, String> {
    let rest = s
        .strip_prefix("loj:")
        .ok_or("a reference starts with loj:")?;
    let (rest, node) = match rest.split_once('#') {
        Some((a, n)) => (a, Some(n.to_owned())),
        None => (rest, None),
    };
    let (path, commit) = match rest.split_once('@') {
        Some((a, c)) => (a, Some(c.to_owned())),
        None => (rest, None),
    };
    let parts: Vec<&str> = path.split('/').collect();
    let (space, record, revision) = match parts.as_slice() {
        [space, record] if commit.is_none() && node.is_none() => (*space, *record, ""),
        [space, record, revision] if !revision.is_empty() => (*space, *record, *revision),
        _ => return Err("expected loj:<space>/<record>[/<revision>[@<commit>][#<node>]]".into()),
    };
    if [space, record]
        .iter()
        .any(|p| p.is_empty() || p.contains(['@', '#']))
        || node
            .as_deref()
            .is_some_and(|n| n.is_empty() || n.contains(['@', '#']))
        || commit.as_deref().is_some_and(|c| c.contains(['@', '#']))
    {
        return Err("empty or invalid component".into());
    }
    Ok(Parsed {
        space: space.into(),
        record: record.into(),
        revision: revision.into(),
        commit,
        node,
    })
}

/// Convert a structured RecordRef, RevisionRef or FixedRef to FIXREF-02.
/// Auxiliary label, path and repository fields are deliberately not serialized.
/// An omitted registration space requires explicit caller context.
///
/// # Errors
/// Returns a diagnostic message for an invalid shape or missing space context.
pub fn to_string_reference(value: &Value, space: Option<&str>) -> Result<String, String> {
    string_reference(value, space).map_err(|error| error.message())
}

#[derive(Debug)]
enum ConversionError {
    MissingSpace,
    Invalid(String),
}
impl ConversionError {
    fn message(self) -> String {
        match self {
            Self::MissingSpace => "registration space was not supplied".into(),
            Self::Invalid(message) => message,
        }
    }
}
impl From<&str> for ConversionError {
    fn from(message: &str) -> Self {
        Self::Invalid(message.into())
    }
}

fn string_reference(value: &Value, space: Option<&str>) -> Result<String, ConversionError> {
    let kind = if value.get("record").is_none() {
        "RecordRef"
    } else if value.get("commit").is_some() {
        "FixedRef"
    } else {
        "RevisionRef"
    };
    let schema = json!({"$ref": format!("https://law-of-japan.github.io/loj-spec/schemas/common/v1alpha1/types.schema.json#/$defs/{kind}")});
    let validator =
        crate::common::validator(&schema.to_string(), &[crate::repository::history::TYPES]);
    if let Some(error) = validator.iter_errors(value).next() {
        return Err(ConversionError::Invalid(error.to_string()));
    }
    let record = if kind == "RecordRef" {
        value
    } else {
        &value["record"]
    };
    let space = record["space"]
        .as_str()
        .or(space)
        .filter(|s| !s.is_empty())
        .ok_or(ConversionError::MissingSpace)?;
    if !crate::common::valid_registration_space(space) {
        return Err("invalid registration space".into());
    }
    let mut text = format!(
        "loj:{space}/{}",
        record["id"].as_str().ok_or("missing Record ID")?
    );
    if kind != "RecordRef" {
        text.push('/');
        text.push_str(value["revision"].as_str().ok_or("missing Revision ID")?);
    }
    if let Some(commit) = value["commit"].as_str() {
        text.push('@');
        text.push_str(commit);
    }
    if let Some(node) = value["node"].as_str() {
        text.push('#');
        text.push_str(node);
    }
    Ok(text)
}

/// Recover the identity fields of a FIXREF-02 reference.
///
/// # Errors
/// Returns an error when the reference has invalid syntax.
pub fn to_structured(reference: &str) -> Result<Value, String> {
    let p = parse(reference)?;
    let record = json!({"space": p.space, "id": p.record});
    if p.revision.is_empty() {
        to_string_reference(&record, None)?;
        return Ok(record);
    }
    let mut value = json!({"record": record, "revision": p.revision});
    if let Some(commit) = p.commit {
        value["commit"] = json!(commit);
    }
    if let Some(node) = p.node {
        value["node"] = json!(node);
    }
    // The schema also checks IDs and prevents nodes on Revision references.
    to_string_reference(&value, None)?;
    Ok(value)
}

/// Resolve a structured reference. `path` is checked against HIST-08 and is
/// never used as an alternative read location.
#[must_use]
pub fn check_structured(
    bundle: &Path,
    registry: &Path,
    value: &Value,
    space: Option<&str>,
) -> Report {
    let text = match string_reference(value, space) {
        Ok(text) => text,
        Err(error) => {
            let mut f = Findings::default();
            f.applicable.insert(RF);
            let class = if matches!(error, ConversionError::MissingSpace) {
                Class::Undetermined
            } else {
                Class::Error
            };
            f.note(RF, class, "FIXREF-01", COMMON, error.message(), "", "");
            let mut report = f.into_report(&[RF]);
            crate::cancellation::finish(&mut report, &[RF]);
            return report;
        }
    };
    let mut report = check_with_space(bundle, registry, &text, space);
    if report
        .resolution
        .as_ref()
        .is_some_and(|r| r["result"] == "resolved")
    {
        if let Some(path) = value["path"].as_str() {
            let expected = format!(
                "versions/{}/law.yaml",
                value["revision"].as_str().unwrap_or("")
            );
            if path != expected {
                report.diagnostics.push(crate::common::Diagnostic::error(
                    "FIXREF-03",
                    COMMON,
                    "path differs from the revision package",
                ));
                report.set(RF, crate::common::ResultValue::Fail);
                report.resolution = Some(json!({"result": "error"}));
            }
        }
    }
    crate::cancellation::finish(&mut report, &[RF]);
    report
}

pub(crate) fn full_hash(c: &str) -> bool {
    (c.len() == 40 || c.len() == 64)
        && c.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[must_use]
pub fn check(bundle: &Path, registry_dir: &Path, reference: &str) -> Report {
    check_with_space(bundle, registry_dir, reference, None)
}

#[must_use]
pub fn check_with_space(
    bundle: &Path,
    registry_dir: &Path,
    reference: &str,
    space: Option<&str>,
) -> Report {
    let mut f = Findings::default();
    f.applicable.insert(RF);
    let mut resolution = json!({"result": "error"});
    resolve(
        bundle,
        registry_dir,
        reference,
        space,
        &mut f,
        &mut resolution,
    );
    match resolution["result"].as_str() {
        Some("notFetched") => {
            f.incomplete.insert(RF);
        }
        Some("error") if !f.failed(RF) => {
            f.error(
                RF,
                "FIXREF-06",
                COMMON,
                "reference not resolved",
                reference,
                "",
            );
        }
        _ => {}
    }
    let mut r = f.into_report(&[RF]);
    r.target
        .insert("repository".into(), json!(bundle.to_string_lossy()));
    if let Some(commit) = resolution["commit"].as_str() {
        r.target.insert("commit".into(), json!(commit));
    }
    r.resolution = Some(resolution);
    crate::cancellation::finish(&mut r, &[RF]);
    r
}

fn resolve(
    bundle: &Path,
    registry_dir: &Path,
    reference: &str,
    space: Option<&str>,
    f: &mut Findings,
    out: &mut Value,
) {
    let err = |f: &mut Findings, rule: &str, msg: String| {
        f.error(RF, rule, COMMON, msg, reference, "");
    };
    let p = match parse(reference) {
        Ok(p) => p,
        Err(e) => return err(f, "FIXREF-02", e),
    };
    if !crate::common::valid_registration_space(&p.space) {
        f.error(
            RF,
            "RID-04",
            registry::REGISTRY,
            "invalid registration space",
            reference,
            "",
        );
        return;
    }
    if !p.revision.is_empty() && !crate::repository::history::valid_revision_id(&p.revision) {
        return err(f, "FIXREF-03", "reference must use a revision_id".into());
    }
    // FIXREF-07: a node needs a commit.
    if p.node.is_some() && p.commit.is_none() {
        err(f, "FIXREF-07", "a node is given without @<commit>".into());
        return err(
            f,
            "FIXREF-06",
            "a reference without commit cannot address a node".into(),
        );
    }
    // FIXREF-04: full hashes only.
    if let Some(c) = &p.commit {
        if !full_hash(c) {
            err(f, "FIXREF-04", format!("{c} is not a full commit hash"));
            return err(f, "FIXREF-06", "commit cannot be identified".into());
        }
    }
    let mut scratch = Findings::default();
    let Some(reg) = registry::load_with_space(registry_dir, space, &mut scratch) else {
        *out = json!({"result": "notFetched"});
        return;
    };
    if p.space != reg.space {
        return not_fetched(
            f,
            out,
            reference,
            if reg.space.is_empty() {
                "registration space was not supplied".into()
            } else {
                "reference names another registration space".into()
            },
        );
    }
    let record = match reg.resolve(&json!({"id": p.record})) {
        Resolution::Resolved(id) => id,
        _ => {
            *out = json!({"result": "notFetched"});
            return;
        }
    };
    let Some(rec) = reg.records.get(&record) else {
        *out = json!({"result": "notFetched"});
        return;
    };
    if p.revision.is_empty() {
        *out = json!({"result": "resolved", "record": record});
        return;
    }
    let branch = rec["repository"]["canonicalBranch"]
        .as_str()
        .unwrap_or("main")
        .to_owned();
    match Bundle::open(bundle) {
        Ok(b) => resolve_in(&b, &p, &record, &branch, reference, f, out),
        Err(e) => not_fetched(f, out, reference, e.to_string()),
    }
}

/// The repository could not be read (git missing or failing, an unusable tree,
/// no canonical branch or commit): the reference is not fetched, with the
/// reason, and never reported as unresolvable (VAL-04). `check` marks the check
/// incomplete for every notFetched result; the note only adds the reason. The
/// note carries FIXREF-06, the rule that defines the result values, notFetched
/// among them.
fn not_fetched(f: &mut Findings, out: &mut Value, reference: &str, why: String) {
    f.note(
        RF,
        Class::Undetermined,
        "FIXREF-06",
        COMMON,
        why,
        reference,
        "",
    );
    *out = json!({"result": "notFetched"});
}

/// Resolve `p` in an opened bundle holding `record`, whose canonical branch is
/// `branch`.
fn resolve_in(
    b: &Bundle,
    p: &Parsed,
    record: &str,
    branch: &str,
    reference: &str,
    f: &mut Findings,
    out: &mut Value,
) {
    let err = |f: &mut Findings, rule: &str, msg: String| {
        f.error(RF, rule, COMMON, msg, reference, "");
    };
    let tip = match b.branch(branch) {
        Ok(Some(tip)) => tip,
        // The canonical branch is not in the bundle: the bundle does not hold the
        // record's history, so the reference is not fetched, and the note says why.
        Ok(None) => {
            let why = format!("the canonical branch {branch} is not in the bundle");
            return not_fetched(f, out, reference, why);
        }
        Err(e) => return not_fetched(f, out, reference, e.to_string()),
    };
    let commit = match &p.commit {
        None => tip.clone(), // FIXREF-05: latest record version of the canonical branch
        Some(c) => match b.commit(c) {
            Ok(Some(full)) if full == *c => match b.is_ancestor(c, &tip) {
                Ok(true) => full,
                Ok(false) => {
                    return err(
                        f,
                        "FIXREF-04",
                        format!("{c} is not reachable from {branch}"),
                    );
                }
                Err(e) => return not_fetched(f, out, reference, e.to_string()),
            },
            // `resolve` has already refused an abbreviated hash (FIXREF-04), and a
            // full hash resolves to itself, so this is a commit not in the bundle.
            Ok(_) => {
                let why = format!("commit {c} is not in the bundle");
                return not_fetched(f, out, reference, why);
            }
            Err(e) => return not_fetched(f, out, reference, e.to_string()),
        },
    };
    let snap = match crate::change::snapshot(b, &commit) {
        Ok(s) => s,
        // Unlike the change check, a refused tree (`ExportError::Tree`) gets the
        // same result as a git failure: either way the revision cannot be read,
        // so the reference is not fetched. Only the reason says which it was.
        Err(ExportError::Tree(e)) => {
            return not_fetched(f, out, reference, format!("tree refused: {e}"));
        }
        Err(e @ (ExportError::Environment(_) | ExportError::Io(_) | ExportError::Cancelled)) => {
            return not_fetched(f, out, reference, e.to_string())
        }
    };
    let Some(h) = snap.history.as_ref() else {
        return err(f, "FIXREF-06", format!("no history manifest at {commit}"));
    };
    let Some(rev) = h["revisions"]
        .as_array()
        .and_then(|a| a.iter().find(|r| r["id"] == p.revision.as_str()))
    else {
        return err(
            f,
            "FIXREF-06",
            format!("revision {} is not in history.yaml at {commit}", p.revision),
        );
    };
    if rev["status"] != "active" {
        // FIXREF-05: report retraction, merge or split; never forward silently.
        f.note(
            RF,
            Class::Warning,
            "FIXREF-05",
            COMMON,
            format!(
                "revision {} is {}; replacedBy {}",
                p.revision, rev["status"], rev["replacedBy"]["revisions"]
            ),
            reference,
            "",
        );
    }
    let mut o = Map::new();
    o.insert("result".into(), json!("resolved"));
    o.insert("record".into(), json!(record));
    o.insert("revision".into(), json!(p.revision));
    o.insert("commit".into(), json!(commit));
    if let Some(node) = &p.node {
        let path = crate::repository::history::package_path(rev);
        let ast = match snap.asts.get(&p.revision) {
            Some(Ok(a)) => a,
            _ => {
                return err(
                    f,
                    "FIXREF-06",
                    format!("revision {} has no valid package at {commit}", p.revision),
                )
            }
        };
        let Some(text) = own_text(&ast["root"], node) else {
            return err(
                f,
                "FIXREF-07",
                format!("node {node} is not in revision {} at {commit}", p.revision),
            );
        };
        o.insert("path".into(), json!(path));
        o.insert("node".into(), json!(node));
        o.insert("text".into(), json!(text));
    }
    *out = Value::Object(o);
}

/// Own text of a node (children excluded), as in the semantic AST.
fn own_text(n: &Value, id: &str) -> Option<String> {
    if n["id"] == id {
        return Some(
            n["content"]
                .as_array()?
                .iter()
                .filter_map(|c| c["text"].as_str())
                .collect(),
        );
    }
    n["content"]
        .as_array()?
        .iter()
        .filter(|c| c.get("kind").is_some())
        .find_map(|c| own_text(c, id))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn structured_identity_roundtrips_and_requires_space_context() {
        let value = json!({"record":{"id":"synthetic", "label":"display"}, "revision":"b7k2m9qd", "commit":"a".repeat(40), "node":"law/main", "path":"versions/b7k2m9qd/law.yaml", "repository":"auxiliary"});
        assert!(to_string_reference(&value, None).is_err());
        let text = to_string_reference(&value, Some("alpha")).unwrap();
        let back = to_structured(&text).unwrap();
        assert_eq!(back["record"], json!({"id":"synthetic", "space":"alpha"}));
        assert_eq!(back["revision"], value["revision"]);
        assert_eq!(back["commit"], value["commit"]);
        assert_eq!(back["node"], value["node"]);
        assert!(back.get("path").is_none());
        assert_eq!(to_string_reference(&back, None).unwrap(), text);
        assert_eq!(
            to_structured("loj:alpha/synthetic").unwrap(),
            json!({"id":"synthetic", "space":"alpha"})
        );
        for commit in ["A".repeat(40), "a".repeat(39), "main".into()] {
            let mut bad = value.clone();
            bad["commit"] = json!(commit);
            assert!(to_string_reference(&bad, Some("alpha")).is_err());
        }
        assert!(to_string_reference(
            &json!({"record":{"id":"synthetic"},"revision":"b7k2m9qd","node":"law"}),
            Some("alpha")
        )
        .is_err());
    }

    #[test]
    fn string_forms() {
        let p = parse("loj:loj/a/b7k2m9qd@0123#a/main").unwrap();
        assert_eq!(p.commit.as_deref(), Some("0123"));
        assert_eq!(p.node.as_deref(), Some("a/main"));
        assert_eq!(parse("loj:loj/a").unwrap().revision, "");
        assert!(parse("loj:loj/a@0123").is_err());
        assert!(parse("lo:loj/a/b").is_err());
        assert!(!full_hash("02086ca"));
        assert!(full_hash(&"a".repeat(64)));
    }

    /// A git failure after the bundle is open leaves the reference not fetched,
    /// with git's reason, and reports no error about the reference.
    #[test]
    fn git_failure_is_not_fetched_not_an_error() {
        let bundle = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../vendor/loj-spec/conformance/common/v1alpha1/valid/common-004/repo.bundle");
        let reference = "loj:loj/20990101-act-1/b7k2m9qd";
        let p = parse(reference).unwrap();
        let resolve = |b: &Bundle| {
            let mut f = Findings::default();
            let mut out = json!({"result": "error"});
            resolve_in(b, &p, "20990101-act-1", "main", reference, &mut f, &mut out);
            (f, out)
        };
        // The unbroken bundle resolves.
        let b = Bundle::open(&bundle).unwrap();
        let (f, out) = resolve(&b);
        assert_eq!(out["result"], "resolved", "{out} {:?}", f.items);
        // A broken repository: every git command fails.
        b.break_for_test();
        let (f, out) = resolve(&b);
        assert_eq!(out["result"], "notFetched", "{out}");
        assert!(!f.failed(RF), "{:?}", f.items);
        assert!(
            f.items
                .iter()
                .any(|(_, d)| d.class == Class::Undetermined && d.message.starts_with("git failed")),
            "{:?}",
            f.items
        );
    }
}
