// SPDX-License-Identifier: Apache-2.0
//! Parse typed Design body-presentation and browser-node records.

use crate::bytes::{lp_utf16_bounded_charged, lp_utf16_bounded_scoped};
use cadmpeg_core::decode::u64_from_index;

use std::collections::HashMap;

use cadmpeg_core::decode::{DecodeContext, ScopedReservation, View};
use cadmpeg_core::CodecError;

use crate::design::decode::byte_fields::{bytes_at, zeros_at};
use crate::design::decode::reference_runs::admit_reference_values;

use crate::bytes::{is_guid_prefix, lp_utf16_bytes, take_reference};
use crate::design::decode::meta::{guid_matches, has_base_type, TypedFrameSource};
use crate::design::decode::sketch::{
    parse_genesis_entity_header, parse_settled_entity_header, NamedEntityHeader,
};

use crate::design::presentation::{
    is_physical_material_token, APPEARANCE_LIBRARY_ID, BODY_PRESENTATION_BASE_TYPE_GUID,
    BODY_PRESENTATION_MATERIAL_ENVELOPE_ID, BODY_PRESENTATION_TYPE_GUID,
    BODY_PRESENTATION_TYPE_VERSION, BODY_SCENE_NODE_TYPE_GUID, BODY_SCENE_NODE_TYPE_VERSION,
    BREP_CONTAINER_TYPE_GUID, BREP_CONTAINER_TYPE_VERSION, BROWSER_NODE_BASE_TYPE_GUID,
    BROWSER_NODE_TYPE_GUID, BROWSER_NODE_TYPE_VERSION, GUID_LEN, MODERN_APPEARANCE_LIBRARY_IDS,
    PHYSICAL_MATERIAL_LIBRARY_ID,
};
use crate::records::entity_header::{DESIGN_MODULE_BODY, DESIGN_MODULE_FUSION};

const MAX_ENVELOPE_GAP: usize = 8;

/// The registered type GUID and record version of one Design entity.
type EntityType<'a> = (&'a crate::records::mesh::DesignRelaxedGuidText, u32);

/// One typed browser-node record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BrowserNodeRecord {
    pub(super) record_index: u32,
    guid: String,
    pub(super) entity_suffix: u64,
    pub(super) hidden_offset: u64,
    pub(super) hidden: bool,
}

/// Material members of one body-presentation envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PresentationMaterial {
    node_guid: String,
    pub(crate) physical_token: String,
    pub(crate) physical_token_offset: u64,
    pub(crate) visual_guid: crate::records::references::DesignVisualToken,
    pub(crate) visual_guid_offset: u64,
    pub(crate) visual_preset: Option<crate::records::identity::Located<String>>,
}

/// One body record, its exact owner header, and its browser-node join.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BodyPresentation {
    pub(crate) byte_offset: u64,
    pub(crate) entity_suffix: u64,
    pub(crate) owner: BodyPresentationOwner,
    pub(crate) browser_node: Option<BrowserNodeRecord>,
    pub(crate) material: Option<PresentationMaterial>,
}

/// Entity identity form stored by one body-presentation owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BodyPresentationOwner {
    /// The owner stores a component-qualified entity ID after its entity
    /// suffix.
    Named {
        entity_id: crate::records::identity::DesignEntityId,
        entity_id_offset: u64,
    },
    /// The owner stores only its u64 entity suffix in the indexed head.
    Bare,
}

/// Decode every browser node whose dynamic class resolves to the registered
/// browser-node type. The record body is ten zero bytes, an LP-UTF16 GUID, the
/// hidden flag, `01 01`, and the owning Design entity suffix.
pub(super) fn browser_node_records(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
) -> Result<Vec<BrowserNodeRecord>, CodecError> {
    browser_nodes_in(ctx, bytes, &mut TypedFrameSource::new(bytes, meta))
}

/// The browser nodes among the typed frames of `source`, whose stream is
/// `bytes`.
fn browser_nodes_in<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &[u8],
    source: &mut TypedFrameSource<'_, '_, 'ctx>,
) -> Result<Vec<BrowserNodeRecord>, CodecError> {
    let mut out = Vec::new();
    let (frames, _frames_storage) = source.frames(ctx, BROWSER_NODE_TYPE_GUID, "browser-node")?;
    for frame in ctx.admit_iter(&frames, "scan F3D browser-node frames")? {
        if frame.design_type.version != BROWSER_NODE_TYPE_VERSION {
            continue;
        }
        if frame.design_type.module != DESIGN_MODULE_FUSION
            || !has_base_type(frame.design_type, BROWSER_NODE_BASE_TYPE_GUID)
        {
            return Err(crate::design::text::malformed_design(
                ctx,
                format_args!(
                    "F3D Design browser-node entity {} has incompatible registration metadata",
                    frame.entity_id
                ),
            ));
        }
        let record = &bytes[frame.start..frame.end];
        let record_index = View::u32_le_at(record, 7).ok_or_else(|| {
            crate::design::text::malformed_design(
                ctx,
                format_args!(
                    "F3D Design browser-node entity {} has a truncated record index",
                    frame.entity_id
                ),
            )
        })?;
        if u64::from(record_index) != frame.entity_id || !zeros_at::<10>(record, 11) {
            return Err(crate::design::text::malformed_design(
                ctx,
                format_args!(
                    "F3D Design browser-node entity {} has an invalid header",
                    frame.entity_id
                ),
            ));
        }
        let Some((guid, after_guid)) = lp_utf16_bounded_charged(
            ctx,
            record,
            21,
            GUID_LEN..=GUID_LEN,
            "f3d Design UTF-16 text",
        )?
        .filter(|(guid, after)| {
            is_guid_prefix(guid)
                && after
                    .checked_add(11)
                    .is_some_and(|record_end| record_end <= record.len())
                && bytes_at::<2>(record, *after + 1) == Some(&[0x01, 0x01])
        }) else {
            continue;
        };
        let hidden @ (0 | 1) = record.get(after_guid).copied().ok_or_else(|| {
            CodecError::Malformed("F3D Design browser-node flag is truncated".into())
        })?
        else {
            return Err(crate::design::text::malformed_design(
                ctx,
                format_args!(
                    "F3D Design browser-node entity {} has an invalid hidden flag",
                    frame.entity_id
                ),
            ));
        };
        let entity_suffix = View::u64_le_at(record, after_guid + 3).ok_or_else(|| {
            CodecError::Malformed("F3D Design browser-node suffix is truncated".into())
        })?;

        ctx.push_vec(
            &mut out,
            BrowserNodeRecord {
                record_index,
                guid,
                entity_suffix,
                hidden_offset: u64_from_index(frame.start + after_guid),
                hidden: hidden == 1,
            },
            "f3d browser node records",
        )?;
    }
    Ok(out)
}

