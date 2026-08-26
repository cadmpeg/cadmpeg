// SPDX-License-Identifier: Apache-2.0
//! Parse the document and asset manifests that assign archive folders to
//! Fusion assets.

use std::collections::BTreeSet;

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;

use crate::bytes::is_guid_hyphenated;

const MAX_MANIFEST_STRING_UNITS: usize = 4 * 1024;
const MAX_REGISTRY_ENTRIES: usize = 64;
const MAX_ASSET_FOLDERS: usize = 64;
const MAX_TOP_LEVEL_MANIFEST_BYTES: usize = 16 * 1024 * 1024;
const DESIGN_ASSET_TYPE: &str = "FusionAssetType";

pub(crate) const GENERATED_DESIGN_ASSET_BASE: &str = "FusionAssetName";
pub(crate) const GENERATED_DESIGN_ASSET_FOLDER: &str = "FusionAssetName[Active]";

const GENERATED_DOCUMENT_GUID: &str = "00000000-0000-4000-8000-000000000001";
const GENERATED_DOCUMENT_ASSET_GUID: &str = "00000000-0000-4000-8000-000000000002";
const GENERATED_ASSET_FOLDER_GUID: &str = "00000000-0000-4000-8000-000000000003";
const GENERATED_ASSET_GUID: &str = "00000000-0000-4000-8000-000000000004";
const GENERATED_PHYSICAL_CHANGE_GUID: &str = "00000000-0000-4000-8000-000000000005";

/// Fields from the top-level manifest that govern asset-folder ownership.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TopLevelManifest {
    asset_folder_bases: Vec<String>,
}

/// Prefix fields that identify one asset manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AssetManifestHeader {
    base_name: String,
    asset_type: String,
    fusion_subtype: Option<String>,
}

struct Cursor<'a> {
    view: View<'a>,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            view: View::over_retained(bytes),
        }
    }

    fn from_offset(bytes: &'a [u8], at: usize) -> Result<Self, CodecError> {
        let mut view = View::over_retained(bytes);
        view.seek(at).ok_or_else(|| truncated("manifest offset"))?;
        Ok(Self { view })
    }

    fn position(&self) -> usize {
        self.view.position()
    }

    fn exhausted(&self) -> bool {
        self.view.is_empty()
    }

    fn u8(&mut self, field: &str) -> Result<u8, CodecError> {
        self.view.u8().ok_or_else(|| truncated(field))
    }

    fn expect_u8(&mut self, field: &str, expected: u8) -> Result<(), CodecError> {
        let actual = self.u8(field)?;
        if actual != expected {
            return Err(malformed(
                field,
                format!("expected {expected}, found {actual}"),
            ));
        }
        Ok(())
    }

    fn u32(&mut self, field: &str) -> Result<u32, CodecError> {
        self.view.u32_le().ok_or_else(|| truncated(field))
    }

    fn expect_u32(&mut self, field: &str, expected: u32) -> Result<(), CodecError> {
        let actual = self.u32(field)?;
        if actual != expected {
            return Err(malformed(
                field,
                format!("expected {expected}, found {actual}"),
            ));
        }
        Ok(())
    }

    fn ascii(&mut self, field: &str) -> Result<String, CodecError> {
        let count = self.count(field, MAX_MANIFEST_STRING_UNITS)?;
        let raw = self.view.take(count).ok_or_else(|| truncated(field))?;
        if !raw.iter().all(|byte| matches!(byte, 0x20..=0x7e)) {
            return Err(malformed(field, "contains a non-printable ASCII byte"));
        }
        Ok(std::str::from_utf8(raw)
            .expect("invariant: printable ASCII is UTF-8")
            .to_owned())
    }

    fn expect_ascii(&mut self, field: &str, expected: &str) -> Result<(), CodecError> {
        let actual = self.ascii(field)?;
        if actual != expected {
            return Err(malformed(
                field,
                format!("expected {expected:?}, found {actual:?}"),
            ));
        }
        Ok(())
    }

    fn utf16(&mut self, field: &str) -> Result<String, CodecError> {
        let count = self.count(field, MAX_MANIFEST_STRING_UNITS)?;
        self.utf16_with_count(count, field)
    }

    fn utf16_with_count(&mut self, count: usize, field: &str) -> Result<String, CodecError> {
        let needed = count.checked_mul(2).ok_or_else(|| truncated(field))?;
        if self.view.remaining() < needed {
            return Err(truncated(field));
        }
        self.view
            .utf16_le(count)
            .ok_or_else(|| malformed(field, "contains invalid UTF-16LE"))
    }

    fn expect_utf16(&mut self, field: &str, expected: &str) -> Result<(), CodecError> {
        let actual = self.utf16(field)?;
        if actual != expected {
            return Err(malformed(
                field,
                format!("expected {expected:?}, found {actual:?}"),
            ));
        }
        Ok(())
    }

    fn guid(&mut self, field: &str) -> Result<String, CodecError> {
        let value = self.utf16(field)?;
        if !is_guid_hyphenated(&value) {
            return Err(malformed(field, format!("invalid GUID {value:?}")));
        }
        Ok(value)
    }

    fn count(&mut self, field: &str, max: usize) -> Result<usize, CodecError> {
        let count = usize::try_from(self.u32(field)?)
            .map_err(|_| malformed(field, "count does not fit memory"))?;
        if count > max {
            return Err(malformed(
                field,
                format!("count {count} exceeds the limit {max}"),
            ));
        }
        Ok(count)
    }

    fn finish(self, field: &str) -> Result<(), CodecError> {
        if !self.view.is_empty() {
            return Err(malformed(
                field,
                format!("{} trailing byte(s)", self.view.remaining()),
            ));
        }
        Ok(())
    }
}

