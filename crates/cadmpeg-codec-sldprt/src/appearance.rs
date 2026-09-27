// SPDX-License-Identifier: Apache-2.0
//! `SolidWorks` appearance definitions, assignments, and resolution.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_ir::topology::Color;
use cadmpeg_ir::StreamName;

use crate::brep::feature_source::FeatureSourceId;
use crate::container::{ContainerScan, Section};
use crate::layout::display_lists_inline_visual_properties_prefix as inline_visual;
use crate::layout::visual_states_feature_appearance_prefix as feature_visual;
use crate::tessellation::DisplayFace;

const VISUAL_PROPERTIES_CLASS: &[u8] = b"moVisualProperties_c";

/// One decoded visual-property definition. Ownership is stored separately.
#[derive(Debug, Clone)]
pub(crate) struct AppearanceDefinition {
    pub(crate) name: String,
    pub(crate) color: Color,
    pub(crate) source_name: StreamName,
    pub(crate) record_offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DisplayAppearanceTarget {
    Body(Vec<usize>),
    Face(usize),
}

#[derive(Debug, Clone)]
struct DisplayAppearanceAssignment {
    target: DisplayAppearanceTarget,
    definition: AppearanceDefinition,
}

#[derive(Debug, Clone)]
pub(crate) struct FeatureAppearanceAssignment {
    pub(crate) feature_source_id: FeatureSourceId,
    feature_timestamp: u32,
    packed_color: u32,
    color: Color,
    source_name: StreamName,
    record_offset: usize,
}

pub(crate) struct ResolvedDisplayAppearances {
    pub(crate) by_face: BTreeMap<usize, AppearanceDefinition>,
    pub(crate) matched_feature_sources: BTreeSet<FeatureSourceId>,
}

fn packed_rgb(packed: u32) -> Color {
    Color::from_rgba8(packed as u8, (packed >> 8) as u8, (packed >> 16) as u8, 255)
}

pub(crate) fn definitions(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<AppearanceDefinition>, cadmpeg_core::CodecError> {
    let mut definitions = Vec::new();
    for section in scan.sections() {
        let bytes = section.payload();
        for (offset, token) in bytes.windows(VISUAL_PROPERTIES_CLASS.len()).enumerate() {
            if token != VISUAL_PROPERTIES_CLASS {
                continue;
            }
            if let Some(definition) = definition_at(
                ctx, section, offset + VISUAL_PROPERTIES_CLASS.len(), offset,
            )? {
                ctx.charge_collection_items(1, "collect SLDPRT appearance definitions")?;
                definitions.try_reserve(1).map_err(|_| ctx.refuse_codec_limit("collect SLDPRT appearance definitions", u64::MAX - 1, u64::MAX))?;
                definitions.push(definition);
            }
        }
    }
    Ok(definitions)
}

fn definition_at(
    ctx: &DecodeContext<'_>,
    section: Section<'_>,
    packed_offset: usize,
    record_offset: usize,
) -> Result<Option<AppearanceDefinition>, cadmpeg_core::CodecError> {
    let bytes = section.payload();
    let Some(packed_color) = View::u32_le_at(bytes, packed_offset) else {
        return Ok(None);
    };
    let name_header = packed_offset + 16;
    if bytes.get(name_header..name_header + 3) != Some(&[0xff, 0xfe, 0xff]) {
        return Ok(None);
    }
    let Some(count) = bytes.get(name_header + 3).map(|count| usize::from(*count)) else {
        return Ok(None);
    };
    let start = name_header + 4;
    let Some(raw_name) = count.checked_mul(2).and_then(|length| start.checked_add(length)).and_then(|end| bytes.get(start..end)) else {
        return Ok(None);
    };
    let mut units = [0_u16; 255];
    for (index, unit) in units[..count].iter_mut().enumerate() {
        let Some(value) = View::u16_le_at(raw_name, index * 2) else {
            return Ok(None);
        };
        *unit = value;
    }
    let mut decoded = ['\0'; 255];
    let mut decoded_count = 0usize;
    for scalar in char::decode_utf16(units[..count].iter().copied()) {
        let Ok(scalar) = scalar else {
            return Ok(None);
        };
        decoded[decoded_count] = scalar;
        decoded_count += 1;
    }
    let Some(first) = decoded[..decoded_count].iter().position(|scalar| !scalar.is_whitespace()) else {
        return Ok(None);
    };
    let Some(last) = decoded[..decoded_count].iter().rposition(|scalar| !scalar.is_whitespace()) else {
        return Ok(None);
    };
    let trimmed = &decoded[first..=last];
    let trimmed_len = trimmed.iter().try_fold(0usize, |size, scalar| size.checked_add(scalar.len_utf8())).ok_or_else(|| ctx.refuse_codec_limit("retain SLDPRT appearance name", u64::MAX - 1, u64::MAX))?;
    ctx.charge_retained(u64::try_from(trimmed_len).map_err(|_| ctx.refuse_codec_limit("retain SLDPRT appearance name", u64::MAX - 1, u64::MAX))?, "retain SLDPRT appearance name")?;
    let mut name = String::new();
    name.try_reserve(trimmed_len).map_err(|_| ctx.refuse_codec_limit("retain SLDPRT appearance name", u64::MAX - 1, u64::MAX))?;
    for scalar in trimmed {
        name.push(*scalar);
    }
    let source_name = clone_stream_name(ctx, section.source_stream())?;
    Ok(Some(AppearanceDefinition {
        name,
        color: packed_rgb(packed_color),
        source_name,
        record_offset,
    }))
}

fn clone_stream_name(
    ctx: &DecodeContext<'_>,
    name: &StreamName,
) -> Result<StreamName, cadmpeg_core::CodecError> {
    ctx.charge_retained(u64::try_from(name.as_str().len()).map_err(|_| ctx.refuse_codec_limit("copy SLDPRT appearance stream name", u64::MAX - 1, u64::MAX))?, "copy SLDPRT appearance stream name")?;
    let mut copy = String::new();
    copy.try_reserve(name.as_str().len()).map_err(|_| ctx.refuse_codec_limit("copy SLDPRT appearance stream name", u64::MAX - 1, u64::MAX))?;
    copy.push_str(name.as_str());
    StreamName::try_from(copy).map_err(|_| cadmpeg_core::CodecError::Malformed("SLDPRT appearance stream name is empty".into()))
}

fn copy_definition(
    ctx: &DecodeContext<'_>,
    definition: &AppearanceDefinition,
) -> Result<AppearanceDefinition, cadmpeg_core::CodecError> {
    ctx.charge_retained(u64::try_from(definition.name.len()).map_err(|_| ctx.refuse_codec_limit("copy SLDPRT appearance name", u64::MAX - 1, u64::MAX))?, "copy SLDPRT appearance name")?;
    let mut name = String::new();
    name.try_reserve(definition.name.len()).map_err(|_| ctx.refuse_codec_limit("copy SLDPRT appearance name", u64::MAX - 1, u64::MAX))?;
    name.push_str(&definition.name);
    Ok(AppearanceDefinition {
        name,
        color: definition.color,
        source_name: clone_stream_name(ctx, &definition.source_name)?,
        record_offset: definition.record_offset,
    })
}

fn inline_definitions(
    ctx: &DecodeContext<'_>,
    section: Section<'_>,
    start: usize,
    end: usize,
) -> Result<Vec<AppearanceDefinition>, cadmpeg_core::CodecError> {
    let Some(bytes) = section.payload().get(start..end) else {
        return Ok(Vec::new());
    };
    let mut definitions = Vec::new();
    for (relative, marker) in bytes.windows(inline_visual::MARKER_VALUE.len()).enumerate() {
        if marker != inline_visual::MARKER_VALUE {
            continue;
        }
        let offset = start + relative;
        if let Some(definition) = definition_at(
            ctx, section, offset + inline_visual::PACKED_COLOR, offset,
        )? {
            ctx.charge_collection_items(1, "collect inline SLDPRT appearances")?;
            definitions.try_reserve(1).map_err(|_| ctx.refuse_codec_limit("collect inline SLDPRT appearances", u64::MAX - 1, u64::MAX))?;
            definitions.push(definition);
        }
    }
    Ok(definitions)
}

/// Decode body/default and face-local assignments from `DisplayLists`.
fn display_assignments(
    ctx: &DecodeContext<'_>,
    section: Section<'_>,
    faces: &[DisplayFace],
) -> Result<Vec<DisplayAppearanceAssignment>, cadmpeg_core::CodecError> {
    let classes = crate::tessellation::class_intervals(ctx, section.payload())?;
    let mut assignments = Vec::new();
    for (table_index, face) in faces.iter().enumerate() {
        let Some(class) = classes.iter().find(|class| {
            class.name == "uoTempFaceTessData_c"
                && class.content.start() <= face.table.start()
                && face.table.start() < class.content.end()
        }) else {
            continue;
        };
        let definitions = inline_definitions(
            ctx,
            section,
            face.metadata.start(),
            face.metadata.end().min(class.content.end()),
        )?;
        if definitions.len() == 1 {
            let Some(definition) = definitions.into_iter().next() else {
                continue;
            };
            ctx.charge_collection_items(1, "collect SLDPRT display assignments")?;
            assignments.try_reserve(1).map_err(|_| ctx.refuse_codec_limit("collect SLDPRT display assignments", u64::MAX - 1, u64::MAX))?;
            assignments.push(DisplayAppearanceAssignment {
                target: DisplayAppearanceTarget::Face(table_index),
                definition,
            });
        }
    }
    for (class_index, class) in classes.iter().enumerate() {
        if class.name != "uoBodyPropInfo_c" {
            continue;
        }
        let definitions = inline_definitions(ctx, section, class.content.start(), class.content.end())?;
        if definitions.len() != 1 {
            continue;
        }
        let previous_body_end = classes[..class_index]
            .iter()
            .rev()
            .find(|previous| previous.name == "uoBodyPropInfo_c")
            .map_or(0, |previous| previous.content.end());
        let mut face_indexes = Vec::new();
        for (table_index, face) in faces.iter().enumerate() {
            if previous_body_end <= face.table.start() && face.table.end() <= class.class_offset {
                ctx.charge_collection_items(1, "collect SLDPRT body appearance faces")?;
                face_indexes.try_reserve(1).map_err(|_| ctx.refuse_codec_limit("collect SLDPRT body appearance faces", u64::MAX - 1, u64::MAX))?;
                face_indexes.push(table_index);
            }
        }
        if !face_indexes.is_empty() {
            let Some(definition) = definitions.into_iter().next() else {
                continue;
            };
            ctx.charge_collection_items(1, "collect SLDPRT display assignments")?;
            assignments.try_reserve(1).map_err(|_| ctx.refuse_codec_limit("collect SLDPRT display assignments", u64::MAX - 1, u64::MAX))?;
            assignments.push(DisplayAppearanceAssignment {
                target: DisplayAppearanceTarget::Body(face_indexes),
                definition,
            });
        }
    }
    Ok(assignments)
}

/// Decode feature-source assignments from `ThirdPtyStore/VisualStates`.
pub(crate) fn feature_assignments(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<FeatureAppearanceAssignment>, cadmpeg_core::CodecError> {
    let mut assignments = Vec::new();
    for section in scan
        .sections()
        .filter(|section| section.name() == Some("ThirdPtyStore/VisualStates"))
    {
        let bytes = section.payload();
        let classes = crate::tessellation::class_intervals(ctx, bytes)?;
        for marker_offset in bytes
            .windows(feature_visual::MARKER_VALUE.len())
            .enumerate()
            .filter_map(|(offset, marker)| {
                (marker == feature_visual::MARKER_VALUE).then_some(offset)
            })
        {
            let Some(record_offset) = marker_offset.checked_sub(feature_visual::MARKER) else {
                continue;
            };
            let Some(record) = bytes.get(record_offset..record_offset + feature_visual::LEN) else {
                continue;
            };
            if !classes.iter().any(|class| {
                class.name == "moCompFeature_c"
                    && class.content.start() <= record_offset
                    && record_offset + feature_visual::LEN <= class.content.end()
            }) || View::u32_le_at(record, feature_visual::VERSION)
                != Some(feature_visual::VERSION_VALUE)
                || View::u32_le_at(record, feature_visual::SELECTOR_ONE_A)
                    != Some(feature_visual::SELECTOR_ONE_A_VALUE)
                || View::u32_le_at(record, feature_visual::SELECTOR_ONE_B)
                    != Some(feature_visual::SELECTOR_ONE_B_VALUE)
                || View::u32_le_at(record, feature_visual::SELECTOR_TWO)
                    != Some(feature_visual::SELECTOR_TWO_VALUE)
                || record.get(
                    feature_visual::INSTANCE_PREFIX
                        ..feature_visual::INSTANCE_PREFIX
                            + feature_visual::INSTANCE_PREFIX_VALUE.len(),
                ) != Some(feature_visual::INSTANCE_PREFIX_VALUE.as_slice())
            {
                continue;
            }
            let Some(feature_source_id) =
                View::u32_le_at(record, feature_visual::FEATURE_SOURCE_ID)
                    .and_then(|value| FeatureSourceId::try_from(value).ok())
            else {
                continue;
            };
            let Some(feature_timestamp) =
                View::u32_le_at(record, feature_visual::FEATURE_TIMESTAMP)
                    .filter(|value| *value != 0 && *value != u32::MAX)
            else {
                continue;
            };
            let Some(packed_color) = View::u32_le_at(record, feature_visual::PACKED_COLOR) else {
                continue;
            };
            let source_name = clone_stream_name(ctx, section.source_stream())?;
            ctx.charge_collection_items(1, "collect SLDPRT feature appearances")?;
            assignments.try_reserve(1).map_err(|_| ctx.refuse_codec_limit("collect SLDPRT feature appearances", u64::MAX - 1, u64::MAX))?;
            assignments.push(FeatureAppearanceAssignment {
                feature_source_id,
                feature_timestamp,
                packed_color,
                color: packed_rgb(packed_color),
                source_name,
                record_offset,
            });
        }
    }
    Ok(assignments)
}

/// Resolve the verified `DisplayLists` precedence: body, feature, then face.
pub(crate) fn resolve_display_appearances(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    section: Section<'_>,
    faces: &[DisplayFace],
) -> Result<ResolvedDisplayAppearances, cadmpeg_core::CodecError> {
    let native_assignments = display_assignments(ctx, section, faces)?;
    let mut by_face = BTreeMap::new();
    for assignment in &native_assignments {
        if let DisplayAppearanceTarget::Body(face_indexes) = &assignment.target {
            for face_index in face_indexes {
                let definition = copy_definition(ctx, &assignment.definition)?;
                if !by_face.contains_key(face_index) {
                    ctx.charge_collection_items(1, "index SLDPRT face appearances")?;
                }
                by_face.insert(*face_index, definition);
            }
        }
    }

    let mut feature_by_source =
        HashMap::<FeatureSourceId, Option<FeatureAppearanceAssignment>>::new();
    for assignment in feature_assignments(ctx, scan)? {
        if !feature_by_source.contains_key(&assignment.feature_source_id) {
            ctx.charge_collection_items(1, "index SLDPRT feature appearances")?;
            feature_by_source.try_reserve(1).map_err(|_| ctx.refuse_codec_limit("index SLDPRT feature appearances", u64::MAX - 1, u64::MAX))?;
        }
        feature_by_source
            .entry(assignment.feature_source_id)
            .and_modify(|existing| {
                if existing.as_ref().is_some_and(|previous| {
                    previous.feature_timestamp != assignment.feature_timestamp
                        || previous.packed_color != assignment.packed_color
                }) {
                    *existing = None;
                }
            })
            .or_insert_with(|| Some(assignment));
    }
    let mut matched_feature_sources = BTreeSet::new();
    let mut faces_by_source = BTreeMap::<FeatureSourceId, Vec<usize>>::new();
    for (table_index, face) in faces.iter().enumerate() {
        if let Some(source_id) = face.feature_source_id() {
            if !faces_by_source.contains_key(&source_id) {
                ctx.charge_collection_items(1, "index SLDPRT appearance face sources")?;
            }
            let faces = faces_by_source.entry(source_id).or_default();
            ctx.charge_collection_items(1, "collect SLDPRT appearance source faces")?;
            faces.try_reserve(1).map_err(|_| ctx.refuse_codec_limit("collect SLDPRT appearance source faces", u64::MAX - 1, u64::MAX))?;
            faces.push(table_index);
        }
    }
    for (source_id, face_indexes) in faces_by_source {
        let Some(Some(assignment)) = feature_by_source.get(&source_id) else {
            continue;
        };
        if !matched_feature_sources.contains(&source_id) {
            ctx.charge_collection_items(1, "collect matched SLDPRT appearance sources")?;
        }
        matched_feature_sources.insert(source_id);
        const FEATURE_APPEARANCE_NAME: &str = "SolidWorks feature appearance";
        ctx.charge_retained(u64::try_from(FEATURE_APPEARANCE_NAME.len()).map_err(|_| ctx.refuse_codec_limit("retain SLDPRT feature appearance name", u64::MAX - 1, u64::MAX))?, "retain SLDPRT feature appearance name")?;
        let mut name = String::new();
        name.try_reserve(FEATURE_APPEARANCE_NAME.len()).map_err(|_| ctx.refuse_codec_limit("retain SLDPRT feature appearance name", u64::MAX - 1, u64::MAX))?;
        name.push_str(FEATURE_APPEARANCE_NAME);
        let definition = AppearanceDefinition {
            name,
            color: assignment.color,
            source_name: clone_stream_name(ctx, &assignment.source_name)?,
            record_offset: assignment.record_offset,
        };
        for face_index in face_indexes {
            let copied = copy_definition(ctx, &definition)?;
            if !by_face.contains_key(&face_index) {
                ctx.charge_collection_items(1, "index SLDPRT face appearances")?;
            }
            by_face.insert(face_index, copied);
        }
    }
    for assignment in native_assignments {
        if let DisplayAppearanceTarget::Face(face_index) = assignment.target {
            if !by_face.contains_key(&face_index) {
                ctx.charge_collection_items(1, "index SLDPRT face appearances")?;
            }
            by_face.insert(face_index, assignment.definition);
        }
    }
    Ok(ResolvedDisplayAppearances {
        by_face,
        matched_feature_sources,
    })
}

#[cfg(test)]
mod tests;
