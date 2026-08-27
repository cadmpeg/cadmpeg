// SPDX-License-Identifier: Apache-2.0
//! Lossless retained-document serialization.

use std::collections::HashSet;
use std::io::{Seek, SeekFrom, Write};

use cadmpeg_core::dialect::DialectId;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{unsupported_target, EncodeInput, ExportPlan, TargetRequest};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::report::{ExportReport, FidelityResolution};
use zip::write::SimpleFileOptions;

use crate::dialect;
use crate::native::{
    DocumentFacts, EntryRecord, ExtensionRecord, ObjectRecord, PropertyRecord, ValueRecord,
};
use crate::FcstdWriteOptions;

pub(crate) trait WriteSeek: Write + Seek {}
impl<T: Write + Seek> WriteSeek for T {}

/// What resolving a [`TargetRequest`] against the source decided (design §8.2).
struct Resolution {
    /// The persistence band to write.
    options: FcstdWriteOptions,
    /// Why the source's own dialect is not what gets written, when it is not.
    /// `None` on the preservation path, and `None` when there is no `fcstd`
    /// source at all: nothing was preserved, so nothing was lost.
    declined: Option<String>,
}

/// Resolve the request against the source, then plan the export it names.
///
/// `Explicit(id)` refuses an id outside the synthesis catalog. It is otherwise
/// the replay law's compare: the retained document is written back exactly when
/// `id` is the source's dialect, and any other id is a transcode the writer
/// declines below.
///
/// `Inherit` asks for preservation instead. This writer repacks the retained
/// entry set and patches `Document.xml` inside it, which reproduces whatever
/// schema the source declared — schema 2 and schema 3 included, neither of which
/// is a synthesis target. Where the retained document graph cannot carry the
/// source's dialect, `Inherit` falls to synthesizing that dialect, and refuses
/// when it is not in the catalog. There is no fall-through to the catalog
/// default: a same-format conversion never silently changes what the file is.
/// `fcstd:schema-2` is the canonical case, and an explicit `--to` is the escape.
pub(crate) fn plan<'a>(
    input: EncodeInput<'a>,
    request: TargetRequest<'_>,
) -> Result<ExportPlan<'a>, CodecError> {
    let resolution = resolve(input.ir, request)?;
    let mut bytes = Vec::new();
    let mut report = write(input.ir, &mut bytes, resolution.options)?;
    // `write` takes no fidelity sidecar, so the report it returns states the
    // only resolution it can see. Whether the caller supplied one, and whether
    // the source's own dialect survived, are known here and only here.
    report.fidelity = match resolution.declined {
        Some(reason) => FidelityResolution::Degraded { reason },
        None if input.fidelity.is_some() => FidelityResolution::NotConsumed,
        None => FidelityResolution::NotProvided,
    };
    Ok(ExportPlan::buffered(report, bytes))
}

/// Decide what to write, from the request and the source (design §8.2).
fn resolve(ir: &CadIr, request: TargetRequest<'_>) -> Result<Resolution, CodecError> {
    let source_dialect = ir
        .source
        .as_ref()
        .filter(|source| source.format == dialect::FORMAT)
        .and_then(|source| source.dialect.clone());
    match request {
        TargetRequest::Explicit(id) => {
            let options = dialect::target_options(id).ok_or_else(|| {
                unsupported_target(
                    dialect::FORMAT,
                    id,
                    "not a target this encoder can synthesize",
                    dialect::TARGETS,
                )
            })?;
            let target = dialect::written_dialect(options);
            let declined = match &source_dialect {
                Some(source) if *source == target => None,
                Some(source) => Some(format!(
                    "source is {source}, target is {target}; the retained FCStd document is not written back"
                )),
                // No FCStd source: a constructed graph, or another format's
                // document. There is no source dialect to lose, so the write is
                // not a degraded one.
                None => None,
            };
            Ok(Resolution { options, declined })
        }
        TargetRequest::Inherit => {
            let Some(source_dialect) = source_dialect else {
                // Nothing to inherit: no FCStd source, or one that records no
                // dialect. Neither is reachable from the command line, which
                // builds `Inherit` only for an FCStd source.
                return Ok(Resolution {
                    options: FcstdWriteOptions::default(),
                    declined: None,
                });
            };
            if let Some(options) = retained_baseline(ir, &source_dialect) {
                return Ok(Resolution {
                    options,
                    declined: None,
                });
            }
            let Some(options) = dialect::target_options(source_dialect.as_str()) else {
                return Err(unsupported_target(
                    dialect::FORMAT,
                    source_dialect.as_str(),
                    "its retained document graph cannot be written back and this encoder cannot \
                     synthesize it",
                    dialect::TARGETS,
                ));
            };
            Ok(Resolution {
                options,
                declined: Some(format!(
                    "the retained FCStd document graph does not carry {source_dialect}"
                )),
            })
        }
    }
}