/// Parse the top-level `Manifest.dat` header, capability registry, and exact
/// asset-folder tail.
pub(crate) fn parse_top_level(bytes: &[u8]) -> Result<TopLevelManifest, CodecError> {
    if bytes.len() > MAX_TOP_LEVEL_MANIFEST_BYTES {
        return Err(malformed(
            "top-level manifest",
            format!(
                "{} bytes exceed the limit {MAX_TOP_LEVEL_MANIFEST_BYTES}",
                bytes.len()
            ),
        ));
    }
    let mut cursor = Cursor::new(bytes);
    cursor.expect_ascii("top-level manifest version", "3-2-0-0")?;
    cursor.expect_ascii("top-level manifest kind", "FusionDocType")?;
    cursor.expect_utf16("top-level manifest extension", ".f3d")?;
    let _display_name = cursor.utf16("top-level manifest display name")?;
    let _description = cursor.utf16("top-level manifest description")?;
    let _document_guid = cursor.guid("top-level manifest document GUID")?;
    let _document_asset_guid = cursor.guid("top-level manifest document-asset GUID")?;

    let generation = cursor.u32("top-level manifest generation")?;
    let registry_count = if generation == 1234 {
        let _generation_major = cursor.u32("top-level manifest generation major")?;
        let _generation_minor = cursor.u32("top-level manifest generation minor")?;
        let _generation_flags = cursor.u32("top-level manifest generation flags")?;
        bounded_count(
            cursor.u32("top-level manifest registry count")?,
            MAX_REGISTRY_ENTRIES,
            "top-level manifest registry count",
        )?
    } else {
        let _legacy_generation = cursor.u32("top-level manifest legacy generation")?;
        bounded_count(
            cursor.u32("top-level manifest registry count")?,
            MAX_REGISTRY_ENTRIES,
            "top-level manifest registry count",
        )?
    };

    let mut registry_names = BTreeSet::new();
    for ordinal in 0..registry_count {
        let name = cursor.ascii(&format!("top-level manifest registry name {ordinal}"))?;
        if name.is_empty() || !registry_names.insert(name.clone()) {
            return Err(malformed(
                "top-level manifest registry",
                format!("empty or duplicate name {name:?}"),
            ));
        }
        let _value = cursor.u32(&format!("top-level manifest registry value {ordinal}"))?;
    }

    parse_asset_tail(bytes, cursor.position())
}

fn parse_asset_tail(bytes: &[u8], start: usize) -> Result<TopLevelManifest, CodecError> {
    let mut selected = None;
    for at in start..bytes.len().saturating_sub(3) {
        if bytes.get(at..at + 4) != Some(36_u32.to_le_bytes().as_slice()) {
            continue;
        }
        let Ok(candidate) = parse_asset_tail_at(bytes, at) else {
            continue;
        };
        if selected.replace(candidate).is_some() {
            return Err(malformed(
                "top-level manifest asset-folder tail",
                "more than one exact tail framing is valid",
            ));
        }
    }
    selected.ok_or_else(|| {
        malformed(
            "top-level manifest asset-folder tail",
            "no exact tail framing is valid",
        )
    })
}

