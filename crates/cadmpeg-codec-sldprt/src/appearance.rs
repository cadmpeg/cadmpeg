// SPDX-License-Identifier: Apache-2.0
//! `SolidWorks` appearance definitions, assignments, and resolution.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::decode::View;
use cadmpeg_ir::topology::Color;

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
    pub(crate) source_name: String,
    pub(crate) record_offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DisplayAppearanceTarget {
    Body(Vec<usize>),
    Feature {
        source_id: u32,
        face_indexes: Vec<usize>,
    },
    Face(usize),
}

#[derive(Debug, Clone)]
pub(crate) struct DisplayAppearanceAssignment {
    pub(crate) target: DisplayAppearanceTarget,
    pub(crate) definition: AppearanceDefinition,
}

#[derive(Debug, Clone)]
pub(crate) struct FeatureAppearanceAssignment {
    pub(crate) feature_source_id: u32,
    pub(crate) feature_timestamp: u32,
    pub(crate) packed_color: u32,
    pub(crate) color: Color,
    pub(crate) source_name: String,
    pub(crate) record_offset: usize,
}

pub(crate) struct ResolvedDisplayAppearances {
    pub(crate) by_face: BTreeMap<usize, AppearanceDefinition>,
    pub(crate) matched_feature_sources: BTreeSet<u32>,
}

pub(crate) fn packed_rgb(packed: u32) -> Color {
    Color {
        r: (packed & 0xff) as f32 / 255.0,
        g: ((packed >> 8) & 0xff) as f32 / 255.0,
        b: ((packed >> 16) & 0xff) as f32 / 255.0,
        a: 1.0,
    }
}

pub(crate) fn definitions(scan: &ContainerScan) -> Vec<AppearanceDefinition> {
    scan.sections()
        .flat_map(|section| {
            let bytes = section.payload();
            bytes
                .windows(VISUAL_PROPERTIES_CLASS.len())
                .enumerate()
                .filter_map(move |(offset, token)| {
                    (token == VISUAL_PROPERTIES_CLASS)
                        .then(|| {
                            definition_at(section, offset + VISUAL_PROPERTIES_CLASS.len(), offset)
                        })
                        .flatten()
                })
        })
        .collect()
}

fn definition_at(
    section: Section<'_>,
    packed_offset: usize,
    record_offset: usize,
) -> Option<AppearanceDefinition> {
    let bytes = section.payload();
    let packed_color = View::u32_le_at(bytes, packed_offset)?;
    let name_header = packed_offset + 16;
    if bytes.get(name_header..name_header + 3) != Some(&[0xff, 0xfe, 0xff]) {
        return None;
    }
    let count = usize::from(*bytes.get(name_header + 3)?);
    let start = name_header + 4;
    let raw_name = bytes.get(start..start.checked_add(count.checked_mul(2)?)?)?;
    let units = raw_name
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    let name = String::from_utf16(&units).ok()?.trim().to_string();
    if name.is_empty() {
        return None;
    }
    Some(AppearanceDefinition {
        name,
        color: packed_rgb(packed_color),
        source_name: section.display_name(),
        record_offset,
    })
}

fn inline_definitions(section: Section<'_>, start: usize, end: usize) -> Vec<AppearanceDefinition> {
    let Some(bytes) = section.payload().get(start..end) else {
        return Vec::new();
    };
    bytes
        .windows(inline_visual::MARKER_VALUE.len())
        .enumerate()
        .filter_map(|(relative, marker)| {
            (marker == inline_visual::MARKER_VALUE)
                .then(|| {
                    let offset = start + relative;
                    definition_at(section, offset + inline_visual::PACKED_COLOR, offset)
                })
                .flatten()
        })
        .collect()
}

