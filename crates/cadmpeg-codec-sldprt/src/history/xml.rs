// SPDX-License-Identifier: Apache-2.0
//! Keywords XML text, read as far as an unambiguous repair allows.
//!
//! A text that does not parse gets two repairs. A namespace prefix that the
//! text does not declare is bound on the root element to a namespace that
//! carries the prefix, so that the reader keys the prefixed name as written.
//! At any other error the text is cut after the last complete markup before
//! the error, and the elements open there are closed. Each repair charges a
//! loss that states the parser error.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::ops::Range;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;

use crate::loss::SldprtLossCode;

/// Namespace that binds an undeclared prefix; the prefix follows it.
const UNBOUND_NAMESPACE: &str = "urn:cadmpeg:sldprt:unbound-prefix:";
/// Repairs tried on one history text.
const MAX_HISTORY_XML_REPAIRS: usize = 64;
const OPERATION: &str = "decode Keywords XML tree";

/// One parsed history text.
pub(super) struct HistoryXml<'text> {
    pub(super) document: roxmltree::Document<'text>,
    inserted: Option<Insertion>,
}

/// Bytes that the prefix repair inserted into the parsed text.
#[derive(Clone, Copy)]
struct Insertion {
    at: usize,
    len: usize,
}

impl Insertion {
    fn source_offset(self, offset: usize) -> usize {
        if offset >= self.at {
            offset.saturating_sub(self.len).max(self.at)
        } else {
            offset
        }
    }
}

impl HistoryXml<'_> {
    /// The offset in the member text of an offset in the parsed text.
    pub(super) fn source_offset(&self, offset: usize) -> usize {
        self.inserted
            .map_or(offset, |inserted| inserted.source_offset(offset))
    }
}

/// The element name as the text writes it.
pub(super) fn element_name<'a>(node: roxmltree::Node<'a, '_>) -> Cow<'a, str> {
    let name = node.tag_name();
    qualified(name.namespace(), name.name())
}

/// The attribute name as the text writes it.
pub(super) fn attribute_name<'a>(attribute: &roxmltree::Attribute<'a, '_>) -> Cow<'a, str> {
    qualified(attribute.namespace(), attribute.name())
}

fn qualified<'a>(namespace: Option<&'a str>, name: &'a str) -> Cow<'a, str> {
    match namespace.and_then(|uri| uri.strip_prefix(UNBOUND_NAMESPACE)) {
        Some(prefix) => Cow::Owned(format!("{prefix}:{name}")),
        None => Cow::Borrowed(name),
    }
}

/// Whether a member name is the name of a Keywords history member.
pub(super) fn names_history(name: Option<&str>) -> bool {
    name.and_then(|name| name.rsplit('/').next())
        .is_some_and(|leaf| leaf.to_ascii_lowercase().starts_with("keywords"))
}

/// Charges a history member that no element is read from.
pub(super) fn unread(losses: &mut Vec<LossNote>, member: &str, cause: &str) {
    losses.push(SldprtLossCode::HistoryUnread.note(format!(
        "Keywords member {member}: {cause}; no configuration or feature of the history is read"
    )));
}

/// Parses the text of one member. A text that does not parse is repaired
/// when the member is a history: `named` or a first element whose name
/// contains `Keywords`. `None` means that no element is read.
pub(super) fn parse<'text>(
    ctx: &DecodeContext<'_>,
    member: &str,
    named: bool,
    text: &'text str,
    repaired: &'text mut Option<String>,
    losses: &mut Vec<LossNote>,
) -> Result<Option<HistoryXml<'text>>, CodecError> {
    let failure = match parse_once(ctx, text)? {
        Parsed::Document(document) => {
            return Ok(Some(HistoryXml {
                document,
                inserted: None,
            }))
        }
        Parsed::Syntax(error) => Ok(error),
        Parsed::Refused(error) => Err(error),
    };
    let whole = scan(text, text.len());
    let history = named
        || whole
            .root_name
            .as_ref()
            .and_then(|name| text.get(name.clone()))
            .is_some_and(|name| local(name).contains("Keywords"));
    if !history {
        return Ok(None);
    }
    let error = match failure {
        Ok(error) => error,
        Err(refused) => {
            unread(losses, member, &refused.to_string());
            return Ok(None);
        }
    };
    let Some(candidate) = repair(ctx, member, text, &whole, error, losses)? else {
        return Ok(None);
    };
    let text = repaired.insert(candidate.text).as_str();
    match parse_once(ctx, text)? {
        Parsed::Document(document) => Ok(Some(HistoryXml {
            document,
            inserted: candidate.inserted,
        })),
        Parsed::Syntax(error) => {
            unread(
                losses,
                member,
                &format!("its repaired XML fails with {error}"),
            );
            Ok(None)
        }
        Parsed::Refused(error) => {
            unread(losses, member, &error.to_string());
            Ok(None)
        }
    }
}

