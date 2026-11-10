use pulldown_cmark::{Event, Options, Parser, Tag};
use regex::Regex;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug)]
struct Block {
    kind: String,
    text: String,
    target: String,
    level: u8,
    ordered: bool,
    start: usize,
    end: usize,
    children: Vec<Block>,
}
impl Block {
    fn plain(&self) -> String {
        let mut s = self.text.clone();
        for c in &self.children {
            if c.kind != "preserved-link" {
                s.push_str(&c.plain());
            }
        }
        s
    }
}
fn parse(source: &str) -> Vec<Block> {
    let mut stack: Vec<Block> = vec![];
    let mut roots = vec![];
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_WIKILINKS;
    for (event, span) in Parser::new_ext(source, options).into_offset_iter() {
        let mut b = Block {
            kind: String::new(),
            text: String::new(),
            target: String::new(),
            level: 0,
            ordered: false,
            start: span.start,
            end: span.end,
            children: vec![],
        };
        match event {
            Event::Start(tag) => {
                b.kind = match tag {
                    Tag::Heading { level, .. } => {
                        b.level = level as u8;
                        "heading"
                    }
                    Tag::Paragraph => "paragraph",
                    Tag::List(n) => {
                        b.ordered = n.is_some();
                        "list"
                    }
                    Tag::Item => "list-item",
                    Tag::BlockQuote(_) => "quote",
                    Tag::CodeBlock(_) => "code",
                    Tag::Link { dest_url, .. } => {
                        b.target = dest_url.to_string();
                        "link"
                    }
                    Tag::Image { dest_url, .. } => {
                        b.target = dest_url.to_string();
                        "image"
                    }
                    Tag::Emphasis | Tag::Strong | Tag::Strikethrough => "inline",
                    Tag::Table(_) => "table",
                    Tag::TableHead | Tag::TableRow | Tag::TableCell => "table-part",
                    _ => "unsupported",
                }
                .into();
                stack.push(b);
                continue;
            }
            Event::End(_) => {
                b = stack.pop().expect("balanced parser events");
                b.end = span.end;
            }
            Event::Text(t) | Event::Code(t) => {
                b.kind = "text".into();
                b.text = t.to_string();
            }
            Event::SoftBreak | Event::HardBreak => {
                b.kind = "text".into();
                b.text = "\n".into();
            }
            Event::Html(t) | Event::InlineHtml(t) => {
                b.kind = "unsupported".into();
                b.text = t.to_string();
            }
            Event::Rule => {
                b.kind = "unsupported".into();
                b.text = source[span].to_string();
            }
            _ => {
                b.kind = "unsupported".into();
                b.text = source[span].to_string();
            }
        }
        if let Some(parent) = stack.last_mut() {
            parent.children.push(b);
        } else {
            roots.push(b);
        }
    }
    roots
}

/// A named group from a profile regex. The group is known to exist (checked at
/// profile load) but may be optional in the pattern; absent reads as empty.
fn group<'t>(caps: &regex::Captures<'t>, name: &str) -> &'t str {
    caps.name(name).map_or("", |m| m.as_str())
}

#[derive(Clone)]
struct Section {
    heading: Block,
    blocks: Vec<Block>,
    sections: Vec<Section>,
    end: usize,
}
fn sections(blocks: &[Block], i: &mut usize, level: u8, end: usize) -> Vec<Section> {
    let mut out = vec![];
    while *i < blocks.len() {
        if blocks[*i].kind != "heading" || blocks[*i].level <= level {
            break;
        }
        let h = blocks[*i].clone();
        *i += 1;
        let mut body = vec![];
        while *i < blocks.len() && blocks[*i].kind != "heading" {
            body.push(blocks[*i].clone());
            *i += 1;
        }
        let children = sections(blocks, i, h.level, end);
        let stop = if *i < blocks.len() {
            blocks[*i].start
        } else {
            end
        };
        out.push(Section {
            heading: h,
            blocks: body,
            sections: children,
            end: stop,
        });
    }
    out
}

