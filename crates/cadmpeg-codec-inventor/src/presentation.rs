// SPDX-License-Identifier: Apache-2.0
//! Typed `PmApp` document-default and rendering-style records.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::BodyId;

use crate::rse::{RecordFrameState, RseInventory, SegmentBulkState, SegmentKind};

const DEFAULT_STYLE_TYPE: [u8; 16] = [
    0xcd, 0xec, 0xfb, 0x11, 0xd1, 0x11, 0x6b, 0x25, 0x00, 0x08, 0xeb, 0xbb, 0x21, 0xed, 0xdc, 0x09,
];
const RENDERING_STYLE_TYPE: [u8; 16] = [
    0x6f, 0xd8, 0x59, 0x67, 0xd2, 0x11, 0x38, 0x78, 0x60, 0x00, 0x94, 0xb7, 0x0b, 0x02, 0xec, 0xb0,
];

#[derive(Debug)]
pub(crate) struct PresentationInventory<'a> {
    pub(crate) default_styles: Vec<PmAppDefaultStyle<'a>>,
    pub(crate) rendering_styles: Vec<PmAppRenderingStyle<'a>>,
    pub(crate) issues: Vec<PresentationRecordIssue>,
}

#[derive(Debug)]
pub(crate) struct PmAppDefaultStyle<'a> {
    pub(crate) segment_token: String,
    pub(crate) record_ordinal: u32,
    pub(crate) segment_version_major: u8,
    pub(crate) header_value: u32,
    pub(crate) header_id: u16,
    pub(crate) material_reference: u32,
    pub(crate) rendering_style_reference: u32,
    pub(crate) related_references: [u32; 7],
    pub(crate) state: u8,
    pub(crate) terminal_reference: u32,
    pub(crate) suffix: View<'a>,
}

#[derive(Debug)]
pub(crate) struct PmAppRenderingStyle<'a> {
    pub(crate) segment_token: String,
    pub(crate) record_ordinal: u32,
    pub(crate) segment_version_major: u8,
    pub(crate) header_value: u32,
    pub(crate) header_id: u16,
    pub(crate) state: u8,
    pub(crate) flags: u16,
    pub(crate) values: [u16; 2],
    pub(crate) default_state: u32,
    pub(crate) value: u32,
    pub(crate) name_reference: u32,
    pub(crate) name: String,
    pub(crate) comment: String,
    pub(crate) long_name: String,
    pub(crate) style_state: Option<u16>,
    pub(crate) style_label: Option<String>,
    pub(crate) asset_guid: Option<String>,
    pub(crate) material_id: Option<String>,
    pub(crate) asset_library_id: Option<String>,
    pub(crate) style_values: Option<[u16; 2]>,
    pub(crate) guid: Option<String>,
    pub(crate) suffix: View<'a>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PresentationRecordIssue {
    pub(crate) segment_token: String,
    pub(crate) record_ordinal: u32,
    pub(crate) detail: String,
}

pub(crate) struct PresentationProjection {
    pub(crate) bindings: Vec<AppearanceBinding>,
    pub(crate) unresolved_defaults: usize,
}

