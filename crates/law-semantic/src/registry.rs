//! Registry checks (loj-registry/v1alpha1: REG, RID), reported apart from
//! the conformance levels (REG-01), and Record resolution for L4 (XREF-01).
use crate::common::findings::{common_type_rule, Findings};
use crate::common::{load_yaml, Check, Loaded, Report, COMMON};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

pub const REGISTRY: &str = "loj-registry/v1alpha1";
pub const SCHEMA: &str =
    include_str!("../../../vendor/loj-spec/schemas/loj-registry/v1alpha1/record.schema.json");
const R: Check = Check::REGISTRY;

fn validator() -> &'static jsonschema::Validator {
    static V: std::sync::OnceLock<jsonschema::Validator> = std::sync::OnceLock::new();
    V.get_or_init(|| crate::common::validator(SCHEMA, &[crate::repository::history::TYPES]))
}

fn classify(schema_path: &str, _instance: &str) -> (Check, &'static str, &'static str) {
    if schema_path.contains("/$defs/spaceId") || schema_path.contains("/properties/space/") {
        return (R, "RID-04", REGISTRY);
    }
    if schema_path.starts_with("/properties/numbers/items/properties/glyphs") {
        return (R, "GLYPH-05", crate::repository::GLYPHS);
    }
    if let Some((_, rule)) = common_type_rule(schema_path) {
        if rule != "TYPE-01" || !schema_path.starts_with("/properties/id") {
            return (R, rule, COMMON);
        }
    }
    if schema_path.starts_with("/properties/numbers") {
        return (R, "REG-03", REGISTRY);
    }
    if schema_path.starts_with("/allOf")
        || schema_path.starts_with("/properties/aliases")
        || schema_path.starts_with("/properties/replacedBy")
        || schema_path.starts_with("/properties/identity")
        || schema_path.starts_with("/properties/collection")
        || schema_path.starts_with("/properties/repository")
        || schema_path.starts_with("/properties/status")
    {
        return (R, "REG-02", REGISTRY);
    }
    if schema_path.starts_with("/properties/externalIds") {
        return (R, "REG-05", REGISTRY);
    }
    if schema_path.starts_with("/properties/id") {
        return (R, "RID-02", REGISTRY);
    }
    (R, "REG-01", REGISTRY)
}

/// A loaded registry.
#[derive(Default)]
pub struct Registry {
    /// Explicit registration space (RID-04); empty when not supplied.
    pub space: String,
    pub records: BTreeMap<String, Value>,
    /// Lowercased alias -> record ID.
    pub aliases: HashMap<String, String>,
}

/// How a Record reference resolved (VAL-06, XREF-01).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolution {
    Resolved(String),
    NotFetched(String),
    NonexistentConfirmed,
}

impl Resolution {
    pub fn as_str(&self) -> &'static str {
        match self {
            Resolution::Resolved(_) => "resolved",
            Resolution::NotFetched(_) => "notFetched",
            Resolution::NonexistentConfirmed => "nonexistentConfirmed",
        }
    }
}

