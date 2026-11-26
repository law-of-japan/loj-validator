//! Semantic construction for loj/v1alpha2 (HEAD, DIR, DISP, AST, SRC, ID,
//! PARA, TABLE, PRE). Derived from the v1alpha1 builder.
use super::directives::{marker, Container, Marker};
use super::kind::Kind;
use super::Context;
use crate::common::{
    markdown::{bare, incomplete_split_blocks, line_end, plain_heading},
    number::Digits,
    structure::{tokens, Token, ARTICLE_RANK, ATX},
};
use regex::Regex;
use serde_json::{json, Value};
use std::{collections::HashSet, ops::Range, sync::LazyLock};

const SEPARATORS: &[char] = &['の', 'ノ'];
/// Open containers at once; the deepest valid nesting is under 20.
const MAX_DEPTH: usize = 64;

/// Unicode 15.1 White_Space, fixed for MD-04 (25 code points).
fn white_space(c: char) -> bool {
    matches!(c, '\u{0009}'..='\u{000d}' | '\u{0020}' | '\u{0085}' | '\u{00a0}' |
        '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' |
        '\u{202f}' | '\u{205f}' | '\u{3000}')
}

fn number(text: &str, separators: &[char], digits: Digits) -> Option<String> {
    let mut style = None;
    for c in text.chars().filter(|c| !separators.contains(c)) {
        let next = if c.is_ascii_digit() {
            0
        } else if ('０'..='９').contains(&c) {
            1
        } else {
            2
        };
        if style.is_some_and(|old| old != next) {
            return None;
        }
        style = Some(next);
    }
    crate::common::number::number(text, separators, digits)
}

fn subitem_number(token: &str) -> (Option<String>, u8) {
    const IROHA: &str = "イロハニホヘトチリヌルヲワカヨタレソツネナラムウヰノオクヤマケフコエテアサキユメミシヱヒモセス";
    if token.chars().count() == 1 {
        if let Some(i) = IROHA.chars().position(|c| token.starts_with(c)) {
            return (Some((i + 1).to_string()), 1);
        }
    }
    if let Some(inner) = token.strip_prefix('（').and_then(|s| s.strip_suffix('）')) {
        let style = if inner.starts_with(|c: char| c.is_ascii_digit()) {
            2
        } else {
            3
        };
        return (number(inner, &[], Digits::Arabic), style);
    }
    (number(token, SEPARATORS, Digits::Ascii), 0)
}

/// TABLE-04 uses intervals, never a grid proportional to untrusted span values.
fn rectangular_table(nodes: &[Node], table: usize) -> bool {
    use std::collections::BTreeMap;
    let rows: Vec<_> = nodes[table].children(nodes, Kind::Row).collect();
    let mut occupied: BTreeMap<u64, u64> = BTreeMap::new();
    let mut expires: BTreeMap<usize, Vec<u64>> = BTreeMap::new();
    let mut covered = 0u64;
    let mut width = None;
    for (row, index) in rows.iter().enumerate() {
        for start in expires.remove(&row).unwrap_or_default() {
            if let Some(end) = occupied.remove(&start) {
                covered -= end - start;
            }
        }
        let mut cursor = 0u64;
        for cell in nodes[*index].children(nodes, Kind::Cell) {
            while let Some(end) = occupied.get(&cursor) {
                cursor = *end;
            }
            let node = &nodes[cell];
            if node.colspan == Some(0) || node.rowspan == Some(0) {
                return false;
            }
            let Some(end) = cursor.checked_add(node.colspan.unwrap_or(1)) else {
                return false;
            };
            if occupied.range(cursor..end).next().is_some() {
                return false;
            }
            let Ok(span) = usize::try_from(node.rowspan.unwrap_or(1)) else {
                return false;
            };
            let Some(until) = row.checked_add(span).filter(|until| *until <= rows.len()) else {
                return false;
            };
            let Some(total) = covered.checked_add(end - cursor) else {
                return false;
            };
            covered = total;
            occupied.insert(cursor, end);
            expires.entry(until).or_default().push(cursor);
            cursor = end;
        }
        let end = occupied.last_key_value().map_or(0, |(_, end)| *end);
        if covered != end || width.is_some_and(|width| width != end) {
            return false;
        }
        width = Some(end);
    }
    true
}

fn caption_ok(text: &str) -> bool {
    if !text.starts_with('（')
        || !text.ends_with('）')
        || !plain_heading(&format!("# {text}"), text)
    {
        return false;
    }
    let mut depth = 0usize;
    let mut previous_open = false;
    for (offset, c) in text.char_indices() {
        match c {
            '（' => {
                depth += 1;
                previous_open = true;
            }
            '）' => {
                if depth == 0 || previous_open {
                    return false;
                }
                depth -= 1;
                if depth == 0 && offset + c.len_utf8() != text.len() {
                    return false;
                }
                previous_open = false;
            }
            '\r' | '\n' => return false,
            _ => previous_open = false,
        }
    }
    depth == 0
}

fn tail_ok(rank: usize, tail: &str) -> bool {
    if rank == ARTICLE_RANK {
        tail.is_empty() || caption_ok(tail)
    } else {
        crate::common::structure::tail_ok(rank, tail)
    }
}

/// A structural heading label: number, kind, branch numbers, rest.
static HEADING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^第([^編章節款目条條]+)([編章節款目条條])((?:[のノ][^のノ 　（）]+)*)(.*)$")
        .expect("constant regex")
});
/// Split a deleted-article label without interpreting words inside captions.
fn range_endpoints(label: &str) -> Option<(&str, &str)> {
    for connector in ["及び", "及ヒ", "乃至", "から"] {
        if let Some((start, end)) = label.split_once(connector) {
            let end = if connector == "から" {
                end.strip_suffix("まで")?
            } else {
                end
            };
            return Some((start, end));
        }
    }
    None
}

fn bare_article(label: &str) -> Option<String> {
    let c = HEADING.captures(label)?;
    if !matches!(&c[2], "条" | "條") || !c[4].is_empty() {
        return None;
    }
    number(&format!("{}{}", &c[1], &c[3]), SEPARATORS, Digits::Any)
}

/// Canonical components have no leading zeroes, so length then lexical order
/// compares each component numerically.
fn compare_numbers(left: &str, right: &str) -> std::cmp::Ordering {
    fn components(number: &str) -> impl Iterator<Item = (usize, &str)> {
        number.split('-').map(|n| (n.len(), n))
    }
    components(left).cmp(components(right))
}

