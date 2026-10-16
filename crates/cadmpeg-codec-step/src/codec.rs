// SPDX-License-Identifier: Apache-2.0
//! STEP codec backend and encoder.

use cadmpeg_core::container::{ContainerRole, EntryStorage, VerbatimLabel};

use std::collections::{btree_map::Entry, BTreeMap};
use std::fmt;

use cadmpeg_core::decode::DecodeDepth;
use cadmpeg_core::dialect::{DialectLayers, DialectMatch};
use cadmpeg_core::{CodecError, ContainerEntry};
use cadmpeg_ir::codec::write::{
    target::{Catalog, ResolvedWrite},
    DepthNeed, EncodeInput, EncoderBackend, ExportBody,
};
use cadmpeg_ir::codec::{CodecBackend, Confidence, Decoded, FormatId};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::ContainerSummary;

use crate::archive;
use crate::dialect::refuse_alternate_encoding;
use crate::options::{StepSchema, StepWriteOptions};
use crate::parse;
use crate::reader;

/// STEP encoder with per-export header options.
#[derive(Debug, Clone, Default)]
pub struct StepCodec {
    /// Header metadata and deterministic writer options.
    pub options: StepWriteOptions,
}

impl EncoderBackend for StepCodec {
    const FORMAT: FormatId = <Self as CodecBackend>::FORMAT;
    /// Writes B-rep, product structure, presentation and PMI; design arenas are
    /// reported as omitted, not read.
    const NEEDS: DepthNeed = DepthNeed::AtLeast(DecodeDepth::Geometry);
    type Target = Catalog;
    const TARGET: Catalog = Catalog::new(StepSchema::TARGETS, Some(2));

    /// Synthesis-only encoder. An off-catalog STEP source cannot be reproduced
    /// because every emitted schema stamps object-identifier arcs.
    fn plan_resolved(
        &self,
        input: EncodeInput<'_>,
        target: ResolvedWrite<'_>,
    ) -> Result<ExportBody, CodecError> {
        crate::writer::target::plan(self, input, &target)
    }
}

impl CodecBackend for StepCodec {
    const FORMAT: FormatId = FormatId::new(crate::dialect::FORMAT);

    fn detect_impl(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        prefix: cadmpeg_core::decode::View<'_>,
    ) -> Result<Confidence, cadmpeg_core::CodecError> {
        let view = prefix;
        let prefix = prefix.window();

        if starts_with_step_magic(prefix) {
            return Ok(Confidence::High);
        }
        if archive::has_root_marker(ctx, view)? {
            return Ok(Confidence::Medium);
        }

        if is_part26_hdf5(prefix) {
            return Ok(Confidence::Medium);
        }
        if is_part28_xml(prefix) {
            return Ok(Confidence::Medium);
        }

        if is_ap242_bo_model_xml(prefix) {
            return Ok(Confidence::Medium);
        }

        Ok(if archive::has_zip_magic(prefix) {
            Confidence::Low
        } else {
            Confidence::No
        })
    }

    fn inspect_impl(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        root: cadmpeg_core::decode::View<'_>,
    ) -> Result<ContainerSummary, CodecError> {
        let bytes = root.window();
        if archive::has_zip_magic(bytes)
            || (!starts_with_step_magic(bytes)
                && cadmpeg_container::ArchiveSnapshot::has_footer(bytes))
        {
            return inspect_zip(ctx, root);
        }
        let inspected = inspect_exchange(self, ctx, root)?;
        Ok(ContainerSummary::classified(
            DialectLayers::of(inspected.matched),
            cadmpeg_ir::ContainerKind::Iso10303ClearText,
            inspected.entries,
            inspected.losses,
            inspected.notes,
        ))
    }

    fn decode_impl(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        root: cadmpeg_core::decode::View<'_>,
    ) -> Result<Decoded, CodecError> {
        let bytes = root.window();
        if archive::has_zip_magic(bytes)
            || (!starts_with_step_magic(bytes)
                && cadmpeg_container::ArchiveSnapshot::has_footer(bytes))
        {
            return decode_zip(ctx, root);
        }
        refuse_alternate_encoding(bytes)?;
        if self.detect_impl(ctx, root)? == Confidence::No {
            return Err(CodecError::WrongFormat("missing ISO-10303-21 magic".into()));
        }
        reader::decode(bytes, ctx, reader::Packaging::Bare)
    }
}