impl Registry {
    /// Resolve a RecordRef: merged Records to their target, split Records
    /// only to candidates (not resolved), aliases to their Record.
    pub fn resolve(&self, r: &Value) -> Resolution {
        if r["space"].is_string() && self.space.is_empty() {
            return Resolution::NotFetched("registration space was not supplied".into());
        }
        if r["space"].as_str().is_some_and(|s| s != self.space) {
            return Resolution::NotFetched("another registration space".into());
        }
        let Some(id) = r["id"].as_str() else {
            return Resolution::NotFetched("no id".into());
        };
        let mut id = id.to_owned();
        for _ in 0..16 {
            let rec = match self.records.get(&id) {
                Some(rec) => rec,
                None => match self
                    .aliases
                    .get(&id.to_ascii_lowercase())
                    .and_then(|target| self.records.get(target))
                {
                    Some(rec) => rec,
                    None => return Resolution::NotFetched(format!("{id} is not in the registry")),
                },
            };
            match rec["status"].as_str() {
                Some("merged") => {
                    let target = &rec["replacedBy"][0]["record"];
                    if target["space"]
                        .as_str()
                        .is_some_and(|space| space != self.space)
                    {
                        return Resolution::NotFetched(
                            "merge target's registration space cannot be resolved".into(),
                        );
                    }
                    match target["id"].as_str() {
                        Some(t) => id = t.to_owned(),
                        None => return Resolution::NotFetched("merged without target".into()),
                    }
                }
                Some("split") => {
                    return Resolution::NotFetched(format!(
                        "{id} is split; candidates: {}",
                        rec["replacedBy"]
                    ))
                }
                Some("withdrawn") => return Resolution::NonexistentConfirmed,
                Some("active" | "provisional") => {
                    return Resolution::Resolved(rec["id"].as_str().unwrap_or(&id).to_owned())
                }
                _ => return Resolution::NotFetched("unknown Record status".into()),
            }
        }
        Resolution::NotFetched("merge chain too long".into())
    }
    pub fn target(&self, r: &Value) -> Option<String> {
        let space = r["space"].as_str().unwrap_or(&self.space);
        (!space.is_empty()).then(|| format!("loj:{space}/{}", r["id"].as_str().unwrap_or("")))
    }
}

/// Load (and, with findings, check) the registry repository.
pub fn load_with_space(dir: &Path, space: Option<&str>, f: &mut Findings) -> Option<Registry> {
    if space.is_some_and(|s| !crate::common::valid_registration_space(s)) {
        f.incomplete.insert(R);
        f.note(
            R,
            crate::common::Class::Undetermined,
            "RID-04",
            REGISTRY,
            "invalid registration-space input",
            "",
            "",
        );
        return None;
    }
    let mut reg = Registry {
        space: space.unwrap_or("").into(),
        ..Registry::default()
    };
    let entries = match std::fs::read_dir(dir.join("records")) {
        Ok(e) => e,
        Err(e) => {
            f.unprocessable(
                R,
                crate::common::Diagnostic::error(
                    "REG-01",
                    REGISTRY,
                    format!("records/ unreadable: {e}"),
                )
                .at_pointer("records", ""),
            );
            f.incomplete.insert(R);
            return None;
        }
    };
    f.applicable.insert(R);
    let mut files = match entries
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()
    {
        Ok(files) => files,
        Err(error) => {
            f.unprocessable(
                R,
                crate::common::Diagnostic::error(
                    "REG-01",
                    REGISTRY,
                    format!("records/ entry unreadable: {error}"),
                )
                .at_pointer("records", ""),
            );
            return None;
        }
    };
    files.sort();
    // RID-01/02: IDs and aliases never collide, ignoring case.
    let mut issued: HashMap<String, (String, bool)> = HashMap::new();
    let mut claim =
        |f: &mut Findings, id: &str, owner: &str, retired: bool, path: &str, ptr: &str| {
            if let Some((prev, was_retired)) =
                issued.insert(id.to_ascii_lowercase(), (owner.to_owned(), retired))
            {
                f.error(
                    R,
                    if retired || was_retired {
                        "RID-05"
                    } else {
                        "RID-01"
                    },
                    REGISTRY,
                    format!("{id} collides (ignoring case) with an ID or alias of {prev}"),
                    path,
                    ptr,
                );
            }
        };
    for path in files {
        if crate::cancellation::checkpoint() {
            break;
        }
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let rel = format!("records/{name}");
        let Some(stem) = name.strip_suffix(".yaml") else {
            f.error(
                R,
                "REG-01",
                REGISTRY,
                "records/ holds only <Record ID>.yaml files",
                &rel,
                "",
            );
            continue;
        };
        let rec = match load_yaml(&path) {
            Loaded::Ok(v, _) => v,
            Loaded::Invalid(e) => {
                f.error(R, "REG-01", REGISTRY, e, &rel, "");
                continue;
            }
            Loaded::Unreadable(e) => {
                f.unprocessable(
                    R,
                    crate::common::Diagnostic::error("REG-01", REGISTRY, e).at_pointer(&rel, ""),
                );
                f.incomplete.insert(R);
                continue;
            }
        };
        if rec["schemaVersion"] != REGISTRY {
            f.push(
                R,
                crate::common::Diagnostic::error("REG-01", REGISTRY, "unsupported schemaVersion")
                    .with_code("loj-registry/v1alpha1/unsupported-version")
                    .at_pointer(&rel, "/schemaVersion"),
            );
            f.incomplete.insert(R);
            continue;
        }
        f.schema(validator(), &rec, &rel, classify);
        if rec["id"] != stem {
            f.error(
                R,
                "REG-01",
                REGISTRY,
                format!("file name {stem} is not the id {}", rec["id"]),
                &rel,
                "/id",
            );
        }
        let id = rec["id"].as_str().unwrap_or(stem).to_owned();
        let retired = matches!(
            rec["status"].as_str(),
            Some("merged" | "split" | "withdrawn")
        );
        claim(f, &id, &id, retired, &rel, "/id");
        for (i, a) in rec["aliases"].as_array().into_iter().flatten().enumerate() {
            if let Some(a) = a["id"].as_str() {
                claim(f, a, &id, retired, &rel, &format!("/aliases/{i}"));
                reg.aliases.insert(a.to_ascii_lowercase(), id.clone());
            }
        }
        check_record(&rec, &rel, f);
        reg.records.insert(id, rec);
    }
    // REG-02: replacedBy targets resolve.
    for (id, rec) in &reg.records {
        for (i, t) in rec["replacedBy"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            let target = &t["record"];
            if target["space"].is_string() && reg.space.is_empty() {
                f.note(
                    R,
                    crate::common::Class::Undetermined,
                    "REG-02",
                    REGISTRY,
                    "registration space is required to resolve explicit-space replacedBy",
                    &format!("records/{id}.yaml"),
                    &format!("/replacedBy/{i}"),
                );
            } else if target["id"].is_string()
                && !matches!(reg.resolve(target), Resolution::Resolved(_))
            {
                f.error(
                    R,
                    "REG-02",
                    REGISTRY,
                    format!("replacedBy {target} does not resolve in the same registry"),
                    &format!("records/{id}.yaml"),
                    &format!("/replacedBy/{i}"),
                );
            }
        }
    }
    Some(reg)
}