fn parse_asset_tail_at(bytes: &[u8], at: usize) -> Result<TopLevelManifest, CodecError> {
    let mut cursor = Cursor::from_offset(bytes, at)?;
    let _active_asset_guid = cursor.guid("top-level manifest active-asset GUID")?;
    let asset_folder_count = bounded_nonzero_count(
        cursor.u32("top-level manifest asset-folder count")?,
        MAX_ASSET_FOLDERS,
        "top-level manifest asset-folder count",
    )?;
    let mut asset_folder_bases = Vec::with_capacity(asset_folder_count);
    let mut unique_bases = BTreeSet::new();
    for ordinal in 0..asset_folder_count {
        let base = cursor.utf16(&format!("top-level manifest asset-folder base {ordinal}"))?;
        validate_asset_base(&base)?;
        if !unique_bases.insert(base.clone()) {
            return Err(malformed(
                "top-level manifest asset-folder run",
                format!("duplicate base name {base:?}"),
            ));
        }
        asset_folder_bases.push(base);
    }
    cursor.expect_u32("top-level manifest terminal word", 0)?;
    if cursor.exhausted() {
        return Ok(TopLevelManifest { asset_folder_bases });
    }
    match cursor.u8("top-level manifest terminal byte")? {
        0 => {
            let display_name = cursor.utf16("top-level manifest terminal display name")?;
            if display_name.is_empty() {
                return Err(malformed(
                    "top-level manifest terminal display name",
                    "value is empty",
                ));
            }
            if !cursor.exhausted() {
                let lineage_urn = cursor.utf16("top-level manifest lineage URN")?;
                let lineage_urn = lineage_urn.as_bytes();
                if lineage_urn.len() <= 4
                    || !lineage_urn[..4].eq_ignore_ascii_case(b"urn:")
                    || !lineage_urn[4..].iter().all(u8::is_ascii_graphic)
                {
                    return Err(malformed(
                        "top-level manifest lineage URN",
                        "value is not a nonempty ASCII URN",
                    ));
                }
            }
        }
        1 if !cursor.exhausted() => {
            cursor.expect_utf16("top-level manifest export marker", "NA_EXPORT")?;
        }
        1 => {}
        value => {
            return Err(malformed(
                "top-level manifest terminal byte",
                format!("expected 0 or 1, found {value}"),
            ))
        }
    }
    cursor.finish("top-level manifest")?;

    Ok(TopLevelManifest { asset_folder_bases })
}

/// Resolve the unique Design archive folder through the top-level folder run
/// and each listed folder's asset-manifest header.
pub(crate) fn resolve_design_folder<'a, 'n>(
    manifest: &TopLevelManifest,
    entry_names: impl IntoIterator<Item = &'n str>,
    mut entry_bytes: impl FnMut(&str) -> Option<&'a [u8]>,
) -> Result<String, CodecError> {
    let entry_names = entry_names.into_iter().collect::<Vec<_>>();
    let mut design_folders = Vec::new();

    for base in &manifest.asset_folder_bases {
        let active = format!("{base}[Active]");
        let mut folder_matches = [base.as_str(), active.as_str()]
            .into_iter()
            .filter(|candidate| {
                entry_names.iter().any(|name| {
                    name.strip_prefix(*candidate)
                        .is_some_and(|suffix| suffix.starts_with('/'))
                })
            })
            .collect::<Vec<_>>();
        folder_matches.dedup();
        let folder = match folder_matches.as_slice() {
            [folder] => *folder,
            [] => {
                return Err(malformed(
                    "top-level manifest asset-folder run",
                    format!("listed base {base:?} has no archive folder"),
                ))
            }
            _ => {
                return Err(malformed(
                    "top-level manifest asset-folder run",
                    format!("listed base {base:?} resolves to multiple archive folders"),
                ))
            }
        };
        let manifest_name = format!("{folder}/Manifest.dat");
        let bytes = entry_bytes(&manifest_name).ok_or_else(|| {
            malformed(
                "asset manifest",
                format!("listed folder {folder:?} has no Manifest.dat"),
            )
        })?;
        let header = parse_asset_header(bytes)?;
        if header.base_name != *base {
            return Err(malformed(
                "asset manifest base name",
                format!(
                    "folder {folder:?} declares {:?}, expected {base:?}",
                    header.base_name
                ),
            ));
        }
        if header.asset_type == DESIGN_ASSET_TYPE && header.fusion_subtype.is_none() {
            design_folders.push(folder.to_owned());
        }
    }

    match design_folders.as_slice() {
        [folder] => Ok(folder.clone()),
        [] => Err(malformed(
            "top-level manifest asset-folder run",
            "no listed folder declares the Design asset",
        )),
        _ => Err(malformed(
            "top-level manifest asset-folder run",
            "more than one listed folder declares the Design asset",
        )),
    }
}