pub(crate) fn project_default_bindings(
    inventory: &PresentationInventory<'_>,
    appearances: &[Appearance],
    bodies: &[BodyId],
) -> PresentationProjection {
    if inventory.default_styles.len() != 1 {
        return PresentationProjection {
            bindings: Vec::new(),
            unresolved_defaults: usize::from(!inventory.default_styles.is_empty()),
        };
    }
    let mut selected = Vec::new();
    for default in &inventory.default_styles {
        let Some(ordinal) = default.rendering_style_reference.checked_sub(1) else {
            continue;
        };
        let matches = inventory
            .rendering_styles
            .iter()
            .filter(|style| {
                style.segment_token == default.segment_token && style.record_ordinal == ordinal
            })
            .collect::<Vec<_>>();
        if matches.len() == 1 {
            selected.push(matches[0]);
        }
    }
    selected.sort_by(|left, right| {
        left.segment_token
            .cmp(&right.segment_token)
            .then_with(|| left.record_ordinal.cmp(&right.record_ordinal))
    });
    selected.dedup_by(|left, right| {
        left.segment_token == right.segment_token && left.record_ordinal == right.record_ordinal
    });
    if selected.len() != 1 {
        return PresentationProjection {
            bindings: Vec::new(),
            unresolved_defaults: usize::from(!inventory.default_styles.is_empty()),
        };
    }
    let style = selected[0];
    let Some(asset_guid) = style
        .asset_guid
        .as_deref()
        .filter(|value| !value.is_empty())
    else {
        return PresentationProjection {
            bindings: Vec::new(),
            unresolved_defaults: 1,
        };
    };
    let matches = appearances
        .iter()
        .filter(|appearance| {
            appearance
                .asset_guid
                .as_deref()
                .is_some_and(|value| value.eq_ignore_ascii_case(asset_guid))
        })
        .filter(|appearance| match style.asset_library_id.as_deref() {
            Some(value) if !value.is_empty() => appearance
                .library_id
                .as_deref()
                .is_some_and(|library| library.eq_ignore_ascii_case(value)),
            _ => true,
        })
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return PresentationProjection {
            bindings: Vec::new(),
            unresolved_defaults: 1,
        };
    }
    let appearance = &matches[0].id;
    let bindings = bodies
        .iter()
        .map(|body| AppearanceBinding {
            id: format!(
                "inventor:presentation:body-default#{}",
                &sha256_hex(body.0.as_bytes())[..16]
            ),
            target: AppearanceTarget::Body(body.clone()),
            appearance: appearance.clone(),
            source_entity_id: Some(format!(
                "inventor:presentation:rendering-style#{}-{}",
                style.segment_token, style.record_ordinal
            )),
            object_type: Some("Body".into()),
            channels: BTreeMap::default(),
        })
        .collect();
    PresentationProjection {
        bindings,
        unresolved_defaults: 0,
    }
}

pub(crate) fn inventory<'a>(
    ctx: &DecodeContext<'a>,
    document: &RseInventory<'a>,
) -> Result<PresentationInventory<'a>, CodecError> {
    let mut default_styles = Vec::new();
    let mut rendering_styles = Vec::new();
    let mut issues = Vec::new();
    for segment in &document.segments {
        if segment.kind != SegmentKind::PmApp {
            continue;
        }
        let Some(version) = segment.registry_version_major else {
            continue;
        };
        let SegmentBulkState::Framed(bulk) = &segment.bulk else {
            continue;
        };
        let RecordFrameState::Framed(table) = &bulk.records else {
            continue;
        };
        for record in &table.records {
            let parsed = match record.type_id {
                DEFAULT_STYLE_TYPE => {
                    parse_default_style(ctx, record.payload, version).map(|mut value| {
                        value.segment_token = segment.pair.token.as_str().into();
                        value.record_ordinal = record.ordinal;
                        default_styles.push(value);
                    })
                }
                RENDERING_STYLE_TYPE => {
                    parse_rendering_style(ctx, record.payload, version).map(|mut value| {
                        value.segment_token = segment.pair.token.as_str().into();
                        value.record_ordinal = record.ordinal;
                        rendering_styles.push(value);
                    })
                }
                _ => continue,
            };
            if let Err(error) = parsed {
                issues.push(PresentationRecordIssue {
                    segment_token: segment.pair.token.as_str().into(),
                    record_ordinal: record.ordinal,
                    detail: error.to_string(),
                });
            }
        }
    }
    ctx.charge_collection_items(
        default_styles.len() as u64 + rendering_styles.len() as u64 + issues.len() as u64,
        "admit Inventor presentation records",
    )?;
    Ok(PresentationInventory {
        default_styles,
        rendering_styles,
        issues,
    })
}

fn parse_default_style<'a>(
    _ctx: &DecodeContext<'a>,
    source: View<'a>,
    version: u8,
) -> Result<PmAppDefaultStyle<'a>, CodecError> {
    let mut cursor = Cursor::new(source);
    let header_value = cursor.u32("default-style header value")?;
    let header_id = cursor.u16("default-style header id")?;
    cursor.skip(
        legacy_block_len(version),
        "default-style legacy header padding",
    )?;
    let material_reference = cursor.reference("default-style material reference")?;
    let rendering_style_reference = cursor.reference("default-style rendering reference")?;
    let mut related_references = [0; 7];
    for (index, reference) in related_references.iter_mut().enumerate() {
        *reference = cursor.reference(&format!("default-style related reference {index}"))?;
    }
    let state = cursor.u8("default-style state")?;
    cursor.skip(
        legacy_block_len(version),
        "default-style legacy state padding",
    )?;
    if version == 15 {
        cursor.skip(4, "default-style version-15 padding")?;
    }
    let terminal_reference = cursor.reference("default-style terminal reference")?;
    if version > 19 {
        cursor.zeroes(8, "default-style suffix padding")?;
    }
    let suffix = cursor.remainder()?;
    if !suffix.window().is_empty() {
        return Err(CodecError::Malformed(format!(
            "PmApp default-style record has {} trailing bytes",
            suffix.window().len()
        )));
    }
    Ok(PmAppDefaultStyle {
        segment_token: String::new(),
        record_ordinal: 0,
        segment_version_major: version,
        header_value,
        header_id,
        material_reference,
        rendering_style_reference,
        related_references,
        state,
        terminal_reference,
        suffix,
    })
}

