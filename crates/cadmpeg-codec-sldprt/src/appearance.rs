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
    let [red, green, blue, _] = packed.to_le_bytes();
    Color::from_rgba8(red, green, blue, 255)
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
            if let Some(definition) =
                definition_at(ctx, section, offset + VISUAL_PROPERTIES_CLASS.len(), offset)?
            {
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
    let Some(raw_name) = count
        .checked_mul(2)
        .and_then(|length| start.checked_add(length))
        .and_then(|end| bytes.get(start..end))
    else {
        return Ok(None);
    };
    let decoded =
        match ctx.utf16le_text(raw_name, count, false, "decode SLDPRT appearance name") {
            Ok(text) => text,
            Err(cadmpeg_core::CodecError::Malformed(_)) => return Ok(None),
            Err(error) => return Err(error),
        };
    let trimmed = decoded.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let name = trimmed.to_owned();
    let source_name = section.source_stream().clone();
    Ok(Some(AppearanceDefinition {
        name,
        color: packed_rgb(packed_color),
        source_name,
        record_offset,
    }))
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
        if let Some(definition) =
            definition_at(ctx, section, offset + inline_visual::PACKED_COLOR, offset)?
        {
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
    let classes = crate::tessellation::class_intervals(section.payload());
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
        let definitions =
            inline_definitions(ctx, section, class.content.start(), class.content.end())?;
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
                face_indexes.push(table_index);
            }
        }
        if !face_indexes.is_empty() {
            let Some(definition) = definitions.into_iter().next() else {
                continue;
            };
            assignments.push(DisplayAppearanceAssignment {
                target: DisplayAppearanceTarget::Body(face_indexes),
                definition,
            });
        }
    }
    Ok(assignments)
}

/// Decode feature-source assignments from `ThirdPtyStore/VisualStates`.
pub(crate) fn feature_assignments(scan: &ContainerScan) -> Vec<FeatureAppearanceAssignment> {
    let mut assignments = Vec::new();
    for section in scan
        .sections()
        .filter(|section| section.name() == Some("ThirdPtyStore/VisualStates"))
    {
        let bytes = section.payload();
        let classes = crate::tessellation::class_intervals(bytes);
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
            let source_name = section.source_stream().clone();
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
    assignments
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
                by_face.insert(*face_index, assignment.definition.clone());
            }
        }
    }

    let mut feature_by_source =
        HashMap::<FeatureSourceId, Option<FeatureAppearanceAssignment>>::new();
    for assignment in feature_assignments(scan) {
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
            faces_by_source
                .entry(source_id)
                .or_default()
                .push(table_index);
        }
    }
    for (source_id, face_indexes) in faces_by_source {
        const FEATURE_APPEARANCE_NAME: &str = "SolidWorks feature appearance";

        let Some(Some(assignment)) = feature_by_source.get(&source_id) else {
            continue;
        };
        matched_feature_sources.insert(source_id);
        let name = FEATURE_APPEARANCE_NAME.to_owned();
        let definition = AppearanceDefinition {
            name,
            color: assignment.color,
            source_name: assignment.source_name.clone(),
            record_offset: assignment.record_offset,
        };
        for face_index in face_indexes {
            by_face.insert(face_index, definition.clone());
        }
    }
    for assignment in native_assignments {
        if let DisplayAppearanceTarget::Face(face_index) = assignment.target {
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