fn parse_asset_header(bytes: &[u8]) -> Result<AssetManifestHeader, CodecError> {
    let mut cursor = Cursor::new(bytes);
    let base_name = cursor.utf16("asset manifest base name")?;
    validate_asset_base(&base_name)?;
    let _primary_guid = cursor.guid("asset manifest primary GUID")?;
    let _secondary_guid = cursor.guid("asset manifest secondary GUID")?;
    let asset_type = cursor.ascii("asset manifest asset type")?;
    if !asset_type.ends_with("AssetType") {
        return Err(malformed(
            "asset manifest asset type",
            format!("invalid asset type {asset_type:?}"),
        ));
    }
    let fusion_subtype = if asset_type == DESIGN_ASSET_TYPE {
        let revision = cursor.u32("Fusion asset manifest revision")?;
        match revision {
            0 => {
                parse_revision_zero_design_asset(&mut cursor)?;
                cursor.finish("revision-0 Fusion asset manifest")?;
                None
            }
            10 => {
                parse_revision_ten_design_asset(&mut cursor)?;
                cursor.finish("revision-10 Fusion asset manifest")?;
                None
            }
            14 | 15 | 19 | 20 => parse_current_design_asset(&mut cursor)?,
            _ => {
                return Err(malformed(
                    "Fusion asset manifest revision",
                    format!("unsupported revision {revision}"),
                ))
            }
        }
    } else {
        None
    };
    Ok(AssetManifestHeader {
        base_name,
        asset_type,
        fusion_subtype,
    })
}

fn parse_capability_registry(cursor: &mut Cursor<'_>) -> Result<(), CodecError> {
    let capability_count = cursor.count(
        "Fusion asset manifest capability count",
        MAX_REGISTRY_ENTRIES,
    )?;
    let mut capability_names = BTreeSet::new();
    for ordinal in 0..capability_count {
        let name = cursor.ascii(&format!("Fusion asset manifest capability name {ordinal}"))?;
        if name.is_empty() || !capability_names.insert(name.clone()) {
            return Err(malformed(
                "Fusion asset manifest capabilities",
                format!("empty or duplicate name {name:?}"),
            ));
        }
        let _value = cursor.u32(&format!("Fusion asset manifest capability value {ordinal}"))?;
    }
    Ok(())
}

fn parse_current_design_asset(cursor: &mut Cursor<'_>) -> Result<Option<String>, CodecError> {
    parse_capability_registry(cursor)?;
    cursor.expect_ascii("Fusion asset manifest kind", "Neutron3DAssetType")?;
    cursor.expect_u8("Fusion asset manifest subtype mode", 0)?;
    let subtype = cursor.ascii("Fusion asset manifest subtype")?;
    Ok((!subtype.is_empty()).then_some(subtype))
}

fn parse_revision_zero_design_asset(cursor: &mut Cursor<'_>) -> Result<(), CodecError> {
    cursor.expect_u32("revision-0 Fusion asset schema", 3)?;
    cursor.expect_u32("revision-0 Fusion asset kind count", 1)?;
    cursor.expect_ascii("revision-0 Fusion asset kind", "Neutron3DAssetType")?;
    cursor.expect_u8("revision-0 Fusion asset subtype mode", 0)?;
    cursor.expect_u32("revision-0 Fusion asset subtype", 0)?;
    cursor.expect_u32("revision-0 Fusion asset schema revision", 6)?;
    cursor.expect_u32("revision-0 Fusion asset root marker", 1)?;
    cursor.expect_u32("revision-0 Fusion asset reserved word", 0)?;
    cursor.expect_ascii("revision-0 Fusion asset role", "Design")?;
    cursor.expect_ascii("revision-0 Fusion asset role name", "Design")?;
    Ok(())
}

fn parse_revision_ten_design_asset(cursor: &mut Cursor<'_>) -> Result<(), CodecError> {
    parse_capability_registry(cursor)?;
    cursor.expect_ascii("revision-10 Fusion asset kind", "Neutron3DAssetType")?;
    cursor.expect_u8("revision-10 Fusion asset subtype mode", 0)?;
    let mut link_count = 0_usize;
    loop {
        cursor.expect_u32("revision-10 Fusion asset entry marker", 2)?;
        let locator_units = cursor.count(
            "revision-10 Fusion asset locator length or root revision",
            MAX_MANIFEST_STRING_UNITS,
        )?;
        if locator_units == 5 {
            break;
        }
        if link_count == MAX_REGISTRY_ENTRIES {
            return Err(malformed(
                "revision-10 Fusion asset links",
                format!("count exceeds the limit {MAX_REGISTRY_ENTRIES}"),
            ));
        }
        let locator = cursor.utf16_with_count(
            locator_units,
            &format!("revision-10 Fusion asset link {link_count} locator"),
        )?;
        if !locator.contains("urn:") {
            return Err(malformed(
                "revision-10 Fusion asset link locator",
                format!("expected an embedded URN, found {locator:?}"),
            ));
        }
        let _first_guid = cursor.guid(&format!(
            "revision-10 Fusion asset link {link_count} GUID 1"
        ))?;
        let _second_guid = cursor.guid(&format!(
            "revision-10 Fusion asset link {link_count} GUID 2"
        ))?;
        link_count += 1;
    }
    cursor.expect_u32("revision-10 Fusion asset root marker", 1)?;
    cursor.expect_u32("revision-10 Fusion asset reserved word", 0)?;
    cursor.expect_ascii("revision-10 Fusion asset role", "Design")?;
    cursor.expect_ascii("revision-10 Fusion asset role name", "Design")?;
    Ok(())
}

