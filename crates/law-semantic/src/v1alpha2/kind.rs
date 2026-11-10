//! Node kinds of the loj/v1alpha2 AST (AST-01).

/// The kind of a node. The names in JSON come from [`Kind::as_str`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Kind {
    Law,
    MainProvision,
    SupplementaryProvision,
    AppendedMaterial,
    Part,
    Chapter,
    Section,
    Subsection,
    Division,
    Article,
    Paragraph,
    Item,
    Subitem,
    Table,
    Row,
    Cell,
    Preamble,
    EnactStatement,
}

impl Kind {
    /// The `kind` value in the AST.
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Kind::Law => "Law",
            Kind::MainProvision => "MainProvision",
            Kind::SupplementaryProvision => "SupplementaryProvision",
            Kind::AppendedMaterial => "AppendedMaterial",
            Kind::Part => "Part",
            Kind::Chapter => "Chapter",
            Kind::Section => "Section",
            Kind::Subsection => "Subsection",
            Kind::Division => "Division",
            Kind::Article => "Article",
            Kind::Paragraph => "Paragraph",
            Kind::Item => "Item",
            Kind::Subitem => "Subitem",
            Kind::Table => "Table",
            Kind::Row => "Row",
            Kind::Cell => "Cell",
            Kind::Preamble => "Preamble",
            Kind::EnactStatement => "EnactStatement",
        }
    }

    /// The kind that a reserved marker name opens or closes (DIR-01).
    pub(super) fn from_marker(name: &str) -> Option<Kind> {
        Some(match name {
            "paragraph" => Kind::Paragraph,
            "item" => Kind::Item,
            "subitem" => Kind::Subitem,
            "table" => Kind::Table,
            "row" => Kind::Row,
            "cell" => Kind::Cell,
            "preamble" => Kind::Preamble,
            "enact-statement" => Kind::EnactStatement,
            _ => return None,
        })
    }

    /// The segment that names this kind in a logical ID (ID-01).
    pub(super) fn id_segment(self) -> String {
        self.as_str().to_lowercase()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_the_kinds_of_the_ast_schema() {
        let schema: serde_json::Value = serde_json::from_str(super::super::AST_SCHEMA).unwrap();
        let text = schema.to_string();
        for kind in [
            Kind::Law,
            Kind::MainProvision,
            Kind::SupplementaryProvision,
            Kind::AppendedMaterial,
            Kind::Part,
            Kind::Chapter,
            Kind::Section,
            Kind::Subsection,
            Kind::Division,
            Kind::Article,
            Kind::Paragraph,
            Kind::Item,
            Kind::Subitem,
            Kind::Table,
            Kind::Row,
            Kind::Cell,
            Kind::Preamble,
            Kind::EnactStatement,
        ] {
            let name = kind.as_str();
            assert!(text.contains(&format!("\"{name}\"")), "{name}");
        }
    }

    #[test]
    fn marker_names_and_id_segments() {
        assert_eq!(
            Kind::from_marker("enact-statement"),
            Some(Kind::EnactStatement)
        );
        assert_eq!(Kind::from_marker("Paragraph"), None);
        assert_eq!(Kind::from_marker("article"), None);
        assert_eq!(Kind::Subsection.id_segment(), "subsection");
        assert_eq!(Kind::Paragraph.id_segment(), "paragraph");
    }
}