fn parse_rendering_style<'a>(
    ctx: &DecodeContext<'a>,
    source: View<'a>,
    version: u8,
) -> Result<PmAppRenderingStyle<'a>, CodecError> {
    let mut cursor = Cursor::new(source);
    let header_value = cursor.u32("rendering-style header value")?;
    let header_id = cursor.u16("rendering-style header id")?;
    cursor.skip(
        legacy_block_len(version),
        "rendering-style legacy header padding",
    )?;
    let state = cursor.u8("rendering-style state")?;
    let flags = cursor.u16("rendering-style flags")?;
    if version > 23 {
        cursor.zeroes(2, "rendering-style alignment padding")?;
    }
    let values = [
        cursor.u16("rendering-style value 0")?,
        cursor.u16("rendering-style value 1")?,
    ];
    let default_state = cursor.u32("rendering-style default state")?;
    let value = cursor.u32("rendering-style value")?;
    let name_reference = cursor.reference("rendering-style name reference")?;
    let name = cursor.utf16(ctx, "rendering-style name")?;
    let comment = if version < 17 {
        let value = cursor.utf16(ctx, "rendering-style comment")?;
        let _comment_state = cursor.u16("rendering-style comment state")?;
        value
    } else {
        String::new()
    };
    let long_name = cursor.utf16(ctx, "rendering-style long name")?;
    cursor.skip(
        legacy_block_len(version),
        "rendering-style legacy name padding",
    )?;
    let (style_state, style_fields, style_values, guid) = if version > 16 {
        let style_state = cursor.u16("rendering-style style state")?;
        let mut text_values = Vec::with_capacity(4);
        for index in 0..4 {
            text_values.push(cursor.utf16(ctx, &format!("rendering-style text {index}"))?);
        }
        let style_values = [
            cursor.u16("rendering-style style value 0")?,
            cursor.u16("rendering-style style value 1")?,
        ];
        let guid = cursor.guid("rendering-style guid")?;
        (
            Some(style_state),
            text_values,
            Some(style_values),
            Some(guid),
        )
    } else {
        (None, Vec::new(), None, None)
    };
    let [style_label, asset_guid, material_id, asset_library_id] = if style_fields.is_empty() {
        [None, None, None, None]
    } else {
        let mut fields = style_fields.into_iter();
        [fields.next(), fields.next(), fields.next(), fields.next()]
    };
    let suffix = cursor.remainder()?;
    Ok(PmAppRenderingStyle {
        segment_token: String::new(),
        record_ordinal: 0,
        segment_version_major: version,
        header_value,
        header_id,
        state,
        flags,
        values,
        default_state,
        value,
        name_reference,
        name,
        comment,
        long_name,
        style_state,
        style_label,
        asset_guid,
        material_id,
        asset_library_id,
        style_values,
        guid,
        suffix,
    })
}

const fn legacy_block_len(version: u8) -> usize {
    if version <= 14 {
        4
    } else {
        0
    }
}

struct Cursor<'a> {
    source: View<'a>,
    position: usize,
}

impl<'a> Cursor<'a> {
    const fn new(source: View<'a>) -> Self {
        Self {
            source,
            position: 0,
        }
    }

