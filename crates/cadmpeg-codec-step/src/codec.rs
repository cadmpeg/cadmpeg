// SPDX-License-Identifier: Apache-2.0
//! STEP codec backend and encoder.

use cadmpeg_core::container::{ContainerRole, EntryStorage, VerbatimLabel};

use std::collections::{btree_map::Entry, BTreeMap};
use std::fmt;

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::dialect::{DialectLayers, DialectMatch};
use cadmpeg_core::{CodecError, ContainerEntry};
use cadmpeg_ir::codec::write::{
    target::{Catalog, ResolvedWrite},
    EncodeInput, EncoderBackend, ExportBody,
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
        if starts_with_step_magic(ctx, prefix)? {
            return Ok(Confidence::High);
        }
        if archive::has_root_marker(ctx, view)? {
            return Ok(Confidence::Medium);
        }
        if is_part26_hdf5(prefix) {
            return Ok(Confidence::Medium);
        }
        if is_part28_xml(ctx, prefix)? {
            return Ok(Confidence::Medium);
        }
        if is_ap242_bo_model_xml(ctx, prefix)? {
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
        if archive::has_zip_magic(bytes) {
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
        if archive::has_zip_magic(bytes) {
            return decode_zip(ctx, root);
        }
        refuse_alternate_encoding(ctx, bytes)?;
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

fn insert_attribute(
    ctx: &DecodeContext<'_>,
    attributes: &mut BTreeMap<String, String>,
    key: &'static str,
    value: String,
) -> Result<(), CodecError> {
    let key = ctx.copy_retained_text(key, "step_inspect_attribute_key")?;
    ctx.insert_btree_map(attributes, key, value, "step_inspect_attributes")?;
    Ok(())
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
    refuse_alternate_encoding(ctx, bytes)?;
    if codec.detect_impl(ctx, root)? == Confidence::No {
        return Err(CodecError::WrongFormat("missing ISO-10303-21 magic".into()));
    }
    let mut graph_storage = ctx.reserve_scoped(0, "STEP inspect parsed graph storage")?;
    let (mut exchange, diagnostics) = parse::parse_with_context(bytes, ctx, &mut graph_storage)?;
    inspect_parsed_exchange(bytes, ctx, &mut exchange, &diagnostics, None)
}

fn inspect_parsed_exchange(
    bytes: &[u8],
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    exchange: &mut parse::Exchange,
    diagnostics: &[parse::ParseDiagnostic],
    logical_storage: Option<&mut ScopedReservation<'_>>,
) -> Result<InspectedExchange, CodecError> {
    let mut analysis_storage = ctx.reserve_scoped(0, "STEP inspection analysis storage")?;
    let analyzed = analysis_storage
        .with_storage(|| reader::analyze_exchange(bytes, exchange, diagnostics, ctx));
    let reader::AnalyzedExchange {
        decoded,
        matched,
        opaque_offsets,
    } = analyzed.or_else(|error| match error {
        CodecError::Malformed(message) => Err(CodecError::Malformed(
            ctx.copy_retained_text(&message, "STEP inspection analysis error")?,
        )),
        error => Err(error),
    })?;
    let matched = matched.try_clone_for_decode(ctx, "STEP inspection retained dialect")?;
    let build_entries = || inspect_logical_entries(ctx, exchange, &opaque_offsets, &decoded.body.notes);
    let entries = match logical_storage {
        Some(storage) => storage.with_storage(build_entries),
        None => build_entries(),
    }.or_else(|error| match error {
        CodecError::Malformed(message) => Err(CodecError::Malformed(
            ctx.copy_retained_text(&message, "STEP inspection logical entry error")?,
        )),
        error => Err(error),
    })?;
    let (_schema_storage, schema) =
        ctx.with_scoped_storage("STEP inspect schema text storage", || {
            let identifiers = exchange.joined_schema_identifiers(ctx)?;
            if identifiers.is_empty() {
                ctx.copy_retained_text("unspecified", "STEP inspect unspecified schema text")
            } else {
                Ok(identifiers)
            }
        }).map(|(schema, storage)| (storage, schema))?;
    let dialect = matched.dialect();
    let mut notes = Vec::new();
    ctx.push_vec(
        &mut notes,
        ctx.format_retained(
            format_args!("schema {schema}; dialect {dialect}"),
            "step_inspect_schema_note",
        )?,
        "step_codec_notes",
    )?;
    let mut visited_items = (diagnostics).iter();
    ctx.charge_work(0, "STEP inspect parsed exchange traversal")?;
    for _ in 0..visited_items.len() {
        let diagnostic = ctx.next_charged(&mut visited_items, "STEP inspect parsed exchange traversal")?
            .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
        let note = ctx.format_retained(
            format_args!("{}", diagnostic.message),
            "step_inspect_diagnostic_copy",
        )?;
        ctx.push_vec(&mut notes, note, "step_codec_notes")?;
    }
    let losses = ctx.try_collect_retained_with(
        decoded.body.losses.iter(),
        "STEP inspection retained loss slots",
        |loss| loss.try_clone_for_decode(ctx, "STEP inspection retained loss"),
    )?;
    Ok(InspectedExchange {
        matched,
        entries,
        losses,
        notes,
    })
}

fn inspect_logical_entries(
    ctx: &DecodeContext<'_>,
    exchange: &parse::Exchange,
    opaque_offsets: &std::collections::BTreeSet<usize>,
    dependency_notes: &[String],
) -> Result<Vec<ContainerEntry>, CodecError> {
    let mut entries = Vec::new();
    ctx.push_vec(
        &mut entries,
        ContainerEntry {
            name: ctx.copy_retained_text("HEADER", "step_inspect_section_label")?,
            role: ContainerRole::Metadata,
            storage: EntryStorage::unreported(VerbatimLabel::None),
            attributes: BTreeMap::default(),
        },
        "step_inspect_entries",
    )?;
    if !exchange.anchors().is_empty() {
        let mut attributes = std::collections::BTreeMap::new();
        insert_attribute(
            ctx,
            &mut attributes,
            "anchor_count",
            ctx.format_retained(
                format_args!("{}", exchange.anchors().len()),
                "step_inspect_attribute_count",
            )?,
        )?;
        ctx.push_vec(
            &mut entries,
            ContainerEntry {
                name: ctx.copy_retained_text("ANCHOR", "step_inspect_section_label")?,
                role: ContainerRole::InFileAnchors,
                storage: EntryStorage::unreported(VerbatimLabel::None),
                attributes,
            },
            "step_inspect_entries",
        )?;
    }
    if !exchange.references().is_empty() {
        let mut attributes = std::collections::BTreeMap::new();
        insert_attribute(
            ctx,
            &mut attributes,
            "external_count",
            ctx.format_retained(
                format_args!("{}", exchange.references().len()),
                "step_inspect_attribute_count",
            )?,
        )?;
        insert_attribute(
            ctx,
            &mut attributes,
            "external_uris",
            ctx.join_display_retained(
                exchange.references().iter().map(|entry| entry.uri.as_str()),
                ",",
                "step_inspect_external_uris",
            )?,
        )?;
        ctx.push_vec(
            &mut entries,
            ContainerEntry {
                name: ctx.copy_retained_text("REFERENCE", "step_inspect_section_label")?,
                role: ContainerRole::ExternalReferences,
                storage: EntryStorage::unreported(VerbatimLabel::None),
                attributes,
            },
            "step_inspect_entries",
        )?;
    }
    let mut visited_items = (exchange.data()).iter().enumerate();
    ctx.charge_work(0, "STEP inspect parsed exchange traversal")?;
    for _ in 0..visited_items.len() {
        let (index, section) = ctx.next_charged(&mut visited_items, "STEP inspect parsed exchange traversal")?
            .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
        let mut counts_storage = ctx.reserve_scoped(0, "STEP inspect unknown count storage")?;
        let mut counts = BTreeMap::<&str, usize>::new();
        let mut visited_items = ((section.records)[..]).iter();
        ctx.charge_work(0, "STEP inspect parsed exchange traversal")?;
        for _ in 0..visited_items.len() {
            let id = ctx.next_charged(&mut visited_items, "STEP inspect parsed exchange traversal")?
                .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
            let record = ctx
                .get_btree_map(exchange.records(), id, "STEP inspect record lookup")?
                .ok_or_else(|| CodecError::malformed("DATA section names no record"))?;
            if !ctx.contains_btree_set(
                opaque_offsets,
                &record.span.start,
                "STEP inspect opaque offset lookup",
            )? {
                continue;
            }
            let mut visited_items = (record.partials[..]).iter();
            ctx.charge_work(0, "STEP inspect parsed exchange traversal")?;
            for _ in 0..visited_items.len() {
                let partial = ctx.next_charged(&mut visited_items, "STEP inspect parsed exchange traversal")?
                    .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
                let name = partial.name.as_str();
                counts_storage.with_storage(|| {
                    match ctx.entry_btree_map(&mut counts, name, "step_inspect_unknown_counts")? {
                        Entry::Occupied(mut entry) => *entry.get_mut() += 1,
                        Entry::Vacant(entry) => {
                            entry.insert(1);
                        }
                    }
                    Ok::<(), CodecError>(())
                })?;
            }
        }
        let unknown = ctx.join_display_retained(
            counts
                .iter()
                .map(|(&name, &count)| CountItem { name, count }),
            ",",
            "step_inspect_unknown_entities",
        )?;
        let mut attributes = std::collections::BTreeMap::new();
        insert_attribute(
            ctx,
            &mut attributes,
            "entity_count",
            ctx.format_retained(
                format_args!("{}", section.records.len()),
                "step_inspect_attribute_count",
            )?,
        )?;
        insert_attribute(ctx, &mut attributes, "unknown_entities", unknown)?;
        ctx.push_vec(
            &mut entries,
            ContainerEntry {
                name: ctx
                    .format_retained(format_args!("DATA[{index}]"), "step_inspect_section_name")?,
                role: ContainerRole::EntityRecords,
                storage: EntryStorage::unreported(VerbatimLabel::None),
                attributes,
            },
            "step_inspect_entries",
        )?;
    }
    let (dependency_buffer, mut dependency_storage) =
        ctx.temporary_vec(0, "STEP inspect dependency storage")?;
    let mut external_dependencies = dependency_buffer;
    let mut notes = dependency_notes.iter();
    ctx.charge_work(0, "STEP inspect dependency traversal")?;
    for _ in 0..notes.len() {
        let note = ctx.next_charged(&mut notes, "STEP inspect dependency traversal")?
            .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
        if note.starts_with("external document ") || note.starts_with("external source ") {
            ctx.push_scoped_vec(
                &mut dependency_storage, &mut external_dependencies, note.as_str(),
                "STEP inspect dependency slots",
            )?;
        }
    }
    let dependency_count = external_dependencies.len();
    if dependency_count > 0 {
        let mut attributes = std::collections::BTreeMap::new();
        insert_attribute(
            ctx,
            &mut attributes,
            "dependency_count",
            ctx.format_retained(
                format_args!("{dependency_count}"),
                "step_inspect_attribute_count",
            )?,
        )?;
        insert_attribute(
            ctx,
            &mut attributes,
            "dependencies",
            ctx.join_display_retained(
                external_dependencies.iter().copied(),
                ",",
                "step_inspect_dependency_text",
            )?,
        )?;
        ctx.push_vec(
            &mut entries,
            ContainerEntry {
                name: ctx
                    .copy_retained_text("EXTERNAL_DEPENDENCIES", "step_inspect_section_label")?,
                role: ContainerRole::ExternalReferences,
                storage: EntryStorage::unreported(VerbatimLabel::None),
                attributes,
            },
            "step_inspect_entries",
        )?;
    }
    let mut visited_items = (exchange.signatures()).iter().enumerate();
    ctx.charge_work(0, "STEP inspect parsed exchange traversal")?;
    for _ in 0..visited_items.len() {
        let (index, signature) = ctx.next_charged(&mut visited_items, "STEP inspect parsed exchange traversal")?
            .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
        ctx.push_vec(
            &mut entries,
            ContainerEntry {
                name: if index == 0 {
                    ctx.copy_retained_text("SIGNATURE", "step_inspect_signature_name")?
                } else {
                    ctx.format_retained(
                        format_args!("SIGNATURE[{index}]"),
                        "step_inspect_signature_indexed_name",
                    )?
                },
                role: ContainerRole::Signature,
                storage: EntryStorage::verbatim(
                    VerbatimLabel::None,
                    cadmpeg_core::decode::u64_from_index(signature.len()),
                ),
                attributes: BTreeMap::default(),
            },
            "step_inspect_entries",
        )?;
    }
    Ok(entries)
}

fn starts_with_step_magic(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<bool, CodecError> {
    let mut at = 0;
    ctx.charge_work(0, "STEP magic cursor traversal")?;
    while at < bytes.len() {
        ctx.charge_work(1, "STEP magic cursor traversal")?;
        while bytes
            .get(at)
            .is_some_and(|byte| byte.is_ascii_control() || *byte == b' ')
        {
            ctx.charge_work(1, "STEP magic cursor traversal")?;
            at += 1;
        }
        if bytes.get(at..at + 2) == Some(b"/*") {
            at += 2;
            let Some(relative_end) = ctx.position_by(
                bytes[at..].windows(2),
                |window| Ok(window == b"*/"),
                "STEP leading comment traversal",
            )?
            else {
                return Ok(false);
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
    for &expected_byte in b"ISO-10303-21;" {
        while bytes.get(at).is_some_and(u8::is_ascii_control) {
            ctx.charge_work(1, "STEP magic cursor traversal")?;
            at += 1;
        }
        if !bytes
            .get(at)
            .is_some_and(|byte| byte.eq_ignore_ascii_case(&expected_byte))
        {
            return Ok(false);
        }
        at += 1;
    }
    Ok(true)
}

fn inspect_zip(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    root: cadmpeg_core::decode::View<'_>,
) -> Result<ContainerSummary, CodecError> {
    let opened = archive::open_root(ctx, root)?;
    let archive = &opened.archive;
    let root_view = opened.view;
    let root_data_offset = opened.data_start;
    let root_bytes = root_view.window();
    refuse_alternate_encoding(ctx, root_bytes)?;
    if StepCodec::default().detect_impl(ctx, root_view)? == Confidence::No {
        return Err(CodecError::WrongFormat("missing ISO-10303-21 magic".into()));
    }
    let mut graph_storage = ctx.reserve_scoped(0, "STEP inspect parsed graph storage")?;
    let (mut exchange, diagnostics) = parse::parse_with_context(root_bytes, ctx, &mut graph_storage)?;
    let resource_notes = archive::root_reference_notes(ctx, archive, &exchange);
    let mut logical_storage = ctx.reserve_scoped(0, "STEP ZIP logical summary storage")?;
    let mut inspected = inspect_parsed_exchange(
        root_bytes, ctx, &mut exchange, &diagnostics, Some(&mut logical_storage),
    )?;
    let resource_notes = resource_notes?;
    let entry_count = archive.entries().len();
    let logical_entries = ctx.join_display_retained(
        inspected.entries.iter().map(|entry| entry.name.as_str()),
        ",",
        "step_inspect_logical_sections",
    )?;
    drop(std::mem::take(&mut inspected.entries));
    drop(logical_storage);
    let mut entries = archive.container_entries(ctx, |_| ContainerRole::Ancillary)?;
    for entry in ctx.admit_iter(entries.as_mut_slice(), "STEP ZIP role classification visits")? {
        entry.role = archive::classify_entry(ctx, &entry.name)?;
    }
    if let Some(root_entry) = ctx.find_by(
        entries.iter_mut(),
        |entry| Ok(entry.name == archive::ROOT_NAME),
        "STEP inspect root entry search",
    )? {
        insert_attribute(
            ctx,
            &mut root_entry.attributes,
            "logical_sections",
            logical_entries,
        )?;
    }
    let mut notes = Vec::new();
    {
        ctx.push_formatted_retained(
            &mut notes,
            format_args!("root {}", archive::ROOT_NAME),
            "step_codec_notes",
            "step_codec_root_note",
        )?;
        ctx.push_formatted_retained(
            &mut notes,
            format_args!("archive entries={entry_count}; root data offset={root_data_offset}"),
            "step_codec_notes",
            "step_codec_archive_note",
        )
    }?;
    ctx.extend_vec(
        &mut notes,
        std::mem::take(&mut inspected.notes),
        "step_codec_notes",
    )?;
    let losses = std::mem::take(&mut inspected.losses);
    ctx.extend_vec(&mut notes, resource_notes, "step_codec_notes")?;
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
    let opened = archive::open_root(ctx, root)?;
    let archive = &opened.archive;
    let root_view = opened.view;
    let root_data_offset = opened.data_start;
    let mut graph_storage = ctx.reserve_scoped(0, "STEP ZIP parsed graph storage")?;
    let (exchange, diagnostics) = parse::parse_with_context(root_view.window(), ctx, &mut graph_storage)?;
    let resource_notes = archive::root_reference_notes(ctx, archive, &exchange)?;
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
    ctx.push_formatted_retained(
        &mut decoded.body.notes,
        format_args!(
            "container root {}; archive entries={entry_count}",
            archive::ROOT_NAME
        ),
        "step_codec_notes",
        "step_codec_container_note",
    )?;
    ctx.extend_vec(&mut decoded.body.notes, resource_notes, "step_codec_notes")?;
    Ok(decoded)
}

pub(crate) fn is_part26_hdf5(
    bytes: &[u8],
) -> bool {
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

pub(crate) fn is_part28_xml(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<bool, CodecError> {
    let bytes = &bytes[..bytes.len().min(4096)];
    let Some(XmlRootTag { name, attributes }) = xml_root_start_tag(ctx, bytes)? else {
        return Ok(false);
    };
    let local_name = ctx
        .rposition_by(
            name,
            |byte| Ok(*byte == b':'),
            "STEP XML qualified name traversal",
        )?
        .map_or(name, |separator| &name[separator + 1..]);
    if local_name.eq_ignore_ascii_case(b"iso_10303_28")
        || ascii_starts_with(local_name, b"iso_10303_28_")
    {
        return Ok(true);
    }

    // A configured UOS can use a local name other than the document marker.
    // Its governing-schema namespace varies by AP, but the Part 28 common
    // namespace remains the bounded admission marker. Schema selection and
    // the derived XML Schema remain caller inputs.
    has_namespace_value(ctx, attributes, &PART28_COMMON_NAMESPACES)
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

struct XmlRootTag<'a> {
    name: &'a [u8],
    attributes: &'a [u8],
}

fn xml_root_start_tag<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
) -> Result<Option<XmlRootTag<'a>>, CodecError> {
    let mut cursor = if bytes.starts_with(b"\xef\xbb\xbf") {
        3
    } else {
        0
    };
    ctx.charge_work(0, "STEP XML root cursor traversal")?;
    while cursor < bytes.len() {
        ctx.charge_work(1, "STEP XML root cursor traversal")?;
        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            ctx.charge_work(1, "STEP XML root cursor traversal")?;
            cursor += 1;
        }
        if bytes.get(cursor) != Some(&b'<') {
            return Ok(None);
        }
        if bytes.get(cursor + 1) == Some(&b'?') {
            let Some(tail) = bytes.get(cursor + 2..) else {
                return Ok(None);
            };
            let Some(relative_end) = ctx.position_by(
                tail.windows(2),
                |window| Ok(window == b"?>"),
                "STEP XML processing instruction traversal",
            )?
            else {
                return Ok(None);
            };
            let end = relative_end + cursor + 2;
            cursor = end + 2;
            continue;
        }
        if bytes.get(cursor + 1..cursor + 4) == Some(b"!--") {
            let Some(tail) = bytes.get(cursor + 4..) else {
                return Ok(None);
            };
            let Some(relative_end) = ctx.position_by(
                tail.windows(3),
                |window| Ok(window == b"-->"),
                "STEP XML comment traversal",
            )?
            else {
                return Ok(None);
            };
            let end = relative_end + cursor + 4;
            cursor = end + 3;
            continue;
        }
        if bytes.get(cursor + 1) == Some(&b'!') {
            let Some(tail) = bytes.get(cursor + 2..) else {
                return Ok(None);
            };
            let Some(relative_end) = ctx.position_by(
                tail,
                |byte| Ok(*byte == b'>'),
                "STEP XML declaration traversal",
            )?
            else {
                return Ok(None);
            };
            let end = relative_end + cursor + 2;
            cursor = end + 1;
            continue;
        }
        break;
    }

    if cursor == bytes.len() {
        return Ok(None);
    }
    let Some(tag_end) = find_xml_tag_end(ctx, bytes, cursor + 1)? else {
        return Ok(None);
    };
    let mut name_end = cursor + 1;
    while bytes
        .get(name_end)
        .is_some_and(|byte| !byte.is_ascii_whitespace() && *byte != b'/' && *byte != b'>')
    {
        ctx.charge_work(1, "STEP XML root cursor traversal")?;
        name_end += 1;
    }
    if name_end == cursor + 1 {
        return Ok(None);
    }
    Ok(Some(XmlRootTag {
        name: &bytes[cursor + 1..name_end],
        attributes: &bytes[name_end..tag_end],
    }))
}

fn find_xml_tag_end(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    mut cursor: usize,
) -> Result<Option<usize>, CodecError> {
    let mut quote = None;
    while let Some(byte) = bytes.get(cursor).copied() {
        ctx.charge_work(1, "STEP XML tag cursor traversal")?;
        match quote {
            Some(delimiter) if byte == delimiter => quote = None,
            Some(_) => {}
            None if byte == b'\'' || byte == b'"' => quote = Some(byte),
            None if byte == b'>' => return Ok(Some(cursor)),
            None => {}
        }
        cursor += 1;
    }
    Ok(None)
}

fn has_namespace_value(
    ctx: &DecodeContext<'_>,
    attributes: &[u8],
    namespaces: &[&[u8]],
) -> Result<bool, CodecError> {
    // Every caller supplies a fixed catalog of namespace literals.
    Ok(xml_attribute_value(ctx, attributes, |_, name, value| {
        Ok((name == b"xmlns" || name.starts_with(b"xmlns:")) && namespaces.contains(&value))
    })?
    .is_some())
}

fn xml_attribute_value<'a>(
    ctx: &DecodeContext<'_>,
    attributes: &'a [u8],
    mut matches: impl FnMut(&DecodeContext<'_>, &[u8], &[u8]) -> Result<bool, CodecError>,
) -> Result<Option<&'a [u8]>, CodecError> {
    let mut cursor = 0;
    while cursor < attributes.len() {
        ctx.charge_work(1, "STEP XML attribute cursor traversal")?;
        while attributes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            ctx.charge_work(1, "STEP XML attribute cursor traversal")?;
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
            ctx.charge_work(1, "STEP XML attribute cursor traversal")?;
            cursor += 1;
        }
        let name = &attributes[name_start..cursor];
        while attributes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            ctx.charge_work(1, "STEP XML attribute cursor traversal")?;
            cursor += 1;
        }
        if attributes.get(cursor) != Some(&b'=') {
            return Ok(None);
        }
        cursor += 1;
        while attributes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            ctx.charge_work(1, "STEP XML attribute cursor traversal")?;
            cursor += 1;
        }
        let Some(&delimiter) = attributes.get(cursor) else {
            return Ok(None);
        };
        if delimiter != b'\'' && delimiter != b'"' {
            return Ok(None);
        }
        cursor += 1;
        let value_start = cursor;
        while attributes
            .get(cursor)
            .is_some_and(|byte| *byte != delimiter)
        {
            ctx.charge_work(1, "STEP XML attribute cursor traversal")?;
            cursor += 1;
        }
        let value = &attributes[value_start..cursor];
        cursor += 1;
        if matches(ctx, name, value)? {
            return Ok(Some(value));
        }
    }
    Ok(None)
}

pub(crate) fn is_ap242_bo_model_xml(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<bool, CodecError> {
    let bytes = &bytes[..bytes.len().min(4096)];
    let Some(XmlRootTag { name, attributes }) = xml_root_start_tag(ctx, bytes)? else {
        return Ok(false);
    };
    let local_name = ctx
        .rposition_by(
            name,
            |byte| Ok(*byte == b':'),
            "STEP XML qualified name traversal",
        )?
        .map_or(name, |separator| &name[separator + 1..]);
    // BM-03: the published namespace must be bound on the Uos document
    // element. Text, comments, schemaLocation values, and local names do not
    // identify the alternate encoding.
    if local_name != b"Uos" {
        return Ok(false);
    }
    has_namespace_value(ctx, attributes, &BO_MODEL_NAMESPACES)
}

const BO_MODEL_NAMESPACES: [&[u8]; 2] = [
    b"http://standards.iso.org/iso/ts/10303/-3001/-ed-1/tech/xml-schema/bo_model",
    b"http://standards.iso.org/iso/ts/10303/-3001/-ed-2/tech/xml-schema/bo_model",
];

#[cfg(test)]
mod tests {
    mod prefix_admission;

    use std::fmt::Write as _;
    use std::io::Cursor;

    use cadmpeg_core::decode::{
        DecodeArena, DecodeContext, DecodePolicy, InspectOptions, ResourceDimension,
    };
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions};

    use super::{insert_attribute, starts_with_step_magic, StepCodec};

    #[test]
    fn scoped_parse_errors_retain_only_the_escaping_text() {
        for (source, operation) in [
            (
                format!("ISO-10303-21;{};", "A".repeat(8192)).into_bytes(),
                "STEP parse error",
            ),
            (b"ISO-10303-21;#0".to_vec(), "STEP lexical error"),
        ] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = 0;
            for inspect in [false, true] {
                crate::test_support::with_policy_context(&source, &policy, |source, ctx| {
                    let result = if inspect {
                        super::inspect_exchange(
                            &StepCodec::default(),
                            ctx,
                            cadmpeg_core::decode::View::over_retained(source),
                        )
                        .map(|_| ())
                    } else {
                        use cadmpeg_ir::codec::CodecBackend;
                        StepCodec::default()
                            .decode_impl(ctx, cadmpeg_core::decode::View::over_retained(source))
                            .map(|_| ())
                    };
                    assert!(matches!(result, Err(CodecError::ResourceLimit(refusal))
                        if refusal.dimension == ResourceDimension::RetainedBytes
                            && refusal.operation == operation));
                });
            }
        }
    }

    #[test]
    fn zip_scoped_parse_errors_retain_only_the_escaping_text() {
        use std::io::Write;
        for (root, operation) in [
            (
                format!("ISO-10303-21;{};", "A".repeat(8192)).into_bytes(),
                "STEP parse error",
            ),
            (b"ISO-10303-21;#0".to_vec(), "STEP lexical error"),
        ] {
            let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
            writer
                .start_file(
                    "ISO-10303.p21",
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Stored),
                )
                .expect("ZIP root");
            writer.write_all(&root).expect("root content");
            let source = writer.finish().expect("ZIP envelope").into_inner();
            for inspect in [false, true] {
                cadmpeg_test_support::refusal::resource_limit_at(
                    ResourceDimension::RetainedBytes,
                    operation,
                    |cap| {
                        let mut policy = DecodePolicy::service();
                        policy.limits.max_retained_bytes = cap;
                        crate::test_support::with_policy_context(&source, &policy, |source, ctx| {
                            let view = cadmpeg_core::decode::View::over_retained(source);
                            if inspect {
                                super::inspect_zip(ctx, view).map(|_| ())
                            } else {
                                super::decode_zip(ctx, view).map(|_| ())
                            }
                        })
                    },
                );
            }
        }
    }

    #[test]
    fn scoped_parse_graph_uses_materialized_storage_until_drop() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 1_048_576;
        crate::test_support::with_policy_context(INSPECTION_TEXT_SOURCE, &policy, |source, ctx| {
            let mut graph_storage = ctx.reserve_scoped(0, "test parsed graph").expect("scope");
            let (exchange, diagnostics) = crate::parse::parse_with_context(source, ctx, &mut graph_storage)
                .expect("temporary graph needs no retained allowance");
            assert_eq!(exchange.records().len(), 1);
            drop(exchange);
            drop(diagnostics);
            drop(graph_storage);
            ctx.reserve_scoped(policy.limits.max_materialized_bytes, "test released graph")
                .expect("dropping the graph releases its reservation");
        });
    }

    fn point_source(count: usize) -> Vec<u8> {
        let mut records = (1..=count).fold(String::new(), |mut records, id| {
            write!(records, "#{id}=CARTESIAN_POINT('',(1.,2.,3.));").expect("point fixture text");
            records
        });
        let members = (1..=count)
            .map(|id| format!("#{id}"))
            .collect::<Vec<_>>()
            .join(",");
        write!(records, "#{}=GEOMETRIC_SET('',({members}));", count + 1).expect("set fixture text");
        format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;").into_bytes()
    }

    #[test]
    fn bare_decode_does_not_retain_the_discarded_parse_graph() {
        use cadmpeg_ir::codec::CodecBackend;
        let source = point_source(128);
        let retained = |scoped| {
            crate::test_support::with_service_context(&source, |source, ctx| {
                let decoded = if scoped {
                    StepCodec::default()
                        .decode_impl(ctx, cadmpeg_core::decode::View::over_retained(source))
                } else {
                    let (exchange, diagnostics) =
                        crate::parse::parse_retained(source, ctx).expect("unscoped control graph");
                    crate::reader::decode_exchange(
                        source,
                        exchange,
                        &diagnostics,
                        ctx,
                        crate::reader::Packaging::Bare,
                    )
                }
                .expect("valid points decode");
                assert_eq!(decoded.ir.model.points.len(), 128);
                let CodecError::ResourceLimit(refusal) = ctx
                    .charge_retained(u64::MAX, "test retained storage")
                    .expect_err("storage probe refuses")
                else {
                    panic!("retained refusal required");
                };
                refusal.used
            })
        };
        assert!(retained(true) < retained(false));
    }

    #[test]
    fn inspection_retains_summary_storage_instead_of_geometry() {
        let source = point_source(1024);
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 16_384;
        crate::test_support::with_policy_context(&source, &policy, |source, ctx| {
            let inspected = super::inspect_exchange(
                &StepCodec::default(),
                ctx,
                cadmpeg_core::decode::View::over_retained(source),
            )
            .expect("summary fits without the discarded point graph");
            let data = inspected
                .entries
                .iter()
                .find(|entry| entry.name == "DATA[0]")
                .expect("DATA entry");
            // The section contains 1024 points and their geometric-set carrier.
            assert_eq!(data.attributes["entity_count"], "1025");
        });
    }

    #[test]
    fn inspection_surviving_dialect_and_losses_preserve_retained_refusal() {
        let source = include_bytes!("../tests/fixtures/noncanonical_solid_angle.p21");
        for operation in [
            "STEP inspection retained dialect",
            "STEP inspection retained loss",
        ] {
            cadmpeg_test_support::refusal::resource_limit_at(
                ResourceDimension::RetainedBytes,
                operation,
                |cap| {
                    let mut policy = DecodePolicy::service();
                    policy.limits.max_retained_bytes = cap;
                    crate::test_support::with_policy_context(source, &policy, |source, ctx| {
                        super::inspect_exchange(
                            &StepCodec::default(),
                            ctx,
                            cadmpeg_core::decode::View::over_retained(source),
                        )
                        .map(|_| ())
                    })
                },
            );
        }
    }

    #[test]
    fn inspection_schema_note_keeps_all_declared_identifiers_in_order() {
        let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;3');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242','CUSTOM_SCHEMA'));ENDSEC;DATA('main',('AP242'));ENDSEC;END-ISO-10303-21;";
        crate::test_support::with_service_context(source, |source, ctx| {
            let inspected = super::inspect_exchange(
                &StepCodec::default(),
                ctx,
                cadmpeg_core::decode::View::over_retained(source),
            )
            .expect("named section and multiple schemas");
            assert!(inspected
                .notes
                .iter()
                .any(|note| note.starts_with("schema AP242,CUSTOM_SCHEMA; dialect ")));
        });
    }

    #[test]
    fn header_cursor_walks_refuse_work_before_advancing() {
        for (source, operation) in [
            (
                b" \0 ISO-10303-21;".as_slice(),
                "STEP magic cursor traversal",
            ),
            (
                b" \n <Uos a='v'>".as_slice(),
                "STEP XML root cursor traversal",
            ),
            (b"<Uos a='v'>".as_slice(), "STEP XML tag cursor traversal"),
            (
                b"<Uos xmlns='urn:test'>".as_slice(),
                "STEP XML attribute cursor traversal",
            ),
        ] {
            cadmpeg_test_support::refusal::resource_limit_at(
                ResourceDimension::WorkUnits,
                operation,
                |cap| {
                    let arena = DecodeArena::new();
                    let mut policy = DecodePolicy::service();
                    policy.limits.max_work_units = cap;
                    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy).unwrap();
                    if operation == "STEP magic cursor traversal" {
                        starts_with_step_magic(&ctx, source)
                    } else {
                        super::is_ap242_bo_model_xml(&ctx, source)
                    }
                },
            );
        }
    }

    #[test]
    fn magic_detection_does_not_charge_unvisited_suffix() {
        use cadmpeg_ir::codec::CodecBackend;

        let source = [b"ISO-10303-21;".as_slice(), &[b' '; 8192]].concat();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // One trivia-loop visit; the fixed magic contains no ignored controls.
        policy.limits.max_work_units = 1;
        let (ctx, root) = DecodeContext::from_root_bytes(&source, &arena, &policy).unwrap();
        assert_eq!(
            StepCodec::default().detect_impl(&ctx, root).unwrap(),
            Confidence::High
        );
    }

    #[test]
    fn xml_namespace_values_match_literal_namespaces() {
        let attributes = b" xmlns='urn:test:namespace'";
        let service = cadmpeg_test_support::service_decode_context();
        assert!(super::has_namespace_value(
            &service,
            attributes,
            &[b"urn:test:namespace".as_slice()]
        )
        .unwrap());
        assert!(!super::has_namespace_value(
            &service,
            attributes,
            &[b"urn:test:different".as_slice()]
        )
        .unwrap());
        assert!(!super::has_namespace_value(
            &service,
            b" xmlns",
            &[b"urn:test:namespace".as_slice()]
        )
        .unwrap());
    }

    #[test]
    fn namespace_match_leaves_following_attributes_unvisited() {
        fn work(attributes: &[u8], namespaces: &[&[u8]]) -> u64 {
            crate::test_support::with_service_context(attributes, |_, ctx| {
                assert!(super::has_namespace_value(ctx, attributes, namespaces)
                    .expect("literal namespace matches"));
                let CodecError::ResourceLimit(refusal) = ctx
                    .charge_work(u64::MAX, "test completed XML namespace work")
                    .expect_err("work probe refuses")
                else {
                    panic!("work refusal required");
                };
                refusal.used
            })
        }
        for namespaces in [
            super::PART28_COMMON_NAMESPACES.as_slice(),
            super::BO_MODEL_NAMESPACES.as_slice(),
        ] {
            let namespace = std::str::from_utf8(namespaces.last().expect("catalog is nonempty"))
                .expect("namespace literal is UTF-8");
            let attributes = format!(" xmlns='{namespace}'");
            let extended = [
                attributes.as_bytes(),
                b" ignored='".as_slice(),
                &[b'x'; 1024],
                b"'".as_slice(),
            ]
            .concat();
            assert_eq!(
                work(attributes.as_bytes(), namespaces),
                work(&extended, namespaces)
            );
        }
    }

    const INSPECTION_TEXT_SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";

    #[test]
    fn inspection_count_text_refuses_retained_limit() {
        inspect_text_refuses(INSPECTION_TEXT_SOURCE, "step_inspect_attribute_count");
    }

    #[test]
    fn inspection_attribute_key_refuses_retained_limit() {
        inspect_text_refuses(INSPECTION_TEXT_SOURCE, "step_inspect_attribute_key");
    }

    #[test]
    fn inspection_owned_attribute_value_is_not_charged_twice() {
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_retained_bytes = u64::try_from(
            13 + 11 * std::mem::size_of::<(String, String)>()
                + 16 * std::mem::size_of::<usize>()
                + 2 * std::mem::align_of::<String>(),
        )
        .expect("attribute node layout");
        crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
            let mut attributes = std::collections::BTreeMap::new();
            let value = ctx
                .copy_retained_text("1", "step_test_attribute_value")
                .expect("one-byte value");
            insert_attribute(ctx, &mut attributes, "entity_count", value)
                .expect("twelve-byte key fits once");
            assert_eq!(attributes["entity_count"], "1");
        });
    }

    #[test]
    fn inspect_attribute_refuses_collection_limit() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
            .expect("empty root fits selected policy");
        let mut attributes = std::collections::BTreeMap::new();
        assert!(matches!(
            insert_attribute(&ctx, &mut attributes, "unknown_entities", "ITEM:1".into()),
            Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == "step_inspect_attributes"
        ));
    }

    fn inspect_text_refuses(source: &[u8], operation: &str) {
        let mut limit = 0u64;
        for _ in 0..512 {
            let (mut exchange, diagnostics) =
                crate::test_support::with_service_context(source, crate::parse::parse_inner)
                    .expect("valid inspect source");
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
                .expect("root fits retained policy");
            match super::inspect_parsed_exchange(source, &ctx, &mut exchange, &diagnostics, None) {
                Err(CodecError::ResourceLimit(refusal))
                    if refusal.dimension == ResourceDimension::RetainedBytes
                        && refusal.operation == operation =>
                {
                    return;
                }
                Err(CodecError::ResourceLimit(refusal))
                    if refusal.dimension == ResourceDimension::RetainedBytes =>
                {
                    let next = refusal
                        .used
                        .checked_add(refusal.additional)
                        .expect("retained requirement fits u64");
                    assert!(next > limit, "retained limit must advance");
                    limit = next;
                }
                Ok(_) => panic!("inspect completed before {operation}"),
                Err(error) => panic!("inspect did not reach {operation}: {error}"),
            }
        }
        panic!("inspect never reached {operation}");
    }

    #[test]
    fn inspect_external_uris_refuse_retained_limit() {
        const SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;REFERENCE;#10=<parts/child.p21>;ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
        inspect_text_refuses(SOURCE, "step_inspect_external_uris");
    }

    #[test]
    fn inspect_unknown_entities_refuse_retained_limit() {
        const SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
        inspect_text_refuses(SOURCE, "step_inspect_unknown_entities");
    }

    #[test]
    fn inspect_schema_note_refuses_retained_limit() {
        const SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
        inspect_text_refuses(SOURCE, "step_inspect_schema_note");
    }

    #[test]
    fn inspect_diagnostic_copy_refuses_retained_limit() {
        const SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;9');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
        inspect_text_refuses(SOURCE, "step_inspect_diagnostic_copy");
    }

    #[test]
    fn inspect_dependency_text_refuses_retained_limit() {
        let source = include_bytes!("../tests/fixtures/ap242_external_documents.p21");
        inspect_text_refuses(source, "step_inspect_dependency_text");
    }

    #[test]
    fn inspect_unknown_count_entries_refuse_collection_limit() {
        const SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
        let mut limit = 0u64;
        for _ in 0..512 {
            let (mut exchange, diagnostics) =
                crate::test_support::with_service_context(SOURCE, crate::parse::parse_inner)
                    .expect("valid inspect source");
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(SOURCE, &arena, &policy)
                .expect("root fits collection policy");
            match super::inspect_parsed_exchange(SOURCE, &ctx, &mut exchange, &diagnostics, None) {
                Err(CodecError::ResourceLimit(refusal))
                    if refusal.dimension == ResourceDimension::CollectionItems
                        && refusal.operation == "step_inspect_unknown_counts" =>
                {
                    return;
                }
                Err(CodecError::ResourceLimit(refusal))
                    if refusal.dimension == ResourceDimension::CollectionItems =>
                {
                    let next = refusal
                        .used
                        .checked_add(refusal.additional)
                        .expect("collection requirement fits u64");
                    assert!(next > limit, "collection limit must advance");
                    limit = next;
                }
                Ok(_) => panic!("inspect completed before count admission"),
                Err(error) => panic!("inspect did not reach count admission: {error}"),
            }
        }
        panic!("inspect never reached count admission");
    }

    fn zip_text_refuses(operation: &str, inspect: bool) {
        use std::io::Write;

        const ROOT: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file(
                "ISO-10303.p21",
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored),
            )
            .expect("start ZIP root");
        writer.write_all(ROOT).expect("write ZIP root");
        let bytes = writer.finish().expect("finish STEP ZIP").into_inner();

        let mut limit = 0u64;
        for _ in 0..1024 {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = limit;
            let result = crate::test_support::with_policy_context(&bytes, &policy, |bytes, ctx| {
                let root = cadmpeg_core::decode::View::over_retained(bytes);
                if inspect {
                    super::inspect_zip(ctx, root).map(|_| ())
                } else {
                    super::decode_zip(ctx, root).map(|_| ())
                }
            });
            match result {
                Err(CodecError::ResourceLimit(refusal))
                    if refusal.dimension == ResourceDimension::RetainedBytes
                        && refusal.operation == operation =>
                {
                    return;
                }
                Err(CodecError::ResourceLimit(refusal))
                    if refusal.dimension == ResourceDimension::RetainedBytes =>
                {
                    let next = refusal
                        .used
                        .checked_add(refusal.additional)
                        .expect("retained requirement fits u64");
                    assert!(next > limit, "retained limit must advance");
                    limit = next;
                }
                Ok(()) => panic!("ZIP operation completed before {operation}"),
                Err(error) => panic!("ZIP operation did not reach {operation}: {error}"),
            }
        }
        panic!("ZIP operation never reached {operation}");
    }

    #[test]
    fn inspect_zip_logical_sections_refuse_retained_limit() {
        zip_text_refuses("step_inspect_logical_sections", true);
    }

    #[test]
    fn inspection_section_name_refuses_retained_limit() {
        inspect_text_refuses(INSPECTION_TEXT_SOURCE, "step_inspect_section_name");
    }

    #[test]
    fn inspection_signature_name_refuses_retained_limit() {
        const SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;SIGNATURE;MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA= ENDSEC;SIGNATURE;MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA= ENDSEC;";
        inspect_text_refuses(SOURCE, "step_inspect_signature_indexed_name");
    }

    #[test]
    fn zip_inspection_archive_note_refuses_retained_limit() {
        zip_text_refuses("step_codec_archive_note", true);
    }

    #[test]
    fn zip_decode_container_note_refuses_retained_limit() {
        zip_text_refuses("step_codec_container_note", false);
    }

    #[test]
    fn detects_magic_after_ignored_controls_and_inside_token() {
        let source = b"\0 /* leading comment */ \\N\\ ISO-10303-\n21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
        let codec = StepCodec::default();

        crate::test_support::with_service_context(source, |source, ctx| {
            assert!(starts_with_step_magic(ctx, source).expect("magic scan"));
        });
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
        crate::test_support::with_service_context(b"/* incomplete ISO-10303-21;", |source, ctx| {
            assert!(!starts_with_step_magic(ctx, source).expect("incomplete comment scan"));
        });
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
    fn fixed_hdf5_signature_checks_need_no_offset_work() {
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let mut at_offset = vec![0; 4096 + 8];
        at_offset[4096..].copy_from_slice(b"\x89HDF\r\n\x1a\n");
        for (bytes, expected) in [(b"\x89HDF\r\n\x1a\n".as_slice(), true), (&[0; 512], false), (&at_offset, true)] {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &policy).expect("root");
                assert_eq!(super::is_part26_hdf5(bytes), expected);
                ctx.finish_session().expect("fixed checks do not refuse work");
        }
    }

    #[test]
    fn logical_inspection_entries_can_use_only_scoped_storage_until_their_drop() {
        let (exchange, _) = crate::test_support::with_service_context(INSPECTION_TEXT_SOURCE, crate::parse::parse_inner).expect("exchange");
        let opaque_offsets = exchange.records().values().map(|record| record.span.start).collect();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 1_048_576;
        crate::test_support::with_policy_context(&[], &policy, |_, ctx| {
            let (entries, storage) = ctx.with_scoped_storage("test logical entries", || {
                super::inspect_logical_entries(ctx, &exchange, &opaque_offsets, &[])
            }).expect("discarded logical summary needs no retained allowance");
            assert_eq!(entries.len(), 2);
            assert_eq!(entries[0].name, "HEADER");
            assert_eq!(entries[1].name, "DATA[0]");
            assert_eq!(entries[1].attributes.get("unknown_entities").map(String::as_str), Some("ITEM:1"));
            drop(entries);
            drop(storage);
            ctx.reserve_scoped(policy.limits.max_materialized_bytes, "test released logical entries").expect("all logical-entry backing is released");
        });
    }
}
