// SPDX-License-Identifier: Apache-2.0
//! Typed `PmApp` document-default and rendering-style records.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::{AppearanceId, BodyId, FaceId};
use cadmpeg_ir::topology::Color;

use crate::rse::{RecordFrameState, RseInventory, SegmentBulkState, SegmentKind};

const DEFAULT_STYLE_TYPE: [u8; 16] = [
    0xcd, 0xec, 0xfb, 0x11, 0xd1, 0x11, 0x6b, 0x25, 0x00, 0x08, 0xeb, 0xbb, 0x21, 0xed, 0xdc, 0x09,
];
const RENDERING_STYLE_TYPE: [u8; 16] = [
    0x6f, 0xd8, 0x59, 0x67, 0xd2, 0x11, 0x38, 0x78, 0x60, 0x00, 0x94, 0xb7, 0x0b, 0x02, 0xec, 0xb0,
];
const GRAPHICS_FACE_TYPE: [u8; 16] = [
    0xa3, 0xe9, 0x94, 0x51, 0xd2, 0x11, 0x9b, 0x28, 0x60, 0x00, 0x6a, 0xb7, 0x2c, 0x39, 0xcd, 0xb0,
];
const GRAPHICS_STYLE_COLLECTION_TYPE: [u8; 16] = [
    0x07, 0x86, 0xeb, 0x48, 0xd2, 0x11, 0x0c, 0x07, 0x60, 0x00, 0xf9, 0x9a, 0xc5, 0x36, 0x1a, 0xb0,
];
const GRAPHICS_PRIMARY_COLOR_STYLE_TYPE: [u8; 16] = [
    0x0f, 0x56, 0x48, 0xaf, 0xd4, 0x11, 0xc7, 0x8d, 0x10, 0x00, 0xd5, 0x8d, 0xc0, 0x4a, 0x0a, 0xb5,
];

#[derive(Debug)]
pub(crate) struct PresentationInventory<'a> {
    pub(crate) default_styles: Vec<PmAppDefaultStyle<'a>>,
    pub(crate) rendering_styles: Vec<PmAppRenderingStyle<'a>>,
    pub(crate) graphics_faces: Vec<PmGraphicsFace>,
    pub(crate) graphics_style_collections: Vec<PmGraphicsStyleCollection>,
    pub(crate) graphics_primary_color_styles: Vec<PmGraphicsPrimaryColorStyle>,
    pub(crate) issues: Vec<PresentationRecordIssue>,
}

#[derive(Debug)]
pub(crate) struct PmGraphicsPrimaryColorStyle {
    pub(crate) segment_token: String,
    pub(crate) record_ordinal: u32,
    pub(crate) segment_version_major: u8,
    pub(crate) header_value: u32,
    pub(crate) controls: [u16; 7],
    pub(crate) color_header: [u8; 2],
    pub(crate) colors: [[f32; 4]; 4],
    pub(crate) color_tail: [u16; 2],
    pub(crate) state: u8,
    pub(crate) values: [u16; 2],
    pub(crate) terminal_state: u8,
}

#[derive(Debug)]
pub(crate) struct PmGraphicsStyleCollection {
    pub(crate) segment_token: String,
    pub(crate) record_ordinal: u32,
    pub(crate) segment_version_major: u8,
    pub(crate) style_references: Vec<u32>,
    pub(crate) style_reference_qualifiers: Vec<bool>,
    pub(crate) list_metadata: Option<[u32; 2]>,
}

#[derive(Debug)]
pub(crate) struct PmGraphicsFace {
    pub(crate) segment_token: String,
    pub(crate) record_ordinal: u32,
    pub(crate) segment_version_major: u8,
    pub(crate) header_value: u32,
    pub(crate) header_id: u16,
    pub(crate) flags: u32,
    pub(crate) styles_reference: u32,
    pub(crate) styles_reference_qualified: bool,
    pub(crate) surface_reference: u32,
    pub(crate) surface_reference_qualified: bool,
    pub(crate) parent_reference: u32,
    pub(crate) parent_reference_qualified: bool,
    pub(crate) state: u32,
    pub(crate) edge_references: Vec<u32>,
    pub(crate) edge_reference_qualifiers: Vec<bool>,
    pub(crate) edge_list_metadata: Option<[u32; 2]>,
    pub(crate) visibility_state: u8,
    pub(crate) bounds: [f64; 6],
    pub(crate) key: u32,
    pub(crate) values: [u32; 2],
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
    pub(crate) appearances: Vec<Appearance>,
    pub(crate) bindings: Vec<AppearanceBinding>,
    pub(crate) unresolved_defaults: usize,
    pub(crate) unresolved_face_overrides: usize,
}