/// Encode the current top-level manifest form for a counted asset-folder run.
pub(crate) fn encode_top_level(
    active_asset_guid: &str,
    asset_folder_bases: &[&str],
) -> Result<Vec<u8>, CodecError> {
    validate_guid(active_asset_guid, "top-level manifest active-asset GUID")?;
    if asset_folder_bases.is_empty() || asset_folder_bases.len() > MAX_ASSET_FOLDERS {
        return Err(malformed(
            "top-level manifest asset-folder count",
            format!("invalid count {}", asset_folder_bases.len()),
        ));
    }
    let mut unique = BTreeSet::new();
    for base in asset_folder_bases {
        validate_asset_base(base)?;
        if !unique.insert(*base) {
            return Err(malformed(
                "top-level manifest asset-folder run",
                format!("duplicate base name {base:?}"),
            ));
        }
    }

    let mut out = Vec::new();
    push_ascii(&mut out, "3-2-0-0")?;
    push_ascii(&mut out, "FusionDocType")?;
    push_utf16(&mut out, ".f3d")?;
    push_utf16(&mut out, "Fusion Document")?;
    push_utf16(&mut out, "A Fusion Document")?;
    push_utf16(&mut out, GENERATED_DOCUMENT_GUID)?;
    push_utf16(&mut out, GENERATED_DOCUMENT_ASSET_GUID)?;
    push_u32(&mut out, 1234);
    push_u32(&mut out, 20);
    push_u32(&mut out, 36);
    push_u32(&mut out, 0x2a40_0040);
    let registry = [
        ("Application", 1),
        ("CAM", 4),
        ("ParaMesh", 8),
        ("SimCommon", 30_005),
        ("SimFEACSObjects", 2),
        ("SimFluidDynamics", 2),
        ("SimStructuralAttributes", 10_002),
    ];
    push_count(
        &mut out,
        registry.len(),
        "top-level manifest registry count",
    )?;
    for (name, value) in registry {
        push_ascii(&mut out, name)?;
        push_u32(&mut out, value);
    }
    push_u32(&mut out, 0);
    out.push(0);
    push_utf16(&mut out, active_asset_guid)?;
    push_count(
        &mut out,
        asset_folder_bases.len(),
        "top-level manifest asset-folder count",
    )?;
    for base in asset_folder_bases {
        push_utf16(&mut out, base)?;
    }
    push_u32(&mut out, 0);
    out.push(1);
    push_utf16(&mut out, "NA_EXPORT")?;
    Ok(out)
}

/// Encode a complete current-generation Design asset manifest.
pub(crate) fn encode_design_asset(
    base_name: &str,
    primary_guid: &str,
) -> Result<Vec<u8>, CodecError> {
    let mut out = encode_asset_header(
        base_name,
        primary_guid,
        GENERATED_ASSET_GUID,
        DESIGN_ASSET_TYPE,
    )?;
    push_u32(&mut out, 20);
    let capabilities = [
        ("Application", 139),
        ("ParaMesh", 13),
        ("Server", 36),
        ("VolField", 4),
    ];
    push_count(&mut out, capabilities.len(), "asset capability count")?;
    for (name, value) in capabilities {
        push_ascii(&mut out, name)?;
        push_u32(&mut out, value);
    }
    push_ascii(&mut out, "Neutron3DAssetType")?;
    out.push(0);
    push_ascii(&mut out, "")?;
    push_u32(&mut out, 1);
    push_ascii(&mut out, "physicalChangeGuid")?;
    push_utf16(&mut out, GENERATED_PHYSICAL_CHANGE_GUID)?;
    push_u32(&mut out, 0);
    push_u32(&mut out, 7);
    let segment_types = [
        "FusionDesignSegmentType",
        "FusionACTSegmentType",
        "FusionBrowserSegmentType",
    ];
    push_count(&mut out, segment_types.len(), "asset segment-type count")?;
    for (ordinal, name) in segment_types.into_iter().enumerate() {
        push_count(&mut out, ordinal, "asset segment-type ordinal")?;
        push_ascii(&mut out, name)?;
        push_ascii(&mut out, name)?;
    }
    Ok(out)
}