/// The write options that reproduce `source_dialect` from the retained document
/// graph, or `None` where that graph cannot carry it.
///
/// The graph is the whole baseline: the writer never regenerates a
/// `Document.xml`, so preservation is possible exactly when the retained
/// document record is present, declares the source's own dialect, and declares
/// it in a form the write options can restate. A `SchemaVersion` of `"04"`
/// classifies as `fcstd:unknown` and does not round-trip through `u32`, so it
/// fails the last condition rather than being rewritten as `"4"`.
fn retained_baseline(ir: &CadIr, source_dialect: &DialectId) -> Option<FcstdWriteOptions> {
    let namespace = ir.native.namespace("fcstd")?;
    let documents = namespace.arena_as::<DocumentFacts>("document").ok()?;
    let [document] = documents.as_slice() else {
        return None;
    };
    let options = FcstdWriteOptions {
        schema_version: document.schema_version.parse().ok()?,
        file_version: document.file_version.parse().ok()?,
    };
    (options.schema_version.to_string() == document.schema_version
        && options.file_version.to_string() == document.file_version
        && dialect::written_dialect(options) == *source_dialect)
        .then_some(options)
}

pub(crate) fn write(
    ir: &CadIr,
    output: &mut dyn Write,
    options: FcstdWriteOptions,
) -> Result<ExportReport, CodecError> {
    let mut staged = tempfile::tempfile()?;
    let report = write_seekable(ir, &mut staged, options)?;
    staged.seek(SeekFrom::Start(0))?;
    std::io::copy(&mut staged, output)?;
    Ok(report)
}