fn check_record(rec: &Value, rel: &str, f: &mut Findings) {
    let empty = Vec::new();
    let sources: HashSet<&str> = rec["sources"]
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .filter_map(|s| s["id"].as_str())
        .collect();
    let mut refs = Vec::new();
    crate::repository::history::evidence_refs(rec, String::new(), &mut refs);
    for (p, s) in refs {
        if !sources.contains(s) {
            f.error(
                R,
                "TYPE-03",
                COMMON,
                format!("Evidence source {s} is not a Source of this file"),
                rel,
                &p,
            );
        }
    }
    let numbers = rec["numbers"].as_array().unwrap_or(&empty);
    let ids: HashMap<&str, &Value> = numbers
        .iter()
        .filter_map(|n| n["id"].as_str().map(|i| (i, n)))
        .collect();
    if ids.len() != numbers.iter().filter(|n| n["id"].is_string()).count() {
        f.error(
            R,
            "REG-03",
            REGISTRY,
            "number claim ids are not unique",
            rel,
            "/numbers",
        );
    }
    let adopted = numbers
        .iter()
        .filter(|n| n["decision"] == "adopted")
        .count();
    if adopted > 1 {
        f.error(
            R,
            "REG-03",
            REGISTRY,
            format!("{adopted} number claims are adopted at once"),
            rel,
            "/numbers",
        );
    }
    for (i, n) in numbers.iter().enumerate() {
        let p = format!("/numbers/{i}");
        if let Some(s) = n["supersedes"].as_str() {
            match ids.get(s) {
                None => f.error(
                    R,
                    "REG-04",
                    REGISTRY,
                    format!("supersedes {s} is not a claim of this file"),
                    rel,
                    &format!("{p}/supersedes"),
                ),
                Some(old) if old["decision"] != "retracted" => f.error(
                    R,
                    "REG-04",
                    REGISTRY,
                    format!("superseded claim {s} is not retracted"),
                    rel,
                    &format!("{p}/supersedes"),
                ),
                _ => {}
            }
        }
        // REG-03: Gregorian period as a range of existing dates.
        let g = &n["structured"]["period"]["gregorian"];
        if let (Some(a), Some(b)) = (g["from"].as_str(), g["to"].as_str()) {
            let (a, b) = (
                crate::common::time::Date::parse(a),
                crate::common::time::Date::parse(b),
            );
            if a.is_none() || b.is_none() || a > b {
                f.error(
                    R,
                    "REG-03",
                    REGISTRY,
                    "period.gregorian is not a range of existing dates",
                    rel,
                    &format!("{p}/structured/period"),
                );
            }
        }
        // GLYPH-04 in number claims: offset into raw.
        if let Some(raw) = n["raw"].as_str() {
            let raw: Vec<char> = raw.chars().collect();
            for (j, g) in n["glyphs"].as_array().into_iter().flatten().enumerate() {
                let (Some(o), Some(rec_)) = (g["offset"].as_u64(), g["recorded"].as_str()) else {
                    continue;
                };
                let r: Vec<char> = rec_.chars().collect();
                if crate::repository::glyphs::recorded_at(&raw, o, &r).is_none() {
                    f.error(
                        R,
                        "GLYPH-04",
                        crate::repository::GLYPHS,
                        format!("recorded text is not at offset {o} of raw"),
                        rel,
                        &format!("{p}/glyphs/{j}"),
                    );
                }
            }
        }
    }
}

