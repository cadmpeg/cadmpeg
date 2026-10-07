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
    for section in scan.sections(ctx)? {
        let bytes = section.payload();
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(bytes.len()),
            "scan SLDPRT appearance definitions",
        )?;
        for (offset, token) in bytes.windows(VISUAL_PROPERTIES_CLASS.len()).enumerate() {
            if token != VISUAL_PROPERTIES_CLASS {
                continue;
            }
            if let Some(definition) =
                definition_at(ctx, section, offset + VISUAL_PROPERTIES_CLASS.len(), offset)?
            {
                ctx.push_vec(
                    &mut definitions,
                    definition,
                    "collect SLDPRT appearance definitions",
                )?;
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
    let (decoded, _name_reservation) =
        match ctx.utf16le_scoped_text(raw_name, count, false, "decode SLDPRT appearance name") {
            Ok(text) => text,
            Err(cadmpeg_core::CodecError::Malformed(_)) => return Ok(None),
            Err(error) => return Err(error),
        };
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(decoded.len()),
        "trim SLDPRT appearance name",
    )?;
    let trimmed = decoded.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(trimmed.len()),
        "retain SLDPRT appearance name",
    )?;
    let name = ctx.copy_retained_text(trimmed, "retain SLDPRT appearance name")?;
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
    let copy = ctx.copy_retained_text(name.as_str(), "copy SLDPRT appearance stream name")?;
    StreamName::try_from(copy).map_err(|_| {
        cadmpeg_core::CodecError::Malformed("SLDPRT appearance stream name is empty".into())
    })
}

fn copy_definition(
    ctx: &DecodeContext<'_>,
    definition: &AppearanceDefinition,
) -> Result<AppearanceDefinition, cadmpeg_core::CodecError> {
    let name = ctx.copy_retained_text(&definition.name, "copy SLDPRT appearance name")?;
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
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        "scan inline SLDPRT appearances",
    )?;
    let mut definitions = Vec::new();
    for (relative, marker) in bytes.windows(inline_visual::MARKER_VALUE.len()).enumerate() {
        if marker != inline_visual::MARKER_VALUE {
            continue;
        }
        let offset = start + relative;
        if let Some(definition) =
            definition_at(ctx, section, offset + inline_visual::PACKED_COLOR, offset)?
        {
            ctx.push_vec(
                &mut definitions,
                definition,
                "collect inline SLDPRT appearances",
            )?;
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
    for (table_index, face) in ctx
        .admit_iter(faces, "scan SLDPRT display_assignments values")?
        .enumerate()
    {
        let Some(class) = containing_class(ctx, &classes, face.table.start())?
            .filter(|class| class.name == "uoTempFaceTessData_c")
        else {
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
            ctx.push_vec(
                &mut assignments,
                DisplayAppearanceAssignment {
                    target: DisplayAppearanceTarget::Face(table_index),
                    definition,
                },
                "collect SLDPRT display assignments",
            )?;
        }
    }
    // A body owns the faces whose tables lie between the previous body record
    // and its own; those windows are disjoint, so the faces ordered by table
    // start are visited once across all bodies.
    let mut faces_by_start = Vec::new();
    let mut previous_body_end = 0;
    for class in ctx.admit_iter(&classes, "scan SLDPRT display_assignments values")? {
        if class.name != "uoBodyPropInfo_c" {
            continue;
        }
        let body_start = std::mem::replace(&mut previous_body_end, class.content.end());
        let definitions =
            inline_definitions(ctx, section, class.content.start(), class.content.end())?;
        if definitions.len() != 1 {
            continue;
        }
        if faces_by_start.len() != faces.len() {
            faces_by_start = ctx.collect_vec(0..faces.len(), "order SLDPRT appearance faces")?;
            ctx.sort_unstable_by_key(
                &mut faces_by_start,
                |&index| (faces[index].table.start(), index),
                Ord::cmp,
                "order SLDPRT appearance faces",
            )?;
        }
        let first = ctx.partition_point(
            &faces_by_start,
            |&index| Ok(faces[index].table.start() < body_start),
            "match SLDPRT body appearance faces",
        )?;
        let mut face_indexes = Vec::new();
        for &table_index in faces_by_start.get(first..).unwrap_or_default() {
            ctx.charge_work(1, "match SLDPRT body appearance faces")?;
            let table = &faces[table_index].table;
            if table.start() > class.class_offset {
                break;
            }
            if table.end() <= class.class_offset {
                ctx.push_vec(
                    &mut face_indexes,
                    table_index,
                    "collect SLDPRT body appearance faces",
                )?;
            }
        }
        ctx.sort_unstable_by_key(
            &mut face_indexes,
            |&index| index,
            Ord::cmp,
            "collect SLDPRT body appearance faces",
        )?;
        if !face_indexes.is_empty() {
            let Some(definition) = definitions.into_iter().next() else {
                continue;
            };
            ctx.push_vec(
                &mut assignments,
                DisplayAppearanceAssignment {
                    target: DisplayAppearanceTarget::Body(face_indexes),
                    definition,
                },
                "collect SLDPRT display assignments",
            )?;
        }
    }
    Ok(assignments)
}

/// The class interval whose content holds `offset`. Intervals are ordered by
/// content start and do not overlap, so at most one holds it.
fn containing_class<'a>(
    ctx: &DecodeContext<'_>,
    classes: &'a [crate::tessellation::ClassInterval],
    offset: usize,
) -> Result<Option<&'a crate::tessellation::ClassInterval>, cadmpeg_core::CodecError> {
    let after = ctx.partition_point(
        classes,
        |class| Ok(class.content.start() <= offset),
        "match SLDPRT appearance classes",
    )?;
    Ok(after
        .checked_sub(1)
        .and_then(|index| classes.get(index))
        .filter(|class| offset < class.content.end()))
}