pub(crate) fn write_seekable(
    ir: &CadIr,
    output: &mut dyn WriteSeek,
    options: FcstdWriteOptions,
) -> Result<ExportReport, CodecError> {
    let namespace = ir.native.namespace("fcstd").ok_or_else(|| {
        CodecError::NotImplemented(
            "source-less FCStd generation requires a constructed native document graph".into(),
        )
    })?;
    let documents = namespace.arena_as::<DocumentFacts>("document")?;
    let document = exactly_one(&documents, "document record")?;
    // The replay law, on the typed dialect: this writer patches the retained
    // `Document.xml` and regenerates none, so it may write only the dialect the
    // retained document already declares. `FileVersion` is not part of a dialect
    // id, so it is the second half of the gate.
    let target = dialect::written_dialect(options);
    let retained = dialect::FcstdDialect::from_schema_version(&document.schema_version).id();
    if retained != target || document.file_version != options.file_version.to_string() {
        return Err(CodecError::NotImplemented(format!(
            "cannot transcode retained {retained} (SchemaVersion={} FileVersion={}) to {target} \
             (SchemaVersion={} FileVersion={})",
            document.schema_version,
            document.file_version,
            options.schema_version,
            options.file_version
        )));
    }
    let entry_records = namespace
        .arenas
        .get("entries")
        .map_or(&[][..], Vec::as_slice);
    let mut entries = entry_records
        .iter()
        .enumerate()
        .map(|(record_index, record)| {
            record
                .field("name")
                .and_then(|name| name.as_str().map(str::to_owned))
                .map(|name| EntrySlot { record_index, name })
                .ok_or_else(|| {
                    CodecError::Malformed("FCStd entry record has no string name".into())
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let objects = namespace.arena_as::<ObjectRecord>("objects")?;
    let extensions = namespace.arena_as::<ExtensionRecord>("extensions")?;
    let properties = namespace.arena_as::<PropertyRecord>("properties")?;
    validate_entry_names(&entries)?;
    let source_document_slot = entries
        .iter()
        .find(|entry| entry.name == "Document.xml")
        .ok_or_else(|| {
            CodecError::Malformed("FCStd native graph has no Document.xml entry".into())
        })?;
    let source_document = entry_at(namespace, source_document_slot.record_index)?;
    validate_entry(&source_document)?;
    let document_xml = patch_document(&source_document.data, &properties)?;
    drop(source_document);
    let written_graph = crate::persistence::parse(&document_xml)?;
    validate_declarations(
        &objects,
        &extensions,
        &written_graph.objects,
        &written_graph.extensions,
    )?;
    for property in &written_graph.properties {
        for entry in &property.side_entries {
            if !entries.iter().any(|candidate| candidate.name == *entry) {
                return Err(CodecError::malformed(format_args!(
                    "edited property {} references missing side entry {entry}",
                    property.id
                )));
            }
        }
    }

    {
        let mut archive = zip::ZipWriter::new(&mut *output);
        let file_options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .last_modified_time(zip::DateTime::default());
        entries.sort_by(|left, right| {
            (left.name != "Document.xml", left.name.as_str())
                .cmp(&(right.name != "Document.xml", right.name.as_str()))
        });
        for slot in &entries {
            let entry = entry_at(namespace, slot.record_index)?;
            validate_entry(&entry)?;
            archive
                .start_file(&entry.name, file_options)
                .map_err(|error| {
                    CodecError::malformed(format_args!("cannot write {}: {error}", entry.name))
                })?;
            archive.write_all(if entry.name == "Document.xml" {
                &document_xml
            } else {
                &entry.data
            })?;
        }
        archive.finish().map_err(|error| {
            CodecError::malformed(format_args!("cannot finish FCStd archive: {error}"))
        })?;
    }
    Ok(ExportReport {
        target: Some(target.clone()),
        format: "fcstd".into(),
        census: cadmpeg_ir::EntityCensus {
            basis: cadmpeg_ir::CensusBasis::IrArenas,
            counts: ir.census(),
        },
        fidelity: cadmpeg_ir::FidelityResolution::NotProvided,
        // Refuses without a retained `fcstd` native graph, then rewrites
        // `Document.xml` inside that entry set and repacks the rest.
        write_path: cadmpeg_ir::WritePath::Patched,
        losses: Vec::new(),
        notes: vec![
            format!(
                "semantic FCStd archive written for {target} (SchemaVersion={} FileVersion={})",
                options.schema_version, options.file_version
            ),
            "unsupported retained entries and unedited XML records were preserved".into(),
        ],
    })
}

fn exactly_one<'a, T>(values: &'a [T], description: &str) -> Result<&'a T, CodecError> {
    if values.len() != 1 {
        return Err(CodecError::malformed(format_args!(
            "FCStd native graph must contain exactly one {description}"
        )));
    }
    Ok(&values[0])
}

struct EntrySlot {
    record_index: usize,
    name: String,
}

fn entry_at(
    namespace: &cadmpeg_ir::native::NativeNamespace,
    index: usize,
) -> Result<EntryRecord, CodecError> {
    namespace
        .arena_iter_as::<EntryRecord>("entries")
        .nth(index)
        .ok_or_else(|| CodecError::Malformed("FCStd entry record disappeared".into()))?
        .map_err(CodecError::from)
}

fn validate_entry_names(entries: &[EntrySlot]) -> Result<(), CodecError> {
    let mut names = HashSet::new();
    for entry in entries {
        if entry.name.is_empty()
            || entry.name.starts_with('/')
            || entry
                .name
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(CodecError::malformed(format_args!(
                "unsafe FCStd output entry name {:?}",
                entry.name
            )));
        }
        if !names.insert(entry.name.as_str()) {
            return Err(CodecError::malformed(format_args!(
                "duplicate FCStd output entry {}",
                entry.name
            )));
        }
    }
    Ok(())
}

fn validate_entry(entry: &EntryRecord) -> Result<(), CodecError> {
    if entry.byte_len != entry.data.len() as u64 || entry.sha256 != sha256_hex(&entry.data) {
        return Err(CodecError::malformed(format_args!(
            "FCStd output entry {} has stale length or digest metadata",
            entry.name
        )));
    }
    Ok(())
}

fn validate_declarations(
    expected_objects: &[ObjectRecord],
    expected_extensions: &[ExtensionRecord],
    written_objects: &[ObjectRecord],
    written_extensions: &[ExtensionRecord],
) -> Result<(), CodecError> {
    if expected_objects.len() != written_objects.len()
        || !expected_objects.iter().all(|expected| {
            written_objects
                .iter()
                .find(|written| written.id == expected.id)
                .is_some_and(|written| {
                    expected.id == written.id
                        && expected.name == written.name
                        && expected.type_name == written.type_name
                        && expected.persistent_id == written.persistent_id
                        && expected.view_type == written.view_type
                        && expected.attributes == written.attributes
                        && expected.dependencies == written.dependencies
                        && expected.dependency_allow_partial == written.dependency_allow_partial
                        && expected.order == written.order
                })
        })
    {
        return Err(CodecError::NotImplemented(
            "object declaration edits require source-less graph regeneration".into(),
        ));
    }
    if expected_extensions.len() != written_extensions.len()
        || !expected_extensions.iter().all(|expected| {
            written_extensions
                .iter()
                .find(|written| written.id == expected.id)
                == Some(expected)
        })
    {
        return Err(CodecError::NotImplemented(
            "extension declaration edits require a typed serializer".into(),
        ));
    }
    Ok(())
}

fn patch_document(source: &[u8], properties: &[PropertyRecord]) -> Result<Vec<u8>, CodecError> {
    let source_text = std::str::from_utf8(source)
        .map_err(|_| CodecError::Malformed("retained Document.xml is not UTF-8".into()))?;
    let mut ordered = properties.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|property| property.byte_start);
    if ordered
        .windows(2)
        .any(|pair| pair[0].byte_end > pair[1].byte_start)
    {
        return Err(CodecError::Malformed(
            "overlapping retained FCStd property spans".into(),
        ));
    }
    let mut result = Vec::with_capacity(source.len());
    let mut cursor = 0usize;
    for property in ordered {
        let start = usize::try_from(property.byte_start)
            .map_err(|_| CodecError::Malformed("property start exceeds address space".into()))?;
        let end = usize::try_from(property.byte_end)
            .map_err(|_| CodecError::Malformed("property end exceeds address space".into()))?;
        if start < cursor || end > source.len() || start >= end {
            return Err(CodecError::malformed(format_args!(
                "invalid retained span for property {}",
                property.id
            )));
        }
        let retained = source_text.get(start..end).ok_or_else(|| {
            CodecError::malformed(format_args!(
                "retained span for property {} is not on UTF-8 boundaries",
                property.id
            ))
        })?;
        if retained != property.raw_xml {
            return Err(CodecError::malformed(format_args!(
                "retained bytes disagree with property {} provenance",
                property.id
            )));
        }
        result.extend_from_slice(&source[cursor..start]);
        result.extend_from_slice(&serialize_property(property)?);
        cursor = end;
    }
    result.extend_from_slice(&source[cursor..]);
    Ok(result)
}

