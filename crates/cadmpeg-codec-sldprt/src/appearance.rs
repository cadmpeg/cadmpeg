// SPDX-License-Identifier: Apache-2.0
//! `SolidWorks` appearance definitions, assignments, and resolution.

use std::cell::OnceCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::decode::{DecodeContext, ScopedReservation, View};
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
pub(crate) struct FeatureAppearanceAssignment<'scan> {
    pub(crate) feature_source_id: FeatureSourceId,
    feature_timestamp: u32,
    packed_color: u32,
    source_name: &'scan StreamName,
    record_offset: usize,
}

/// The winning source of one face appearance, before its output copy.
enum AppearanceSource<'a> {
    Display(&'a AppearanceDefinition),
    Feature(&'a FeatureAppearanceAssignment<'a>),
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
        for (offset, token) in ctx
            .admit_iter(bytes, "scan SLDPRT appearance definitions")?
            .windows(const { crate::nonzero(VISUAL_PROPERTIES_CLASS.len()) })
            .enumerate()
        {
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

/// A validated name and color before any output copies are made.
struct AppearanceCandidate<'ctx> {
    name: String,
    color: Color,
    name_storage: ScopedReservation<'ctx>,
}

fn candidate_at<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    section: Section<'_>,
    packed_offset: usize,
) -> Result<Option<AppearanceCandidate<'ctx>>, cadmpeg_core::CodecError> {
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
    let name_reservation;
    let decoded;
    (decoded, name_reservation) =
        match ctx.utf16le_scoped_text(raw_name, count, false, "decode SLDPRT appearance name") {
            Ok(text) => text,
            Err(cadmpeg_core::CodecError::Malformed(_)) => return Ok(None),
            Err(error) => return Err(error),
        };
    if !ctx.any_by(
        decoded.chars(),
        |character| Ok(!character.is_whitespace()),
        "validate SLDPRT appearance name",
    )? {
        return Ok(None);
    }
    Ok(Some(AppearanceCandidate {
        name: decoded,
        color: packed_rgb(packed_color),
        name_storage: name_reservation,
    }))
}

fn materialize_candidate(
    ctx: &DecodeContext<'_>,
    section: Section<'_>,
    candidate: AppearanceCandidate<'_>,
    record_offset: usize,
) -> Result<AppearanceDefinition, cadmpeg_core::CodecError> {
    let trimmed = ctx.trim_text(&candidate.name, "trim SLDPRT appearance name")?;
    let name = ctx.copy_retained_text(trimmed, "retain SLDPRT appearance name")?;
    let color = candidate.color;
    drop(candidate.name);
    drop(candidate.name_storage);
    let source_name = clone_stream_name(ctx, section.source_stream())?;
    Ok(AppearanceDefinition {
        name,
        color,
        source_name,
        record_offset,
    })
}

fn definition_at(
    ctx: &DecodeContext<'_>,
    section: Section<'_>,
    packed_offset: usize,
    record_offset: usize,
) -> Result<Option<AppearanceDefinition>, cadmpeg_core::CodecError> {
    candidate_at(ctx, section, packed_offset)?
        .map(|candidate| materialize_candidate(ctx, section, candidate, record_offset))
        .transpose()
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

fn inline_definition<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    section: Section<'_>,
    start: usize,
    end: usize,
) -> Result<Option<(AppearanceDefinition, ScopedReservation<'ctx>)>, cadmpeg_core::CodecError> {
    let Some(bytes) = section.payload().get(start..end) else {
        return Ok(None);
    };
    let mut selected = None;
    let mut windows = bytes.windows(inline_visual::MARKER_VALUE.len()).enumerate();
    while let Some(relative) = ctx.find_map(
        windows.by_ref(),
        |(relative, marker)| Ok((marker == inline_visual::MARKER_VALUE).then_some(relative)),
        "scan inline SLDPRT appearances",
    )? {
        let offset = start + relative;
        let Some(candidate) = candidate_at(ctx, section, offset + inline_visual::PACKED_COLOR)?
        else {
            continue;
        };
        if selected.is_some() {
            return Ok(None);
        }
        selected = Some((candidate, offset));
    }
    selected
        .map(|(candidate, offset)| {
            ctx.with_scoped_storage("SLDPRT inline appearance candidate", || {
                materialize_candidate(ctx, section, candidate, offset)
            })
        })
        .transpose()
}