/// One Part 21 exchange as `inspect` sees it: its single classification and
/// the container facts every packaging of it reports.
struct InspectedExchange {
    matched: DialectMatch,
    entries: Vec<ContainerEntry>,
    losses: Vec<LossNote>,
    notes: Vec<String>,
}

struct CountItem<'a> {
    name: &'a str,
    count: usize,
}

impl fmt::Display for CountItem<'_> {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(output, "{}:{}", self.name, self.count)
    }
}

/// Deep semantic analysis of a STEP exchange, exposed as container inspect.
///
/// Runs the semantic decode path to populate `unknown_entities` and related
/// attributes. Not a cheap syntactic census; see `docs/formats/step-inspect.md`.
fn inspect_exchange(
    codec: &StepCodec,
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    root: cadmpeg_core::decode::View<'_>,
) -> Result<InspectedExchange, CodecError> {
    let bytes = root.window();
    refuse_alternate_encoding(bytes)?;
    if codec.detect_impl(ctx, root)? == Confidence::No {
        return Err(CodecError::WrongFormat("missing ISO-10303-21 magic".into()));
    }
    let (mut exchange, diagnostics) = parse::parse(bytes, ctx)?;
    inspect_parsed_exchange(bytes, ctx, &mut exchange, &diagnostics)
}