/// Decode body/default and face-local assignments from `DisplayLists`.
pub(crate) fn display_assignments(
    section: Section<'_>,
    faces: &[DisplayFace],
) -> Vec<DisplayAppearanceAssignment> {
    let classes = crate::tessellation::class_intervals(section.payload());
    let mut assignments = Vec::new();
    for face in faces {
        let Some(class) = classes.iter().find(|class| {
            class.name == "uoTempFaceTessData_c"
                && class.content.start <= face.table.start
                && face.table.start < class.content.end
        }) else {
            continue;
        };
        let definitions = inline_definitions(
            section,
            face.metadata.start,
            face.metadata.end.min(class.content.end),
        );
        if let [definition] = definitions.as_slice() {
            assignments.push(DisplayAppearanceAssignment {
                target: DisplayAppearanceTarget::Face(face.table_index),
                definition: definition.clone(),
            });
        }
    }
    for (class_index, class) in classes.iter().enumerate() {
        if class.name != "uoBodyPropInfo_c" {
            continue;
        }
        let definitions = inline_definitions(section, class.content.start, class.content.end);
        let [definition] = definitions.as_slice() else {
            continue;
        };
        let previous_body_end = classes[..class_index]
            .iter()
            .rev()
            .find(|previous| previous.name == "uoBodyPropInfo_c")
            .map_or(0, |previous| previous.content.end);
        let face_indexes = faces
            .iter()
            .filter(|face| {
                previous_body_end <= face.table.start && face.table.end <= class.class_offset
            })
            .map(|face| face.table_index)
            .collect::<Vec<_>>();
        if !face_indexes.is_empty() {
            assignments.push(DisplayAppearanceAssignment {
                target: DisplayAppearanceTarget::Body(face_indexes),
                definition: definition.clone(),
            });
        }
    }
    assignments
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
                    && class.content.start <= record_offset
                    && record_offset + feature_visual::LEN <= class.content.end
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
                    .filter(|value| *value != 0 && *value != u32::MAX)
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
            assignments.push(FeatureAppearanceAssignment {
                feature_source_id,
                feature_timestamp,
                packed_color,
                color: packed_rgb(packed_color),
                source_name: section.display_name(),
                record_offset,
            });
        }
    }
    assignments
}

/// Resolve the verified `DisplayLists` precedence: body, feature, then face.
pub(crate) fn resolve_display_appearances(
    scan: &ContainerScan,
    section: Section<'_>,
    faces: &[DisplayFace],
) -> ResolvedDisplayAppearances {
    let native_assignments = display_assignments(section, faces);
    let mut by_face = BTreeMap::new();
    for assignment in &native_assignments {
        if let DisplayAppearanceTarget::Body(face_indexes) = &assignment.target {
            for face_index in face_indexes {
                by_face.insert(*face_index, assignment.definition.clone());
            }
        }
    }

    let mut feature_by_source = HashMap::<u32, Option<FeatureAppearanceAssignment>>::new();
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
    let mut faces_by_source = BTreeMap::<u32, Vec<usize>>::new();
    for face in faces {
        if let Some(source_id) = face.feature_source_id() {
            faces_by_source
                .entry(source_id)
                .or_default()
                .push(face.table_index);
        }
    }
    for (source_id, face_indexes) in faces_by_source {
        let Some(Some(assignment)) = feature_by_source.get(&source_id) else {
            continue;
        };
        matched_feature_sources.insert(source_id);
        let definition = AppearanceDefinition {
            name: "SolidWorks feature appearance".into(),
            color: assignment.color,
            source_name: assignment.source_name.clone(),
            record_offset: assignment.record_offset,
        };
        let assignment = DisplayAppearanceAssignment {
            target: DisplayAppearanceTarget::Feature {
                source_id,
                face_indexes,
            },
            definition,
        };
        if let DisplayAppearanceTarget::Feature { face_indexes, .. } = &assignment.target {
            for face_index in face_indexes {
                by_face.insert(*face_index, assignment.definition.clone());
            }
        }
    }
    for assignment in native_assignments {
        if let DisplayAppearanceTarget::Face(face_index) = assignment.target {
            by_face.insert(face_index, assignment.definition);
        }
    }
    ResolvedDisplayAppearances {
        by_face,
        matched_feature_sources,
    }
}

#[cfg(test)]
mod tests;
