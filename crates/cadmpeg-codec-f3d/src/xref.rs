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

use cadmpeg_codec_core::le::u32_at;
use cadmpeg_codec_core::CodecError;
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
            let placements = occurrence_placements(bytes, &headers);
            (bytes, headers, placements)
        })
        .collect::<Vec<_>>();
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
fn occurrence_placements(bytes: &[u8], records: &[IndexedRecord]) -> Vec<OccurrencePlacement> {
    records
        .iter()
        .filter_map(|record| occurrence_placement(bytes.get(record.offset..record.end)?))
        .collect()
}

/// Parse one record body, header included, requiring the member sequence to end
/// exactly at the record end.
fn occurrence_placement(body: &[u8]) -> Option<OccurrencePlacement> {
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
    let count = usize::try_from(u32_at(body, at)?).ok()?;
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
        discriminators.push(u32_at(body, at)?);
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
        if placement_tail(body, cursor).is_some() {
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
fn placement_tail(body: &[u8], mut at: usize) -> Option<()> {
    let count = usize::try_from(u32_at(body, at)?).ok()?;
    if count > 256 {
        return None;
    }
    at += 4;
    for _ in 0..count {
        take_reference(body, &mut at)?;
    }
    // A modern container inserts a tagged u32 run and one reference before the
    // two closing references. Its tag byte is neither reference presence value.
    if !matches!(body.get(at), Some(0 | 1)) {
        at += 1;
        let tagged = usize::try_from(u32_at(body, at)?).ok()?;
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
            occurrences[0].transform.rows,
            [
                [0.0, -1.0, 0.0, 10.0],
                [1.0, 0.0, 0.0, 20.0],
                [0.0, 0.0, 1.0, 30.0],
                [0.0, 0.0, 0.0, 1.0],
            ]
        );
        assert_eq!(
            occurrences[0].parent,
            cadmpeg_ir::products::OccurrenceParent::Root
        );
        assert_eq!(
            occurrences[0].prototype,
            cadmpeg_ir::products::PrototypeReference::External {
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

    fn local_reference(target: u64) -> Vec<u8> {
        let mut bytes = vec![1];
        bytes.extend_from_slice(&target.to_le_bytes());
        bytes.extend_from_slice(&[0, 0]);
        bytes
    }

    fn cross_document_reference(target: u64, link_name: &str) -> Vec<u8> {
        let mut bytes = vec![1];
        bytes.extend_from_slice(&target.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend(crate::bytes::lp_utf16_bytes(
            "11111111-2222-3333-4444-555555555555",
        ));
        bytes.push(0);
        bytes.extend_from_slice(&36_u32.to_le_bytes());
        bytes.extend_from_slice(b"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
        bytes.extend(crate::bytes::lp_utf16_bytes(link_name));
        bytes.push(0);
        bytes
    }

    /// One occurrence-placement record: a target path whose last element
    /// carries `role` as its cross-document link name, the identity marker,
    /// and the three closing reference runs.
    fn occurrence_record(
        role: &str,
        entity_id: u64,
        discriminators: &[u32],
        transform: Option<[[f64; 4]; 4]>,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&3_u32.to_le_bytes());
        bytes.extend_from_slice(b"380");
        bytes.extend_from_slice(&entity_id.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&(discriminators.len() as u32).to_le_bytes());
        for (ordinal, discriminator) in discriminators.iter().enumerate() {
            let target = 100 + ordinal as u64;
            if ordinal + 1 == discriminators.len() {
                bytes.extend(cross_document_reference(target, role));
            } else {
                bytes.extend(local_reference(target));
            }
            bytes.extend_from_slice(&discriminator.to_le_bytes());
        }
        bytes.push(0);
        match transform {
            Some(transform) => {
                bytes.push(0);
                for value in transform.into_iter().flatten() {
                    bytes.extend_from_slice(&value.to_le_bytes());
                }
            }
            None => bytes.push(1),
        }
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend(local_reference(7));
        bytes.extend(local_reference(3));
        bytes.extend(local_reference(6));
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
        let mut bytes = occurrence_record("role", 10, &[1], Some(first));
        bytes.extend_from_slice(&occurrence_record("role", 11, &[1, 2], Some(second)));
        let placements = super::occurrence_placements(&bytes, &super::indexed_records(&bytes));

        assert_eq!(
            super::occurrence_transforms(&placements, "role"),
            vec![Some(first), Some(second)]
        );
    }

    #[test]
    fn identity_marked_placement_stores_no_matrix() {
        let mut bytes = occurrence_record("role", 10, &[1], None);
        bytes.extend_from_slice(&occurrence_record("role", 11, &[3], None));
        let placements = super::occurrence_placements(&bytes, &super::indexed_records(&bytes));

        assert_eq!(placements.len(), 2);
        assert_eq!(
            super::occurrence_transforms(&placements, "role"),
            vec![None, None]
        );
    }

    #[test]
    fn placement_keeps_the_instance_discriminator_of_every_path_element() {
        let bytes = occurrence_record("role", 10, &[7, 4, 2], None);
        let placements = super::occurrence_placements(&bytes, &super::indexed_records(&bytes));

        assert_eq!(placements[0].discriminators, vec![7, 4, 2]);
        assert_eq!(placements[0].link_names, vec!["role".to_owned()]);
    }

    #[test]
    fn a_placement_that_does_not_close_on_the_record_end_is_not_a_placement() {
        let mut bytes = occurrence_record("role", 10, &[1], None);
        bytes.push(0);
        let records = super::indexed_records(&bytes);

        assert_eq!(super::occurrence_placements(&bytes, &records), Vec::new());
    }

    #[test]
    fn a_nonrigid_matrix_is_not_a_placement() {
        let mut nonrigid = [[0.0; 4]; 4];
        nonrigid[0][0] = 2.0;
        nonrigid[1][1] = 1.0;
        nonrigid[2][2] = 1.0;
        nonrigid[3][3] = 1.0;
        let bytes = occurrence_record("role", 10, &[1], Some(nonrigid));
        let records = super::indexed_records(&bytes);

        assert_eq!(super::occurrence_placements(&bytes, &records), Vec::new());
    }

    #[test]
    fn a_role_that_no_path_element_names_places_nothing() {
        let bytes = occurrence_record("role", 10, &[1], None);
        let placements = super::occurrence_placements(&bytes, &super::indexed_records(&bytes));

        assert_eq!(
            super::occurrence_transforms(&placements, "other"),
            Vec::new()
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
}