fn serialize_property(property: &PropertyRecord) -> Result<Vec<u8>, CodecError> {
    validate_property_wrapper(property)?;
    let mut replacement = property.raw_xml.clone();
    let wrapped = format!("<Root>{}</Root>", property.raw_xml);
    let parsed = roxmltree::Document::parse(&wrapped).map_err(|error| {
        CodecError::malformed(format_args!("invalid retained property XML: {error}"))
    })?;
    let source_ranges = parsed
        .root_element()
        .first_element_child()
        .into_iter()
        .flat_map(|property| {
            property
                .descendants()
                .filter(move |node| node.is_element() && *node != property)
        })
        .map(|node| (node.range().start - 6, node.range().end - 6))
        .collect::<Vec<_>>();
    if source_ranges.len() != property.values.len() {
        return Err(CodecError::malformed(format_args!(
            "property {} value provenance count changed",
            property.id
        )));
    }
    let mut edits = Vec::new();
    for (value, (start, end)) in property.values.iter().zip(source_ranges) {
        let serialized = serialize_value(value)?;
        if serialized == value.raw_xml {
            continue;
        }
        if property.raw_xml[start..end] != value.raw_xml {
            return Err(CodecError::malformed(format_args!(
                "property {} retained value {} disagrees with provenance",
                property.id, value.order
            )));
        }
        edits.push((start, end, serialized));
    }
    edits.sort_by_key(|(start, _, _)| *start);
    if edits.windows(2).any(|pair| pair[0].1 > pair[1].0) {
        return Err(CodecError::NotImplemented(format!(
            "overlapping edits in nested FCStd property {}",
            property.id
        )));
    }
    for (start, end, serialized) in edits.into_iter().rev() {
        replacement.replace_range(start..end, &serialized);
    }
    Ok(replacement.into_bytes())
}

fn validate_property_wrapper(property: &PropertyRecord) -> Result<(), CodecError> {
    let wrapped = format!("<Root>{}</Root>", property.raw_xml);
    let parsed = roxmltree::Document::parse(&wrapped).map_err(|error| {
        CodecError::malformed(format_args!("invalid retained property XML: {error}"))
    })?;
    let element = parsed
        .root_element()
        .first_element_child()
        .ok_or_else(|| CodecError::Malformed("retained property has no element".into()))?;
    let expected_tag = if property.transient {
        "_Property"
    } else {
        "Property"
    };
    let status = element
        .attribute("status")
        .map(str::parse::<u64>)
        .transpose()
        .map_err(|_| CodecError::Malformed("retained property has invalid status".into()))?;
    if element.tag_name().name() != expected_tag
        || element.attribute("name") != Some(property.name.as_str())
        || element.attribute("type") != Some(property.type_name.as_str())
        || status != property.status
    {
        return Err(CodecError::NotImplemented(format!(
            "editing FCStd property declaration {} requires a typed serializer",
            property.id
        )));
    }
    Ok(())
}

