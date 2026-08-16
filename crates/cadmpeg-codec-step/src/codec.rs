// SPDX-License-Identifier: Apache-2.0
//! STEP codec backend and encoder.

use std::collections::BTreeMap;

use cadmpeg_core::{CodecError, ContainerEntry, ContainerSummary};
use cadmpeg_ir::codec::{
    CodecBackend, Confidence, DecodeOptions, DecodeResult, EncodeInput, Encoder, ExportPlan,
};
use cadmpeg_ir::FidelityResolution;

use crate::archive;
use crate::export::write_step;
use crate::options::{StepSchema, StepWriteOptions};
use crate::parse;
use crate::reader;

/// STEP encoder with per-export header options.
#[derive(Debug, Clone, Default)]
pub struct StepCodec {
    /// Header metadata and deterministic writer options.
    pub options: StepWriteOptions,
}

impl Encoder for StepCodec {
    fn id(&self) -> &'static str {
        "step"
    }

    fn plan<'a>(&self, input: EncodeInput<'a>) -> Result<ExportPlan<'a>, CodecError> {
        let mut bytes = Vec::new();
        let mut report =
            write_step(input.ir, &mut bytes, &self.options).map_err(CodecError::from)?;
        // `write_step` takes no fidelity sidecar, so the report it returns
        // states the only resolution it can see. Whether the caller supplied
        // one is known here, and only here.
        report.fidelity = if input.fidelity.is_some() {
            FidelityResolution::NotConsumed
        } else {
            FidelityResolution::NotProvided
        };
        Ok(ExportPlan::buffered(report, bytes))
    }
}

