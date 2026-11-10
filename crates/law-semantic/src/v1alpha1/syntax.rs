use super::directives::marker;
use super::Context;
use crate::common::{
    markdown::{
        bare, blocks, incomplete_split_blocks, line_end, line_start, plain_heading, reserved,
        BlockKind,
    },
    number::number,
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
/// A plain ATX heading line; group 1 is the label.
static ATX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^#{1,6} ([^\r\n]+)$").expect("constant regex"));
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
    let mode = match n.kind.as_str() {
        "Paragraph" => 1,
        "Subitem" => 2,
        _ => 0,
    };
    let parsed = number(token, SEPARATORS, mode);
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
        let mut relative = offset;
        let mut pos = &positions[0];
        for p in positions {
            let len = (p["end"].as_u64().unwrap() - p["start"].as_u64().unwrap()) as usize;
            pos = p;
            if relative < len {
                break;
            }
            relative -= len;
        }
        let start = pos["start"].as_u64().unwrap() as usize + relative;
        let end = (start + first_line_bytes).min(pos["end"].as_u64().unwrap() as usize);
        ctx.diag(
            "DISP-01",
            "Display number missing or inconsistent",
            pos["path"].as_str().unwrap(),
            start,
            end,
        );
    }
}

pub(super) fn build(
    manifest: &Value,
    documents: &[(String, String, String)],
    ctx: &mut Context,
) -> Value {
    let law = manifest["law"]["id"].as_str().unwrap();
    let mut nodes = vec![Node::new(
        "Law",
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
                let scope = format!("{prefix}/{}", s["key"].as_str().unwrap());
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
        let appendix = nodes[scope].kind == "AppendedMaterial";
        let supplementary = nodes[scope].kind == "SupplementaryProvision";
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
                text(&mut nodes, i, source, path, cursor..span.start);
            } else if appendix && special_heading {
                text(&mut nodes, scope, source, path, cursor..span.start);
            } else {
                outside(&mut nodes, article, source, path, cursor..span.start, ctx);
            }
            let raw = bare(&source[span.clone()]);
            if is_heading {
                if appendix && (!first || special_heading) {
                    text(&mut nodes, scope, source, path, span.clone());
                    cursor = span.end;
                    continue;
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
                if (supplementary && label == "附則") || appendix {
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
                    nodes[scope].path = path.clone();
                    nodes[scope].span = span.clone();
                    special_heading = true;
                    cursor = span.end;
                    continue;
                }
                if label == "附則" {
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
                let num = number(&format!("{}{}", &c[1], &c[3]), SEPARATORS, 0);
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
                    "編" => ("Part", 1),
                    "章" => ("Chapter", 2),
                    "節" => ("Section", 3),
                    "款" => ("Subsection", 4),
                    "目" => ("Division", 5),
                    _ => ("Article", 6),
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
                    format!("{}/{}/{}", nodes[id_parent].id, kind.to_lowercase(), num),
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
                let Some(m) = marker(raw) else {
                    ctx.diag(
                        "DIR-01",
                        "Invalid reserved marker syntax or attributes",
                        path,
                        span.start,
                        span.end,
                    );
                    cursor = span.end;
                    continue;
                };
                if m.close {
                    if let Some(&i) = stack.last() {
                        if nodes[i].kind == m.kind {
                            nodes[i].span.end = span.end;
                            display(&mut nodes, i, ctx);
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
                } else {
                    let parent = stack.last().copied().or(article).unwrap_or(scope);
                    let p = &nodes[parent];
                    let valid = match m.kind {
                        "Paragraph" => p.kind == "Article" || p.kind == "SupplementaryProvision",
                        "Item" => p.kind == "Paragraph",
                        _ => {
                            if m.level == 1 {
                                p.kind == "Item"
                            } else {
                                p.kind == "Subitem" && p.level + 1 == m.level
                            }
                        }
                    };
                    if !valid {
                        ctx.diag(
                            "DIR-02",
                            "Invalid legal parent or subitem level",
                            path,
                            span.start,
                            span.end,
                        );
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
                    let i = add(&mut nodes, parent, node, ctx, &mut ids);
                    stack.push(i);
                }
            }
            cursor = span.end;
        }
        if let Some(&i) = stack.last() {
            text(&mut nodes, i, source, path, cursor..source.len());
        } else if appendix {
            text(&mut nodes, scope, source, path, cursor..source.len());
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
        if (n.rank > 0 && n.rank < 6
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
    json!({"schemaVersion":"loj/v1alpha1","law":manifest["law"],"root":export(&nodes,0)})
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