pub(crate) fn project_bindings(
    inventory: &PresentationInventory<'_>,
    appearances: &[Appearance],
    bodies: &[BodyId],
    face_keys: &std::collections::HashMap<FaceId, u64>,
) -> PresentationProjection {
    let mut projection = project_default_bindings(inventory, appearances, bodies);
    project_face_bindings(inventory, face_keys, &mut projection);
    projection
}

fn project_default_bindings(
    inventory: &PresentationInventory<'_>,
    appearances: &[Appearance],
    bodies: &[BodyId],
) -> PresentationProjection {
    if inventory.default_styles.len() != 1 {
        return PresentationProjection {
            appearances: Vec::new(),
            bindings: Vec::new(),
            unresolved_defaults: usize::from(!inventory.default_styles.is_empty()),
            unresolved_face_overrides: 0,
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
            appearances: Vec::new(),
            bindings: Vec::new(),
            unresolved_defaults: usize::from(!inventory.default_styles.is_empty()),
            unresolved_face_overrides: 0,
        };
    }
    let style = selected[0];
    let Some(asset_guid) = style
        .asset_guid
        .as_deref()
        .filter(|value| !value.is_empty())
    else {
        return PresentationProjection {
            appearances: Vec::new(),
            bindings: Vec::new(),
            unresolved_defaults: 1,
            unresolved_face_overrides: 0,
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
            appearances: Vec::new(),
            bindings: Vec::new(),
            unresolved_defaults: 1,
            unresolved_face_overrides: 0,
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
        appearances: Vec::new(),
        bindings,
        unresolved_defaults: 0,
        unresolved_face_overrides: 0,
    }
}

fn project_face_bindings(
    inventory: &PresentationInventory<'_>,
    face_keys: &std::collections::HashMap<FaceId, u64>,
    projection: &mut PresentationProjection,
) {
    let mut key_counts = std::collections::HashMap::new();
    for key in face_keys.values() {
        *key_counts.entry(*key).or_insert(0_usize) += 1;
    }
    let mut appearance_ids = std::collections::HashMap::new();
    let mut ordered_face_keys = face_keys.iter().collect::<Vec<_>>();
    ordered_face_keys.sort_by(|(left, _), (right, _)| left.0.cmp(&right.0));
    for (face_id, key) in ordered_face_keys {
        let matching_faces = inventory
            .graphics_faces
            .iter()
            .filter(|face| u64::from(face.key) == *key)
            .collect::<Vec<_>>();
        if matching_faces.is_empty() {
            continue;
        }
        if matching_faces.len() != 1 {
            projection.unresolved_face_overrides +=
                usize::from(matching_faces.iter().any(|face| face.styles_reference != 0));
            continue;
        }
        let graphics_face = matching_faces[0];
        if graphics_face.styles_reference == 0 {
            continue;
        }
        if key_counts.get(key) != Some(&1) {
            projection.unresolved_face_overrides += 1;
            continue;
        }
        let Some(collection_ordinal) = graphics_face.styles_reference.checked_sub(1) else {
            projection.unresolved_face_overrides += 1;
            continue;
        };
        let collections = inventory
            .graphics_style_collections
            .iter()
            .filter(|collection| {
                collection.segment_token == graphics_face.segment_token
                    && collection.record_ordinal == collection_ordinal
            })
            .collect::<Vec<_>>();
        if collections.len() != 1 {
            projection.unresolved_face_overrides += 1;
            continue;
        }
        let collection = collections[0];
        let color_styles = collection
            .style_references
            .iter()
            .filter_map(|reference| reference.checked_sub(1))
            .flat_map(|ordinal| {
                inventory
                    .graphics_primary_color_styles
                    .iter()
                    .filter(move |style| {
                        style.segment_token == collection.segment_token
                            && style.record_ordinal == ordinal
                    })
            })
            .collect::<Vec<_>>();
        if color_styles.len() != 1 {
            projection.unresolved_face_overrides += 1;
            continue;
        }
        let style = color_styles[0];
        let [r, g, b, a] = style.colors[1];
        if ![r, g, b, a]
            .iter()
            .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
        {
            projection.unresolved_face_overrides += 1;
            continue;
        }
        let appearance_id = appearance_ids
            .entry((style.segment_token.as_str(), style.record_ordinal))
            .or_insert_with(|| {
                let id = AppearanceId(format!(
                    "inventor:presentation:face-color#{}-{}",
                    style.segment_token, style.record_ordinal
                ));
                projection.appearances.push(Appearance {
                    id: id.clone(),
                    name: None,
                    asset_guid: None,
                    library_id: None,
                    visual_guid: None,
                    physical_token: None,
                    schema: Some("InventorPrimaryColorStyle".into()),
                    category: None,
                    base_color: Some(Color { r, g, b, a }),
                    properties: BTreeMap::new(),
                    textures: Vec::new(),
                });
                id
            })
            .clone();
        projection.bindings.push(AppearanceBinding {
            id: format!(
                "inventor:presentation:face-override#{}",
                &sha256_hex(face_id.0.as_bytes())[..16]
            ),
            target: AppearanceTarget::Face(face_id.clone()),
            appearance: appearance_id,
            source_entity_id: Some(format!(
                "inventor:presentation:graphics-face#{}-{}",
                graphics_face.segment_token, graphics_face.record_ordinal
            )),
            object_type: Some("Face".into()),
            channels: BTreeMap::from([("precedence".into(), "face_over_body".into())]),
        });
    }
}

pub(crate) fn inventory<'a>(
    ctx: &DecodeContext<'a>,
    document: &RseInventory<'a>,
) -> Result<PresentationInventory<'a>, CodecError> {
    let mut default_styles = Vec::new();
    let mut rendering_styles = Vec::new();
    let mut graphics_faces = Vec::new();
    let mut graphics_style_collections = Vec::new();
    let mut graphics_primary_color_styles = Vec::new();
    let mut issues = Vec::new();
    for segment in &document.segments {
        if !matches!(segment.kind, SegmentKind::PmApp | SegmentKind::PmGraphics) {
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
                GRAPHICS_FACE_TYPE if segment.kind == SegmentKind::PmGraphics => {
                    parse_graphics_face(ctx, record.payload, version).map(|mut value| {
                        value.segment_token = segment.pair.token.as_str().into();
                        value.record_ordinal = record.ordinal;
                        graphics_faces.push(value);
                    })
                }
                GRAPHICS_STYLE_COLLECTION_TYPE if segment.kind == SegmentKind::PmGraphics => {
                    parse_graphics_style_collection(ctx, record.payload, version).map(
                        |mut value| {
                            value.segment_token = segment.pair.token.as_str().into();
                            value.record_ordinal = record.ordinal;
                            graphics_style_collections.push(value);
                        },
                    )
                }
                GRAPHICS_PRIMARY_COLOR_STYLE_TYPE if segment.kind == SegmentKind::PmGraphics => {
                    parse_graphics_primary_color_style(record.payload, version).map(|mut value| {
                        value.segment_token = segment.pair.token.as_str().into();
                        value.record_ordinal = record.ordinal;
                        graphics_primary_color_styles.push(value);
                    })
                }
                _ => continue,
            };
            if let Err(error) = parsed {
                issues.push(PresentationRecordIssue {
                    segment_token: segment.pair.token.as_str().into(),
                    record_ordinal: record.ordinal,
                    detail: crate::issue_detail(error)?,
                });
            }
        }
    }
    ctx.charge_collection_items(
        default_styles.len() as u64
            + rendering_styles.len() as u64
            + graphics_faces.len() as u64
            + graphics_style_collections.len() as u64
            + graphics_primary_color_styles.len() as u64
            + issues.len() as u64,
        "admit Inventor presentation records",
    )?;
    Ok(PresentationInventory {
        default_styles,
        rendering_styles,
        graphics_faces,
        graphics_style_collections,
        graphics_primary_color_styles,
        issues,
    })
}

