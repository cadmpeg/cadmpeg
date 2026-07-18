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

use crate::container::role;
use crate::container::ContainerScan;
use crate::records::{XrefDesign, XrefReference};

/// Top-level container entry holding the external-reference table.
pub const REDIRECTIONS_ENTRY: &str = "RedirectionsStream.dat";
/// Top-level container entry holding the document-properties slot.
pub const PROPERTIES_ENTRY: &str = "Properties.dat";

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

/// Parse the top-level `RedirectionsStream.dat` table, if present.
pub fn decode(scan: &ContainerScan) -> Result<Option<XrefTable>, CodecError> {
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

/// Expand container references through their occurrence records in the active
/// Design `BulkStream` and retain each occurrence-local placement matrix.
pub(crate) fn bind_occurrences(scan: &ContainerScan, table: &mut XrefTable) {
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
    let streams = streams
        .filter_map(|entry| scan.entry_bytes(&entry.name).ok())
        .filter_map(|bytes| {
            let headers = indexed_headers(bytes);
            let class_tag = xref_class_tag(bytes, &headers, &roles)?;
            Some((bytes, headers, class_tag))
        })
        .collect::<Vec<_>>();
    let mut expanded = Vec::new();
    for reference in &table.references {
        let mut occurrences = Vec::new();
        for (bytes, headers, class_tag) in &streams {
            occurrences.extend(occurrence_transforms(
                bytes,
                headers,
                &reference.neutron_role,
                class_tag,
            ));
        }
        if occurrences.is_empty() {
            expanded.push(reference.clone());
            continue;
        }
        for (occurrence_ordinal, transform) in occurrences.into_iter().enumerate() {
            let mut occurrence = reference.clone();
            occurrence.id = format!(
                "f3d:xref:reference#{}/occurrence#{occurrence_ordinal}",
                reference.ordinal
            );
            occurrence.occurrence_ordinal = occurrence_ordinal as u32;
            occurrence.transform = transform;
            expanded.push(occurrence);
        }
    }
    table.references = expanded;
}

fn xref_class_tag(bytes: &[u8], headers: &[(usize, String)], roles: &[&str]) -> Option<String> {
    let encoded = roles
        .iter()
        .map(|role| (role.encode_utf16().collect::<Vec<_>>(), *role))
        .collect::<Vec<_>>();
    let mut by_tag = std::collections::HashMap::<String, std::collections::HashSet<&str>>::new();
    for (at, class_tag) in headers {
        let role_at = *at + 185;
        let Some((candidate, _)) = lp_utf16(bytes, role_at) else {
            continue;
        };
        if let Some((_, role)) = encoded.iter().find(|(encoded, _)| *encoded == candidate) {
            by_tag.entry(class_tag.clone()).or_default().insert(role);
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
    headers: &[(usize, String)],
    role: &str,
    xref_class_tag: &str,
) -> Vec<Option<[[f64; 4]; 4]>> {
    let encoded_role = role.encode_utf16().collect::<Vec<_>>();
    let mut out = Vec::new();
    for (ordinal, (at, class_tag)) in headers.iter().enumerate() {
        if class_tag != xref_class_tag {
            continue;
        }
        let at = *at;
        let role_at = at + 185;
        let Some((candidate, _)) = lp_utf16(bytes, role_at) else {
            continue;
        };
        if candidate != encoded_role {
            continue;
        }
        let end = headers
            .get(ordinal + 1)
            .map_or(bytes.len(), |(offset, _)| *offset);
        let mut matrices = Vec::new();
        let mut search_at = at;
        while search_at < end {
            let Some(relative) = find_lp_utf16(bytes.get(search_at..end).unwrap_or_default(), role)
            else {
                break;
            };
            let role_at = search_at + relative;
            let Some((_, after_role)) = lp_utf16(bytes, role_at) else {
                break;
            };
            if let Some(matrix) = role_adjacent_transform(bytes, after_role) {
                if !matrices.contains(&matrix) {
                    matrices.push(matrix);
                }
            }
            search_at = after_role;
        }
        out.push(match matrices.as_slice() {
            [matrix] => Some(*matrix),
            _ => None,
        });
    }
    out
}

fn indexed_headers(bytes: &[u8]) -> Vec<(usize, String)> {
    let mut headers = Vec::new();
    for at in 0..bytes.len().saturating_sub(11) {
        let Some((class_tag, after_tag)) = lp_ascii(bytes, at) else {
            continue;
        };
        if after_tag == at + 7
            && class_tag.len() == 3
            && class_tag.bytes().all(|byte| byte.is_ascii_digit())
        {
            headers.push((at, class_tag));
        }
    }
    headers
}

fn find_lp_utf16(bytes: &[u8], value: &str) -> Option<usize> {
    let encoded = value.encode_utf16().collect::<Vec<_>>();
    (0..bytes.len().saturating_sub(4))
        .find(|at| lp_utf16(bytes, *at).is_some_and(|(candidate, _)| candidate == encoded))
}

fn role_adjacent_transform(bytes: &[u8], mut at: usize) -> Option<[[f64; 4]; 4]> {
    if bytes.get(at) != Some(&0) {
        return None;
    }
    at += 5;
    while bytes.get(at) == Some(&1) {
        if bytes.get(at + 9..at + 11) != Some(&[0, 0]) {
            return None;
        }
        at = at.checked_add(15)?;
    }
    if bytes.get(at..at + 2) != Some(&[0, 0]) {
        return None;
    }
    decode_rigid_matrix(bytes, at + 2)
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

fn lp_ascii(bytes: &[u8], at: usize) -> Option<(String, usize)> {
    let count =
        usize::try_from(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?)).ok()?;
    let end = at.checked_add(4)?.checked_add(count)?;
    Some((
        std::str::from_utf8(bytes.get(at + 4..end)?)
            .ok()?
            .to_owned(),
        end,
    ))
}

fn lp_utf16(bytes: &[u8], at: usize) -> Option<(Vec<u16>, usize)> {
    let count =
        usize::try_from(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?)).ok()?;
    let end = at.checked_add(4)?.checked_add(count.checked_mul(2)?)?;
    let values = bytes
        .get(at + 4..end)?
        .chunks_exact(2)
        .map(|raw| u16::from_le_bytes([raw[0], raw[1]]))
        .collect();
    Some((values, end))
}

#[cfg(test)]
mod tests {
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
        bytes.extend_from_slice(&ordinal.to_le_bytes());
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

        assert_eq!(
            super::occurrence_transforms(&bytes, &super::indexed_headers(&bytes), "role", "380"),
            vec![Some(first), Some(second)]
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

        assert_eq!(
            super::occurrence_transforms(&bytes, &super::indexed_headers(&bytes), "role", "380"),
            vec![None, None]
        );
    }
}
