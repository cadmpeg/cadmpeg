// SPDX-License-Identifier: Apache-2.0
//! Typed `PmApp` document-default and rendering-style records.

use std::collections::BTreeMap;
use std::num::NonZeroUsize;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::ids::{AppearanceBindingId, AppearanceId, BodyId, FaceId, IdentityKey};
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::topology::Color;

use crate::assembly::count_unresolved;
use crate::pmdc::{type_id_string, PmDcPairedReferenceList, PmDcReference};
use crate::record_identity::Located;
use crate::record_issue::{admit_issue_detail, RecordIssue, RecordIssueFamily};
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
    pub(crate) default_styles: Vec<Located<PmAppDefaultStyle<'a>>>,
    pub(crate) rendering_styles: Vec<Located<PmAppRenderingStyle<'a>>>,
    pub(crate) graphics_faces: Vec<Located<PmGraphicsFace>>,
    pub(crate) graphics_style_collections: Vec<Located<PmGraphicsStyleCollection>>,
    pub(crate) graphics_primary_color_styles: Vec<Located<PmGraphicsPrimaryColorStyle>>,
    pub(crate) issues: Vec<RecordIssue>,
}

#[derive(Debug)]
pub(crate) struct PmGraphicsPrimaryColorStyle {
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
    pub(crate) segment_version_major: u8,
    pub(crate) style_references: PmDcPairedReferenceList<[u32; 2]>,
}

#[derive(Debug)]
pub(crate) struct PmGraphicsFace {
    pub(crate) segment_version_major: u8,
    pub(crate) header_value: u32,
    pub(crate) header_id: u16,
    pub(crate) flags: u32,
    pub(crate) styles: PmDcReference,
    pub(crate) surface: PmDcReference,
    pub(crate) parent: PmDcReference,
    pub(crate) state: u32,
    pub(crate) edge_references: PmDcPairedReferenceList<[u32; 2]>,
    pub(crate) visibility_state: u8,
    pub(crate) bounds: [FiniteReal; 6],
    pub(crate) key: u32,
    pub(crate) values: [u32; 2],
}

#[derive(Debug)]
pub(crate) struct PmAppDefaultStyle<'a> {
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
    pub(crate) extension: Option<RenderingStyleExtension>,
    pub(crate) suffix: View<'a>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RenderingStyleExtension {
    pub(crate) style_state: u16,
    pub(crate) style_label: String,
    pub(crate) asset_guid: String,
    pub(crate) material_id: String,
    pub(crate) asset_library_id: String,
    pub(crate) style_values: [u16; 2],
    pub(crate) guid: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum UnresolvedCause {
    GraphicsFace,
    FaceKey,
    StyleCollection,
    ColorStyle,
    Color,
}

impl UnresolvedCause {
    pub(crate) fn description(self) -> &'static str {
        match self {
            Self::GraphicsFace => "graphics face key is ambiguous",
            Self::FaceKey => "model face key is ambiguous",
            Self::StyleCollection => "style collection is missing or ambiguous",
            Self::ColorStyle => "primary color style is missing or ambiguous",
            Self::Color => "primary color is invalid",
        }
    }
}