enum Parsed<'text> {
    Document(roxmltree::Document<'text>),
    Syntax(roxmltree::Error),
    /// A refusal of this text alone by the resource limits.
    Refused(CodecError),
}

/// Parses once. A resource refusal of the session is the error.
fn parse_once<'text>(
    ctx: &DecodeContext<'_>,
    text: &'text str,
) -> Result<Parsed<'text>, CodecError> {
    match ctx.parse_xml_syntax(text, OPERATION) {
        Ok(Ok(document)) => Ok(Parsed::Document(document)),
        Ok(Err(error)) => Ok(Parsed::Syntax(error)),
        Err(error @ CodecError::ResourceLimit(_)) => Err(error),
        Err(error) => Ok(Parsed::Refused(error)),
    }
}

struct Candidate {
    text: String,
    inserted: Option<Insertion>,
    /// Offset in `text` where the inserted end tags begin.
    body_end: usize,
}

/// A cut of the member text after the last complete markup before an error.
struct Cut {
    end: usize,
    open: Vec<Range<usize>>,
    error: String,
}

fn repair(
    ctx: &DecodeContext<'_>,
    member: &str,
    text: &str,
    whole: &Scan,
    first: roxmltree::Error,
    losses: &mut Vec<LossNote>,
) -> Result<Option<Candidate>, CodecError> {
    let bindable = !text.contains(UNBOUND_NAMESPACE);
    let mut prefixes = BTreeSet::new();
    let mut prefix_error = None;
    let mut cut: Option<Cut> = None;
    let mut error = first;
    let mut candidate = Candidate {
        text: text.to_owned(),
        inserted: None,
        body_end: text.len(),
    };
    for _ in 0..MAX_HISTORY_XML_REPAIRS {
        let root_name_end = whole.root_name.as_ref().map(|name| name.end);
        match &error {
            roxmltree::Error::UnknownNamespace(prefix, _)
                if bindable
                    && root_name_end
                        .is_some_and(|end| cut.as_ref().is_none_or(|cut| end < cut.end))
                    && prefixes.insert(prefix.clone()) =>
            {
                prefix_error.get_or_insert_with(|| error.to_string());
            }
            _ => {
                let Some(limit) = error_limit(text, &candidate, cut.as_ref(), &error) else {
                    unread(losses, member, &format!("its XML fails with {error}"));
                    return Ok(None);
                };
                let mut kept = scan(text, limit);
                kept.drop_interrupted_text();
                let root_complete = kept
                    .root_name
                    .as_ref()
                    .is_some_and(|name| kept.keep > name.end);
                if !root_complete {
                    unread(
                        losses,
                        member,
                        &format!("its XML fails with {error} before the root element is complete"),
                    );
                    return Ok(None);
                }
                cut = Some(Cut {
                    end: kept.keep,
                    open: kept.open.into_iter().map(|open| open.name).collect(),
                    error: error.to_string(),
                });
            }
        }
        candidate = build(text, root_name_end, &prefixes, cut.as_ref());
        match parse_once(ctx, &candidate.text)? {
            Parsed::Document(_) => {
                charge(
                    losses,
                    member,
                    text,
                    &prefixes,
                    prefix_error.as_deref(),
                    cut.as_ref(),
                );
                return Ok(Some(candidate));
            }
            Parsed::Syntax(next) => error = next,
            Parsed::Refused(refused) => {
                unread(losses, member, &refused.to_string());
                return Ok(None);
            }
        }
    }
    unread(
        losses,
        member,
        &format!("its XML still fails with {error} after {MAX_HISTORY_XML_REPAIRS} repairs"),
    );
    Ok(None)
}