/// Encode the framed prefix shared by every per-asset manifest.
pub(crate) fn encode_asset_header(
    base_name: &str,
    primary_guid: &str,
    secondary_guid: &str,
    asset_type: &str,
) -> Result<Vec<u8>, CodecError> {
    validate_asset_base(base_name)?;
    validate_guid(primary_guid, "asset manifest primary GUID")?;
    validate_guid(secondary_guid, "asset manifest secondary GUID")?;
    if !asset_type.ends_with("AssetType") {
        return Err(malformed(
            "asset manifest asset type",
            format!("invalid asset type {asset_type:?}"),
        ));
    }
    let mut out = Vec::new();
    push_utf16(&mut out, base_name)?;
    push_utf16(&mut out, primary_guid)?;
    push_utf16(&mut out, secondary_guid)?;
    push_ascii(&mut out, asset_type)?;
    Ok(out)
}

pub(crate) fn generated_top_level() -> Result<Vec<u8>, CodecError> {
    encode_top_level(GENERATED_ASSET_FOLDER_GUID, &[GENERATED_DESIGN_ASSET_BASE])
}

pub(crate) fn generated_design_asset() -> Result<Vec<u8>, CodecError> {
    encode_design_asset(GENERATED_DESIGN_ASSET_BASE, GENERATED_ASSET_FOLDER_GUID)
}

fn validate_guid(value: &str, field: &str) -> Result<(), CodecError> {
    if !is_guid_hyphenated(value) {
        return Err(malformed(field, format!("invalid GUID {value:?}")));
    }
    Ok(())
}

fn validate_asset_base(value: &str) -> Result<(), CodecError> {
    if value.is_empty()
        || matches!(value, "." | "..")
        || value.chars().any(|ch| matches!(ch, '/' | '\\' | '\0'))
    {
        return Err(malformed(
            "asset-folder base name",
            format!("invalid path component {value:?}"),
        ));
    }
    Ok(())
}

fn bounded_nonzero_count(value: u32, max: usize, field: &str) -> Result<usize, CodecError> {
    let count = bounded_count(value, max, field)?;
    if count == 0 {
        return Err(malformed(
            field,
            format!("count {count} is outside 1..={max}"),
        ));
    }
    Ok(count)
}

fn bounded_count(value: u32, max: usize, field: &str) -> Result<usize, CodecError> {
    let count =
        usize::try_from(value).map_err(|_| malformed(field, "count does not fit memory"))?;
    if count > max {
        return Err(malformed(
            field,
            format!("count {count} exceeds the limit {max}"),
        ));
    }
    Ok(count)
}

fn push_ascii(out: &mut Vec<u8>, value: &str) -> Result<(), CodecError> {
    if value.len() > MAX_MANIFEST_STRING_UNITS
        || !value.bytes().all(|byte| matches!(byte, 0x20..=0x7e))
    {
        return Err(malformed(
            "manifest ASCII string",
            format!("invalid value {value:?}"),
        ));
    }
    push_count(out, value.len(), "manifest ASCII string length")?;
    out.extend_from_slice(value.as_bytes());
    Ok(())
}