fn serialize_value(value: &ValueRecord) -> Result<String, CodecError> {
    let wrapped = format!("<Root>{}</Root>", value.raw_xml);
    let parsed = roxmltree::Document::parse(&wrapped).map_err(|error| {
        CodecError::malformed(format_args!("invalid retained property value XML: {error}"))
    })?;
    let original = parsed
        .root_element()
        .first_element_child()
        .ok_or_else(|| CodecError::Malformed("retained property value has no element".into()))?;
    let original_attributes = original
        .attributes()
        .map(|attribute| (attribute.name().to_owned(), attribute.value().to_owned()))
        .collect::<std::collections::BTreeMap<_, _>>();
    let original_text = original
        .children()
        .find_map(|node| node.text())
        .map(str::to_owned);
    if original.tag_name().name() == value.tag
        && original_attributes == value.attributes
        && original_text == value.text
    {
        return Ok(value.raw_xml.clone());
    }
    if original.children().any(|node| node.is_element()) {
        return Err(CodecError::NotImplemented(format!(
            "editing nested FCStd value element {} requires a typed serializer",
            value.tag
        )));
    }
    let mut serialized = String::new();
    serialized.push('<');
    serialized.push_str(&value.tag);
    for (name, content) in &value.attributes {
        serialized.push(' ');
        serialized.push_str(name);
        serialized.push_str("=\"");
        escape_xml(content, &mut serialized, true);
        serialized.push('"');
    }
    match &value.text {
        Some(text) => {
            serialized.push('>');
            escape_xml(text, &mut serialized, false);
            serialized.push_str("</");
            serialized.push_str(&value.tag);
            serialized.push('>');
        }
        None => serialized.push_str("/>"),
    }
    Ok(serialized)
}

