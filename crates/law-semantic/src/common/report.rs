//! The check report of `schemas/common/v1alpha1/report.schema.json` (VAL-04, VAL-07).
use serde_json::{json, Map, Value};

/// A check (`L0` … `reference`), with the category for L3 and L4.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Check {
    pub check: &'static str,
    pub category: Option<&'static str>,
}

impl Check {
    pub const L0: Check = Check::plain("L0");
    pub const L1: Check = Check::plain("L1");
    pub const REGISTRY: Check = Check::plain("registry");
    pub const REBUILD: Check = Check::plain("rebuild");
    pub const CHANGE: Check = Check::plain("change");
    pub const REFERENCE: Check = Check::plain("reference");
    pub const HISTORY_STRUCTURE: Check = Check::l3("historyStructure");
    pub const EVIDENCE: Check = Check::l3("evidence");
    pub const RELATION_RESOLUTION: Check = Check::l4("relationResolution");
    pub const RELATION_CONSISTENCY: Check = Check::l4("relationConsistency");
    pub const TIME_CONSISTENCY: Check = Check::l4("timeConsistency");

    pub const fn plain(check: &'static str) -> Check {
        Check {
            check,
            category: None,
        }
    }
    pub const fn l3(category: &'static str) -> Check {
        Check {
            check: "L3",
            category: Some(category),
        }
    }
    pub const fn l4(category: &'static str) -> Check {
        Check {
            check: "L4",
            category: Some(category),
        }
    }
    /// Parse `L1`, `L3`, `L3/evidence`, `registry`, … Reserved checks are refused.
    pub fn parse(s: &str) -> Result<Vec<Check>, String> {
        let (check, category) = match s.split_once('/') {
            Some((c, k)) => (c, Some(k)),
            None => (s, None),
        };
        let all: &[Check] = &[
            Check::L0,
            Check::L1,
            Check::HISTORY_STRUCTURE,
            Check::EVIDENCE,
            Check::RELATION_RESOLUTION,
            Check::RELATION_CONSISTENCY,
            Check::TIME_CONSISTENCY,
            Check::REGISTRY,
            Check::REBUILD,
            Check::CHANGE,
            Check::REFERENCE,
        ];
        let found: Vec<Check> = all
            .iter()
            .copied()
            .filter(|c| c.check == check && category.is_none_or(|k| c.category == Some(k)))
            .collect();
        if found.is_empty() {
            return Err(match (check, category) {
                ("L2", _) | ("L3", Some("sourceMatch")) | ("L4", Some("clauseReference")) => {
                    format!("{s} is reserved and not implemented")
                }
                _ => format!("unknown check: {s}"),
            });
        }
        Ok(found)
    }
    pub fn label(&self) -> String {
        match self.category {
            Some(c) => format!("{}/{}", self.check, c),
            None => self.check.to_owned(),
        }
    }
}

/// VAL-04 result values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResultValue {
    Pass,
    Fail,
    NotApplicable,
    Incomplete,
}

impl ResultValue {
    pub fn as_str(self) -> &'static str {
        match self {
            ResultValue::Pass => "pass",
            ResultValue::Fail => "fail",
            ResultValue::NotApplicable => "notApplicable",
            ResultValue::Incomplete => "incomplete",
        }
    }
    /// Combine two results of the same check: fail > incomplete > pass > notApplicable.
    pub fn worst(self, other: ResultValue) -> ResultValue {
        use ResultValue::*;
        let rank = |r| match r {
            Fail => 3,
            Incomplete => 2,
            Pass => 1,
            NotApplicable => 0,
        };
        if rank(other) > rank(self) {
            other
        } else {
            self
        }
    }
}

/// One diagnostic. `rule` + `schemaVersion` identify the normative rule (VAL-07);
/// `code`, `message` and `location` are implementation specific.
#[derive(Clone, Debug, PartialEq)]
pub struct Diagnostic {
    pub rule: String,
    pub schema_version: String,
    pub class: &'static str,
    pub code: String,
    pub message: String,
    pub location: Option<Value>,
}

