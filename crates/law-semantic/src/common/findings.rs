//! Diagnostics collected per check, and the mapping of JSON Schema errors to rules.
use super::{Check, Class, Diagnostic, Report, ResultValue};
use std::collections::HashSet;

/// Diagnostics tagged with the check (L3/L4: category) they count against.
#[derive(Default, Debug)]
pub struct Findings {
    pub items: Vec<(Check, Diagnostic)>,
    /// Checks that could not be completed (VAL-04).
    pub incomplete: HashSet<Check>,
    /// Checks that had something to examine.
    pub applicable: HashSet<Check>,
}

impl Findings {
    pub fn push(&mut self, check: Check, d: Diagnostic) {
        self.items.push((check, d));
    }
    pub fn error(
        &mut self,
        check: Check,
        rule: &str,
        sv: &str,
        message: impl Into<String>,
        path: &str,
        pointer: &str,
    ) {
        self.push(
            check,
            Diagnostic::error(rule, sv, message).at_pointer(path, pointer),
        );
    }
    #[allow(clippy::too_many_arguments)]
    pub fn note(
        &mut self,
        check: Check,
        class: Class,
        rule: &str,
        sv: &str,
        message: impl Into<String>,
        path: &str,
        pointer: &str,
    ) {
        self.push(
            check,
            Diagnostic::error(rule, sv, message)
                .with_class(class)
                .at_pointer(path, pointer),
        );
    }
    #[must_use]
    pub fn failed(&self, check: Check) -> bool {
        self.items.iter().any(|(c, d)| *c == check && d.is_error())
    }
    /// fail > incomplete > pass; notApplicable when nothing was examined.
    #[must_use]
    pub fn result(&self, check: Check) -> ResultValue {
        if self.failed(check) {
            ResultValue::Fail
        } else if self.incomplete.contains(&check) {
            ResultValue::Incomplete
        } else if self.applicable.contains(&check) {
            ResultValue::Pass
        } else {
            ResultValue::NotApplicable
        }
    }
    #[must_use]
    pub fn into_report(self, checks: &[Check]) -> Report {
        let mut r = Report::default();
        for c in checks {
            r.set(*c, self.result(*c));
        }
        r.diagnostics = self.items.into_iter().map(|(_, d)| d).collect();
        r
    }
    /// Report every error of `validator` under the rule its schema location names.
    pub fn schema(
        &mut self,
        validator: &jsonschema::Validator,
        doc: &serde_json::Value,
        path: &str,
        classify: impl Fn(&str, &str) -> (Check, &'static str, &'static str),
    ) {
        for e in validator.iter_errors(doc) {
            let schema_path = e.schema_path().to_string();
            let instance = e.instance_path().to_string();
            let (check, rule, sv) = classify(&schema_path, &instance);
            self.error(check, rule, sv, e.to_string(), path, &instance);
        }
    }
}

/// Rules of the common types (`types.schema.json#/$defs/...`), if the error lies in one.
#[must_use]
pub fn common_type_rule(schema_path: &str) -> Option<(Check, &'static str)> {
    let def = schema_path
        .rsplit_once("/$defs/")
        .map(|(_, d)| d.split('/').next().unwrap_or(""))?;
    Some(match def {
        "claimRule" => (Check::EVIDENCE, "TYPE-05"),
        "TimePoint" | "Calc" | "Delegation" | "DeterminedBy" | "SameAs" | "date" => {
            (Check::HISTORY_STRUCTURE, "TIME-01")
        }
        "RecordRef" | "recordId" | "spaceId" => (Check::EVIDENCE, "TYPE-01"),
        "Source" => (Check::EVIDENCE, "TYPE-02"),
        "Evidence" | "evidenceList" => (Check::EVIDENCE, "TYPE-03"),
        "Verification" => (Check::EVIDENCE, "TYPE-04"),
        "Scope" => (Check::EVIDENCE, "TYPE-06"),
        _ => return None,
    })
}
