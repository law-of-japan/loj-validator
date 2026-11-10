use super::directives::{marker, Marker};
use super::Context;
use crate::common::{
    markdown::{bare, incomplete_split_blocks, line_end, plain_heading},
    number::{number, Digits},
    structure::{tail_ok, tokens, Token, ARTICLE_RANK, ATX},
};
use regex::Regex;
use serde_json::{json, Value};
use std::{collections::HashSet, ops::Range, sync::LazyLock};

/// loj/v1alpha1 accepts only `の` between the parts of a branch number.
const SEPARATORS: &[char] = &['の'];

/// A structural heading label: number, kind, branch numbers, rest.
static HEADING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^第([^編章節款目条]+)([編章節款目条])((?:の[^の 　（）]+)*)(.*)$")
        .expect("constant regex")
});
#[derive(Clone)]
enum Content {
    Text(String, Vec<Value>),
    Node(usize),
}
struct Node {
    kind: String,
    id: String,
    number: Option<String>,
    display: String,
    level: usize,
    deleted: bool,
    rank: usize,
    content: Vec<Content>,
    path: String,
    span: Range<usize>,
}
impl Node {
    fn new(
        kind: &str,
        id: String,
        number: Option<String>,
        display: String,
        path: &str,
        span: Range<usize>,
    ) -> Self {
        Self {
            kind: kind.into(),
            id,
            number,
            display,
            level: 0,
            deleted: false,
            rank: 0,
            content: Vec::new(),
            path: path.into(),
            span,
        }
    }
}
fn export(nodes: &[Node], index: usize) -> Value {
    let n = &nodes[index];
    let content: Vec<_> = n
        .content
        .iter()
        .map(|c| match c {
            Content::Text(t, _) => json!({"text":t}),
            Content::Node(i) => export(nodes, *i),
        })
        .collect();
    let mut v =
        json!({"kind":n.kind,"id":n.id,"number":n.number,"display":n.display,"content":content});
    if n.kind == "Article" {
        v["status"] = json!(if n.deleted { "deleted" } else { "active" });
    }
    if n.kind == "Subitem" {
        v["level"] = json!(n.level);
    }
    v
}
fn add(
    nodes: &mut Vec<Node>,
    parent: usize,
    node: Node,
    ctx: &mut Context,
    ids: &mut HashSet<String>,
) -> usize {
    if !ids.insert(node.id.clone()) {
        let rule = if node.rank > 0 { "HEAD-02" } else { "DIR-02" };
        ctx.diag(
            rule,
            "Duplicate scoped number",
            &node.path,
            node.span.start,
            node.span.end,
        );
        ctx.diag(
            "AST-02",
            "Duplicate logical ID",
            &node.path,
            node.span.start,
            node.span.end,
        );
    }
    let i = nodes.len();
    nodes.push(node);
    nodes[parent].content.push(Content::Node(i));
    i
}
fn text(nodes: &mut [Node], index: usize, source: &str, path: &str, span: Range<usize>) {
    if span.is_empty() {
        return;
    }
    let position = json!({"path":path,"start":span.start,"end":span.end});
    if let Some(Content::Text(t, p)) = nodes[index].content.last_mut() {
        t.push_str(&source[span]);
        p.push(position);
    } else {
        nodes[index]
            .content
            .push(Content::Text(source[span].into(), vec![position]));
    }
}

