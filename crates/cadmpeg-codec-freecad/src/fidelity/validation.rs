// SPDX-License-Identifier: Apache-2.0
//! Source layout closure and ownership checks, independent of CADIR content.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::{finding, native};
use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::report::check::{Check, Finding};
use cadmpeg_ir::{CadIr, SourceFidelity};

fn read_layout<T: serde::de::DeserializeOwned>(
    ctx: &DecodeContext<'_>,
    fidelity: &SourceFidelity,
    name: &str,
) -> Result<T, CodecError> {
    let id = native::native_id("source-layout", name);
    let bytes = fidelity
        .retained_record(&id)
        .and_then(|record| record.data())
        .ok_or_else(|| CodecError::malformed(format!("missing FCStd source layout {name}")))?;
    let text = std::str::from_utf8(bytes).map_err(CodecError::malformed)?;
    ctx.parse_json(text, "read FCStd source layout")
}

fn read_arena<T: serde::de::DeserializeOwned>(
    ctx: &DecodeContext<'_>,
    namespace: &cadmpeg_ir::native::NativeNamespace,
    name: &str,
) -> Result<Vec<T>, CodecError> {
    namespace
        .arena_as_for_decode(ctx, name)
        .map_err(|error| match error {
            cadmpeg_ir::native::NativeConvertError::Resource(error) => error,
            error => CodecError::malformed(error),
        })
}

pub(crate) fn validate(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    fidelity: &SourceFidelity,
) -> Result<Vec<Finding>, CodecError> {
    let Some(namespace) = ir.native.namespace("fcstd") else {
        return Ok(Vec::new());
    };
    let physical: Vec<native::ArchiveSpan> = read_layout(ctx, fidelity, "physical-ledger")?;
    let logical: Vec<native::LogicalSpan> = if fidelity
        .retained_record(&native::native_id("source-layout", "logical-ledger"))
        .is_some()
    {
        read_layout(ctx, fidelity, "logical-ledger")?
    } else {
        Vec::new()
    };
    let coverage: native::ByteCoverageRecord = read_layout(ctx, fidelity, "byte-coverage")?;
    let entries: Vec<native::EntryRecord> = read_arena(ctx, namespace, "entries")?;
    let unreadable: Vec<crate::container::UnreadableEntry> =
        read_arena(ctx, namespace, "unreadable_entries")?;
    let properties: Vec<native::PropertyRecord> = read_arena(ctx, namespace, "properties")?;
    let gui_properties: Vec<native::GuiPropertyRecord> =
        read_arena(ctx, namespace, "gui_properties")?;
    let gui_documents: Vec<native::GuiDocumentRecord> =
        read_arena(ctx, namespace, "gui_documents")?;
    let shape_payloads: Vec<crate::brep::ShapePayloadRecord> =
        read_arena(ctx, namespace, "shape_payloads")?;
    let string_tables: Vec<native::StringTableRecord> =
        read_arena(ctx, namespace, "string_tables")?;
    let element_maps: Vec<native::element_map::ElementMapRecord> =
        read_arena(ctx, namespace, "element_maps")?;
    let physical_end = fidelity
        .retained_record(super::SOURCE_IMAGE)
        .map(cadmpeg_ir::RetainedSourceRecord::byte_len);
    let mut findings = Vec::new();
    validate_span_chain("physical archive", &physical, physical_end, &mut findings);
    for entry in &unreadable {
        if !fidelity.retained_record(&entry.id).is_some_and(|record| {
            let source_bytes = fidelity
                .retained_record(super::SOURCE_IMAGE)
                .and_then(|image| image.data())
                .and_then(|image| {
                    let start = usize::try_from(record.offset()).ok()?;
                    let end = usize::try_from(record.end_offset()).ok()?;
                    image.get(start..end)
                });
            record.stream() == super::SOURCE_IMAGE
                && record.byte_len() == entry.byte_len
                && record.sha256() == entry.sha256
                && source_bytes
                    .is_some_and(|bytes| cadmpeg_ir::hash::sha256_hex(bytes) == entry.sha256)
        }) {
            findings.push(finding(
                Check::PayloadIntegrity,
                "invalid source-only FCStd entry",
                Some(entry.id.clone()),
            ));
        }
    }
    let property_ids = properties
        .iter()
        .map(|property| property.id.as_str())
        .collect::<HashSet<_>>();
    let string_table_ids = string_tables
        .iter()
        .map(native::StringTableRecord::id)
        .collect::<Vec<_>>();
    let logical_owner_ids = property_ids
        .iter()
        .copied()
        .chain(gui_properties.iter().map(|record| record.id.as_str()))
        .chain(
            gui_documents
                .iter()
                .flat_map(|document| document.states.iter().map(|record| record.id.as_str())),
        )
        .chain(shape_payloads.iter().map(|record| record.id.as_str()))
        .chain(string_table_ids.iter().map(String::as_str))
        .chain(element_maps.iter().map(|record| record.id.as_str()))
        .chain(entries.iter().map(native::EntryRecord::id))
        .collect::<HashSet<_>>();
    let mut entry_lengths = HashMap::new();
    for entry in &entries {
        let record = fidelity.retained_record(entry.id()).ok_or_else(|| {
            CodecError::malformed(format!("missing FCStd source entry {}", entry.name()))
        })?;
        entry_lengths.insert(entry.name(), record.byte_len());
    }
    let mut logical_by_entry = BTreeMap::<&str, Vec<&native::LogicalSpan>>::new();
    for span in &logical {
        logical_by_entry.entry(&span.entry).or_default().push(span);
        let owner_valid = match &span.classification {
            native::LogicalClassification::Structural => true,
            native::LogicalClassification::Typed { owner }
            | native::LogicalClassification::NamedOpaque { owner } => {
                logical_owner_ids.contains(owner.as_str())
            }
        };
        if !entry_lengths.contains_key(span.entry.as_str()) || !owner_valid {
            findings.push(finding(
                Check::PayloadIntegrity,
                format!("{} has an invalid logical entry or owner", span.id),
                Some(span.id.clone()),
            ));
        }
    }
    let covered_entries = logical_by_entry.keys().copied().collect::<HashSet<_>>();
    for entry in &entries {
        if entry_lengths
            .get(entry.name())
            .is_some_and(|length| *length > 0)
            && !covered_entries.contains(entry.name())
        {
            findings.push(finding(
                Check::PayloadIntegrity,
                format!("logical ledger omits nonempty entry {}", entry.name()),
                Some(entry.id().to_owned()),
            ));
        }
    }
    for (name, mut spans) in logical_by_entry {
        spans.sort_by_key(|left| left.span.start());
        validate_logical_chain(
            name,
            &spans,
            entry_lengths.get(name).copied(),
            &mut findings,
        );
    }
    let expected_coverage = crate::container::byte_coverage(
        &physical,
        &entries,
        &logical,
        physical_end.unwrap_or_default(),
    );
    if coverage != expected_coverage || !expected_coverage.exact {
        findings.push(finding(
            Check::PayloadIntegrity,
            "FCStd byte coverage report is stale or does not prove exact closure",
            None,
        ));
    }
    Ok(findings)
}