fn parse_graphics_primary_color_style(
    source: View<'_>,
    version: u8,
) -> Result<PmGraphicsPrimaryColorStyle, CodecError> {
    let mut cursor = Cursor::new(source);
    cursor.skip(
        legacy_block_len(version),
        "graphics primary-color legacy prefix",
    )?;
    let header_value = cursor.u32("graphics primary-color header value")?;
    cursor.skip(
        legacy_block_len(version),
        "graphics primary-color legacy header padding",
    )?;
    let mut controls = [0; 7];
    for (index, value) in controls.iter_mut().enumerate() {
        *value = cursor.u16(&format!("graphics primary-color control {index}"))?;
    }
    cursor.skip(
        legacy_block_len(version),
        "graphics primary-color legacy color prefix",
    )?;
    let color_header = [
        cursor.u8("graphics primary-color header 0")?,
        cursor.u8("graphics primary-color header 1")?,
    ];
    let mut colors = [[0.0; 4]; 4];
    for (color_index, color) in colors.iter_mut().enumerate() {
        for (component_index, component) in color.iter_mut().enumerate() {
            *component = cursor.f32(&format!(
                "graphics primary-color {color_index} component {component_index}"
            ))?;
        }
        cursor.skip(
            legacy_block_len(version),
            "graphics primary-color legacy component padding",
        )?;
    }
    let color_tail = [
        cursor.u16("graphics primary-color tail 0")?,
        cursor.u16("graphics primary-color tail 1")?,
    ];
    cursor.skip(
        legacy_block_len(version),
        "graphics primary-color legacy tail padding",
    )?;
    let state = cursor.u8("graphics primary-color state")?;
    let values = [
        cursor.u16("graphics primary-color value 0")?,
        cursor.u16("graphics primary-color value 1")?,
    ];
    cursor.skip(
        legacy_block_len(version),
        "graphics primary-color legacy value padding",
    )?;
    let terminal_state = cursor.u8("graphics primary-color terminal state")?;
    let suffix = cursor.remainder()?;
    if !suffix.window().is_empty() {
        return Err(CodecError::malformed(format_args!(
            "PmGraphics primary-color record has {} trailing bytes",
            suffix.window().len()
        )));
    }
    Ok(PmGraphicsPrimaryColorStyle {
        segment_token: String::new(),
        record_ordinal: 0,
        segment_version_major: version,
        header_value,
        controls,
        color_header,
        colors,
        color_tail,
        state,
        values,
        terminal_state,
    })
}

