// SPDX-License-Identifier: Apache-2.0
//! External-reference (`XRef`) and document-type entries of a `.f3d` container
//! ([spec §1.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#12-stored-property-and-configuration-entries),
//! [§1.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#14-external-references)).
//!
//! [`decode`] parses the top-level `RedirectionsStream.dat` table into
//! [`XrefDesign`] and [`XrefReference`] records. [`docstruct`] parses the JSON
//! form of `Properties.dat`. [`is_assembly`] classifies a BREP-less document
//! whose model is the placement of its XREF targets.

use std::collections::HashSet;

use serde::Deserialize;

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{Feature, FeatureDefinition};
use cadmpeg_ir::products::{
    ExternalDocumentReference, ExternalResolution, Occurrence, OccurrenceParent, PrototypeReference,
};

use crate::bytes::{
    is_guid_prefix, is_guid_relaxed, lp_ascii_filtered, lp_ascii_strict, lp_utf16_bounded,
    take_reference,
};
use crate::container::role;
use crate::container::ContainerScan;
use crate::layout::component_insert_grouped_identity_carrier as grouped_identity_layout;
use crate::records::{
    DesignComponentInsertConstruction, DesignParameterScope, XrefDesign, XrefReference,
};

const EPS_XREF_DECODE_RIGID_MATRIX_E8: f64 = 1.0e-8;

/// Top-level container entry holding the external-reference table.
pub const REDIRECTIONS_ENTRY: &str = "RedirectionsStream.dat";
/// Top-level container entry holding the document-properties slot.
pub const PROPERTIES_ENTRY: &str = "Properties.dat";
/// Top-level JSON document carrying component-reference extension data.
pub const COMPONENT_REFERENCE_ENTRY: &str = "ComponentReferenceData.json";

/// Stable type-table identity of a Design occurrence-placement record.
const OCCURRENCE_PLACEMENT_TYPE_GUID: &str = "CE2913AA-CFE0-4F04-9102-24424ED3BCFA";

/// The parsed `RedirectionsStream.dat` table.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct XrefTable {
    /// Design entries in source order; entry 0 is the document itself.
    pub designs: Vec<XrefDesign>,
    /// Outgoing XREF placements in source order; empty for a leaf document.
    pub references: Vec<XrefReference>,
    /// Source reference ordinals whose role-named placement records were
    /// admitted by the type table but did not close under the generation's
    /// placement grammar and had no other valid placement carrier.
    pub(crate) placement_failures: Vec<u32>,
    /// `(XREF ordinal, placement-record count)` pairs whose structured
    /// placements were superseded by scope-bound Component Insert carriers.
    pub(crate) placement_overrides: Vec<(u32, usize)>,
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
        CodecError::malformed(format_args!(
            "{COMPONENT_REFERENCE_ENTRY} is not valid JSON: {error}"
        ))
    })?;
    if !value.is_object() {
        return Err(CodecError::malformed(format_args!(
            "{COMPONENT_REFERENCE_ENTRY} must contain a top-level JSON object"
        )));
    }
    Ok(value)
}

/// Parse the top-level `RedirectionsStream.dat` table, if present.
pub fn decode(scan: &ContainerScan) -> Result<Option<XrefTable>, CodecError> {
    decode_with_scopes(scan, &[])
}

/// Parse the external-reference table and bind its occurrences to exact
/// `Component Insert` constructions already decoded from the Design streams.
pub(crate) fn decode_with_scopes(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
) -> Result<Option<XrefTable>, CodecError> {
    // Validate the extension document independently of whether a redirections
    // table is present. Its members are application-defined and retained by
    // source fidelity, so no field-level semantics are guessed here.
    validate_component_reference_data(scan)?;
    let Ok(bytes) = scan.entry_bytes(REDIRECTIONS_ENTRY) else {
        return Ok(None);
    };
    let mut table = parse(bytes)?;
    bind_occurrences(scan, &mut table, scopes)?;
    Ok(Some(table))
}