fn push_utf16(out: &mut Vec<u8>, value: &str) -> Result<(), CodecError> {
    let units = value.encode_utf16().collect::<Vec<_>>();
    if units.len() > MAX_MANIFEST_STRING_UNITS {
        return Err(malformed(
            "manifest UTF-16 string",
            format!("{} code units exceed the limit", units.len()),
        ));
    }
    push_count(out, units.len(), "manifest UTF-16 string length")?;
    for unit in units {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    Ok(())
}

fn push_count(out: &mut Vec<u8>, value: usize, field: &str) -> Result<(), CodecError> {
    let value = u32::try_from(value).map_err(|_| malformed(field, "value does not fit u32"))?;
    push_u32(out, value);
    Ok(())
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn malformed(field: &str, message: impl std::fmt::Display) -> CodecError {
    crate::error::malformed(format!("F3D {field}: {message}"))
}

fn truncated(field: &str) -> CodecError {
    malformed(field, "truncated")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    const DESIGN_GUID: &str = "10000000-0000-4000-8000-000000000001";
    const OTHER_GUID: &str = "20000000-0000-4000-8000-000000000002";
    const SECONDARY_GUID: &str = "30000000-0000-4000-8000-000000000003";

    #[test]
    fn current_top_level_manifest_round_trips() {
        let bytes = encode_top_level(DESIGN_GUID, &["Design Base", "Simulation"]).unwrap();
        let manifest = parse_top_level(&bytes).unwrap();
        assert_eq!(manifest.asset_folder_bases, ["Design Base", "Simulation"]);
    }

    #[test]
    fn legacy_top_level_manifest_accepts_both_terminal_forms() {
        let mut bytes = Vec::new();
        push_ascii(&mut bytes, "3-2-0-0").unwrap();
        push_ascii(&mut bytes, "FusionDocType").unwrap();
        push_utf16(&mut bytes, ".f3d").unwrap();
        push_utf16(&mut bytes, "Fusion Document").unwrap();
        push_utf16(&mut bytes, "A Fusion Document").unwrap();
        push_utf16(&mut bytes, GENERATED_DOCUMENT_GUID).unwrap();
        push_utf16(&mut bytes, GENERATED_DOCUMENT_ASSET_GUID).unwrap();
        push_u32(&mut bytes, 0x0800_06e1);
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 1);
        push_utf16(&mut bytes, OTHER_GUID).unwrap();
        push_utf16(&mut bytes, DESIGN_GUID).unwrap();
        push_u32(&mut bytes, 2);
        push_utf16(&mut bytes, "Simulation").unwrap();
        push_utf16(&mut bytes, "Design Base").unwrap();
        push_u32(&mut bytes, 0);

        let word_only = parse_top_level(&bytes).unwrap();
        assert_eq!(word_only.asset_folder_bases, ["Simulation", "Design Base"]);

        bytes.push(0);
        push_utf16(&mut bytes, "Legacy Document").unwrap();

        let manifest = parse_top_level(&bytes).unwrap();
        assert_eq!(manifest.asset_folder_bases, ["Simulation", "Design Base"]);

        push_utf16(&mut bytes, "urn:synthetic:lineage").unwrap();
        let with_lineage = parse_top_level(&bytes).unwrap();
        assert_eq!(
            with_lineage.asset_folder_bases,
            ["Simulation", "Design Base"]
        );
    }

    #[test]
    fn top_level_manifest_accepts_a_terminal_export_flag_without_a_marker() {
        let mut bytes = encode_top_level(DESIGN_GUID, &["Design Base"]).unwrap();
        let mut marker = Vec::new();
        push_utf16(&mut marker, "NA_EXPORT").unwrap();
        assert!(bytes.ends_with(&marker));
        bytes.truncate(bytes.len() - marker.len());

        let manifest = parse_top_level(&bytes).unwrap();
        assert_eq!(manifest.asset_folder_bases, ["Design Base"]);
    }

    #[test]
    fn design_folder_uses_root_fusion_asset_not_active_guid_or_run_order() {
        let manifest =
            parse_top_level(&encode_top_level(OTHER_GUID, &["Simulation", "Design Base"]).unwrap())
                .unwrap();
        let mut entries = BTreeMap::new();
        entries.insert(
            "Simulation/Manifest.dat".to_string(),
            encode_fusion_subtype_asset("Simulation", OTHER_GUID, "Simulation").unwrap(),
        );
        entries.insert("Simulation/Breps.BlobParts/decoy.smbh".to_string(), vec![0]);
        entries.insert(
            "Design Base[Active]/Manifest.dat".to_string(),
            encode_design_asset("Design Base", DESIGN_GUID).unwrap(),
        );
        entries.insert(
            "Design Base[Active]/Design1/BulkStream.dat".to_string(),
            vec![1],
        );

        let folder = resolve_design_folder(&manifest, entries.keys().map(String::as_str), |name| {
            entries.get(name).map(Vec::as_slice)
        })
        .unwrap();
        assert_eq!(folder, "Design Base[Active]");
    }

    #[test]
    fn active_guid_can_be_shared_by_a_non_design_asset() {
        let manifest = parse_top_level(
            &encode_top_level(DESIGN_GUID, &["Simulation", "Design Base"]).unwrap(),
        )
        .unwrap();
        let entries = BTreeMap::from([
            (
                "Simulation/Manifest.dat".to_string(),
                encode_asset_header(
                    "Simulation",
                    DESIGN_GUID,
                    SECONDARY_GUID,
                    "SimulationAssetType",
                )
                .unwrap(),
            ),
            (
                "Design Base/Manifest.dat".to_string(),
                encode_design_asset("Design Base", DESIGN_GUID).unwrap(),
            ),
            ("Design Base/Design1/BulkStream.dat".to_string(), vec![1]),
        ]);
        let folder = resolve_design_folder(&manifest, entries.keys().map(String::as_str), |name| {
            entries.get(name).map(Vec::as_slice)
        })
        .unwrap();
        assert_eq!(folder, "Design Base");
    }

    #[test]
    fn top_level_manifest_rejects_trailing_bytes() {
        let mut bytes = encode_top_level(DESIGN_GUID, &["Design Base"]).unwrap();
        bytes.push(0);
        assert!(parse_top_level(&bytes).is_err());
    }

    #[test]
    fn generated_asset_manifest_has_a_joinable_header() {
        let bytes = generated_design_asset().unwrap();
        let header = parse_asset_header(&bytes).unwrap();
        assert_eq!(header.base_name, GENERATED_DESIGN_ASSET_BASE);
        assert_eq!(header.asset_type, DESIGN_ASSET_TYPE);
        assert_eq!(header.fusion_subtype, None);
    }

    #[test]
    fn revision_zero_design_asset_has_no_named_capability_registry() {
        let mut bytes = encode_asset_header(
            "Legacy Design",
            DESIGN_GUID,
            SECONDARY_GUID,
            DESIGN_ASSET_TYPE,
        )
        .unwrap();
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 3);
        push_u32(&mut bytes, 1);
        push_ascii(&mut bytes, "Neutron3DAssetType").unwrap();
        bytes.push(0);
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 6);
        push_u32(&mut bytes, 1);
        push_u32(&mut bytes, 0);
        push_ascii(&mut bytes, "Design").unwrap();
        push_ascii(&mut bytes, "Design").unwrap();

        let header = parse_asset_header(&bytes).unwrap();
        assert_eq!(header.base_name, "Legacy Design");
        assert_eq!(header.fusion_subtype, None);
    }

    #[test]
    fn revision_ten_design_asset_carries_linked_document_triples() {
        let mut bytes = encode_asset_header(
            "Linked Design",
            DESIGN_GUID,
            SECONDARY_GUID,
            DESIGN_ASSET_TYPE,
        )
        .unwrap();
        push_u32(&mut bytes, 10);
        push_u32(&mut bytes, 1);
        push_ascii(&mut bytes, "Application").unwrap();
        push_u32(&mut bytes, 52);
        push_ascii(&mut bytes, "Neutron3DAssetType").unwrap();
        bytes.push(0);
        push_u32(&mut bytes, 2);
        push_utf16(&mut bytes, "synthetic_urn:synthetic:version:1").unwrap();
        push_utf16(&mut bytes, DESIGN_GUID).unwrap();
        push_utf16(&mut bytes, OTHER_GUID).unwrap();
        push_u32(&mut bytes, 2);
        push_u32(&mut bytes, 5);
        push_u32(&mut bytes, 1);
        push_u32(&mut bytes, 0);
        push_ascii(&mut bytes, "Design").unwrap();
        push_ascii(&mut bytes, "Design").unwrap();

        let header = parse_asset_header(&bytes).unwrap();
        assert_eq!(header.base_name, "Linked Design");
        assert_eq!(header.fusion_subtype, None);
    }

    #[test]
    fn revision_fourteen_uses_the_ascii_subtype_header() {
        let mut bytes =
            encode_asset_header("Design 14", DESIGN_GUID, SECONDARY_GUID, DESIGN_ASSET_TYPE)
                .unwrap();
        push_u32(&mut bytes, 14);
        push_u32(&mut bytes, 0);
        push_ascii(&mut bytes, "Neutron3DAssetType").unwrap();
        bytes.push(0);
        push_ascii(&mut bytes, "").unwrap();

        let header = parse_asset_header(&bytes).unwrap();
        assert_eq!(header.base_name, "Design 14");
        assert_eq!(header.fusion_subtype, None);
    }

    #[test]
    fn revisions_fifteen_and_nineteen_use_the_current_asset_header() {
        for revision in [15, 19] {
            let mut bytes = encode_asset_header(
                "Intermediate Design",
                DESIGN_GUID,
                SECONDARY_GUID,
                DESIGN_ASSET_TYPE,
            )
            .unwrap();
            push_u32(&mut bytes, revision);
            push_u32(&mut bytes, 1);
            push_ascii(&mut bytes, "Application").unwrap();
            push_u32(&mut bytes, 139);
            push_ascii(&mut bytes, "Neutron3DAssetType").unwrap();
            bytes.push(0);
            push_ascii(&mut bytes, "").unwrap();

            let header = parse_asset_header(&bytes).unwrap();
            assert_eq!(header.base_name, "Intermediate Design");
            assert_eq!(header.fusion_subtype, None);
        }
    }

    fn encode_fusion_subtype_asset(
        base_name: &str,
        primary_guid: &str,
        subtype: &str,
    ) -> Result<Vec<u8>, CodecError> {
        let mut bytes =
            encode_asset_header(base_name, primary_guid, SECONDARY_GUID, DESIGN_ASSET_TYPE)?;
        push_u32(&mut bytes, 20);
        push_u32(&mut bytes, 0);
        push_ascii(&mut bytes, "Neutron3DAssetType")?;
        bytes.push(0);
        push_ascii(&mut bytes, subtype)?;
        Ok(bytes)
    }
}