fn parse_graphics_style_collection(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmGraphicsStyleCollection, CodecError> {
    let mut cursor = Cursor::new(source);
    cursor.skip(
        legacy_block_len(version),
        "graphics-style collection legacy prefix",
    )?;
    let ReferenceList {
        references: style_references,
        qualifiers: style_reference_qualifiers,
        metadata: list_metadata,
    } = cursor.reference_list(ctx, "graphics-style collection")?;
    let suffix = cursor.remainder()?;
    if !suffix.window().is_empty() {
        return Err(CodecError::malformed(format_args!(
            "PmGraphics style-collection record has {} trailing bytes",
            suffix.window().len()
        )));
    }
    Ok(PmGraphicsStyleCollection {
        segment_token: String::new(),
        record_ordinal: 0,
        segment_version_major: version,
        style_references,
        style_reference_qualifiers,
        list_metadata,
    })
}

fn parse_graphics_face(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmGraphicsFace, CodecError> {
    let mut cursor = Cursor::new(source);
    let header_value = cursor.u32("graphics-face header value")?;
    let header_id = cursor.u16("graphics-face header id")?;
    cursor.skip(
        legacy_block_len(version),
        "graphics-face legacy header padding",
    )?;
    let flags = cursor.u32("graphics-face flags")?;
    let (styles_reference, styles_reference_qualified) =
        cursor.node_reference("graphics-face styles reference")?;
    let (surface_reference, surface_reference_qualified) =
        cursor.node_reference("graphics-face surface reference")?;
    let (parent_reference, parent_reference_qualified) =
        cursor.node_reference("graphics-face parent reference")?;
    let state = cursor.u32("graphics-face state")?;
    cursor.skip(
        legacy_block_len(version),
        "graphics-face legacy object padding",
    )?;
    let ReferenceList {
        references: edge_references,
        qualifiers: edge_reference_qualifiers,
        metadata: edge_list_metadata,
    } = cursor.reference_list(ctx, "graphics-face edge list")?;
    let visibility_state = cursor.u8("graphics-face visibility state")?;
    cursor.skip(
        legacy_block_len(version) * 2,
        "graphics-face legacy visibility padding",
    )?;
    let mut bounds = [0.0; 6];
    for (index, value) in bounds.iter_mut().enumerate() {
        *value = cursor.f64(&format!("graphics-face bound {index}"))?;
    }
    cursor.skip(
        legacy_block_len(version),
        "graphics-face legacy bounds padding",
    )?;
    let key = cursor.u32("graphics-face key")?;
    let values = [
        cursor.u32("graphics-face value 0")?,
        cursor.u32("graphics-face value 1")?,
    ];
    let suffix = cursor.remainder()?;
    if !suffix.window().is_empty() {
        return Err(CodecError::malformed(format_args!(
            "PmGraphics face record has {} trailing bytes",
            suffix.window().len()
        )));
    }
    Ok(PmGraphicsFace {
        segment_token: String::new(),
        record_ordinal: 0,
        segment_version_major: version,
        header_value,
        header_id,
        flags,
        styles_reference,
        styles_reference_qualified,
        surface_reference,
        surface_reference_qualified,
        parent_reference,
        parent_reference_qualified,
        state,
        edge_references,
        edge_reference_qualifiers,
        edge_list_metadata,
        visibility_state,
        bounds,
        key,
        values,
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
        return Err(CodecError::malformed(format_args!(
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
}

struct ReferenceList {
    references: Vec<u32>,
    qualifiers: Vec<bool>,
    metadata: Option<[u32; 2]>,
}

impl<'a> Cursor<'a> {
    const fn new(source: View<'a>) -> Self {
        Self { source }
    }

    fn take(&mut self, len: usize, _field: &str) -> Result<&'a [u8], CodecError> {
        Ok(self.source.req_take(len)?)
    }

    fn skip(&mut self, len: usize, field: &str) -> Result<(), CodecError> {
        self.take(len, field).map(|_| ())
    }

    fn zeroes(&mut self, len: usize, field: &str) -> Result<(), CodecError> {
        if self.take(len, field)?.iter().any(|byte| *byte != 0) {
            return Err(CodecError::malformed(format_args!(
                "Inventor presentation {field} is not zero-filled"
            )));
        }
        Ok(())
    }

    fn u8(&mut self, _field: &str) -> Result<u8, CodecError> {
        Ok(self.source.req_u8()?)
    }

    fn u16(&mut self, _field: &str) -> Result<u16, CodecError> {
        Ok(self.source.req_u16_le()?)
    }

    fn u32(&mut self, _field: &str) -> Result<u32, CodecError> {
        Ok(self.source.req_u32_le()?)
    }

    fn f64(&mut self, _field: &str) -> Result<f64, CodecError> {
        Ok(self.source.req_f64_le()?)
    }

    fn f32(&mut self, _field: &str) -> Result<f32, CodecError> {
        Ok(self.source.req_f32_le()?)
    }

    fn reference(&mut self, field: &str) -> Result<u32, CodecError> {
        let value = self.u32(field)?;
        if value != 0 && value & 0x8000_0000 == 0 {
            return Err(CodecError::malformed(format_args!(
                "Inventor presentation {field} lacks its reference qualifier"
            )));
        }
        Ok(value & 0x7fff_ffff)
    }

    fn node_reference(&mut self, field: &str) -> Result<(u32, bool), CodecError> {
        let value = self.u32(field)?;
        Ok((value & 0x7fff_ffff, value & 0x8000_0000 != 0))
    }

    fn reference_list(
        &mut self,
        ctx: &DecodeContext<'_>,
        field: &str,
    ) -> Result<ReferenceList, CodecError> {
        let marker = [
            self.u16(&format!("{field} marker 0"))?,
            self.u16(&format!("{field} marker 1"))?,
        ];
        if marker != [2, 0x3000] {
            return Err(CodecError::malformed(format_args!(
                "PmGraphics {field} has marker {marker:?}, expected [2, 12288]"
            )));
        }
        let count = self.u32(&format!("{field} count"))? as usize;
        ctx.charge_collection_items(count as u64, "admit Inventor PmGraphics references")?;
        let metadata = if count == 0 {
            None
        } else {
            Some([
                self.u32(&format!("{field} metadata 0"))?,
                self.u32(&format!("{field} metadata 1"))?,
            ])
        };
        let mut references = Vec::with_capacity(count);
        let mut qualifiers = Vec::with_capacity(count);
        for index in 0..count {
            let (reference, qualified) =
                self.node_reference(&format!("{field} reference {index}"))?;
            references.push(reference);
            qualifiers.push(qualified);
        }
        Ok(ReferenceList {
            references,
            qualifiers,
            metadata,
        })
    }

    fn utf16(&mut self, ctx: &DecodeContext<'_>, field: &str) -> Result<String, CodecError> {
        let units = self.u32(&format!("{field} length"))? as usize;
        if units > 1_048_576 {
            return Err(CodecError::malformed(format_args!(
                "Inventor presentation {field} exceeds 1048576 UTF-16 code units"
            )));
        }
        let byte_len = units.checked_mul(2).ok_or_else(|| {
            CodecError::malformed(format_args!(
                "Inventor presentation {field} byte length overflows"
            ))
        })?;
        ctx.charge_retained(byte_len as u64, "retain Inventor PmApp UTF-16 string", None)?;
        self.source
            .utf16_le(units)
            .map(|value| value.trim_end_matches('\0').to_owned())
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "Inventor presentation {field} is invalid UTF-16"
                ))
            })
    }

    fn guid(&mut self, field: &str) -> Result<String, CodecError> {
        let bytes: [u8; 16] = self.take(16, field)?.try_into().expect("16-byte read");
        Ok(format!(
            "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{}",
            View::u32_le_at(&bytes, 0).expect("four-byte group"),
            View::u16_le_at(&bytes, 4).expect("two-byte group"),
            View::u16_le_at(&bytes, 6).expect("two-byte group"),
            bytes[8],
            bytes[9],
            hex(&bytes[10..])
        ))
    }

    fn remainder(&mut self) -> Result<View<'a>, CodecError> {
        let start = self.source.position();
        let end = self.source.end();
        self.source.seek(end).ok_or_else(|| {
            CodecError::Malformed("Inventor presentation suffix range is invalid".into())
        })?;
        self.source.child(start, end).ok_or_else(|| {
            CodecError::Malformed("Inventor presentation suffix range is invalid".into())
        })
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
            graphics_faces: Vec::new(),
            graphics_style_collections: Vec::new(),
            graphics_primary_color_styles: Vec::new(),
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

    #[test]
    fn parses_current_graphics_face_with_qualified_references() {
        let mut bytes = Vec::new();
        bytes.extend(4_u32.to_le_bytes());
        bytes.extend(5_u16.to_le_bytes());
        bytes.extend(6_u32.to_le_bytes());
        for reference in 7_u32..=9 {
            bytes.extend((reference | 0x8000_0000).to_le_bytes());
        }
        bytes.extend(10_u32.to_le_bytes());
        bytes.extend(2_u16.to_le_bytes());
        bytes.extend(0x3000_u16.to_le_bytes());
        bytes.extend(2_u32.to_le_bytes());
        bytes.extend(11_u32.to_le_bytes());
        bytes.extend(12_u32.to_le_bytes());
        bytes.extend(0x8000_000d_u32.to_le_bytes());
        bytes.extend(0x8000_000e_u32.to_le_bytes());
        bytes.push(1);
        for value in [1.0_f64, 2.0, 3.0, 4.0, 5.0, 6.0] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(15_u32.to_le_bytes());
        bytes.extend(16_u32.to_le_bytes());
        bytes.extend(17_u32.to_le_bytes());
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic graphics face fits policy");

        let face = parse_graphics_face(&ctx, root, 26).expect("graphics face parses");

        assert_eq!(face.styles_reference, 7);
        assert!(face.styles_reference_qualified);
        assert_eq!(face.surface_reference, 8);
        assert!(face.surface_reference_qualified);
        assert_eq!(face.parent_reference, 9);
        assert!(face.parent_reference_qualified);
        assert_eq!(face.edge_references, [13, 14]);
        assert_eq!(face.edge_reference_qualifiers, [true, true]);
        assert_eq!(face.edge_list_metadata, Some([11, 12]));
        assert_eq!(face.bounds, [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        assert_eq!(face.key, 15);
        assert_eq!(face.values, [16, 17]);
    }

    #[test]
    fn parses_current_graphics_style_collection() {
        let mut bytes = Vec::new();
        bytes.extend(2_u16.to_le_bytes());
        bytes.extend(0x3000_u16.to_le_bytes());
        bytes.extend(2_u32.to_le_bytes());
        bytes.extend(21_u32.to_le_bytes());
        bytes.extend(22_u32.to_le_bytes());
        bytes.extend(0x8000_0017_u32.to_le_bytes());
        bytes.extend(24_u32.to_le_bytes());
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic graphics style collection fits policy");

        let styles = parse_graphics_style_collection(&ctx, root, 26)
            .expect("graphics style collection parses");

        assert_eq!(styles.style_references, [23, 24]);
        assert_eq!(styles.style_reference_qualifiers, [true, false]);
        assert_eq!(styles.list_metadata, Some([21, 22]));
    }

    #[test]
    fn parses_current_graphics_primary_color_style() {
        let bytes = primary_color_fixture([0.2, 0.4, 0.6, 0.8]);
        let arena = DecodeArena::new();
        let (_, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic primary-color style fits policy");

        let style = parse_graphics_primary_color_style(root, 26)
            .expect("graphics primary-color style parses");

        assert_eq!(style.header_value, 31);
        assert_eq!(style.controls, [32, 33, 34, 35, 36, 37, 38]);
        assert_eq!(style.color_header, [39, 40]);
        assert_eq!(style.colors[1], [0.2, 0.4, 0.6, 0.8]);
        assert_eq!(style.color_tail, [41, 42]);
        assert_eq!(style.state, 43);
        assert_eq!(style.values, [44, 45]);
        assert_eq!(style.terminal_state, 46);
    }

    #[test]
    fn projects_face_override_through_native_key_and_style_graph() {
        let face = PmGraphicsFace {
            segment_token: "graphics".into(),
            record_ordinal: 2,
            segment_version_major: 26,
            header_value: 0,
            header_id: 0,
            flags: 0,
            styles_reference: 5,
            styles_reference_qualified: true,
            surface_reference: 0,
            surface_reference_qualified: false,
            parent_reference: 0,
            parent_reference_qualified: false,
            state: 0,
            edge_references: Vec::new(),
            edge_reference_qualifiers: Vec::new(),
            edge_list_metadata: None,
            visibility_state: 0,
            bounds: [0.0; 6],
            key: 42,
            values: [0; 2],
        };
        let collection = PmGraphicsStyleCollection {
            segment_token: "graphics".into(),
            record_ordinal: 4,
            segment_version_major: 26,
            style_references: vec![7],
            style_reference_qualifiers: vec![true],
            list_metadata: Some([1, 2]),
        };
        let style = PmGraphicsPrimaryColorStyle {
            segment_token: "graphics".into(),
            record_ordinal: 6,
            segment_version_major: 26,
            header_value: 0,
            controls: [0; 7],
            color_header: [0; 2],
            colors: [[0.0; 4], [0.2, 0.4, 0.6, 0.8], [0.0; 4], [0.0; 4]],
            color_tail: [0; 2],
            state: 0,
            values: [0; 2],
            terminal_state: 0,
        };
        let inventory = PresentationInventory {
            default_styles: Vec::new(),
            rendering_styles: Vec::new(),
            graphics_faces: vec![face],
            graphics_style_collections: vec![collection],
            graphics_primary_color_styles: vec![style],
            issues: Vec::new(),
        };
        let face_id = FaceId("face".into());
        let face_keys = std::collections::HashMap::from([(face_id.clone(), 42)]);

        let projection = project_bindings(&inventory, &[], &[], &face_keys);

        assert_eq!(projection.unresolved_face_overrides, 0);
        let [appearance] = projection.appearances.as_slice() else {
            panic!("one primary-color appearance must be projected");
        };
        assert_eq!(
            appearance.base_color,
            Some(Color {
                r: 0.2,
                g: 0.4,
                b: 0.6,
                a: 0.8
            })
        );
        let [binding] = projection.bindings.as_slice() else {
            panic!("one face binding must be projected");
        };
        assert_eq!(binding.target, AppearanceTarget::Face(face_id));
        assert_eq!(binding.appearance, appearance.id);
        assert_eq!(
            binding.channels.get("precedence").map(String::as_str),
            Some("face_over_body")
        );
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

    fn primary_color_fixture(diffuse: [f32; 4]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend(31_u32.to_le_bytes());
        for value in 32_u16..=38 {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend([39, 40]);
        for color in [[0.1; 4], diffuse, [0.7; 4], [0.9; 4]] {
            for component in color {
                bytes.extend(component.to_le_bytes());
            }
        }
        bytes.extend(41_u16.to_le_bytes());
        bytes.extend(42_u16.to_le_bytes());
        bytes.push(43);
        bytes.extend(44_u16.to_le_bytes());
        bytes.extend(45_u16.to_le_bytes());
        bytes.push(46);
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