pub(crate) struct PresentationProjection {
    pub(crate) appearances: Vec<Appearance>,
    pub(crate) bindings: Vec<AppearanceBinding>,
    pub(crate) unresolved_defaults: usize,
    pub(crate) unresolved_face_overrides: BTreeMap<UnresolvedCause, NonZeroUsize>,
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
            unresolved_face_overrides: BTreeMap::new(),
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
                style.identity.segment_token == default.identity.segment_token
                    && style.identity.record_ordinal == ordinal
            })
            .collect::<Vec<_>>();
        if matches.len() == 1 {
            selected.push(matches[0]);
        }
    }
    selected.sort_by(|left, right| {
        left.identity
            .segment_token
            .cmp(&right.identity.segment_token)
            .then_with(|| {
                left.identity
                    .record_ordinal
                    .cmp(&right.identity.record_ordinal)
            })
    });
    selected.dedup_by(|left, right| {
        left.identity.segment_token == right.identity.segment_token
            && left.identity.record_ordinal == right.identity.record_ordinal
    });
    if selected.len() != 1 {
        return PresentationProjection {
            appearances: Vec::new(),
            bindings: Vec::new(),
            unresolved_defaults: usize::from(!inventory.default_styles.is_empty()),
            unresolved_face_overrides: BTreeMap::new(),
        };
    }
    let style = selected[0];
    let Some(asset_guid) = style
        .extension
        .as_ref()
        .map(|extension| extension.asset_guid.as_str())
        .filter(|value| !value.is_empty())
    else {
        return PresentationProjection {
            appearances: Vec::new(),
            bindings: Vec::new(),
            unresolved_defaults: 1,
            unresolved_face_overrides: BTreeMap::new(),
        };
    };
    let library_id = style
        .extension
        .as_ref()
        .map(|extension| extension.asset_library_id.as_str());
    let matches = appearances
        .iter()
        .filter(|appearance| {
            appearance
                .asset_guid
                .as_deref()
                .is_some_and(|value| value.eq_ignore_ascii_case(asset_guid))
        })
        .filter(|appearance| match library_id {
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
            unresolved_face_overrides: BTreeMap::new(),
        };
    }
    let appearance = &matches[0].id;
    let bindings = bodies
        .iter()
        .map(|body| AppearanceBinding {
            id: AppearanceBindingId::compose(
                &cadmpeg_ir::identity_namespace!("inventor", "presentation", "body-default"),
                short_digest_key(body.as_str().as_bytes()),
            ),
            target: AppearanceTarget::Body(body.clone()),
            appearance: appearance.clone(),
            source_entity_id: Some(format!(
                "inventor:presentation:rendering-style#{}-{}",
                style.identity.segment_token, style.identity.record_ordinal
            )),
            object_type: Some("Body".into()),
            visible: None,
            channels: BTreeMap::default(),
        })
        .collect();
    PresentationProjection {
        appearances: Vec::new(),
        bindings,
        unresolved_defaults: 0,
        unresolved_face_overrides: BTreeMap::new(),
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
    ordered_face_keys.sort_by_key(|(left, _)| *left);
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
            if matching_faces.iter().any(|face| face.styles.index != 0) {
                count_unresolved(
                    &mut projection.unresolved_face_overrides,
                    UnresolvedCause::GraphicsFace,
                );
            }
            continue;
        }
        let graphics_face = matching_faces[0];
        let Some(collection_ordinal) = graphics_face.styles.index.checked_sub(1) else {
            continue;
        };
        if key_counts.get(key) != Some(&1) {
            count_unresolved(
                &mut projection.unresolved_face_overrides,
                UnresolvedCause::FaceKey,
            );
            continue;
        }
        let collections = inventory
            .graphics_style_collections
            .iter()
            .filter(|collection| {
                collection.identity.segment_token == graphics_face.identity.segment_token
                    && collection.identity.record_ordinal == collection_ordinal
            })
            .collect::<Vec<_>>();
        if collections.len() != 1 {
            count_unresolved(
                &mut projection.unresolved_face_overrides,
                UnresolvedCause::StyleCollection,
            );
            continue;
        }
        let collection = collections[0];
        let color_styles = collection
            .style_references
            .references()
            .iter()
            .filter_map(|reference| reference.index.checked_sub(1))
            .flat_map(|ordinal| {
                inventory
                    .graphics_primary_color_styles
                    .iter()
                    .filter(move |style| {
                        style.identity.segment_token == collection.identity.segment_token
                            && style.identity.record_ordinal == ordinal
                    })
            })
            .collect::<Vec<_>>();
        if color_styles.len() != 1 {
            count_unresolved(
                &mut projection.unresolved_face_overrides,
                UnresolvedCause::ColorStyle,
            );
            continue;
        }
        let style = color_styles[0];
        let [r, g, b, a] = style.colors[1];
        let Some(color) = Color::new(r, g, b, a) else {
            count_unresolved(
                &mut projection.unresolved_face_overrides,
                UnresolvedCause::Color,
            );
            continue;
        };
        let appearance_id = appearance_ids
            .entry((
                style.identity.segment_token.as_str(),
                style.identity.record_ordinal,
            ))
            .or_insert_with(|| {
                let id = AppearanceId::compose(
                    &cadmpeg_ir::identity_namespace!("inventor", "presentation", "face-color"),
                    style.identity.key(),
                );
                projection.appearances.push(Appearance {
                    id: id.clone(),
                    name: None,
                    asset_guid: None,
                    library_id: None,
                    visual_guid: None,
                    physical_token: None,
                    schema: Some("InventorPrimaryColorStyle".into()),
                    category: None,
                    base_color: Some(color),
                    properties: BTreeMap::new(),
                    textures: Vec::new(),
                });
                id
            })
            .clone();
        projection.bindings.push(AppearanceBinding {
            id: AppearanceBindingId::compose(
                &cadmpeg_ir::identity_namespace!("inventor", "presentation", "face-override"),
                short_digest_key(face_id.as_str().as_bytes()),
            ),
            target: AppearanceTarget::Face(face_id.clone()),
            appearance: appearance_id,
            source_entity_id: Some(format!(
                "inventor:presentation:graphics-face#{}-{}",
                graphics_face.identity.segment_token, graphics_face.identity.record_ordinal
            )),
            object_type: Some("Face".into()),
            visible: None,
            channels: BTreeMap::from([(
                cadmpeg_core::nonblank_literal!("precedence"),
                "face_over_body".into(),
            )]),
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
        let Some(version) = segment.registry.map(|join| join.version_major) else {
            continue;
        };
        let SegmentBulkState::Framed(bulk) = &segment.bulk else {
            continue;
        };
        let RecordFrameState::Framed(table) = &bulk.records else {
            continue;
        };
        for record in &table.records {
            let token = segment.pair.token.key();
            let ordinal = record.ordinal;
            let parsed = match record.type_id {
                DEFAULT_STYLE_TYPE => {
                    parse_default_style(ctx, record.payload, version).and_then(|value| {
                        push_presentation_record(
                            ctx,
                            &mut default_styles,
                            value,
                            record.type_id,
                            token,
                            ordinal,
                            "admit Inventor default style record",
                        )
                    })
                }
                RENDERING_STYLE_TYPE => parse_rendering_style(ctx, record.payload, version)
                    .and_then(|value| {
                        push_presentation_record(
                            ctx,
                            &mut rendering_styles,
                            value,
                            record.type_id,
                            token,
                            ordinal,
                            "admit Inventor rendering style record",
                        )
                    }),
                GRAPHICS_FACE_TYPE if segment.kind == SegmentKind::PmGraphics => {
                    parse_graphics_face(ctx, record.payload, version).and_then(|value| {
                        push_presentation_record(
                            ctx,
                            &mut graphics_faces,
                            value,
                            record.type_id,
                            token,
                            ordinal,
                            "admit Inventor graphics face record",
                        )
                    })
                }
                GRAPHICS_STYLE_COLLECTION_TYPE if segment.kind == SegmentKind::PmGraphics => {
                    parse_graphics_style_collection(ctx, record.payload, version).and_then(
                        |value| {
                            push_presentation_record(
                                ctx,
                                &mut graphics_style_collections,
                                value,
                                record.type_id,
                                token,
                                ordinal,
                                "admit Inventor graphics style collection record",
                            )
                        },
                    )
                }
                GRAPHICS_PRIMARY_COLOR_STYLE_TYPE if segment.kind == SegmentKind::PmGraphics => {
                    parse_graphics_primary_color_style(record.payload, version).and_then(|value| {
                        push_presentation_record(
                            ctx,
                            &mut graphics_primary_color_styles,
                            value,
                            record.type_id,
                            token,
                            ordinal,
                            "admit Inventor graphics primary color style record",
                        )
                    })
                }
                _ => continue,
            };
            if let Err(error) = parsed {
                if matches!(error, CodecError::ResourceLimit(_)) {
                    return Err(error);
                }
                ctx.charge_collection_items(1, "admit Inventor presentation issue")?;
                admit_issue_detail(ctx, &error, "retain Inventor presentation issue detail")?;
                ctx.charge_retained(
                    segment.pair.token.as_str().len() as u64,
                    "retain Inventor presentation issue token",
                )?;
                issues.push(RecordIssue {
                    family: RecordIssueFamily::Presentation,
                    segment_token: segment.pair.token.as_str().into(),
                    record_ordinal: record.ordinal,
                    detail: crate::issue_detail(error)?,
                });
            }
        }
    }
    Ok(PresentationInventory {
        default_styles,
        rendering_styles,
        graphics_faces,
        graphics_style_collections,
        graphics_primary_color_styles,
        issues,
    })
}

