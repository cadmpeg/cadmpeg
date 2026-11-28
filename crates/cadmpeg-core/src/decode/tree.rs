// SPDX-License-Identifier: Apache-2.0
//! XML and JSON parsing with nesting depth charged to the decode session.
//!
//! Each parser charges the document's nesting depth through
//! [`DecodeContext::enter_nested`] before it parses, so a deep document
//! refuses with the session's recursion-depth dimension before the parser or
//! other code walks it recursively. A scan of the text gives the depth and
//! bounds the tree's storage. Each parse charges that bound, and an XML
//! parse also its name comparisons, to the session as expanded bytes against
//! the caller's ceiling on the bytes expanded in all. An XML text's DOCTYPE
//! adds what its entity references expand to before the charge, and the
//! session records what the parse leaves unread from it.

use std::collections::HashSet;
use std::ops::Range;

use super::{u64_from_index, DecodeContext, DepthGuard, ResourceDimension, ResourceFailure};
use crate::CodecError;

mod dtd;
mod xml_document;

pub use xml_document::XmlDocument;

/// One roxmltree 0.21.1 `NodeData` with the `positions` feature: four links,
/// the node kind with an inline expanded name, and the source range.
const XML_NODE_BYTES: u64 = 72;
/// One roxmltree 0.21.1 `AttributeData` with the `positions` feature.
const XML_ATTRIBUTE_BYTES: u64 = 72;
/// One roxmltree 0.21.1 `TempAttributeData`.
const XML_TEMP_ATTRIBUTE_BYTES: u64 = 80;
/// The temporary attributes roxmltree reserves before it grows their vector.
const XML_TEMP_ATTRIBUTE_SLOTS: u64 = 16;
/// One name in the scan's prefix set: a slice and its control byte, in a
/// table that keeps an eighth of its buckets free and can hold its old and
/// its doubled buckets at once.
const XML_PREFIX_BYTES: u64 = 4 * (16 + 1);
/// Bounds one roxmltree 0.21.1 `Namespace` with its sorted index.
const XML_NAMESPACE_BYTES: u64 = 64;
/// One nesting level: a slot in roxmltree's prefix stack (16 bytes) and in
/// its list of nodes that wait for a subtree link (4 bytes), and one depth
/// guard (8 bytes). A growing vector can hold its old and its doubled
/// storage at once, three times its length.
const XML_LEVEL_BYTES: u64 = 3 * (16 + 4 + 8);
/// One text piece that roxmltree keeps to merge with adjacent CDATA, a
/// `Cow<str>` in a growing vector.
const XML_TEXT_PIECE_BYTES: u64 = 3 * 24;
/// The initial text buffer and the padded shared-string header of one
/// copied text or attribute value.
const XML_COPY_BYTES: u64 = 32 + 24;
/// While roxmltree copies one text or value, it can hold the growing
/// buffer, the merged pieces, their join and two shared strings. None is
/// longer than the source of that text, and earlier copies keep one shared
/// string each.
const XML_COPY_FACTOR: u64 = 5;
/// Parser state and the initial capacities of the parser and the scan.
const XML_FIXED_BYTES: u64 = 1024 + 16 * 1024;
/// One roxmltree 0.21.1 `Entity`, a name and a value span, in a growing
/// vector.
const XML_ENTITY_BYTES: u64 = 3 * 40;
/// One 32-byte `serde_json::Value` in an array that may hold twice its
/// length.
const JSON_VALUE_BYTES: u64 = 64;
/// Bounds one `BTreeMap<String, Value>` node of `serde_json` 1.0.151 without
/// `preserve_order`: eleven keys and values, twelve edges and parent links.
const JSON_MAP_NODE_BYTES: u64 = 768;
/// The allocation of one short JSON string, with allocator rounding.
const JSON_STRING_BYTES: u64 = 32;
/// Deserializer state and initial capacities.
const JSON_FIXED_BYTES: u64 = 1024;