fn inspect_parsed_exchange(
    bytes: &[u8],
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    exchange: &mut parse::Exchange,
    diagnostics: &[parse::ParseDiagnostic],
) -> Result<InspectedExchange, CodecError> {
    let reader::AnalyzedExchange {
        decoded,
        matched,
        opaque_offsets,
    } = reader::analyze_exchange(bytes, exchange, diagnostics, ctx)?;
    let mut entries = Vec::new();
    entries.push(ContainerEntry {
        name: "HEADER".to_owned(),
        role: ContainerRole::Metadata,
        storage: EntryStorage::unreported(VerbatimLabel::None),
        attributes: BTreeMap::default(),
    });
    if !exchange.anchors().is_empty() {
        let mut attributes = std::collections::BTreeMap::new();
        attributes.insert(
            "anchor_count".to_owned(),
            format!("{}", exchange.anchors().len()),
        );
        entries.push(ContainerEntry {
            name: "ANCHOR".to_owned(),
            role: ContainerRole::InFileAnchors,
            storage: EntryStorage::unreported(VerbatimLabel::None),
            attributes,
        });
    }
    if !exchange.references().is_empty() {
        let mut attributes = std::collections::BTreeMap::new();
        attributes.insert(
            "external_count".to_owned(),
            format!("{}", exchange.references().len()),
        );
        attributes.insert(
            "external_uris".to_owned(),
            exchange
                .references()
                .iter()
                .map(|entry| entry.uri.as_str())
                .map(std::string::ToString::to_string)
                .collect::<Vec<_>>()
                .join(","),
        );
        entries.push(ContainerEntry {
            name: "REFERENCE".to_owned(),
            role: ContainerRole::ExternalReferences,
            storage: EntryStorage::unreported(VerbatimLabel::None),
            attributes,
        });
    }
    let mut section_counts = ctx.alloc_filled(
        exchange.data().len(),
        (0_usize, BTreeMap::<&str, usize>::new()),
        "step_inspect_data_counts",
    )?;
    for record in exchange.records().values() {
        let index = exchange
            .data()
            .partition_point(|section| section.span.end <= record.span.start);
        let (entity_count, counts) = section_counts
            .get_mut(index)
            .ok_or_else(|| CodecError::malformed("STEP instance has no DATA section"))?;
        *entity_count += 1;
        if !opaque_offsets.contains(&record.span.start) {
            continue;
        }
        for partial in &record.partials {
            let name = partial.name.as_str();

            match counts.entry(name) {
                Entry::Occupied(mut entry) => *entry.get_mut() += 1,
                Entry::Vacant(entry) => {
                    entry.insert(1);
                }
            }
        }
    }
    for (index, (entity_count, counts)) in section_counts.into_iter().enumerate() {
        let unknown = counts
            .iter()
            .map(|(&name, &count)| CountItem { name, count })
            .map(|value| value.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let mut attributes = std::collections::BTreeMap::new();
        attributes.insert("entity_count".to_owned(), format!("{entity_count}"));
        attributes.insert("unknown_entities".to_owned(), unknown);
        entries.push(ContainerEntry {
            name: format!("DATA[{index}]"),
            role: ContainerRole::EntityRecords,
            storage: EntryStorage::unreported(VerbatimLabel::None),
            attributes,
        });
    }
    let external_dependencies = decoded.body.notes.iter().filter(|note| {
        note.starts_with("external document ") || note.starts_with("external source ")
    });
    let dependency_count = external_dependencies.clone().count();
    if dependency_count > 0 {
        let mut attributes = std::collections::BTreeMap::new();
        attributes.insert("dependency_count".to_owned(), format!("{dependency_count}"));
        attributes.insert(
            "dependencies".to_owned(),
            external_dependencies
                .map(String::as_str)
                .map(std::string::ToString::to_string)
                .collect::<Vec<_>>()
                .join(","),
        );
        entries.push(ContainerEntry {
            name: "EXTERNAL_DEPENDENCIES".to_owned(),
            role: ContainerRole::ExternalReferences,
            storage: EntryStorage::unreported(VerbatimLabel::None),
            attributes,
        });
    }
    for (index, signature) in exchange.signatures().iter().enumerate() {
        entries.push(ContainerEntry {
            name: if index == 0 {
                "SIGNATURE".to_owned()
            } else {
                format!("SIGNATURE[{index}]")
            },
            role: ContainerRole::Signature,
            storage: EntryStorage::verbatim(
                VerbatimLabel::None,
                cadmpeg_core::decode::u64_from_index(signature.len()),
            ),
            attributes: BTreeMap::default(),
        });
    }
    let identifiers = exchange.joined_schema_identifiers();
    let schema = if identifiers.is_empty() {
        "unspecified".into()
    } else {
        identifiers
    };
    let dialect = matched.dialect();
    let mut notes = Vec::new();
    notes.push(format!("schema {schema}; dialect {dialect}"));
    for diagnostic in diagnostics {
        let note = diagnostic.message.clone();
        notes.push(note);
    }
    Ok(InspectedExchange {
        matched,
        entries,
        losses: decoded.body.losses,
        notes,
    })
}

fn starts_with_step_magic(bytes: &[u8]) -> bool {
    starts_with_exchange_magic(bytes, b"ISO-10303-21;", false)
        || draft_exchange_offset(bytes).is_some()
}

pub(crate) fn draft_exchange_offset(bytes: &[u8]) -> Option<usize> {
    if starts_with_exchange_magic(bytes, b"STEP;", true) {
        return Some(0);
    }
    // A leading identifier ends at a comment delimiter. A complete comment
    // followed by the draft magic supplies a known exchange boundary.
    let prefix_end = bytes
        .iter()
        .take_while(|byte| byte.is_ascii_alphanumeric() || **byte == b'_')
        .count();
    (bytes.first().is_some_and(u8::is_ascii_alphabetic)
        && matches!(bytes.get(prefix_end..prefix_end + 2), Some(b"/*" | b"!*"))
        && starts_with_exchange_magic(&bytes[prefix_end..], b"STEP;", true))
    .then_some(prefix_end)
}

fn starts_with_exchange_magic(bytes: &[u8], magic: &[u8], draft: bool) -> bool {
    let mut at = 0;
    loop {
        while bytes
            .get(at)
            .is_some_and(|byte| byte.is_ascii_control() || *byte == b' ')
        {
            at += 1;
        }
        if bytes.get(at..at + 2) == Some(b"/*") || (draft && bytes.get(at..at + 2) == Some(b"!*")) {
            let close = if bytes[at] == b'/' { b"*/" } else { b"*!" };
            at += 2;
            let Some(relative_end) = bytes[at..].windows(2).position(|window| window == close)
            else {
                return false;
            };
            at += relative_end + 2;
            continue;
        }
        if bytes
            .get(at..at + 3)
            .is_some_and(|prefix| prefix == b"\\N\\" || prefix == b"\\F\\")
        {
            at += 3;
            continue;
        }
        break;
    }
    for &expected_byte in magic {
        while bytes.get(at).is_some_and(u8::is_ascii_control) {
            at += 1;
        }
        if !bytes
            .get(at)
            .is_some_and(|byte| byte.eq_ignore_ascii_case(&expected_byte))
        {
            return false;
        }
        at += 1;
    }
    true
}

fn inspect_zip(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    root: cadmpeg_core::decode::View<'_>,
) -> Result<ContainerSummary, CodecError> {
    let archive::OpenedRoot {
        archive,
        view: root_view,
        data_start: root_data_offset,
    } = archive::open_root(ctx, root)?;
    let root_bytes = root_view.window();
    refuse_alternate_encoding(root_bytes)?;
    if StepCodec::default().detect_impl(ctx, root_view)? == Confidence::No {
        return Err(CodecError::WrongFormat("missing ISO-10303-21 magic".into()));
    }
    let (mut exchange, diagnostics) = parse::parse(root_bytes, ctx)?;
    let resource_notes = archive::root_reference_notes(&archive, &exchange);
    let mut inspected = inspect_parsed_exchange(root_bytes, ctx, &mut exchange, &diagnostics)?;
    let resource_notes = resource_notes?;
    let entry_count = archive.entries().len();
    let logical_entries = inspected
        .entries
        .iter()
        .map(|entry| entry.name.as_str())
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let mut entries = archive.container_entries(ctx, archive::classify_entry)?;
    if let Some(root_entry) = entries
        .iter_mut()
        .find(|entry| entry.name == archive::ROOT_NAME)
    {
        root_entry
            .attributes
            .insert("logical_sections".to_owned(), logical_entries);
    }
    let mut notes = Vec::new();
    notes.push(format!("root {}", archive::ROOT_NAME));
    notes.push(format!(
        "archive entries={entry_count}; root data offset={root_data_offset}"
    ));
    notes.extend(std::mem::take(&mut inspected.notes));
    let mut losses = std::mem::take(&mut inspected.losses);
    losses.extend(archive::recovery::losses(&archive));
    notes.extend(resource_notes);
    // ZIP packaging is a container fact, not an identity axis: the
    // `ISO-10303.p21` root carries the FILE_SCHEMA that classifies the
    // document, so the root's own match is this summary's match.
    Ok(ContainerSummary::classified(
        DialectLayers::of(inspected.matched),
        cadmpeg_ir::ContainerKind::Iso10303Zip,
        entries,
        losses,
        notes,
    ))
}

fn decode_zip(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    root: cadmpeg_core::decode::View<'_>,
) -> Result<Decoded, CodecError> {
    let archive::OpenedRoot {
        archive,
        view: root_view,
        data_start: root_data_offset,
    } = archive::open_root(ctx, root)?;
    let (exchange, diagnostics) = parse::parse(root_view.window(), ctx)?;
    let resource_notes = archive::root_reference_notes(&archive, &exchange)?;
    let entry_count = archive.entries().len();
    let mut decoded = reader::decode_exchange(
        root_view.window(),
        exchange,
        &diagnostics,
        ctx,
        reader::Packaging::Zip {
            entry_count,
            root_data_offset,
        },
    )?;
    decoded.body.notes.push(format!(
        "container root {}; archive entries={entry_count}",
        archive::ROOT_NAME
    ));
    decoded.body.notes.extend(resource_notes);
    decoded
        .body
        .losses
        .extend(archive::recovery::losses(&archive));
    archive::recovery::retain(ctx, &archive, root, &mut decoded)?;
    Ok(decoded)
}

pub(crate) fn is_part26_hdf5(bytes: &[u8]) -> bool {
    const SIGNATURE: &[u8] = b"\x89HDF\r\n\x1a\n";
    if bytes.starts_with(SIGNATURE) {
        return true;
    }

    let mut offset = 512;
    while offset < bytes.len() {
        if bytes[offset..].starts_with(SIGNATURE) {
            return true;
        }
        let Some(next) = offset.checked_mul(2) else {
            break;
        };
        offset = next;
    }
    false
}

pub(crate) fn is_part28_xml(bytes: &[u8]) -> bool {
    let bytes = &bytes[..bytes.len().min(4096)];
    let Some((name, attributes)) = xml_root_start_tag(bytes) else {
        return false;
    };
    let local_name = name
        .iter()
        .rposition(|byte| *byte == b':')
        .map_or(name, |separator| &name[separator + 1..]);
    if local_name.eq_ignore_ascii_case(b"iso_10303_28")
        || ascii_starts_with(local_name, b"iso_10303_28_")
    {
        return true;
    }

    // A configured UOS can use a local name other than the document marker.
    // Its governing-schema namespace varies by AP, but the Part 28 common
    // namespace remains the bounded admission marker. Schema selection and
    // the derived XML Schema remain caller inputs.
    PART28_COMMON_NAMESPACES
        .iter()
        .any(|namespace| has_namespace_value(attributes, namespace))
}

const PART28_COMMON_NAMESPACES: [&[u8]; 3] = [
    b"urn:oid:1.0.10303.28.2.1.1",
    b"urn:iso:std:iso:10303:-28:ed-2:tech:XMLschema:common",
    b"urn:iso.org:standard:10303:part(28):version(2):xmlschema:common",
];

fn ascii_starts_with(value: &[u8], prefix: &[u8]) -> bool {
    value.len() >= prefix.len()
        && value[..prefix.len()]
            .iter()
            .zip(prefix)
            .all(|(value, prefix)| value.eq_ignore_ascii_case(prefix))
}

fn xml_root_start_tag(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    let mut cursor = if bytes.starts_with(b"\xef\xbb\xbf") {
        3
    } else {
        0
    };
    loop {
        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        if bytes.get(cursor) != Some(&b'<') {
            return None;
        }
        if bytes.get(cursor + 1) == Some(&b'?') {
            let end = bytes
                .get(cursor + 2..)?
                .windows(2)
                .position(|window| window == b"?>")?
                + cursor
                + 2;
            cursor = end + 2;
            continue;
        }
        if bytes.get(cursor + 1..cursor + 4) == Some(b"!--") {
            let end = bytes
                .get(cursor + 4..)?
                .windows(3)
                .position(|window| window == b"-->")?
                + cursor
                + 4;
            cursor = end + 3;
            continue;
        }
        if bytes.get(cursor + 1) == Some(&b'!') {
            let end = bytes
                .get(cursor + 2..)?
                .iter()
                .position(|byte| *byte == b'>')?
                + cursor
                + 2;
            cursor = end + 1;
            continue;
        }
        break;
    }

    let tag_end = find_xml_tag_end(bytes, cursor + 1)?;
    let mut name_end = cursor + 1;
    while bytes
        .get(name_end)
        .is_some_and(|byte| !byte.is_ascii_whitespace() && *byte != b'/' && *byte != b'>')
    {
        name_end += 1;
    }
    if name_end == cursor + 1 {
        return None;
    }
    Some((&bytes[cursor + 1..name_end], &bytes[name_end..tag_end]))
}

fn find_xml_tag_end(bytes: &[u8], mut cursor: usize) -> Option<usize> {
    let mut quote = None;
    while let Some(byte) = bytes.get(cursor).copied() {
        match quote {
            Some(delimiter) if byte == delimiter => quote = None,
            Some(_) => {}
            None if byte == b'\'' || byte == b'"' => quote = Some(byte),
            None if byte == b'>' => return Some(cursor),
            None => {}
        }
        cursor += 1;
    }
    None
}

fn has_namespace_value(attributes: &[u8], expected: &[u8]) -> bool {
    xml_attribute_value(attributes, |name, value| {
        (name == b"xmlns" || name.starts_with(b"xmlns:")) && value == expected
    })
    .is_some()
}

fn xml_attribute_value(
    attributes: &[u8],
    mut matches: impl FnMut(&[u8], &[u8]) -> bool,
) -> Option<&[u8]> {
    let mut cursor = 0;
    while cursor < attributes.len() {
        while attributes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        if attributes.get(cursor).is_none_or(|byte| *byte == b'/') {
            break;
        }
        let name_start = cursor;
        while attributes
            .get(cursor)
            .is_some_and(|byte| !byte.is_ascii_whitespace() && *byte != b'=' && *byte != b'/')
        {
            cursor += 1;
        }
        let name = &attributes[name_start..cursor];
        while attributes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        if attributes.get(cursor) != Some(&b'=') {
            return None;
        }
        cursor += 1;
        while attributes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        let delimiter = *attributes.get(cursor)?;
        if delimiter != b'\'' && delimiter != b'"' {
            return None;
        }
        cursor += 1;
        let value_start = cursor;
        while attributes
            .get(cursor)
            .is_some_and(|byte| *byte != delimiter)
        {
            cursor += 1;
        }
        let value = &attributes[value_start..cursor];
        cursor += 1;
        if matches(name, value) {
            return Some(value);
        }
    }
    None
}

pub(crate) fn is_ap242_bo_model_xml(bytes: &[u8]) -> bool {
    let bytes = &bytes[..bytes.len().min(4096)];
    let Some((name, attributes)) = xml_root_start_tag(bytes) else {
        return false;
    };
    let local_name = name
        .iter()
        .rposition(|byte| *byte == b':')
        .map_or(name, |separator| &name[separator + 1..]);
    // BM-03: the published namespace must be bound on the Uos document
    // element. Text, comments, schemaLocation values, and local names do not
    // identify the alternate encoding.
    local_name == b"Uos"
        && BO_MODEL_NAMESPACES
            .iter()
            .any(|namespace| has_namespace_value(attributes, namespace))
}

const BO_MODEL_NAMESPACES: [&[u8]; 2] = [
    b"http://standards.iso.org/iso/ts/10303/-3001/-ed-1/tech/xml-schema/bo_model",
    b"http://standards.iso.org/iso/ts/10303/-3001/-ed-2/tech/xml-schema/bo_model",
];

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use cadmpeg_core::decode::InspectOptions;

    use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions};

    use super::{starts_with_step_magic, StepCodec};

    #[test]
    fn detects_magic_after_ignored_controls_and_inside_token() {
        let source = b"\0 /* leading comment */ \\N\\ ISO-10303-\n21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
        let codec = StepCodec::default();

        assert!(starts_with_step_magic(source));
        assert_eq!(
            cadmpeg_test_support::detection::confidence(&codec, source),
            Confidence::High
        );
        codec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .expect("decode Part 21 with ignored framing octets");

        let with_bom = [b"\xEF\xBB\xBF".as_slice(), source].concat();
        assert_eq!(
            cadmpeg_test_support::detection::confidence(&codec, &with_bom),
            Confidence::No
        );
        assert!(!starts_with_step_magic(b"/* incomplete ISO-10303-21;"));
    }

    #[test]
    fn inspect_accepts_noncanonical_complex_partial_order_and_reports_a_note() {
        let bytes = include_bytes!("../tests/fixtures/noncanonical_solid_angle.p21");
        let summary = StepCodec::default()
            .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
            .expect("inspection describes recoverable source order");

        assert!(summary
            .notes
            .iter()
            .any(|note| note.contains("complex partial records are not alphabetical")));
    }
    #[test]
    fn inspection_counts_data_populations_by_source_extent() {
        let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;3');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA('first',('AP242'));#90=FIRST_ITEM();#3=SECOND_ITEM(#90);ENDSEC;DATA('second',('AP242'));#1=THIRD_ITEM();ENDSEC;END-ISO-10303-21;";
        let summary = StepCodec::default()
            .inspect(&mut Cursor::new(source), &InspectOptions::default())
            .expect("named DATA sections retain their own populations");
        let first = summary
            .entries
            .iter()
            .find(|entry| entry.name == "DATA[0]")
            .expect("first population");
        let second = summary
            .entries
            .iter()
            .find(|entry| entry.name == "DATA[1]")
            .expect("second population");
        assert_eq!(first.attributes["entity_count"], "2");
        assert_eq!(
            first.attributes["unknown_entities"],
            "FIRST_ITEM:1,SECOND_ITEM:1"
        );
        assert_eq!(second.attributes["entity_count"], "1");
        assert_eq!(second.attributes["unknown_entities"], "THIRD_ITEM:1");
    }
}
