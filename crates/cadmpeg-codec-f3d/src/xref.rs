// SPDX-License-Identifier: Apache-2.0
//! External-reference (`XRef`) and document-type entries of a `.f3d` container
//! ([spec §1.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#12-stored-property-and-configuration-entries),
//! [§1.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#14-external-references)).
//!
//! [`decode`] parses the top-level `RedirectionsStream.dat` table into
//! [`XrefDesign`] and [`XrefReference`] records. [`docstruct`] parses the JSON
//! form of `Properties.dat`. [`is_assembly`] classifies a BREP-less document
//! whose model is the placement of its XREF targets.

use serde::Deserialize;

use cadmpeg_core::bytes::find_in;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{Feature, FeatureDefinition};
use cadmpeg_ir::products::{
    ExternalDocumentReference, ExternalResolution, Occurrence, OccurrenceParent, PrototypeReference,
};

use crate::bytes::{lp_ascii_strict, take_reference};
use crate::container::role;
use crate::container::ContainerScan;
use crate::records::{DesignParameterScope, XrefDesign, XrefReference};

/// Top-level container entry holding the external-reference table.
pub const REDIRECTIONS_ENTRY: &str = "RedirectionsStream.dat";
/// Top-level container entry holding the document-properties slot.
pub const PROPERTIES_ENTRY: &str = "Properties.dat";
/// Top-level JSON document carrying component-reference extension data.
pub const COMPONENT_REFERENCE_ENTRY: &str = "ComponentReferenceData.json";

/// The parsed `RedirectionsStream.dat` table.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct XrefTable {
    /// Design entries in source order; entry 0 is the document itself.
    pub designs: Vec<XrefDesign>,
    /// Outgoing XREF placements in source order; empty for a leaf document.
    pub references: Vec<XrefReference>,
}

/// The `docstruct` document-type declaration of a JSON `Properties.dat`.
#[derive(Debug, Clone, PartialEq)]
pub struct Docstruct {
    /// Document type: `assembly-design` or `part-design`.
    pub doc_type: String,
    /// Document subtype, e.g. `assembly-standard` or `part-sheetmetal`.
    pub subtype: String,
}

#[derive(Deserialize)]
struct RedirectionsJson {
    #[serde(default)]
    designs: Vec<DesignJson>,
    /// `{}` in a leaf document, an array in a referencing document.
    #[serde(default)]
    references: ReferencesJson,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ReferencesJson {
    List(Vec<ReferenceJson>),
    /// The leaf form `references: {}`; any non-array value carries no
    /// references.
    Other(serde::de::IgnoredAny),
}

impl Default for ReferencesJson {
    fn default() -> Self {
        ReferencesJson::List(Vec::new())
    }
}

#[derive(Deserialize)]
struct DesignJson {
    #[serde(rename = "file-version", default)]
    file_version: i64,
    #[serde(rename = "targetFileName", default)]
    target_file_name: String,
    #[serde(rename = "displayName", default)]
    display_name: String,
    #[serde(rename = "lineageUrn", default)]
    lineage_urn: String,
    #[serde(rename = "versionUrn", default)]
    version_urn: String,
}

#[derive(Deserialize)]
struct ReferenceJson {
    #[serde(default)]
    from: String,
    #[serde(rename = "relativePath", default)]
    relative_path: String,
    #[serde(rename = "type", default)]
    reference_type: String,
    #[serde(default)]
    properties: Vec<serde_json::Map<String, serde_json::Value>>,
}

impl ReferenceJson {
    fn property(&self, name: &str) -> String {
        self.properties
            .iter()
            .find_map(|object| object.get(name))
            .and_then(|property| property.get("value"))
            .and_then(|value| value.as_str())
            .map(str::to_string)
            .unwrap_or_default()
    }
}

/// Validate `ComponentReferenceData.json`, if present.
///
/// The stable grammar is an open top-level JSON object. Member names and values
/// are application-defined extension data: the codec validates the envelope,
/// preserves the original ZIP entry byte-for-byte, and performs no semantic
/// projection without a separately identified field contract.
fn validate_component_reference_data(scan: &ContainerScan) -> Result<(), CodecError> {
    if !scan
        .entries
        .iter()
        .any(|entry| entry.name == COMPONENT_REFERENCE_ENTRY)
    {
        return Ok(());
    }
    parse_component_reference_data(scan.entry_bytes(COMPONENT_REFERENCE_ENTRY)?)?;
    Ok(())
}

fn parse_component_reference_data(bytes: &[u8]) -> Result<serde_json::Value, CodecError> {
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|error| {
        CodecError::Malformed(format!(
            "{COMPONENT_REFERENCE_ENTRY} is not valid JSON: {error}"
        ))
    })?;
    if !value.is_object() {
        return Err(CodecError::Malformed(format!(
            "{COMPONENT_REFERENCE_ENTRY} must contain a top-level JSON object"
        )));
    }
    Ok(value)
}