fn display(nodes: &mut [Node], index: usize, ctx: &mut Context) {
    let n = &mut nodes[index];
    let first = n.content.iter().find_map(|c| {
        if let Content::Text(t, p) = c {
            (!t.trim().is_empty()).then_some((t, p))
        } else {
            None
        }
    });
    let Some((t, positions)) = first else {
        ctx.diag(
            "DIR-02",
            "Container has no own non-whitespace text",
            &n.path,
            n.span.start,
            n.span.end,
        );
        return;
    };
    let mut offset = 0;
    let mut first_line = "";
    let mut first_line_bytes = 0;
    for line in t.split_inclusive('\n') {
        if !line.trim().is_empty() {
            first_line = bare(line);
            first_line_bytes = line.len();
            break;
        }
        offset += line.len();
    }
    let token = first_line.split([' ', '　']).next().unwrap_or("");
    let digits = match n.kind.as_str() {
        "Paragraph" => Digits::Arabic,
        "Subitem" => Digits::Ascii,
        _ => Digits::Any,
    };
    let parsed = number(token, SEPARATORS, digits);
    let omitted = n.kind == "Paragraph" && n.number.as_deref() == Some("1") && parsed.is_none();
    if !omitted {
        n.display = token.into();
    }
    if !omitted
        && (parsed != n.number
            || first_line
                .get(token.len()..)
                .is_none_or(|r| r.trim().is_empty()))
    {
        // Locate this line even when own text has multiple source fragments.
        // The positions are the `{path, start, end}` objects that `text` built.
        let at = |p: &Value, key: &str| {
            p[key]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .expect("a byte offset written by `text`")
        };
        let mut relative = offset;
        let mut pos = &positions[0];
        for p in positions {
            let len = at(p, "end") - at(p, "start");
            pos = p;
            if relative < len {
                break;
            }
            relative -= len;
        }
        let start = at(pos, "start") + relative;
        let end = (start + first_line_bytes).min(at(pos, "end"));
        ctx.diag(
            "DISP-01",
            "Display number missing or inconsistent",
            pos["path"].as_str().expect("a path written by `text`"),
            start,
            end,
        );
    }
}

/// DIR-02: whether the container that `m` opens may be a child of `p`.
fn valid_parent(m: &Marker, p: &Node) -> bool {
    match m.kind {
        "Paragraph" => p.kind == "Article" || p.kind == "SupplementaryProvision",
        "Item" => p.kind == "Paragraph",
        _ => {
            if m.level == 1 {
                p.kind == "Item"
            } else {
                p.kind == "Subitem" && p.level + 1 == m.level
            }
        }
    }
}

/// The document being read and the state that ends with it.
struct Document<'a> {
    path: &'a str,
    source: &'a str,
    /// Node of the scope (main, a supplement or an appendix) the file belongs to.
    scope: usize,
    /// This is the first file of its scope.
    first: bool,
    appendix: bool,
    supplementary: bool,
    /// The scope heading (`附則`, or the appendix title) has been read.
    special_heading: bool,
    /// The article that following markers belong to.
    article: Option<usize>,
    /// Open containers, innermost last.
    stack: Vec<usize>,
    /// End of the last structural line that was handled.
    cursor: usize,
}

/// MAN-03: how a later file of a scope begins, and how a nonfinal appendix
/// file ends. `continued` says that the next file belongs to the same scope.
fn file_boundaries(doc: &Document, tokens: &[Token], continued: bool, ctx: &mut Context) {
    let (path, source) = (doc.path, doc.source);
    if !doc.first && !doc.appendix {
        let starts_with_structure = tokens.first().is_some_and(|t| {
            t.heading
                && source[..t.span.start].trim().is_empty()
                && ATX
                    .captures(bare(&source[t.span.clone()]))
                    .is_some_and(|c| HEADING.is_match(&c[1]))
        });
        if !starts_with_structure {
            ctx.diag(
                "MAN-03",
                "Subsequent file must start with an article or classification heading",
                path,
                0,
                line_end(source, 0),
            );
        }
    }
    if doc.appendix {
        if !doc.first && source.trim().is_empty() {
            ctx.diag(
                "MAN-03",
                "Subsequent appendix file must contain nonblank text",
                path,
                0,
                source.len(),
            );
        }
        if continued {
            for span in incomplete_split_blocks(source) {
                ctx.diag(
                    "MAN-03",
                    "Nonfinal appendix block requires an explicit terminator",
                    path,
                    span.start,
                    span.end,
                );
            }
        }
    }
}

/// The tree under construction and the state that spans documents.
struct Builder<'a> {
    law: &'a str,
    nodes: Vec<Node>,
    ids: HashSet<String>,
    /// Scope name (`main`, `supplement/…`, `appendix/…`) to its node.
    scopes: std::collections::HashMap<String, usize>,
    /// Open classification headings (Part to Division) of the current scope.
    groups: Vec<usize>,
    /// Scope name of the previous document.
    previous: String,
    visited: HashSet<String>,
}