/// The only browser node owned by `entity_suffix` whose GUID equals
/// `node_guid` without ASCII case. The search stops at a second match; each
/// GUID comparison reads at most the 36 bytes of a node GUID.
fn unique_browser_node<'nodes>(
    ctx: &DecodeContext<'_>,
    nodes: &'nodes [BrowserNodeRecord],
    entity_suffix: u64,
    node_guid: &str,
) -> Result<Option<&'nodes BrowserNodeRecord>, CodecError> {
    let operation = "find F3D body presentation browser node";
    let matches = |node: &BrowserNodeRecord| -> Result<bool, CodecError> {
        Ok(node.entity_suffix == entity_suffix && node.guid.eq_ignore_ascii_case(node_guid))
    };
    let Some(first) = ctx.position_by(nodes, matches, operation)? else {
        return Ok(None);
    };
    if ctx.any_by(nodes.get(first + 1..).unwrap_or(&[]), matches, operation)? {
        return Ok(None);
    }
    Ok(nodes.get(first))
}

/// Decode every typed body presentation in one Design stream. The browser
/// nodes and the entity type index are held only while the stream decodes.
pub(crate) fn body_presentations(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
) -> Result<Vec<BodyPresentation>, CodecError> {
    let mut source = TypedFrameSource::new(bytes, meta);
    let (nodes, _nodes_storage) = ctx.with_scoped_storage("f3d browser node records", || {
        browser_nodes_in(ctx, bytes, &mut source)
    })?;
    let (entity_types, _entity_types_storage) =
        ctx.with_scoped_storage("f3d presentation entity types", || entity_types(ctx, meta))?;

    let mut out = Vec::new();
    let (frames, _frames_storage) =
        source.frames(ctx, BODY_PRESENTATION_TYPE_GUID, "body-presentation")?;
    for frame in ctx.admit_iter(&frames, "scan F3D body-presentation frames")? {
        if frame.design_type.version != BODY_PRESENTATION_TYPE_VERSION {
            continue;
        }
        if frame.design_type.module != DESIGN_MODULE_BODY
            || !has_base_type(frame.design_type, BODY_PRESENTATION_BASE_TYPE_GUID)
        {
            return Err(crate::design::text::malformed_design(
                ctx,
                format_args!(
                    "F3D Design body-presentation entity {} has incompatible registration metadata",
                    frame.entity_id
                ),
            ));
        }
        let framed_bytes = &bytes[..frame.end];
        let named_header = match parse_settled_entity_header(ctx, framed_bytes, frame.start)? {
            Some(header) => Some(header),
            None => parse_genesis_entity_header(ctx, framed_bytes, frame.start)?,
        };
        let (entity_suffix, owner, material) = if let Some(NamedEntityHeader {
            entity_id,
            entity_id_offset,
            end: header_end,
            ..
        }) = named_header
        {
            let entity_suffix = entity_id.suffix();
            if entity_suffix != frame.entity_id {
                return Err(crate::design::text::malformed_design(ctx, format_args!(
                    "F3D Design body-presentation entity {} disagrees with its named header entity {entity_suffix}",
                    frame.entity_id
                )));
            }
            (
                entity_suffix,
                BodyPresentationOwner::Named {
                    entity_id,
                    entity_id_offset: u64_from_index(entity_id_offset),
                },
                presentation_material(
                    ctx,
                    framed_bytes,
                    header_end,
                    frame.end,
                    entity_suffix,
                    &entity_types,
                )?,
            )
        } else {
            let entity_suffix =
                View::u64_le_at(framed_bytes, frame.start + 7).ok_or_else(|| {
                    crate::design::text::malformed_design(
                        ctx,
                        format_args!(
                            "F3D Design bare body-presentation entity {} has a truncated head",
                            frame.entity_id
                        ),
                    )
                })?;
            if entity_suffix == 0 || entity_suffix != frame.entity_id {
                return Err(crate::design::text::malformed_design(
                    ctx,
                    format_args!(
                    "F3D Design bare body-presentation entity {} has head entity {entity_suffix}",
                    frame.entity_id
                ),
                ));
            }
            let Some(material) = bare_presentation_material(
                ctx,
                framed_bytes,
                frame.start + 15,
                frame.end,
                entity_suffix,
            )?
            else {
                continue;
            };
            (entity_suffix, BodyPresentationOwner::Bare, Some(material))
        };
        let browser_node = match material.as_ref() {
            Some(material) => {
                match unique_browser_node(ctx, &nodes, entity_suffix, &material.node_guid)? {
                    Some(node) => Some(copy_browser_node(ctx, node)?),
                    None => None,
                }
            }
            None => None,
        };

        ctx.push_vec(
            &mut out,
            BodyPresentation {
                byte_offset: u64_from_index(frame.start),
                entity_suffix,
                owner,
                browser_node,
                material,
            },
            "f3d body presentation records",
        )?;
    }
    Ok(out)
}

fn copy_browser_node(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    node: &BrowserNodeRecord,
) -> Result<BrowserNodeRecord, CodecError> {
    let guid = ctx.copy_retained_text(&node.guid, "f3d body presentation browser node GUID")?;
    Ok(BrowserNodeRecord {
        record_index: node.record_index,
        guid,
        entity_suffix: node.entity_suffix,
        hidden_offset: node.hidden_offset,
        hidden: node.hidden,
    })
}

fn entity_types<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    meta: &'a crate::metastream::MetaStream,
) -> Result<HashMap<u64, EntityType<'a>>, CodecError> {
    let mut out = HashMap::new();
    for design_type in ctx.admit_iter(&meta.types, "scan F3D presentation entity types")? {
        for entity_id in
            admit_reference_values(ctx, &design_type.entities, "scan F3D presentation entities")?
                .copied()
        {
            if ctx
                .insert_hash_map(
                    &mut out,
                    entity_id,
                    (&design_type.type_guid, design_type.version),
                    "f3d presentation entity types",
                )?
                .is_some()
            {
                return Err(crate::design::text::malformed_design(
                    ctx,
                    format_args!("F3D Design entity {entity_id} has multiple registered types"),
                ));
            }
        }
    }
    Ok(out)
}

/// Whether `entity` is registered with the type `type_guid` (compared
/// without ASCII case) at `version`.
fn entity_has_type(
    ctx: &DecodeContext<'_>,
    entity_types: &HashMap<u64, EntityType<'_>>,
    entity: u64,
    type_guid: &str,
    version: u32,
) -> Result<bool, CodecError> {
    let Some((guid, registered_version)) =
        ctx.get_hash_map(entity_types, &entity, "find F3D presentation entity type")?
    else {
        return Ok(false);
    };
    Ok(*registered_version == version && guid_matches(guid, type_guid))
}

/// The one material envelope after a named body-presentation header in
/// `start..end`. Each envelope candidate is parsed under its own scoped
/// reservation; only the candidate that is returned becomes retained.
fn presentation_material(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    end: usize,
    entity_suffix: u64,
    entity_types: &HashMap<u64, EntityType<'_>>,
) -> Result<Option<PresentationMaterial>, CodecError> {
    let Some(bytes) = bytes.get(..end) else {
        return Ok(None);
    };
    let markers = MaterialMarkers::new()?;
    let legacy_marker = lp_utf16_bytes(APPEARANCE_LIBRARY_ID)?;
    let envelope = PresentationEnvelope {
        bytes,
        start,
        end,
        entity_suffix,
    };
    let mut candidate = None;
    let mut search_at = start;
    while let Some(physical_at) = next_match(
        ctx,
        bytes,
        search_at,
        end,
        &[&markers.physical],
        "find F3D physical material marker",
    )? {
        search_at = physical_at + markers.physical.len();
        let (material, storage) = ctx
            .with_scoped_storage("f3d body presentation material candidates", || {
                envelope.named_material_at(ctx, &markers, &legacy_marker, entity_types, physical_at)
            })?;
        let Some(material) = material else {
            continue;
        };
        if candidate.is_some() {
            return Ok(None);
        }
        candidate = Some((material, storage));
    }
    let Some((material, storage)) = candidate else {
        return Ok(None);
    };
    storage.commit()?;
    Ok(Some(material))
}

