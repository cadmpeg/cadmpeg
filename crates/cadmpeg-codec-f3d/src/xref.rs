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

use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::features::{Feature, FeatureDefinition};
use cadmpeg_ir::products::{
    ComponentReference, ExternalDocumentReference, ExternalResolution, Occurrence,
};

use crate::bytes::lp_ascii_strict;
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
    bind_occurrences(scan, &mut table);
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
    let (count, payload) = bytes.split_first_chunk::<4>()?;
    let count = u32::from_le_bytes(*count) as usize;
    let payload = payload.get(..count)?;
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
    scan.breps.is_empty()
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
        .map(|reference| {
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
                prototype: ComponentReference::External {
                    document: ExternalDocumentReference {
                        path: Some(reference.relative_path.clone()),
                        document_id: None,
                        resolution: ExternalResolution::Unresolved,
                    },
                    object: None,
                },
                parent: None,
                array_index: None,
                local_transform: transform,
                prototype_transform: [
                    [1.0, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0],
                ],
                resolved_transform: transform,
                scale: [1.0; 3],
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
        if matches!(
            &feature.definition,
            FeatureDefinition::Native { kind, .. } if kind == "Component Insert"
        ) {
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
fn bind_occurrences(scan: &ContainerScan, table: &mut XrefTable) {
    let roles = table
        .references
        .iter()
        .map(|reference| reference.neutron_role.as_str())
        .collect::<Vec<_>>();
    let streams = scan.entries.iter().filter(|entry| {
        entry.role == role::BULKSTREAM
            && entry.name.contains("Design")
            && scan
                .asset_folder
                .as_ref()
                .is_none_or(|folder| entry.name.starts_with(&format!("{folder}/")))
    });
    let mut streams = streams
        .filter_map(|entry| scan.entry_bytes(&entry.name).ok())
        .map(|bytes| {
            let headers = indexed_records(bytes);
            let matrices = std::collections::HashMap::new();
            let class_tag = xref_class_tag(bytes, &headers, &roles);
            (bytes, headers, class_tag, matrices)
        })
        .collect::<Vec<_>>();
    let mut expanded = Vec::new();
    for reference in &table.references {
        let mut occurrences = Vec::new();
        for (bytes, headers, class_tag, matrices) in &mut streams {
            let direct = role_adjacent_transforms(bytes, headers, &reference.neutron_role);
            if direct.is_empty() {
                if let Some(class_tag) = class_tag {
                    occurrences.extend(occurrence_transforms(
                        bytes,
                        headers,
                        &reference.neutron_role,
                        class_tag,
                        matrices,
                    ));
                }
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
}

fn role_adjacent_transforms(
    bytes: &[u8],
    records: &[IndexedRecord],
    role: &str,
) -> Vec<[[f64; 4]; 4]> {
    records
        .iter()
        .flat_map(|record| {
            role_tails(bytes, record, role)
                .into_iter()
                .filter_map(|after_role| {
                    let matrix_at = after_role.checked_add(2)?;
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
    class_tag: String,
    record_index: u64,
}

fn xref_class_tag(bytes: &[u8], records: &[IndexedRecord], roles: &[&str]) -> Option<String> {
    let mut by_tag = std::collections::HashMap::<String, std::collections::HashSet<&str>>::new();
    for record in records {
        for role in roles {
            let tails = role_tails(bytes, record, role);
            if tails.len() == 1 && record_occurrence_tail(bytes, record, tails[0]).is_some() {
                by_tag
                    .entry(record.class_tag.clone())
                    .or_default()
                    .insert(role);
            }
        }
    }
    let maximum = by_tag.values().map(std::collections::HashSet::len).max()?;
    let mut candidates = by_tag
        .into_iter()
        .filter(|(_, roles)| roles.len() == maximum);
    let (class_tag, _) = candidates.next()?;
    candidates.next().is_none().then_some(class_tag)
}

fn occurrence_transforms(
    bytes: &[u8],
    records: &[IndexedRecord],
    role: &str,
    xref_class_tag: &str,
    indexed_matrices: &mut std::collections::HashMap<u64, Option<[[f64; 4]; 4]>>,
) -> Vec<Option<[[f64; 4]; 4]>> {
    let mut out = Vec::new();
    for record in records {
        if record.class_tag != xref_class_tag {
            continue;
        }
        let tails = role_tails(bytes, record, role);
        let [after_role] = tails.as_slice() else {
            continue;
        };
        let Some(tail) = record_occurrence_tail(bytes, record, *after_role) else {
            continue;
        };
        let mut matrices = tail.transform.into_iter().collect::<Vec<_>>();
        if matrices.is_empty() {
            matrices.extend(tail.references.into_iter().filter_map(|reference| {
                indexed_transform(bytes, records, reference, indexed_matrices)
            }));
        }
        matrices.sort_by(matrix_order);
        matrices.dedup();
        out.push(match matrices.as_slice() {
            [matrix] => Some(*matrix),
            _ => None,
        });
    }
    out
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
            let Some(record_index) = bytes
                .get(after_tag..after_tag + 8)
                .and_then(|raw| raw.try_into().ok())
                .map(u64::from_le_bytes)
            else {
                continue;
            };
            headers.push((at, class_tag, record_index));
        }
    }
    headers
        .iter()
        .enumerate()
        .map(
            |(ordinal, (offset, class_tag, record_index))| IndexedRecord {
                offset: *offset,
                end: headers
                    .get(ordinal + 1)
                    .map_or(bytes.len(), |(offset, _, _)| *offset),
                class_tag: class_tag.clone(),
                record_index: *record_index,
            },
        )
        .collect()
}

fn role_tails(bytes: &[u8], record: &IndexedRecord, value: &str) -> Vec<usize> {
    let encoded = value.encode_utf16().collect::<Vec<_>>();
    let mut needle = Vec::with_capacity(4 + encoded.len() * 2);
    needle.extend_from_slice(&(encoded.len() as u32).to_le_bytes());
    needle.extend(encoded.into_iter().flat_map(u16::to_le_bytes));
    bytes[record.offset..record.end]
        .windows(needle.len())
        .enumerate()
        .filter_map(|(relative, candidate)| {
            (candidate == needle).then_some(record.offset + relative + needle.len())
        })
        .collect()
}

struct OccurrenceTail {
    /// Record index of the marked reference opening the tail.
    #[allow(dead_code)]
    lead_reference: u64,
    references: Vec<u64>,
    transform: Option<[[f64; 4]; 4]>,
}

fn record_occurrence_tail(
    bytes: &[u8],
    record: &IndexedRecord,
    after_role: usize,
) -> Option<OccurrenceTail> {
    occurrence_tail(
        bytes.get(record.offset..record.end)?,
        after_role.checked_sub(record.offset)?,
    )
}

fn occurrence_tail(bytes: &[u8], mut at: usize) -> Option<OccurrenceTail> {
    if bytes.get(at) != Some(&0) {
        return None;
    }
    // A marked record reference follows the leading zero byte: `0x01`, a
    // little-endian u32 record index, and six zero bytes.
    if bytes.get(at + 1) != Some(&1) || bytes.get(at + 6..at + 12) != Some(&[0u8; 6]) {
        return None;
    }
    let lead_reference = u64::from(u32::from_le_bytes(
        bytes.get(at + 2..at + 6)?.try_into().ok()?,
    ));
    at += 12;
    let mut references = Vec::new();
    while bytes.get(at) == Some(&1) {
        if bytes.get(at + 9..at + 11) != Some(&[0, 0]) {
            return None;
        }
        references.push(u64::from_le_bytes(
            bytes.get(at + 1..at + 9)?.try_into().ok()?,
        ));
        at = at.checked_add(15)?;
    }
    if bytes.get(at..at + 2) != Some(&[0, 0]) {
        return None;
    }
    Some(OccurrenceTail {
        lead_reference,
        references,
        transform: decode_rigid_matrix(bytes, at + 2),
    })
}

fn indexed_transform(
    bytes: &[u8],
    records: &[IndexedRecord],
    record_index: u64,
    cache: &mut std::collections::HashMap<u64, Option<[[f64; 4]; 4]>>,
) -> Option<[[f64; 4]; 4]> {
    if let Some(matrix) = cache.get(&record_index) {
        return *matrix;
    }
    let mut matrices = records
        .iter()
        .filter(|record| record.record_index == record_index)
        .flat_map(|record| indexed_placement_matrices(bytes, record))
        .collect::<Vec<_>>();
    matrices.sort_by(matrix_order);
    matrices.dedup();
    let resolved = match matrices.as_slice() {
        [matrix] => Some(*matrix),
        _ => None,
    };
    cache.insert(record_index, resolved);
    resolved
}

fn indexed_placement_matrices<'a>(
    bytes: &'a [u8],
    record: &'a IndexedRecord,
) -> impl Iterator<Item = [[f64; 4]; 4]> + 'a {
    const FIRST_MATRIX_OFFSET: usize = 32;
    const MATRIX_STRIDE: usize = 142;

    std::iter::successors(record.offset.checked_add(FIRST_MATRIX_OFFSET), |at| {
        at.checked_add(MATRIX_STRIDE)
    })
    .take_while(|at| at.checked_add(128).is_some_and(|end| end <= record.end))
    .filter_map(|at| decode_rigid_matrix(bytes, at))
}

fn matrix_order(left: &[[f64; 4]; 4], right: &[[f64; 4]; 4]) -> std::cmp::Ordering {
    left.iter()
        .flatten()
        .zip(right.iter().flatten())
        .map(|(left, right)| left.total_cmp(right))
        .find(|order| !order.is_eq())
        .unwrap_or(std::cmp::Ordering::Equal)
}

fn decode_rigid_matrix(bytes: &[u8], at: usize) -> Option<[[f64; 4]; 4]> {
    let mut rows = [[0.0; 4]; 4];
    for (index, value) in rows.iter_mut().flatten().enumerate() {
        let offset = at.checked_add(index.checked_mul(8)?)?;
        *value = f64::from_le_bytes(bytes.get(offset..offset + 8)?.try_into().ok()?);
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
mod tests {
    #[test]
    fn redirections_keep_neutron_role_and_data_independent() {
        let table = super::parse(
            br#"{"designs":[],"references":[{"from":"root.f3d","relativePath":"part.f3d","type":"XREF","properties":[{"neutronRole":{"value":"role-guid","dataType":"STRING"}},{"neutronData":{"value":"data-guid","dataType":"STRING"}}]}]}"#,
        )
        .expect("redirections JSON");
        assert_eq!(table.references.len(), 1);
        assert_eq!(table.references[0].neutron_role, "role-guid");
        assert_eq!(table.references[0].neutron_data, "data-guid");
    }

    #[test]
    fn external_reference_placements_project_as_root_occurrences_in_millimetres() {
        let transform = [
            [0.0, -1.0, 0.0, 1.0],
            [1.0, 0.0, 0.0, 2.0],
            [0.0, 0.0, 1.0, 3.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let table = super::XrefTable {
            designs: Vec::new(),
            references: vec![crate::records::XrefReference {
                id: "f3d:xref:reference#0-occurrence-0".into(),
                ordinal: 0,
                occurrence_ordinal: 0,
                from: "root.f3d".into(),
                relative_path: "part.f3d".into(),
                neutron_role: "role".into(),
                neutron_data: "data".into(),
                transform: Some(transform),
            }],
        };

        let occurrences = super::project_occurrences(&table);

        assert_eq!(occurrences.len(), 1);
        assert_eq!(occurrences[0].id.0, "f3d:model:occurrence#xref-0-0");
        assert_eq!(
            occurrences[0].local_transform,
            [
                [0.0, -1.0, 0.0, 10.0],
                [1.0, 0.0, 0.0, 20.0],
                [0.0, 0.0, 1.0, 30.0],
                [0.0, 0.0, 0.0, 1.0],
            ]
        );
        assert_eq!(
            occurrences[0].resolved_transform,
            occurrences[0].local_transform
        );
        assert!(occurrences[0].parent.is_none());
        assert_eq!(
            occurrences[0].prototype,
            cadmpeg_ir::products::ComponentReference::External {
                document: cadmpeg_ir::products::ExternalDocumentReference {
                    path: Some("part.f3d".into()),
                    document_id: None,
                    resolution: cadmpeg_ir::products::ExternalResolution::Unresolved,
                },
                object: None,
            }
        );
    }

    #[test]
    fn component_reference_data_is_an_open_json_object() {
        let value = super::parse_component_reference_data(
            br#"{"schema":7,"references":[{"id":"component"}],"extension":{"x":true}}"#,
        )
        .expect("open component-reference object");
        assert_eq!(value["schema"], 7);
        assert!(super::parse_component_reference_data(br"[]").is_err());
        assert!(super::parse_component_reference_data(b"not-json").is_err());
    }

    fn placement_record(
        index: u64,
        transforms: &[[[f64; 4]; 4]],
        decoy: Option<[[f64; 4]; 4]>,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&3_u32.to_le_bytes());
        bytes.extend_from_slice(b"381");
        bytes.extend_from_slice(&index.to_le_bytes());
        bytes.extend_from_slice(&[0; 17]);
        for (ordinal, transform) in transforms.iter().enumerate() {
            if ordinal != 0 {
                bytes.extend_from_slice(&[0; 14]);
            }
            for value in transform.iter().flatten() {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        if let Some(decoy) = decoy {
            bytes.extend_from_slice(&[0; 5]);
            for value in decoy.into_iter().flatten() {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        bytes
    }

    fn occurrence_record(
        role: &str,
        ordinal: u32,
        references: &[u64],
        transform: Option<[[f64; 4]; 4]>,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&3_u32.to_le_bytes());
        bytes.extend_from_slice(b"380");
        bytes.extend_from_slice(&ordinal.to_le_bytes());
        bytes.resize(185, 0);
        let role = role.encode_utf16().collect::<Vec<_>>();
        bytes.extend_from_slice(&(role.len() as u32).to_le_bytes());
        for value in role {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0);
        bytes.push(1);
        bytes.extend_from_slice(&ordinal.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        for reference in references {
            bytes.push(1);
            bytes.extend_from_slice(&reference.to_le_bytes());
            bytes.extend_from_slice(&[0, 0]);
            bytes.extend_from_slice(&ordinal.to_le_bytes());
        }
        bytes.extend_from_slice(&[0, 0]);
        if let Some(transform) = transform {
            for value in transform.into_iter().flatten() {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        bytes
    }

    fn direct_occurrence_record(role: &str, transforms: &[[[f64; 4]; 4]]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&3_u32.to_le_bytes());
        bytes.extend_from_slice(b"382");
        bytes.extend_from_slice(&10_u64.to_le_bytes());
        for transform in transforms {
            bytes.extend_from_slice(&[0; 9]);
            let role = role.encode_utf16().collect::<Vec<_>>();
            bytes.extend_from_slice(&(role.len() as u32).to_le_bytes());
            for value in role {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            bytes.extend_from_slice(&[0, 0]);
            for value in transform.iter().flatten() {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        bytes
    }

    #[test]
    fn occurrence_records_expand_shared_roles_and_decode_rigid_matrices() {
        let first = [
            [0.0, -1.0, 0.0, 2.0],
            [1.0, 0.0, 0.0, 3.0],
            [0.0, 0.0, 1.0, 4.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let second = [
            [1.0, 0.0, 0.0, -5.0],
            [0.0, 1.0, 0.0, 6.0],
            [0.0, 0.0, 1.0, 7.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let mut bytes = occurrence_record("role", 10, &[], Some(first));
        bytes.extend_from_slice(&occurrence_record("role", 11, &[42, 43], Some(second)));
        let mut matrices = std::collections::HashMap::new();

        assert_eq!(
            super::occurrence_transforms(
                &bytes,
                &super::indexed_records(&bytes),
                "role",
                "380",
                &mut matrices,
            ),
            vec![Some(first), Some(second)]
        );
    }

    #[test]
    fn repeated_roles_retain_each_directly_adjacent_occurrence_transform() {
        let first = [
            [1.0, 0.0, 0.0, -1.3],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let second = [
            [-1.0, 0.0, 0.0, -5.8],
            [0.0, 1.0, 0.0, 6.16],
            [0.0, 0.0, -1.0, 0.568],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let bytes = direct_occurrence_record("role", &[first, second]);

        assert_eq!(
            super::role_adjacent_transforms(&bytes, &super::indexed_records(&bytes), "role"),
            [first, second]
        );
    }

    #[test]
    fn absent_or_nonrigid_occurrence_matrix_is_identity_placement() {
        let mut nonrigid = [[0.0; 4]; 4];
        nonrigid[0][0] = 2.0;
        nonrigid[1][1] = 1.0;
        nonrigid[2][2] = 1.0;
        nonrigid[3][3] = 1.0;
        let mut bytes = occurrence_record("role", 10, &[], None);
        bytes.extend_from_slice(&occurrence_record("role", 11, &[], Some(nonrigid)));
        let mut matrices = std::collections::HashMap::new();

        assert_eq!(
            super::occurrence_transforms(
                &bytes,
                &super::indexed_records(&bytes),
                "role",
                "380",
                &mut matrices,
            ),
            vec![None, None]
        );
    }

    #[test]
    fn role_adjacent_matrix_cannot_cross_the_indexed_record_boundary() {
        let transform = [
            [1.0, 0.0, 0.0, 2.0],
            [0.0, 1.0, 0.0, 3.0],
            [0.0, 0.0, 1.0, 4.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let bytes = occurrence_record("role", 10, &[], Some(transform));
        let role_tail = 185 + 4 + "role".len() * 2;
        let matrix_at = role_tail + 14;
        let record = |end| super::IndexedRecord {
            offset: 0,
            end,
            class_tag: "380".into(),
            record_index: 10,
        };

        assert_eq!(
            super::record_occurrence_tail(&bytes, &record(matrix_at), role_tail)
                .and_then(|tail| tail.transform),
            None
        );
        assert_eq!(
            super::record_occurrence_tail(&bytes, &record(bytes.len()), role_tail)
                .and_then(|tail| tail.transform),
            Some(transform)
        );
    }

    #[test]
    fn identity_occurrence_records_identify_the_xref_class_and_preserve_multiplicity() {
        let mut bytes = occurrence_record("role", 10, &[], None);
        bytes.extend_from_slice(&occurrence_record("role", 11, &[], None));
        let records = super::indexed_records(&bytes);
        let mut matrices = std::collections::HashMap::new();
        let class_tag = super::xref_class_tag(&bytes, &records, &["role"]);

        assert_eq!(class_tag.as_deref(), Some("380"));
        assert_eq!(
            super::occurrence_transforms(&bytes, &records, "role", "380", &mut matrices),
            vec![None, None]
        );
    }

    #[test]
    fn occurrence_resolves_matrix_from_referenced_indexed_record() {
        let transform = [
            [0.0, 0.0, 1.0, 2.0],
            [0.0, 1.0, 0.0, 3.0],
            [-1.0, 0.0, 0.0, 4.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let mut bytes = placement_record(42, &[transform], None);
        bytes.extend_from_slice(&occurrence_record("role", 10, &[42], None));
        let records = super::indexed_records(&bytes);
        let mut matrices = std::collections::HashMap::new();
        let class_tag = super::xref_class_tag(&bytes, &records, &["role"]);

        assert_eq!(class_tag.as_deref(), Some("380"));
        assert_eq!(
            super::occurrence_transforms(&bytes, &records, "role", "380", &mut matrices),
            vec![Some(transform)]
        );
    }

    #[test]
    fn role_adjacent_matrix_precedes_other_referenced_matrices() {
        let local = [
            [1.0, 0.0, 0.0, 5.0],
            [0.0, 1.0, 0.0, 6.0],
            [0.0, 0.0, 1.0, 7.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let referenced = [
            [0.0, -1.0, 0.0, 2.0],
            [1.0, 0.0, 0.0, 3.0],
            [0.0, 0.0, 1.0, 4.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let mut bytes = placement_record(42, &[referenced], None);
        bytes.extend_from_slice(&occurrence_record("role", 10, &[42], Some(local)));
        let records = super::indexed_records(&bytes);
        let mut matrices = std::collections::HashMap::new();

        assert_eq!(
            super::occurrence_transforms(&bytes, &records, "role", "380", &mut matrices),
            vec![Some(local)]
        );
    }

    #[test]
    fn indexed_placement_ignores_rigid_matrices_outside_list_slots() {
        let placement = [
            [1.0, 0.0, 0.0, 2.0],
            [0.0, 1.0, 0.0, 3.0],
            [0.0, 0.0, 1.0, 4.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let decoy = [
            [0.0, -1.0, 0.0, 5.0],
            [1.0, 0.0, 0.0, 6.0],
            [0.0, 0.0, 1.0, 7.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let mut bytes = placement_record(42, &[placement], Some(decoy));
        bytes.extend_from_slice(&occurrence_record("role", 10, &[42], None));
        let records = super::indexed_records(&bytes);
        let mut matrices = std::collections::HashMap::new();

        assert_eq!(
            super::occurrence_transforms(&bytes, &records, "role", "380", &mut matrices),
            vec![Some(placement)]
        );
    }

    #[test]
    fn indexed_placement_decodes_back_to_back_matrix_list() {
        let first = [
            [1.0, 0.0, 0.0, 2.0],
            [0.0, 1.0, 0.0, 3.0],
            [0.0, 0.0, 1.0, 4.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let second = [
            [0.0, -1.0, 0.0, 5.0],
            [1.0, 0.0, 0.0, 6.0],
            [0.0, 0.0, 1.0, 7.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let bytes = placement_record(42, &[first, second], None);
        let records = super::indexed_records(&bytes);

        assert_eq!(
            super::indexed_placement_matrices(&bytes, &records[0]).collect::<Vec<_>>(),
            vec![first, second]
        );
    }
}