impl<'a> Builder<'a> {
    /// The law node and one node per scope of the manifest.
    fn new(manifest: &'a Value, ctx: &mut Context) -> Self {
        let law = manifest["law"]["id"]
            .as_str()
            .expect("a string by the manifest schema");
        let mut nodes = vec![Node::new(
            "Law",
            law.into(),
            None,
            manifest["law"]["title"]
                .as_str()
                .expect("a string by the manifest schema")
                .into(),
            "",
            0..0,
        )];
        let mut ids = HashSet::from([law.to_owned()]);
        let mut scopes = std::collections::HashMap::new();
        let main = add(
            &mut nodes,
            0,
            Node::new(
                "MainProvision",
                format!("{law}/main"),
                None,
                String::new(),
                "",
                0..0,
            ),
            ctx,
            &mut ids,
        );
        scopes.insert("main".to_owned(), main);
        for (field, prefix, kind) in [
            ("supplements", "supplement", "SupplementaryProvision"),
            ("appendices", "appendix", "AppendedMaterial"),
        ] {
            if let Some(a) = manifest[field].as_array() {
                for s in a {
                    let scope = format!(
                        "{prefix}/{}",
                        s["key"].as_str().expect("a string by the manifest schema")
                    );
                    let i = add(
                        &mut nodes,
                        0,
                        Node::new(
                            kind,
                            format!("{law}/{scope}"),
                            None,
                            if prefix == "supplement" {
                                "附則".into()
                            } else {
                                s["title"]
                                    .as_str()
                                    .expect("a string by the manifest schema")
                                    .into()
                            },
                            "",
                            0..0,
                        ),
                        ctx,
                        &mut ids,
                    );
                    scopes.insert(scope, i);
                }
            }
        }
        Builder {
            law,
            nodes,
            ids,
            scopes,
            groups: Vec::new(),
            previous: String::new(),
            visited: HashSet::new(),
        }
    }

    /// Read document `index` into the tree.
    fn read(&mut self, documents: &[(String, String, String)], index: usize, ctx: &mut Context) {
        let (path, source, scope_name) = &documents[index];
        let Some(&scope) = self.scopes.get(scope_name) else {
            return;
        };
        if self.previous != *scope_name {
            self.groups.clear();
        }
        self.previous = scope_name.clone();
        let first = self.visited.insert(scope_name.clone());
        let appendix = self.nodes[scope].kind == "AppendedMaterial";
        let mut doc = Document {
            path,
            source,
            scope,
            first,
            appendix,
            supplementary: self.nodes[scope].kind == "SupplementaryProvision",
            special_heading: appendix && !first,
            article: None,
            stack: Vec::new(),
            cursor: 0,
        };
        let tokens = tokens(source);
        let continued = documents
            .get(index + 1)
            .is_some_and(|(_, _, next)| next == scope_name);
        file_boundaries(&doc, &tokens, continued, ctx);
        for token in &tokens {
            let span = &token.span;
            if self.nodes.len() >= 20_000 || span.len() > 4096 {
                ctx.incomplete = true;
                ctx.diag(
                    "AST-01",
                    "Implementation limit: 20000 nodes or 4096-byte structural token",
                    path,
                    span.start,
                    span.end,
                );
                break;
            }
            if span.start < doc.cursor {
                continue;
            }
            let before = doc.cursor..span.start;
            if let Some(&i) = doc.stack.last() {
                text(&mut self.nodes, i, source, path, before);
            } else if appendix && doc.special_heading {
                text(&mut self.nodes, scope, source, path, before);
            } else {
                outside(&mut self.nodes, doc.article, source, path, before, ctx);
            }
            if token.heading {
                self.heading(&mut doc, token, ctx);
            } else {
                self.reserved_marker(&mut doc, span, ctx);
            }
            doc.cursor = span.end;
        }
        self.end_of_file(doc, ctx);
    }