impl CodecBackend for StepCodec {
    fn id(&self) -> &'static str {
        "step"
    }

    fn detect(&self, prefix: &[u8]) -> Confidence {
        if starts_with_step_magic(prefix) {
            Confidence::High
        } else if archive::has_root_marker(prefix)
            || is_part28_xml(prefix)
            || is_ap242_bo_model_xml(prefix)
        {
            Confidence::Medium
        } else {
            Confidence::No
        }
    }

    /// Deep semantic analysis of a STEP exchange, exposed as container inspect.
    ///
    /// Runs the semantic decode path to populate `unknown_entities` and related
    /// attributes. Not a cheap syntactic census; see
    /// `docs/formats/step-inspect.md`.
    fn inspect_impl(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        root: cadmpeg_core::decode::View<'_>,
    ) -> Result<ContainerSummary, CodecError> {
        let bytes = root.window();
        if archive::has_zip_magic(bytes) {
            return inspect_zip(ctx, root);
        }
        refuse_alternate_encoding(bytes)?;
        if self.detect(bytes) == Confidence::No {
            return Err(CodecError::WrongFormat("missing ISO-10303-21 magic".into()));
        }
        let (mut exchange, diagnostics) = parse::parse_with_context(bytes, ctx)?;
        let (decoded, opaque_offsets) =
            reader::analyze_exchange(bytes, &mut exchange, &diagnostics, Some(ctx))?;
        let mut entries = vec![ContainerEntry {
            name: "HEADER".into(),
            role: "metadata".into(),
            compression: "none".into(),
            compressed_size: 0,
            uncompressed_size: 0,
            attributes: BTreeMap::default(),
        }];
        if !exchange.anchors.is_empty() {
            let mut attributes = std::collections::BTreeMap::new();
            attributes.insert("anchor_count".into(), exchange.anchors.len().to_string());
            entries.push(ContainerEntry {
                name: "ANCHOR".into(),
                role: "in_file_anchors".into(),
                compression: "none".into(),
                compressed_size: 0,
                uncompressed_size: 0,
                attributes,
            });
        }
        if !exchange.references.is_empty() {
            let mut attributes = std::collections::BTreeMap::new();
            attributes.insert(
                "external_count".into(),
                exchange.references.len().to_string(),
            );
            attributes.insert(
                "external_uris".into(),
                exchange
                    .references
                    .iter()
                    .map(|entry| entry.uri.as_str())
                    .collect::<Vec<_>>()
                    .join(","),
            );
            entries.push(ContainerEntry {
                name: "REFERENCE".into(),
                role: "external_references".into(),
                compression: "none".into(),
                compressed_size: 0,
                uncompressed_size: 0,
                attributes,
            });
        }
        for (index, section) in exchange.data.iter().enumerate() {
            let mut counts = std::collections::BTreeMap::<String, usize>::new();
            for id in &section.records {
                if !opaque_offsets.contains(&exchange.records[id].span.start) {
                    continue;
                }
                for partial in &exchange.records[id].partials {
                    *counts.entry(partial.name.clone()).or_default() += 1;
                }
            }
            let unknown = counts
                .iter()
                .map(|(name, count)| format!("{name}:{count}"))
                .collect::<Vec<_>>()
                .join(",");
            let mut attributes = std::collections::BTreeMap::new();
            attributes.insert("entity_count".into(), section.records.len().to_string());
            attributes.insert("unknown_entities".into(), unknown);
            entries.push(ContainerEntry {
                name: format!("DATA[{index}]"),
                role: "entity_records".into(),
                compression: "none".into(),
                compressed_size: 0,
                uncompressed_size: 0,
                attributes,
            });
        }
        let external_dependencies = decoded
            .report()
            .notes
            .iter()
            .filter(|note| {
                note.starts_with("external document ") || note.starts_with("external source ")
            })
            .cloned()
            .collect::<Vec<_>>();
        if !external_dependencies.is_empty() {
            let mut attributes = std::collections::BTreeMap::new();
            attributes.insert(
                "dependency_count".into(),
                external_dependencies.len().to_string(),
            );
            attributes.insert("dependencies".into(), external_dependencies.join(","));
            entries.push(ContainerEntry {
                name: "EXTERNAL_DEPENDENCIES".into(),
                role: "external_references".into(),
                compression: "none".into(),
                compressed_size: 0,
                uncompressed_size: 0,
                attributes,
            });
        }
        for (index, _) in exchange.signatures.iter().enumerate() {
            entries.push(ContainerEntry {
                name: if index == 0 {
                    "SIGNATURE".into()
                } else {
                    format!("SIGNATURE[{index}]")
                },
                role: "signature".into(),
                compression: "none".into(),
                compressed_size: 0,
                uncompressed_size: 0,
                attributes: BTreeMap::default(),
            });
        }
        let identifiers = reader::schema_identifiers(&exchange);
        let schema = if identifiers.is_empty() {
            "unspecified".into()
        } else {
            identifiers.join(",")
        };
        let edition = identifiers
            .first()
            .and_then(|identifier| StepSchema::ap242_edition(identifier))
            .unwrap_or("edition unspecified");
        let mut notes = vec![format!("schema {schema}; {edition}")];
        notes.extend(diagnostics.into_iter().map(|diagnostic| diagnostic.message));
        Ok(ContainerSummary {
            format: "step".into(),
            container_kind: "iso-10303-21-clear-text".into(),
            entries,
            notes,
        })
    }

    fn decode_impl(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        root: cadmpeg_core::decode::View<'_>,
    ) -> Result<DecodeResult, CodecError> {
        let bytes = root.window();
        if archive::has_zip_magic(bytes) {
            return decode_zip(ctx, root);
        }
        refuse_alternate_encoding(bytes)?;
        if self.detect(bytes) == Confidence::No {
            return Err(CodecError::WrongFormat("missing ISO-10303-21 magic".into()));
        }
        reader::decode(
            bytes,
            DecodeOptions {
                container_only: ctx.container_only(),
                policy: *ctx.policy(),
            },
            ctx,
        )
    }
}

fn starts_with_step_magic(bytes: &[u8]) -> bool {
    let mut at = 0;
    loop {
        while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
            at += 1;
        }
        if bytes.get(at..at + 2) != Some(b"/*") {
            break;
        }
        let Some(relative_end) = bytes[at + 2..]
            .windows(2)
            .position(|window| window == b"*/")
        else {
            return false;
        };
        at += relative_end + 4;
    }
    let magic = b"ISO-10303-21;";
    bytes
        .get(at..at + magic.len())
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(magic))
}

fn inspect_zip(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    root: cadmpeg_core::decode::View<'_>,
) -> Result<ContainerSummary, CodecError> {
    let (archive, root_view) = archive::open_root(ctx, root)?;
    let root_summary = StepCodec::default().inspect_impl(ctx, root_view)?;
    let resource_notes = archive::root_reference_notes(&archive, root_view.window())?;
    let entry_count = archive.entries().len();
    let root_entry = archive
        .entry(archive::ROOT_NAME)
        .expect("validated STEP ZIP root");
    let root_data_offset = root_entry.data_start;
    let logical_entries = root_summary
        .entries
        .iter()
        .map(|entry| entry.name.as_str())
        .collect::<Vec<_>>()
        .join(",");
    let mut entries = archive.container_entries(archive::classify_entry);
    if let Some(root_entry) = entries
        .iter_mut()
        .find(|entry| entry.name == archive::ROOT_NAME)
    {
        root_entry
            .attributes
            .insert("logical_sections".into(), logical_entries);
    }
    let mut notes = vec![
        format!("root {}", archive::ROOT_NAME),
        format!("archive entries={entry_count}; root data offset={root_data_offset}"),
    ];
    notes.extend(root_summary.notes);
    notes.extend(resource_notes);
    Ok(ContainerSummary {
        format: "step".into(),
        container_kind: "iso-10303-21-zip".into(),
        entries,
        notes,
    })
}