impl DecodeContext<'_> {
    /// Holds `depth` nesting guards for the duration of `run`.
    fn at_depth<T>(
        &self,
        depth: u64,
        operation: &'static str,
        run: impl FnOnce() -> Result<T, CodecError>,
    ) -> Result<T, CodecError> {
        let mut guards: Vec<DepthGuard<'_>> = Vec::new();
        for _ in 0..depth {
            guards.push(self.enter_nested(operation)?);
        }
        let result = run();
        drop(guards);
        result
    }

    fn tree_malformed(error: impl std::fmt::Display) -> CodecError {
        CodecError::malformed(format_args!("{error}"))
    }

    /// The session holds the document's depth, so the parser's own
    /// recursion limit does not apply.
    fn deserialize_json<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, CodecError> {
        let mut deserializer = serde_json::Deserializer::from_str(text);
        deserializer.disable_recursion_limit();
        T::deserialize(&mut deserializer)
            .and_then(|value| deserializer.end().map(|()| value))
            .map_err(Self::tree_malformed)
    }

    /// Charges the storage of a `serde_json::Value` tree of `text` as
    /// expanded bytes against the caller's ceiling on the bytes expanded in
    /// all, and returns the depth the parse can reach. A refusal is
    /// `ResourceLimit` of `scope: Session`: its `additional` is the storage
    /// bound, `u64::MAX` where the bound passes `u64`.
    fn admit_json(&self, text: &str, operation: &'static str) -> Result<u64, CodecError> {
        if let Some(limit) = self.budget.fused() {
            return Err(CodecError::ResourceLimit(limit));
        }
        let shape = json_shape(text);
        let Some(storage) = json_storage_bytes(u64_from_index(text.len()), &shape) else {
            return Err(self.refuse_tree(
                self.budget.policy().limits.max_decompressed_bytes_total,
                self.budget.decompressed_used(),
                TreeNeed::Unbounded,
                operation,
            ));
        };
        self.budget.charge_expanded(storage, operation)?;
        Ok(shape.depth)
    }

    /// Parses XML and charges its node depth. The scan bounds the storage
    /// that roxmltree reserves, copies and links and the name comparisons it
    /// makes, one byte each, with what the entity references of a DOCTYPE
    /// expand to. The session charges that bound as expanded bytes against
    /// the caller's ceiling on the bytes expanded in all, before the parse.
    /// A refusal is `ResourceLimit` of `scope: Session`: its `additional` is
    /// the bound, `u64::MAX` where the bound passes `u64`. External entities
    /// expand to empty text and no external subset is read; the session
    /// records each as an [`XmlNote`].
    pub fn parse_xml<'input>(
        &self,
        text: &'input str,
        operation: &'static str,
    ) -> Result<XmlDocument<'input>, CodecError> {
        let plan = self.admit_xml(text, operation)?;
        match self.parse_charged_xml(text, &plan, operation)? {
            Ok(tree) => {
                self.note_xml(plan.unread, operation);
                Ok(XmlDocument::original(tree))
            }
            Err(error @ roxmltree::Error::UnknownEntityReference(..))
                if plan.unread.external_subset =>
            {
                let Some(retry) = dtd::retry_insertion(text) else {
                    return Err(Self::tree_malformed(error));
                };
                let references = memchr::memchr_iter(b'&', text.as_bytes()).count();
                let charge = u64_from_index(references)
                    .checked_mul(256)
                    .and_then(|names| names.checked_add(u64_from_index(text.len()).checked_mul(2)?))
                    .and_then(|bytes| bytes.checked_add(XML_FIXED_BYTES));
                let Some(charge) = charge else {
                    return Err(self.refuse_tree(
                        self.budget.policy().limits.max_decompressed_bytes_total,
                        self.budget.decompressed_used(),
                        TreeNeed::Unbounded,
                        operation,
                    ));
                };
                self.budget.charge_expanded(charge, operation)?;
                let Some(prepared) = retry.prepare(text) else {
                    return Err(Self::tree_malformed(error));
                };
                let plan = self.admit_xml(&prepared.text, operation)?;
                let document = XmlDocument::recovered(text, prepared, |text| {
                    self.parse_charged_xml(text, &plan, operation)?
                        .map_err(|error| Self::xml_error(error, plan.node_limit(), operation))
                })?;
                self.note_xml(plan.unread, operation);
                Ok(document)
            }
            Err(error) => Err(Self::xml_error(error, plan.node_limit(), operation)),
        }
    }

    /// Parses XML text that the session reads from memory, as a decoder reads
    /// a companion file: the text's length is charged as input bytes, and the
    /// text is then parsed as [`Self::parse_xml`] parses it. Each attribute,
    /// a namespace declaration included, and each distinct namespace prefix
    /// of the text is an entity that the session admits before the parse.
    pub fn parse_input_xml<'input>(
        &self,
        text: &'input str,
        operation: &'static str,
    ) -> Result<roxmltree::Document<'input>, CodecError> {
        self.budget
            .charge_input(u64_from_index(text.len()), operation)?;
        let plan = self.admit_xml(text, operation)?;
        self.charge_entities(plan.entities, operation)?;
        let document = self
            .parse_charged_xml(text, &plan, operation)?
            .map_err(|error| Self::xml_error(error, plan.node_limit(), operation))?;
        self.note_xml(plan.unread, operation);
        Ok(document)
    }

    /// What the session's XML parses left unread from DOCTYPEs since the
    /// last call, one note per operation and cause.
    pub fn take_xml_notes(&self) -> Vec<XmlNote> {
        self.budget.take_xml_notes()
    }

    fn admit_xml(&self, text: &str, operation: &'static str) -> Result<XmlPlan, CodecError> {
        if let Some(limit) = self.budget.fused() {
            return Err(CodecError::ResourceLimit(limit));
        }
        let ceiling = self.budget.policy().limits.max_decompressed_bytes_total;
        let used = self.budget.decompressed_used();
        let fits =
            |bytes: Option<u64>| match bytes.and_then(|bytes| bytes.checked_add(XML_FIXED_BYTES)) {
                Some(bytes) if used.checked_add(bytes).is_some_and(|all| all <= ceiling) => Ok(()),
                need => Err(need.map_or(TreeNeed::Unbounded, TreeNeed::Bytes)),
            };
        let plan = plan_xml(
            text,
            || Ok(()),
            |count, analysis| {
                fits(
                    count
                        .checked_mul(XML_PREFIX_BYTES)
                        .and_then(|bytes| bytes.checked_add(analysis)),
                )
            },
            |analysis| fits(Some(analysis)),
        )
        .map_err(|stop| match stop {
            dtd::Stop::Refused(need) => self.refuse_tree(ceiling, used, need, operation),
            dtd::Stop::Unbounded => self.refuse_tree(ceiling, used, TreeNeed::Unbounded, operation),
            dtd::Stop::Malformed(reason) => Self::tree_malformed(reason),
        })?;
        self.budget.charge_expanded(plan.cost, operation)?;
        Ok(plan)
    }

    fn parse_charged_xml<'input>(
        &self,
        text: &'input str,
        plan: &XmlPlan,
        operation: &'static str,
    ) -> Result<Result<roxmltree::Document<'input>, roxmltree::Error>, CodecError> {
        let options = roxmltree::ParsingOptions {
            nodes_limit: plan.node_limit(),
            ..plan.options()
        };
        self.at_depth(plan.depth, operation, || {
            Ok(roxmltree::Document::parse_with_options(text, options))
        })
    }

    fn xml_error(error: roxmltree::Error, limit: u32, operation: &'static str) -> CodecError {
        match error {
            roxmltree::Error::NodesLimitReached => CodecError::NotImplemented(format!(
                "{operation}: the XML tree holds more than {limit} nodes"
            )),
            roxmltree::Error::EntityReferenceLoop(_) => CodecError::NotImplemented(format!(
                "{operation}: an XML entity reference expands more than 255 nested references or 10 levels"
            )),
            error => Self::tree_malformed(error),
        }
    }

    fn note_xml(&self, unread: dtd::Unread, operation: &'static str) {
        for (cause, held, references) in [
            (XmlUnread::ExternalSubset, unread.external_subset, 0),
            (XmlUnread::AttributeDefaults, unread.attribute_defaults, 0),
            (
                XmlUnread::ExternalEntity,
                unread.external_references != 0,
                unread.external_references,
            ),
        ] {
            if held {
                self.budget.note_xml(XmlNote {
                    operation,
                    unread: cause,
                    documents: 1,
                    references,
                });
            }
        }
    }

    /// Refuses a tree parse whose bound `need` passes the expanded bytes that
    /// the session has left.
    fn refuse_tree(
        &self,
        ceiling: u64,
        used: u64,
        need: TreeNeed,
        operation: &'static str,
    ) -> CodecError {
        self.budget.refuse(
            ResourceDimension::DecompressedBytes,
            ResourceFailure::BudgetExceeded,
            ceiling,
            used,
            need.saturated_bytes(),
            operation,
        )
    }

    /// Parses a JSON value tree after charging its nesting depth.
    pub fn parse_json_value(
        &self,
        text: &str,
        operation: &'static str,
    ) -> Result<serde_json::Value, CodecError> {
        let depth = self.admit_json(text, operation)?;
        self.at_depth(depth, operation, || Self::deserialize_json(text))
    }

    /// Parses a type with a derived `Deserialize` after charging the source's
    /// nesting depth. Its storage is bounded as a `serde_json::Value` tree of
    /// the same text.
    pub fn parse_json<T: serde::de::DeserializeOwned>(
        &self,
        text: &str,
        operation: &'static str,
    ) -> Result<T, CodecError> {
        let depth = self.admit_json(text, operation)?;
        self.at_depth(depth, operation, || Self::deserialize_json(text))
    }
}

impl crate::scan::ScanBudget<'_> {
    /// Parses XML after admitting its input, tree storage, name comparisons,
    /// entity expansion and nesting depth.
    pub fn parse_xml<'input>(
        &self,
        text: &'input str,
    ) -> Result<roxmltree::Document<'input>, crate::scan::StopReason> {
        use crate::scan::StopReason;

        self.source(u64_from_index(text.len()))?;
        self.retain(XML_FIXED_BYTES)?;
        let plan = plan_xml(
            text,
            || self.check(),
            |_, _| self.retain(XML_PREFIX_BYTES),
            |analysis| self.retain(analysis),
        )
        .map_err(|stop| match stop {
            dtd::Stop::Refused(reason) => reason,
            dtd::Stop::Unbounded => StopReason::RetainedBytes,
            dtd::Stop::Malformed(reason) => StopReason::Malformed(reason),
        })?;
        self.check()?;
        self.depth(plan.depth)?;
        let storage = plan
            .prefixes
            .checked_mul(XML_PREFIX_BYTES)
            .and_then(|bytes| bytes.checked_add(XML_FIXED_BYTES))
            .and_then(|bytes| bytes.checked_add(plan.analysis))
            .and_then(|held| plan.cost.checked_sub(held))
            .ok_or(StopReason::RetainedBytes)?;
        self.retain(storage)?;
        let options = roxmltree::ParsingOptions {
            nodes_limit: u32::try_from(plan.nodes).map_err(|_| StopReason::RetainedBytes)?,
            ..plan.options()
        };
        let document = roxmltree::Document::parse_with_options(text, options)
            .map_err(|error| StopReason::Malformed(error.to_string()))?;
        self.check()?;
        Ok(document)
    }
}

/// What a parse leaves unread from an XML text's DOCTYPE.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XmlUnread {
    /// The DOCTYPE names an external subset, whose declarations the parse
    /// does not read.
    ExternalSubset,
    /// The internal subset declares attribute defaults, which the parse
    /// does not apply.
    AttributeDefaults,
    /// References with an unread declaration or external replacement,
    /// which expand to empty text.
    ExternalEntity,
}

/// What the XML parses of one operation left unread, for one cause.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XmlNote {
    /// The operation that parsed the texts.
    pub operation: &'static str,
    /// What the parses left unread.
    pub unread: XmlUnread,
    /// The texts parsed with this cause.
    pub documents: u64,
    /// For [`XmlUnread::ExternalEntity`], the references that expanded to
    /// empty text.
    pub references: u64,
}

/// What a parse of an XML text holds and spends, admitted before it runs.
struct XmlPlan {
    /// The most nodes roxmltree makes.
    nodes: u64,
    /// The bytes the parse holds and spends, with the analysis and the
    /// scan's prefix set.
    cost: u64,
    /// The bytes the DOCTYPE analysis holds.
    analysis: u64,
    /// Distinct namespace prefixes.
    prefixes: u64,
    /// Attributes, namespace declarations included, and distinct namespace
    /// prefixes.
    entities: u64,
    /// The nesting levels the parse reaches.
    depth: u64,
    /// Whether the text has a DOCTYPE, which roxmltree then reads.
    doctype: bool,
    unread: dtd::Unread,
}

