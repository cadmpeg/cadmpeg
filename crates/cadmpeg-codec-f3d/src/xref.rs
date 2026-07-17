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
    parse(bytes).map(Some)
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
            neutron_role: reference.property("neutronRole"),
            neutron_data: reference.property("neutronData"),
            from: reference.from,
            relative_path: reference.relative_path,
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
