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
            || is_part26_hdf5(prefix)
            || is_part28_xml(prefix)
            || is_ap242_bo_model_xml(prefix)
        {
            Confidence::Medium
        } else if archive::has_zip_magic(prefix) {
            Confidence::Low
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
        while bytes
            .get(at)
            .is_some_and(|byte| byte.is_ascii_control() || *byte == b' ')
        {
            at += 1;
        }
        if bytes.get(at..at + 2) == Some(b"/*") {
            at += 2;
            let Some(relative_end) = bytes[at..].windows(2).position(|window| window == b"*/")
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
    for &expected_byte in b"ISO-10303-21;" {
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
    // CE-03/CE-04: Part 28 marker detection is not UOS conformance or schema
    // mapping. The caller owns the exact binding, governing EXPRESS schema,
    // derived XML Schema, identity/reference checks, and validation result;
    // this codec has no Part 28 adapter and builds no partial graph.
    // CE-05: HDF5 signature detection is not HDF5 validation or Part 26
    // mapping. The caller owns the mapping edition, governing EXPRESS schema,
    // HDF5 and Part 26 validation, resource-local row/reference mapping, and
    // malformed-input result; this codec builds no partial graph.
    // CE-06: Part 26 and Part 21 resource graphs have no universal join. The caller
    // owns the exact resource identities, row-to-occurrence map, schema/unit/context
    // agreement, conflict policy, and retention of both source graphs.
    if is_part26_hdf5(bytes) {
        return Err(CodecError::NotImplemented(
            "STEP Part 26 binary/HDF5 encoding".into(),
        ));
    }
    if is_part28_xml(bytes) {
        return Err(CodecError::NotImplemented(
            "STEP Part 28 XML encoding".into(),
        ));
    }
    if is_ap242_bo_model_xml(bytes) {
        return Err(CodecError::NotImplemented(
            "AP242 BO-Model XML sidecar".into(),
        ));
    }
    Ok(())
}

fn is_part26_hdf5(bytes: &[u8]) -> bool {
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

fn is_part28_xml(bytes: &[u8]) -> bool {
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

fn is_ap242_bo_model_xml(bytes: &[u8]) -> bool {
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

    use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions};

    use super::{starts_with_step_magic, StepCodec};

    #[test]
    fn detects_magic_after_ignored_controls_and_inside_token() {
        let source = b"\0 /* leading comment */ \\N\\ ISO-10303-\n21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
        let codec = StepCodec::default();

        assert!(starts_with_step_magic(source));
        assert_eq!(codec.detect(source), Confidence::High);
        codec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .expect("decode Part 21 with ignored framing octets");

        let with_bom = [b"\xEF\xBB\xBF".as_slice(), source].concat();
        assert_eq!(codec.detect(&with_bom), Confidence::No);
        assert!(!starts_with_step_magic(b"/* incomplete ISO-10303-21;"));
    }
}