    fn take(&mut self, len: usize, field: &str) -> Result<&'a [u8], CodecError> {
        let end = self
            .position
            .checked_add(len)
            .ok_or_else(|| CodecError::Malformed(format!("PmApp {field} range overflows")))?;
        let bytes = self
            .source
            .window()
            .get(self.position..end)
            .ok_or_else(|| CodecError::Malformed(format!("truncated PmApp {field}")))?;
        self.position = end;
        Ok(bytes)
    }

    fn skip(&mut self, len: usize, field: &str) -> Result<(), CodecError> {
        self.take(len, field).map(|_| ())
    }

    fn zeroes(&mut self, len: usize, field: &str) -> Result<(), CodecError> {
        if self.take(len, field)?.iter().any(|byte| *byte != 0) {
            return Err(CodecError::Malformed(format!(
                "PmApp {field} is not zero-filled"
            )));
        }
        Ok(())
    }

    fn u8(&mut self, field: &str) -> Result<u8, CodecError> {
        Ok(self.take(1, field)?[0])
    }

    fn u16(&mut self, field: &str) -> Result<u16, CodecError> {
        Ok(u16::from_le_bytes(
            self.take(2, field)?.try_into().expect("two-byte read"),
        ))
    }

    fn u32(&mut self, field: &str) -> Result<u32, CodecError> {
        Ok(u32::from_le_bytes(
            self.take(4, field)?.try_into().expect("four-byte read"),
        ))
    }

    fn reference(&mut self, field: &str) -> Result<u32, CodecError> {
        let value = self.u32(field)?;
        if value != 0 && value & 0x8000_0000 == 0 {
            return Err(CodecError::Malformed(format!(
                "PmApp {field} lacks its reference qualifier"
            )));
        }
        Ok(value & 0x7fff_ffff)
    }

    fn utf16(&mut self, ctx: &DecodeContext<'_>, field: &str) -> Result<String, CodecError> {
        let units = self.u32(&format!("{field} length"))? as usize;
        if units > 1_048_576 {
            return Err(CodecError::Malformed(format!(
                "PmApp {field} exceeds 1048576 UTF-16 code units"
            )));
        }
        let byte_len = units
            .checked_mul(2)
            .ok_or_else(|| CodecError::Malformed(format!("PmApp {field} byte length overflows")))?;
        ctx.charge_retained(byte_len as u64, "retain Inventor PmApp UTF-16 string", None)?;
        let bytes = self.take(byte_len, field)?;
        let units = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        String::from_utf16(&units)
            .map(|value| value.trim_end_matches('\0').to_owned())
            .map_err(|_| CodecError::Malformed(format!("PmApp {field} is invalid UTF-16")))
    }

    fn guid(&mut self, field: &str) -> Result<String, CodecError> {
        let bytes: [u8; 16] = self.take(16, field)?.try_into().expect("16-byte read");
        Ok(format!(
            "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{}",
            u32::from_le_bytes(bytes[0..4].try_into().expect("four-byte group")),
            u16::from_le_bytes(bytes[4..6].try_into().expect("two-byte group")),
            u16::from_le_bytes(bytes[6..8].try_into().expect("two-byte group")),
            bytes[8],
            bytes[9],
            hex(&bytes[10..])
        ))
    }

    fn remainder(&mut self) -> Result<View<'a>, CodecError> {
        let start = self.source.start() + self.position;
        self.position = self.source.window().len();
        self.source
            .child(start, self.source.end())
            .ok_or_else(|| CodecError::Malformed("PmApp suffix range is invalid".into()))
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut value, byte| {
            write!(value, "{byte:02x}").expect("writing to String cannot fail");
            value
        })
}