impl XmlPlan {
    fn node_limit(&self) -> u32 {
        let mut options = roxmltree::ParsingOptions::default();
        if let Ok(limit) = u32::try_from(self.nodes) {
            options.nodes_limit = limit;
        }
        options.nodes_limit
    }

    fn options<'input>(&self) -> roxmltree::ParsingOptions<'input> {
        roxmltree::ParsingOptions {
            allow_dtd: self.doctype,
            // An external entity expands to empty text, so the parse fetches nothing.
            entity_resolver: Some(&|_, _| Ok(Some(""))),
            ..roxmltree::ParsingOptions::default()
        }
    }
}

/// Plans a parse of `text`. `check` runs before each byte the scans read,
/// `admit_prefix` before the prefix set grows, with its new length and the
/// analysis bytes, and `admit_analysis` before the DOCTYPE analysis
/// allocates.
fn plan_xml<E>(
    text: &str,
    mut check: impl FnMut() -> Result<(), E>,
    mut admit_prefix: impl FnMut(u64, u64) -> Result<(), E>,
    admit_analysis: impl FnOnce(u64) -> Result<(), E>,
) -> Result<XmlPlan, dtd::Stop<E>> {
    let mut doctype = dtd::Doctype::read(text, admit_analysis)?;
    let analysis = doctype.as_ref().map_or(0, dtd::Doctype::analysis);
    let scope = XmlScope {
        range: 0..text.len(),
        skip: doctype
            .as_ref()
            .map_or(0..0, |doctype| doctype.range.clone()),
        open: 0,
    };
    let mut prefixes = HashSet::new();
    let counts = xml_counts_checked(
        text,
        &scope,
        &mut prefixes,
        &mut check,
        |count| admit_prefix(count, analysis),
        |name, in_text| {
            if let Some(doctype) = doctype.as_mut() {
                doctype.refer(name, in_text);
            }
        },
    )
    .map_err(dtd::Stop::Refused)?;
    let (counts, expansion, unread) = match &doctype {
        None => (counts, XmlExpansion::default(), dtd::Unread::default()),
        Some(doctype) => {
            let expanded = doctype.expand(text, &mut prefixes, &mut check, |count| {
                admit_prefix(count, analysis)
            })?;
            let mut total = counts
                .plus(&expanded.counts, 1)
                .ok_or(dtd::Stop::Unbounded)?;
            total.depth = counts
                .depth
                .checked_add(expanded.depth)
                .ok_or(dtd::Stop::Unbounded)?;
            (total, expanded.expansion, expanded.unread)
        }
    };
    let prefixes = u64_from_index(prefixes.len());
    let (nodes, cost) =
        xml_parse_cost(&counts, prefixes, &expansion).ok_or(dtd::Stop::Unbounded)?;
    Ok(XmlPlan {
        nodes,
        cost,
        analysis,
        prefixes,
        entities: counts
            .attributes
            .checked_add(prefixes)
            .ok_or(dtd::Stop::Unbounded)?,
        depth: counts.depth,
        doctype: doctype.is_some(),
        unread,
    })
}

/// The bytes a tree parse holds and spends.
#[derive(Clone, Copy)]
enum TreeNeed {
    Bytes(u64),
    /// The bound passes `u64`.
    Unbounded,
}

impl TreeNeed {
    /// The need as a refusal's `additional`, which saturates at `u64::MAX`.
    fn saturated_bytes(self) -> u64 {
        match self {
            Self::Bytes(bytes) => bytes,
            Self::Unbounded => u64::MAX,
        }
    }
}

#[derive(Clone, Copy)]
enum XmlScan {
    Text,
    Tag { quote: u8 },
    Comment,
    Cdata,
    Instruction,
    Declaration,
}

/// Upper bounds on what roxmltree reserves, copies and keeps per nesting
/// level while it parses an XML text.
#[derive(Clone, Copy, Default)]
struct XmlCounts {
    /// `<` anywhere in the text. roxmltree reserves one node for each.
    less_than: u64,
    /// `=` anywhere in the text. roxmltree reserves one attribute for each.
    equals: u64,
    /// `=` outside quoted values in tags, one per attribute.
    attributes: u64,
    /// The most attributes in one tag.
    tag_attributes: u64,
    /// The attribute pairs of each tag, summed. roxmltree compares each
    /// attribute with the earlier attributes of its tag.
    attribute_pairs: u128,
    /// `xmlns` outside quoted values in tags, one per namespace declaration.
    declarations: u64,
    /// The deepest node, counted as roxmltree counts ancestors: the root
    /// element is one, and an element, text, CDATA, comment or instruction
    /// is one more than the element that contains it.
    depth: u64,
    /// CDATA sections. roxmltree merges each with adjacent text.
    cdata: u64,
    /// Texts and attribute values that roxmltree copies: a text run with a
    /// reference, a carriage return or CDATA, and a value with a reference,
    /// a tab or a line break.
    copies: u64,
    /// The source bytes of those texts and values.
    copied: u64,
    /// Whether the text closes each element it opens, and no other, and
    /// ends outside markup.
    balanced: bool,
}

impl XmlCounts {
    fn copy(&mut self, copied: bool, source: &[u8]) {
        if copied {
            self.copies += 1;
            self.copied += u64_from_index(source.len());
        }
    }

    /// Counts a node inside `open` elements.
    fn contains(&mut self, open: u64) {
        self.depth = self.depth.max(open + 1);
    }

    /// These counts and `times` the counts of `other`; the widest tag and
    /// the deepest node of either.
    fn plus(&self, other: &Self, times: u64) -> Option<Self> {
        let sum = |own: u64, more: u64| own.checked_add(more.checked_mul(times)?);
        Some(Self {
            less_than: sum(self.less_than, other.less_than)?,
            equals: sum(self.equals, other.equals)?,
            attributes: sum(self.attributes, other.attributes)?,
            tag_attributes: self.tag_attributes.max(other.tag_attributes),
            attribute_pairs: self
                .attribute_pairs
                .checked_add(other.attribute_pairs.checked_mul(u128::from(times))?)?,
            declarations: sum(self.declarations, other.declarations)?,
            depth: self.depth.max(other.depth),
            cdata: sum(self.cdata, other.cdata)?,
            copies: sum(self.copies, other.copies)?,
            copied: sum(self.copied, other.copied)?,
            balanced: self.balanced && other.balanced,
        })
    }
}

/// What a DOCTYPE adds to a parse besides the counts of the text its
/// references expand to.
#[derive(Clone, Copy, Default)]
struct XmlExpansion {
    /// Entity references roxmltree expands, nested ones included. Each
    /// adds text pieces to a merged run.
    references: u64,
    /// Bytes of entity names roxmltree compares while it looks the
    /// references up.
    lookups: u64,
    /// Entity declarations roxmltree stores.
    declared: u64,
    /// Bytes the DOCTYPE analysis holds.
    analysis: u64,
    /// Whether expansions add attributes past roxmltree's reservation.
    attributes: bool,
}

/// The bytes a scan reads.
struct XmlScope {
    range: Range<usize>,
    /// A DOCTYPE inside `range`, of which only `<` and `=` count.
    skip: Range<usize>,
    /// Elements open where the scan starts: none in a document, one in an
    /// entity value, which expands inside an element.
    open: u64,
}

/// Adds a declared prefix, after `admit` accepts the set's new length when
/// the name is new.
fn add_prefix<'text, E>(
    prefixes: &mut HashSet<&'text [u8]>,
    name: &'text [u8],
    admit: &mut impl FnMut(u64) -> Result<(), E>,
) -> Result<(), E> {
    if !prefixes.contains(name) {
        admit(u64_from_index(prefixes.len()) + 1)?;
        prefixes.insert(name);
    }
    Ok(())
}