    /// The text after the last structural line, and what the file left open.
    fn end_of_file(&mut self, doc: Document, ctx: &mut Context) {
        let (path, source) = (doc.path, doc.source);
        let rest = doc.cursor..source.len();
        if let Some(&i) = doc.stack.last() {
            text(&mut self.nodes, i, source, path, rest);
        } else if doc.appendix {
            text(&mut self.nodes, doc.scope, source, path, rest);
        } else {
            outside(&mut self.nodes, doc.article, source, path, rest, ctx);
        }
        for i in doc.stack {
            ctx.diag(
                "DIR-02",
                "Unclosed marker at file boundary",
                path,
                self.nodes[i].span.start,
                self.nodes[i].span.end,
            );
        }
        if doc.first && (doc.supplementary || doc.appendix) && !doc.special_heading {
            ctx.diag(
                "HEAD-03",
                "Required initial scope heading missing",
                path,
                0,
                line_end(source, 0),
            );
        }
    }

    /// A heading line (HEAD-01, HEAD-03, MD-03).
    fn heading(&mut self, doc: &mut Document, token: &Token, ctx: &mut Context) {
        let (path, source, span) = (doc.path, doc.source, &token.span);
        if doc.appendix && (!doc.first || doc.special_heading) {
            // After its title, a heading in an appendix is plain content.
            text(&mut self.nodes, doc.scope, source, path, span.clone());
            return;
        }
        if !doc.stack.is_empty() {
            ctx.diag(
                "MD-03",
                "Heading inside explicit container",
                path,
                span.start,
                span.end,
            );
            for i in doc.stack.drain(..) {
                ctx.diag(
                    "DIR-02",
                    "Unclosed before heading",
                    path,
                    self.nodes[i].span.start,
                    self.nodes[i].span.end,
                );
            }
        }
        let raw = bare(&source[span.clone()]);
        let label = ATX
            .captures(raw)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str())
            .filter(|l| token.plain && !l.ends_with(char::is_whitespace) && plain_heading(raw, l));
        let Some(label) = label else {
            ctx.diag(
                "HEAD-01",
                "Expected strict plain ATX heading",
                path,
                span.start,
                span.end,
            );
            return;
        };
        if (doc.supplementary && label == "附則") || doc.appendix {
            self.scope_heading(doc, label, span, ctx);
        } else if label == "附則" {
            ctx.diag(
                "HEAD-03",
                "Supplement heading cannot change scope",
                path,
                span.start,
                span.end,
            );
        } else {
            self.structural_heading(doc, label, span, ctx);
        }
    }

    /// HEAD-03: the heading that opens a supplement or an appendix.
    fn scope_heading(
        &mut self,
        doc: &mut Document,
        label: &str,
        span: &Range<usize>,
        ctx: &mut Context,
    ) {
        let scope = &mut self.nodes[doc.scope];
        if !doc.first
            || doc.special_heading
            || span.start != 0
            || (doc.appendix && label != scope.display)
        {
            ctx.diag(
                "HEAD-03",
                "Invalid scope heading",
                doc.path,
                span.start,
                span.end,
            );
        }
        scope.path = doc.path.to_owned();
        scope.span = span.clone();
        doc.special_heading = true;
    }

    /// A classification or article heading (HEAD-01, NUM-01).
    fn structural_heading(
        &mut self,
        doc: &mut Document,
        label: &str,
        span: &Range<usize>,
        ctx: &mut Context,
    ) {
        let path = doc.path;
        let Some(c) = HEADING.captures(label) else {
            ctx.diag(
                "HEAD-01",
                "Unrecognized legal heading",
                path,
                span.start,
                span.end,
            );
            return;
        };
        let num = number(&format!("{}{}", &c[1], &c[3]), SEPARATORS, Digits::Any);
        let Some(num) = num else {
            ctx.diag(
                "NUM-01",
                "Invalid legal numeral",
                path,
                span.start,
                span.end,
            );
            return;
        };
        let (kind, rank) = match &c[2] {
            "編" => ("Part", 1),
            "章" => ("Chapter", 2),
            "節" => ("Section", 3),
            "款" => ("Subsection", 4),
            "目" => ("Division", 5),
            _ => ("Article", ARTICLE_RANK),
        };
        if !tail_ok(rank, &c[4]) {
            ctx.diag(
                "HEAD-01",
                "Invalid heading title or caption",
                path,
                span.start,
                span.end,
            );
            return;
        }
        if rank < ARTICLE_RANK {
            while self
                .groups
                .last()
                .is_some_and(|i| self.nodes[*i].rank >= rank)
            {
                self.groups.pop();
            }
        }
        let parent = self.groups.last().copied().unwrap_or(doc.scope);
        let id_parent = if rank == ARTICLE_RANK {
            doc.scope
        } else {
            parent
        };
        let mut node = Node::new(
            kind,
            format!(
                "{}/{}/{}",
                self.nodes[id_parent].id,
                kind.to_lowercase(),
                num
            ),
            Some(num),
            label.into(),
            path,
            span.clone(),
        );
        node.rank = rank;
        let index = add(&mut self.nodes, parent, node, ctx, &mut self.ids);
        if rank == ARTICLE_RANK {
            doc.article = Some(index);
        } else {
            self.groups.push(index);
            doc.article = None;
        }
    }

    /// A reserved marker line (DIR-01, DIR-02).
    fn reserved_marker(&mut self, doc: &mut Document, span: &Range<usize>, ctx: &mut Context) {
        let path = doc.path;
        if doc.appendix {
            ctx.diag(
                "HEAD-03",
                "Reserved marker in appendix",
                path,
                span.start,
                span.end,
            );
            return;
        }
        let Some(m) = marker(bare(&doc.source[span.clone()])) else {
            ctx.diag(
                "DIR-01",
                "Invalid reserved marker syntax or attributes",
                path,
                span.start,
                span.end,
            );
            return;
        };
        if m.close {
            self.closing_marker(doc, &m, span, ctx);
        } else {
            self.container(doc, m, span, ctx);
        }
    }

    /// DIR-02: a closing marker ends the innermost open container of its kind.
    fn closing_marker(
        &mut self,
        doc: &mut Document,
        m: &Marker,
        span: &Range<usize>,
        ctx: &mut Context,
    ) {
        let Some(&i) = doc.stack.last() else {
            ctx.diag(
                "DIR-02",
                "Unexpected closing marker",
                doc.path,
                span.start,
                span.end,
            );
            return;
        };
        if self.nodes[i].kind == m.kind {
            self.nodes[i].span.end = span.end;
            display(&mut self.nodes, i, ctx);
            doc.stack.pop();
        } else {
            ctx.diag(
                "DIR-02",
                "Mismatched closing marker",
                doc.path,
                span.start,
                span.end,
            );
        }
    }

    /// An opening marker of a paragraph, item or subitem.
    fn container(&mut self, doc: &mut Document, m: Marker, span: &Range<usize>, ctx: &mut Context) {
        let path = doc.path;
        let parent = doc
            .stack
            .last()
            .copied()
            .or(doc.article)
            .unwrap_or(doc.scope);
        let p = &self.nodes[parent];
        if !valid_parent(&m, p) {
            ctx.diag(
                "DIR-02",
                "Invalid legal parent or subitem level",
                path,
                span.start,
                span.end,
            );
            return;
        }
        if p.deleted {
            ctx.diag(
                "HEAD-04",
                "Deleted article cannot contain paragraphs",
                path,
                span.start,
                span.end,
            );
        }
        let suffix = if m.kind == "Subitem" {
            format!("subitem/{}/{}", m.level, m.number)
        } else {
            format!("{}/{}", m.kind.to_lowercase(), m.number)
        };
        let mut node = Node::new(
            m.kind,
            format!("{}/{suffix}", p.id),
            Some(m.number),
            String::new(),
            path,
            span.clone(),
        );
        node.level = m.level;
        let i = add(&mut self.nodes, parent, node, ctx, &mut self.ids);
        doc.stack.push(i);
    }

    /// Check the finished tree, record the positions and export the AST.
    fn finish(
        self,
        manifest: &Value,
        documents: &[(String, String, String)],
        ctx: &mut Context,
    ) -> Value {
        let nodes = &self.nodes;
        for (i, n) in nodes.iter().enumerate() {
            structure(nodes, i, documents, self.law, ctx);
            if !n.path.is_empty() {
                ctx.position(&n.id, &n.path, n.span.start, n.span.end);
            } else {
                ctx.source_map
                    .entry(n.id.clone())
                    .or_insert_with(|| json!([]));
            }
            for (j, c) in n.content.iter().enumerate() {
                if let Content::Text(t, positions) = c {
                    ctx.positions
                        .push(json!({"nodeId":n.id,"contentIndex":j,"text":t,"spans":positions}));
                }
            }
        }
        json!({"schemaVersion":"loj/v1alpha1","law":manifest["law"],"root":export(nodes,0)})
    }
}