/// Parse the top-level `RedirectionsStream.dat` table, if present.
pub fn decode(scan: &ContainerScan) -> Result<Option<XrefTable>, CodecError> {
    // Validate the extension document independently of whether a redirections
    // table is present. Its members are application-defined and retained by
    // source fidelity, so no field-level semantics are guessed here.
    validate_component_reference_data(scan)?;
    let Ok(bytes) = scan.entry_bytes(REDIRECTIONS_ENTRY) else {
        return Ok(None);
    };
    let mut table = parse(bytes)?;
    bind_occurrences(scan, &mut table)?;
    Ok(Some(table))
}

/// Parse `RedirectionsStream.dat` bytes into an [`XrefTable`].
pub fn parse(bytes: &[u8]) -> Result<XrefTable, CodecError> {
    let parsed: RedirectionsJson = serde_json::from_slice(bytes).map_err(|error| {
        CodecError::Malformed(format!("{REDIRECTIONS_ENTRY} is not valid JSON: {error}"))
    })?;
    let designs = parsed
        .designs
        .into_iter()
        .enumerate()
        .map(|(ordinal, design)| XrefDesign {
            id: format!("f3d:xref:design#{ordinal}"),
            ordinal: ordinal as u32,
            file_version: design.file_version,
            target_file_name: design.target_file_name,
            display_name: design.display_name,
            lineage_urn: design.lineage_urn,
            version_urn: design.version_urn,
        })
        .collect();
    let references = match parsed.references {
        ReferencesJson::List(references) => references,
        ReferencesJson::Other(_) => Vec::new(),
    };
    let references = references
        .into_iter()
        .filter(|reference| reference.reference_type == "XREF")
        .enumerate()
        .map(|(ordinal, reference)| XrefReference {
            id: format!("f3d:xref:reference#{ordinal}"),
            ordinal: ordinal as u32,
            occurrence_ordinal: 0,
            neutron_role: reference.property("neutronRole"),
            neutron_data: reference.property("neutronData"),
            from: reference.from,
            relative_path: reference.relative_path,
            transform: None,
        })
        .collect();
    Ok(XrefTable {
        designs,
        references,
    })
}

/// Parse the `docstruct` declaration of a non-empty `Properties.dat`, if
/// present. The entry is a `u32` payload byte count followed by that many
/// JSON bytes; count 0 is the empty slot and carries no declaration.
pub fn docstruct(scan: &ContainerScan) -> Option<Docstruct> {
    let bytes = scan.entry_bytes(PROPERTIES_ENTRY).ok()?;
    let mut view = View::over_retained(bytes);
    let count = view.u32_le()? as usize;
    let payload = view.take(count)?;
    let value: serde_json::Value = serde_json::from_slice(payload).ok()?;
    let docstruct = value.get("docstruct")?;
    Some(Docstruct {
        doc_type: docstruct.get("type")?.as_str()?.to_string(),
        subtype: docstruct
            .get("subtype")
            .and_then(|subtype| subtype.as_str())
            .unwrap_or_default()
            .to_string(),
    })
}

/// A valid assembly document: declared `assembly-design`, at least one
/// outgoing XREF, and no B-rep streams. Its model is the placement of its
/// XREF targets.
pub fn is_assembly(scan: &ContainerScan, table: Option<&XrefTable>) -> bool {
    crate::container::design_breps(scan).next().is_none()
        && table.is_some_and(|table| !table.references.is_empty())
        && docstruct(scan).is_some_and(|docstruct| docstruct.doc_type == "assembly-design")
}