/// The registry check of `dir`.
pub fn check_with_space(dir: &Path, space: Option<&str>) -> Report {
    let mut f = Findings::default();
    load_with_space(dir, space, &mut f);
    let mut r = f.into_report(&[R]);
    r.target
        .insert("repository".into(), json!(dir.to_string_lossy()));
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn glyph_findings(offset: u64) -> Vec<(String, String)> {
        let rec = json!({"numbers": [{
            "id": "n1",
            "decision": "adopted",
            "raw": "第一号",
            "glyphs": [{"offset": offset, "recorded": "号"}],
        }]});
        let mut f = Findings::default();
        check_record(&rec, "records/x.yaml", &mut f);
        f.items
            .iter()
            .map(|(_, d)| (d.rule.clone(), d.message.clone()))
            .collect()
    }

    #[test]
    fn a_glyph_offset_beyond_the_raw_number_is_a_finding_not_a_panic() {
        assert!(glyph_findings(2).is_empty());
        for offset in [0, 3, 9, u64::from(u32::MAX) + 1, u64::MAX - 1, u64::MAX] {
            assert_eq!(
                glyph_findings(offset),
                [(
                    "GLYPH-04".to_owned(),
                    format!("recorded text is not at offset {offset} of raw")
                )],
                "{offset}"
            );
        }
    }
}

#[cfg(test)]
mod space_tests {
    use super::*;
    #[test]
    fn explicit_space_is_context_not_an_identifier_alias() {
        let mut reg = Registry::default();
        reg.records.insert(
            "synthetic".into(),
            json!({"id":"synthetic", "status":"active"}),
        );
        let local = json!({"id":"synthetic"});
        let explicit = json!({"id":"synthetic", "space":"alpha"});
        assert_eq!(
            reg.resolve(&local),
            Resolution::Resolved("synthetic".into())
        );
        assert!(matches!(reg.resolve(&explicit), Resolution::NotFetched(_)));
        assert_eq!(reg.target(&local), None);
        reg.space = "alpha".into();
        assert_eq!(reg.resolve(&explicit), reg.resolve(&local));
        assert_eq!(reg.target(&local).as_deref(), Some("loj:alpha/synthetic"));
        assert!(matches!(
            reg.resolve(&json!({"id":"synthetic", "space":"beta"})),
            Resolution::NotFetched(_)
        ));
        reg.records.get_mut("synthetic").unwrap()["status"] = json!("unknown-future-status");
        assert!(matches!(reg.resolve(&local), Resolution::NotFetched(_)));
    }
}