/// Scans the bytes of `scope` once. Comments, CDATA, processing
/// instructions and declarations end at their first terminator, never after
/// the parser's, so the counts do not fall below what the parser stores. A
/// text run ends at markup other than CDATA, where roxmltree stops merging
/// text. `check` runs before each byte, `admit_prefix` before `prefixes`
/// grows, with its new length, and `refer` at each general entity
/// reference in a text run or a quoted value, with whether it is in text.
fn xml_counts_checked<'text, E>(
    text: &'text str,
    scope: &XmlScope,
    prefixes: &mut HashSet<&'text [u8]>,
    mut check: impl FnMut() -> Result<(), E>,
    mut admit_prefix: impl FnMut(u64) -> Result<(), E>,
    mut refer: impl FnMut(&'text [u8], bool),
) -> Result<XmlCounts, E> {
    let bytes = text.as_bytes().get(..scope.range.end).unwrap_or_default();
    let mut counts = XmlCounts::default();
    let mut state = XmlScan::Text;
    let mut tag_attributes = 0_u64;
    let mut prefix_start = None::<usize>;
    // Open elements; whether the current tag closes one; its last byte
    // outside whitespace, which is `/` for an empty element.
    let mut open = scope.open;
    let mut balanced = true;
    let mut closing = false;
    let mut last = 0_u8;
    // Start of the current text run or attribute value, and whether
    // roxmltree copies it.
    let mut run_start = scope.range.start;
    let mut copied = false;
    for (index, &byte) in bytes.iter().enumerate().skip(scope.range.start) {
        check()?;
        counts.less_than += u64::from(byte == b'<');
        counts.equals += u64::from(byte == b'=');
        if scope.skip.contains(&index) {
            if index + 1 == scope.skip.end {
                run_start = scope.skip.end;
            }
            continue;
        }
        let tail = bytes.get(index..).unwrap_or_default();
        state = match state {
            XmlScan::Text if byte == b'<' => {
                if tail.starts_with(b"<![CDATA[") {
                    counts.cdata += 1;
                    counts.contains(open);
                    copied = true;
                    XmlScan::Cdata
                } else {
                    counts.copy(copied, bytes.get(run_start..index).unwrap_or_default());
                    copied = false;
                    if tail.starts_with(b"<!--") {
                        counts.contains(open);
                        XmlScan::Comment
                    } else if tail.starts_with(b"<?") {
                        counts.contains(open);
                        XmlScan::Instruction
                    } else if tail.starts_with(b"<!") {
                        XmlScan::Declaration
                    } else {
                        tag_attributes = 0;
                        closing = tail.get(1) == Some(&b'/');
                        XmlScan::Tag { quote: 0 }
                    }
                }
            }
            XmlScan::Text => {
                copied |= matches!(byte, b'&' | b'\r');
                if let Some(name) = dtd::reference(tail) {
                    refer(name, true);
                }
                // Text outside the root element is not a node.
                if open != 0 {
                    counts.contains(open);
                }
                XmlScan::Text
            }
            XmlScan::Tag { quote: 0 } => {
                let delimiter =
                    byte.is_ascii_whitespace() || matches!(byte, b'=' | b'/' | b'>' | b'\'' | b'"');
                if let Some(start) = prefix_start.filter(|_| delimiter) {
                    add_prefix(
                        prefixes,
                        bytes.get(start..index).unwrap_or_default(),
                        &mut admit_prefix,
                    )?;
                    prefix_start = None;
                }
                if tail.starts_with(b"xmlns") {
                    counts.declarations += 1;
                    if prefix_start.is_none() && tail.get(5) == Some(&b':') {
                        prefix_start = Some(index + 6);
                    }
                }
                let next = match byte {
                    b'\'' | b'"' => {
                        run_start = index + 1;
                        copied = false;
                        XmlScan::Tag { quote: byte }
                    }
                    b'>' => {
                        if closing {
                            match open.checked_sub(1).filter(|_| open > scope.open) {
                                Some(outer) => open = outer,
                                None => balanced = false,
                            }
                        } else {
                            counts.contains(open);
                            if last != b'/' {
                                open += 1;
                            }
                        }
                        run_start = index + 1;
                        XmlScan::Text
                    }
                    b'=' => {
                        counts.attribute_pairs += u128::from(tag_attributes);
                        tag_attributes += 1;
                        counts.attributes += 1;
                        counts.tag_attributes = counts.tag_attributes.max(tag_attributes);
                        XmlScan::Tag { quote: 0 }
                    }
                    _ => XmlScan::Tag { quote: 0 },
                };
                if !byte.is_ascii_whitespace() {
                    last = byte;
                }
                next
            }
            XmlScan::Tag { quote } if byte == quote => {
                counts.copy(copied, bytes.get(run_start..index).unwrap_or_default());
                copied = false;
                last = byte;
                XmlScan::Tag { quote: 0 }
            }
            XmlScan::Tag { quote } => {
                copied |= matches!(byte, b'&' | b'\t' | b'\n' | b'\r');
                if let Some(name) = dtd::reference(tail) {
                    refer(name, false);
                }
                XmlScan::Tag { quote }
            }
            XmlScan::Comment if tail.starts_with(b"-->") => {
                run_start = index + 3;
                XmlScan::Text
            }
            XmlScan::Cdata if tail.starts_with(b"]]>") => XmlScan::Text,
            XmlScan::Instruction if tail.starts_with(b"?>") => {
                run_start = index + 2;
                XmlScan::Text
            }
            XmlScan::Declaration if byte == b'>' => {
                run_start = index + 1;
                XmlScan::Text
            }
            other => other,
        };
    }
    // A text run or CDATA section that the text ends in.
    counts.copy(copied, bytes.get(run_start..).unwrap_or_default());
    if let Some(start) = prefix_start {
        add_prefix(
            prefixes,
            bytes.get(start..).unwrap_or_default(),
            &mut admit_prefix,
        )?;
    }
    counts.balanced = balanced && open == scope.open && matches!(state, XmlScan::Text);
    Ok(counts)
}

/// Bytes that roxmltree 0.21.1 and this module hold while they parse a text
/// of `counts`, besides nodes past the reserved slots. roxmltree reserves a
/// node for each `<` and an attribute for each `=` before it parses, and
/// shrinking the attribute vector copies the attributes; attributes that
/// entity expansions add grow the vector, which can then hold three slots
/// for each. Each element that declares a namespace stores its declarations
/// and copies the namespaces in scope: at most one per declared prefix, the
/// default namespace and `xml`. The temporary attributes of the widest tag
/// and the scan's prefix set grow as vectors and tables do. Each entity
/// reference adds at most two text pieces to a merged run, and the DOCTYPE
/// adds roxmltree's entity declarations and the analysis's own bytes.
fn xml_storage_bytes(counts: &XmlCounts, prefixes: u64, expansion: &XmlExpansion) -> Option<u64> {
    let namespace_indices = counts
        .declarations
        .checked_mul(prefixes.checked_add(3)?)?
        .checked_add(1)?;
    let mut attribute_slots = counts.equals.checked_add(counts.attributes)?;
    if expansion.attributes {
        attribute_slots = attribute_slots.checked_mul(3)?;
    }
    XML_FIXED_BYTES
        .checked_add(counts.less_than.checked_mul(XML_NODE_BYTES)?)?
        .checked_add(attribute_slots.checked_mul(XML_ATTRIBUTE_BYTES)?)?
        .checked_add(counts.depth.checked_add(2)?.checked_mul(XML_LEVEL_BYTES)?)?
        .checked_add(
            counts
                .cdata
                .checked_add(expansion.references)?
                .checked_mul(2)?
                .checked_add(1)?
                .checked_mul(XML_TEXT_PIECE_BYTES)?,
        )?
        .checked_add(counts.copied.checked_mul(XML_COPY_FACTOR)?)?
        .checked_add(counts.copies.checked_mul(XML_COPY_BYTES)?)?
        .checked_add(
            counts
                .declarations
                .checked_add(1)?
                .checked_mul(2 * XML_NAMESPACE_BYTES)?,
        )?
        .checked_add(namespace_indices.checked_mul(3 * 2)?)?
        .checked_add(
            counts
                .tag_attributes
                .checked_mul(3)?
                .max(XML_TEMP_ATTRIBUTE_SLOTS)
                .checked_mul(XML_TEMP_ATTRIBUTE_BYTES)?,
        )?
        .checked_add(prefixes.checked_mul(XML_PREFIX_BYTES)?)?
        .checked_add(expansion.declared.checked_mul(XML_ENTITY_BYTES)?)?
        .checked_add(expansion.analysis)
}

/// The name comparisons roxmltree 0.21.1 makes while it parses a text of
/// `counts`: each attribute with the earlier attributes of its tag, the
/// name of each tag and prefixed attribute with the namespaces in scope, at
/// most one per declared prefix, the default namespace and `xml`, and the
/// entity names it looks references up among.
fn xml_comparisons(counts: &XmlCounts, prefixes: u64, expansion: &XmlExpansion) -> Option<u64> {
    counts
        .less_than
        .checked_add(counts.attributes)?
        .checked_mul(prefixes.checked_add(2)?)?
        .checked_add(u64::try_from(counts.attribute_pairs).ok()?)?
        .checked_add(expansion.lookups)
}

