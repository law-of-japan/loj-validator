//! Change declarations and comparisons of acquired snapshots (CHG-02 through CHG-04).
use super::{Change, Findings, Snapshot, CH, COMMON};
use crate::common::Class;
use crate::repository::history::list;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

fn indexed(values: &Value) -> BTreeMap<&str, &Value> {
    list(values)
        .iter()
        .filter_map(|v| v["id"].as_str().map(|id| (id, v)))
        .collect()
}

pub(super) fn revision_ids(snapshot: &Snapshot) -> BTreeSet<String> {
    snapshot
        .history
        .as_ref()
        .map(|h| {
            indexed(&h["revisions"])
                .keys()
                .map(|s| (*s).to_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// Count entries before indexing revisions by ID, which would hide reuse.
fn revision_counts(h: &Value) -> BTreeMap<&str, usize> {
    let mut counts = BTreeMap::new();
    for id in list(&h["revisions"])
        .iter()
        .filter_map(|r| r["id"].as_str())
    {
        *counts.entry(id).or_default() += 1;
    }
    counts
}

fn retired_id_reuse(change: &Change<'_>, old: &Value, new: &Value, f: &mut Findings) {
    let old_counts = revision_counts(old);
    let new_counts = revision_counts(new);
    for r in list(&old["revisions"]) {
        let Some(id) = r["id"].as_str() else { continue };
        if matches!(r["status"].as_str(), Some("retracted" | "merged" | "split"))
            && old_counts.get(id) == Some(&1)
            && new_counts.get(id).is_some_and(|n| *n > 1)
        {
            f.error(
                CH,
                "REV-01",
                crate::repository::HISTORY,
                format!("retired Revision id reused: {id}"),
                change.at,
                "",
            );
        }
    }
}

fn version_file(path: &str) -> Option<(&str, &str)> {
    path.strip_prefix("versions/")?.split_once('/')
}

/// CHG-01: the canonical data files, from the layouts of HIST-01 (a law
/// repository) and REG-01 (a registry repository). Every other file of the
/// canonical branch (CI configuration, generators, documents) is outside the
/// canonical data and is not subject to any change kind's check.
/// The change-check loader filters snapshots to the selected layout before
/// applying this policy. Fixed-reference resolution uses an unfiltered snapshot
/// and never applies change policy.
fn canonical(path: &str) -> bool {
    use crate::checks::RepositoryKind;
    RepositoryKind::Law.canonical(path) || RepositoryKind::Registry.canonical(path)
}

/// CHG-02 `auxiliary`: the canonical data files are identical before and
/// after: none is added, removed or renamed, and each keeps its bytes and its
/// Git file mode. No AST is compared.
/// `files` always lists the whole tree: an export over the input budget is
/// refused as a whole (the check is then incomplete), never truncated.
pub(super) fn auxiliary(
    change: &Change<'_>,
    before: &Snapshot,
    after: &Snapshot,
    f: &mut Findings,
) {
    let paths: BTreeSet<&String> = before
        .files
        .keys()
        .chain(after.files.keys())
        .filter(|path| canonical(path))
        .collect();
    for path in paths {
        let problem = match (before.files.get(path), after.files.get(path)) {
            (None, _) => "adds",
            (_, None) => "removes",
            (Some(a), Some(b)) if a != b => "changes",
            _ if before.executable.contains(path) != after.executable.contains(path) => {
                "changes the file mode of"
            }
            _ => continue,
        };
        change.error(
            f,
            "CHG-02",
            format!("auxiliary {problem} canonical data file {path}"),
        );
    }
}

fn sources(value: &Value, out: &mut BTreeSet<String>) {
    let mut pending = vec![value];
    while let Some(value) = pending.pop() {
        match value {
            Value::Object(object) => {
                if let Some(source) = object.get("source").and_then(Value::as_str) {
                    out.insert(source.into());
                }
                pending.extend(object.values());
            }
            Value::Array(values) => pending.extend(values),
            _ => {}
        }
    }
}

/// Ambiguous IDs cannot be projected into maps for a change comparison. This
/// is a missing comparison prerequisite, not a duplicate L3 conformance check.
pub(super) fn processable(change: &Change<'_>, snapshot: &Snapshot, f: &mut Findings) -> bool {
    let Some(history) = &snapshot.history else {
        return true;
    };
    for field in ["revisions", "relations", "sources"] {
        let Some(value) = history.get(field) else {
            continue;
        };
        let mut seen = BTreeSet::new();
        let valid = value.as_array().is_some_and(|values| {
            values.iter().all(|v| {
                v["id"]
                    .as_str()
                    .is_some_and(|id| !id.is_empty() && seen.insert(id))
            })
        });
        if !valid {
            f.note(
                CH,
                Class::Undetermined,
                "CHG-02",
                COMMON,
                format!("cannot compare ambiguous or malformed {field} IDs"),
                change.at,
                "",
            );
            return false;
        }
    }
    true
}

fn remove(value: &mut Value, keys: &[&str]) {
    if let Some(object) = value.as_object_mut() {
        for key in keys {
            object.remove(*key);
        }
    }
}

/// CHG-02: whether a commit of this kind may change the canonical data file
/// `path`; `affected` are its declared (or, for `revisionAdd`, added) versions.
fn may_change(change: &Change<'_>, affected: &BTreeSet<String>, path: &str) -> bool {
    let version = version_file(path);
    let package = version.is_some_and(|(_, file)| file != "glyphs.yaml");
    match change.kind {
        "format" => package,
        "metadata" => !package,
        "correction" | "officialCorrection" | "restoration" | "revisionAdd" | "revisionFix" => {
            match version {
                Some((id, _)) => affected.contains(id),
                None if path == "history.yaml" => true,
                None if path == "rebuild.yaml" => {
                    matches!(change.kind, "restoration" | "revisionAdd")
                }
                _ => false,
            }
        }
        _ => false,
    }
}

/// CHG-02: a canonical data file that the commit adds or changes (by bytes or
/// by mode) has Git file mode `100644`. Files the commit leaves untouched are
/// not checked, so one wrong mode does not fail every later commit; a commit
/// without a parent changes every file. The export refuses any mode other than
/// `100644` and `100755`, so only `100755` is found here.
/// `executable` lists files of the tree only, so a removed file is never seen.
pub(super) fn modes(change: &Change<'_>, before: &Snapshot, after: &Snapshot, f: &mut Findings) {
    for path in &after.executable {
        if canonical(path)
            && (before.files.get(path) != after.files.get(path)
                || !before.executable.contains(path))
        {
            change.error(
                f,
                "CHG-02",
                format!("canonical data file {path} has mode 100755, not 100644"),
            );
        }
    }
}

pub(super) fn check(change: &Change<'_>, before: &Snapshot, after: &Snapshot, f: &mut Findings) {
    let old_ids = revision_ids(before);
    let new_ids = revision_ids(after);
    let affected: BTreeSet<String> = if change.kind == "revisionAdd" {
        new_ids.difference(&old_ids).cloned().collect()
    } else {
        change.declared.iter().cloned().collect()
    };
    if change.kind == "revisionAdd" {
        if let Some(history) = &after.history {
            let by_id: std::collections::HashMap<_, _> =
                indexed(&history["revisions"]).into_iter().collect();
            for (id, r) in &by_id {
                if !affected.contains(*id) {
                    continue;
                }
                if r.get("basedOn").is_some() == r.get("origin").is_some() {
                    change.error(
                        f,
                        "CHG-02",
                        format!("new Revision {id} must declare either origin or basedOn"),
                    );
                }
                if let Some(base) = r["basedOn"].as_str() {
                    crate::repository::history::based_on_order(r, base, &by_id, |message| {
                        change.error(f, "CHG-02", format!("new Revision {id}: {message}"))
                    });
                }
            }
        }
    }
    if change.kind != "revisionAdd"
        && new_ids
            .difference(&old_ids)
            .any(|id| !affected.contains(id))
    {
        change.error(f, "CHG-02", "new Revision was not declared".into());
    }
    if change.kind == "revisionFix"
        && before
            .packages
            .values()
            .any(|path| before.files.contains_key(path) && !after.files.contains_key(path))
    {
        change.error(
            f,
            "CHG-02",
            "revisionFix must retain existing version packages".into(),
        );
    }
    if !old_ids.is_subset(&new_ids) {
        change.error(
            f,
            "CHG-02",
            "Revision records must be retained, including on revert".into(),
        );
    }
    if !matches!(change.kind, "revisionAdd" | "revisionFix") && old_ids != new_ids {
        change.error(
            f,
            "CHG-02",
            "This change kind cannot add or remove Revision identities".into(),
        );
    }
    let changed: BTreeSet<&String> = before
        .files
        .keys()
        .chain(after.files.keys())
        .filter(|path| {
            before.files.get(*path) != after.files.get(*path)
                || before.executable.contains(*path) != after.executable.contains(*path)
        })
        .collect();
    for path in changed {
        if !canonical(path) {
            // CHG-01/CHG-02: files outside the canonical data may change in a
            // commit of any kind and do not affect its check.
            continue;
        }
        if !may_change(change, &affected, path) {
            change.error(f, "CHG-02", format!("{} cannot change {path}", change.kind));
        }
        // A mode-only change keeps the bytes, so there is nothing new to inspect.
        if path.ends_with(".yaml")
            && before.files.get(path) != after.files.get(path)
            && after.files.contains_key(path)
            && !after.documents.contains_key(path)
        {
            f.note(
                CH,
                Class::Undetermined,
                "CHG-02",
                COMMON,
                format!("cannot inspect changed YAML {path}"),
                change.at,
                "",
            );
        }
    }
    if !change.initial
        && matches!(
            change.kind,
            "correction" | "officialCorrection" | "restoration" | "revisionAdd" | "revisionFix"
        )
    {
        history(change, before, after, &affected, f);
        if before.files.get("rebuild.yaml") != after.files.get("rebuild.yaml") {
            mappings(change, before, after, &affected, f);
        }
    }
    if change.kind == "metadata" {
        registry_ids(change, before, after, f);
    }
}

fn history(
    change: &Change<'_>,
    before: &Snapshot,
    after: &Snapshot,
    affected: &BTreeSet<String>,
    f: &mut Findings,
) {
    if before.history.is_none() && after.history.is_some() && change.kind != "revisionAdd" {
        change.error(
            f,
            "CHG-02",
            "only revisionAdd can introduce a history manifest in a text change".into(),
        );
    }
    let empty = serde_json::json!({});
    let (old, new) = (
        before.history.as_ref().unwrap_or(&empty),
        after.history.as_ref().unwrap_or(&empty),
    );
    let (old_revisions, new_revisions) = (indexed(&old["revisions"]), indexed(&new["revisions"]));
    let mut allowed_sources = BTreeSet::new();
    let mut selected_items: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for h in [old, new] {
        for revision in list(&h["revisions"])
            .iter()
            .filter(|r| r["id"].as_str().is_some_and(|id| affected.contains(id)))
        {
            sources(revision, &mut allowed_sources);
            for basis in list(&revision["basis"]) {
                if let Some(relation) = basis["relation"].as_str() {
                    selected_items.entry(relation.into()).or_default().extend(
                        list(&basis["items"])
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned),
                    );
                }
            }
        }
    }
    for (id, revision) in &old_revisions {
        let Some(next) = new_revisions.get(id) else {
            continue;
        };
        let (mut a, mut b) = ((*revision).clone(), (*next).clone());
        if affected.contains(*id) && change.kind != "revisionAdd" {
            if change.kind == "revisionFix" {
                continue;
            }
            let fields: &[&str] = if change.kind == "restoration" {
                &["status", "gaps", "sources", "glyphCheck", "glyphNotes"]
            } else {
                &["sources", "glyphCheck", "glyphNotes"]
            };
            if let Some(text) = a.get_mut("text") {
                remove(text, fields);
            }
            if let Some(text) = b.get_mut("text") {
                remove(text, fields);
            }
        }
        if a != b {
            change.error(
                f,
                "CHG-02",
                format!("unrelated Revision metadata changed: {id}"),
            );
        }
    }
    if matches!(
        change.kind,
        "correction" | "officialCorrection" | "restoration"
    ) {
        for id in affected {
            let Some(revision) = new_revisions.get(id.as_str()) else {
                change.error(
                    f,
                    "CHG-02",
                    format!("declared Revision does not exist: {id}"),
                );
                continue;
            };
            if list(&revision["text"]["sources"]).is_empty() {
                change.error(
                    f,
                    "CHG-02",
                    format!("declared Revision {id} requires source evidence"),
                );
            }
            if change.kind == "officialCorrection"
                && !list(&revision["text"]["sources"]).iter().any(|e| {
                    list(&new["sources"])
                        .iter()
                        .any(|s| s["id"] == e["source"] && s["kind"] == "officialCorrection")
                })
            {
                change.error(
                    f,
                    "CHG-02",
                    format!("Revision {id} has no official-correction Source"),
                );
            }
            if change.kind == "restoration"
                && old_revisions.get(id.as_str()).is_some_and(|old| {
                    old["text"]["status"] == revision["text"]["status"]
                        && old["text"]["gaps"] == revision["text"]["gaps"]
                })
            {
                change.error(
                    f,
                    "CHG-02",
                    format!("restoration does not update the text status or gaps of {id}"),
                );
            }
        }
    }
    let (old_relations, new_relations) = (indexed(&old["relations"]), indexed(&new["relations"]));
    for id in old_relations
        .keys()
        .chain(new_relations.keys())
        .copied()
        .collect::<BTreeSet<_>>()
    {
        let (a, b) = (old_relations.get(id), new_relations.get(id));
        if a == b {
            continue;
        }
        if a.is_none() && change.kind == "revisionAdd" {
            if let Some(relation) = b {
                sources(relation, &mut allowed_sources);
            }
            continue;
        }
        let permitted = selected_items.get(id);
        if change.kind == "revisionFix"
            && permitted.is_some()
            && b.is_some_and(|r| r["status"] != "retracted")
        {
            for relation in [a, b].into_iter().flatten() {
                sources(relation, &mut allowed_sources);
            }
            continue;
        }
        let project = |relation: Option<&&Value>| {
            let mut value = relation.map_or(Value::Null, |value| (*value).clone());
            if let Some(items) = permitted {
                remove(&mut value, &["evidence"]);
                if let Some(values) = value.get_mut("items").and_then(Value::as_array_mut) {
                    for item in values {
                        if item["id"].as_str().is_some_and(|id| items.contains(id)) {
                            remove(item, &["evidence"]);
                            if let Some(scope) = item.get_mut("scope") {
                                remove(scope, &["nodes"]);
                            }
                        }
                    }
                }
            }
            value
        };
        if change.kind == "revisionAdd" || project(a) != project(b) {
            change.error(
                f,
                "CHG-02",
                format!("unrelated relation metadata changed: {id}"),
            );
        } else {
            for relation in [a, b].into_iter().flatten() {
                sources(relation, &mut allowed_sources);
            }
        }
    }
    let (old_sources, new_sources) = (indexed(&old["sources"]), indexed(&new["sources"]));
    for id in old_sources
        .keys()
        .chain(new_sources.keys())
        .copied()
        .collect::<BTreeSet<_>>()
    {
        if old_sources.get(id) == new_sources.get(id) {
            continue;
        }
        if !allowed_sources.contains(id)
            || (change.kind == "revisionAdd" && old_sources.contains_key(id))
        {
            change.error(f, "CHG-02", format!("unrelated Source changed: {id}"));
        }
    }
    let (mut a, mut b) = (old.clone(), new.clone());
    remove(&mut a, &["revisions", "relations", "sources"]);
    remove(&mut b, &["revisions", "relations", "sources"]);
    if a != b {
        change.error(f, "CHG-02", "unrelated history metadata changed".into());
    }
}

fn mappings(
    change: &Change<'_>,
    before: &Snapshot,
    after: &Snapshot,
    affected: &BTreeSet<String>,
    f: &mut Findings,
) {
    let project = |snapshot: &Snapshot| {
        let mut doc = snapshot
            .documents
            .get("rebuild.yaml")
            .cloned()
            .unwrap_or(Value::Null);
        if let Some(mappings) = doc.get_mut("mappings").and_then(Value::as_array_mut) {
            mappings.retain_mut(|mapping| {
                let Some(revisions) = mapping.get_mut("revisions").and_then(Value::as_array_mut)
                else {
                    return true;
                };
                let had_revisions = !revisions.is_empty();
                revisions.retain(|id| !id.as_str().is_some_and(|id| affected.contains(id)));
                !had_revisions || !revisions.is_empty()
            });
        }
        doc
    };
    if project(before) != project(after) {
        change.error(f, "CHG-02", "unrelated rebuild data changed".into());
    }
}

fn registry_ids(change: &Change<'_>, before: &Snapshot, after: &Snapshot, f: &mut Findings) {
    let mut reserved = BTreeMap::new();
    for (path, record) in &before.documents {
        if !path.starts_with("records/")
            || !matches!(
                record["status"].as_str(),
                Some("merged" | "split" | "withdrawn")
            )
        {
            continue;
        }
        if !after.files.contains_key(path) && !after.documents.contains_key(path) {
            f.error(
                CH,
                "REG-01",
                crate::registry::REGISTRY,
                format!("retired Record file must be retained: {path}"),
                change.at,
                "",
            );
        }
        for id in std::iter::once(&record["id"])
            .chain(list(&record["aliases"]).iter().map(|alias| &alias["id"]))
        {
            if let Some(id) = id.as_str() {
                reserved.insert(id.to_ascii_lowercase(), path);
            }
        }
    }
    for (path, record) in &after.documents {
        if !path.starts_with("records/") {
            continue;
        }
        for id in std::iter::once(&record["id"])
            .chain(list(&record["aliases"]).iter().map(|alias| &alias["id"]))
        {
            if let Some(owner) = id
                .as_str()
                .and_then(|id| reserved.get(&id.to_ascii_lowercase()))
            {
                if *owner != path
                    || !matches!(
                        record["status"].as_str(),
                        Some("merged" | "split" | "withdrawn")
                    )
                {
                    f.error(
                        CH,
                        "RID-05",
                        crate::registry::REGISTRY,
                        format!("retired registry identifier reused in {path}"),
                        change.at,
                        "",
                    );
                }
            }
        }
    }
}

pub(super) const KINDS: &[&str] = &[
    "format",
    "correction",
    "officialCorrection",
    "restoration",
    "metadata",
    "revisionAdd",
    "revisionFix",
    "migration",
    "auxiliary",
];

/// CHG-03: only a wholly valid final paragraph, excluding the first one.
fn trailer_fields(message: &str) -> Vec<(&str, &str)> {
    let mut paragraphs: Vec<Vec<&str>> = Vec::new();
    let mut current = Vec::new();
    for line in message
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
    {
        if line.trim_matches([' ', '\t']).is_empty() {
            if !current.is_empty() {
                paragraphs.push(std::mem::take(&mut current));
            }
        } else {
            current.push(line);
        }
    }
    if !current.is_empty() {
        paragraphs.push(current);
    }
    if paragraphs.len() < 2 {
        return Vec::new();
    }
    let mut fields = Vec::new();
    for line in paragraphs.last().into_iter().flatten() {
        let Some((name, value)) = line.split_once(':') else {
            return Vec::new();
        };
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Vec::new();
        }
        fields.push((name, value.trim_matches([' ', '\t'])));
    }
    fields
}

pub(super) fn trailers(message: &str) -> (Vec<String>, Vec<String>) {
    let fields = trailer_fields(message);
    let kinds = fields
        .iter()
        .filter(|(name, _)| *name == "LoJ-Change")
        .map(|(_, value)| (*value).into())
        .collect();
    let revisions: Vec<String> = fields
        .iter()
        .filter(|(name, _)| *name == "LoJ-Revisions")
        .map(|(_, value)| (*value).into())
        .collect();
    (kinds, revisions)
}

pub(super) fn reviewed(message: &str) -> bool {
    let fields = trailer_fields(message);
    let values: Vec<_> = fields
        .iter()
        .filter(|(name, _)| *name == "LoJ-Reviewed")
        .collect();
    matches!(values.as_slice(), [(_, value)] if !value.is_empty())
}

/// CHG-03: the commit declares exactly one known `LoJ-Change`. Returns the
/// kind and the `LoJ-Revisions`, or `None` when the declarations are invalid
/// and the trees must not be compared. Revision trailer lines are not combined.
pub(super) fn declared_change(
    commit: &str,
    at: &str,
    message: &str,
    f: &mut Findings,
) -> Option<(String, Vec<String>)> {
    let (kinds, revisions) = trailers(message);
    if revisions.len() > 1 {
        f.error(
            CH,
            "CHG-03",
            COMMON,
            format!("{commit} declares more than one LoJ-Revisions"),
            at,
            "",
        );
        return None;
    }
    let problem = match kinds.as_slice() {
        [k] if KINDS.contains(&k.as_str()) => {
            let declared = revisions
                .first()
                .map(|value| value.split(", ").map(str::to_owned).collect())
                .unwrap_or_default();
            return Some((k.clone(), declared));
        }
        [] => format!("{commit} declares no LoJ-Change"),
        [k] => format!("{commit}: unknown change kind {k}"),
        _ => format!("{commit} declares more than one LoJ-Change"),
    };
    f.error(CH, "CHG-03", COMMON, problem, at, "");
    None
}

impl Change<'_> {
    fn error(&self, f: &mut Findings, rule: &str, message: String) {
        f.error(CH, rule, COMMON, message, self.at, "");
    }

    /// Check modes even if a manifest is unusable. Only auxiliary changes can
    /// also compare the canonical files without a usable manifest.
    pub(super) fn check_snapshots(
        &self,
        parent: Option<&str>,
        before: &Snapshot,
        after: &Snapshot,
        noted: &mut BTreeSet<String>,
        f: &mut Findings,
    ) {
        if self.kind != "auxiliary" {
            modes(self, before, after, f);
            let mut usable = true;
            for (commit, snapshot) in parent
                .map(|id| (id, before))
                .into_iter()
                .chain([(self.commit, after)])
            {
                if let Some(error) = &snapshot.manifest_error {
                    usable = false;
                    if noted.insert(commit.to_owned()) {
                        f.incomplete.insert(CH);
                        f.note(
                            CH,
                            Class::Undetermined,
                            "CHG-02",
                            COMMON,
                            format!("{commit}: tree refused: {error}"),
                            self.at,
                            "",
                        );
                    }
                }
            }
            if !usable {
                return;
            }
        }
        self.compare(before, after, f);
    }

    /// CHG-02 / CHG-03 / CHG-04: compare the states before and after the commit.
    pub(super) fn compare(&self, before: &Snapshot, after: &Snapshot, f: &mut Findings) {
        let (commit, kind) = (self.commit, self.kind);
        if kind == "migration" {
            self.error(
                f,
                "CHG-02",
                "this specification provides no normative AST migration mapping".into(),
            );
            return;
        }
        if kind == "auxiliary" {
            // CHG-02: only the canonical data files are compared, byte for byte
            // and by mode. No AST is built or compared, so a version that cannot
            // build one is not undetermined here (CHG-04).
            auxiliary(self, before, after, f);
            return;
        }
        if kind == "revisionFix" {
            if let (Some(old), Some(new)) = (&before.history, &after.history) {
                retired_id_reuse(self, old, new, f);
            }
        }
        if !processable(self, before, f) || !processable(self, after, f) {
            return;
        }
        self.kept_asts(before, after, f);
        check(self, before, after, f);
        match kind {
            "format" if before.history_bytes != after.history_bytes => self.error(
                f,
                "CHG-02",
                format!("{commit}: format changes history.yaml"),
            ),
            "metadata" if before.packages != after.packages => self.error(
                f,
                "CHG-02",
                format!("{commit}: metadata changes the version packages"),
            ),
            "revisionAdd"
                if !self.initial
                    && revision_ids(after)
                        .iter()
                        .all(|k| revision_ids(before).contains(k)) =>
            {
                self.error(
                    f,
                    "CHG-02",
                    format!("{commit}: revisionAdd adds no version"),
                )
            }
            _ => {}
        }
    }

    /// Whether the AST of revision `id` must stay identical under this change.
    pub(super) fn must_keep(&self, id: &str) -> bool {
        match self.kind {
            "format" | "metadata" | "revisionAdd" => true,
            "auxiliary" => false,
            _ => !self.declared.iter().any(|d| d == id),
        }
    }

    /// CHG-02: the ASTs that the change must keep are identical. CHG-04: an AST
    /// that cannot be built is never compared as equal.
    fn kept_asts(&self, before: &Snapshot, after: &Snapshot, f: &mut Findings) {
        let (commit, kind) = (self.commit, self.kind);
        for (id, old) in &before.asts {
            if !self.must_keep(id) {
                continue;
            }
            match (old, after.asts.get(id)) {
                (Ok(a), Some(Ok(b))) => {
                    if a != b {
                        self.error(
                            f,
                            "CHG-02",
                            format!("{commit}: {kind} changes the AST of revision {id}"),
                        );
                    }
                }
                (_, None) => {
                    if kind != "revisionFix" || !self.declared.contains(id) {
                        self.error(
                            f,
                            "CHG-02",
                            format!("{commit}: {kind} removes revision {id}"),
                        );
                    }
                }
                (Err(e), _) | (_, Some(Err(e))) => {
                    let prefix = format!("versions/{id}/");
                    let changed = before.files.keys().chain(after.files.keys()).any(|path| {
                        path.starts_with(&prefix)
                            && !path.ends_with("/glyphs.yaml")
                            && before.files.get(path) != after.files.get(path)
                    });
                    if kind == "format" && changed && !self.reviewed {
                        self.error(f, "CHG-04", format!("{commit}: format changes uncomparable revision {id} without a review record"));
                    }
                    f.incomplete.insert(CH);
                    f.note(
                        CH,
                        Class::Undetermined,
                        "CHG-04",
                        COMMON,
                        format!("{commit}: revision {id} not compared: {e}"),
                        self.at,
                        "",
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn inspect(kind: &str, before: &Snapshot, after: &Snapshot) -> Findings {
        let mut f = Findings::default();
        let change = Change {
            initial: false,
            commit: "c",
            at: "c",
            kind,
            declared: &["b7k2m9qd".into()],
            reviewed: false,
        };
        check(&change, before, after, &mut f);
        f
    }
    fn snapshot(history: Value) -> Snapshot {
        Snapshot {
            history: Some(history),
            ..Snapshot::default()
        }
    }
    #[test]
    fn revision_add_after_a_root_without_history_cannot_introduce_top_level_metadata() {
        let after = snapshot(json!({
            "schemaVersion": "loj-history/v1alpha1",
            "record": "20990101-act-1",
            "revisions": [{"id": "b7k2m9qd", "text": {"status": "absent"}}]
        }));
        let f = inspect("revisionAdd", &Snapshot::default(), &after);
        assert!(f
            .items
            .iter()
            .any(|(_, d)| d.rule == "CHG-02" && d.message == "unrelated history metadata changed"));
    }

    #[test]
    fn revision_add_reports_a_missing_base_as_a_change_violation() {
        let after = snapshot(json!({"revisions":[{"id":"a","basedOn":"missing"}]}));
        let f = inspect("revisionAdd", &Snapshot::default(), &after);
        assert!(f
            .items
            .iter()
            .any(|(_, d)| d.rule == "CHG-02" && d.message.contains("basedOn missing")));
    }

    #[test]
    fn retired_id_reuse_compares_multiplicities() {
        let change = Change {
            initial: false,
            commit: "c",
            at: "c",
            kind: "revisionFix",
            declared: &[],
            reviewed: false,
        };
        for status in ["retracted", "merged", "split"] {
            let retired = json!({"id":"a","status":status});
            let restored = json!({"id":"a","status":"active","text":{"status":"absent"}});
            let old = json!({"revisions":[retired]});
            let single = json!({"revisions":[restored]});
            let duplicate = json!({"revisions":[restored,retired]});
            for (before, after, expected) in [
                (&old, &single, false),
                (&old, &duplicate, true),
                (&duplicate, &duplicate, false),
            ] {
                let mut f = Findings::default();
                retired_id_reuse(&change, before, after, &mut f);
                assert_eq!(f.items.iter().any(|(_, d)| d.rule == "REV-01"), expected);
            }
        }
    }

    #[test]
    fn dependent_mappings_and_revision_fix_are_scoped() {
        let h = json!({"revisions":[{"id":"b7k2m9qd","status":"active","basis":[{"relation":"rel","items":["item"]}]}],"relations":[{"id":"rel","status":"active","items":[{"id":"item"}]}],"termination":[{"relation":"rel"}]});
        let before = snapshot(h.clone());
        let mut after = snapshot(h);
        let h = after.history.as_mut().unwrap();
        h["revisions"][0]["status"] = json!("retracted");
        assert!(!inspect("revisionFix", &before, &after).failed(CH));
        let h = after.history.as_mut().unwrap();
        h["relations"] = json!([]);
        assert!(inspect("revisionFix", &before, &after).failed(CH));
        let h = after.history.as_mut().unwrap();
        h["relations"] = before.history.as_ref().unwrap()["relations"].clone();
        h["relations"][0]["status"] = json!("retracted");
        assert!(inspect("revisionFix", &before, &after).failed(CH));
        let h = after.history.as_mut().unwrap();
        h["relations"] = before.history.as_ref().unwrap()["relations"].clone();
        h["termination"] = json!([]);
        assert!(inspect("revisionFix", &before, &after).failed(CH));
        after.history.as_mut().unwrap()["revisions"][0]["id"] = json!("c7k2m9qd");
        assert!(inspect("revisionFix", &before, &after).failed(CH));
        let mut before = Snapshot::default();
        let mut after = Snapshot::default();
        before.documents.insert(
            "rebuild.yaml".into(),
            json!({"mappings":[{"commit":"c","revisions":["b7k2m9qd"],"verification":"old"}]}),
        );
        after
            .documents
            .insert("rebuild.yaml".into(), json!({"mappings":[]}));
        let change = Change {
            initial: false,
            commit: "c",
            at: "c",
            kind: "restoration",
            declared: &[],
            reviewed: false,
        };
        let mut f = Findings::default();
        mappings(
            &change,
            &before,
            &after,
            &BTreeSet::from(["b7k2m9qd".into()]),
            &mut f,
        );
        assert!(!f.failed(CH));
        before.documents.get_mut("rebuild.yaml").unwrap()["mappings"][0]["revisions"] =
            json!(["b7k2m9qd", "c7k2m9qd"]);
        mappings(
            &change,
            &before,
            &after,
            &BTreeSet::from(["b7k2m9qd".into()]),
            &mut f,
        );
        assert!(f.failed(CH));
    }

    #[test]
    fn official_correction_requires_its_source_kind_and_retired_records_survive() {
        let before = snapshot(
            json!({"revisions":[{"id":"b7k2m9qd","text":{"sources":[{"source":"s"}]}}],"sources":[{"id":"s","kind":"other"}]}),
        );
        let after = snapshot(before.history.clone().unwrap());
        assert!(inspect("officialCorrection", &before, &after).failed(CH));
        let mut after = after;
        after.history.as_mut().unwrap()["sources"][0]["kind"] = json!("officialCorrection");
        assert!(!inspect("officialCorrection", &before, &after).failed(CH));
        let mut before = Snapshot::default();
        before.documents.insert(
            "records/retired.yaml".into(),
            json!({"id":"retired","status":"merged"}),
        );
        let after = Snapshot::default();
        let f = inspect("metadata", &before, &after);
        assert!(f
            .items
            .iter()
            .any(|(_, d)| d.rule == "REG-01" && d.schema_version == crate::registry::REGISTRY));
        let empty = Snapshot::default();
        assert!(inspect("correction", &empty, &snapshot(json!({"revisions":[]}))).failed(CH));
    }

    #[test]
    fn ambiguous_ids_are_incomplete_and_unknown_kinds_cannot_change_data() {
        let change = Change {
            initial: false,
            commit: "c",
            at: "c",
            kind: "metadata",
            declared: &[],
            reviewed: false,
        };
        for entries in [
            json!([{"id":"same"},{"id":"same"}]),
            json!([{"id":false}]),
            json!([{}]),
            json!("not an array"),
        ] {
            let mut f = Findings::default();
            assert!(!processable(
                &change,
                &snapshot(json!({"revisions":entries})),
                &mut f
            ));
            assert_eq!(f.result(CH), crate::common::ResultValue::Incomplete);
        }
        let before = Snapshot::default();
        let mut after = Snapshot::default();
        after.files.insert("history.yaml".into(), [1; 32]);
        assert!(inspect("future-kind", &before, &after).failed(CH));
        after.files.clear();
        after.files.insert("records/new.yaml".into(), [1; 32]);
        assert!(inspect("revisionAdd", &before, &after).failed(CH));
    }

    #[test]
    fn corrections_keep_unrelated_revision_metadata_and_allow_only_dependent_fields() {
        let h = json!({"revisions":[{"id":"b7k2m9qd","effective":"unchanged","text":{"sources":[{"source":"s"}],"glyphCheck":"pending"}}, {"id":"c7k2m9qd","text":{}}]});
        let before = snapshot(h.clone());
        let mut after = snapshot(h);
        after.history.as_mut().unwrap()["revisions"][0]["text"]["glyphCheck"] = json!("verified");
        assert!(!inspect("correction", &before, &after).failed(CH));
        after.history.as_mut().unwrap()["revisions"][1]["label"] = json!("unrelated");
        assert!(inspect("correction", &before, &after).failed(CH));
    }
    #[test]
    fn declarations_of_a_version_without_text_follow_chg02() {
        // An absent version carries no text.sources (HIST-08), so every
        // correction-like kind that declares it lacks evidence, and only
        // restoration may change text.status.
        let absent = json!({"revisions":[{"id":"b7k2m9qd","text":{"status":"absent"}}]});
        let partial = json!({"revisions":[{"id":"b7k2m9qd","text":{"status":"partial","gaps":[{"article":"1"}],"sources":[{"source":"s"}]}}]});
        let complete = json!({"revisions":[{"id":"b7k2m9qd","text":{"status":"complete","sources":[{"source":"s"}]}}]});
        let errors = |kind: &str, before: &Value, after: &Value| -> Vec<String> {
            let f = inspect(kind, &snapshot(before.clone()), &snapshot(after.clone()));
            f.items
                .iter()
                .filter(|(_, d)| d.rule == "CHG-02" && d.is_error())
                .map(|(_, d)| d.message.clone())
                .collect()
        };
        let has = |errors: &[String], text: &str| errors.iter().any(|m| m.contains(text));
        const EVIDENCE: &str = "requires source evidence";
        const STATUS: &str = "unrelated Revision metadata changed";
        // restoration: absent before and after, and text turned into absent
        let e = errors("restoration", &absent, &absent);
        assert!(has(&e, EVIDENCE) && has(&e, "does not update the text status"));
        assert!(has(&errors("restoration", &complete, &absent), EVIDENCE));
        assert!(has(&errors("restoration", &partial, &absent), EVIDENCE));
        // restoration that adds or completes text passes
        assert_eq!(
            errors("restoration", &absent, &complete),
            Vec::<String>::new()
        );
        assert_eq!(
            errors("restoration", &partial, &complete),
            Vec::<String>::new()
        );
        // correction may not change text.status, in either direction
        assert_eq!(
            errors("correction", &partial, &complete),
            vec![format!("{STATUS}: b7k2m9qd")]
        );
        assert_eq!(
            errors("correction", &absent, &complete),
            vec![format!("{STATUS}: b7k2m9qd")]
        );
        let e = errors("correction", &complete, &absent);
        assert!(has(&e, STATUS) && has(&e, EVIDENCE));
        assert!(has(
            &errors("officialCorrection", &absent, &absent),
            EVIDENCE
        ));
        assert_eq!(
            errors("correction", &complete, &complete),
            Vec::<String>::new()
        );
    }

    #[test]
    fn metadata_cannot_touch_package_bytes_and_revert_keeps_revision_records() {
        let mut before = snapshot(json!({"revisions":[{"id":"b7k2m9qd"}]}));
        before
            .files
            .insert("versions/b7k2m9qd/main.md".into(), [0; 32]);
        let mut after = snapshot(before.history.clone().unwrap());
        after
            .files
            .insert("versions/b7k2m9qd/main.md".into(), [1; 32]);
        assert!(inspect("metadata", &before, &after).failed(CH));
        after.history = Some(json!({"revisions":[]}));
        assert!(inspect("revisionFix", &before, &after).failed(CH));
    }

    fn auxiliary_findings(before: &Snapshot, after: &Snapshot) -> Findings {
        let mut f = Findings::default();
        let change = Change {
            initial: false,
            commit: "c",
            at: "c",
            kind: "auxiliary",
            declared: &[],
            reviewed: false,
        };
        auxiliary(&change, before, after, &mut f);
        f
    }

    #[test]
    fn auxiliary_keeps_every_canonical_file_by_bytes_and_mode() {
        let mut before = Snapshot::default();
        for path in [
            "history.yaml",
            "rebuild.yaml",
            "space.yaml",
            "versions/b7k2m9qd/main.md",
            "versions/b7k2m9qd/README.md",
            "records/a.yaml",
            ".github/workflows/ci.yml",
            "tools/gen.sh",
        ] {
            before.files.insert(path.into(), [0; 32]);
        }
        before.executable.insert("tools/gen.sh".into());
        let base = || Snapshot {
            files: before.files.clone(),
            executable: before.executable.clone(),
            ..Snapshot::default()
        };
        // Files outside the canonical data: bytes, mode, addition and removal.
        let mut after = base();
        after
            .files
            .insert(".github/workflows/ci.yml".into(), [1; 32]);
        after.files.insert("docs/new.md".into(), [1; 32]);
        after.files.remove("tools/gen.sh");
        after.executable.clear();
        assert!(!auxiliary_findings(&before, &after).failed(CH));
        assert!(!auxiliary_findings(&before, &before).failed(CH));
        for path in [
            "history.yaml",
            "rebuild.yaml",
            "space.yaml",
            "versions/b7k2m9qd/main.md",
            "versions/b7k2m9qd/README.md",
            "records/a.yaml",
        ] {
            let mut changed = base();
            changed.files.insert(path.into(), [1; 32]);
            assert!(auxiliary_findings(&before, &changed).failed(CH), "{path}");
            let mut removed = base();
            removed.files.remove(path);
            assert!(auxiliary_findings(&before, &removed).failed(CH), "{path}");
            let mut executable = base();
            executable.executable.insert(path.into());
            assert!(
                auxiliary_findings(&before, &executable).failed(CH),
                "{path}"
            );
            assert!(
                auxiliary_findings(&executable, &before).failed(CH),
                "{path}"
            );
        }
        // A rename within the canonical data is an addition and a removal.
        let mut renamed = base();
        renamed.files.remove("records/a.yaml");
        renamed.files.insert("records/b.yaml".into(), [0; 32]);
        let f = auxiliary_findings(&before, &renamed);
        assert_eq!(
            f.items.iter().filter(|(_, d)| d.rule == "CHG-02").count(),
            2
        );
        for path in ["versions/b7k2m9qd/new.md", "records/new.yaml", "space.yaml"] {
            let mut added = Snapshot::default();
            added.files.insert(path.into(), [0; 32]);
            assert!(
                auxiliary_findings(&Snapshot::default(), &added).failed(CH),
                "{path}"
            );
        }
        // Lookalike paths outside the canonical data.
        let mut lookalike = base();
        for path in [
            "docs/history.yaml",
            "versions.md",
            "records.yaml",
            "x/space.yaml",
        ] {
            lookalike.files.insert(path.into(), [1; 32]);
        }
        assert!(!auxiliary_findings(&before, &lookalike).failed(CH));
    }

    #[test]
    fn space_manifest_is_canonical_data_for_the_other_kinds() {
        let before = Snapshot::default();
        let mut after = Snapshot::default();
        after.files.insert("space.yaml".into(), [1; 32]);
        assert!(inspect("format", &before, &after).failed(CH));
        assert!(inspect("correction", &before, &after).failed(CH));
        assert!(!inspect("metadata", &before, &after).failed(CH));
        let mut ci = Snapshot::default();
        ci.files.insert(".github/workflows/ci.yml".into(), [1; 32]);
        assert!(!inspect("format", &before, &ci).failed(CH));
    }

    /// CHG-02: changing only the Git file mode changes the file, so a kind
    /// fails for a mode-only change to a file outside its range and passes one
    /// to a file inside it, in either direction (law-of-japan/loj-validator#57).
    #[test]
    fn mode_only_change_is_a_change_of_the_file() {
        let package = "versions/b7k2m9qd/main.md";
        for (kind, path, outside) in [
            ("metadata", package, true),
            ("format", "history.yaml", true),
            ("correction", "versions/c7k2m9qd/main.md", true),
            ("format", package, false),
            ("metadata", "history.yaml", false),
            ("correction", package, false),
            ("metadata", ".github/gen.sh", false),
            ("correction", "rebuild.yaml", true),
            ("restoration", "rebuild.yaml", false),
            ("revisionAdd", "versions/c7k2m9qd/main.md", true),
            ("revisionFix", package, false),
        ] {
            for to_executable in [true, false] {
                let mut before = Snapshot::default();
                before.files.insert(path.into(), [1; 32]);
                let mut after = Snapshot {
                    files: before.files.clone(),
                    ..Snapshot::default()
                };
                if to_executable {
                    after.executable.insert(path.into());
                } else {
                    before.executable.insert(path.into());
                }
                let f = inspect(kind, &before, &after);
                // `correction` fails here for its missing history, never for the file.
                let about_path: Vec<_> = f
                    .items
                    .iter()
                    .map(|(_, d)| d)
                    .filter(|d| d.message.contains(path))
                    .collect();
                assert!(
                    about_path
                        .iter()
                        .all(|d| d.rule == "CHG-02" && d.is_error()),
                    "{kind} {path}"
                );
                assert_eq!(!about_path.is_empty(), outside, "{kind} {path}");
                assert!(!f.incomplete.contains(&CH), "{kind} {path}");
            }
        }
    }

    fn modes_findings(kind: &str, before: &Snapshot, after: &Snapshot) -> Vec<String> {
        let mut f = Findings::default();
        let change = Change {
            initial: false,
            commit: "c",
            at: "c",
            kind,
            declared: &["b7k2m9qd".into()],
            reviewed: false,
        };
        modes(&change, before, after, &mut f);
        assert!(f
            .items
            .iter()
            .all(|(_, d)| d.rule == "CHG-02" && d.is_error()));
        f.items.into_iter().map(|(_, d)| d.message).collect()
    }

    /// CHG-02: only a canonical data file that the commit adds or changes must
    /// be `100644`; one left untouched with a wrong mode is not reported again.
    #[test]
    fn file_modes_are_checked_for_added_or_changed_canonical_files_only() {
        let package = "versions/b7k2m9qd/main.md";
        let files = |paths: &[(&str, u8)]| {
            paths
                .iter()
                .map(|(p, b)| ((*p).to_owned(), [*b; 32]))
                .collect::<BTreeMap<_, _>>()
        };
        let snapshot = |paths: &[(&str, u8)], executable: &[&str]| Snapshot {
            files: files(paths),
            executable: executable.iter().map(|p| (*p).to_owned()).collect(),
            ..Snapshot::default()
        };
        let report = |path: &str| format!("canonical data file {path} has mode 100755, not 100644");
        // A commit without a parent: every canonical file is added.
        let first = snapshot(
            &[(package, 1), ("history.yaml", 1), ("tools/gen.sh", 1)],
            &[package, "history.yaml", "tools/gen.sh"],
        );
        assert_eq!(
            modes_findings("revisionAdd", &Snapshot::default(), &first),
            [report("history.yaml"), report(package)]
        );
        // Untouched, wrong already: no finding.
        assert!(modes_findings("metadata", &first, &first).is_empty());
        // Bytes changed, the mode kept at 100755: reported for the change.
        let edited = snapshot(
            &[(package, 2), ("history.yaml", 1)],
            &[package, "history.yaml"],
        );
        assert_eq!(modes_findings("format", &first, &edited), [report(package)]);
        // Made executable, inside the range of the kind.
        let regular = snapshot(&[(package, 1)], &[]);
        let executable = snapshot(&[(package, 1)], &[package]);
        assert_eq!(
            modes_findings("format", &regular, &executable),
            [report(package)]
        );
        // Made executable outside the range: both the range and the mode are
        // CHG-02 and both are reported, as the specification names each.
        let mut f = inspect("metadata", &regular, &executable);
        assert_eq!(f.items.len(), 1);
        let mode = modes_findings("metadata", &regular, &executable);
        f.items
            .retain(|(_, d)| d.message == format!("metadata cannot change {package}"));
        assert_eq!((f.items.len(), mode), (1, vec![report(package)]));
        // Back to 100644, and a file outside the canonical data.
        assert!(modes_findings("format", &executable, &regular).is_empty());
        let tool = snapshot(&[("tools/gen.sh", 1)], &["tools/gen.sh"]);
        assert!(modes_findings("metadata", &Snapshot::default(), &tool).is_empty());
    }

    /// A file directly under `versions/` belongs to no version package but is
    /// canonical data (CHG-01): only `metadata` may change it.
    #[test]
    fn file_directly_under_versions_is_canonical_non_package_data() {
        let before = Snapshot::default();
        let mut after = Snapshot::default();
        after.files.insert("versions/README.md".into(), [1; 32]);
        assert!(!inspect("metadata", &before, &after).failed(CH));
        for kind in ["format", "correction", "restoration", "revisionAdd"] {
            assert!(inspect(kind, &before, &after).failed(CH), "{kind}");
        }
        assert!(auxiliary_findings(&before, &after).failed(CH));
    }

    #[test]
    fn untrusted_scalar_fields_do_not_panic() {
        for value in [json!(false), json!(1), json!("x"), json!(null)] {
            let before = snapshot(json!({"revisions":[{"id":"b7k2m9qd","text":value}]}));
            let after = snapshot(json!({"revisions":[{"id":"b7k2m9qd","text":{}}]}));
            assert!(inspect("correction", &before, &after).failed(CH));
        }
    }
}