/// A byte range of one source file.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Position {
    path: String,
    start: usize,
    end: usize,
}
impl Position {
    fn to_json(&self) -> Value {
        json!({"path":self.path,"start":self.start,"end":self.end})
    }
}
/// The source fragments that one run of own text was read from, in order.
/// There is always at least one.
#[derive(Clone, Debug)]
struct Fragments {
    first: Position,
    rest: Vec<Position>,
}
impl Fragments {
    fn new(first: Position) -> Self {
        Fragments {
            first,
            rest: Vec::new(),
        }
    }
    fn push(&mut self, position: Position) {
        self.rest.push(position);
    }
    fn first(&self) -> &Position {
        &self.first
    }
    fn last(&self) -> &Position {
        self.rest.last().unwrap_or(&self.first)
    }
    fn iter(&self) -> impl Iterator<Item = &Position> {
        std::iter::once(&self.first).chain(&self.rest)
    }
    /// Map a concatenated text offset back to its original source fragment.
    fn locate(&self, mut offset: usize, length: usize) -> Position {
        for p in self.iter() {
            let size = p.end - p.start;
            if offset < size {
                let start = p.start + offset;
                return Position {
                    path: p.path.clone(),
                    start,
                    end: start.saturating_add(length).min(p.end),
                };
            }
            offset -= size;
        }
        let last = self.last();
        Position {
            path: last.path.clone(),
            start: last.end,
            end: last.end,
        }
    }
}
#[derive(Clone)]
enum Content {
    /// Own text and where it was read from.
    Text(String, Fragments),
    Node(usize),
}
struct Node {
    kind: Kind,
    id: String,
    number: Option<String>,
    display: String,
    level: usize,
    deleted: bool,
    rank: usize,
    implicit: bool,
    caption_present: bool,
    caption: Option<String>,
    key: Option<String>,
    rowspan: Option<u64>,
    colspan: Option<u64>,
    content: Vec<Content>,
    path: String,
    span: Range<usize>,
}
impl Node {
    fn new(
        kind: Kind,
        id: String,
        number: Option<String>,
        display: String,
        path: &str,
        span: Range<usize>,
    ) -> Self {
        Self {
            kind,
            id,
            number,
            display,
            level: 0,
            deleted: false,
            rank: 0,
            implicit: false,
            caption_present: false,
            caption: None,
            key: None,
            rowspan: None,
            colspan: None,
            content: Vec::new(),
            path: path.into(),
            span,
        }
    }
    fn children<'a>(&'a self, nodes: &'a [Node], kind: Kind) -> impl Iterator<Item = usize> + 'a {
        self.content.iter().filter_map(move |c| match c {
            Content::Node(i) if nodes[*i].kind == kind => Some(*i),
            _ => None,
        })
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
    let mut v = json!({"kind":n.kind.as_str(),"id":n.id,"number":n.number,"display":n.display,"content":content});
    match n.kind {
        Kind::Article => v["status"] = json!(if n.deleted { "deleted" } else { "active" }),
        Kind::Subitem => v["level"] = json!(n.level),
        Kind::Paragraph => {
            v["numbering"] = json!(if n.implicit { "implicit" } else { "explicit" });
            if let Some(caption) = &n.caption {
                v["caption"] = json!(caption);
            }
        }
        // AMEND-01: `explicit` stands for an omitted `instruction`.
        Kind::Amendment => {
            v["instruction"] = json!(if n.implicit { "implicit" } else { "explicit" })
        }
        Kind::Row => v["key"] = json!(n.key),
        Kind::Cell => {
            if let Some(r) = n.rowspan {
                v["rowspan"] = json!(r);
            }
            if let Some(c) = n.colspan {
                v["colspan"] = json!(c);
            }
        }
        _ => {}
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
        let rule = if node.rank > 0 {
            "HEAD-02"
        } else if node.kind == Kind::Row {
            "TABLE-02"
        } else if matches!(node.kind, Kind::Preamble | Kind::EnactStatement | Kind::Toc) {
            if node.kind == Kind::Toc {
                "TOC-01"
            } else if node.kind == Kind::Preamble {
                "PRE-01"
            } else {
                "PRE-02"
            }
        } else {
            "DIR-02"
        };
        ctx.diag(
            rule,
            "Duplicate scoped number or key",
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
    let front = matches!(node.kind, Kind::Preamble | Kind::EnactStatement | Kind::Toc);
    nodes.push(node);
    if front {
        // AST-01: EnactStatement, Preamble, then MainProvision.
        let at = nodes[parent]
            .content
            .iter()
            .take_while(|c| matches!(c, Content::Node(j) if matches!(nodes[*j].kind, Kind::EnactStatement | Kind::Preamble | Kind::Toc)))
            .count();
        nodes[parent].content.insert(at, Content::Node(i));
    } else {
        nodes[parent].content.push(Content::Node(i));
    }
    i
}
fn text(
    nodes: &mut [Node],
    index: usize,
    source: &str,
    path: &str,
    span: Range<usize>,
    ctx: &mut Context,
) {
    if span.is_empty() {
        return;
    }
    if matches!(nodes[index].kind, Kind::Table | Kind::Row) {
        // DIR-02: only blank lines directly inside Table and Row; never stored.
        if !source[span.clone()].trim_matches(white_space).is_empty() {
            ctx.diag(
                "DIR-02",
                "Text directly inside a table or row",
                path,
                span.start,
                span.end,
            );
        }
        return;
    }
    let position = Position {
        path: path.into(),
        start: span.start,
        end: span.end,
    };
    if let Some(Content::Text(t, p)) = nodes[index].content.last_mut() {
        t.push_str(&source[span]);
        p.push(position);
    } else {
        nodes[index]
            .content
            .push(Content::Text(source[span].into(), Fragments::new(position)));
    }
}

/// Close a container: own-text (DIR-02), children counts and display (DISP-01).
fn close(nodes: &mut [Node], index: usize, ctx: &mut Context) {
    let kind = nodes[index].kind;
    if matches!(kind, Kind::Amendment | Kind::AmendmentContent) {
        // AMEND-01/02 check the shape on the finished tree.
        return;
    }
    if matches!(kind, Kind::Table | Kind::Row) {
        let child = if kind == Kind::Table {
            Kind::Row
        } else {
            Kind::Cell
        };
        if nodes[index].children(nodes, child).next().is_none() {
            let n = &nodes[index];
            ctx.diag(
                "DIR-02",
                if kind == Kind::Table {
                    "Table requires a row"
                } else {
                    "Row requires a cell"
                },
                &n.path,
                n.span.start,
                n.span.end,
            );
        }
        return;
    }
    let n = &mut nodes[index];
    let first = n
        .content
        .iter()
        .take_while(|c| !n.caption_present || !matches!(c, Content::Node(_)))
        .find_map(|c| {
            if let Content::Text(t, p) = c {
                (!t.trim_matches(white_space).is_empty()).then_some((t, p))
            } else {
                None
            }
        });
    let Some((t, positions)) = first else {
        ctx.diag(
            if n.caption_present {
                "PARA-02"
            } else {
                "DIR-02"
            },
            "Container has no own non-whitespace text",
            &n.path,
            n.span.start,
            n.span.end,
        );
        return;
    };
    // DISP-01 applies to explicit paragraphs, items and subitems only.
    if !matches!(kind, Kind::Paragraph | Kind::Item | Kind::Subitem) {
        return;
    }
    let mut offset = 0;
    let mut first_line = "";
    let mut first_line_bytes = 0;
    let mut caption_span = None;
    for line in t.split_inclusive('\n') {
        if !line.trim_matches(white_space).is_empty() {
            if n.caption_present && caption_span.is_none() {
                let caption = bare(line);
                n.caption = Some(caption.into());
                caption_span = Some((offset, line.len()));
                if !caption_ok(caption) {
                    let p = positions.locate(offset, line.len());
                    ctx.diag(
                        "PARA-02",
                        "Invalid paragraph caption",
                        &p.path,
                        p.start,
                        p.end,
                    );
                }
                offset += line.len();
                continue;
            }
            first_line = bare(line);
            first_line_bytes = line.len();
            break;
        }
        offset += line.len();
    }
    if n.caption_present && first_line.is_empty() {
        let first = positions.first();
        let (offset, length) = caption_span.unwrap_or((0, first.end - first.start));
        let p = positions.locate(offset, length);
        ctx.diag(
            "PARA-02",
            "Paragraph caption requires following body text before its children",
            &p.path,
            p.start,
            p.end,
        );
    }
    if n.implicit {
        return;
    }
    let token = first_line.split([' ', '　']).next().unwrap_or("");
    let digits = match kind {
        Kind::Paragraph => Digits::Arabic,
        Kind::Subitem => Digits::Ascii,
        _ => Digits::Any,
    };
    let parsed = if kind == Kind::Subitem {
        subitem_number(token).0
    } else if kind == Kind::Item && token.chars().any(|c| ('０'..='９').contains(&c)) {
        None
    } else {
        number(token, SEPARATORS, digits)
    };
    let omitted = kind == Kind::Paragraph && n.number.as_deref() == Some("1") && parsed.is_none();
    if !omitted {
        n.display = token.into();
    }
    if !omitted
        && (parsed != n.number
            || first_line
                .get(token.len()..)
                .is_none_or(|r| r.trim_matches(white_space).is_empty()))
    {
        // Locate this line even when own text has multiple source fragments.
        let pos = positions.locate(offset, first_line_bytes);
        ctx.diag(
            "DISP-01",
            "Display number missing or inconsistent",
            &pos.path,
            pos.start,
            pos.end,
        );
    }
}

/// HEAD-03: `附`, zero or more U+3000, `則`. Other spacing is reported, not ignored.
enum Supplement {
    No,
    Valid,
    BadSpacing,
}
fn supplement_label(label: &str) -> Supplement {
    let Some(middle) = label.strip_prefix('附').and_then(|s| s.strip_suffix('則')) else {
        return Supplement::No;
    };
    if middle.chars().all(|c| c == '\u{3000}') {
        Supplement::Valid
    } else if middle.chars().all(char::is_whitespace) {
        Supplement::BadSpacing
    } else {
        Supplement::No
    }
}

/// DIR-02: whether the container that `open` starts may be a child of `p`.
fn valid_parent(open: &Container, p: &Node) -> bool {
    match open {
        // PARA-03: a main provision without articles; AMEND-02: an amendment content.
        Container::Paragraph { .. } => matches!(
            p.kind,
            Kind::Article
                | Kind::SupplementaryProvision
                | Kind::MainProvision
                | Kind::AmendmentContent
        ),
        Container::Item { .. } => matches!(p.kind, Kind::Paragraph | Kind::AmendmentContent),
        // AMEND-02: any level directly in an amendment content.
        Container::Subitem { .. } if p.kind == Kind::AmendmentContent => true,
        Container::Subitem { level: 1, .. } => p.kind == Kind::Item,
        Container::Subitem { level, .. } => p.kind == Kind::Subitem && p.level + 1 == *level,
        // LIST-01: a list holds text only, so a list in a list is a parent error.
        Container::List { .. } | Container::Table { .. } => matches!(
            p.kind,
            Kind::Paragraph | Kind::Item | Kind::Subitem | Kind::AmendmentContent
        ),
        Container::Row { .. } => p.kind == Kind::Table,
        Container::Cell { .. } => p.kind == Kind::Row,
        // AMEND-01; an amendment inside a content is AMEND-02, not a parent error.
        Container::Amendment { .. } => {
            matches!(p.kind, Kind::Paragraph | Kind::Item | Kind::Subitem)
        }
        Container::AmendmentContent => p.kind == Kind::Amendment,
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
    /// Open amendment contents, innermost last (AMEND-02).
    frames: Vec<Frame>,
    /// End of the last structural line that was handled.
    cursor: usize,
}

/// AMEND-02: an open amendment content is a scope of its own for its
/// classifications and articles.
struct Frame {
    content: usize,
    groups: Vec<usize>,
    article: Option<usize>,
}

impl Document<'_> {
    /// The open amendment content when it is the innermost container.
    fn content_frame(&self, nodes: &[Node]) -> Option<&Frame> {
        let innermost = *self.stack.last()?;
        (nodes[innermost].kind == Kind::AmendmentContent)
            .then(|| self.frames.last())
            .flatten()
    }
}

/// MAN-03: how a later file of a scope begins, and how a nonfinal appendix
/// file ends. `continued` says that the next file belongs to the same scope.
fn file_boundaries(
    doc: &Document,
    tokens: &[Token],
    continued: bool,
    paragraph_form: bool,
    ctx: &mut Context,
) {
    let (path, source) = (doc.path, doc.source);
    // A main provision in the paragraph form has one file (PARA-03), which
    // is reported there instead.
    if !doc.first && !doc.appendix && !paragraph_form {
        let starts_with_structure = tokens.first().is_some_and(|t| {
            t.heading
                && source[..t.span.start].trim_matches(white_space).is_empty()
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
        if !doc.first && source.trim_matches(white_space).is_empty() {
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
    main: usize,
    /// Open classification headings (Part to Division) of the current scope.
    groups: Vec<usize>,
    /// Scope name of the previous document.
    previous: String,
    visited: HashSet<String>,
    // PRE-01/02: only before the first structural heading of the main scope.
    main_heading_seen: bool,
    preamble_seen: bool,
}

impl<'a> Builder<'a> {
    /// The law node and one node per scope of the manifest.
    fn new(manifest: &'a Value, ctx: &mut Context) -> Self {
        let law = manifest["law"]["id"]
            .as_str()
            .expect("a string by the manifest schema");
        let mut nodes = vec![Node::new(
            Kind::Law,
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
                Kind::MainProvision,
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
            ("supplements", "supplement", Kind::SupplementaryProvision),
            ("appendices", "appendix", Kind::AppendedMaterial),
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
                                String::new()
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
            main,
            groups: Vec::new(),
            previous: String::new(),
            visited: HashSet::new(),
            main_heading_seen: false,
            preamble_seen: false,
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
        let appendix = self.nodes[scope].kind == Kind::AppendedMaterial;
        let mut doc = Document {
            path,
            source,
            scope,
            first,
            appendix,
            supplementary: self.nodes[scope].kind == Kind::SupplementaryProvision,
            special_heading: appendix && !first,
            article: None,
            stack: Vec::new(),
            frames: Vec::new(),
            cursor: 0,
        };
        let tokens = tokens(source);
        let continued = documents
            .get(index + 1)
            .is_some_and(|(_, _, next)| next == scope_name);
        let paragraph_form = scope == self.main && self.main_paragraphs();
        file_boundaries(&doc, &tokens, continued, paragraph_form, ctx);
        for token in &tokens {
            if crate::cancellation::checkpoint() {
                ctx.incomplete = true;
                break;
            }
            let span = &token.span;
            // MAX_DEPTH bounds the recursion of `export`: amendments inside
            // contents (AMEND-02) are kept in the tree, so nesting is otherwise
            // unbounded. A valid package stays far below it.
            if self.nodes.len() >= 20_000 || span.len() > 4096 || doc.stack.len() >= MAX_DEPTH {
                ctx.incomplete = true;
                ctx.diag(
                    "AST-01",
                    "Implementation limit: 20000 nodes, 4096-byte structural token or 64 open containers",
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
            if let Some(frame) = doc
                .content_frame(&self.nodes)
                .filter(|f| f.article.is_some() || !f.groups.is_empty())
            {
                // AMEND-02: after a heading in a content, text belongs to its
                // article as at document level (a deleted article, or MD-03).
                let article = frame.article;
                outside(&mut self.nodes, article, source, path, before, ctx);
            } else if let Some(&i) = doc.stack.last() {
                text(&mut self.nodes, i, source, path, before, ctx);
            } else if appendix && doc.special_heading {
                text(&mut self.nodes, scope, source, path, before, ctx);
            } else {
                outside(&mut self.nodes, doc.article, source, path, before, ctx);
            }
            if token.heading {
                if doc
                    .stack
                    .last()
                    .is_some_and(|i| self.nodes[*i].kind == Kind::Toc)
                {
                    ctx.diag(
                        "DIR-02",
                        "ATX heading inside Toc container",
                        path,
                        span.start,
                        span.end,
                    );
                    ctx.diag(
                        "TOC-01",
                        "TOC content must not use structural headings",
                        path,
                        span.start,
                        span.end,
                    );
                    doc.cursor = span.end;
                    continue;
                }
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
            text(&mut self.nodes, i, source, path, rest, ctx);
        } else if doc.appendix {
            text(&mut self.nodes, doc.scope, source, path, rest, ctx);
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
            text(&mut self.nodes, doc.scope, source, path, span.clone(), ctx);
            return;
        }
        // AMEND-02: a heading directly in an amendment content is one of its
        // classifications or articles, and closes none of the open markers.
        let in_content = doc.content_frame(&self.nodes).is_some();
        if doc.scope == self.main && !in_content {
            self.main_heading_seen = true;
        }
        if !doc.stack.is_empty() && !in_content {
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
        let supplement = supplement_label(label);
        if in_content {
            if !matches!(supplement, Supplement::No) {
                // AMEND-02/03: a supplement heading in a content is unrecognized.
                ctx.diag(
                    "HEAD-01",
                    "Unrecognized legal heading in an amendment content",
                    path,
                    span.start,
                    span.end,
                );
                return;
            }
            self.content_heading(doc, label, span, ctx);
            return;
        }
        if matches!(supplement, Supplement::BadSpacing) {
            ctx.diag(
                "HEAD-03",
                "Supplement heading allows only U+3000 between 附 and 則",
                path,
                span.start,
                span.end,
            );
            return;
        }
        let is_supplement = matches!(supplement, Supplement::Valid);
        if (doc.supplementary && is_supplement) || doc.appendix {
            self.scope_heading(doc, label, span, ctx);
        } else if is_supplement {
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
        if doc.supplementary && !doc.special_heading {
            scope.display = label.into();
        }
        scope.path = doc.path.to_owned();
        scope.span = span.clone();
        doc.special_heading = true;
    }

    /// AMEND-02: read a heading with the content as the scope and its own
    /// classification context, then restore the context of the document.
    fn content_heading(
        &mut self,
        doc: &mut Document,
        label: &str,
        span: &Range<usize>,
        ctx: &mut Context,
    ) {
        let Some(mut frame) = doc.frames.pop() else {
            // Unreachable: the caller found an open content. Never drop the heading silently.
            ctx.diag(
                "AMEND-02",
                "Heading in an amendment content without its context",
                doc.path,
                span.start,
                span.end,
            );
            return;
        };
        let scope = std::mem::replace(&mut doc.scope, frame.content);
        std::mem::swap(&mut doc.article, &mut frame.article);
        std::mem::swap(&mut self.groups, &mut frame.groups);
        self.structural_heading(doc, label, span, ctx);
        std::mem::swap(&mut self.groups, &mut frame.groups);
        std::mem::swap(&mut doc.article, &mut frame.article);
        doc.scope = scope;
        doc.frames.push(frame);
    }

    /// PARA-03: the main provision has paragraphs directly under it.
    fn main_paragraphs(&self) -> bool {
        self.nodes[self.main]
            .children(&self.nodes, Kind::Paragraph)
            .next()
            .is_some()
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
        let range =
            range_endpoints(label).and_then(|(a, b)| Some((bare_article(a)?, bare_article(b)?)));
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
        // The single-heading pattern can consume another article in its
        // branch suffix. A malformed compound label is a HEAD-01 finding.
        if range.is_none() && matches!(&c[2], "条" | "條") && c[3].contains('第') {
            ctx.diag(
                "HEAD-01",
                "Unrecognized article range",
                path,
                span.start,
                span.end,
            );
            return;
        }
        let num = number(&format!("{}{}", &c[1], &c[3]), SEPARATORS, Digits::Any);
        let num = range.as_ref().map(|(a, b)| format!("{a}:{b}")).or(num);
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
            "編" => (Kind::Part, 1),
            "章" => (Kind::Chapter, 2),
            "節" => (Kind::Section, 3),
            "款" => (Kind::Subsection, 4),
            "目" => (Kind::Division, 5),
            _ => (Kind::Article, ARTICLE_RANK),
        };
        if range.is_none() && !tail_ok(rank, &c[4]) {
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
        if let Some((start, end)) = &range {
            if !compare_numbers(start, end).is_lt() {
                ctx.diag(
                    "HEAD-04",
                    "Article range endpoints must increase",
                    path,
                    span.start,
                    span.end,
                );
            }
        }
        let mut node = Node::new(
            kind,
            format!("{}/{}/{}", self.nodes[id_parent].id, kind.id_segment(), num),
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
        let m = match marker(bare(&doc.source[span.clone()])) {
            Ok(m) => m,
            Err(e) => {
                ctx.diag(
                    "DIR-01",
                    "Invalid reserved marker syntax or attributes",
                    path,
                    span.start,
                    span.end,
                );
                if let Some(rule) = e.also {
                    ctx.diag(rule, "Invalid attribute value", path, span.start, span.end);
                }
                return;
            }
        };
        match m {
            Marker::Close(kind) => self.closing_marker(doc, kind, span, ctx),
            Marker::Preamble => self.front_matter(doc, Kind::Preamble, span, ctx),
            Marker::EnactStatement => self.front_matter(doc, Kind::EnactStatement, span, ctx),
            Marker::Toc => self.front_matter(doc, Kind::Toc, span, ctx),
            Marker::Open(open) => self.container(doc, open, span, ctx),
        }
    }

    /// DIR-02: a closing marker ends the innermost open container of its kind.
    fn closing_marker(
        &mut self,
        doc: &mut Document,
        kind: Kind,
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
        if self.nodes[i].kind == kind {
            self.nodes[i].span.end = span.end;
            close(&mut self.nodes, i, ctx);
            doc.stack.pop();
            if kind == Kind::AmendmentContent {
                doc.frames.pop();
            }
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

    /// PRE-01/PRE-02: the preamble and the enact statement are at document
    /// level, in the first main file, before any structural heading; the enact
    /// statement precedes the preamble.
    fn front_matter(
        &mut self,
        doc: &mut Document,
        kind: Kind,
        span: &Range<usize>,
        ctx: &mut Context,
    ) {
        let path = doc.path;
        let preamble = kind == Kind::Preamble;
        let toc_seen = self.nodes.iter().any(|node| node.kind == Kind::Toc);
        let placed = doc.stack.is_empty()
            && doc.article.is_none()
            && self.groups.is_empty()
            && doc.scope == self.main
            && doc.first
            && !self.main_heading_seen
            // PRE-01/PRE-02/TOC-01: in the paragraph form, before the first paragraph.
            && !self.main_paragraphs()
            && (preamble || !self.preamble_seen)
            && (kind != Kind::EnactStatement || !toc_seen);
        if !placed {
            ctx.diag(
                if kind == Kind::Toc || (kind == Kind::EnactStatement && toc_seen) {
                    "TOC-01"
                } else if preamble {
                    "PRE-01"
                } else {
                    "PRE-02"
                },
                "Preamble or enact statement out of place",
                path,
                span.start,
                span.end,
            );
            if !doc.stack.is_empty() {
                ctx.diag("DIR-02", "Invalid legal parent", path, span.start, span.end);
            }
        }
        if preamble {
            self.preamble_seen = true;
        }
        let id = if kind == Kind::Toc {
            format!("{}/toc", self.law)
        } else if preamble {
            format!("{}/preamble", self.law)
        } else {
            format!("{}/enact-statement", self.law)
        };
        let node = Node::new(kind, id, None, String::new(), path, span.clone());
        let i = add(&mut self.nodes, 0, node, ctx, &mut self.ids);
        doc.stack.push(i);
    }

    /// An opening marker of a paragraph, item, subitem, table, row or cell.
    fn container(
        &mut self,
        doc: &mut Document,
        open: Container,
        span: &Range<usize>,
        ctx: &mut Context,
    ) {
        let path = doc.path;
        let parent = if let Some(frame) = doc.content_frame(&self.nodes) {
            // AMEND-02: in a content, the open article or classification, as
            // at document level.
            frame
                .article
                .or(frame.groups.last().copied())
                .unwrap_or(frame.content)
        } else if let Some(&i) = doc.stack.last() {
            i
        } else if doc.scope == self.main {
            // PARA-03: a paragraph after a classification without an article
            // belongs to the classification (and is a parent error, DIR-02).
            doc.article
                .or(self.groups.last().copied())
                .unwrap_or(doc.scope)
        } else {
            doc.article.unwrap_or(doc.scope)
        };
        if matches!(open, Container::Amendment { .. }) && !doc.frames.is_empty() {
            ctx.diag(
                "AMEND-02",
                "An amendment content cannot contain an amendment",
                path,
                span.start,
                span.end,
            );
        }
        let p = &self.nodes[parent];
        if !valid_parent(&open, p) {
            if p.kind == Kind::Toc {
                ctx.diag(
                    "TOC-01",
                    "TOC content cannot contain markers",
                    path,
                    span.start,
                    span.end,
                );
            }
            ctx.diag(
                "DIR-02",
                "Invalid legal parent or subitem level",
                path,
                span.start,
                span.end,
            );
            if p.kind == Kind::Cell {
                ctx.diag(
                    "TABLE-03",
                    "A cell contains text only",
                    path,
                    span.start,
                    span.end,
                );
            }
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
        let kind = open.kind();
        // ID-01: the segment that this node adds to the ID of its parent.
        let (suffix, num) = match &open {
            Container::Subitem { number, level } => {
                (format!("subitem/{level}/{number}"), Some(number.clone()))
            }
            Container::Row { key } => (format!("row/{key}"), None),
            Container::Cell { .. } => {
                let c = (p.children(&self.nodes, Kind::Cell).count() + 1).to_string();
                (format!("cell/{c}"), Some(c))
            }
            Container::AmendmentContent => (kind.id_segment(), None),
            Container::Paragraph { number, .. }
            | Container::Item { number }
            | Container::List { number }
            | Container::Table { number }
            | Container::Amendment { number, .. } => (
                format!("{}/{number}", kind.id_segment()),
                Some(number.clone()),
            ),
        };
        let mut node = Node::new(
            kind,
            format!("{}/{suffix}", p.id),
            num,
            String::new(),
            path,
            span.clone(),
        );
        match open {
            Container::Paragraph {
                implicit, caption, ..
            } => {
                node.implicit = implicit;
                node.caption_present = caption;
            }
            Container::Subitem { level, .. } => node.level = level,
            Container::Row { key } => node.key = Some(key),
            Container::Cell { rowspan, colspan } => {
                node.rowspan = rowspan;
                node.colspan = colspan;
            }
            // AMEND-01: an amendment without instruction text.
            Container::Amendment { implicit, .. } => node.implicit = implicit,
            Container::Item { .. }
            | Container::List { .. }
            | Container::Table { .. }
            | Container::AmendmentContent => {}
        }
        let i = add(&mut self.nodes, parent, node, ctx, &mut self.ids);
        doc.stack.push(i);
        if kind == Kind::AmendmentContent {
            doc.frames.push(Frame {
                content: i,
                groups: Vec::new(),
                article: None,
            });
        }
    }

    /// Check the finished tree, record the positions (SRC-01) and export the AST.
    fn finish(
        self,
        manifest: &Value,
        documents: &[(String, String, String)],
        ctx: &mut Context,
    ) -> Value {
        let nodes = &self.nodes;
        check_article_overlaps(nodes, ctx);
        for i in 0..nodes.len() {
            if crate::cancellation::checkpoint() {
                ctx.incomplete = true;
                break;
            }
            structure(nodes, i, documents, self.law, ctx);
        }
        for n in nodes {
            if !n.path.is_empty() {
                ctx.position(&n.id, &n.path, n.span.start, n.span.end);
            } else {
                ctx.source_map
                    .entry(n.id.clone())
                    .or_insert_with(|| json!([]));
            }
            for (j, c) in n.content.iter().enumerate() {
                if let Content::Text(t, positions) = c {
                    let spans: Vec<Value> = positions.iter().map(Position::to_json).collect();
                    ctx.positions
                        .push(json!({"nodeId":n.id,"contentIndex":j,"text":t,"spans":spans}));
                }
            }
        }
        json!({"schemaVersion":super::SCHEMA_VERSION,"law":manifest["law"],"root":export(nodes,0)})
    }
}

/// Sort by scope and numeric start so a single sweep detects overlaps,
/// including branches within a range, without expanding the range.
fn check_article_overlaps(nodes: &[Node], ctx: &mut Context) {
    if !nodes
        .iter()
        .any(|n| n.kind == Kind::Article && n.number.as_deref().is_some_and(|v| v.contains(':')))
    {
        return;
    }
    let mut intervals = Vec::new();
    for node in nodes.iter().filter(|n| n.kind == Kind::Article) {
        let Some(number) = node.number.as_deref() else {
            continue;
        };
        let (start, end) = number.split_once(':').unwrap_or((number, number));
        let scope = node.id.rsplit_once('/').map(|(scope, _)| scope);
        intervals.push((scope, start, end, node));
    }
    intervals.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| compare_numbers(a.1, b.1)));
    let mut sorted = intervals.into_iter();
    let Some(mut furthest) = sorted.next() else {
        return;
    };
    for current in sorted {
        let (scope, start, end, node) = current;
        if scope == furthest.0 && !compare_numbers(start, furthest.2).is_gt() {
            // Equal IDs are diagnosed when the node is inserted.
            if node.id != furthest.3.id {
                ctx.diag(
                    "HEAD-02",
                    "Overlapping scoped article numbers",
                    &node.path,
                    node.span.start,
                    node.span.end,
                );
            }
            if !compare_numbers(end, furthest.2).is_gt() {
                continue;
            }
        }
        furthest = current;
    }
}

pub(super) fn build(
    manifest: &Value,
    documents: &[(String, String, String)],
    ctx: &mut Context,
) -> Value {
    let mut builder = Builder::new(manifest, ctx);
    for index in 0..documents.len() {
        if crate::cancellation::checkpoint() {
            ctx.incomplete = true;
            break;
        }
        builder.read(documents, index, ctx);
    }
    builder.finish(manifest, documents, ctx)
}

/// Per-node checks that need the finished tree.
fn structure(
    nodes: &[Node],
    i: usize,
    documents: &[(String, String, String)],
    law: &str,
    ctx: &mut Context,
) {
    let n = &nodes[i];
    if n.kind == Kind::Table && !rectangular_table(nodes, i) {
        ctx.diag(
            "TABLE-04",
            "Table cells overlap, leave gaps, or extend outside the rectangle",
            &n.path,
            n.span.start,
            n.span.end,
        );
    }
    let mut subitem_style = None;
    for child in n.children(nodes, Kind::Subitem) {
        let subitem = &nodes[child];
        let (parsed, style) = subitem_number(&subitem.display);
        if parsed.is_some() {
            if subitem_style.is_some_and(|old| old != style) {
                ctx.diag(
                    "DISP-01",
                    "Sibling subitems must use the same display-number style",
                    &subitem.path,
                    subitem.span.start,
                    subitem.span.end,
                );
            }
            subitem_style = Some(style);
        }
    }
    if n.kind == Kind::Article && !n.deleted && n.number.as_deref().is_some_and(|n| n.contains(':'))
    {
        ctx.diag(
            "HEAD-04",
            "An article range must be deleted",
            &n.path,
            n.span.start,
            n.span.end,
        );
    } else if n.kind == Kind::Article && !n.deleted && n.content.is_empty() {
        ctx.diag(
            "HEAD-04",
            "Active article requires a paragraph",
            &n.path,
            n.span.start,
            n.span.end,
        );
    }
    if (n.rank > 0 && n.rank < 6
        || matches!(
            n.kind,
            Kind::SupplementaryProvision | Kind::AppendedMaterial
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
    if n.kind == Kind::MainProvision {
        // PARA-03: one form, one file for the paragraph form, never empty.
        let direct = n.children(nodes, Kind::Paragraph).next().is_some();
        let structural = n
            .content
            .iter()
            .any(|c| matches!(c, Content::Node(j) if nodes[*j].kind != Kind::Paragraph));
        let files = documents.iter().filter(|d| d.2 == "main").count();
        let message = if n.content.is_empty() {
            Some("Main provision has no classification, article or paragraph")
        } else if direct && structural {
            Some("Main provision mixes paragraphs with classifications or articles")
        } else if direct && files > 1 {
            Some("Main provision in the paragraph form must be one file")
        } else {
            None
        };
        if let Some(message) = message {
            ctx.diag("PARA-03", message, &n.path, n.span.start, n.span.end);
        }
    }
    amendment_shape(nodes, i, ctx);
    if n.kind == Kind::SupplementaryProvision {
        let direct = n.children(nodes, Kind::Paragraph).next().is_some();
        let structural = n
            .content
            .iter()
            .any(|c| matches!(c,Content::Node(j) if nodes[*j].kind!=Kind::Paragraph));
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
    // PARA-01: one numbering form per parent, implicit numbers are 1..n,
    // and a sole paragraph is explicit.
    let paragraphs: Vec<usize> = n.children(nodes, Kind::Paragraph).collect();
    if !paragraphs.is_empty() {
        let implicit = paragraphs.iter().filter(|j| nodes[**j].implicit).count();
        let first = &nodes[paragraphs[0]];
        if implicit > 0 && implicit < paragraphs.len() {
            ctx.diag(
                "PARA-01",
                "Implicit and explicit paragraphs are mixed",
                &first.path,
                first.span.start,
                first.span.end,
            );
        } else if implicit == 1 && paragraphs.len() == 1 {
            ctx.diag(
                "PARA-01",
                "A sole paragraph must be explicit",
                &first.path,
                first.span.start,
                first.span.end,
            );
        }
        if implicit == paragraphs.len() {
            for (k, j) in paragraphs.iter().enumerate() {
                let p = &nodes[*j];
                if p.number.as_deref() != Some(&(k + 1).to_string()) {
                    ctx.diag(
                        "PARA-01",
                        "Implicit paragraph numbers are the order 1..n",
                        &p.path,
                        p.span.start,
                        p.span.end,
                    );
                }
            }
        }
    }
    // LIST-01: list numbers are the order 1..n within the parent, so a branch
    // number is never one.
    for (k, j) in n.children(nodes, Kind::List).enumerate() {
        let l = &nodes[j];
        if l.number.as_deref() != Some(&(k + 1).to_string()) {
            ctx.diag(
                "LIST-01",
                "List numbers are the order 1..n within the parent",
                &l.path,
                l.span.start,
                l.span.end,
            );
        }
    }
    // LIST-02: a paragraph, item or subitem has items or subitems, or lists,
    // not both. In an amendment content the mix is AMEND-02 instead.
    if matches!(n.kind, Kind::Paragraph | Kind::Item | Kind::Subitem) {
        let numbered = n
            .content
            .iter()
            .any(|c| matches!(c, Content::Node(j) if matches!(nodes[*j].kind, Kind::Item | Kind::Subitem)));
        if numbered {
            if let Some(l) = n.children(nodes, Kind::List).next() {
                let l = &nodes[l];
                ctx.diag(
                    "LIST-02",
                    "Items or subitems and lists cannot share a parent",
                    &l.path,
                    l.span.start,
                    l.span.end,
                );
            }
        }
    }
    // TABLE-01: table numbers are the order 1..n within the parent.
    for (k, j) in n.children(nodes, Kind::Table).enumerate() {
        let t = &nodes[j];
        if t.number.as_deref() != Some(&(k + 1).to_string()) {
            ctx.diag(
                "TABLE-01",
                "Table numbers are the order 1..n within the parent",
                &t.path,
                t.span.start,
                t.span.end,
            );
        }
    }
    // TABLE-01 / MD-02: a GFM table in paragraph, item or subitem text.
    if matches!(n.kind, Kind::Paragraph | Kind::Item | Kind::Subitem) {
        for c in &n.content {
            if let Content::Text(t, positions) = c {
                if has_gfm_table(t) {
                    ctx.diag(
                        "TABLE-01",
                        "Write legal tables with table markers, not GFM tables",
                        &positions.first().path,
                        positions.first().start,
                        positions.last().end,
                    );
                }
            }
        }
    }
}

/// AMEND-01: instruction text, then exactly one content, nothing after it;
/// amendments are numbered 1..n within their parent. AMEND-02: a content is
/// one group of children, or text only, and never empty.
fn amendment_shape(nodes: &[Node], i: usize, ctx: &mut Context) {
    let n = &nodes[i];
    let blank =
        |c: &Content| matches!(c, Content::Text(t, _) if t.trim_matches(white_space).is_empty());
    for (k, j) in n.children(nodes, Kind::Amendment).enumerate() {
        let a = &nodes[j];
        if a.number.as_deref() != Some(&(k + 1).to_string()) {
            ctx.diag(
                "AMEND-01",
                "Amendment numbers are the order 1..n within the parent",
                &a.path,
                a.span.start,
                a.span.end,
            );
        }
    }
    // AMEND-01: an amendment without instruction text never follows another
    // one in its parent with only blank parent text between them.
    let mut previous_implicit = false;
    for c in &n.content {
        match c {
            Content::Node(j) if nodes[*j].kind == Kind::Amendment => {
                let a = &nodes[*j];
                if a.implicit && previous_implicit {
                    ctx.diag(
                        "AMEND-01",
                        "An amendment without instruction text cannot directly follow another",
                        &a.path,
                        a.span.start,
                        a.span.end,
                    );
                }
                previous_implicit = a.implicit;
            }
            Content::Text(..) if !blank(c) => previous_implicit = false,
            _ => {}
        }
    }
    match n.kind {
        Kind::Amendment => {
            let contents = n.children(nodes, Kind::AmendmentContent).count();
            let position = n.content.iter().position(|c| matches!(c, Content::Node(_)));
            let before = position.map_or(&n.content[..], |p| &n.content[..p]);
            // With instruction text: some non-blank text first. Without it
            // (`instruction=implicit`): no text at all, not even a blank line.
            let instruction = if n.implicit {
                position.is_some() && before.is_empty()
            } else {
                before
                    .iter()
                    .any(|c| matches!(c, Content::Text(..)) && !blank(c))
            };
            let after = position.map_or(&[][..], |p| &n.content[p + 1..]);
            if contents != 1 || !instruction || !after.iter().all(blank) {
                ctx.diag(
                    "AMEND-01",
                    "An amendment is instruction text followed by exactly one content",
                    &n.path,
                    n.span.start,
                    n.span.end,
                );
            }
        }
        Kind::AmendmentContent => {
            let children: Vec<&Node> = n
                .content
                .iter()
                .filter_map(|c| match c {
                    Content::Node(j) => Some(&nodes[*j]),
                    Content::Text(..) => None,
                })
                .collect();
            let text = n
                .content
                .iter()
                .any(|c| !blank(c) && matches!(c, Content::Text(..)));
            let one = |f: &dyn Fn(&Node) -> bool| children.iter().all(|c| f(c));
            let group = one(&|c| c.rank > 0)
                || one(&|c| c.kind == Kind::Paragraph)
                || one(&|c| c.kind == Kind::Item)
                || one(&|c| c.kind == Kind::Table)
                || one(&|c| c.kind == Kind::List)
                || (one(&|c| c.kind == Kind::Subitem)
                    && children
                        .first()
                        .is_some_and(|f| children.iter().all(|c| c.level == f.level)));
            let valid = if children.is_empty() {
                text
            } else {
                group && !text
            };
            if !valid {
                ctx.diag(
                    "AMEND-02",
                    "An amendment content is one group of children, or text only",
                    &n.path,
                    n.span.start,
                    n.span.end,
                );
            }
        }
        _ => {}
    }
}

fn has_gfm_table(text: &str) -> bool {
    use pulldown_cmark::{Event, Options, Parser, Tag};
    Parser::new_ext(text, Options::ENABLE_TABLES).any(|e| matches!(e, Event::Start(Tag::Table(_))))
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
    if s.trim_matches(white_space).is_empty() {
        return;
    }
    if let Some(i) = article {
        let n = &mut nodes[i];
        let mut lines = s.split_inclusive('\n');
        let first = lines.next().unwrap_or("");
        let second = lines.next();
        if span.start == n.span.end
            && bare(first) == "削除"
            && second.is_none_or(|l| bare(l).is_empty())
            && s[first.len()..].trim_matches(white_space).is_empty()
        {
            n.deleted = true;
            // HEAD-04: `削除` is followed by a blank line or the end of the
            // file, also before a heading or a closing marker of a content.
            if second.is_none() && span.end != source.len() {
                ctx.diag(
                    "HEAD-04",
                    "削除 must be followed by a blank line or the end of the file",
                    path,
                    span.start,
                    span.end,
                );
            }
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
    fn whitespace_and_nested_captions_use_pinned_rules() {
        let spaces = [
            0x9, 0xa, 0xb, 0xc, 0xd, 0x20, 0x85, 0xa0, 0x1680, 0x2000, 0x2001, 0x2002, 0x2003,
            0x2004, 0x2005, 0x2006, 0x2007, 0x2008, 0x2009, 0x200a, 0x2028, 0x2029, 0x202f, 0x205f,
            0x3000,
        ];
        for code in spaces {
            assert!(white_space(char::from_u32(code).unwrap()));
        }
        for c in ['\u{200b}', '\u{feff}', '\u{180e}'] {
            assert!(!white_space(c));
        }
        assert!(caption_ok("（本文（補足））"));
        for text in [
            "（）",
            "（本文（））",
            "（本文）（補足）",
            "（**本文**）",
            "（本文",
            "本文）",
        ] {
            assert!(!caption_ok(text), "{text}");
        }
    }

    #[test]
    fn rectangular_table_checks_intervals_without_expanding_large_spans() {
        fn table(rows: &[&[(u64, u64)]]) -> bool {
            let node = |kind| Node::new(kind, String::new(), None, String::new(), "", 0..0);
            let mut nodes = vec![node(Kind::Table)];
            for cells in rows {
                let row = nodes.len();
                nodes.push(node(Kind::Row));
                nodes[0].content.push(Content::Node(row));
                for &(colspan, rowspan) in *cells {
                    let cell = nodes.len();
                    let mut n = node(Kind::Cell);
                    n.colspan = Some(colspan);
                    n.rowspan = Some(rowspan);
                    nodes.push(n);
                    nodes[row].content.push(Content::Node(cell));
                }
            }
            rectangular_table(&nodes, 0)
        }
        assert!(table(&[&[(1, 2), (1, 1)], &[(1, 1)]]));
        assert!(!table(&[&[(1, 1), (1, 2)], &[(2, 1)]])); // overlap, never slide right
        assert!(!table(&[&[(1, 1), (1, 2)], &[]])); // hole
        assert!(!table(&[&[(1, 2)]])); // below the last row
        assert!(table(&[&[(u64::MAX, 1)]])); // constant memory
        assert!(!table(&[&[(u64::MAX, 1), (1, 1)]])); // overflow
        assert!(!table(&[&[(1, u64::MAX)]]));
    }

    #[test]
    fn fragments_always_have_a_first_and_a_last() {
        let at = |start, end| Position {
            path: "a.md".into(),
            start,
            end,
        };
        let mut fragments = Fragments::new(at(3, 5));
        assert_eq!(fragments.first(), &at(3, 5));
        assert_eq!(fragments.last(), &at(3, 5));
        assert_eq!(fragments.iter().count(), 1);
        fragments.push(at(9, 12));
        fragments.push(at(20, 21));
        assert_eq!(fragments.first(), &at(3, 5));
        assert_eq!(fragments.last(), &at(20, 21));
        let all: Vec<_> = fragments.iter().cloned().collect();
        assert_eq!(all, [at(3, 5), at(9, 12), at(20, 21)]);
    }

    #[test]
    fn supplement_labels() {
        for valid in ["附則", "附　則", "附　　則"] {
            assert!(
                matches!(supplement_label(valid), Supplement::Valid),
                "{valid:?}"
            );
        }
        for bad in ["附 則", "附\t則", "附　 則"] {
            assert!(
                matches!(supplement_label(bad), Supplement::BadSpacing),
                "{bad:?}"
            );
        }
        for no in ["附属書", "附x則", "則", "附則　抄", "第一条"] {
            assert!(matches!(supplement_label(no), Supplement::No), "{no:?}");
        }
    }

    #[test]
    fn malformed_article_ranges_are_unrecognized_headings() {
        let manifest = json!({"law":{"id":"law","title":"例"}});
        for label in [
            "第一条から第三条まで及び第五条",
            "第一条乃至第三条余分",
            "第一条の二から第三条",
            "第一条の二乃至第三条まで",
        ] {
            let documents = [(
                "main.md".into(),
                format!("# {label}\n削除\n"),
                "main".into(),
            )];
            let mut ctx = Context::default();
            build(&manifest, &documents, &mut ctx);
            assert!(
                ctx.diagnostics.iter().any(|(_, d)| d.rule == "HEAD-01"),
                "{label}"
            );
        }
    }

    #[test]
    fn article_ranges_compare_numeric_branches_within_their_own_scope() {
        let article = |scope: &str, number: &str| {
            Node::new(
                Kind::Article,
                format!("{scope}/art/{number}"),
                Some(number.to_owned()),
                String::new(),
                "main.md",
                0..1,
            )
        };
        let mut nodes = vec![
            article("main", "10-2"),
            article("main", "9-2:10"),
            article("supplement", "9-2"),
            article("main", "9-1"),
        ];
        let mut ctx = Context::default();
        check_article_overlaps(&nodes, &mut ctx);
        assert!(ctx.diagnostics.is_empty());
        for number in ["9-2-1:9-3", "9-2:9-4", "10", "10:10-1"] {
            nodes.push(article("main", number));
            check_article_overlaps(&nodes, &mut ctx);
            assert_eq!(ctx.diagnostics.len(), 1, "{number}");
            assert_eq!(ctx.diagnostics[0].1.rule, "HEAD-02");
            ctx.diagnostics.clear();
            nodes.pop();
        }
    }

    #[test]
    fn parents_of_containers() {
        let node = |kind, level| {
            let mut n = Node::new(kind, String::new(), None, String::new(), "", 0..0);
            n.level = level;
            n
        };
        let open = |s: &str| match marker(s) {
            Ok(Marker::Open(container)) => container,
            _ => panic!("not an opening marker: {s}"),
        };
        let paragraph = open("<!-- loj:paragraph number=1 -->");
        assert!(valid_parent(&paragraph, &node(Kind::Article, 0)));
        assert!(valid_parent(
            &paragraph,
            &node(Kind::SupplementaryProvision, 0)
        ));
        assert!(!valid_parent(&paragraph, &node(Kind::Chapter, 0)));
        assert!(!valid_parent(&paragraph, &node(Kind::Paragraph, 0)));
        let item = open("<!-- loj:item number=1 -->");
        assert!(valid_parent(&item, &node(Kind::Paragraph, 0)));
        assert!(!valid_parent(&item, &node(Kind::Article, 0)));
        let first = open("<!-- loj:subitem number=1 level=1 -->");
        assert!(valid_parent(&first, &node(Kind::Item, 0)));
        assert!(!valid_parent(&first, &node(Kind::Subitem, 1)));
        let third = open("<!-- loj:subitem number=1 level=3 -->");
        assert!(valid_parent(&third, &node(Kind::Subitem, 2)));
        assert!(!valid_parent(&third, &node(Kind::Subitem, 1)));
        assert!(!valid_parent(&third, &node(Kind::Item, 0)));
        let table = open("<!-- loj:table number=1 -->");
        for kind in [Kind::Paragraph, Kind::Item, Kind::Subitem] {
            assert!(valid_parent(&table, &node(kind, 0)));
        }
        assert!(!valid_parent(&table, &node(Kind::Cell, 0)));
        let row = open("<!-- loj:row key=k3v9 -->");
        assert!(valid_parent(&row, &node(Kind::Table, 0)));
        assert!(!valid_parent(&row, &node(Kind::Row, 0)));
        let cell = open("<!-- loj:cell -->");
        assert!(valid_parent(&cell, &node(Kind::Row, 0)));
        assert!(!valid_parent(&cell, &node(Kind::Table, 0)));
    }
}
