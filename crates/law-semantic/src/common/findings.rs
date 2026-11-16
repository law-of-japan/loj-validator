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
    /// VAL-04: retain the error explaining an unavailable input without
    /// counting it as a violation of that input.
    pub fn unprocessable(&mut self, check: Check, diagnostic: Diagnostic) {
        self.incomplete.insert(check);
        let code = format!("{}/unprocessable", diagnostic.schema_version);
        self.push(check, diagnostic.with_code(&code));
    }
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
        self.items.iter().any(|(c, d)| {
            *c == check
                && d.is_error()
                && !d.code.ends_with("/unprocessable")
                && !d.code.ends_with("/unsupported-version")
        })
    }
    /// fail > incomplete > pass; notApplicable when nothing was examined.
    #[must_use]
    pub fn result(&self, check: Check) -> ResultValue {
        if self.failed(check) {
            ResultValue::Fail
        } else if self.incomplete.contains(&check)
            || self.items.iter().any(|(c, d)| {
                *c == check
                    && d.class == Class::Undetermined
                    && !(d.schema_version == "loj-history/v1alpha1"
                        && matches!(d.rule.as_str(), "HIST-06" | "HIST-12"))
            })
        {
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
            let (check, mut rule, mut sv) = classify(&schema_path, &instance);
            if instance.ends_with("/fixed") {
                if let Some(fixed) = doc.pointer(&instance).filter(|v| v.is_object()) {
                    let revision = fixed["revision"].as_str();
                    rule = if revision.is_none_or(|r| {
                        r.len() != 8
                            || !r
                                .bytes()
                                .all(|b| b"0123456789abcdefghjkmnpqrstvwxyz".contains(&b))
                    }) {
                        "FIXREF-03"
                    } else if fixed
                        .get("commit")
                        .is_some_and(|c| c.as_str().is_none_or(|c| !crate::fixref::full_hash(c)))
                    {
                        "FIXREF-04"
                    } else {
                        "FIXREF-01"
                    };
                    sv = super::COMMON;
                }
            }
            if instance.ends_with("/space") {
                rule = "RID-04";
                sv = "loj-registry/v1alpha1";
            }
            self.error(check, rule, sv, e.to_string(), path, &instance);
        }
    }
}

/// Rules of the common types (`types.schema.json#/$defs/...`), if the error lies in one.
#[must_use]
pub fn common_type_rule(schema_path: &str) -> Option<(Check, &'static str)> {
    if schema_path.contains("/properties/fixed/") {
        if schema_path.contains("/properties/commit/") {
            return Some((Check::EVIDENCE, "FIXREF-04"));
        }
        if schema_path.contains("/properties/revision/") {
            return Some((Check::EVIDENCE, "FIXREF-03"));
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aggregation_is_per_check_with_warning_only_exceptions() {
        for check in [
            Check::HISTORY_STRUCTURE,
            Check::EVIDENCE,
            Check::CHANGE,
            Check::REGISTRY,
            Check::REBUILD,
            Check::REFERENCE,
        ] {
            let mut f = Findings::default();
            f.applicable.insert(check);
            f.note(
                check,
                Class::Pending,
                "TIME-01",
                super::super::COMMON,
                "pending",
                "",
                "",
            );
            assert_eq!(f.result(check), ResultValue::Pass);
            f.note(
                check,
                Class::Undetermined,
                "HIST-12",
                "loj-history/v1alpha1",
                "order",
                "",
                "",
            );
            assert_eq!(f.result(check), ResultValue::Pass);
            f.note(
                check,
                Class::Undetermined,
                "TYPE-07",
                super::super::COMMON,
                "missing AST",
                "",
                "",
            );
            assert_eq!(f.result(check), ResultValue::Incomplete);
            f.error(
                check,
                "TYPE-03",
                super::super::COMMON,
                "missing source",
                "",
                "",
            );
            assert_eq!(f.result(check), ResultValue::Fail);
            assert_eq!(f.result(Check::L0), ResultValue::NotApplicable);
        }
        let mut f = Findings::default();
        f.unprocessable(
            Check::REGISTRY,
            Diagnostic::error("REG-01", "loj-registry/v1alpha1", "unreadable"),
        );
        assert_eq!(f.result(Check::REGISTRY), ResultValue::Incomplete);
    }
}