/// Decode body/default and face-local assignments from `DisplayLists`.
fn display_assignments(
    ctx: &DecodeContext<'_>,
    section: Section<'_>,
    faces: &[DisplayFace],
) -> Result<Vec<DisplayAppearanceAssignment>, cadmpeg_core::CodecError> {
    let _classes_storage;
    let classes;
    (classes, _classes_storage) = ctx
        .with_scoped_storage("SLDPRT appearance class workspace", || {
            crate::tessellation::class_intervals(ctx, section.payload())
        })?;
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
        let definition = inline_definition(
            ctx,
            section,
            face.metadata.start(),
            face.metadata.end().min(class.content.end()),
        )?;
        if let Some((definition, definition_storage)) = definition {
            let definition = definition_storage.commit_value(definition)?;
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
    let mut face_order_storage = ctx.reserve_scoped(0, "order SLDPRT appearance faces")?;
    let mut faces_by_start = Vec::new();
    let mut previous_body_end = 0;
    for class in ctx.admit_iter(&classes, "scan SLDPRT display_assignments values")? {
        if class.name != "uoBodyPropInfo_c" {
            continue;
        }
        let body_start = std::mem::replace(&mut previous_body_end, class.content.end());
        let Some((definition, definition_storage)) =
            inline_definition(ctx, section, class.content.start(), class.content.end())?
        else {
            continue;
        };
        if faces_by_start.len() != faces.len() {
            faces_by_start = face_order_storage.with_storage(|| {
                ctx.collect_vec(0..faces.len(), "order SLDPRT appearance faces")
            })?;
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
        let mut remaining = faces_by_start.get(first..).unwrap_or_default().iter();
        while let Some(&table_index) =
            ctx.next_charged(&mut remaining, "match SLDPRT body appearance faces")?
        {
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
            let definition = definition_storage.commit_value(definition)?;
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
fn containing_class<'a, 'c>(
    ctx: &DecodeContext<'_>,
    classes: &'a [crate::tessellation::ClassInterval<'c>],
    offset: usize,
) -> Result<Option<&'a crate::tessellation::ClassInterval<'c>>, cadmpeg_core::CodecError> {
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

struct FeatureAppearanceRecord {
    source_id: FeatureSourceId,
    timestamp: u32,
    packed_color: u32,
    offset: usize,
}

/// Visits every structurally valid feature appearance in source order.
fn visit_feature_appearances<'scan>(
    ctx: &DecodeContext<'_>,
    scan: &'scan ContainerScan,
    mut visit: impl FnMut(
        Section<'scan>,
        FeatureAppearanceRecord,
    ) -> Result<(), cadmpeg_core::CodecError>,
) -> Result<(), cadmpeg_core::CodecError> {
    for section in scan
        .sections(ctx)?
        .filter(|section| section.name() == Some("ThirdPtyStore/VisualStates"))
    {
        let bytes = section.payload();
        let _classes_storage;
        let classes;
        (classes, _classes_storage) = ctx
            .with_scoped_storage("SLDPRT appearance class workspace", || {
                crate::tessellation::class_intervals(ctx, bytes)
            })?;
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
            visit(
                section,
                FeatureAppearanceRecord {
                    source_id: feature_source_id,
                    timestamp: feature_timestamp,
                    packed_color,
                    offset: record_offset,
                },
            )?;
        }
    }
    Ok(())
}

/// Decode feature-source assignments from `ThirdPtyStore/VisualStates`.
pub(crate) fn feature_assignments<'scan>(
    ctx: &DecodeContext<'_>,
    scan: &'scan ContainerScan,
) -> Result<Vec<FeatureAppearanceAssignment<'scan>>, cadmpeg_core::CodecError> {
    let mut assignments = Vec::new();
    visit_feature_appearances(ctx, scan, |section, record| {
        let source_name = section.source_stream();
        ctx.push_vec(
            &mut assignments,
            FeatureAppearanceAssignment {
                feature_source_id: record.source_id,
                feature_timestamp: record.timestamp,
                packed_color: record.packed_color,
                source_name,
                record_offset: record.offset,
            },
            "collect SLDPRT feature appearances",
        )
    })?;
    Ok(assignments)
}

/// Feature records and their unique-source index shared by one geometry pass.
/// Each is built at its first query and owns its temporary storage.
#[derive(Default)]
pub(crate) struct FeatureAppearances<'scan, 'ctx> {
    records: OnceCell<(
        Vec<FeatureAppearanceAssignment<'scan>>,
        ScopedReservation<'ctx>,
    )>,
    index: OnceCell<(
        HashMap<FeatureSourceId, Option<usize>>,
        ScopedReservation<'ctx>,
    )>,
}

impl<'scan, 'ctx> FeatureAppearances<'scan, 'ctx> {
    fn records(
        &self,
        ctx: &'ctx DecodeContext<'_>,
        scan: &'scan ContainerScan,
    ) -> Result<&[FeatureAppearanceAssignment<'scan>], cadmpeg_core::CodecError> {
        if self.records.get().is_none() {
            let records = ctx.with_scoped_storage("SLDPRT feature appearance workspace", || {
                feature_assignments(ctx, scan)
            })?;
            self.records.set(records).map_err(|_| {
                cadmpeg_core::CodecError::malformed("SLDPRT appearance records initialized twice")
            })?;
        }
        self.records
            .get()
            .map(|(records, _)| records.as_slice())
            .ok_or_else(|| cadmpeg_core::CodecError::malformed("missing SLDPRT appearance records"))
    }

    fn lookup(
        &self,
        ctx: &'ctx DecodeContext<'_>,
        scan: &'scan ContainerScan,
        source_id: FeatureSourceId,
    ) -> Result<Option<&FeatureAppearanceAssignment<'scan>>, cadmpeg_core::CodecError> {
        let records = self.records(ctx, scan)?;
        if self.index.get().is_none() {
            let index =
                ctx.with_scoped_storage("SLDPRT feature appearance index workspace", || {
                    let mut index = HashMap::<FeatureSourceId, Option<usize>>::new();
                    for (position, assignment) in ctx
                        .admit_iter(records, "scan SLDPRT feature appearance assignments")?
                        .enumerate()
                    {
                        if let Some(existing) = ctx.get_mut_hash_map(
                            &mut index,
                            &assignment.feature_source_id,
                            "index SLDPRT feature appearances",
                        )? {
                            if existing
                                .and_then(|position| records.get(position))
                                .is_some_and(|previous| {
                                    previous.feature_timestamp != assignment.feature_timestamp
                                        || previous.packed_color != assignment.packed_color
                                })
                            {
                                *existing = None;
                            }
                        } else {
                            ctx.insert_hash_map(
                                &mut index,
                                assignment.feature_source_id,
                                Some(position),
                                "index SLDPRT feature appearances",
                            )?;
                        }
                    }
                    Ok::<_, cadmpeg_core::CodecError>(index)
                })?;
            self.index.set(index).map_err(|_| {
                cadmpeg_core::CodecError::malformed("SLDPRT appearance index initialized twice")
            })?;
        }
        let (index, _) = self.index.get().ok_or_else(|| {
            cadmpeg_core::CodecError::malformed("missing SLDPRT appearance index")
        })?;
        Ok(ctx
            .get_hash_map(index, &source_id, "look up SLDPRT hash key")?
            .and_then(|position| *position)
            .and_then(|position| records.get(position)))
    }

    /// Collects source identities and releases all cached record and index storage.
    pub(crate) fn into_source_ids(
        self,
        ctx: &'ctx DecodeContext<'_>,
        scan: &'scan ContainerScan,
    ) -> Result<BTreeSet<FeatureSourceId>, cadmpeg_core::CodecError> {
        if let Some((records, _)) = self.records.get() {
            return ctx.collect_btree_set(
                records
                    .iter()
                    .map(|assignment| assignment.feature_source_id),
                "index SLDPRT feature appearance sources",
            );
        }
        let mut sources = BTreeSet::new();
        visit_feature_appearances(ctx, scan, |_, record| {
            ctx.insert_btree_set(
                &mut sources,
                record.source_id,
                "index SLDPRT feature appearance sources",
            )?;
            Ok(())
        })?;
        Ok(sources)
    }
}