/// The member-text offset before which the next cut keeps markup. Each cut
/// keeps less than the one before, so the repair ends.
fn error_limit(
    text: &str,
    candidate: &Candidate,
    cut: Option<&Cut>,
    error: &roxmltree::Error,
) -> Option<usize> {
    let limit = match error {
        roxmltree::Error::NoRootNode
        | roxmltree::Error::UnclosedRootNode
        | roxmltree::Error::UnexpectedEndOfStream => text.len(),
        roxmltree::Error::DtdDetected
        | roxmltree::Error::NodesLimitReached
        | roxmltree::Error::AttributesLimitReached
        | roxmltree::Error::NamespacesLimitReached => return None,
        error => {
            let offset = text_offset(&candidate.text, error.pos());
            if offset >= candidate.body_end {
                text.len()
            } else {
                candidate
                    .inserted
                    .map_or(offset, |inserted| inserted.source_offset(offset))
            }
        }
    };
    match cut {
        Some(cut) => Some(limit.min(cut.end.checked_sub(1)?)),
        None => Some(limit),
    }
}

fn build(
    text: &str,
    root_name_end: Option<usize>,
    prefixes: &BTreeSet<String>,
    cut: Option<&Cut>,
) -> Candidate {
    let end = cut.map_or(text.len(), |cut| cut.end);
    let body = text.get(..end).unwrap_or(text);
    let mut out = String::with_capacity(body.len() + 64);
    let mut inserted = None;
    match root_name_end.filter(|at| !prefixes.is_empty() && *at <= body.len()) {
        Some(at) => {
            out.push_str(body.get(..at).unwrap_or_default());
            for prefix in prefixes {
                out.push_str(" xmlns:");
                out.push_str(prefix);
                out.push_str("=\"");
                out.push_str(UNBOUND_NAMESPACE);
                out.push_str(prefix);
                out.push('"');
            }
            inserted = Some(Insertion {
                at,
                len: out.len() - at,
            });
            out.push_str(body.get(at..).unwrap_or_default());
        }
        None => out.push_str(body),
    }
    let body_end = out.len();
    for open in cut.iter().flat_map(|cut| cut.open.iter().rev()) {
        out.push_str("</");
        out.push_str(text.get(open.clone()).unwrap_or_default());
        out.push('>');
    }
    Candidate {
        text: out,
        inserted,
        body_end,
    }
}

fn charge(
    losses: &mut Vec<LossNote>,
    member: &str,
    text: &str,
    prefixes: &BTreeSet<String>,
    prefix_error: Option<&str>,
    cut: Option<&Cut>,
) {
    if let Some(error) = prefix_error {
        let list = prefixes
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        losses.push(SldprtLossCode::HistoryXmlPrefixUnbound.note(format!(
            "Keywords member {member}: its XML fails with {error}; the undeclared prefixes \
             {list} are read as part of the element and attribute names that carry them"
        )));
    }
    if let Some(cut) = cut {
        let next = text
            .get(cut.end..)
            .and_then(|rest| rest.find('<').map(|at| cut.end + at))
            .map(|at| markup_head(text, at))
            .filter(|markup| !markup.is_empty());
        let unread = next.map_or_else(String::new, |markup| format!(", from `{markup}`,"));
        losses.push(SldprtLossCode::HistoryXmlTruncated.note(format!(
            "Keywords member {member}: its XML fails with {}; the history is read through \
             byte {} of {}, the {} elements open there are closed, and the text after it{unread} \
             is not read",
            cut.error,
            cut.end,
            text.len(),
            cut.open.len(),
        )));
    }
}

/// The `<` or `</` of the markup at `at` and the name after it.
fn markup_head(text: &str, at: usize) -> &str {
    let name = if text.get(at..).is_some_and(|rest| rest.starts_with("</")) {
        at + 2
    } else {
        at + 1
    };
    text.get(at..name_end(text, name)).unwrap_or_default()
}

fn local(name: &str) -> &str {
    name.rsplit_once(':').map_or(name, |(_, local)| local)
}

/// The byte offset of a parser position: a 1-based line and a 1-based
/// character column.
fn text_offset(text: &str, position: roxmltree::TextPos) -> usize {
    let row = usize::try_from(position.row.saturating_sub(1)).unwrap_or(usize::MAX);
    let column = usize::try_from(position.col.saturating_sub(1)).unwrap_or(usize::MAX);
    let line_start = text
        .split_inclusive('\n')
        .take(row)
        .map(str::len)
        .sum::<usize>();
    text.get(line_start..).map_or(text.len(), |line| {
        line_start
            + line
                .char_indices()
                .nth(column)
                .map_or(line.len(), |(offset, _)| offset)
    })
}