/// The lineage/version design entry for one reference: the entry whose
/// `target_file_name` equals the reference's `relative_path`.
pub fn design_for<'a>(table: &'a XrefTable, reference: &XrefReference) -> Option<&'a XrefDesign> {
    table
        .designs
        .iter()
        .find(|design| design.target_file_name == reference.relative_path)
}

/// Project each external-reference placement as one root product occurrence.
pub fn project_occurrences(table: &XrefTable) -> Vec<Occurrence> {
    table
        .references
        .iter()
        .enumerate()
        .map(|(ordinal, reference)| {
            let mut transform = reference.transform.unwrap_or([
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ]);
            for row in transform.iter_mut().take(3) {
                row[3] *= 10.0;
            }
            Occurrence {
                id: crate::ids::neutral_xref_occurrence_id(
                    reference.ordinal,
                    reference.occurrence_ordinal,
                ),
                prototype: PrototypeReference::External {
                    document: ExternalDocumentReference {
                        path: Some(reference.relative_path.clone()),
                        document_id: None,
                        resolution: ExternalResolution::Unresolved,
                    },
                    object: None,
                },
                parent: OccurrenceParent::Root,
                ordinal: u32::try_from(ordinal).unwrap_or(u32::MAX),
                transform: cadmpeg_ir::transform::Transform { rows: transform },
                prototype_transform: cadmpeg_ir::transform::Transform::identity(),
                scale: [1.0; 3],
                name: None,
                linked_subelements: Vec::new(),
                visible: None,
                element_component: None,
                claim_child: None,
                copy_on_change: None,
                copy_on_change_source: None,
                copy_on_change_group: None,
                copy_on_change_touched: None,
                link_transform: None,
                native_ref: Some(reference.id.clone()),
            }
        })
        .collect()
}

/// Resolve exact `Component Insert` history scopes to their placed occurrences.
pub fn bind_component_insert_features(
    features: &mut [Feature],
    scopes: &[DesignParameterScope],
    table: &XrefTable,
) {
    for scope in scopes {
        let Some(construction) = &scope.component_insert_construction else {
            continue;
        };
        let mut matches = table.references.iter().filter(|reference| {
            reference.neutron_role == construction.neutron_role
                && reference.transform == Some(construction.transform)
        });
        let Some(reference) = matches.next() else {
            continue;
        };
        if matches.next().is_some() {
            continue;
        }
        let Some(feature) = features
            .iter_mut()
            .find(|feature| feature.native_ref.as_deref() == Some(scope.id.as_str()))
        else {
            continue;
        };
        if matches!(&feature.definition, FeatureDefinition::Native { .. }) {
            feature.definition = FeatureDefinition::InsertComponent {
                occurrence: crate::ids::neutral_xref_occurrence_id(
                    reference.ordinal,
                    reference.occurrence_ordinal,
                ),
            };
        }
    }
}

/// Expand container references through their occurrence records in the active
/// Design `BulkStream` and retain each occurrence-local placement matrix.
fn bind_occurrences(scan: &ContainerScan, table: &mut XrefTable) -> Result<(), CodecError> {
    let mut streams = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, role::BULKSTREAM))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let meta_name = entry
            .name
            .strip_suffix("BulkStream.dat")
            .map(|prefix| format!("{prefix}MetaStream.dat"));
        let serializer_magic = meta_name
            .filter(|name| scan.entries.iter().any(|candidate| candidate.name == *name))
            .map(|name| {
                let bytes = scan.entry_bytes(&name)?;
                crate::metastream::serializer_magic(bytes, &name)
            })
            .transpose()?;
        let headers = indexed_records(bytes);
        let placements = occurrence_placements(bytes, &headers, serializer_magic);
        streams.push((bytes, headers, placements));
    }
    let mut expanded = Vec::new();
    for reference in &table.references {
        let mut occurrences = Vec::new();
        for (bytes, headers, placements) in &mut streams {
            let direct = role_adjacent_transforms(bytes, headers, &reference.neutron_role);
            if direct.is_empty() {
                occurrences.extend(occurrence_transforms(placements, &reference.neutron_role));
            } else {
                occurrences.extend(direct.into_iter().map(Some));
            }
        }
        if occurrences.is_empty() {
            expanded.push(reference.clone());
            continue;
        }
        for (occurrence_ordinal, transform) in occurrences.into_iter().enumerate() {
            let mut occurrence = reference.clone();
            occurrence.id = format!(
                "f3d:xref:reference#{}-occurrence-{occurrence_ordinal}",
                reference.ordinal
            );
            occurrence.occurrence_ordinal = occurrence_ordinal as u32;
            occurrence.transform = transform;
            expanded.push(occurrence);
        }
    }
    table.references = expanded;
    Ok(())
}