/// The counted UTF-16LE library identifiers both material envelope forms
/// store.
struct MaterialMarkers {
    physical: Vec<u8>,
    modern: Vec<u8>,
    modern_trailer: Vec<u8>,
}

impl MaterialMarkers {
    fn new() -> Result<Self, CodecError> {
        Ok(Self {
            physical: lp_utf16_bytes(PHYSICAL_MATERIAL_LIBRARY_ID)?,
            modern: lp_utf16_bytes(MODERN_APPEARANCE_LIBRARY_IDS[0])?,
            modern_trailer: lp_utf16_bytes(MODERN_APPEARANCE_LIBRARY_IDS[1])?,
        })
    }
}

/// The bytes of one body-presentation frame that hold its material envelope:
/// `bytes[start..end]`, owned by the entity `entity_suffix`.
struct PresentationEnvelope<'a> {
    bytes: &'a [u8],
    start: usize,
    end: usize,
    entity_suffix: u64,
}

impl PresentationEnvelope<'_> {
    /// The material envelope of a named owner whose physical-library marker
    /// opens at `physical_at`. The visual record ends with `legacy_marker` or
    /// with the modern marker pair.
    fn named_material_at(
        &self,
        ctx: &DecodeContext<'_>,
        markers: &MaterialMarkers,
        legacy_marker: &[u8],
        entity_types: &HashMap<u64, EntityType<'_>>,
        physical_at: usize,
    ) -> Result<Option<PresentationMaterial>, CodecError> {
        let Self {
            bytes,
            start,
            end,
            entity_suffix,
        } = *self;
        let Some((physical_guid_at, physical_guid, _physical_guid_storage)) =
            preceding_lp_utf16(ctx, bytes, start, physical_at)?
        else {
            return Ok(None);
        };
        let Some(node_tail_at) = physical_guid_at.checked_sub(11) else {
            return Ok(None);
        };
        if bytes_at::<11>(bytes, node_tail_at) != Some(&[1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]) {
            return Ok(None);
        }
        let Some((_, node_guid, _node_guid_storage)) =
            preceding_lp_utf16(ctx, bytes, start, node_tail_at)?
        else {
            return Ok(None);
        };
        if physical_guid.len() != GUID_LEN
            || node_guid.len() != GUID_LEN
            || !is_guid_prefix(&physical_guid)
            || !is_guid_prefix(&node_guid)
        {
            return Ok(None);
        }
        let Some(token_at) = skip_zeros(bytes, physical_at + markers.physical.len(), end) else {
            return Ok(None);
        };
        let Some((physical_token, after_token)) =
            lp_utf16_bounded_charged(ctx, bytes, token_at, 1..=256, "f3d Design UTF-16 text")?
        else {
            return Ok(None);
        };
        if !is_physical_material_token(&physical_token) || after_token > end {
            return Ok(None);
        }
        let mut reference_at = after_token;
        let Some(brep_container_entity) = local_reference(ctx, bytes, &mut reference_at)? else {
            return Ok(None);
        };
        if local_reference_value(ctx, bytes, &mut reference_at)? != Some(LocalReference::Null) {
            return Ok(None);
        }
        let Some(scene_node_entity) = local_reference(ctx, bytes, &mut reference_at)? else {
            return Ok(None);
        };
        if entity_suffix.checked_add(1) != Some(scene_node_entity)
            || !entity_has_type(
                ctx,
                entity_types,
                brep_container_entity,
                BREP_CONTAINER_TYPE_GUID,
                BREP_CONTAINER_TYPE_VERSION,
            )?
            || !entity_has_type(
                ctx,
                entity_types,
                scene_node_entity,
                BODY_SCENE_NODE_TYPE_GUID,
                BODY_SCENE_NODE_TYPE_VERSION,
            )?
        {
            return Ok(None);
        }
        let Some((_, after_name, _name_storage)) =
            lp_utf16_bounded_scoped(ctx, bytes, reference_at, 0..=256, "f3d Design UTF-16 text")?
        else {
            return Ok(None);
        };
        let Some(visual_at) = record_tail_visual_offset(bytes, after_name, end) else {
            return Ok(None);
        };
        let Some((visual_guid, after_visual)) =
            lp_utf16_bounded_charged(ctx, bytes, visual_at, 1..=256, "f3d Design UTF-16 text")?
        else {
            return Ok(None);
        };
        let Ok(visual_guid) = crate::records::references::DesignVisualToken::try_from(visual_guid)
        else {
            return Ok(None);
        };
        let Some(visual_marker_at) = skip_zeros(bytes, after_visual, end) else {
            return Ok(None);
        };
        let (after_visual_marker, legacy) =
            if needle_at(bytes, visual_marker_at, end, &[legacy_marker]) {
                (visual_marker_at + legacy_marker.len(), true)
            } else if needle_at(bytes, visual_marker_at, end, &[&markers.modern]) {
                let Some(trailer_at) =
                    skip_zeros(bytes, visual_marker_at + markers.modern.len(), end)
                else {
                    return Ok(None);
                };
                if !needle_at(bytes, trailer_at, end, &[&markers.modern_trailer]) {
                    return Ok(None);
                }
                (trailer_at + markers.modern_trailer.len(), false)
            } else {
                return Ok(None);
            };
        let visual_preset = match skip_zeros(bytes, after_visual_marker, end) {
            Some(at) if legacy => {
                lp_utf16_bounded_charged(ctx, bytes, at, 1..=256, "f3d Design UTF-16 text")?
                    .and_then(|(value, _)| value.starts_with("Prism-").then_some((at, value)))
            }
            _ => None,
        };
        let node_guid =
            ctx.copy_retained_text(&node_guid, "f3d body presentation material node GUID")?;
        Ok(Some(PresentationMaterial {
            node_guid,
            physical_token,
            physical_token_offset: u64_from_index(token_at + 4),
            visual_guid,
            visual_guid_offset: u64_from_index(visual_at + 4),
            visual_preset: visual_preset.map(|(at, value)| crate::records::identity::Located {
                value,
                offset: u64_from_index(at + 4),
            }),
        }))
    }

    /// The material envelope of a bare owner whose envelope and
    /// physical-library markers open at `marker_at`.
    fn bare_material_at(
        &self,
        ctx: &DecodeContext<'_>,
        markers: &MaterialMarkers,
        marker_at: usize,
        marker_len: usize,
    ) -> Result<Option<PresentationMaterial>, CodecError> {
        let Self {
            bytes,
            end,
            entity_suffix,
            ..
        } = *self;
        let Some(token_at) = skip_zeros(bytes, marker_at + marker_len, end) else {
            return Ok(None);
        };
        let Some((physical_token, after_token)) =
            lp_utf16_bounded_charged(ctx, bytes, token_at, 1..=256, "f3d Design UTF-16 text")?
        else {
            return Ok(None);
        };
        if !is_physical_material_token(&physical_token) {
            return Ok(None);
        }

        let mut physical_reference_at = after_token;
        if local_reference(ctx, bytes, &mut physical_reference_at)?.is_none() {
            return Ok(None);
        }
        let Some(node_guid_at) = skip_zeros(bytes, physical_reference_at, end) else {
            return Ok(None);
        };
        let Some((node_guid, after_node_guid)) = lp_utf16_bounded_charged(
            ctx,
            bytes,
            node_guid_at,
            GUID_LEN..=GUID_LEN,
            "f3d Design UTF-16 text",
        )?
        else {
            return Ok(None);
        };
        if !is_guid_prefix(&node_guid) {
            return Ok(None);
        }
        let mut node_reference_at = after_node_guid;
        let Some(node_entity) = local_reference(ctx, bytes, &mut node_reference_at)? else {
            return Ok(None);
        };
        if entity_suffix.checked_add(1) != Some(node_entity) {
            return Ok(None);
        }

        // The visual record follows the node reference directly or after an
        // optional name; both readings must agree on its offset.
        let after_name = match skip_zeros(bytes, node_reference_at, end) {
            Some(name_at) => {
                lp_utf16_bounded_scoped(ctx, bytes, name_at, 1..=256, "f3d Design UTF-16 text")?
                    .map(|(_, name_end, _name_storage)| name_end)
            }
            None => None,
        };
        let direct_visual_at = record_tail_visual_offset(bytes, node_reference_at, end);
        let named_visual_at =
            after_name.and_then(|name_end| record_tail_visual_offset(bytes, name_end, end));
        let visual_at = match (direct_visual_at, named_visual_at) {
            (Some(direct), Some(named)) if direct != named => return Ok(None),
            (Some(visual_at), _) | (None, Some(visual_at)) => visual_at,
            (None, None) => return Ok(None),
        };
        let Some((visual_guid, after_visual)) =
            lp_utf16_bounded_charged(ctx, bytes, visual_at, 1..=256, "f3d Design UTF-16 text")?
        else {
            return Ok(None);
        };
        let Ok(visual_guid) = crate::records::references::DesignVisualToken::try_from(visual_guid)
        else {
            return Ok(None);
        };
        let Some(visual_marker_at) = skip_zeros(bytes, after_visual, end) else {
            return Ok(None);
        };
        if !needle_at(bytes, visual_marker_at, end, &[&markers.modern]) {
            return Ok(None);
        }
        let Some(trailer_at) = skip_zeros(bytes, visual_marker_at + markers.modern.len(), end)
        else {
            return Ok(None);
        };
        if !needle_at(bytes, trailer_at, end, &[&markers.modern_trailer]) {
            return Ok(None);
        }
        Ok(Some(PresentationMaterial {
            node_guid,
            physical_token,
            physical_token_offset: u64_from_index(token_at + 4),
            visual_guid,
            visual_guid_offset: u64_from_index(visual_at + 4),
            visual_preset: None,
        }))
    }
}