pub(super) fn build(
    manifest: &Value,
    documents: &[(String, String, String)],
    ctx: &mut Context,
) -> Value {
    let mut builder = Builder::new(manifest, ctx);
    for index in 0..documents.len() {
        builder.read(documents, index, ctx);
    }
    builder.finish(manifest, documents, ctx)
}

/// Per-node checks that need the finished tree (HEAD-03, HEAD-04, AST-01).
fn structure(
    nodes: &[Node],
    i: usize,
    documents: &[(String, String, String)],
    law: &str,
    ctx: &mut Context,
) {
    let n = &nodes[i];
    if n.kind == "Article" && !n.deleted && n.content.is_empty() {
        ctx.diag(
            "HEAD-04",
            "Active article requires a paragraph",
            &n.path,
            n.span.start,
            n.span.end,
        );
    }
    if (n.rank > 0 && n.rank < ARTICLE_RANK
        || matches!(
            n.kind.as_str(),
            "MainProvision" | "SupplementaryProvision" | "AppendedMaterial"
        ))
        && n.content.is_empty()
    {
        ctx.diag(
            "AST-01",
            "Empty structural container",
            &n.path,
            n.span.start,
            n.span.end,
        );
    }
    if n.kind == "SupplementaryProvision" {
        let direct = n
            .content
            .iter()
            .any(|c| matches!(c,Content::Node(j) if nodes[*j].kind=="Paragraph"));
        let structural = n
            .content
            .iter()
            .any(|c| matches!(c,Content::Node(j) if nodes[*j].kind!="Paragraph"));
        let files = documents
            .iter()
            .filter(|d| format!("{law}/{}", d.2) == n.id)
            .count();
        if direct && (structural || files > 1) {
            ctx.diag(
                "HEAD-03",
                "Mixed supplement form or split direct paragraphs",
                &n.path,
                n.span.start,
                n.span.end,
            );
        }
    }
}