fn role_adjacent_transforms(
    bytes: &[u8],
    records: &[IndexedRecord],
    role: &str,
) -> Vec<[[f64; 4]; 4]> {
    records
        .iter()
        .filter(|record| record.class_tag == 256)
        .flat_map(|record| {
            role_tails(bytes, record, role)
                .into_iter()
                .filter_map(|after_role| {
                    let flags_end = after_role.checked_add(2)?;
                    if bytes.get(after_role..flags_end) != Some([0, 0].as_slice()) {
                        return None;
                    }
                    let matrix_at = flags_end;
                    if matrix_at.checked_add(128)? <= record.end {
                        decode_rigid_matrix(bytes, matrix_at)
                    } else {
                        None
                    }
                })
        })
        .collect()
}

#[derive(Debug)]
struct IndexedRecord {
    offset: usize,
    end: usize,
    class_tag: u32,
}

/// The transforms of every placement whose target path carries `role`, in
/// record order. One occurrence-placement record is one occurrence.
fn occurrence_transforms(
    placements: &[OccurrencePlacement],
    role: &str,
) -> Vec<Option<[[f64; 4]; 4]>> {
    placements
        .iter()
        .filter(|placement| placement.link_names.iter().any(|name| name == role))
        .map(|placement| placement.transform)
        .collect()
}

fn indexed_records(bytes: &[u8]) -> Vec<IndexedRecord> {
    let mut headers = Vec::new();
    for at in 0..bytes.len().saturating_sub(11) {
        let Some((class_tag, after_tag)) = lp_ascii_strict(bytes, at, 0..=usize::MAX) else {
            continue;
        };
        let Some(class_tag_number) = class_tag.parse::<u32>().ok() else {
            continue;
        };
        if after_tag == at + 7
            && class_tag.len() == 3
            && class_tag.bytes().all(|byte| byte.is_ascii_digit())
        {
            if bytes.get(after_tag..after_tag + 8).is_none() {
                continue;
            }
            headers.push((at, class_tag_number));
        }
    }
    headers
        .iter()
        .enumerate()
        .map(|(ordinal, (offset, class_tag))| IndexedRecord {
            offset: *offset,
            end: headers
                .get(ordinal + 1)
                .map_or(bytes.len(), |(offset, _)| *offset),
            class_tag: *class_tag,
        })
        .collect()
}

fn role_tails(bytes: &[u8], record: &IndexedRecord, value: &str) -> Vec<usize> {
    let encoded = value.encode_utf16().collect::<Vec<_>>();
    let mut needle = Vec::with_capacity(4 + encoded.len() * 2);
    needle.extend_from_slice(&(encoded.len() as u32).to_le_bytes());
    needle.extend(encoded.into_iter().flat_map(u16::to_le_bytes));
    let mut tails = Vec::new();
    let mut from = record.offset;
    while let Some(at) = find_in(bytes, &needle, from, record.end) {
        tails.push(at + needle.len());
        from = at + 1;
    }
    tails
}

/// One occurrence-placement record: the target path it names and the transform
/// it places that path at
/// ([spec §1.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#14-external-references)
/// "**Placement.**").
#[derive(Debug, Clone, PartialEq)]
struct OccurrencePlacement {
    /// Cross-document link names carried by the path elements, in path order.
    /// The role-bearing element is not necessarily the first.
    link_names: Vec<String>,
    /// Instance discriminator of each path element, in path order.
    discriminators: Vec<u32>,
    /// `None` is the stored identity form, which carries no matrix.
    transform: Option<[[f64; 4]; 4]>,
}

/// Parse every indexed record that closes exactly under the occurrence-placement
/// grammar, in record order.
fn occurrence_placements(
    bytes: &[u8],
    records: &[IndexedRecord],
    serializer_magic: Option<u32>,
) -> Vec<OccurrencePlacement> {
    records
        .iter()
        .filter_map(|record| {
            occurrence_placement(bytes.get(record.offset..record.end)?, serializer_magic)
        })
        .collect()
}