struct Rule {
    id: String,
    kind: String,
    levels: Vec<u8>,
    pattern: Regex,
}
pub struct Mapper<'a> {
    source: &'a str,
    path: &'a str,
    mode: String,
    rules: Vec<Rule>,
    item: Regex,
    issues: Vec<Value>,
    serial: usize,
    law_headings: usize,
    anchors: Vec<Value>,
    reference_kinds: Value,
    bare_paragraph: Option<Regex>,
    plain_items: bool,
}
impl<'a> Mapper<'a> {
    fn bind_anchor(&mut self, start: usize, node_id: &str) {
        for a in &mut self.anchors {
            if a["origins"][0]["start_byte"].as_u64() == Some(start as u64) {
                a["node_id"] = json!(node_id);
            }
        }
    }
    fn span(&self, start: usize, end: usize) -> Value {
        json!({"source":self.path,"start_byte":start,"end_byte":end})
    }
    fn diagnostic(&mut self, code: &str, message: String, rule: &str, start: usize, end: usize) {
        self.issues.push(
            json!({"code":code,"severity":"error","message":message,"rule_id":rule,
            "node_id":null,"primary_span":self.span(start,end),"related_spans":[]}),
        );
    }
    fn node(
        &mut self,
        kind: &str,
        id: String,
        number: Value,
        title: String,
        start: usize,
        end: usize,
    ) -> Value {
        json!({"kind":kind,"id":id,"number":number,"title":title,"status":"active",
            "children":[],"content":[],"references":[],"origins":[self.span(start,end)]})
    }
    fn numbering(&mut self, display: &str, rule: &str, start: usize, end: usize) -> Option<Value> {
        match crate::number::normalize(display) {
            Ok(parts) => Some(json!({"display":display,"parts":parts,"implicit":false})),
            Err(msg) => {
                self.diagnostic("NUMBER_INVALID", msg, rule, start, end);
                None
            }
        }
    }
    fn identify(&mut self, s: &Section) -> Option<(String, String, Value, String)> {
        let text = s.heading.plain();
        let candidates: Vec<_> = self
            .rules
            .iter()
            .filter(|r| r.levels.contains(&s.heading.level) && r.pattern.is_match(&text))
            .collect();
        if candidates.len() != 1 {
            let code = if candidates.is_empty() {
                "UNMAPPED"
            } else {
                "AMBIGUOUS"
            };
            self.diagnostic(
                code,
                format!("heading: {text}"),
                "mapping/heading",
                s.heading.start,
                s.heading.end,
            );
            return None;
        }
        let rule = candidates[0];
        let caps = rule.pattern.captures(&text).unwrap();
        let kind = rule.kind.clone();
        let id = rule.id.clone();
        let title = caps
            .name("title")
            .map(|m| m.as_str().to_owned())
            .unwrap_or_default();
        let display = caps.name("number").map(|m| {
            format!(
                "{}{}",
                m.as_str(),
                caps.name("branch").map(|b| b.as_str()).unwrap_or("")
            )
        });
        let number = if let Some(display) = display {
            self.numbering(&display, &id, s.heading.start, s.heading.end)?
        } else {
            Value::Null
        };
        Some((kind, id, number, title))
    }
    fn refs(&self, b: &Block, out: &mut Vec<Value>) {
        if b.kind == "link" || b.kind == "preserved-link" {
            out.push(json!({"label":b.plain(),"target_uri":b.target,"expected_kinds":self.reference_kinds,
                "target_id":null,"resolution":if b.target.starts_with('#'){"unresolved"}else{"unsupported"},
                "origins":[self.span(b.start,b.end)]}));
        }
        for c in &b.children {
            self.refs(c, out);
        }
    }
    fn content(&mut self, b: &Block) -> Value {
        let kind = match b.kind.as_str() {
            "paragraph" | "list" | "list-item" | "quote" | "table" | "code" => b.kind.as_str(),
            _ => "unsupported",
        };
        let mut v = json!({"kind":kind,"text":b.plain(),"origins":[self.span(b.start,b.end)]});
        if kind == "list" || kind == "quote" || kind == "list-item" {
            // Keep nested blocks and individual entries, not a flattened string only.
            let children: Vec<Value> = b
                .children
                .iter()
                .filter(|c| !matches!(c.kind.as_str(), "text" | "inline" | "link"))
                .map(|c| self.content(c))
                .collect();
            v["children"] = json!(children);
        }
        if kind == "list" {
            v["ordered"] = json!(b.ordered);
        }
        if kind == "unsupported" {
            self.diagnostic(
                "INCOMPLETE",
                format!("unsupported block: {}", b.kind),
                "mapping/content",
                b.start,
                b.end,
            );
        }
        v
    }
    fn audit_inline(&mut self, b: &Block) {
        fn unsupported_descendants<'b>(b: &'b Block, out: &mut Vec<&'b Block>) {
            for c in &b.children {
                if matches!(c.kind.as_str(), "unsupported" | "image") {
                    out.push(c);
                }
                unsupported_descendants(c, out);
            }
        }
        let mut unsupported = vec![];
        unsupported_descendants(b, &mut unsupported);
        for child in unsupported {
            self.diagnostic(
                "INCOMPLETE",
                format!("unsupported inline content: {}", child.kind),
                "mapping/content",
                child.start,
                child.end,
            );
        }
    }
    fn attach_content(&mut self, node: &mut Value, b: &Block) {
        let v = self.content(b);
        node["content"].as_array_mut().unwrap().push(v);
        let mut refs = vec![];
        self.refs(b, &mut refs);
        node["references"].as_array_mut().unwrap().extend(refs);
    }
    fn fill_paragraph(&mut self, node: &mut Value, blocks: &[Block]) {
        let mut plain_item_seen = false;
        for b in blocks {
            if self.plain_items && b.kind == "paragraph" {
                let text = b.plain();
                if let Some(c) = self.item.captures(&text) {
                    let display = group(&c, "number").to_owned();
                    let body = group(&c, "body").to_owned();
                    if let Some(number) =
                        self.numbering(&display, "mapping/plain-item", b.start, b.end)
                    {
                        let id = format!("{}/item/{}", node["id"].as_str().unwrap(), key(&number));
                        let mut item = self.node("Item", id, number, String::new(), b.start, b.end);
                        self.attach_content(&mut item, b);
                        item["content"][0]["text"] = json!(body);
                        node["children"].as_array_mut().unwrap().push(item);
                        plain_item_seen = true;
                        continue;
                    }
                } else if plain_item_seen {
                    self.diagnostic(
                        "AMBIGUOUS",
                        "unmarked content after plain item".into(),
                        "mapping/plain-item",
                        b.start,
                        b.end,
                    );
                }
            }
            if b.kind == "list" && !b.ordered {
                let matches: Vec<bool> = b
                    .children
                    .iter()
                    .map(|i| self.item.is_match(&i.plain()))
                    .collect();
                if matches.iter().any(|x| *x) && !matches.iter().all(|x| *x) {
                    self.diagnostic(
                        "AMBIGUOUS",
                        "mixed numbered and unnumbered list".into(),
                        "mapping/items",
                        b.start,
                        b.end,
                    );
                    self.attach_content(node, b);
                    continue;
                }
                if !matches.is_empty() && matches.iter().all(|x| *x) {
                    for item in &b.children {
                        let plain = item.plain();
                        let captures = self.item.captures(&plain).unwrap();
                        let display = group(&captures, "number").to_owned();
                        let body = group(&captures, "body").to_owned();
                        if let Some(num) =
                            self.numbering(&display, "mapping/item", item.start, item.end)
                        {
                            let id = format!("{}/item/{}", node["id"].as_str().unwrap(), key(&num));
                            let mut n =
                                self.node("Item", id, num, String::new(), item.start, item.end);
                            let paragraphs: Vec<_> = item
                                .children
                                .iter()
                                .filter(|c| c.kind == "paragraph")
                                .collect();
                            if paragraphs.is_empty() {
                                n["content"].as_array_mut().unwrap().push(json!({"kind":"paragraph","text":body,"origins":[self.span(item.start,item.end)]}));
                            } else {
                                for (index, p) in paragraphs.iter().enumerate() {
                                    let mut block = self.content(p);
                                    if index == 0 {
                                        let text = p.plain();
                                        if let Some(c) = self.item.captures(&text) {
                                            block["text"] = json!(group(&c, "body"));
                                        }
                                    }
                                    n["content"].as_array_mut().unwrap().push(block);
                                }
                            }
                            let mut refs = vec![];
                            self.refs(item, &mut refs);
                            n["references"] = json!(refs);
                            // Nested item lists require a richer semantic model; preserve them explicitly.
                            for child in &item.children {
                                if child.kind == "list" {
                                    self.attach_content(&mut n, child);
                                    self.diagnostic(
                                        "INCOMPLETE",
                                        "nested semantic item list is not typed".into(),
                                        "mapping/item",
                                        child.start,
                                        child.end,
                                    );
                                }
                            }
                            node["children"].as_array_mut().unwrap().push(n);
                        } else {
                            self.attach_content(node, item);
                        }
                    }
                    continue;
                }
            }
            self.attach_content(node, b);
        }
    }
    fn paragraph(
        &mut self,
        parent: &mut Value,
        blocks: &[Block],
        num: Value,
        start: usize,
        end: usize,
    ) {
        let id = format!("{}/paragraph/{}", parent["id"].as_str().unwrap(), key(&num));
        let mut n = self.node("Paragraph", id, num, String::new(), start, end);
        self.fill_paragraph(&mut n, blocks);
        parent["children"].as_array_mut().unwrap().push(n);
    }
    fn bodies(&mut self, parent: &mut Value, blocks: &[Block]) {
        if blocks.is_empty() {
            return;
        }
        if self.mode == "bare" {
            let rule = self.bare_paragraph.clone().unwrap();
            let mut pending: Vec<Block> = vec![];
            let mut number = json!({"display":null,"parts":[1],"implicit":true});
            for b in blocks {
                let text = b.plain();
                if b.kind == "paragraph"
                    && text.lines().skip(1).any(|line| {
                        rule.is_match(line) || (self.plain_items && self.item.is_match(line))
                    })
                {
                    self.diagnostic("AMBIGUOUS","bare markers inside one Markdown paragraph require explicit block boundaries".into(),"mapping/bare-paragraph",b.start,b.end);
                }
                if b.kind == "paragraph" && rule.is_match(&text) {
                    if !pending.is_empty() {
                        self.paragraph(
                            parent,
                            &pending,
                            number.clone(),
                            pending[0].start,
                            pending.last().unwrap().end,
                        );
                        pending.clear();
                    }
                    let caps = rule.captures(&text).unwrap();
                    let display = group(&caps, "number");
                    if let Some(n) =
                        self.numbering(display, "mapping/bare-paragraph", b.start, b.end)
                    {
                        number = n;
                        let mut body = b.clone();
                        body.text = group(&caps, "body").to_owned();
                        body.children.clear();
                        // Keep inline links as children for reference collection, without repeating their label in plain text.
                        fn links(b: &Block, out: &mut Vec<Block>) {
                            for c in &b.children {
                                if c.kind == "link" {
                                    let mut link = c.clone();
                                    link.kind = "preserved-link".into();
                                    out.push(link);
                                } else {
                                    links(c, out);
                                }
                            }
                        }
                        links(b, &mut body.children);
                        pending.push(body);
                    } else {
                        self.attach_content(parent, b);
                    }
                } else {
                    pending.push(b.clone());
                }
            }
            if !pending.is_empty() {
                self.paragraph(
                    parent,
                    &pending,
                    number,
                    pending[0].start,
                    pending.last().unwrap().end,
                );
            }
            return;
        }
        if self.mode == "heading" {
            self.diagnostic(
                "UNMAPPED",
                "body requires a paragraph heading in this profile".into(),
                "mapping/paragraph",
                blocks[0].start,
                blocks.last().unwrap().end,
            );
            for b in blocks {
                self.attach_content(parent, b);
            }
            return;
        }
        let mut implicit = vec![];
        let mut explicit_seen = false;
        for b in blocks {
            if b.kind == "list" && b.ordered {
                if !implicit.is_empty() {
                    let first: &Block = &implicit[0];
                    let last = implicit.last().unwrap();
                    self.paragraph(
                        parent,
                        &implicit,
                        json!({"display":null,"parts":[1],"implicit":true}),
                        first.start,
                        last.end,
                    );
                    implicit.clear();
                }
                explicit_seen = true;
                for item in &b.children {
                    let raw = &self.source[item.start..item.end];
                    let marker = raw
                        .chars()
                        .take_while(|c| c.is_ascii_digit())
                        .collect::<String>();
                    if let Some(num) =
                        self.numbering(&marker, "mapping/ordered-paragraph", item.start, item.end)
                    {
                        let mut children = item.children.clone();
                        // Tight lists have inline events directly below Item.
                        if children
                            .iter()
                            .any(|c| matches!(c.kind.as_str(), "text" | "inline" | "link"))
                        {
                            let mut p = item.clone();
                            p.kind = "paragraph".into();
                            p.children.retain(|c| c.kind != "list");
                            let nested: Vec<_> =
                                children.into_iter().filter(|c| c.kind == "list").collect();
                            children = vec![p];
                            children.extend(nested);
                        }
                        self.paragraph(parent, &children, num, item.start, item.end);
                    } else {
                        self.attach_content(parent, item);
                    }
                }
            } else {
                if explicit_seen {
                    self.diagnostic(
                        "AMBIGUOUS",
                        "body after explicit paragraph list has no paragraph owner".into(),
                        "mapping/paragraph",
                        b.start,
                        b.end,
                    );
                }
                implicit.push(b.clone());
            }
        }
        if !implicit.is_empty() {
            self.paragraph(
                parent,
                &implicit,
                json!({"display":null,"parts":[1],"implicit":true}),
                implicit[0].start,
                implicit.last().unwrap().end,
            );
        }
    }
    fn section(&mut self, s: &Section, parent: &mut Value, scope: &str) {
        let Some((kind, _rule, number, title)) = self.identify(s) else {
            let b = Block {
                kind: "unsupported".into(),
                text: self.source[s.heading.start..s.end].to_owned(),
                target: String::new(),
                level: 0,
                ordered: false,
                start: s.heading.start,
                end: s.end,
                children: vec![],
            };
            self.attach_content(parent, &b);
            return;
        };
        if kind == "Law" {
            self.law_headings += 1;
            if self.law_headings > 1 {
                self.diagnostic(
                    "LAW_TITLE_DUPLICATE",
                    "multiple law headings in one source".into(),
                    "mapping/law",
                    s.heading.start,
                    s.heading.end,
                );
            }
            if parent["kind"] != "Law" {
                self.diagnostic(
                    "PARENT_INVALID",
                    "nested law heading".into(),
                    "mapping/law",
                    s.heading.start,
                    s.heading.end,
                );
            }
            parent["title"] = json!(title);
            self.bind_anchor(s.heading.start, parent["id"].as_str().unwrap());
            let mut heading_refs = vec![];
            self.refs(&s.heading, &mut heading_refs);
            parent["references"]
                .as_array_mut()
                .unwrap()
                .extend(heading_refs);
            for b in &s.blocks {
                self.attach_content(parent, b);
            }
            for child in &s.sections {
                self.section(child, parent, scope);
            }
            return;
        }
        let owner = if kind == "Article" {
            scope
        } else {
            parent["id"].as_str().unwrap()
        };
        let id = if kind == "Supplement" {
            self.serial += 1;
            format!("{scope}/supplement/{}", self.serial)
        } else {
            format!("{owner}/{}/{}", kind.to_lowercase(), key(&number))
        };
        let mut n = self.node(&kind, id.clone(), number, title, s.heading.start, s.end);
        self.bind_anchor(s.heading.start, &id);
        let mut heading_refs = vec![];
        self.refs(&s.heading, &mut heading_refs);
        n["references"] = json!(heading_refs);
        let child_scope = if kind == "Supplement" {
            id.as_str()
        } else {
            scope
        };
        match kind.as_str() {
            "Article" | "Supplement" => {
                if kind == "Article"
                    && s.blocks.len() == 1
                    && s.blocks[0].kind == "paragraph"
                    && s.blocks[0].plain().trim() == "削除"
                {
                    n["status"] = json!("deleted");
                    self.attach_content(&mut n, &s.blocks[0]);
                } else {
                    self.bodies(&mut n, &s.blocks);
                }
            }
            "Paragraph" => self.fill_paragraph(&mut n, &s.blocks),
            _ => {
                for b in &s.blocks {
                    self.attach_content(&mut n, b);
                }
            }
        }
        for child in &s.sections {
            self.section(child, &mut n, child_scope);
        }
        parent["children"].as_array_mut().unwrap().push(n);
    }
}
fn key(n: &Value) -> String {
    n["parts"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|x| x.to_string())
                .collect::<Vec<_>>()
                .join(".")
        })
        .unwrap_or_default()
}