/// Decode feature-source assignments from `ThirdPtyStore/VisualStates`.
pub(crate) fn feature_assignments(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<FeatureAppearanceAssignment>, cadmpeg_core::CodecError> {
    let mut assignments = Vec::new();
    for section in scan
        .sections(ctx)?
        .filter(|section| section.name() == Some("ThirdPtyStore/VisualStates"))
    {
        let bytes = section.payload();
        let classes = crate::tessellation::class_intervals(ctx, bytes)?;
        for marker_offset in ctx
            .admit_iter(bytes, "scan SLDPRT feature appearance markers")?
            .windows(const { crate::nonzero(feature_visual::MARKER_VALUE.len()) })
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
            if !containing_class(ctx, &classes, record_offset)?.is_some_and(|class| {
                class.name == "moCompFeature_c"
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
            ctx.push_vec(
                &mut assignments,
                FeatureAppearanceAssignment {
                    feature_source_id,
                    feature_timestamp,
                    packed_color,
                    color: packed_rgb(packed_color),
                    source_name,
                    record_offset,
                },
                "collect SLDPRT feature appearances",
            )?;
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
    for assignment in ctx.admit_iter(
        &native_assignments,
        "scan SLDPRT resolve_display_appearances values",
    )? {
        if let DisplayAppearanceTarget::Body(face_indexes) = &assignment.target {
            for face_index in ctx.admit_iter(face_indexes, "scan SLDPRT body appearance targets")? {
                let definition = copy_definition(ctx, &assignment.definition)?;
                ctx.insert_btree_map(
                    &mut by_face,
                    *face_index,
                    definition,
                    "index SLDPRT face appearances",
                )?;
            }
        }
    }

    let mut feature_by_source =
        HashMap::<FeatureSourceId, Option<FeatureAppearanceAssignment>>::new();
    for assignment in feature_assignments(ctx, scan)? {
        if let Some(existing) = ctx.get_mut_hash_map(
            &mut feature_by_source,
            &assignment.feature_source_id,
            "index SLDPRT feature appearances",
        )? {
            if existing.as_ref().is_some_and(|previous| {
                previous.feature_timestamp != assignment.feature_timestamp
                    || previous.packed_color != assignment.packed_color
            }) {
                *existing = None;
            }
        } else {
            ctx.insert_hash_map(
                &mut feature_by_source,
                assignment.feature_source_id,
                Some(assignment),
                "index SLDPRT feature appearances",
            )?;
        }
    }
    let mut matched_feature_sources = BTreeSet::new();
    let mut faces_by_source = BTreeMap::<FeatureSourceId, Vec<usize>>::new();
    for (table_index, face) in ctx
        .admit_iter(faces, "scan SLDPRT resolve_display_appearances values")?
        .enumerate()
    {
        if let Some(source_id) = face.feature_source_id(ctx)? {
            ctx.push_btree_group(
                &mut faces_by_source,
                source_id,
                table_index,
                "index SLDPRT appearance face sources",
                "collect SLDPRT appearance source faces",
            )?;
        }
    }
    for (&source_id, face_indexes) in ctx.admit_iter(
        &faces_by_source,
        "scan SLDPRT feature appearance face sources",
    )? {
        const FEATURE_APPEARANCE_NAME: &str = "SolidWorks feature appearance";

        let Some(Some(assignment)) =
            ctx.get_hash_map(&(feature_by_source), &source_id, "look up SLDPRT hash key")?
        else {
            continue;
        };
        ctx.insert_btree_set(
            &mut matched_feature_sources,
            source_id,
            "collect matched SLDPRT appearance sources",
        )?;
        let name = ctx.copy_retained_text(
            FEATURE_APPEARANCE_NAME,
            "retain SLDPRT feature appearance name",
        )?;
        let definition = AppearanceDefinition {
            name,
            color: assignment.color,
            source_name: clone_stream_name(ctx, &assignment.source_name)?,
            record_offset: assignment.record_offset,
        };
        for face_index in ctx
            .admit_iter(face_indexes, "scan SLDPRT feature appearance face indexes")?
            .copied()
        {
            let copied = copy_definition(ctx, &definition)?;
            ctx.insert_btree_map(
                &mut by_face,
                face_index,
                copied,
                "index SLDPRT face appearances",
            )?;
        }
    }
    for assignment in ctx.admit_iter(native_assignments, "index SLDPRT face appearances")? {
        if let DisplayAppearanceTarget::Face(face_index) = assignment.target {
            ctx.insert_btree_map(
                &mut by_face,
                face_index,
                assignment.definition,
                "index SLDPRT face appearances",
            )?;
        }
    }
    Ok(ResolvedDisplayAppearances {
        by_face,
        matched_feature_sources,
    })
}

#[cfg(test)]
mod tests;