fn validate_span_chain(
    label: &str,
    spans: &[native::ArchiveSpan],
    expected_end: Option<u64>,
    findings: &mut Vec<Finding>,
) {
    let mut ordered = spans.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|left| left.span.start());
    let valid = ordered.first().is_some_and(|span| span.span.start() == 0)
        && ordered
            .windows(2)
            .all(|pair| pair[0].span.end() == pair[1].span.start())
        && expected_end.is_none_or(|end| ordered.last().is_some_and(|span| span.span.end() == end));
    if !valid {
        findings.push(finding(
            Check::PayloadIntegrity,
            format!("{label} ledger has a gap, overlap, or invalid boundary"),
            None,
        ));
    }
}

fn validate_logical_chain(
    name: &str,
    spans: &[&native::LogicalSpan],
    expected_end: Option<u64>,
    findings: &mut Vec<Finding>,
) {
    let valid = expected_end.is_some()
        && spans.first().is_some_and(|span| span.span.start() == 0)
        && spans
            .windows(2)
            .all(|pair| pair[0].span.end() == pair[1].span.start())
        && expected_end.is_some_and(|end| spans.last().is_some_and(|span| span.span.end() == end));
    if !valid {
        findings.push(finding(
            Check::PayloadIntegrity,
            format!("logical ledger for {name} has a gap, overlap, or invalid boundary"),
            None,
        ));
    }
}