pub(crate) fn escape_xml(value: &str, output: &mut String, attribute: bool) {
    for character in value.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' if attribute => output.push_str("&quot;"),
            '\'' if attribute => output.push_str("&apos;"),
            '\t' => output.push_str("&#9;"),
            '\n' => output.push_str("&#10;"),
            '\r' => output.push_str("&#13;"),
            other => output.push(other),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::test_support::*;
    use crate::FcstdCodec;
    use cadmpeg_ir::codec::{EncodeInput, TargetRequest};
    use cadmpeg_ir::{Codec, DecodeOptions, Encoder};
    use std::io::Cursor;

    #[test]
    fn property_edits_use_value_order_when_raw_xml_is_identical() {
        let raw_value = r#"<String value="same"/>"#;
        let mut values = (0..2)
            .map(|order| ValueRecord {
                tag: "String".into(),
                order,
                attributes: [("value".into(), "same".into())].into(),
                text: None,
                raw_xml: raw_value.into(),
            })
            .collect::<Vec<_>>();
        values[1]
            .attributes
            .insert("value".into(), "changed".into());
        let property = PropertyRecord {
            id: "test:property#values".into(),
            owner: "test:object#owner".into(),
            name: "Values".into(),
            type_name: "App::PropertyStringList".into(),
            family: crate::native::PropertyFamily::List,
            status: None,
            transient: false,
            dynamic: None,
            order: 0,
            values,
            links: Vec::new(),
            side_entries: Vec::new(),
            raw_xml: format!(
                r#"<Property name="Values" type="App::PropertyStringList">{raw_value}{raw_value}</Property>"#
            ),
            byte_start: 0,
            byte_end: 0,
        };
        let output = String::from_utf8(serialize_property(&property).expect("required invariant"))
            .expect("required invariant");
        assert_eq!(output.matches(r#"value="same""#).count(), 1);
        assert_eq!(output.matches(r#"value="changed""#).count(), 1);
        assert!(
            output.find("same").expect("required invariant")
                < output.find("changed").expect("required invariant")
        );
    }

    #[test]
    fn xml_serialization_preserves_normalized_whitespace() {
        let value = ValueRecord {
            tag: "String".into(),
            order: 0,
            attributes: [("value".into(), "a\tb\nc\rd".into())].into(),
            text: Some("a\tb\nc\rd".into()),
            raw_xml: r#"<String value="old">old</String>"#.into(),
        };
        let serialized = serialize_value(&value).expect("required invariant");
        assert!(serialized.contains("a&#9;b&#10;c&#13;d"));
        assert_eq!(serialized.matches("&#9;").count(), 2);
        assert_eq!(serialized.matches("&#10;").count(), 2);
        assert_eq!(serialized.matches("&#13;").count(), 2);
    }

    #[test]
    fn writes_typed_property_edits_and_preserves_other_entries() {
        let decoded = FcstdCodec
            .decode(
                &mut Cursor::new(CORE_DESIGN_PRODUCT),
                &DecodeOptions::default(),
            )
            .expect("decode source");
        let source_entries = decoded
            .ir()
            .native
            .namespace("fcstd")
            .expect("namespace")
            .arena_as::<crate::native::EntryRecord>("entries")
            .expect("entries");
        let mut edited = decoded.ir().clone();
        FcstdCodec
            .set_property_value_attribute(
                &mut edited,
                crate::FcstdPropertyOwner::Document,
                "Label",
                0,
                "value",
                "edited & verified",
            )
            .expect("edit Label");

        let mut encoded = Vec::new();
        let report = FcstdCodec
            .plan(EncodeInput::new(&edited, None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut encoded))
            .expect("encode edit");
        assert!(report.losses.is_empty());
        let round_trip = FcstdCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("decode output");
        let output_namespace = round_trip
            .ir()
            .native
            .namespace("fcstd")
            .expect("namespace");
        let output_properties = output_namespace
            .arena_as::<crate::native::PropertyRecord>("properties")
            .expect("properties");
        let output_label = output_properties
            .iter()
            .find(|property| {
                property.owner == crate::native::native_id("document", "0")
                    && property.name == "Label"
            })
            .expect("document Label");
        assert_eq!(
            output_label.values[0]
                .attributes
                .get("value")
                .map(String::as_str),
            Some("edited & verified")
        );
        let output_entries = output_namespace
            .arena_as::<crate::native::EntryRecord>("entries")
            .expect("entries");
        for source in source_entries
            .iter()
            .filter(|entry| entry.name != "Document.xml")
        {
            let output = output_entries
                .iter()
                .find(|entry| entry.name == source.name)
                .expect("preserved entry");
            assert_eq!(output.data, source.data, "{}", source.name);
        }
        assert!(crate::validate_native(round_trip.ir()).is_empty());
    }

    #[test]
    pub(crate) fn write_target_and_source_requirements_are_explicit() {
        let decoded = FcstdCodec
            .decode(
                &mut Cursor::new(CORE_DESIGN_PRODUCT),
                &DecodeOptions::default(),
            )
            .expect("decode source");
        let unsupported = FcstdCodec
            .encode_with_options(
                decoded.ir(),
                &mut Vec::new(),
                crate::FcstdWriteOptions {
                    schema_version: 3,
                    file_version: 1,
                },
            )
            .expect_err("unsupported target must fail");
        assert!(unsupported.to_string().contains("SchemaVersion=3"));

        let source_less = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
        let missing_graph = FcstdCodec
            .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut Vec::new()))
            .expect_err("missing graph must fail");
        assert!(missing_graph.to_string().contains("source-less"));
    }

    #[test]
    fn seekable_encoder_matches_the_write_only_fallback() {
        let decoded = FcstdCodec
            .decode(
                &mut Cursor::new(CORE_DESIGN_PRODUCT),
                &DecodeOptions::default(),
            )
            .expect("decode source");
        let mut staged = Vec::new();
        FcstdCodec
            .plan(EncodeInput::new(decoded.ir(), None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut staged))
            .expect("write-only fallback");
        let mut streamed = Cursor::new(Vec::new());
        crate::writer::write_seekable(
            decoded.ir(),
            &mut streamed,
            crate::FcstdWriteOptions::default(),
        )
        .expect("seekable writer");

        assert_eq!(streamed.into_inner(), staged);
    }

    #[test]
    pub(crate) fn writer_rejects_unserialized_declaration_and_stale_payload_edits() {
        let decoded = FcstdCodec
            .decode(
                &mut Cursor::new(CORE_DESIGN_PRODUCT),
                &DecodeOptions::default(),
            )
            .expect("decode source");

        let mut declaration_edit = decoded.ir().clone();
        let namespace = declaration_edit.native.namespace_mut("fcstd");
        let mut objects = namespace
            .arena_as::<crate::native::ObjectRecord>("objects")
            .expect("objects");
        objects[0].type_name = "App::FeaturePython".into();
        namespace
            .set_arena("objects", &objects)
            .expect("replace objects");
        let error = FcstdCodec
            .plan(
                EncodeInput::new(&declaration_edit, None),
                TargetRequest::Inherit,
            )
            .and_then(|plan| plan.write_to(&mut Vec::new()))
            .expect_err("unserialized declaration edit must fail");
        assert!(error.to_string().contains("declaration edits"));

        let (mut stale_entry, _, _) = decoded.into_parts();
        let namespace = stale_entry.native.namespace_mut("fcstd");
        let mut entries = namespace
            .arena_as::<crate::native::EntryRecord>("entries")
            .expect("entries");
        entries
            .iter_mut()
            .find(|entry| entry.name != "Document.xml")
            .expect("side entry")
            .data
            .push(0);
        namespace
            .set_arena("entries", &entries)
            .expect("replace entries");
        let error = FcstdCodec
            .plan(EncodeInput::new(&stale_entry, None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut Vec::new()))
            .expect_err("stale entry metadata must fail");
        assert!(error.to_string().contains("stale length or digest"));
    }
    /// An explicit target this writer does not produce is refused by `plan`
    /// itself, with the catalog in the message.
    ///
    /// The check runs before any synthesis, so an empty document is enough:
    /// what is under test is that the request reaches the encoder at all. This
    /// writer reaches one schema through the trait, which is exactly why the
    /// refusal must exist — every other id is a claim it cannot honour.
    #[test]
    fn plan_refuses_an_explicit_target_outside_the_catalog() {
        let ir = CadIr::empty(cadmpeg_ir::units::Units::default());
        let error = Encoder::plan(
            &FcstdCodec,
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit("fcstd:nonesuch"),
        )
        .err()
        .expect("an id outside the catalog is refused");

        let cadmpeg_core::CodecError::UnsupportedTarget {
            format,
            requested,
            available,
            ..
        } = &error
        else {
            panic!("expected a target refusal, got {error}");
        };
        assert_eq!(format, "fcstd");
        assert_eq!(requested, "fcstd:nonesuch");
        for target in Encoder::targets(&FcstdCodec) {
            assert!(available.contains(target.id), "{available}");
        }
    }

    /// A schema-2 `Document.xml`, in the `Features`/`FeatureData` vocabulary
    /// that schema declares, wrapped in an archive.
    fn schema_two_archive() -> Vec<u8> {
        archive(
            r#"<Document SchemaVersion="2" ProgramVersion="0.13">
<Properties Count="1"><Property name="Label" type="App::PropertyString"><String value="Document"/></Property></Properties>
<Features Count="1"><Feature type="App::Feature" name="First"/></Features>
<FeatureData Count="1"><Feature name="First"><Properties Count="0"/></Feature></FeatureData>
</Document>"#,
        )
    }

    fn inherit(ir: &CadIr) -> Result<cadmpeg_ir::codec::ExportPlan<'_>, CodecError> {
        Encoder::plan(
            &FcstdCodec,
            EncodeInput::new(ir, None),
            TargetRequest::Inherit,
        )
    }

    /// Names every archive entry with its payload, so preservation can be
    /// asserted against the source rather than against the writer's own output.
    fn entry_payloads(archive: &[u8]) -> std::collections::BTreeMap<String, Vec<u8>> {
        let mut zip = zip::ZipArchive::new(Cursor::new(archive.to_vec())).expect("readable ZIP");
        (0..zip.len())
            .map(|index| {
                let mut entry = zip.by_index(index).expect("archive entry");
                let name = entry.name().to_owned();
                let mut payload = Vec::new();
                std::io::Read::read_to_end(&mut entry, &mut payload).expect("inflate entry");
                (name, payload)
            })
            .collect()
    }

    /// `Inherit` on a schema-4 source states schema 4 and writes every retained
    /// entry back byte for byte, `Document.xml` included.
    ///
    /// This is the catalog dialect, so the resolution and the old hardcoded
    /// `FcstdWriteOptions::default()` agree on what to write. What is new is
    /// that the report states it.
    #[test]
    fn inherit_preserves_a_schema_four_source_entry_for_entry() {
        let decoded = FcstdCodec
            .decode(
                &mut Cursor::new(CORE_DESIGN_PRODUCT),
                &DecodeOptions::default(),
            )
            .expect("decode source");
        let plan = inherit(decoded.ir()).expect("schema 4 is preserved");

        assert_eq!(plan.write_path(), cadmpeg_ir::WritePath::Patched);
        assert_eq!(
            plan.report().target.as_ref().map(ToString::to_string),
            Some("fcstd:schema-4".to_owned())
        );
        let mut written = Vec::new();
        plan.write_to(&mut written).expect("write");
        assert_eq!(
            entry_payloads(&written),
            entry_payloads(CORE_DESIGN_PRODUCT)
        );
    }

    /// The canonical §8.2 case, in its preserving half: a schema-2 source with a
    /// usable retained document graph writes back as schema 2.
    ///
    /// `fcstd:schema-2` is not in `targets()` and never will be — this writer
    /// regenerates no `Document.xml`. Preservation is the other capability, and
    /// it reaches every dialect the codec reads. Before the resolution existed,
    /// `plan` hardcoded `FcstdWriteOptions::default()` and this source was
    /// either rewritten as schema 4 or refused.
    #[test]
    fn inherit_preserves_a_schema_two_source_outside_the_catalog() {
        let source = schema_two_archive();
        let decoded = FcstdCodec
            .decode(&mut Cursor::new(source.clone()), &DecodeOptions::default())
            .expect("decode schema 2");
        assert_eq!(
            decoded
                .ir()
                .source
                .as_ref()
                .and_then(|source| source.dialect.as_ref())
                .map(ToString::to_string),
            Some("fcstd:schema-2".to_owned())
        );
        assert!(
            cadmpeg_ir::codec::find_target(Encoder::targets(&FcstdCodec), "fcstd:schema-2")
                .is_none(),
            "schema 2 is preserved, never synthesized"
        );

        let plan = inherit(decoded.ir()).expect("schema 2 is preserved");
        assert_eq!(
            plan.report().target.as_ref().map(ToString::to_string),
            Some("fcstd:schema-2".to_owned())
        );
        let mut written = Vec::new();
        plan.write_to(&mut written).expect("write");
        assert_eq!(entry_payloads(&written), entry_payloads(&source));

        let round_trip = FcstdCodec
            .decode(&mut Cursor::new(written), &DecodeOptions::default())
            .expect("decode output");
        assert_eq!(
            round_trip
                .ir()
                .source
                .as_ref()
                .and_then(|source| source.dialect.as_ref())
                .map(ToString::to_string),
            Some("fcstd:schema-2".to_owned())
        );
    }

    /// The canonical §8.2 case, in its refusing half: a schema-2 source whose
    /// retained document graph cannot be written back is refused, not quietly
    /// rewritten as schema 4.
    ///
    /// There is no fall-through to the catalog default. The refusal names the
    /// source's own dialect and the catalog, so the caller can reach the file
    /// with an explicit `--to` from the message alone.
    #[test]
    fn inherit_refuses_a_schema_two_source_with_no_usable_baseline() {
        let decoded = FcstdCodec
            .decode(
                &mut Cursor::new(schema_two_archive()),
                &DecodeOptions::default(),
            )
            .expect("decode schema 2");
        let (mut ir, _, _) = decoded.into_parts();
        ir.native
            .namespace_mut("fcstd")
            .set_arena("document", &[] as &[DocumentFacts])
            .expect("drop the document record");

        let error = inherit(&ir)
            .err()
            .expect("a schema-2 source with no baseline is refused");
        let CodecError::UnsupportedTarget {
            format,
            requested,
            available,
            ..
        } = &error
        else {
            panic!("expected a target refusal, got {error}");
        };
        assert_eq!(format, "fcstd");
        assert_eq!(requested, "fcstd:schema-2");
        assert!(available.contains("fcstd:schema-4"), "{available}");
    }

    /// An explicit `--to` is the escape from the inherit refusal, and it is
    /// where this codec's synthesis gap becomes visible.
    ///
    /// The request is in the catalog, so `plan` admits it and declines
    /// preservation with a reason naming both dialects. The write then fails:
    /// this writer patches the retained `Document.xml` and regenerates none, so
    /// schema 2 to schema 4 is a transcode it cannot perform. The refusal names
    /// both sides rather than emitting a schema-4 archive built from schema-2
    /// records.
    #[test]
    fn an_explicit_schema_four_target_declines_a_schema_two_source_by_name() {
        let decoded = FcstdCodec
            .decode(
                &mut Cursor::new(schema_two_archive()),
                &DecodeOptions::default(),
            )
            .expect("decode schema 2");
        let resolution = resolve(decoded.ir(), TargetRequest::Explicit("fcstd:schema-4"))
            .expect("schema 4 is in the catalog");
        assert_eq!(resolution.options, FcstdWriteOptions::default());
        let declined = resolution.declined.expect("preservation is declined");
        assert!(declined.contains("fcstd:schema-2"), "{declined}");
        assert!(declined.contains("fcstd:schema-4"), "{declined}");

        let error = Encoder::plan(
            &FcstdCodec,
            EncodeInput::new(decoded.ir(), None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .err()
        .expect("this writer regenerates no Document.xml");
        let message = error.to_string();
        assert!(message.contains("fcstd:schema-2"), "{message}");
        assert!(message.contains("fcstd:schema-4"), "{message}");
    }
}