/// Parse one record body, header included, requiring the member sequence to end
/// exactly at the record end.
fn occurrence_placement(body: &[u8], serializer_magic: Option<u32>) -> Option<OccurrencePlacement> {
    // Header: the LP-ASCII decimal class tag, the u64 entity ID, and the
    // LP-ASCII record name.
    let (_class_tag, after_tag) = lp_ascii_strict(body, 0, 3..=3)?;
    let mut at = after_tag.checked_add(8)?;
    let (_name, after_name) = lp_ascii_strict(body, at, 0..=256)?;
    at = after_name;
    if body.get(at) != Some(&1) {
        return None;
    }
    at += 1;
    let count = usize::try_from(View::u32_le_at(body, at)?).ok()?;
    if count == 0 || count > 4096 {
        return None;
    }
    at += 4;
    let mut link_names = Vec::new();
    let mut discriminators = Vec::with_capacity(count);
    for _ in 0..count {
        let element = take_reference(body, &mut at)?;
        if let Some(link_name) = element.link_name {
            link_names.push(link_name);
        }
        discriminators.push(View::u32_le_at(body, at)?);
        at += 4;
    }
    if body.get(at) != Some(&0) {
        return None;
    }
    at += 1;
    // The identity marker is absent in the oldest container generation, which
    // always stores the matrix. Both readings start with a zero byte when the
    // marker is present and the matrix follows, so the record end decides.
    for identity_marker in [true, false] {
        let mut cursor = at;
        let mut transform = None;
        if identity_marker {
            match body.get(cursor) {
                Some(1) => cursor += 1,
                Some(0) => {
                    let Some(matrix) = decode_rigid_matrix(body, cursor + 1) else {
                        continue;
                    };
                    transform = Some(matrix);
                    cursor += 129;
                }
                _ => continue,
            }
        } else {
            let Some(matrix) = decode_rigid_matrix(body, cursor) else {
                continue;
            };
            transform = Some(matrix);
            cursor += 128;
        }
        if placement_tail(body, cursor, serializer_magic).is_some() {
            return Some(OccurrencePlacement {
                link_names,
                discriminators,
                transform,
            });
        }
    }
    None
}

/// Consume the three reference runs that close a placement, returning `Some`
/// only when they end exactly at the record end.
fn placement_tail(body: &[u8], mut at: usize, serializer_magic: Option<u32>) -> Option<()> {
    let count = usize::try_from(View::u32_le_at(body, at)?).ok()?;
    if count > 256 {
        return None;
    }
    at += 4;
    for _ in 0..count {
        take_reference(body, &mut at)?;
    }
    // Only the modern MetaStream serializer magic admits the tagged run. Its
    // tag byte is neither reference presence value.
    if serializer_magic == Some(crate::metastream::MODERN_SERIALIZER_MAGIC) {
        if matches!(body.get(at), Some(0 | 1)) {
            return None;
        }
        at += 1;
        let tagged = usize::try_from(View::u32_le_at(body, at)?).ok()?;
        if tagged > 256 {
            return None;
        }
        at = at.checked_add(4)?.checked_add(tagged.checked_mul(4)?)?;
        take_reference(body, &mut at)?;
    }
    take_reference(body, &mut at)?;
    take_reference(body, &mut at)?;
    (at == body.len()).then_some(())
}

fn decode_rigid_matrix(bytes: &[u8], at: usize) -> Option<[[f64; 4]; 4]> {
    let mut view = View::over_retained(bytes);
    view.seek(at)?;
    let mut rows = [[0.0; 4]; 4];
    for row in &mut rows {
        for value in row {
            *value = view.f64_le()?;
        }
    }
    if !rows.iter().flatten().all(|value| value.is_finite()) || rows[3] != [0.0, 0.0, 0.0, 1.0] {
        return None;
    }
    let tolerance = 1.0e-8;
    for left in 0..3 {
        for right in 0..3 {
            let dot = (0..3)
                .map(|row| rows[row][left] * rows[row][right])
                .sum::<f64>();
            if (dot - f64::from(left == right)).abs() > tolerance {
                return None;
            }
        }
    }
    Some(rows)
}

#[cfg(test)]
mod tests;
