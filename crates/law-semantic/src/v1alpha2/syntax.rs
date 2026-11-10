//! Semantic construction for loj/v1alpha2 (HEAD, DIR, DISP, AST, SRC, ID,
//! PARA, TABLE, PRE). Derived from the v1alpha1 builder.
use super::directives::marker;
use super::kind::Kind;
use super::Context;
use crate::common::{
    markdown::{
        bare, blocks, incomplete_split_blocks, line_end, line_start, plain_heading, reserved,
        BlockKind,
    },
    number::{number, Digits},
};
use regex::Regex;
use serde_json::{json, Value};
use std::{collections::HashSet, ops::Range, sync::LazyLock};

const SEPARATORS: &[char] = &['の', 'ノ'];

/// A structural heading label: number, kind, branch numbers, rest.
static HEADING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^第([^編章節款目条條]+)([編章節款目条條])((?:[のノ][^のノ 　（）]+)*)(.*)$")
        .expect("constant regex")
});
/// A plain ATX heading line; group 1 is the label.
static ATX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^#{1,6} ([^\r\n]+)$").expect("constant regex"));
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
#[derive(Clone)]
enum Content {
    /// Own text and the source fragments it was read from, in order (never empty).
    Text(String, Vec<Position>),
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
        Kind::Paragraph => v["numbering"] = json!(if n.implicit { "implicit" } else { "explicit" }),
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
        } else if matches!(node.kind, Kind::Preamble | Kind::EnactStatement) {
            if node.kind == Kind::Preamble {
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
    let front = matches!(node.kind, Kind::Preamble | Kind::EnactStatement);
    nodes.push(node);
    if front {
        // AST-01: EnactStatement, Preamble, then MainProvision.
        let at = nodes[parent]
            .content
            .iter()
            .take_while(|c| matches!(c, Content::Node(j) if matches!(nodes[*j].kind, Kind::EnactStatement | Kind::Preamble)))
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
        if !source[span.clone()].trim().is_empty() {
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
            .push(Content::Text(source[span].into(), vec![position]));
    }
}

/// Close a container: own-text (DIR-02), children counts and display (DISP-01).
fn close(nodes: &mut [Node], index: usize, ctx: &mut Context) {
    let kind = nodes[index].kind;
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
    // DISP-01 applies to explicit paragraphs, items and subitems only.
    if !matches!(kind, Kind::Paragraph | Kind::Item | Kind::Subitem) || n.implicit {
        return;
    }
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
    let digits = match kind {
        Kind::Paragraph => Digits::Arabic,
        Kind::Subitem => Digits::Ascii,
        _ => Digits::Any,
    };
    let parsed = number(token, SEPARATORS, digits);
    let omitted = kind == Kind::Paragraph && n.number.as_deref() == Some("1") && parsed.is_none();
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
        let mut relative = offset;
        let mut pos = &positions[0];
        for p in positions {
            let len = p.end - p.start;
            pos = p;
            if relative < len {
                break;
            }
            relative -= len;
        }
        let start = pos.start + relative;
        let end = (start + first_line_bytes).min(pos.end);
        ctx.diag(
            "DISP-01",
            "Display number missing or inconsistent",
            &pos.path,
            start,
            end,
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

pub(super) fn build(
    manifest: &Value,
    documents: &[(String, String, String)],
    ctx: &mut Context,
) -> Value {
    let law = manifest["law"]["id"].as_str().unwrap();
    let mut nodes = vec![Node::new(
        Kind::Law,
        law.into(),
        None,
        manifest["law"]["title"].as_str().unwrap().into(),
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
                let scope = format!("{prefix}/{}", s["key"].as_str().unwrap());
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
                            s["title"].as_str().unwrap().into()
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
    let (heading_re, atx): (&Regex, &Regex) = (&HEADING, &ATX);
    let mut previous = String::new();
    let mut groups: Vec<usize> = Vec::new();
    let mut visited = HashSet::new();
    // PRE-01/02: only before the first structural heading of the main scope.
    let mut main_heading_seen = false;
    let mut preamble_seen = false;
    for (document_index, (path, source, scope_name)) in documents.iter().enumerate() {
        let Some(&scope) = scopes.get(scope_name) else {
            continue;
        };
        if previous != *scope_name {
            groups.clear();
        }
        previous = scope_name.clone();
        let first = visited.insert(scope_name.clone());
        let mut article = None;
        let mut stack: Vec<usize> = Vec::new();
        let mut cursor = 0;
        let appendix = nodes[scope].kind == Kind::AppendedMaterial;
        let supplementary = nodes[scope].kind == Kind::SupplementaryProvision;
        let mut special_heading = appendix && !first;
        let parsed = blocks(source);
        let mut events = Vec::new();
        for (kind, span) in parsed {
            let start = line_start(source, span.start);
            let end = line_end(source, span.end.saturating_sub(1));
            match kind {
                BlockKind::Heading(plain) => events.push((true, plain, start..end)),
                BlockKind::Html if reserved(&source[start..end]) => {
                    events.push((false, false, start..end))
                }
                _ => {}
            }
        }
        if !first && !appendix {
            let starts_with_structure = events.first().is_some_and(|(heading, _, span)| {
                *heading
                    && source[..span.start].trim().is_empty()
                    && atx
                        .captures(bare(&source[span.clone()]))
                        .is_some_and(|c| heading_re.is_match(&c[1]))
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
        if appendix {
            if !first && source.trim().is_empty() {
                ctx.diag(
                    "MAN-03",
                    "Subsequent appendix file must contain nonblank text",
                    path,
                    0,
                    source.len(),
                );
            }
            if documents
                .get(document_index + 1)
                .is_some_and(|(_, _, next)| next == scope_name)
            {
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
        for (is_heading, heading_plain, span) in events {
            if nodes.len() >= 20_000 || span.len() > 4096 {
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
            if span.start < cursor {
                continue;
            }
            if let Some(&i) = stack.last() {
                text(&mut nodes, i, source, path, cursor..span.start, ctx);
            } else if appendix && special_heading {
                text(&mut nodes, scope, source, path, cursor..span.start, ctx);
            } else {
                outside(&mut nodes, article, source, path, cursor..span.start, ctx);
            }
            let raw = bare(&source[span.clone()]);
            if is_heading {
                if appendix && (!first || special_heading) {
                    text(&mut nodes, scope, source, path, span.clone(), ctx);
                    cursor = span.end;
                    continue;
                }
                if scope == main {
                    main_heading_seen = true;
                }
                if !stack.is_empty() {
                    ctx.diag(
                        "MD-03",
                        "Heading inside explicit container",
                        path,
                        span.start,
                        span.end,
                    );
                    for i in stack.drain(..) {
                        ctx.diag(
                            "DIR-02",
                            "Unclosed before heading",
                            path,
                            nodes[i].span.start,
                            nodes[i].span.end,
                        );
                    }
                }
                let label = atx.captures(raw).and_then(|c| c.get(1)).map(|m| m.as_str());
                let plain = label.is_some_and(|l| {
                    heading_plain && !l.ends_with(char::is_whitespace) && plain_heading(raw, l)
                });
                if !plain {
                    ctx.diag(
                        "HEAD-01",
                        "Expected strict plain ATX heading",
                        path,
                        span.start,
                        span.end,
                    );
                    cursor = span.end;
                    continue;
                }
                let label = label.unwrap();
                let supplement = supplement_label(label);
                if matches!(supplement, Supplement::BadSpacing) {
                    ctx.diag(
                        "HEAD-03",
                        "Supplement heading allows only U+3000 between 附 and 則",
                        path,
                        span.start,
                        span.end,
                    );
                    cursor = span.end;
                    continue;
                }
                let is_supplement = matches!(supplement, Supplement::Valid);
                if (supplementary && is_supplement) || appendix {
                    if !first
                        || special_heading
                        || span.start != 0
                        || (appendix && label != nodes[scope].display)
                    {
                        ctx.diag(
                            "HEAD-03",
                            "Invalid scope heading",
                            path,
                            span.start,
                            span.end,
                        );
                    }
                    if supplementary && !special_heading {
                        nodes[scope].display = label.into();
                    }
                    nodes[scope].path = path.clone();
                    nodes[scope].span = span.clone();
                    special_heading = true;
                    cursor = span.end;
                    continue;
                }
                if is_supplement {
                    ctx.diag(
                        "HEAD-03",
                        "Supplement heading cannot change scope",
                        path,
                        span.start,
                        span.end,
                    );
                    cursor = span.end;
                    continue;
                }
                let Some(c) = heading_re.captures(label) else {
                    ctx.diag(
                        "HEAD-01",
                        "Unrecognized legal heading",
                        path,
                        span.start,
                        span.end,
                    );
                    cursor = span.end;
                    continue;
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
                    cursor = span.end;
                    continue;
                };
                let (kind, rank) = match &c[2] {
                    "編" => (Kind::Part, 1),
                    "章" => (Kind::Chapter, 2),
                    "節" => (Kind::Section, 3),
                    "款" => (Kind::Subsection, 4),
                    "目" => (Kind::Division, 5),
                    _ => (Kind::Article, 6),
                };
                let tail = &c[4];
                let tail_ok = if rank == 6 {
                    tail.is_empty()
                        || (tail.starts_with('（')
                            && tail.ends_with('）')
                            && tail.len() > 6
                            && !tail[3..tail.len() - 3].ends_with(char::is_whitespace)
                            && !tail[3..tail.len() - 3].contains(['（', '）', '(', ')']))
                } else {
                    tail.strip_prefix([' ', '　'])
                        .is_some_and(|s| !s.is_empty() && !s.starts_with([' ', '　']))
                };
                if !tail_ok {
                    ctx.diag(
                        "HEAD-01",
                        "Invalid heading title or caption",
                        path,
                        span.start,
                        span.end,
                    );
                    cursor = span.end;
                    continue;
                }
                if rank < 6 {
                    while groups.last().is_some_and(|i| nodes[*i].rank >= rank) {
                        groups.pop();
                    }
                }
                let parent = groups.last().copied().unwrap_or(scope);
                let id_parent = if rank == 6 { scope } else { parent };
                let mut node = Node::new(
                    kind,
                    format!("{}/{}/{}", nodes[id_parent].id, kind.id_segment(), num),
                    Some(num),
                    label.into(),
                    path,
                    span.clone(),
                );
                node.rank = rank;
                let index = add(&mut nodes, parent, node, ctx, &mut ids);
                if rank == 6 {
                    article = Some(index);
                } else {
                    groups.push(index);
                    article = None;
                }
            } else {
                if appendix {
                    ctx.diag(
                        "HEAD-03",
                        "Reserved marker in appendix",
                        path,
                        span.start,
                        span.end,
                    );
                    cursor = span.end;
                    continue;
                }
                let m = match marker(raw) {
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
                        cursor = span.end;
                        continue;
                    }
                };
                if m.close {
                    if let Some(&i) = stack.last() {
                        if nodes[i].kind == m.kind {
                            nodes[i].span.end = span.end;
                            close(&mut nodes, i, ctx);
                            stack.pop();
                        } else {
                            ctx.diag(
                                "DIR-02",
                                "Mismatched closing marker",
                                path,
                                span.start,
                                span.end,
                            );
                        }
                    } else {
                        ctx.diag(
                            "DIR-02",
                            "Unexpected closing marker",
                            path,
                            span.start,
                            span.end,
                        );
                    }
                    cursor = span.end;
                    continue;
                }
                if matches!(m.kind, Kind::Preamble | Kind::EnactStatement) {
                    // PRE-01/PRE-02: document level, first main file, before any
                    // structural heading; the enact statement precedes the preamble.
                    let rule = if m.kind == Kind::Preamble {
                        "PRE-01"
                    } else {
                        "PRE-02"
                    };
                    let placed = stack.is_empty()
                        && article.is_none()
                        && groups.is_empty()
                        && scope == main
                        && first
                        && !main_heading_seen
                        && (m.kind == Kind::Preamble || !preamble_seen);
                    if !placed {
                        ctx.diag(
                            rule,
                            "Preamble or enact statement out of place",
                            path,
                            span.start,
                            span.end,
                        );
                        if !stack.is_empty() {
                            ctx.diag("DIR-02", "Invalid legal parent", path, span.start, span.end);
                        }
                    }
                    if m.kind == Kind::Preamble {
                        preamble_seen = true;
                    }
                    let id = if m.kind == Kind::Preamble {
                        format!("{law}/preamble")
                    } else {
                        format!("{law}/enact-statement")
                    };
                    let node = Node::new(m.kind, id, None, String::new(), path, span.clone());
                    let i = add(&mut nodes, 0, node, ctx, &mut ids);
                    stack.push(i);
                    cursor = span.end;
                    continue;
                }
                let parent = stack.last().copied().or(article).unwrap_or(scope);
                let p = &nodes[parent];
                let valid = match m.kind {
                    Kind::Paragraph => {
                        p.kind == Kind::Article || p.kind == Kind::SupplementaryProvision
                    }
                    Kind::Item => p.kind == Kind::Paragraph,
                    Kind::Subitem => {
                        if m.level == 1 {
                            p.kind == Kind::Item
                        } else {
                            p.kind == Kind::Subitem && p.level + 1 == m.level
                        }
                    }
                    Kind::Table => matches!(p.kind, Kind::Paragraph | Kind::Item | Kind::Subitem),
                    Kind::Row => p.kind == Kind::Table,
                    _ => p.kind == Kind::Row,
                };
                if !valid {
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
                    cursor = span.end;
                    continue;
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
                let (suffix, num) = match m.kind {
                    Kind::Subitem => (
                        format!("subitem/{}/{}", m.level, m.number.as_deref().unwrap()),
                        m.number.clone(),
                    ),
                    Kind::Row => (format!("row/{}", m.key.as_deref().unwrap()), None),
                    Kind::Cell => {
                        let c = (p.children(&nodes, Kind::Cell).count() + 1).to_string();
                        (format!("cell/{c}"), Some(c))
                    }
                    _ => (
                        format!("{}/{}", m.kind.id_segment(), m.number.as_deref().unwrap()),
                        m.number.clone(),
                    ),
                };
                let mut node = Node::new(
                    m.kind,
                    format!("{}/{suffix}", p.id),
                    num,
                    String::new(),
                    path,
                    span.clone(),
                );
                node.level = m.level;
                node.implicit = m.implicit;
                node.key = m.key;
                node.rowspan = m.rowspan;
                node.colspan = m.colspan;
                let i = add(&mut nodes, parent, node, ctx, &mut ids);
                stack.push(i);
            }
            cursor = span.end;
        }
        if let Some(&i) = stack.last() {
            text(&mut nodes, i, source, path, cursor..source.len(), ctx);
        } else if appendix {
            text(&mut nodes, scope, source, path, cursor..source.len(), ctx);
        } else {
            outside(&mut nodes, article, source, path, cursor..source.len(), ctx);
        }
        for i in stack {
            ctx.diag(
                "DIR-02",
                "Unclosed marker at file boundary",
                path,
                nodes[i].span.start,
                nodes[i].span.end,
            );
        }
        if first && (supplementary || appendix) && !special_heading {
            ctx.diag(
                "HEAD-03",
                "Required initial scope heading missing",
                path,
                0,
                line_end(source, 0),
            );
        }
    }
    for i in 0..nodes.len() {
        structure(&nodes, i, documents, law, ctx);
    }
    for n in &nodes {
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
    json!({"schemaVersion":super::SCHEMA_VERSION,"law":manifest["law"],"root":export(&nodes,0)})
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
    if n.kind == Kind::Article && !n.deleted && n.content.is_empty() {
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
            Kind::MainProvision | Kind::SupplementaryProvision | Kind::AppendedMaterial
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
                if let (true, Some(first), Some(last)) =
                    (has_gfm_table(t), positions.first(), positions.last())
                {
                    ctx.diag(
                        "TABLE-01",
                        "Write legal tables with table markers, not GFM tables",
                        &first.path,
                        first.start,
                        last.end,
                    );
                }
            }
        }
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