/// A mapping profile that has been checked.
struct Profile {
    mode: String,
    rules: Vec<Rule>,
    item: Regex,
    reference_kinds: Value,
    bare_paragraph: Option<Regex>,
    plain_items: bool,
}

/// Read a mapping profile. `Err` says what is wrong with it; nothing in a
/// profile is taken on trust, since its patterns drive the mapping.
fn read_profile(profile: &Value) -> Result<Profile, String> {
    if profile
        .get("anchor_mode")
        .and_then(Value::as_str)
        .unwrap_or("github-basic")
        != "github-basic"
    {
        return Err("unsupported anchor_mode".into());
    }
    let reference_kinds = profile
        .get("reference_target_kinds")
        .cloned()
        .unwrap_or(json!(["Article"]));
    if !reference_kinds.as_array().is_some_and(|a| {
        !a.is_empty()
            && a.iter().all(|v| {
                matches!(
                    v.as_str(),
                    Some("Law" | "Chapter" | "Article" | "Paragraph" | "Item" | "Supplement")
                )
            })
    }) {
        return Err("invalid reference_target_kinds".into());
    }
    let mode = profile["paragraph_mode"]
        .as_str()
        .ok_or("paragraph_mode missing")?
        .to_owned();
    if !matches!(mode.as_str(), "list" | "heading" | "bare") {
        return Err("unsupported paragraph_mode".into());
    }
    let bare_paragraph = if mode == "bare" {
        let regex = Regex::new(
            profile["paragraph_pattern"]
                .as_str()
                .ok_or("paragraph_pattern missing")?,
        )
        .map_err(|e| e.to_string())?;
        for name in ["number", "body"] {
            if !regex.capture_names().any(|n| n == Some(name)) {
                return Err(format!("paragraph capture missing: {name}"));
            }
        }
        Some(regex)
    } else {
        None
    };
    let mut rules = vec![];
    for r in profile["headings"].as_array().ok_or("headings missing")? {
        let kind = r["kind"].as_str().ok_or("kind missing")?;
        if !matches!(
            kind,
            "Law" | "Chapter" | "Article" | "Paragraph" | "Supplement"
        ) {
            return Err("unknown heading kind".into());
        }
        let pattern = Regex::new(r["pattern"].as_str().ok_or("pattern missing")?)
            .map_err(|e| e.to_string())?;
        if matches!(kind, "Chapter" | "Article" | "Paragraph")
            && !pattern.capture_names().any(|n| n == Some("number"))
        {
            return Err("number capture missing".into());
        }
        let levels = r["levels"]
            .as_array()
            .ok_or("levels missing")?
            .iter()
            .map(|x| {
                x.as_u64()
                    .filter(|v| (1..=6).contains(v))
                    .map(|v| v as u8)
                    .ok_or("invalid heading level".to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        rules.push(Rule {
            id: r["id"].as_str().ok_or("id missing")?.into(),
            kind: kind.into(),
            levels,
            pattern,
        });
    }
    let item = Regex::new(
        profile["item_pattern"]
            .as_str()
            .ok_or("item_pattern missing")?,
    )
    .map_err(|e| e.to_string())?;
    for name in ["number", "body"] {
        if !item.capture_names().any(|n| n == Some(name)) {
            return Err(format!("item capture missing: {name}"));
        }
    }
    Ok(Profile {
        mode,
        rules,
        item,
        reference_kinds,
        bare_paragraph,
        plain_items: profile
            .get("plain_items")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

/// Every heading of the source as an anchor, in document order.
fn heading_anchors(
    blocks: &[Block],
    path: &str,
    used: &mut std::collections::HashSet<String>,
    out: &mut Vec<Value>,
) {
    for b in blocks {
        if b.kind == "heading" {
            out.push(json!({"name":crate::references::slug(&b.plain(),used),"text":b.plain(),"node_id":null,
                "origins":[{"source":path,"start_byte":b.start,"end_byte":b.end}]}));
        }
        heading_anchors(&b.children, path, used, out);
    }
}

pub fn map(source: &str, path: &str, law_id: &str, profile: &Value) -> Result<Value, String> {
    let Profile {
        mode,
        rules,
        item,
        reference_kinds,
        bare_paragraph,
        plain_items,
    } = read_profile(profile)?;
    let mut m = Mapper {
        source,
        path,
        mode,
        rules,
        item,
        issues: vec![],
        serial: 0,
        law_headings: 0,
        anchors: vec![],
        reference_kinds,
        bare_paragraph,
        plain_items,
    };
    let blocks = parse(source);
    heading_anchors(
        &blocks,
        path,
        &mut std::collections::HashSet::new(),
        &mut m.anchors,
    );
    for block in &blocks {
        m.audit_inline(block);
    }
    let mut index = 0;
    let mut law = m.node(
        "Law",
        law_id.into(),
        Value::Null,
        String::new(),
        0,
        source.len(),
    );
    while index < blocks.len() && blocks[index].kind != "heading" {
        m.diagnostic(
            "UNMAPPED",
            "content before law heading".into(),
            "mapping/root",
            blocks[index].start,
            blocks[index].end,
        );
        m.attach_content(&mut law, &blocks[index]);
        index += 1;
    }
    let roots = sections(&blocks, &mut index, 0, source.len());
    for s in &roots {
        m.section(s, &mut law, law_id);
    }
    if law["title"].as_str().unwrap().is_empty() {
        m.diagnostic(
            "LAW_TITLE_MISSING",
            "no law title".into(),
            "mapping/root",
            0,
            source.len(),
        );
    }
    Ok(
        json!({"version":"0.1","scope":"excerpt","sources":[{"path":path,"text":source,
        "sha256":format!("{:x}",Sha256::digest(source.as_bytes())),"provenance":{"repository":"input","revision":"unspecified"}}],
        "laws":[law],"diagnostics":m.issues,"anchors":m.anchors}),
    )
}

/// Combine single-file ASTs from [`map`] into one AST (the prototype's
/// `map-corpus`). Files sharing a law `id` are merged in order; differing
/// titles for one identity are reported as `LAW_TITLE_MISMATCH`. Callers set
/// each source's `provenance` before merging.
pub fn merge(parts: &[Value]) -> Result<Value, String> {
    let mut combined = json!({"version":"0.1","scope":"excerpt","sources":[],"laws":[],"diagnostics":[],"anchors":[]});
    fn list<'v>(v: &'v Value, field: &str) -> Result<&'v Vec<Value>, String> {
        v[field]
            .as_array()
            .ok_or_else(|| format!("{field} is not an array"))
    }
    for mapped in parts {
        for field in ["sources", "diagnostics", "anchors"] {
            let items = list(mapped, field)?.clone();
            combined[field].as_array_mut().unwrap().extend(items);
        }
        let n = list(mapped, "laws")?
            .first()
            .ok_or("mapped document has no law")?;
        let existing = combined["laws"]
            .as_array()
            .unwrap()
            .iter()
            .position(|l| l["id"] == n["id"]);
        if let Some(i) = existing {
            if combined["laws"][i]["title"] != n["title"] {
                combined["diagnostics"].as_array_mut().unwrap().push(json!({"code":"LAW_TITLE_MISMATCH","severity":"error","message":"same law identity has different titles","rule_id":"mapping/law-set","node_id":n["id"],"primary_span":n["origins"][0],"related_spans":[]}));
            }
            for field in ["origins", "children", "content", "references"] {
                let items = list(n, field)?.clone();
                combined["laws"][i][field]
                    .as_array_mut()
                    .ok_or_else(|| format!("{field} is not an array"))?
                    .extend(items);
            }
        } else {
            combined["laws"].as_array_mut().unwrap().push(n.clone());
        }
    }
    Ok(combined)
}