/// Markup of the member text that ends at or before a limit.
struct Scan {
    /// End of the last complete markup that keeps the element stack valid.
    keep: usize,
    /// The elements open at `keep`, outermost first.
    open: Vec<OpenElement>,
    /// Name of the first start tag, complete or not.
    root_name: Option<Range<usize>>,
}

struct OpenElement {
    name: Range<usize>,
    /// Offset of the `<` of its start tag.
    start: usize,
    /// It holds a complete child element.
    parent: bool,
}

impl Scan {
    /// Drops the innermost open element when it holds no complete child
    /// element: the cut interrupts its text, so its value is unknown.
    fn drop_interrupted_text(&mut self) {
        if let Some(innermost) = self.open.pop_if(|open| !open.parent) {
            self.keep = innermost.start;
        }
    }
}

enum Step {
    Other,
    Open(Range<usize>),
    Empty,
    Close(Range<usize>),
}

fn scan(text: &str, limit: usize) -> Scan {
    let mut scan = Scan {
        keep: 0,
        open: Vec::new(),
        root_name: None,
    };
    let mut at = 0;
    while let Some(start) = text
        .get(at..)
        .and_then(|rest| rest.find('<'))
        .map(|offset| at + offset)
    {
        let rest = text.get(start..).unwrap_or_default();
        let (end, step) = if rest.starts_with("<!--") {
            (find_after(text, start + 4, "-->"), Step::Other)
        } else if rest.starts_with("<![CDATA[") {
            (find_after(text, start + 9, "]]>"), Step::Other)
        } else if rest.starts_with("<?") {
            (find_after(text, start + 2, "?>"), Step::Other)
        } else if rest.starts_with("</") {
            let name = start + 2..name_end(text, start + 2);
            (find_after(text, name.end, ">"), Step::Close(name))
        } else if rest.starts_with("<!") {
            (find_after(text, start + 2, ">"), Step::Other)
        } else {
            let name = start + 1..name_end(text, start + 1);
            if name.is_empty() {
                break;
            }
            if scan.root_name.is_none() {
                scan.root_name = Some(name.clone());
            }
            match tag_end(text, name.end) {
                Some((end, true)) => (Some(end), Step::Empty),
                Some((end, false)) => (Some(end), Step::Open(name)),
                None => (None, Step::Other),
            }
        };
        let Some(end) = end.filter(|end| *end <= limit) else {
            break;
        };
        match step {
            Step::Close(name) => {
                let closes = scan
                    .open
                    .last()
                    .is_some_and(|open| text.get(open.name.clone()) == text.get(name.clone()));
                if !closes {
                    break;
                }
                scan.open.pop();
                mark_parent(&mut scan.open);
            }
            Step::Open(name) => scan.open.push(OpenElement {
                name,
                start,
                parent: false,
            }),
            Step::Empty => mark_parent(&mut scan.open),
            Step::Other => {}
        }
        scan.keep = end;
        at = end;
    }
    scan
}

fn mark_parent(open: &mut [OpenElement]) {
    if let Some(parent) = open.last_mut() {
        parent.parent = true;
    }
}

/// The offset after the first `pattern` at or after `from`.
fn find_after(text: &str, from: usize, pattern: &str) -> Option<usize> {
    text.get(from..)?
        .find(pattern)
        .map(|offset| from + offset + pattern.len())
}

fn name_end(text: &str, from: usize) -> usize {
    text.get(from..).map_or(from, |rest| {
        from + rest
            .find(|c: char| c.is_whitespace() || matches!(c, '/' | '>' | '='))
            .unwrap_or(rest.len())
    })
}

/// The offset after the `>` that ends a start tag, and whether the tag is
/// empty.
fn tag_end(text: &str, from: usize) -> Option<(usize, bool)> {
    let mut quote = None;
    let mut last = None;
    for (offset, c) in text.get(from..)?.char_indices() {
        match (quote, c) {
            (Some(open), c) if c == open => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '>') => return Some((from + offset + 1, last == Some('/'))),
            (None, c) if c.is_whitespace() => {}
            (None, c) => last = Some(c),
        }
        if quote.is_some() {
            last = None;
        }
    }
    None
}