impl Diagnostic {
    pub fn error(rule: &str, schema_version: &str, message: impl Into<String>) -> Self {
        Diagnostic {
            rule: rule.into(),
            schema_version: schema_version.into(),
            class: "error",
            code: format!("{schema_version}/{}", rule.to_ascii_lowercase()),
            message: message.into(),
            location: None,
        }
    }
    pub fn with_class(mut self, class: &'static str) -> Self {
        self.class = class;
        self
    }
    pub fn with_code(mut self, code: &str) -> Self {
        self.code = code.into();
        self
    }
    /// A file location; `start`/`end` are UTF-8 byte offsets when known.
    pub fn at(mut self, path: &str, span: Option<(usize, usize)>) -> Self {
        let mut l = json!({ "path": path });
        if let Some((s, e)) = span {
            l["start"] = json!(s);
            l["end"] = json!(e);
        }
        self.location = Some(l);
        self
    }
    /// A location inside a YAML document, by JSON pointer.
    pub fn at_pointer(mut self, path: &str, pointer: &str) -> Self {
        self.location = Some(json!({ "path": path, "pointer": pointer }));
        self
    }
    pub fn is_error(&self) -> bool {
        self.class == "error"
    }
    pub fn to_json(&self) -> Value {
        let mut v = json!({
            "rule": self.rule,
            "schemaVersion": self.schema_version,
            "class": self.class,
            "code": self.code,
            "message": self.message,
        });
        if let Some(l) = &self.location {
            v["location"] = l.clone();
        }
        v
    }
}

/// A check report.
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub target: Map<String, Value>,
    pub results: Vec<(Check, ResultValue)>,
    pub diagnostics: Vec<Diagnostic>,
    pub references: Vec<Value>,
    pub resolution: Option<Value>,
}

impl Report {
    pub fn set(&mut self, check: Check, result: ResultValue) {
        if let Some(r) = self.results.iter_mut().find(|(c, _)| *c == check) {
            r.1 = result;
        } else {
            self.results.push((check, result));
        }
    }
    pub fn result(&self, check: Check) -> Option<ResultValue> {
        self.results
            .iter()
            .find(|(c, _)| *c == check)
            .map(|(_, r)| *r)
    }
    /// Keep only the selected checks (diagnostics are kept: they explain results).
    pub fn select(&mut self, selected: &[Check]) {
        self.results.retain(|(c, _)| selected.contains(c));
    }
    pub fn merge(&mut self, other: Report) {
        for (c, r) in other.results {
            let r = self.result(c).map_or(r, |old| old.worst(r));
            self.set(c, r);
        }
        self.diagnostics.extend(other.diagnostics);
        self.references.extend(other.references);
        if other.resolution.is_some() {
            self.resolution = other.resolution;
        }
        for (k, v) in other.target {
            self.target.entry(k).or_insert(v);
        }
    }
    pub fn to_json(&self) -> Value {
        let mut v = json!({
            "results": self.results.iter().map(|(c, r)| {
                let mut o = json!({"check": c.check, "result": r.as_str()});
                if let Some(k) = c.category { o["category"] = json!(k); }
                o
            }).collect::<Vec<_>>(),
            "diagnostics": self.diagnostics.iter().map(Diagnostic::to_json).collect::<Vec<_>>(),
        });
        if !self.target.is_empty() {
            v["target"] = Value::Object(self.target.clone());
        }
        if !self.references.is_empty() {
            v["references"] = json!(self.references);
        }
        if let Some(r) = &self.resolution {
            v["resolution"] = r.clone();
        }
        v
    }
    /// Process exit code: 1 any fail; 3 any incomplete or nothing passed
    /// (notApplicable alone is not a pass, VAL-04); otherwise 0.
    pub fn exit_code(&self) -> i32 {
        if self.results.iter().any(|(_, r)| *r == ResultValue::Fail) {
            1
        } else if !self.results.iter().any(|(_, r)| *r == ResultValue::Pass)
            || self
                .results
                .iter()
                .any(|(_, r)| *r == ResultValue::Incomplete)
        {
            3
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn report_matches_schema() {
        let mut r = Report::default();
        r.set(Check::L1, ResultValue::Pass);
        r.set(Check::EVIDENCE, ResultValue::Incomplete);
        r.diagnostics
            .push(Diagnostic::error("HEAD-01", "loj/v1alpha2", "x").at("main.md", Some((0, 3))));
        let schema =
            include_str!("../../../../vendor/loj-spec/schemas/common/v1alpha1/report.schema.json");
        let v = super::super::validator(schema, &[]);
        assert!(v.is_valid(&r.to_json()), "{}", r.to_json());
        assert_eq!(r.exit_code(), 3);
        assert!(Check::parse("L2").is_err());
        assert!(Check::parse("L3/sourceMatch").is_err());
        assert_eq!(Check::parse("L3").unwrap().len(), 2);
    }
}