/// Parse the material envelope of a body-presentation owner whose indexed
/// head stores no component-qualified entity ID. Each envelope candidate is
/// parsed under its own scoped reservation; only the candidate that is
/// returned becomes retained.
fn bare_presentation_material(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    end: usize,
    entity_suffix: u64,
) -> Result<Option<PresentationMaterial>, CodecError> {
    let Some(bytes) = bytes.get(..end) else {
        return Ok(None);
    };
    let envelope_marker = lp_utf16_bytes(BODY_PRESENTATION_MATERIAL_ENVELOPE_ID)?;
    let markers = MaterialMarkers::new()?;
    let marker: [&[u8]; 2] = [&envelope_marker, &markers.physical];
    let marker_len = envelope_marker.len() + markers.physical.len();
    let envelope = PresentationEnvelope {
        bytes,
        start,
        end,
        entity_suffix,
    };
    let mut candidate = None;
    let mut search_at = start;
    while let Some(marker_at) = next_match(
        ctx,
        bytes,
        search_at,
        end,
        &marker,
        "find F3D bare material marker",
    )? {
        search_at = marker_at + marker_len;
        let (material, storage) = ctx
            .with_scoped_storage("f3d body presentation material candidates", || {
                envelope.bare_material_at(ctx, &markers, marker_at, marker_len)
            })?;
        let Some(material) = material else {
            continue;
        };
        if candidate.is_some() {
            return Ok(None);
        }
        candidate = Some((material, storage));
    }
    let Some((material, storage)) = candidate else {
        return Ok(None);
    };
    storage.commit()?;
    Ok(Some(material))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocalReference {
    Null,
    Target(u64),
}

fn local_reference_value(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    at: &mut usize,
) -> Result<Option<LocalReference>, CodecError> {
    let Some(reference) = take_reference(ctx, bytes, at)? else {
        return Ok(None);
    };
    Ok(match reference {
        crate::bytes::Reference::Null => Some(LocalReference::Null),
        crate::bytes::Reference::Local { target, .. } => Some(LocalReference::Target(target)),
        _ => None,
    })
}

fn local_reference(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    at: &mut usize,
) -> Result<Option<u64>, CodecError> {
    Ok(match local_reference_value(ctx, bytes, at)? {
        Some(LocalReference::Target(target)) if target != 0 => Some(target),
        Some(LocalReference::Null | LocalReference::Target(_)) | None => None,
    })
}

/// The offset of the visual GUID after a record tail that starts at
/// `name_end`: a `01 01` marker within the next forty bytes, preceded only by
/// zero padding or by zero padding and an opacity of one, then up to twelve
/// zero bytes. The test reads a constant number of bytes.
fn record_tail_visual_offset(bytes: &[u8], name_end: usize, end: usize) -> Option<usize> {
    const OPACITY_ONE: [u8; 4] = 1.0f32.to_le_bytes();
    let tail = bytes.get(name_end..end)?;
    for relative_marker_at in 0..tail.len().min(40) {
        let marker_at = name_end + relative_marker_at;
        if bytes_at::<2>(bytes, marker_at) != Some(&[0x01, 0x01]) {
            continue;
        }
        let gap = &tail[..relative_marker_at];
        let zeros_only = gap.iter().all(|byte| *byte == 0);
        let opacity_tail = gap
            .strip_suffix(&OPACITY_ONE)
            .is_some_and(|padding| padding.iter().all(|byte| *byte == 0));
        if zeros_only || opacity_tail {
            return skip_zeros_capped(bytes, marker_at + 2, end, 12);
        }
    }
    None
}

/// Whether the concatenation of `needle` occurs at `at` within `..end`. The
/// needle is constant, so the test reads a constant number of bytes.
fn needle_at(bytes: &[u8], at: usize, end: usize, needle: &[&[u8]]) -> bool {
    let mut cursor = at;
    needle.iter().all(|part| {
        let Some(part_end) = cursor
            .checked_add(part.len())
            .filter(|part_end| *part_end <= end)
        else {
            return false;
        };
        let matches = bytes.get(cursor..part_end) == Some(*part);
        cursor = part_end;
        matches
    })
}

/// The first offset in `from..end` at which the concatenation of the
/// non-empty constant `needle` occurs. Each byte the search visits is
/// admitted once before its test, so a scan that resumes past each match pays
/// for the bytes it reads and no more.
fn next_match(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    from: usize,
    end: usize,
    needle: &[&[u8]],
    operation: &'static str,
) -> Result<Option<usize>, CodecError> {
    let Some(&first) = needle.iter().find_map(|part| part.first()) else {
        return Ok(None);
    };
    let mut cursor = from;
    loop {
        let Some(range) = bytes.get(cursor..end) else {
            return Ok(None);
        };
        let Some(relative) = ctx.position_by(range, |byte| Ok(*byte == first), operation)? else {
            return Ok(None);
        };
        // `relative` indexes `range`, so the sum stays within `end`.
        let at = cursor + relative;
        if needle_at(bytes, at, end, needle) {
            return Ok(Some(at));
        }
        cursor = at + 1;
    }
}

fn skip_zeros(bytes: &[u8], start: usize, end: usize) -> Option<usize> {
    skip_zeros_capped(bytes, start, end, MAX_ENVELOPE_GAP)
}

fn skip_zeros_capped(bytes: &[u8], start: usize, end: usize, cap: usize) -> Option<usize> {
    let mut at = start;
    while at < end && at - start < cap && bytes.get(at) == Some(&0) {
        at += 1;
    }
    (at <= end && (at == end || bytes.get(at) != Some(&0))).then_some(at)
}

/// The only counted UTF-16 text in `start..marker_at` that ends at most
/// `MAX_ENVELOPE_GAP` zero bytes before `marker_at`, with its offset. The
/// text is held under a scoped reservation; callers copy what they keep.
/// Candidate counts are tried for each padding width, so the test of a
/// candidate header reads a constant number of bytes; each complete candidate
/// is decoded under the work budget.
fn preceding_lp_utf16<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    marker_at: usize,
) -> Result<Option<(usize, String, ScopedReservation<'ctx>)>, CodecError> {
    let mut candidate = None;
    for gap in 0..=MAX_ENVELOPE_GAP {
        let Some(end) = marker_at.checked_sub(gap) else {
            continue;
        };
        if end < start {
            continue;
        }
        let Some(padding) = bytes.get(end..marker_at) else {
            continue;
        };
        if padding.iter().any(|byte| *byte != 0) {
            continue;
        }
        // Larger counts start earlier, so the offsets ascend.
        for count in (1..=256_usize).rev() {
            let Some(at) = end.checked_sub(4 + 2 * count).filter(|at| *at >= start) else {
                continue;
            };
            if View::u32_le_at(bytes, at).and_then(|value| usize::try_from(value).ok())
                != Some(count)
            {
                continue;
            }
            let Some((value, _, storage)) =
                lp_utf16_bounded_scoped(ctx, bytes, at, 1..=256, "f3d Design UTF-16 text")?
            else {
                continue;
            };
            if candidate.is_some() {
                return Ok(None);
            }
            candidate = Some((at, value, storage));
        }
    }
    Ok(candidate)
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::u64_from_index;

    use super::{
        bare_presentation_material as bare_presentation_material_with_context,
        body_presentations as body_presentations_with_context,
        browser_node_records as browser_node_records_with_context, next_match,
        BodyPresentationOwner,
    };
    use crate::bytes::lp_utf16_bytes;
    use crate::design::presentation::{
        APPEARANCE_LIBRARY_ID, BODY_PRESENTATION_BASE_TYPE_GUID,
        BODY_PRESENTATION_MATERIAL_ENVELOPE_ID, BODY_PRESENTATION_TYPE_GUID,
        BODY_PRESENTATION_TYPE_VERSION, BODY_SCENE_NODE_TYPE_GUID, BODY_SCENE_NODE_TYPE_VERSION,
        BREP_CONTAINER_TYPE_GUID, BREP_CONTAINER_TYPE_VERSION, BROWSER_NODE_BASE_TYPE_GUID,
        BROWSER_NODE_TYPE_GUID, BROWSER_NODE_TYPE_VERSION, MODERN_APPEARANCE_LIBRARY_IDS,
        PHYSICAL_MATERIAL_LIBRARY_ID,
    };
    use crate::design::test_support::{design_type, primary_record};
    use crate::records::entity_header::{DESIGN_MODULE_BODY, DESIGN_MODULE_FUSION};
    use crate::test_support::{lp_ascii, lp_utf16, push_reference_u64};

    fn bare_presentation_material(
        bytes: &[u8],
        start: usize,
        end: usize,
        entity_suffix: u64,
    ) -> Option<super::PresentationMaterial> {
        crate::design::test_support::with_test_decode_context(|ctx| {
            bare_presentation_material_with_context(ctx, bytes, start, end, entity_suffix)
        })
        .unwrap()
    }

    fn body_presentations(
        bytes: &[u8],
        meta: &crate::metastream::MetaStream,
    ) -> Result<Vec<super::BodyPresentation>, cadmpeg_core::CodecError> {
        crate::design::test_support::with_test_decode_context(|ctx| {
            body_presentations_with_context(ctx, bytes, meta)
        })
    }

    fn browser_node_records(
        bytes: &[u8],
        meta: &crate::metastream::MetaStream,
    ) -> Result<Vec<super::BrowserNodeRecord>, cadmpeg_core::CodecError> {
        crate::design::test_support::with_test_decode_context(|ctx| {
            browser_node_records_with_context(ctx, bytes, meta)
        })
    }

    /// Every non-overlapping match of `needle` in `from..end`.
    fn all_matches(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        bytes: &[u8],
        from: usize,
        end: usize,
        needle: &[&[u8]],
    ) -> Vec<usize> {
        let needle_len = needle.iter().map(|part| part.len()).sum::<usize>();
        let mut matches = Vec::new();
        let mut at = from;
        while let Some(found) =
            next_match(ctx, bytes, at, end, needle, "test F3D marker").expect("search")
        {
            matches.push(found);
            at = found + needle_len;
        }
        matches
    }

    #[test]
    fn next_match_finds_ascending_nonoverlapping_matches() {
        crate::design::test_support::with_test_decode_context(|ctx| {
            assert_eq!(all_matches(ctx, b"xababaabaY", 1, 9, &[b"aba"]), [1, 6]);
            assert_eq!(all_matches(ctx, b"xababaabaY", 1, 8, &[b"aba"]), [1]);
            assert_eq!(
                all_matches(ctx, b"xababaabaY", 1, 9, &[b"ab", b"a"]),
                [1, 6]
            );
            assert_eq!(
                all_matches(ctx, b"xababaabaY", 1, 9, &[b"ab", b"b"]),
                Vec::<usize>::new()
            );
            assert_eq!(all_matches(ctx, b"abc", 2, 1, &[b"c"]), Vec::<usize>::new());
        });
    }

    #[test]
    fn next_match_charges_only_the_bytes_it_visits() {
        let bytes = b"xxaxxabxx";
        for (skip, additional) in [(0, 1), (5, 1)] {
            let refusal = crate::test_support::resource_refusal_at(
                cadmpeg_core::decode::ResourceDimension::WorkUnits,
                "test F3D marker",
                skip,
                |ctx| next_match(ctx, bytes, 0, bytes.len(), &[b"ab"], "test F3D marker"),
            );
            assert!(matches!(
                refusal,
                cadmpeg_core::CodecError::ResourceLimit(limit)
                    if limit.operation == "test F3D marker" && limit.additional == additional
            ));
        }
        crate::design::test_support::with_test_decode_context(|ctx| {
            assert_eq!(
                next_match(ctx, bytes, 0, bytes.len(), &[b"ab"], "test F3D marker").unwrap(),
                Some(5)
            );
        });
    }

    #[test]
    fn presentation_utf16_text_refuses_exact_retained_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        let text = "A雪𐍈";
        let mut bytes = Vec::new();
        lp_utf16(&mut bytes, text);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = u64::try_from(text.len() - 1).unwrap();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = crate::bytes::lp_utf16_bounded_charged(
            &ctx,
            &bytes,
            0,
            1..=256,
            "f3d Design UTF-16 text",
        )
        .err()
        .unwrap();
        assert!(matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "f3d Design UTF-16 text"
        ));
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let decoded = crate::bytes::lp_utf16_bounded_charged(
            &ctx,
            &bytes,
            0,
            1..=256,
            "f3d Design UTF-16 text",
        )
        .unwrap()
        .unwrap();
        assert_eq!(decoded.0, text);
        assert_eq!(decoded.1, bytes.len());

        let invalid = [1, 0, 0, 0, 0, 0xd8];
        assert!(crate::bytes::lp_utf16_bounded_charged(
            &ctx,
            &invalid,
            0,
            1..=256,
            "f3d Design UTF-16 text"
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn browser_records_and_entity_types_refuse_collection_limits() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        let mut bytes = Vec::new();
        lp_ascii(&mut bytes, "256");
        bytes.extend_from_slice(&43_u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 10]);
        lp_utf16(&mut bytes, "11111111-2222-8333-A444-555555555555");
        bytes.extend_from_slice(&[0, 1, 1]);
        bytes.extend_from_slice(&42_u64.to_le_bytes());
        let meta = crate::metastream::MetaStream {
            types: vec![design_type(
                BROWSER_NODE_TYPE_GUID,
                Some(BROWSER_NODE_BASE_TYPE_GUID),
                BROWSER_NODE_TYPE_VERSION,
                DESIGN_MODULE_FUSION,
                vec![43],
            )],
            records: vec![primary_record(43, 0)],
            secondary_records: Vec::new(),
        };
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::CollectionItems,
            "f3d browser node records",
            0,
            |ctx| browser_node_records_with_context(ctx, &bytes, &meta),
        );
        assert!(matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "f3d browser node records"
        ));
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = super::entity_types(&ctx, &meta).err().unwrap();
        assert!(matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "f3d presentation entity types"
        ));
        let nodes = crate::design::test_support::with_test_decode_context(|ctx| {
            browser_node_records_with_context(ctx, &bytes, &meta).unwrap()
        });
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].entity_suffix, 42);
    }

    #[test]
    fn typed_presentation_joins_its_exact_browser_node() {
        let body_tag = 256u32;
        let node_tag = 257u32;
        let entity = (1u64 << 40) + 42;
        let node_guid = "11111111-2222-8333-A444-555555555555";
        let visual_guid = "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE_Post2015";
        let mut bytes = Vec::new();
        lp_ascii(&mut bytes, &body_tag.to_string());
        bytes.extend_from_slice(&entity.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        lp_utf16(&mut bytes, &format!("0_{entity}"));
        lp_utf16(&mut bytes, node_guid);
        bytes.extend_from_slice(&[1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        lp_utf16(&mut bytes, "99999999-8888-8777-A666-555555555555");
        lp_utf16(&mut bytes, PHYSICAL_MATERIAL_LIBRARY_ID);
        lp_utf16(&mut bytes, "PrismMaterial-001");
        push_reference_u64(&mut bytes, 7);
        bytes.push(0);
        push_reference_u64(&mut bytes, entity + 1);
        lp_utf16(&mut bytes, "Body");
        bytes.extend_from_slice(&1.0f32.to_le_bytes());
        bytes.extend_from_slice(&[1, 1]);
        lp_utf16(&mut bytes, visual_guid);
        for marker in MODERN_APPEARANCE_LIBRARY_IDS {
            lp_utf16(&mut bytes, marker);
        }
        let node_start = bytes.len();
        lp_ascii(&mut bytes, &node_tag.to_string());
        bytes.extend_from_slice(&43u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 10]);
        lp_utf16(&mut bytes, node_guid);
        bytes.extend_from_slice(&[0, 1, 1]);
        bytes.extend_from_slice(&entity.to_le_bytes());

        let meta = crate::metastream::MetaStream {
            types: vec![
                design_type(
                    BODY_PRESENTATION_TYPE_GUID,
                    Some(BODY_PRESENTATION_BASE_TYPE_GUID),
                    BODY_PRESENTATION_TYPE_VERSION,
                    DESIGN_MODULE_BODY,
                    vec![entity],
                ),
                design_type(
                    BROWSER_NODE_TYPE_GUID,
                    Some(BROWSER_NODE_BASE_TYPE_GUID),
                    BROWSER_NODE_TYPE_VERSION,
                    DESIGN_MODULE_FUSION,
                    vec![43],
                ),
                design_type(
                    BREP_CONTAINER_TYPE_GUID,
                    None,
                    BREP_CONTAINER_TYPE_VERSION,
                    "",
                    vec![7],
                ),
                design_type(
                    BODY_SCENE_NODE_TYPE_GUID,
                    None,
                    BODY_SCENE_NODE_TYPE_VERSION,
                    "",
                    vec![entity + 1],
                ),
            ],
            records: vec![primary_record(entity, 0), primary_record(43, node_start)],
            secondary_records: Vec::new(),
        };
        let presentations = body_presentations(&bytes, &meta).expect("typed primary frames");
        assert_eq!(presentations.len(), 1);
        let presentation = &presentations[0];
        assert_eq!(presentation.entity_suffix, entity);
        assert_eq!(
            presentation.owner,
            BodyPresentationOwner::Named {
                entity_id: crate::records::identity::DesignEntityId::from_parts("0", entity),
                entity_id_offset: 25,
            }
        );
        assert_eq!(presentation.browser_node.as_ref().unwrap().guid, node_guid);
        let material = presentation.material.as_ref().unwrap();
        assert_eq!(material.node_guid, node_guid);
        assert_eq!(material.physical_token, "PrismMaterial-001");
        assert_eq!(&*material.visual_guid, visual_guid);
        assert_eq!(material.visual_preset, None);
    }

    #[test]
    fn linked_body_presentation_refuses_output_and_node_copy_limits() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        let body_tag = 256u32;
        let node_tag = 257u32;
        let entity = (1u64 << 40) + 42;
        let node_guid = "11111111-2222-8333-A444-555555555555";
        let visual_guid = "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE_Post2015";
        let mut bytes = Vec::new();
        lp_ascii(&mut bytes, &body_tag.to_string());
        bytes.extend_from_slice(&entity.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        lp_utf16(&mut bytes, &format!("0_{entity}"));
        lp_utf16(&mut bytes, node_guid);
        bytes.extend_from_slice(&[1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        lp_utf16(&mut bytes, "99999999-8888-8777-A666-555555555555");
        lp_utf16(&mut bytes, PHYSICAL_MATERIAL_LIBRARY_ID);
        lp_utf16(&mut bytes, "PrismMaterial-001");
        push_reference_u64(&mut bytes, 7);
        bytes.push(0);
        push_reference_u64(&mut bytes, entity + 1);
        lp_utf16(&mut bytes, "Body");
        bytes.extend_from_slice(&1.0f32.to_le_bytes());
        bytes.extend_from_slice(&[1, 1]);
        lp_utf16(&mut bytes, visual_guid);
        for marker in MODERN_APPEARANCE_LIBRARY_IDS {
            lp_utf16(&mut bytes, marker);
        }
        let node_start = bytes.len();
        lp_ascii(&mut bytes, &node_tag.to_string());
        bytes.extend_from_slice(&43u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 10]);
        lp_utf16(&mut bytes, node_guid);
        bytes.extend_from_slice(&[0, 1, 1]);
        bytes.extend_from_slice(&entity.to_le_bytes());

        let meta = crate::metastream::MetaStream {
            types: vec![
                design_type(
                    BODY_PRESENTATION_TYPE_GUID,
                    Some(BODY_PRESENTATION_BASE_TYPE_GUID),
                    BODY_PRESENTATION_TYPE_VERSION,
                    DESIGN_MODULE_BODY,
                    vec![entity],
                ),
                design_type(
                    BROWSER_NODE_TYPE_GUID,
                    Some(BROWSER_NODE_BASE_TYPE_GUID),
                    BROWSER_NODE_TYPE_VERSION,
                    DESIGN_MODULE_FUSION,
                    vec![43],
                ),
                design_type(
                    BREP_CONTAINER_TYPE_GUID,
                    None,
                    BREP_CONTAINER_TYPE_VERSION,
                    "",
                    vec![7],
                ),
                design_type(
                    BODY_SCENE_NODE_TYPE_GUID,
                    None,
                    BODY_SCENE_NODE_TYPE_VERSION,
                    "",
                    vec![entity + 1],
                ),
            ],
            records: vec![primary_record(entity, 0), primary_record(43, node_start)],
            secondary_records: Vec::new(),
        };
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::CollectionItems,
            "f3d body presentation records",
            0,
            |ctx| body_presentations_with_context(ctx, &bytes, &meta),
        );
        assert!(matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "f3d body presentation records"
        ));
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = u64_from_index(node_guid.len() - 1);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let nodes = browser_node_records(&bytes, &meta).unwrap();
        let error = super::copy_browser_node(&ctx, &nodes[0]).err().unwrap();
        assert!(matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "f3d body presentation browser node GUID"
        ));
        let presentations = body_presentations(&bytes, &meta).unwrap();
        assert_eq!(presentations.len(), 1);
        assert_eq!(
            presentations[0].browser_node.as_ref().unwrap().guid,
            node_guid
        );
    }

    #[test]
    fn presentation_refuses_an_unregistered_node_shaped_byte_run() {
        let body_tag = 256u32;
        let entity = 42u64;
        let node_guid = "11111111-2222-8333-A444-555555555555";
        let mut bytes = Vec::new();
        lp_ascii(&mut bytes, &body_tag.to_string());
        bytes.extend_from_slice(&entity.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        lp_utf16(&mut bytes, "0_42");
        lp_utf16(&mut bytes, node_guid);
        bytes.extend_from_slice(&[1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        lp_utf16(&mut bytes, "99999999-8888-8777-A666-555555555555");
        lp_utf16(&mut bytes, PHYSICAL_MATERIAL_LIBRARY_ID);
        lp_utf16(&mut bytes, "PrismMaterial-001");
        push_reference_u64(&mut bytes, 7);
        bytes.push(0);
        push_reference_u64(&mut bytes, entity + 1);
        lp_utf16(&mut bytes, "Body");
        bytes.extend_from_slice(&1.0f32.to_le_bytes());
        bytes.extend_from_slice(&[1, 1]);
        lp_utf16(&mut bytes, "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE");
        lp_utf16(&mut bytes, APPEARANCE_LIBRARY_ID);
        lp_utf16(&mut bytes, "Prism-001");
        lp_utf16(&mut bytes, node_guid);
        bytes.extend_from_slice(&[0, 1, 1]);
        bytes.extend_from_slice(&entity.to_le_bytes());

        let meta = crate::metastream::MetaStream {
            types: vec![
                design_type(
                    BODY_PRESENTATION_TYPE_GUID,
                    Some(BODY_PRESENTATION_BASE_TYPE_GUID),
                    BODY_PRESENTATION_TYPE_VERSION,
                    DESIGN_MODULE_BODY,
                    vec![entity],
                ),
                design_type(
                    BREP_CONTAINER_TYPE_GUID,
                    None,
                    BREP_CONTAINER_TYPE_VERSION,
                    "",
                    vec![7],
                ),
                design_type(
                    BODY_SCENE_NODE_TYPE_GUID,
                    None,
                    BODY_SCENE_NODE_TYPE_VERSION,
                    "",
                    vec![entity + 1],
                ),
            ],
            records: vec![primary_record(entity, 0)],
            secondary_records: Vec::new(),
        };
        let presentations = body_presentations(&bytes, &meta).expect("typed body owner");
        assert_eq!(presentations.len(), 1);
        assert!(presentations[0].browser_node.is_none());
        assert_eq!(
            presentations[0]
                .material
                .as_ref()
                .and_then(|material| material
                    .visual_preset
                    .as_ref()
                    .map(|field| field.value.as_str())),
            Some("Prism-001")
        );
    }

    #[test]
    fn presentation_does_not_consume_the_next_primary_frame() {
        let entity = 42u64;
        let node_guid = "11111111-2222-8333-A444-555555555555";
        let mut bytes = Vec::new();
        lp_ascii(&mut bytes, "256");
        bytes.extend_from_slice(&entity.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        lp_utf16(&mut bytes, "0_42");

        let next_start = bytes.len();
        lp_ascii(&mut bytes, "257");
        bytes.extend_from_slice(&99u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 10]);
        lp_utf16(&mut bytes, node_guid);
        bytes.extend_from_slice(&[1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        lp_utf16(&mut bytes, "99999999-8888-8777-A666-555555555555");
        lp_utf16(&mut bytes, PHYSICAL_MATERIAL_LIBRARY_ID);
        lp_utf16(&mut bytes, "PrismMaterial-001");
        push_reference_u64(&mut bytes, 7);
        bytes.push(0);
        push_reference_u64(&mut bytes, entity + 1);
        lp_utf16(&mut bytes, "Body");
        bytes.extend_from_slice(&1.0f32.to_le_bytes());
        bytes.extend_from_slice(&[1, 1]);
        lp_utf16(&mut bytes, "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE");
        lp_utf16(&mut bytes, APPEARANCE_LIBRARY_ID);

        let meta = crate::metastream::MetaStream {
            types: vec![
                design_type(
                    BODY_PRESENTATION_TYPE_GUID,
                    Some(BODY_PRESENTATION_BASE_TYPE_GUID),
                    BODY_PRESENTATION_TYPE_VERSION,
                    DESIGN_MODULE_BODY,
                    vec![entity],
                ),
                design_type(
                    "00000000-0000-0000-0000-000000000000",
                    None,
                    0,
                    "",
                    vec![99],
                ),
                design_type(
                    BREP_CONTAINER_TYPE_GUID,
                    None,
                    BREP_CONTAINER_TYPE_VERSION,
                    "",
                    vec![7],
                ),
                design_type(
                    BODY_SCENE_NODE_TYPE_GUID,
                    None,
                    BODY_SCENE_NODE_TYPE_VERSION,
                    "",
                    vec![entity + 1],
                ),
            ],
            records: vec![primary_record(entity, 0), primary_record(99, next_start)],
            secondary_records: Vec::new(),
        };

        let presentations = body_presentations(&bytes, &meta).expect("exact body frame");
        let [presentation] = presentations.as_slice() else {
            panic!("one body presentation expected")
        };
        assert!(presentation.material.is_none());
    }

    #[test]
    fn browser_node_records_skip_other_typed_member_variants() {
        let record_index = 43u32;
        let mut bytes = Vec::new();
        lp_ascii(&mut bytes, "256");
        bytes.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&[0; 10]);
        lp_utf16(&mut bytes, "11111111-2222-8333-A444-555555555555");
        bytes.extend_from_slice(&[1, 0, 1]);
        bytes.extend_from_slice(&42u64.to_le_bytes());
        let meta = crate::metastream::MetaStream {
            types: vec![design_type(
                BROWSER_NODE_TYPE_GUID,
                Some(BROWSER_NODE_BASE_TYPE_GUID),
                BROWSER_NODE_TYPE_VERSION,
                DESIGN_MODULE_FUSION,
                vec![u64::from(record_index)],
            )],
            records: vec![primary_record(u64::from(record_index), 0)],
            secondary_records: Vec::new(),
        };

        assert!(browser_node_records(&bytes, &meta)
            .expect("typed alternate member")
            .is_empty());
    }

    #[test]
    fn bare_presentation_uses_its_typed_primary_owner_not_class_299() {
        let entity = 2_000u64;
        let node_record = 2_100u32;
        let node_guid = "11111111-2222-8333-A444-555555555555";
        let visual_guid = "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE_Post2015";
        let mut types = (0..44)
            .map(|_| {
                design_type(
                    "00000000-0000-0000-0000-000000000000",
                    None,
                    0,
                    "",
                    Vec::new(),
                )
            })
            .collect::<Vec<_>>();
        let body_tag = 300u32;
        let node_tag = 301u32;
        types.extend([
            design_type(
                BODY_PRESENTATION_TYPE_GUID,
                Some(BODY_PRESENTATION_BASE_TYPE_GUID),
                BODY_PRESENTATION_TYPE_VERSION,
                DESIGN_MODULE_BODY,
                vec![entity],
            ),
            design_type(
                BROWSER_NODE_TYPE_GUID,
                Some(BROWSER_NODE_BASE_TYPE_GUID),
                BROWSER_NODE_TYPE_VERSION,
                DESIGN_MODULE_FUSION,
                vec![u64::from(node_record)],
            ),
        ]);

        let mut bytes = Vec::new();
        lp_ascii(&mut bytes, &body_tag.to_string());
        bytes.extend_from_slice(&entity.to_le_bytes());
        bytes.extend_from_slice(&[0; 16]);
        lp_ascii(&mut bytes, "299");
        bytes.extend_from_slice(&999_999u64.to_le_bytes());
        bytes.extend_from_slice(&[0; 16]);
        lp_utf16(&mut bytes, BODY_PRESENTATION_MATERIAL_ENVELOPE_ID);
        lp_utf16(&mut bytes, PHYSICAL_MATERIAL_LIBRARY_ID);
        bytes.extend_from_slice(&[0; 4]);
        lp_utf16(&mut bytes, "PrismMaterial-018");
        push_reference_u64(&mut bytes, 1_900);
        bytes.push(0);
        lp_utf16(&mut bytes, node_guid);
        push_reference_u64(&mut bytes, entity + 1);
        bytes.extend_from_slice(&[0; 12]);
        bytes.extend_from_slice(&1.0f32.to_le_bytes());
        bytes.extend_from_slice(&[1, 1]);
        bytes.extend_from_slice(&[0; 10]);
        lp_utf16(&mut bytes, visual_guid);
        for marker in MODERN_APPEARANCE_LIBRARY_IDS {
            lp_utf16(&mut bytes, marker);
        }

        let node_start = bytes.len();
        assert!(bare_presentation_material(&bytes, 15, node_start, entity).is_some());
        let trailer_len = lp_utf16_bytes(MODERN_APPEARANCE_LIBRARY_IDS[1])
            .expect("fixture UTF-16 code-unit count fits u32")
            .len();
        assert!(
            bare_presentation_material(&bytes, 15, node_start - trailer_len, entity,).is_none()
        );
        lp_ascii(&mut bytes, &node_tag.to_string());
        bytes.extend_from_slice(&node_record.to_le_bytes());
        bytes.extend_from_slice(&[0; 10]);
        lp_utf16(&mut bytes, node_guid);
        bytes.extend_from_slice(&[0, 1, 1]);
        bytes.extend_from_slice(&entity.to_le_bytes());

        let meta = crate::metastream::MetaStream {
            types,
            records: vec![
                primary_record(entity, 0),
                primary_record(u64::from(node_record), node_start),
            ],
            secondary_records: Vec::new(),
        };
        let presentations = body_presentations(&bytes, &meta).expect("exact primary owner frame");
        let [presentation] = presentations.as_slice() else {
            panic!("one bare body presentation expected")
        };
        assert_eq!(presentation.entity_suffix, entity);
        assert_eq!(presentation.owner, BodyPresentationOwner::Bare);
        assert_eq!(
            presentation
                .browser_node
                .as_ref()
                .map(|node| node.guid.as_str()),
            Some(node_guid)
        );
        let material = presentation.material.as_ref().expect("material envelope");
        assert_eq!(material.physical_token, "PrismMaterial-018");
        assert_eq!(&*material.visual_guid, visual_guid);

        let parse_material = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
            bare_presentation_material_with_context(ctx, &bytes, 15, node_start, entity)
        };
        let material = crate::test_support::with_decode_context(|ctx| parse_material(ctx))
            .expect("service admission")
            .expect("valid bare material envelope");
        assert_eq!(material.physical_token, "PrismMaterial-018");
        assert_eq!(&*material.visual_guid, visual_guid);
    }
}