fn decode_zip(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    root: cadmpeg_core::decode::View<'_>,
) -> Result<DecodeResult, CodecError> {
    let (archive, root_view) = archive::open_root(ctx, root)?;
    let resource_notes = archive::root_reference_notes(&archive, root_view.window())?;
    let entry_count = archive.entries().len();
    let root_entry = archive
        .entry(archive::ROOT_NAME)
        .expect("validated STEP ZIP root");
    let root_data_offset = root_entry.data_start;
    let mut result = reader::decode(
        root_view.window(),
        DecodeOptions {
            container_only: ctx.container_only(),
            policy: *ctx.policy(),
        },
        ctx,
    )?;
    if let Some(source) = &mut result.ir_mut().source {
        source
            .attributes
            .insert("container_kind".into(), "iso-10303-21-zip".into());
        source
            .attributes
            .insert("archive_root".into(), archive::ROOT_NAME.into());
        source
            .attributes
            .insert("archive_entries".into(), entry_count.to_string());
        source.attributes.insert(
            "archive_root_data_offset".into(),
            root_data_offset.to_string(),
        );
    }
    result.report_mut().notes.push(format!(
        "container root {}; archive entries={entry_count}",
        archive::ROOT_NAME
    ));
    result.report_mut().notes.extend(resource_notes);
    Ok(result)
}

fn refuse_alternate_encoding(bytes: &[u8]) -> Result<(), CodecError> {
    if bytes.starts_with(b"\x89HDF\r\n\x1a\n") {
        return Err(CodecError::NotImplemented(
            "STEP Part 26 binary/HDF5 encoding".into(),
        ));
    }
    if is_part28_xml(bytes) {
        return Err(CodecError::NotImplemented(
            "STEP Part 28 XML encoding".into(),
        ));
    }
    // BM-02: alternate XML remains outside the Part 21 graph; no implicit
    // sidecar composition occurs here.
    if is_ap242_bo_model_xml(bytes) {
        return Err(CodecError::NotImplemented(
            "AP242 BO-Model XML sidecar".into(),
        ));
    }
    Ok(())
}

fn is_part28_xml(bytes: &[u8]) -> bool {
    let lower = bytes
        .iter()
        .take(4096)
        .map(u8::to_ascii_lowercase)
        .collect::<Vec<_>>();
    lower.starts_with(b"<?xml")
        && (lower.windows(12).any(|window| window == b"iso_10303_28")
            || lower
                .windows(21)
                .any(|window| window == b"iso:std:iso:10303:-28"))
}

const AP242_BO_MODEL_NAMESPACES: [&[u8]; 2] = [
    b"http://standards.iso.org/iso/ts/10303/-3001/-ed-1/tech/xml-schema/bo_model",
    b"http://standards.iso.org/iso/ts/10303/-3001/-ed-2/tech/xml-schema/bo_model",
];

// BM-01: the root namespace is the schema identity; local names and schema
// locations alone do not establish an AP242 BO-Model document.
fn is_ap242_bo_model_xml(bytes: &[u8]) -> bool {
    let bytes = &bytes[..bytes.len().min(4096)];
    let Some((name, attributes)) = xml_root_start_tag(bytes) else {
        return false;
    };
    let Some(separator) = name.iter().position(|byte| *byte == b':') else {
        return name == b"Uos"
            && namespace_for_prefix(attributes, &[])
                .is_some_and(|namespace| AP242_BO_MODEL_NAMESPACES.contains(&namespace));
    };
    if &name[separator + 1..] != b"Uos" {
        return false;
    }
    let prefix = &name[..separator];
    namespace_for_prefix(attributes, prefix)
        .is_some_and(|namespace| AP242_BO_MODEL_NAMESPACES.contains(&namespace))
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

fn namespace_for_prefix<'a>(attributes: &'a [u8], prefix: &[u8]) -> Option<&'a [u8]> {
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
        let matches_prefix = if prefix.is_empty() {
            name == b"xmlns"
        } else {
            name.strip_prefix(b"xmlns:") == Some(prefix)
        };
        if matches_prefix {
            return Some(value);
        }
    }
    None
}