pub(crate) fn suffix_fields(source: View<'_>) -> (u64, String) {
    (source.window().len() as u64, sha256_hex(source.window()))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
    use cadmpeg_ir::appearance::AppearanceTarget;
    use cadmpeg_ir::ids::AppearanceId;

    use super::*;

    #[test]
    fn parses_current_default_style_and_one_based_rendering_reference() {
        let bytes = default_style_fixture();
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic default style fits policy");

        let style = parse_default_style(&ctx, root, 26).expect("default style parses");

        assert_eq!(style.material_reference, 8);
        assert_eq!(style.rendering_style_reference, 9);
        assert_eq!(style.related_references, [10, 11, 12, 13, 14, 15, 16]);
        assert_eq!(style.terminal_reference, 17);
        assert!(style.suffix.window().is_empty());
    }

    #[test]
    fn rejects_nonzero_unqualified_record_reference() {
        let mut bytes = default_style_fixture();
        bytes[10..14].copy_from_slice(&9_u32.to_le_bytes());
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic default style fits policy");

        let error = parse_default_style(&ctx, root, 26).expect_err("reference must be qualified");

        assert!(error.to_string().contains("lacks its reference qualifier"));
    }

    #[test]
    fn parses_current_rendering_style_asset_identity_and_retains_suffix() {
        let bytes = rendering_style_fixture();
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic rendering style fits policy");

        let style = parse_rendering_style(&ctx, root, 26).expect("rendering style parses");

        assert_eq!(style.name, "Steel");
        assert_eq!(style.style_label.as_deref(), Some("1:Steel"));
        assert_eq!(
            style.asset_guid.as_deref(),
            Some("d3c6130d-6c0f-4525-b268-53517ab46a78")
        );
        assert_eq!(style.material_id.as_deref(), Some("InvGen-066"));
        assert_eq!(
            style.asset_library_id.as_deref(),
            Some("afefc330-5e61-4e24-814f-ae810148b79d")
        );
        assert_eq!(style.suffix.window(), &[0xaa, 0x55]);
    }

    #[test]
    fn projects_only_the_exact_default_style_asset_join() {
        let default_bytes = default_style_fixture();
        let style_bytes = rendering_style_fixture();
        let arena = DecodeArena::new();
        let (ctx, default_root) =
            DecodeContext::from_root_bytes(&default_bytes, &arena, &DecodePolicy::default())
                .expect("synthetic default style fits policy");
        let (_, style_root) =
            DecodeContext::from_root_bytes(&style_bytes, &arena, &DecodePolicy::default())
                .expect("synthetic rendering style fits policy");
        let mut default = parse_default_style(&ctx, default_root, 26).expect("default parses");
        default.segment_token = "segment".into();
        let mut style = parse_rendering_style(&ctx, style_root, 26).expect("style parses");
        style.segment_token = "segment".into();
        style.record_ordinal = 8;
        let inventory = PresentationInventory {
            default_styles: vec![default],
            rendering_styles: vec![style],
            issues: Vec::new(),
        };
        let appearance = Appearance {
            id: AppearanceId("appearance".into()),
            name: None,
            asset_guid: Some("d3c6130d-6c0f-4525-b268-53517ab46a78".into()),
            library_id: Some("afefc330-5e61-4e24-814f-ae810148b79d".into()),
            visual_guid: None,
            physical_token: None,
            schema: None,
            category: None,
            base_color: None,
            properties: BTreeMap::new(),
            textures: Vec::new(),
        };

        let projection =
            project_default_bindings(&inventory, &[appearance], &[BodyId("body".into())]);

        assert_eq!(projection.unresolved_defaults, 0);
        let [binding] = projection.bindings.as_slice() else {
            panic!("one body binding must be projected");
        };
        assert_eq!(binding.appearance, AppearanceId("appearance".into()));
        assert_eq!(
            binding.target,
            AppearanceTarget::Body(BodyId("body".into()))
        );
    }

    fn default_style_fixture() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend(0_u32.to_le_bytes());
        bytes.extend(1_u16.to_le_bytes());
        for reference in 8_u32..=16 {
            bytes.extend((reference | 0x8000_0000).to_le_bytes());
        }
        bytes.push(0);
        bytes.extend(0x8000_0011_u32.to_le_bytes());
        bytes.extend([0; 8]);
        bytes
    }

    fn rendering_style_fixture() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend(0_u32.to_le_bytes());
        bytes.extend(7_u16.to_le_bytes());
        bytes.push(0);
        bytes.extend(3_u16.to_le_bytes());
        bytes.extend([0; 2]);
        bytes.extend(0_u16.to_le_bytes());
        bytes.extend(0_u16.to_le_bytes());
        bytes.extend(1_u32.to_le_bytes());
        bytes.extend(0_u32.to_le_bytes());
        bytes.extend(0x8000_0016_u32.to_le_bytes());
        utf16(&mut bytes, "Steel");
        utf16(&mut bytes, "Generic material");
        bytes.extend(0_u16.to_le_bytes());
        utf16(&mut bytes, "1:Steel");
        utf16(&mut bytes, "d3c6130d-6c0f-4525-b268-53517ab46a78");
        utf16(&mut bytes, "InvGen-066");
        utf16(&mut bytes, "afefc330-5e61-4e24-814f-ae810148b79d");
        bytes.extend(0_u16.to_le_bytes());
        bytes.extend(2_u16.to_le_bytes());
        bytes.extend([
            0xf0, 0x23, 0x1c, 0x26, 0xcd, 0x46, 0x2d, 0x79, 0x3e, 0x2d, 0x3d, 0x21, 0x13, 0xbd,
            0x6b, 0xac,
        ]);
        bytes.extend([0xaa, 0x55]);
        bytes
    }

    fn utf16(bytes: &mut Vec<u8>, value: &str) {
        let units = value.encode_utf16().collect::<Vec<_>>();
        bytes.extend((units.len() as u32).to_le_bytes());
        for unit in units {
            bytes.extend(unit.to_le_bytes());
        }
    }
}