/// The most nodes roxmltree 0.21.1 makes for a text of `counts`, and the
/// bytes a parse of it holds and spends: [`xml_storage_bytes`],
/// [`xml_comparisons`], and three slots for each node, which a node vector
/// that grows or shrinks can hold. roxmltree makes the root node, at most one
/// node for each `<`, and at most one text node after each `<`.
fn xml_parse_cost(
    counts: &XmlCounts,
    prefixes: u64,
    expansion: &XmlExpansion,
) -> Option<(u64, u64)> {
    let nodes = counts.less_than.checked_mul(2)?.checked_add(1)?;
    let cost = xml_storage_bytes(counts, prefixes, expansion)?
        .checked_add(xml_comparisons(counts, prefixes, expansion)?)?
        .checked_add(nodes.checked_mul(3 * XML_NODE_BYTES)?)?;
    Some((nodes, cost))
}
/// Upper bounds on a JSON parse: the nesting depth it can reach and the
/// parts of a `serde_json::Value` tree.
#[derive(Default)]
struct JsonShape {
    depth: u64,
    /// Array elements and the root value.
    values: u64,
    /// Object members.
    members: u64,
    maps: u64,
    strings: u64,
}

/// Scans `text` once, counting delimiters outside strings. With the
/// `raw_value` feature, a string that spells the private raw-value token is
/// parsed again, so its escaped delimiters can add depth and values that the
/// visible delimiters do not show; when such a string occurs, every delimiter
/// and escape counts.
fn json_shape(text: &str) -> JsonShape {
    let mut depth = 0_u64;
    let mut visible = JsonShape {
        values: 1,
        ..JsonShape::default()
    };
    let mut every = JsonShape {
        values: 1,
        ..JsonShape::default()
    };
    let mut raw = false;
    let mut string = JsonStringScan::default();
    for &byte in text.as_bytes() {
        let opening = u64::from(matches!(byte, b'[' | b'{'));
        let escape = u64::from(byte == b'\\');
        every.depth += opening + escape;
        every.values += u64::from(byte == b',') + opening + escape;
        every.members += u64::from(byte == b':') + escape;
        every.maps += u64::from(byte == b'{') + escape;
        every.strings += u64::from(byte == b'"') + escape;
        if string.active {
            raw |= string.byte(byte);
            continue;
        }
        match byte {
            b'"' => {
                visible.strings += 1;
                string = JsonStringScan {
                    active: true,
                    possible: true,
                    ..JsonStringScan::default()
                }
            }
            b'[' | b'{' => {
                depth += 1;
                visible.depth = visible.depth.max(depth);
                visible.values += 1;
                visible.maps += u64::from(byte == b'{');
            }
            b']' | b'}' if depth != 0 => depth -= 1,
            b',' => visible.values += 1,
            b':' => visible.members += 1,
            _ => {}
        }
    }
    if raw {
        every
    } else {
        visible
    }
}

/// Bytes that a `serde_json::Value` tree of `shape` may need. String
/// contents, the deserializer's scratch buffer, which may hold twice the
/// longest string, and a replayed raw value each stay within the text
/// length. Every map node but a root holds at least five members.
fn json_storage_bytes(length: u64, shape: &JsonShape) -> Option<u64> {
    let map_nodes = shape.maps.checked_add(shape.members / 5)?;
    JSON_FIXED_BYTES
        .checked_add(length.checked_mul(4)?)?
        .checked_add(shape.values.checked_mul(JSON_VALUE_BYTES)?)?
        .checked_add(map_nodes.checked_mul(JSON_MAP_NODE_BYTES)?)?
        .checked_add(shape.strings.checked_mul(JSON_STRING_BYTES)?)
}

const RAW_VALUE_TOKEN: &[u8] = b"$serde_json::private::RawValue";

#[derive(Default)]
struct JsonStringScan {
    active: bool,
    escaped: bool,
    unicode_digits: u8,
    unicode_value: u32,
    matched: usize,
    possible: bool,
}

impl JsonStringScan {
    fn decoded(&mut self, byte: Option<u8>) {
        if byte.is_some() && byte == RAW_VALUE_TOKEN.get(self.matched).copied() {
            self.matched += 1;
        } else {
            self.possible = false;
        }
    }