fn push_presentation_record<T>(
    ctx: &DecodeContext<'_>,
    records: &mut Vec<Located<T>>,
    value: T,
    type_id: [u8; 16],
    token: &IdentityKey,
    ordinal: u32,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(1, operation)?;
    ctx.charge_retained(32, "retain Inventor presentation record type id")?;
    ctx.charge_retained(
        token.as_str().len() as u64,
        "retain Inventor presentation record segment token",
    )?;
    records.push(Located::new(value, type_id_string(type_id), token, ordinal));
    Ok(())
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
    for value in &mut controls {
        *value = cursor.u16("graphics primary-color control")?;
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
    for color in &mut colors {
        for component in color {
            *component = cursor.f32("graphics primary-color component")?;
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
    let style_references = cursor.reference_list(ctx, "graphics-style collection")?;
    let suffix = cursor.remainder()?;
    if !suffix.window().is_empty() {
        return Err(CodecError::malformed(format_args!(
            "PmGraphics style-collection record has {} trailing bytes",
            suffix.window().len()
        )));
    }
    Ok(PmGraphicsStyleCollection {
        segment_version_major: version,
        style_references,
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
    let styles = cursor.node_reference("graphics-face styles reference")?;
    let surface = cursor.node_reference("graphics-face surface reference")?;
    let parent = cursor.node_reference("graphics-face parent reference")?;
    let state = cursor.u32("graphics-face state")?;
    cursor.skip(
        legacy_block_len(version),
        "graphics-face legacy object padding",
    )?;
    let edge_references = cursor.reference_list(ctx, "graphics-face edge list")?;
    let visibility_state = cursor.u8("graphics-face visibility state")?;
    cursor.skip(
        legacy_block_len(version) * 2,
        "graphics-face legacy visibility padding",
    )?;
    let mut bounds = [FiniteReal::ZERO; 6];
    for value in &mut bounds {
        *value = cursor.finite_f64("graphics-face bound")?;
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
        segment_version_major: version,
        header_value,
        header_id,
        flags,
        styles,
        surface,
        parent,
        state,
        edge_references,
        visibility_state,
        bounds,
        key,
        values,
    })
}

/// Field names of the seven related references a default style states, in
/// wire order.
const RELATED_REFERENCE_FIELDS: [&str; 7] = [
    "default-style related reference 0",
    "default-style related reference 1",
    "default-style related reference 2",
    "default-style related reference 3",
    "default-style related reference 4",
    "default-style related reference 5",
    "default-style related reference 6",
];

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
    let mut related_references = [0; RELATED_REFERENCE_FIELDS.len()];
    for (field, reference) in RELATED_REFERENCE_FIELDS
        .into_iter()
        .zip(related_references.iter_mut())
    {
        *reference = cursor.reference(field)?;
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
    let extension = if version > 16 {
        let style_state = cursor.u16("rendering-style style state")?;
        let style_label = cursor.utf16(ctx, "rendering-style text 0")?;
        let asset_guid = cursor.utf16(ctx, "rendering-style text 1")?;
        let material_id = cursor.utf16(ctx, "rendering-style text 2")?;
        let asset_library_id = cursor.utf16(ctx, "rendering-style text 3")?;
        let style_values = [
            cursor.u16("rendering-style style value 0")?,
            cursor.u16("rendering-style style value 1")?,
        ];
        let guid = cursor.guid("rendering-style guid")?;
        Some(RenderingStyleExtension {
            style_state,
            style_label,
            asset_guid,
            material_id,
            asset_library_id,
            style_values,
            guid,
        })
    } else {
        None
    };
    let suffix = cursor.remainder()?;
    Ok(PmAppRenderingStyle {
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
        extension,
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

impl<'a> Cursor<'a> {
    const fn new(source: View<'a>) -> Self {
        Self { source }
    }

    fn take(&mut self, len: usize, field: &'static str) -> Result<&'a [u8], CodecError> {
        crate::reader::take(&mut self.source, len, field)
    }

    fn skip(&mut self, len: usize, field: &'static str) -> Result<(), CodecError> {
        self.take(len, field).map(|_| ())
    }

    fn zeroes(&mut self, len: usize, field: &'static str) -> Result<(), CodecError> {
        if self.take(len, field)?.iter().any(|byte| *byte != 0) {
            return Err(CodecError::malformed(format_args!(
                "Inventor presentation {field} is not zero-filled"
            )));
        }
        Ok(())
    }

    fn u8(&mut self, field: &'static str) -> Result<u8, CodecError> {
        crate::reader::u8(&mut self.source, field)
    }

    fn u16(&mut self, field: &'static str) -> Result<u16, CodecError> {
        crate::reader::u16(&mut self.source, field)
    }

    fn u32(&mut self, field: &'static str) -> Result<u32, CodecError> {
        crate::reader::u32(&mut self.source, field)
    }

    fn finite_f64(&mut self, field: &'static str) -> Result<FiniteReal, CodecError> {
        let value = self
            .source
            .req_f64_le()
            .map_err(|error| error.during(field))?;
        FiniteReal::new(value).ok_or_else(|| {
            CodecError::malformed(format_args!("Inventor presentation {field} is not finite"))
        })
    }

    fn f32(&mut self, field: &'static str) -> Result<f32, CodecError> {
        Ok(self
            .source
            .req_f32_le()
            .map_err(|error| error.during(field))?)
    }

    fn reference(&mut self, field: &'static str) -> Result<u32, CodecError> {
        let value = self.u32(field)?;
        if value != 0 && value & 0x8000_0000 == 0 {
            return Err(CodecError::malformed(format_args!(
                "Inventor presentation {field} lacks its reference qualifier"
            )));
        }
        Ok(value & 0x7fff_ffff)
    }

    fn node_reference(&mut self, field: &'static str) -> Result<PmDcReference, CodecError> {
        crate::reader::pmdc_reference(&mut self.source, field)
    }

    fn reference_list(
        &mut self,
        ctx: &DecodeContext<'_>,
        field: &'static str,
    ) -> Result<PmDcPairedReferenceList<[u32; 2]>, CodecError> {
        let marker = [
            self.u16("graphics reference-list marker 0")?,
            self.u16("graphics reference-list marker 1")?,
        ];
        if marker != [2, 0x3000] {
            return Err(CodecError::malformed(format_args!(
                "PmGraphics {field} has marker {marker:?}, expected [2, 12288]"
            )));
        }
        let count = self.u32("graphics reference-list count")? as usize;
        ctx.charge_collection_items(count as u64, "admit Inventor PmGraphics references")?;
        if count == 0 {
            return Ok(PmDcPairedReferenceList::default());
        }
        let metadata = [
            self.u32("graphics reference-list metadata 0")?,
            self.u32("graphics reference-list metadata 1")?,
        ];
        let mut references = Vec::with_capacity(count);
        for _ in 0..count {
            references.push(self.node_reference("graphics reference-list entry")?);
        }
        PmDcPairedReferenceList::new(Some(metadata), references).ok_or_else(|| {
            CodecError::Malformed(
                "Inventor graphics reference list metadata disagrees with length".into(),
            )
        })
    }

    fn utf16(
        &mut self,
        ctx: &DecodeContext<'_>,
        field: &'static str,
    ) -> Result<String, CodecError> {
        let units = self.u32(field)? as usize;
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
        ctx.charge_retained(byte_len as u64, "retain Inventor PmApp UTF-16 string")?;
        self.source
            .utf16_le(units)
            .map(|value| value.trim_end_matches('\0').to_owned())
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "Inventor presentation {field} is invalid UTF-16"
                ))
            })
    }

    fn guid(&mut self, field: &'static str) -> Result<String, CodecError> {
        let first = self.u32(field)?;
        let second = self.u16(field)?;
        let third = self.u16(field)?;
        let tail: [u8; 8] = self.source.array().ok_or_else(|| {
            CodecError::malformed(format_args!("truncated Inventor presentation {field}"))
        })?;
        Ok(format!(
            "{first:08x}-{second:04x}-{third:04x}-{:02x}{:02x}-{}",
            tail[0],
            tail[1],
            hex(&tail[2..])
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

/// The first eight digest bytes of `bytes`, as sixteen lowercase hex digits.
///
/// A hexadecimal digit is identity-key text, so the key is built from the
/// digest bytes rather than sliced back out of a rendered string.
fn short_digest_key(bytes: &[u8]) -> cadmpeg_ir::ids::IdentityKey {
    let digest = cadmpeg_ir::hash::sha256(bytes);
    cadmpeg_ir::ids::IdentityKey::hex_byte(digest[0]).with_hex_bytes(&digest[1..8])
}

fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    crate::pmdc::push_hex(&mut text, bytes);
    text
}

pub(crate) fn suffix_fields(source: View<'_>) -> (u64, crate::native::digest::Sha256Hex) {
    (
        source.window().len() as u64,
        crate::native::digest::Sha256Hex::digest(source.window()),
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::appearance::AppearanceTarget;
    use cadmpeg_ir::ids::AppearanceId;
    use cadmpeg_ir::scalar::FiniteReal;

    use super::{
        inventory, parse_default_style, parse_graphics_face, parse_graphics_primary_color_style,
        parse_graphics_style_collection, parse_rendering_style, project_bindings,
        project_default_bindings, Cursor, PmGraphicsFace, PmGraphicsPrimaryColorStyle,
        PmGraphicsStyleCollection, PresentationInventory, DEFAULT_STYLE_TYPE, GRAPHICS_FACE_TYPE,
        GRAPHICS_PRIMARY_COLOR_STYLE_TYPE, GRAPHICS_STYLE_COLLECTION_TYPE, RENDERING_STYLE_TYPE,
    };
    use crate::container::InventorContainer;
    use crate::pmdc::{type_id_string, PmDcPairedReferenceList, PmDcReference};
    use crate::record_identity::Located;
    use crate::rse::{RecordFrameState, SegmentBulkState, SegmentKind};
    use crate::test_support::test_fixtures::primary_envelope_fixture;
    use crate::test_support::truncation::displayed_truncation;
    use cadmpeg_core::decode::{DecodeContext, View};
    use cadmpeg_ir::appearance::Appearance;
    use cadmpeg_ir::ids::{BodyId, FaceId};
    use cadmpeg_ir::topology::Color;

    fn inventory_with_record(
        kind: SegmentKind,
        type_id: [u8; 16],
        payload: &[u8],
        policy: DecodePolicy,
    ) -> Result<(usize, Vec<crate::record_issue::RecordIssue>), CodecError> {
        let bytes = primary_envelope_fixture();
        let payload = payload.to_vec();
        let arena = DecodeArena::new();
        let (setup_ctx, source) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
                .expect("envelope view");
        let mut container = InventorContainer::open(&setup_ctx, source).expect("framed envelope");
        let segment = &mut container.rse.segments[0];
        segment.kind = kind;
        let SegmentBulkState::Framed(bulk) = &mut segment.bulk else {
            panic!("framed bulk fixture");
        };
        let RecordFrameState::Framed(table) = &mut bulk.records else {
            panic!("framed record fixture");
        };
        table.records[0].type_id = type_id;
        table.records[0].payload = View::over_retained(&payload);
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("input view");
        let result = inventory(&ctx, &container.rse)?;
        Ok((
            result.default_styles.len()
                + result.rendering_styles.len()
                + result.graphics_faces.len()
                + result.graphics_style_collections.len()
                + result.graphics_primary_color_styles.len(),
            result.issues,
        ))
    }

    #[test]
    fn presentation_record_forms_refuse_collection_limit_before_push() {
        let mut default = default_style_fixture();
        default.truncate(default.len() - 8);
        let mut rendering = Vec::new();
        rendering.extend_from_slice(&0u32.to_le_bytes());
        rendering.extend_from_slice(&0u16.to_le_bytes());
        rendering.push(0);
        rendering.extend_from_slice(&[0; 2 + 4 + 4 + 4 + 4]);
        utf16(&mut rendering, "a");
        utf16(&mut rendering, "b");
        rendering.extend_from_slice(&0u16.to_le_bytes());
        for _ in 0..4 {
            utf16(&mut rendering, "");
        }
        rendering.extend_from_slice(&[0; 4 + 16]);
        let mut collection = Vec::new();
        collection.extend_from_slice(&2u16.to_le_bytes());
        collection.extend_from_slice(&0x3000u16.to_le_bytes());
        collection.extend_from_slice(&0u32.to_le_bytes());
        for (kind, type_id, payload, parser_items, operation) in [
            (
                SegmentKind::PmApp,
                DEFAULT_STYLE_TYPE,
                default,
                0,
                "admit Inventor default style record",
            ),
            (
                SegmentKind::PmApp,
                RENDERING_STYLE_TYPE,
                rendering,
                0,
                "admit Inventor rendering style record",
            ),
            (
                SegmentKind::PmGraphics,
                GRAPHICS_FACE_TYPE,
                graphics_face_fixture([1.0; 6]),
                2,
                "admit Inventor graphics face record",
            ),
            (
                SegmentKind::PmGraphics,
                GRAPHICS_STYLE_COLLECTION_TYPE,
                collection,
                0,
                "admit Inventor graphics style collection record",
            ),
            (
                SegmentKind::PmGraphics,
                GRAPHICS_PRIMARY_COLOR_STYLE_TYPE,
                primary_color_fixture([0.4; 4]),
                0,
                "admit Inventor graphics primary color style record",
            ),
        ] {
            let admitted =
                inventory_with_record(kind.clone(), type_id, &payload, DecodePolicy::service())
                    .expect("presentation record is admitted");
            assert_eq!(admitted.0, 1, "{operation}");
            assert!(admitted.1.is_empty());
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = parser_items;
            assert!(matches!(
                inventory_with_record(kind, type_id, &payload, policy),
                Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::CollectionItems
                        && limit.operation == operation
                        && limit.used == parser_items
            ));
        }
    }

    #[test]
    fn presentation_parse_issue_refuses_collection_limit_before_push() {
        assert_eq!(
            inventory_with_record(
                SegmentKind::PmApp,
                DEFAULT_STYLE_TYPE,
                &[],
                DecodePolicy::service()
            )
            .expect("truncated style becomes an issue")
            .1
            .len(),
            1
        );
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        assert!(matches!(
            inventory_with_record(SegmentKind::PmApp, DEFAULT_STYLE_TYPE, &[], policy),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "admit Inventor presentation issue"
                    && limit.used == 0
        ));
    }

    #[test]
    fn presentation_record_identity_refuses_retained_limits_before_copy() {
        let mut payload = default_style_fixture();
        payload.truncate(payload.len() - 8);
        for (limit_bytes, operation, used) in [
            (31, "retain Inventor presentation record type id", 0),
            (32, "retain Inventor presentation record segment token", 32),
        ] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = limit_bytes;
            assert!(matches!(
                inventory_with_record(SegmentKind::PmApp, DEFAULT_STYLE_TYPE, &payload, policy),
                Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::RetainedBytes
                        && limit.operation == operation
                        && limit.used == used
            ));
        }
    }

    #[test]
    fn presentation_issue_copies_refuse_retained_limits_before_creation() {
        let admitted = inventory_with_record(
            SegmentKind::PmApp,
            DEFAULT_STYLE_TYPE,
            &[],
            DecodePolicy::service(),
        )
        .expect("truncated style becomes an issue");
        let detail_len = admitted.1[0].detail.len();
        let token_len = admitted.1[0].segment_token.len();
        for (limit_bytes, operation, used) in [
            (
                detail_len - 1,
                "retain Inventor presentation issue detail",
                0,
            ),
            (
                detail_len + token_len - 1,
                "retain Inventor presentation issue token",
                detail_len,
            ),
        ] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = limit_bytes as u64;
            assert!(matches!(
                inventory_with_record(SegmentKind::PmApp, DEFAULT_STYLE_TYPE, &[], policy),
                Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::RetainedBytes
                        && limit.operation == operation
                        && limit.used == used as u64
            ));
        }
    }

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
        let extension = style
            .extension
            .as_ref()
            .expect("version 26 stores the rendering-style extension");
        assert_eq!(extension.style_label, "1:Steel");
        assert_eq!(extension.asset_guid, "d3c6130d-6c0f-4525-b268-53517ab46a78");
        assert_eq!(extension.material_id, "InvGen-066");
        assert_eq!(
            extension.asset_library_id,
            "afefc330-5e61-4e24-814f-ae810148b79d"
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
        let default = parse_default_style(&ctx, default_root, 26).expect("default parses");
        let style = parse_rendering_style(&ctx, style_root, 26).expect("style parses");
        let inventory = PresentationInventory {
            default_styles: vec![Located::new(
                default,
                type_id_string(DEFAULT_STYLE_TYPE),
                &cadmpeg_ir::identity_key!("segment"),
                0,
            )],
            rendering_styles: vec![Located::new(
                style,
                type_id_string(RENDERING_STYLE_TYPE),
                &cadmpeg_ir::identity_key!("segment"),
                8,
            )],
            graphics_faces: Vec::new(),
            graphics_style_collections: Vec::new(),
            graphics_primary_color_styles: Vec::new(),
            issues: Vec::new(),
        };
        let appearance = Appearance {
            id: AppearanceId::mint("inventor:test:appearance#1").expect("identity grammar"),
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

        let projection = project_default_bindings(
            &inventory,
            &[appearance],
            &[BodyId::mint("inventor:test:body#1").expect("identity grammar")],
        );

        assert_eq!(projection.unresolved_defaults, 0);
        let [binding] = projection.bindings.as_slice() else {
            panic!("one body binding must be projected");
        };
        assert_eq!(
            binding.appearance,
            AppearanceId::mint("inventor:test:appearance#1").expect("identity grammar")
        );
        assert_eq!(
            binding.target,
            AppearanceTarget::Body(BodyId::mint("inventor:test:body#1").expect("identity grammar"))
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

    /// A current graphics-face record with qualified references and `bounds`.
    fn graphics_face_fixture(bounds: [f64; 6]) -> Vec<u8> {
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
        for value in bounds {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(15_u32.to_le_bytes());
        bytes.extend(16_u32.to_le_bytes());
        bytes.extend(17_u32.to_le_bytes());
        bytes
    }

    #[test]
    fn parses_current_graphics_face_with_qualified_references() {
        let bytes = graphics_face_fixture([1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic graphics face fits policy");

        let face = parse_graphics_face(&ctx, root, 26).expect("graphics face parses");

        assert_eq!(face.styles.index, 7);
        assert!(face.styles.qualified);
        assert_eq!(face.surface.index, 8);
        assert!(face.surface.qualified);
        assert_eq!(face.parent.index, 9);
        assert!(face.parent.qualified);
        assert_eq!(
            face.edge_references.references(),
            [
                PmDcReference {
                    index: 13,
                    qualified: true
                },
                PmDcReference {
                    index: 14,
                    qualified: true
                }
            ]
        );
        assert_eq!(face.edge_references.metadata().copied(), Some([11, 12]));
        assert_eq!(
            FiniteReal::raw_array(face.bounds),
            [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]
        );
        assert_eq!(face.key, 15);
        assert_eq!(face.values, [16, 17]);
    }

    #[test]
    fn a_non_finite_graphics_face_bound_is_refused() {
        let bytes = graphics_face_fixture([1.0, 2.0, f64::NAN, 4.0, 5.0, 6.0]);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic graphics face fits policy");

        let error = parse_graphics_face(&ctx, root, 26).expect_err("a NaN bound is refused");
        assert_eq!(
            error.to_string(),
            "malformed container: Inventor presentation graphics-face bound is not finite"
        );
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

        assert_eq!(
            styles.style_references.references(),
            [
                PmDcReference {
                    index: 23,
                    qualified: true
                },
                PmDcReference {
                    index: 24,
                    qualified: false
                }
            ]
        );
        assert_eq!(styles.style_references.metadata().copied(), Some([21, 22]));
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
        let face = Located::new(
            PmGraphicsFace {
                segment_version_major: 26,
                header_value: 0,
                header_id: 0,
                flags: 0,
                styles: PmDcReference {
                    index: 5,
                    qualified: true,
                },
                surface: PmDcReference {
                    index: 0,
                    qualified: false,
                },
                parent: PmDcReference {
                    index: 0,
                    qualified: false,
                },
                state: 0,
                edge_references: PmDcPairedReferenceList::default(),
                visibility_state: 0,
                bounds: [FiniteReal::ZERO; 6],
                key: 42,
                values: [0; 2],
            },
            type_id_string(GRAPHICS_FACE_TYPE),
            &cadmpeg_ir::identity_key!("graphics"),
            2,
        );
        let collection = Located::new(
            PmGraphicsStyleCollection {
                segment_version_major: 26,
                style_references: PmDcPairedReferenceList::new(
                    Some([1, 2]),
                    vec![PmDcReference {
                        index: 7,
                        qualified: true,
                    }],
                )
                .expect("valid reference list"),
            },
            type_id_string(GRAPHICS_STYLE_COLLECTION_TYPE),
            &cadmpeg_ir::identity_key!("graphics"),
            4,
        );
        let style = Located::new(
            PmGraphicsPrimaryColorStyle {
                segment_version_major: 26,
                header_value: 0,
                controls: [0; 7],
                color_header: [0; 2],
                colors: [[0.0; 4], [0.2, 0.4, 0.6, 0.8], [0.0; 4], [0.0; 4]],
                color_tail: [0; 2],
                state: 0,
                values: [0; 2],
                terminal_state: 0,
            },
            type_id_string(GRAPHICS_PRIMARY_COLOR_STYLE_TYPE),
            &cadmpeg_ir::identity_key!("graphics"),
            6,
        );
        let inventory = PresentationInventory {
            default_styles: Vec::new(),
            rendering_styles: Vec::new(),
            graphics_faces: vec![face],
            graphics_style_collections: vec![collection],
            graphics_primary_color_styles: vec![style],
            issues: Vec::new(),
        };
        let face_id = FaceId::mint("inventor:test:face#1").expect("identity grammar");
        let face_keys = std::collections::HashMap::from([(face_id.clone(), 42)]);

        let projection = project_bindings(&inventory, &[], &[], &face_keys);

        assert!(projection.unresolved_face_overrides.is_empty());
        let [appearance] = projection.appearances.as_slice() else {
            panic!("one primary-color appearance must be projected");
        };
        assert_eq!(
            appearance.base_color,
            Some(Color::new(0.2, 0.4, 0.6, 0.8).expect("valid color"))
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

    #[test]
    fn truncated_presentation_reads_name_the_field() {
        let empty = &[];
        for (field, text) in [
            (
                "graphics primary-color state",
                displayed_truncation(
                    Cursor::new(View::over_retained(empty)).u8("graphics primary-color state"),
                ),
            ),
            (
                "graphics-face header id",
                displayed_truncation(
                    Cursor::new(View::over_retained(empty)).u16("graphics-face header id"),
                ),
            ),
            (
                "graphics-face key",
                displayed_truncation(
                    Cursor::new(View::over_retained(empty)).u32("graphics-face key"),
                ),
            ),
            (
                "graphics-face bound",
                displayed_truncation(
                    Cursor::new(View::over_retained(empty)).finite_f64("graphics-face bound"),
                ),
            ),
            (
                "graphics primary-color component",
                displayed_truncation(
                    Cursor::new(View::over_retained(empty)).f32("graphics primary-color component"),
                ),
            ),
            (
                "graphics-face legacy object padding",
                displayed_truncation(
                    Cursor::new(View::over_retained(empty))
                        .take(4, "graphics-face legacy object padding"),
                ),
            ),
            (
                "default-style suffix padding",
                displayed_truncation(
                    Cursor::new(View::over_retained(empty))
                        .zeroes(8, "default-style suffix padding"),
                ),
            ),
            (
                "default-style material reference",
                displayed_truncation(
                    Cursor::new(View::over_retained(empty))
                        .reference("default-style material reference"),
                ),
            ),
            (
                "graphics-face styles reference",
                displayed_truncation(
                    Cursor::new(View::over_retained(empty))
                        .node_reference("graphics-face styles reference"),
                ),
            ),
            (
                "rendering-style guid",
                displayed_truncation(
                    Cursor::new(View::over_retained(empty)).guid("rendering-style guid"),
                ),
            ),
        ] {
            assert_eq!(
                text,
                format!("truncated input during {field} at space 0 offset 0")
            );
        }
    }

    #[test]
    fn a_truncated_graphics_record_names_the_field_it_stopped_in() {
        let mut bytes = Vec::new();
        bytes.extend(31_u32.to_le_bytes());
        bytes.extend(32_u16.to_le_bytes());
        assert_eq!(
            displayed_truncation(parse_graphics_primary_color_style(
                View::over_retained(&bytes),
                26
            )),
            "truncated input during graphics primary-color control at space 0 offset 6"
        );
    }
}