/// Parse `RedirectionsStream.dat` bytes into an [`XrefTable`].
pub fn parse(bytes: &[u8]) -> Result<XrefTable, CodecError> {
    let parsed: RedirectionsJson = serde_json::from_slice(bytes).map_err(|error| {
        CodecError::malformed(format_args!(
            "{REDIRECTIONS_ENTRY} is not valid JSON: {error}"
        ))
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
        placement_failures: Vec::new(),
        placement_overrides: Vec::new(),
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
                    document: ExternalDocumentReference::path(
                        reference.relative_path.clone(),
                        ExternalResolution::Unresolved,
                    ),
                    object: None,
                },
                parent: OccurrenceParent::Root,
                ordinal: u32::try_from(ordinal).unwrap_or(u32::MAX),
                transform: cadmpeg_ir::transform::Transform { rows: transform },
                linked_prototype: None,
                scale: [1.0; 3],
                name: None,
                visible: None,
                link: None,
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
fn bind_occurrences(
    scan: &ContainerScan,
    table: &mut XrefTable,
    scopes: &[DesignParameterScope],
) -> Result<(), CodecError> {
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
        let (serializer_magic, placement_offsets) = if let Some(name) =
            meta_name.filter(|name| scan.entries.iter().any(|candidate| candidate.name == *name))
        {
            let meta = scan.parsed_metastream(&name)?;
            let meta_bytes = scan.entry_bytes(&name)?;
            (
                Some(crate::metastream::serializer_magic(meta_bytes, &name)?),
                Some(typed_occurrence_placement_offsets(&meta)?),
            )
        } else {
            (None, None)
        };
        let headers = indexed_records(bytes);
        let (placements, failures) = occurrence_placements_with_failures(
            bytes,
            &headers,
            serializer_magic,
            placement_offsets.as_ref(),
        );
        streams.push((placements, failures, crate::ids::native_scope(&entry.name)));
    }
    let mut expanded = Vec::new();
    let mut placement_failures = Vec::new();
    let mut placement_overrides = Vec::new();
    for reference in &table.references {
        let mut occurrences = Vec::new();
        for (placements, _, stream) in &streams {
            let direct = select_component_insert_transforms(
                scopes.iter().filter_map(|scope| {
                    let stream = crate::ids::native_stream(&scope.id)?;
                    let construction = scope.component_insert_construction.as_ref()?;
                    Some((stream, construction))
                }),
                stream,
                &reference.neutron_role,
            );
            let structured_count =
                superseded_placement_count(&direct, placements, &reference.neutron_role);
            if !direct.is_empty() && structured_count != 0 {
                placement_overrides.push((reference.ordinal, structured_count));
            }
            occurrences.extend(occurrence_transforms_with_precedence(
                direct,
                placements,
                &reference.neutron_role,
            ));
        }
        if occurrences.is_empty() {
            if streams.iter().any(|(_, failures, stream)| {
                let direct = select_component_insert_transforms(
                    scopes.iter().filter_map(|scope| {
                        let stream = crate::ids::native_stream(&scope.id)?;
                        let construction = scope.component_insert_construction.as_ref()?;
                        Some((stream, construction))
                    }),
                    stream,
                    &reference.neutron_role,
                );
                direct.is_empty()
                    && failures.iter().any(|failure| {
                        failure
                            .link_names
                            .iter()
                            .any(|name| name == &reference.neutron_role)
                    })
            }) {
                placement_failures.push(reference.ordinal);
            }
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
    table.placement_failures = placement_failures;
    table.placement_overrides = placement_overrides;
    Ok(())
}

/// Select the exact `Component Insert` constructions for one Design stream
/// and external-reference role. The construction parser has already joined
/// each role to its scope-owned relation record and verified its carrier
/// transform, so the class tag is not an admission discriminator here.
fn select_component_insert_transforms<'a, I>(
    constructions: I,
    stream: &str,
    role: &str,
) -> Vec<[[f64; 4]; 4]>
where
    I: IntoIterator<Item = (&'a str, &'a DesignComponentInsertConstruction)>,
{
    constructions
        .into_iter()
        .filter(|(construction_stream, construction)| {
            *construction_stream == stream && construction.neutron_role == role
        })
        .map(|(_, construction)| construction.transform)
        .collect()
}

/// Use scope-bound carriers when present. Placement records are the fallback
/// for a stream with no exact carrier for this role.
fn occurrence_transforms_with_precedence(
    direct: Vec<[[f64; 4]; 4]>,
    placements: &[OccurrencePlacement],
    role: &str,
) -> Vec<Option<[[f64; 4]; 4]>> {
    if direct.is_empty() {
        occurrence_transforms(placements, role)
    } else {
        direct.into_iter().map(Some).collect()
    }
}

/// Count structured placements that are discarded when a scope-bound carrier
/// supplies at least one transform for the same role.
fn superseded_placement_count(
    direct: &[[[f64; 4]; 4]],
    placements: &[OccurrencePlacement],
    role: &str,
) -> usize {
    if direct.is_empty() {
        0
    } else {
        occurrence_transforms(placements, role).len()
    }
}

/// Return the `BulkStream` offsets whose type-table identity is the stable
/// occurrence-placement type. Dynamic class tags and record shape are not
/// sufficient because unrelated component records can share that shape.
fn typed_occurrence_placement_offsets(
    meta: &crate::metastream::MetaStream,
) -> Result<HashSet<usize>, CodecError> {
    let placement_entities = meta
        .types
        .iter()
        .filter(|design_type| {
            design_type
                .type_guid
                .eq_ignore_ascii_case(OCCURRENCE_PLACEMENT_TYPE_GUID)
        })
        .flat_map(|design_type| design_type.entity_ids.iter().copied())
        .collect::<HashSet<_>>();
    meta.records
        .iter()
        .chain(meta.secondary_records.iter())
        .filter(|record| placement_entities.contains(&record.entity_id))
        .map(|record| {
            usize::try_from(record.bulk_offset).map_err(|_| {
                CodecError::Malformed(
                    "F3D occurrence-placement BulkStream offset exceeds usize".into(),
                )
            })
        })
        .collect()
}

#[derive(Debug)]
struct IndexedRecord {
    offset: usize,
    end: usize,
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
        if after_tag == at + 7
            && class_tag.len() == 3
            && class_tag.bytes().all(|byte| byte.is_ascii_digit())
        {
            if bytes.get(after_tag..after_tag + 8).is_none() {
                continue;
            }
            headers.push(at);
        }
    }
    headers
        .iter()
        .enumerate()
        .map(|(ordinal, offset)| IndexedRecord {
            offset: *offset,
            end: headers.get(ordinal + 1).copied().unwrap_or(bytes.len()),
        })
        .collect()
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct OccurrencePlacementFailure {
    /// Link names recovered from the valid target-path prefix.
    link_names: Vec<String>,
}

/// Parse every indexed record that closes exactly under the occurrence-placement
/// grammar, in record order.
#[cfg(test)]
fn occurrence_placements(
    bytes: &[u8],
    records: &[IndexedRecord],
    serializer_magic: Option<u32>,
) -> Vec<OccurrencePlacement> {
    occurrence_placements_filtered(bytes, records, serializer_magic, None)
}

/// Parse occurrence-placement records, optionally restricted by the
/// `MetaStream` type-table admission set.
#[cfg(test)]
fn occurrence_placements_filtered(
    bytes: &[u8],
    records: &[IndexedRecord],
    serializer_magic: Option<u32>,
    typed_offsets: Option<&HashSet<usize>>,
) -> Vec<OccurrencePlacement> {
    occurrence_placements_with_failures(bytes, records, serializer_magic, typed_offsets).0
}

/// Parse admitted placement records and retain role names from records whose
/// target path is valid but whose remaining generation-specific payload is not.
fn occurrence_placements_with_failures(
    bytes: &[u8],
    records: &[IndexedRecord],
    serializer_magic: Option<u32>,
    typed_offsets: Option<&HashSet<usize>>,
) -> (Vec<OccurrencePlacement>, Vec<OccurrencePlacementFailure>) {
    let mut placements = Vec::new();
    let mut failures = Vec::new();
    records
        .iter()
        .filter(|record| typed_offsets.is_none_or(|offsets| offsets.contains(&record.offset)))
        .for_each(|record| {
            let Some(body) = bytes.get(record.offset..record.end) else {
                return;
            };
            if let Some(placement) = occurrence_placement(body, serializer_magic) {
                placements.push(placement);
            } else if let Some((link_names, _, _)) = occurrence_path(body) {
                failures.push(OccurrencePlacementFailure { link_names });
            } else if let Some(link_name) = legacy_occurrence_role(body) {
                failures.push(OccurrencePlacementFailure {
                    link_names: vec![link_name],
                });
            }
        });
    (placements, failures)
}

/// Parse one record body, header included, requiring the member sequence to end
/// exactly at the record end.
fn occurrence_placement(body: &[u8], serializer_magic: Option<u32>) -> Option<OccurrencePlacement> {
    legacy_occurrence_placement(body)
        .or_else(|| repeated_target_occurrence_placement(body))
        .or_else(|| modern_occurrence_placement(body, serializer_magic))
        .or_else(|| {
            let record_index = View::u32_le_at(body, 7)?;
            let (link_name, _) =
                grouped_component_insert_identity(body, 0, body.len(), record_index)?;
            Some(OccurrencePlacement {
                link_names: vec![link_name],
                discriminators: vec![1],
                transform: None,
            })
        })
}

/// Parse the placement generation that repeats the target identity after the
/// standard path and stores the identity flag beside that repeated target.
fn repeated_target_occurrence_placement(body: &[u8]) -> Option<OccurrencePlacement> {
    repeated_target_occurrence_placement_details(body).map(|details| details.placement)
}

struct RepeatedTargetPlacementDetails {
    placement: OccurrencePlacement,
    role: String,
    role_offset: usize,
    transform_offset: Option<usize>,
}

fn repeated_target_occurrence_placement_details(
    body: &[u8],
) -> Option<RepeatedTargetPlacementDetails> {
    const METADATA_MARKER: &[u8] = &[0, 1, 3, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0];
    let (link_names, discriminators, mut at) = occurrence_path(body)?;
    if !matches!(View::u32_le_at(body, at)?, 1..=6) {
        return None;
    }
    at += 4;
    for _ in 0..2 {
        let (guid, next) = lp_utf16_bounded(body, at, 36..=36)?;
        if !is_guid_relaxed(&guid) {
            return None;
        }
        at = next;
    }
    if body.get(at..at + METADATA_MARKER.len())? != METADATA_MARKER {
        return None;
    }
    at += METADATA_MARKER.len();

    let (component_guid, next) = lp_utf16_bounded(body, at, 36..=36)?;
    if !is_guid_relaxed(&component_guid) {
        return None;
    }
    at = next;
    if body.get(at) != Some(&0) {
        return None;
    }
    at += 1;
    let (type_guid, next) = lp_ascii_strict(body, at, 36..=36)?;
    if !is_guid_relaxed(&type_guid) {
        return None;
    }
    at = next;
    let role_offset = at;
    let (role, next) = lp_utf16_bounded(body, at, 36..=256)?;
    if !is_guid_prefix(&role) {
        return None;
    }
    at = next;
    if body.get(at) != Some(&0) {
        return None;
    }
    at += 1;
    let mut transform_offset = None;
    let transform = match *body.get(at)? {
        1 => {
            at += 1;
            None
        }
        0 => {
            at += 1;
            transform_offset = Some(at);
            let matrix = decode_rigid_matrix(body, at)?;
            at = at.checked_add(128)?;
            Some(matrix)
        }
        _ => return None,
    };
    if View::u32_le_at(body, at)? != 0 {
        return None;
    }
    at += 4;
    let (final_role, next) = lp_utf16_bounded(body, at, 36..=256)?;
    if !final_role.eq_ignore_ascii_case(&role) {
        return None;
    }
    at = next;
    if body.get(at) != Some(&0) {
        return None;
    }
    at += 1;
    take_reference(body, &mut at)?;
    (at == body.len()).then_some(RepeatedTargetPlacementDetails {
        placement: OccurrencePlacement {
            link_names,
            discriminators,
            transform,
        },
        role,
        role_offset: role_offset + 4,
        transform_offset,
    })
}

/// Bind a repeated-target occurrence carrier to a Component Insert scope
/// through its relation record.
pub(crate) fn repeated_target_component_insert(
    bytes: &[u8],
    carrier_at: usize,
    relation_at: usize,
    carrier_record_index: u32,
    expected_transform: [[f64; 4]; 4],
) -> Option<(String, usize, Option<usize>)> {
    let body = bytes.get(carrier_at..relation_at)?;
    if View::u64_le_at(body, 7)? != u64::from(carrier_record_index) {
        return None;
    }
    let details = repeated_target_occurrence_placement_details(body)?;
    let transform = details.placement.transform.unwrap_or([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]);
    if transform != expected_transform {
        return None;
    }
    Some((
        details.role,
        carrier_at + details.role_offset,
        details.transform_offset.map(|offset| carrier_at + offset),
    ))
}

/// Parse the grouped identity carrier used by the compact `Component Insert`
/// generation. The carrier has no matrix; its placement is the stored
/// identity transform. The repeated GUID and role fields are part of the
/// carrier grammar, not an occurrence-count signal.
pub(crate) fn grouped_component_insert_identity(
    bytes: &[u8],
    carrier_at: usize,
    relation_at: usize,
    carrier_record_index: u32,
) -> Option<(String, usize)> {
    grouped_component_insert_identity_with_layout(
        bytes,
        carrier_at,
        relation_at,
        carrier_record_index,
        "382",
    )
}

/// Parse the class-380 grouped identity carrier used by the class-410/class-261
/// `Component Insert` generation.
pub(crate) fn grouped_component_insert_identity_class380(
    bytes: &[u8],
    carrier_at: usize,
    relation_at: usize,
    carrier_record_index: u32,
) -> Option<(String, usize)> {
    grouped_component_insert_identity_with_layout(
        bytes,
        carrier_at,
        relation_at,
        carrier_record_index,
        "380",
    )
}

/// Parse the class-369 grouped identity carrier used by the class-426/class-258
/// `Component Insert` generation.
pub(crate) fn grouped_component_insert_identity_class369(
    bytes: &[u8],
    carrier_at: usize,
    relation_at: usize,
    carrier_record_index: u32,
) -> Option<(String, usize)> {
    grouped_component_insert_identity_with_layout(
        bytes,
        carrier_at,
        relation_at,
        carrier_record_index,
        "369",
    )
}

/// Parse the variable-role grouped identity carrier used by the class-434/
/// class-266 `Component Insert` generation.
pub(crate) fn grouped_component_insert_identity_class341(
    bytes: &[u8],
    carrier_at: usize,
    relation_at: usize,
    carrier_record_index: u32,
) -> Option<(String, usize)> {
    grouped_component_insert_identity_with_layout(
        bytes,
        carrier_at,
        relation_at,
        carrier_record_index,
        "341",
    )
}

fn grouped_component_insert_identity_with_layout(
    bytes: &[u8],
    carrier_at: usize,
    relation_at: usize,
    carrier_record_index: u32,
    expected_class_tag: &str,
) -> Option<(String, usize)> {
    const MARKER_AFTER_ROLE: &[u8] = &[0, 1, 0, 0, 0, 0, 1, 0, 0, 0];
    const CLASS_369_GUID_ROLE_MARKER: &[u8] = &[0, 3, 0, 0, 0, 0, 1, 0, 0, 0];
    const CLASS_369_EXTERNAL_ROLE_MARKER: &[u8] = &[0, 4, 0, 0, 0, 0, 1, 0, 0, 0];
    const MARKER_AFTER_METADATA: &[u8] = &[0, 1, 3, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0];
    const MARKER_AFTER_PLACEMENT: &[u8] = &[0, 1, 0, 0, 0, 0];
    const CLASS_341_REPEAT_MARKER: &[u8] = &[1, 0, 0, 0, 0];
    const CLOSURE: &[u8] = &[0, 1, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0];

    let (class_tag, after_tag) = lp_ascii_filtered(bytes, carrier_at, 3..=3, u8::is_ascii_digit)?;
    let carrier_span = relation_at.checked_sub(carrier_at)?;
    if class_tag != expected_class_tag
        || after_tag != carrier_at + 7
        || View::u32_le_at(bytes, after_tag) != Some(carrier_record_index)
        || (expected_class_tag != "369"
            && expected_class_tag != "341"
            && carrier_span != grouped_identity_layout::LEN)
        || ((expected_class_tag == "369" || expected_class_tag == "341")
            && carrier_span < grouped_identity_layout::LEN)
        || bytes.get(carrier_at + 11..carrier_at + 19)? != [0; 8]
        || bytes.get(carrier_at + 19) != Some(&1)
        || bytes.get(carrier_at + 20..carrier_at + 24)? != [1, 0, 0, 0]
        || bytes.get(carrier_at + 24) != Some(&1)
        || bytes.get(carrier_at + 33) != Some(&1)
        || bytes.get(carrier_at + 34..carrier_at + 38)? != [0; 4]
    {
        return None;
    }
    let occurrence_identity = View::u64_le_at(
        bytes,
        carrier_at + grouped_identity_layout::OCCURRENCE_IDENTITY,
    )?;
    let (component_guid, mut at) = lp_utf16_bounded(
        bytes,
        carrier_at + grouped_identity_layout::FIRST_COMPONENT_GUID,
        36..=36,
    )?;
    if !is_guid_relaxed(&component_guid) {
        return None;
    }
    if bytes.get(at) != Some(&0) {
        return None;
    }
    at += 1;
    let (type_guid, next) = lp_ascii_strict(bytes, at, 36..=36)?;
    if !is_guid_relaxed(&type_guid) {
        return None;
    }
    at = next;
    let first_role_at = at;
    let variable_role = matches!(expected_class_tag, "341" | "369");
    let role_bounds = if variable_role { 36..=256 } else { 36..=36 };
    let (role, next) = lp_utf16_bounded(bytes, at, role_bounds.clone())?;
    let valid_role = if variable_role {
        is_guid_relaxed(&role)
            || (is_guid_prefix(&role)
                && role
                    .get(36..)
                    .is_some_and(|suffix| suffix.starts_with("_urn:")))
    } else {
        is_guid_relaxed(&role)
    };
    if !valid_role {
        return None;
    }
    at = next;
    let marker_after_role = if expected_class_tag == "369" {
        if role.len() == 36 {
            CLASS_369_GUID_ROLE_MARKER
        } else {
            CLASS_369_EXTERNAL_ROLE_MARKER
        }
    } else {
        MARKER_AFTER_ROLE
    };
    if bytes.get(at..at + marker_after_role.len())? != marker_after_role {
        return None;
    }
    at += marker_after_role.len();

    let (metadata_guid_a, next) = lp_utf16_bounded(bytes, at, 36..=36)?;
    if !is_guid_relaxed(&metadata_guid_a) {
        return None;
    }
    at = next;
    let (metadata_guid_b, next) = lp_utf16_bounded(bytes, at, 36..=36)?;
    if !is_guid_relaxed(&metadata_guid_b) {
        return None;
    }
    at = next;
    if expected_class_tag == "341" {
        if bytes.get(at..at + 2)? != [0, 1]
            || View::u64_le_at(bytes, at + 2)? != occurrence_identity
        {
            return None;
        }
        at += 10;
        if bytes.get(at..at + CLASS_341_REPEAT_MARKER.len())? != CLASS_341_REPEAT_MARKER {
            return None;
        }
        at += CLASS_341_REPEAT_MARKER.len();
    } else {
        if bytes.get(at..at + MARKER_AFTER_METADATA.len())? != MARKER_AFTER_METADATA {
            return None;
        }
        at += MARKER_AFTER_METADATA.len();
    }

    let (repeated_component_guid, next) = lp_utf16_bounded(bytes, at, 36..=36)?;
    if !is_guid_relaxed(&repeated_component_guid)
        || !repeated_component_guid.eq_ignore_ascii_case(&component_guid)
    {
        return None;
    }
    at = next;
    if bytes.get(at) != Some(&0) {
        return None;
    }
    at += 1;
    let (repeated_type_guid, next) = lp_ascii_strict(bytes, at, 36..=36)?;
    if !is_guid_relaxed(&repeated_type_guid) || !repeated_type_guid.eq_ignore_ascii_case(&type_guid)
    {
        return None;
    }
    at = next;
    let (repeated_role, next) = lp_utf16_bounded(bytes, at, role_bounds.clone())?;
    if !repeated_role.eq_ignore_ascii_case(&role) {
        return None;
    }
    at = next;
    if bytes.get(at..at + MARKER_AFTER_PLACEMENT.len())? != MARKER_AFTER_PLACEMENT {
        return None;
    }
    at += MARKER_AFTER_PLACEMENT.len();

    let (final_role, next) = lp_utf16_bounded(bytes, at, role_bounds)?;
    if !final_role.eq_ignore_ascii_case(&role) {
        return None;
    }
    at = next;
    if bytes.get(at..at + CLOSURE.len())? != CLOSURE || at + CLOSURE.len() != relation_at {
        return None;
    }
    Some((role, first_role_at + 4))
}

/// Parse the current placement envelope: a standard target path, an optional
/// rigid matrix, and the generation-selected reference runs.
fn modern_occurrence_placement(
    body: &[u8],
    serializer_magic: Option<u32>,
) -> Option<OccurrencePlacement> {
    let (link_names, discriminators, at) = occurrence_path(body)?;
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

/// Parse the legacy typed placement envelope.
///
/// This form keeps the same stable type-table identity as the current
/// placement, but its target-reference carrier is wider and its matrix is
/// after the repeated target envelope. The dynamic class tag is deliberately
/// not an admission key: the type-table identity and exact member framing are
/// the stable discriminators.
fn legacy_occurrence_placement(body: &[u8]) -> Option<OccurrencePlacement> {
    let (discriminator, mut at) = legacy_occurrence_prefix(body)?;
    let identity_marker = *body.get(at)?;
    at += 1;
    let transform = match identity_marker {
        1 => None,
        0 => {
            let matrix = decode_rigid_matrix(body, at)?;
            at = at.checked_add(128)?;
            Some(matrix)
        }
        _ => return None,
    };
    if View::u32_le_at(body, at)? != 0 {
        return None;
    }
    at += 4;
    let (link_name, after_role) = lp_utf16_bounded(body, at, 36..=36)?;
    if !is_guid_relaxed(&link_name) {
        return None;
    }
    at = after_role;
    if body.get(at..at + 12)? != [0, 1, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0] {
        return None;
    }
    at += 12;
    (at == body.len()).then_some(OccurrencePlacement {
        link_names: vec![link_name],
        discriminators: vec![discriminator],
        transform,
    })
}

/// Return the role from a structurally valid legacy placement prefix.
///
/// The role is recovered even when the transform or closing tail is damaged,
/// so the caller can report an undecoded typed placement against the correct
/// external reference instead of treating it as an unrelated record.
fn legacy_occurrence_role(body: &[u8]) -> Option<String> {
    let (_, mut at) = legacy_occurrence_prefix(body)?;
    match *body.get(at)? {
        1 => at += 1,
        0 => at = at.checked_add(129)?,
        _ => return None,
    }
    if View::u32_le_at(body, at)? != 0 {
        return None;
    }
    at += 4;
    let (link_name, _) = lp_utf16_bounded(body, at, 36..=36)?;
    is_guid_relaxed(&link_name).then_some(link_name)
}

/// Parse the shared prefix of the legacy identity and matrix forms.
fn legacy_occurrence_prefix(body: &[u8]) -> Option<(u32, usize)> {
    let (_class_tag, after_tag) = lp_ascii_strict(body, 0, 3..=3)?;
    let mut at = after_tag.checked_add(8)?;
    let (_name, after_name) = lp_ascii_strict(body, at, 0..=256)?;
    at = after_name;
    if body.get(at) != Some(&1) {
        return None;
    }
    at += 1;
    if View::u32_le_at(body, at)? != 1 {
        return None;
    }
    at += 4;
    take_legacy_occurrence_reference(body, &mut at)?;
    let discriminator = View::u32_le_at(body, at)?;
    at += 4;
    if View::u32_le_at(body, at)? != 1 {
        return None;
    }
    at += 4;
    if body.get(at) != Some(&0) {
        return None;
    }
    at += 1;
    if View::u32_le_at(body, at)? != 1 {
        return None;
    }
    at += 4;
    for _ in 0..2 {
        let (guid, next) = lp_utf16_bounded(body, at, 36..=36)?;
        if !is_guid_relaxed(&guid) {
            return None;
        }
        at = next;
    }
    if body.get(at) != Some(&0) {
        return None;
    }
    at += 1;
    take_legacy_occurrence_reference(body, &mut at)?;
    Some((discriminator, at))
}

/// Consume one legacy occurrence target reference.
fn take_legacy_occurrence_reference(body: &[u8], at: &mut usize) -> Option<()> {
    if body.get(*at) != Some(&1) {
        return None;
    }
    *at += 1;
    *at = at.checked_add(8)?;
    if body.get(*at) != Some(&1) {
        return None;
    }
    *at += 1;
    *at = at.checked_add(8)?;
    if body.get(*at) != Some(&0) {
        return None;
    }
    *at += 1;
    let (type_guid, next) = lp_ascii_strict(body, *at, 36..=36)?;
    if !is_guid_relaxed(&type_guid) {
        return None;
    }
    *at = next;
    if body.get(*at) != Some(&0) {
        return None;
    }
    *at += 1;
    Some(())
}

/// Parse the target-path prefix shared by every occurrence-placement form.
fn occurrence_path(body: &[u8]) -> Option<(Vec<String>, Vec<u32>, usize)> {
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
    Some((link_names, discriminators, at))
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
    let tolerance = EPS_XREF_DECODE_RIGID_MATRIX_E8;
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