/// Resolve the verified `DisplayLists` precedence: body, feature, then face.
pub(crate) fn resolve_display_appearances<'scan, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'scan ContainerScan,
    feature_appearances: &FeatureAppearances<'scan, 'ctx>,
    section: Section<'_>,
    faces: &[DisplayFace],
) -> Result<ResolvedDisplayAppearances, cadmpeg_core::CodecError> {
    let (native_assignments, assignment_storage) = ctx
        .with_scoped_storage("SLDPRT display assignment workspace", || {
            display_assignments(ctx, section, faces)
        })?;
    let mut winner_storage = ctx.reserve_scoped(0, "SLDPRT appearance winner workspace")?;
    let mut winners = BTreeMap::new();
    for assignment in ctx.admit_iter(
        &native_assignments,
        "scan SLDPRT resolve_display_appearances values",
    )? {
        if let DisplayAppearanceTarget::Body(face_indexes) = &assignment.target {
            for &face_index in
                ctx.admit_iter(face_indexes, "scan SLDPRT body appearance targets")?
            {
                winner_storage.with_storage(|| {
                    ctx.insert_btree_map(
                        &mut winners,
                        face_index,
                        AppearanceSource::Display(&assignment.definition),
                        "index SLDPRT appearance winners",
                    )
                })?;
            }
        }
    }
    let mut matched_feature_sources = BTreeSet::new();
    for (face_index, face) in ctx
        .admit_iter(faces, "scan SLDPRT feature appearance faces")?
        .enumerate()
    {
        let Some(source_id) = face.feature_source_id(ctx)? else {
            continue;
        };
        let Some(assignment) = feature_appearances.lookup(ctx, scan, source_id)? else {
            continue;
        };
        ctx.insert_btree_set(
            &mut matched_feature_sources,
            source_id,
            "collect matched SLDPRT appearance sources",
        )?;
        winner_storage.with_storage(|| {
            ctx.insert_btree_map(
                &mut winners,
                face_index,
                AppearanceSource::Feature(assignment),
                "index SLDPRT appearance winners",
            )
        })?;
    }
    for assignment in ctx.admit_iter(&native_assignments, "index SLDPRT face appearances")? {
        if let DisplayAppearanceTarget::Face(face_index) = assignment.target {
            winner_storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut winners,
                    face_index,
                    AppearanceSource::Display(&assignment.definition),
                    "index SLDPRT appearance winners",
                )
            })?;
        }
    }
    let mut by_face = BTreeMap::new();
    for (face_index, source) in ctx.admit_iter(winners, "emit SLDPRT appearance winners")? {
        let definition = match source {
            AppearanceSource::Display(definition) => copy_definition(ctx, definition)?,
            AppearanceSource::Feature(assignment) => AppearanceDefinition {
                name: ctx.copy_retained_text(
                    "SolidWorks feature appearance",
                    "copy SLDPRT appearance name",
                )?,
                color: packed_rgb(assignment.packed_color),
                source_name: clone_stream_name(ctx, assignment.source_name)?,
                record_offset: assignment.record_offset,
            },
        };
        ctx.insert_btree_map(
            &mut by_face,
            face_index,
            definition,
            "index SLDPRT face appearances",
        )?;
    }
    drop(winner_storage);
    drop((native_assignments, assignment_storage));
    Ok(ResolvedDisplayAppearances {
        by_face,
        matched_feature_sources,
    })
}

#[cfg(test)]
mod tests;