fn outside(
    nodes: &mut [Node],
    article: Option<usize>,
    source: &str,
    path: &str,
    span: Range<usize>,
    ctx: &mut Context,
) {
    let s = &source[span.clone()];
    if s.trim().is_empty() {
        return;
    }
    if let Some(i) = article {
        let n = &mut nodes[i];
        let mut lines = s.split_inclusive('\n');
        let first = lines.next().unwrap_or("");
        if span.start == n.span.end
            && bare(first) == "削除"
            && lines.next().is_none_or(|l| bare(l).is_empty())
            && s[first.len()..].trim().is_empty()
        {
            n.deleted = true;
            return;
        }
    }
    ctx.diag(
        "MD-03",
        "Text outside a legal text container",
        path,
        span.start,
        span.end,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parents_of_containers() {
        let node = |kind, level| {
            let mut n = Node::new(kind, String::new(), None, String::new(), "", 0..0);
            n.level = level;
            n
        };
        let open = |s: &str| marker(s).unwrap();
        let paragraph = open("<!-- loj:paragraph number=1 -->");
        assert!(valid_parent(&paragraph, &node("Article", 0)));
        assert!(valid_parent(&paragraph, &node("SupplementaryProvision", 0)));
        assert!(!valid_parent(&paragraph, &node("Chapter", 0)));
        assert!(!valid_parent(&paragraph, &node("Paragraph", 0)));
        let item = open("<!-- loj:item number=1 -->");
        assert!(valid_parent(&item, &node("Paragraph", 0)));
        assert!(!valid_parent(&item, &node("Article", 0)));
        let first = open("<!-- loj:subitem number=1 level=1 -->");
        assert!(valid_parent(&first, &node("Item", 0)));
        assert!(!valid_parent(&first, &node("Subitem", 1)));
        let third = open("<!-- loj:subitem number=1 level=3 -->");
        assert!(valid_parent(&third, &node("Subitem", 2)));
        assert!(!valid_parent(&third, &node("Subitem", 1)));
        assert!(!valid_parent(&third, &node("Item", 0)));
    }
}