    /// Returns true only when a completed string spells the raw-value token.
    fn byte(&mut self, byte: u8) -> bool {
        if self.unicode_digits != 0 {
            if let Some(digit) = char::from(byte).to_digit(16) {
                self.unicode_value = self.unicode_value * 16 + digit;
            } else {
                self.possible = false;
            }
            self.unicode_digits -= 1;
            if self.unicode_digits == 0 {
                self.decoded(u8::try_from(self.unicode_value).ok());
            }
        } else if self.escaped {
            self.escaped = false;
            if byte == b'u' {
                self.unicode_digits = 4;
                self.unicode_value = 0;
            } else {
                self.decoded(match byte {
                    b'"' | b'\\' | b'/' => Some(byte),
                    b'b' => Some(8),
                    b'f' => Some(12),
                    b'n' => Some(b'\n'),
                    b'r' => Some(b'\r'),
                    b't' => Some(b'\t'),
                    _ => None,
                });
            }
        } else if byte == b'\\' {
            self.escaped = true;
        } else if byte == b'"' {
            self.active = false;
            return self.possible && self.matched == RAW_VALUE_TOKEN.len();
        } else {
            self.decoded(Some(byte));
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{
        dtd, json_shape, json_storage_bytes, plan_xml, xml_counts_checked, XmlCounts, XmlPlan,
        XmlScope, XML_FIXED_BYTES, XML_PREFIX_BYTES,
    };
    use crate::decode::{
        u64_from_index, DecodeArena, DecodeContext, DecodeDepth, DecodePolicy, LimitScope,
        ResourceDimension,
    };
    use crate::CodecError;

    /// Per-thread heap observation for the storage-bound tests.
    pub(super) mod heap {
        use std::alloc::{GlobalAlloc, Layout, System};
        use std::cell::Cell;

        use crate::decode::u64_from_index;

        thread_local! {
            static TRACK: Cell<bool> = const { Cell::new(false) };
            static LIVE: Cell<i128> = const { Cell::new(0) };
            static PEAK: Cell<i128> = const { Cell::new(0) };
        }

        struct PeakAllocator;

        #[global_allocator]
        static ALLOCATOR: PeakAllocator = PeakAllocator;

        /// A reallocation holds the old and the new block at once.
        fn record(acquired: usize, released: usize) {
            if TRACK.try_with(Cell::get).unwrap_or(false) {
                let _recorded = LIVE.try_with(|live| {
                    let held = live.get() + i128::from(u64_from_index(acquired));
                    let _peaked = PEAK.try_with(|peak| peak.set(peak.get().max(held)));
                    live.set(held - i128::from(u64_from_index(released)));
                });
            }
        }

        unsafe impl GlobalAlloc for PeakAllocator {
            unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
                let pointer = unsafe { System.alloc(layout) };
                if !pointer.is_null() {
                    record(layout.size(), 0);
                }
                pointer
            }

            unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
                let pointer = unsafe { System.alloc_zeroed(layout) };
                if !pointer.is_null() {
                    record(layout.size(), 0);
                }
                pointer
            }

            unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
                let resized = unsafe { System.realloc(pointer, layout, new_size) };
                if !resized.is_null() {
                    record(new_size, layout.size());
                }
                resized
            }

            unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
                unsafe { System.dealloc(pointer, layout) };
                record(0, layout.size());
            }
        }

        /// The most heap bytes that `action` held at once on this thread,
        /// besides what it returns.
        pub(in crate::decode::tree) fn peak<R>(action: impl FnOnce() -> R) -> (R, u64) {
            LIVE.with(|live| live.set(0));
            PEAK.with(|peak| peak.set(0));
            TRACK.with(|track| track.set(true));
            let result = action();
            TRACK.with(|track| track.set(false));
            let peak = u64::try_from(PEAK.with(Cell::get)).expect("peak fits u64");
            (result, peak)
        }
    }

    /// The counts of a scan of the whole `text` and the namespace prefixes
    /// it declares.
    fn xml_scan(text: &str) -> (XmlCounts, HashSet<&[u8]>) {
        let mut prefixes = HashSet::new();
        let scope = XmlScope {
            range: 0..text.len(),
            skip: 0..0,
            open: 0,
        };
        let counts = xml_counts_checked(
            text,
            &scope,
            &mut prefixes,
            || Ok::<(), std::convert::Infallible>(()),
            |_| Ok(()),
            |_, _| {},
        )
        .unwrap_or_else(|never| match never {});
        (counts, prefixes)
    }

    fn xml_shape(text: &str) -> XmlCounts {
        xml_scan(text).0
    }

    fn xml_plan(text: &str) -> XmlPlan {
        match plan_xml(
            text,
            || Ok::<(), std::convert::Infallible>(()),
            |_, _| Ok(()),
            |_| Ok(()),
        ) {
            Ok(plan) => plan,
            Err(dtd::Stop::Refused(never)) => match never {},
            Err(dtd::Stop::Unbounded) => panic!("unbounded XML parse"),
            Err(dtd::Stop::Malformed(reason)) => panic!("malformed DOCTYPE: {reason}"),
        }
    }

    /// What a parse of `text` charges to the session as expanded bytes.
    fn xml_cost(text: &str) -> u64 {
        xml_plan(text).cost
    }

    fn repeated(count: usize, part: impl Fn(usize) -> String) -> String {
        (0..count).map(part).collect()
    }

    /// One empty element with `count` attributes.
    fn wide_tag(count: usize) -> String {
        format!("<r{}/>", repeated(count, |index| format!(" a{index}=''")))
    }

    /// `count` elements that each declare their own namespace prefix.
    fn declared_prefixes(count: usize) -> String {
        format!(
            "<r>{}</r>",
            repeated(count, |index| format!("<c xmlns:p{index}='u'/>"))
        )
    }

    /// A FreeCAD-shaped document: indented objects with typed properties.
    fn realistic_document(objects: usize) -> String {
        let declarations = repeated(objects, |index| {
            format!("\n        <Object type=\"Part::Feature\" name=\"Box{index}\" id=\"{index}\"/>")
        });
        let data = repeated(objects, |index| {
            format!(
                "\n        <Object name=\"Box{index}\">\n            <Properties Count=\"2\">\
                 \n                <Property name=\"Label\" type=\"App::PropertyString\">\
                 \n                    <String value=\"Box &amp; {index}\"/>\
                 \n                </Property>\
                 \n                <Property name=\"Length\" type=\"App::PropertyLength\">\
                 \n                    <Float value=\"10.0\"/>\
                 \n                </Property>\n            </Properties>\n        </Object>"
            )
        });
        format!(
            "<?xml version='1.0' encoding='utf-8'?>\n<Document SchemaVersion=\"4\">\
             \n    <Objects Count=\"{objects}\">{declarations}\n    </Objects>\
             \n    <ObjectData Count=\"{objects}\">{data}\n    </ObjectData>\n</Document>\n"
        )
    }

    /// Documents whose DOCTYPE declares entities that `count` references
    /// expand, with a label each.
    fn doctype_documents(count: usize) -> Vec<(&'static str, String)> {
        let big = "y".repeat(4096);
        vec![
            (
                "a DOCTYPE without a subset",
                format!("<!DOCTYPE r>\n<r>{}</r>", "<a/>".repeat(count)),
            ),
            (
                "markup in a DOCTYPE",
                format!(
                    "<?xml version='1.0'?><!-- c --><!DOCTYPE r SYSTEM 'r.dtd' [<!-- < = -->\
                     <?p x=1?><!ELEMENT r ANY><!ATTLIST r a CDATA 'b=c'>]><r>{}</r>",
                    "<a b='1'/>".repeat(count)
                ),
            ),
            (
                "elements from entities",
                format!(
                    "<!DOCTYPE r [<!ENTITY e \"<b x='1' xmlns:q='u' q:y='2'>t&amp;\r</b>\">]>\
                     <r>{}</r>",
                    "&e;".repeat(count)
                ),
            ),
            (
                "nested entities",
                format!(
                    "<!DOCTYPE r [<!ENTITY a 'abc'><!ENTITY b '&a;&a;&a;&a;'>\
                     <!ENTITY c '&b;<![CDATA[d]]>&b;&b;&b;'>]><r>{}</r>",
                    "<p>&c;</p>".repeat(count / 4)
                ),
            ),
            (
                "entities in values",
                format!(
                    "<!DOCTYPE r [<!ENTITY v 'x&#10;y'><!ENTITY w '&v;\t&v;'>]><r>{}</r>",
                    "<a b='&w;' c='&v;'/>".repeat(count)
                ),
            ),
            (
                "external entities",
                format!(
                    "<!DOCTYPE r [<!ENTITY x SYSTEM 'x.xml'><!ENTITY p PUBLIC 'p' 'p.xml'>]>\
                     <r>{}</r>",
                    "&x;t&p;".repeat(count)
                ),
            ),
            (
                "a long value referenced often",
                format!(
                    "<!DOCTYPE r [<!ENTITY big '{big}'>]><r>{}</r>",
                    "<a>&big;</a>".repeat(count / 16)
                ),
            ),
            (
                "many declarations",
                format!(
                    "<!DOCTYPE r [{}]><r>{}</r>",
                    repeated(count, |index| format!("<!ENTITY e{index} 'v{index}'>")),
                    repeated(count, |index| format!("&e{index};"))
                ),
            ),
        ]
    }

    #[derive(Debug, serde::Deserialize, PartialEq)]
    struct JsonRecord {
        name: String,
        values: Vec<u64>,
    }

    fn with_depth<T>(depth: u64, run: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_recursion_depth = depth;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("valid fixture");
        run(&ctx)
    }

    fn is_depth_refusal<T: std::fmt::Debug>(result: &Result<T, CodecError>) -> bool {
        matches!(result, Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RecursionDepth)
    }

    #[test]
    fn xml_parses_and_rejects_malformed_text() {
        with_depth(256, |ctx| {
            let document = ctx
                .parse_xml("<r a=\"1\"><c/></r>", "XML")
                .expect("valid XML");
            assert_eq!(document.root_element().attribute("a"), Some("1"));
            assert!(matches!(
                ctx.parse_xml("<r>", "XML"),
                Err(CodecError::Malformed(_))
            ));
        });
    }

    #[test]
    fn xml_depth_is_charged_to_the_session() {
        let nested = "<a><b><c/></b></a>";
        with_depth(3, |ctx| assert!(ctx.parse_xml(nested, "XML").is_ok()));
        with_depth(2, |ctx| {
            assert!(is_depth_refusal(&ctx.parse_xml(nested, "XML")));
        });
    }

    #[test]
    fn xml_shape_counts_tag_attributes_and_declared_prefixes() {
        let (shape, prefixes) = xml_scan(
            "<?xml version=\"1.0\"?><r xmlns=\"u\" xmlns:p=\"u\" a=\"x=y\"><!-- b=c xmlns:q='u' -->\
             <p:c xmlns:p ='v' d='1'/><![CDATA[e=f]]>g=h</r>",
        );
        assert_eq!(shape.attributes, 5);
        assert_eq!(shape.tag_attributes, 3);
        // Three attributes give three pairs, and two give one.
        assert_eq!(shape.attribute_pairs, 4);
        assert_eq!(shape.declarations, 3);
        assert_eq!(prefixes.into_iter().collect::<Vec<_>>(), [&b"p"[..]]);
    }

    #[test]
    fn xml_shape_counts_reserved_slots_and_copied_text() {
        let shape = xml_shape(
            "<r a='x&amp;y' b='p\tq' c='z=w'><!-- < = -->t&lt;u<![CDATA[v]]>w<s/>\r\n</r>",
        );
        assert_eq!(shape.less_than, 6);
        assert_eq!(shape.equals, 5);
        assert_eq!(shape.cdata, 1);
        // Two values, the run that joins the CDATA, and the run with `\r`.
        assert_eq!(shape.copies, 4);
        assert_eq!(shape.copied, 7 + 3 + 20 + 2);
    }

    #[test]
    fn xml_shape_depth_matches_the_parsed_tree() {
        for text in [
            "<a/>",
            "<a>x</a>",
            "<a></a>",
            "<a><b> </b></a>",
            "<a><b/></a>",
            "<a><b></b><c><d>e</d></c></a>",
            "<!--c--><a/><?p?>",
            "<?xml version='1.0'?><a><b><!--c--></b></a>",
            "<a><b><?p x?></b></a>",
            "<a><b><![CDATA[x]]></b></a>",
            "<a b='/' c=\">\"><d e='1' /></a >",
            "<p:a xmlns:p='u'><p:b>t</p:b></p:a>",
            "<a>\r\n<b>\r\n</b>\r\n</a>\n",
        ] {
            let document = roxmltree::Document::parse(text).expect(text);
            let depth = document
                .descendants()
                .map(|node| u64_from_index(node.ancestors().count()) - 1)
                .max();
            assert_eq!(Some(xml_shape(text).depth), depth, "{text}");
        }
    }

    #[test]
    fn scan_xml_admits_input_storage_and_depth_before_parsing() {
        use crate::scan::{ManualClock, ScanBudget, ScanLimits, StopReason};

        let clock = ManualClock::default();
        let malformed = "<r><a><b>";
        for (limits, expected) in [
            (
                ScanLimits {
                    source_bytes: 1,
                    ..ScanLimits::default()
                },
                StopReason::SourceBytes,
            ),
            (
                ScanLimits {
                    retained_bytes: 1,
                    ..ScanLimits::default()
                },
                StopReason::RetainedBytes,
            ),
            (
                ScanLimits {
                    nesting_depth: 2,
                    ..ScanLimits::default()
                },
                StopReason::NestingDepth,
            ),
        ] {
            let budget = ScanBudget::new(limits, &clock);
            assert_eq!(
                budget
                    .parse_xml(malformed)
                    .expect_err("scan resource refusal"),
                expected
            );
        }
        let budget = ScanBudget::new(ScanLimits::default(), &clock);
        assert!(matches!(
            budget.parse_xml(malformed),
            Err(StopReason::Malformed(_))
        ));
    }

    #[test]
    fn scan_xml_cancellation_interrupts_the_admission_pass() {
        use crate::scan::{ScanBudget, ScanClock, ScanLimits, StopReason};
        use std::cell::Cell;
        use std::time::Duration;

        struct Clock(Cell<u64>);
        impl ScanClock for Clock {
            fn elapsed(&self) -> Duration {
                Duration::ZERO
            }
            fn is_cancelled(&self) -> bool {
                let calls = self.0.get();
                self.0.set(calls + 1);
                calls >= 16
            }
        }
        let clock = Clock(Cell::new(0));
        let budget = ScanBudget::new(ScanLimits::default(), &clock);
        let text = format!("<r>{}</r>", "<a/>".repeat(100));
        assert_eq!(
            budget.parse_xml(&text).expect_err("cancelled input pass"),
            StopReason::Cancelled
        );
        assert_eq!(budget.stop_reason(), Some(StopReason::Cancelled));
    }

    #[test]
    fn scan_xml_storage_reservation_covers_the_parse_peak() {
        use crate::scan::{ManualClock, ScanBudget, ScanLimits, StopReason};

        for text in [
            realistic_document(128),
            format!("<r>{}</r>", "a<![CDATA[b]]>&amp;<c/>".repeat(128)),
            format!(
                "<r xmlns:p='u'>{}</r>",
                "<p:a xmlns:q='v' q:b='&amp;'/>".repeat(128)
            ),
            wide_tag(1000),
            declared_prefixes(300),
        ]
        .into_iter()
        .chain(doctype_documents(128).into_iter().map(|(_, text)| text))
        {
            let storage = xml_cost(&text);
            let clock = ManualClock::default();
            let budget = ScanBudget::new(
                ScanLimits {
                    retained_bytes: storage - 1,
                    ..ScanLimits::default()
                },
                &clock,
            );
            assert_eq!(
                budget.parse_xml(&text).expect_err("tree storage refusal"),
                StopReason::RetainedBytes
            );
            let budget = ScanBudget::new(
                ScanLimits {
                    retained_bytes: storage,
                    ..ScanLimits::default()
                },
                &clock,
            );
            let (document, peak) = heap::peak(|| budget.parse_xml(&text));
            assert!(document.is_ok());
            assert!(peak <= storage, "peak {peak} exceeds reservation {storage}");
        }
    }

    #[test]
    fn xml_storage_bound_covers_the_parse_peak() {
        let count = 1 << 12;
        let root_prefixes = repeated(100, |index| format!(" xmlns:q{index}='u'"));
        let documents = [
            ("elements", format!("<r>{}</r>", "<a/>".repeat(count))),
            (
                "attributes",
                format!("<r>{}</r>", "<a b='1' c='2' d='3' e='4'/>".repeat(count)),
            ),
            (
                "equals in text",
                format!("<r>{}</r>", "=".repeat(16 * count)),
            ),
            (
                "less-than in a comment",
                format!("<r><!--{}--></r>", "<".repeat(16 * count)),
            ),
            (
                "text around CDATA",
                format!("<r>{}</r>", "x<![CDATA[y]]>".repeat(count)),
            ),
            (
                "references in text",
                format!("<r>{}</r>", "<a>&amp;&lt;</a>".repeat(count)),
            ),
            (
                "references in values",
                format!("<r>{}</r>", "<a b='&amp;x&#10;'/>".repeat(count)),
            ),
            (
                "carriage returns",
                format!("<r>{}</r>", "a\r\n".repeat(4 * count)),
            ),
            (
                "nesting",
                format!("{}{}", "<a>".repeat(count), "</a>".repeat(count)),
            ),
            (
                "namespaces in scope",
                format!(
                    "<r xmlns='u'{root_prefixes}>{}</r>",
                    "<p:a xmlns:p='v' p:b='1'/>".repeat(count / 4)
                ),
            ),
            ("realistic", realistic_document(count / 4)),
            ("a wide tag", wide_tag(count / 4)),
            ("declared prefixes", declared_prefixes(count / 4)),
        ]
        .into_iter()
        .chain(doctype_documents(count))
        .collect::<Vec<_>>();
        // roxmltree recurses once per level, past a test thread's stack.
        std::thread::Builder::new()
            .stack_size(64 << 20)
            .spawn(move || {
                with_depth(1 << 12, |ctx| {
                    for (label, text) in &documents {
                        let before = ctx.budget.decompressed_used();
                        let (document, peak) = heap::peak(|| ctx.parse_xml(text, "XML"));
                        document.expect(label);
                        let charged = ctx.budget.decompressed_used() - before;
                        assert_eq!(charged, xml_cost(text), "{label}");
                        assert!(peak <= charged, "{label}: peak {peak} > charge {charged}");
                    }
                });
            })
            .expect("spawn the parsing thread")
            .join()
            .expect("every peak within its estimate");
    }

    #[test]
    fn xml_depth_refuses_before_the_parser_recurses() {
        let levels = 512;
        let nested = format!("{}{}", "<a>".repeat(levels), "</a>".repeat(levels));
        with_depth(256, |ctx| {
            assert!(is_depth_refusal(&ctx.parse_xml(&nested, "XML")));
        });
    }

    #[test]
    fn xml_parses_many_namespace_prefixes() {
        let text = declared_prefixes(1 << 10);
        with_depth(256, |ctx| {
            let document = ctx.parse_xml(&text, "XML").expect("declared prefixes");
            assert_eq!(document.root_element().children().count(), 1 << 10);
        });
    }

    /// A session with no root input, as an encode opens, under `limits`.
    fn with_session<T>(
        limits: impl FnOnce(&mut DecodePolicy),
        run: impl FnOnce(&DecodeContext<'_>) -> T,
    ) -> T {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        limits(&mut policy);
        run(&DecodeContext::new(&arena, &policy, DecodeDepth::Full))
    }

    /// A session whose ceiling on the bytes expanded in all is `bytes`.
    fn with_expanded_ceiling<T>(bytes: u64, run: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
        with_session(
            |policy| policy.limits.max_decompressed_bytes_total = bytes,
            run,
        )
    }

    /// The `(scope, limit, used, additional)` of an expanded-bytes refusal,
    /// after it checks that the refusal fuses the session.
    fn expanded_refusal<T: std::fmt::Debug>(
        ctx: &DecodeContext<'_>,
        result: &Result<T, CodecError>,
    ) -> (LimitScope, u64, u64, u64) {
        match result {
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::DecompressedBytes =>
            {
                assert_eq!(ctx.resource_refusal(), Some(*limit));
                (limit.scope, limit.limit, limit.used, limit.additional)
            }
            other => panic!("expected an expanded-bytes refusal: {other:?}"),
        }
    }

    #[test]
    fn xml_attribute_comparisons_charge_the_session_s_expanded_bytes() {
        let wide = wide_tag(1 << 13);
        let cost = xml_cost(&wide);
        // Each attribute is compared with the earlier attributes of its tag.
        assert!(cost > (1 << 13) * ((1 << 13) - 1) / 2);
        with_expanded_ceiling(cost - 1, |ctx| {
            let refused = ctx.parse_xml(&wide, "XML");
            assert_eq!(
                expanded_refusal(ctx, &refused),
                (LimitScope::Session, cost - 1, 0, cost)
            );
        });
        with_expanded_ceiling(cost, |ctx| {
            assert!(ctx.parse_input_xml(&wide, "XML").is_ok());
            assert_eq!(ctx.budget.decompressed_used(), cost);
            let refused = ctx.parse_input_xml(&wide, "XML");
            assert_eq!(
                expanded_refusal(ctx, &refused),
                (LimitScope::Session, cost, cost, cost)
            );
        });
    }

    #[test]
    fn xml_tree_storage_charges_the_session_s_expanded_bytes() {
        let document = |elements: usize| format!("<r>{}</r>", "<a/>".repeat(elements));
        let (small, large) = (document(1 << 10), document(1 << 14));
        let ceiling = 1 << 20;
        let charged = xml_cost(&small);
        assert!(charged < ceiling && charged + xml_cost(&large) > ceiling);
        with_expanded_ceiling(ceiling, |ctx| {
            assert!(ctx.parse_xml(&small, "XML").is_ok());
            let refused = ctx.parse_xml(&large, "XML");
            assert_eq!(
                expanded_refusal(ctx, &refused),
                (LimitScope::Session, ceiling, charged, xml_cost(&large))
            );
        });
    }

    #[test]
    fn xml_prefix_scan_refuses_past_the_session_s_expanded_bytes() {
        let ceiling = XML_FIXED_BYTES + 10 * XML_PREFIX_BYTES - 1;
        with_expanded_ceiling(ceiling, |ctx| {
            let text = declared_prefixes(10);
            let refused = ctx.parse_xml(&text, "XML");
            assert_eq!(
                expanded_refusal(ctx, &refused),
                (LimitScope::Session, ceiling, 0, ceiling + 1)
            );
            // A later parse gives the fuse in force.
            let wider = declared_prefixes(20);
            let refused = ctx.parse_xml(&wider, "XML");
            assert_eq!(
                expanded_refusal(ctx, &refused),
                (LimitScope::Session, ceiling, 0, ceiling + 1)
            );
        });
    }

    #[test]
    fn input_xml_charges_its_text_to_the_session_input() {
        let text = "<r/>";
        let length = u64_from_index(text.len());
        with_session(
            |policy| policy.limits.max_input_bytes = 2 * length,
            |ctx| {
                assert!(ctx.parse_input_xml(text, "XML").is_ok());
                assert!(ctx.parse_input_xml(text, "XML").is_ok());
                let Err(CodecError::ResourceLimit(limit)) = ctx.parse_input_xml(text, "XML") else {
                    panic!("the third parse passes the input ceiling");
                };
                assert_eq!(
                    (
                        limit.dimension,
                        limit.scope,
                        limit.limit,
                        limit.used,
                        limit.additional
                    ),
                    (
                        ResourceDimension::InputBytes,
                        LimitScope::Session,
                        2 * length,
                        2 * length,
                        length
                    )
                );
            },
        );
    }

    #[test]
    fn input_xml_admits_its_attributes_and_prefixes_as_entities() {
        // Four attributes, two of them declarations, and two prefixes.
        let text = "<r xmlns:p='u' a='1'><p:c xmlns:q='v' b='2'/></r>";
        with_session(
            |policy| policy.limits.max_entities = 6,
            |ctx| {
                assert!(ctx.parse_xml(text, "XML").is_ok());
                assert!(ctx.parse_input_xml(text, "XML").is_ok());
                let Err(CodecError::ResourceLimit(limit)) = ctx.parse_input_xml(text, "XML") else {
                    panic!("the second input parse passes the entity ceiling");
                };
                assert_eq!(
                    (
                        limit.dimension,
                        limit.scope,
                        limit.limit,
                        limit.used,
                        limit.additional
                    ),
                    (ResourceDimension::Entities, LimitScope::Session, 6, 6, 6)
                );
                assert_eq!(ctx.resource_refusal(), Some(limit));
            },
        );
    }

    #[test]
    fn json_depth_follows_the_session_past_the_parser_limit() {
        let nested = format!("{}{}", "[".repeat(200), "]".repeat(200));
        with_depth(256, |ctx| {
            assert!(ctx.parse_json_value(&nested, "JSON").is_ok());
            assert!(ctx.parse_json::<serde_json::Value>(&nested, "JSON").is_ok());
        });
        with_depth(199, |ctx| {
            assert!(is_depth_refusal(&ctx.parse_json_value(&nested, "JSON")));
        });
    }

    #[test]
    fn json_tree_storage_charges_the_session_s_expanded_bytes() {
        let text = format!("[{}]", vec![r#"{"a":0}"#; 1 << 11].join(","));
        let cost = json_storage_bytes(u64_from_index(text.len()), &json_shape(&text))
            .expect("the bound fits u64");
        with_expanded_ceiling(cost - 1, |ctx| {
            let refused = ctx.parse_json_value(&text, "JSON");
            assert_eq!(
                expanded_refusal(ctx, &refused),
                (LimitScope::Session, cost - 1, 0, cost)
            );
        });
        with_expanded_ceiling(cost, |ctx| {
            assert!(ctx.parse_json::<serde_json::Value>(&text, "JSON").is_ok());
            assert_eq!(ctx.budget.decompressed_used(), cost);
            let refused = ctx.parse_json_value(&text, "JSON");
            assert_eq!(
                expanded_refusal(ctx, &refused),
                (LimitScope::Session, cost, cost, cost)
            );
        });
    }

    #[test]
    fn json_value_and_typed_parse() {
        with_depth(256, |ctx| {
            let value = ctx
                .parse_json_value(r#"{"a":[1,2]}"#, "JSON")
                .expect("valid JSON");
            assert_eq!(value, crate::json!({"a": [1, 2]}));
            let record: JsonRecord = ctx
                .parse_json(r#"{"name":"r","values":[3]}"#, "JSON")
                .expect("valid JSON");
            assert_eq!(
                record,
                JsonRecord {
                    name: "r".into(),
                    values: vec![3],
                }
            );
            assert!(matches!(
                ctx.parse_json_value("{", "JSON"),
                Err(CodecError::Malformed(_))
            ));
        });
    }

    #[test]
    fn typed_json_keeps_duplicate_field_errors() {
        with_depth(256, |ctx| {
            let text = r#"{"name":"a","name":"b","values":[]}"#;
            assert!(matches!(
                ctx.parse_json::<JsonRecord>(text, "JSON"),
                Err(CodecError::Malformed(_))
            ));
            let value = ctx.parse_json_value(text, "JSON").expect("valid JSON");
            assert_eq!(value["name"], "b");
        });
    }

    #[test]
    fn typed_json_ignores_raw_carrier_extensions() {
        with_depth(256, |ctx| {
            let text = r#"{"name":"r","values":[],"extension":{"$serde_json::private::RawValue":"not-json"}}"#;
            let record: JsonRecord = ctx.parse_json(text, "JSON").expect("valid JSON");
            assert_eq!(record.name, "r");
        });
    }

    #[test]
    fn json_shape_counts_delimiters_outside_strings_and_raw_replay() {
        assert_eq!(json_shape(r#"["[[["]"#).depth, 1);
        let shape = json_shape(r#"{"a":[[1,2]],"b":"x,y:z"}"#);
        assert_eq!(shape.depth, 3);
        assert_eq!(shape.values, 6);
        assert_eq!(shape.members, 2);
        assert_eq!(shape.maps, 1);
        assert_eq!(shape.strings, 3);
        let raw = r#"{"$serde_json::private::RawValue":"\u005b0\u005d"}"#;
        assert!(json_shape(raw).depth >= 2);
        let escaped_key = r#"{"\u0024serde_json::private::RawValue":"[0]"}"#;
        assert!(json_shape(escaped_key).depth >= 2);
        with_depth(1, |ctx| {
            assert!(is_depth_refusal(&ctx.parse_json_value(raw, "JSON")));
        });
        with_depth(256, |ctx| {
            let value = ctx
                .parse_json_value(raw, "JSON")
                .expect("valid raw carrier");
            assert_eq!(value, crate::json!([0]));
        });
    }
}
