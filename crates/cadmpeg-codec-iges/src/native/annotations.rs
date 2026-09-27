// SPDX-License-Identifier: Apache-2.0

//! Native retention of annotation records: the arena types, the text-run
//! builder, and the pass that fills the `annotations` arena while recording
//! counted-tail verdicts.

use super::{collect_native_items, OverdeclaredCounts};
use crate::decode_resource::{collect_result_vec, format_retained};
use crate::directory::{DirectoryEntry, UseFlag};
use crate::entities::annotation::{
    classify, parameterized_curve_type, section_boundary_type, AnnotationKind,
};
use crate::global::GlobalTable;
use crate::graph::expectation::{ExpectationLabel, ReferenceExpectation};
use crate::graph::ParameterResolver;
use crate::parameter::ParameterRecord;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(super) struct NativeTextRun {
    declared_character_count: Option<i64>,
    text: Option<Vec<u8>>,
    box_size: [Option<f64>; 2],
    font_code: Option<i64>,
    font_definition: Option<String>,
    slant_angle: Option<f64>,
    rotation_angle: Option<f64>,
    mirror: Option<i64>,
    vertical: Option<i64>,
    start: [Option<f64>; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(super) struct NativeNewTextRun {
    fixed_or_variable: Option<i64>,
    character_size: [Option<f64>; 2],
    character_spacing: Option<f64>,
    line_spacing: Option<f64>,
    font_style: Option<i64>,
    character_angle: Option<f64>,
    control_codes: Option<Vec<u8>>,
    text: NativeTextRun,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum NativeAnnotation {
    GeneralNote {
        id: String,
        source_entity: String,
        form: i64,
        declared_string_count: Option<i64>,
        strings: Vec<NativeTextRun>,
        transformation: Option<String>,
    },
    NewGeneralNote {
        id: String,
        source_entity: String,
        containment_size: [Option<f64>; 2],
        justification: Option<i64>,
        containment_origin: [Option<f64>; 3],
        containment_angle: Option<f64>,
        baseline_origin: [Option<f64>; 3],
        normal_interline_spacing: Option<f64>,
        declared_string_count: Option<i64>,
        strings: Vec<NativeNewTextRun>,
        transformation: Option<String>,
    },
    Leader {
        id: String,
        source_entity: String,
        form: i64,
        declared_segment_count: Option<i64>,
        arrowhead_size: [Option<f64>; 2],
        arrowhead: [Option<f64>; 3],
        segment_tails: Vec<[Option<f64>; 3]>,
        transformation: Option<String>,
    },
    AngularDimension {
        id: String,
        source_entity: String,
        note: Option<String>,
        witnesses: [Option<String>; 2],
        vertex: [Option<f64>; 2],
        radius: Option<f64>,
        leaders: [Option<String>; 2],
        transformation: Option<String>,
    },
    CurveDimension {
        id: String,
        source_entity: String,
        note: Option<String>,
        curves: [Option<String>; 2],
        leaders: [Option<String>; 2],
        witnesses: [Option<String>; 2],
        transformation: Option<String>,
    },
    DiameterDimension {
        id: String,
        source_entity: String,
        note: Option<String>,
        leaders: [Option<String>; 2],
        center: [Option<f64>; 2],
        transformation: Option<String>,
    },
    FlagNote {
        id: String,
        source_entity: String,
        origin: [Option<f64>; 3],
        rotation: Option<f64>,
        note: Option<String>,
        declared_leader_count: Option<i64>,
        leaders: Vec<Option<String>>,
        transformation: Option<String>,
    },
    GeneralLabel {
        id: String,
        source_entity: String,
        note: Option<String>,
        declared_leader_count: Option<i64>,
        leaders: Vec<Option<String>>,
        transformation: Option<String>,
    },
    LinearDimension {
        id: String,
        source_entity: String,
        form: i64,
        note: Option<String>,
        leaders: [Option<String>; 2],
        witnesses: [Option<String>; 2],
        transformation: Option<String>,
    },
    OrdinateDimension {
        id: String,
        source_entity: String,
        form: i64,
        note: Option<String>,
        ordinate: Option<String>,
        supplemental_leader: Option<String>,
        transformation: Option<String>,
    },
    PointDimension {
        id: String,
        source_entity: String,
        note: Option<String>,
        leader: Option<String>,
        enclosure: Option<String>,
        transformation: Option<String>,
    },
    RadiusDimension {
        id: String,
        source_entity: String,
        form: i64,
        note: Option<String>,
        leaders: [Option<String>; 2],
        center: [Option<f64>; 2],
        transformation: Option<String>,
    },
    GeneralSymbol {
        id: String,
        source_entity: String,
        form: i64,
        note: Option<String>,
        declared_geometry_count: Option<i64>,
        geometry: Vec<Option<String>>,
        declared_leader_count: Option<i64>,
        leaders: Vec<Option<String>>,
        transformation: Option<String>,
    },
    SectionedArea {
        id: String,
        source_entity: String,
        form: i64,
        boundary: Option<String>,
        fill_pattern: Option<i64>,
        pattern_anchor: [Option<f64>; 3],
        pattern_spacing: Option<f64>,
        pattern_angle: Option<f64>,
        declared_island_count: Option<i64>,
        islands: Vec<Option<String>>,
        transformation: Option<String>,
    },
}

/// One admitted directory entry under construction: its record, its
/// precomputed clamped primary end, and the resolver context the link
/// builders read.
struct Subject<'a, 'ctx> {
    sequence: u32,
    form: i64,
    record: Option<&'a ParameterRecord>,
    primary_end: usize,
    entries: &'a BTreeMap<u32, &'a DirectoryEntry>,
    parameter_resolver: &'a ParameterResolver<'a, 'ctx>,
    ctx: &'a DecodeContext<'ctx>,
    v5_null_string_rule: bool,
}

impl Subject<'_, '_> {
    fn id(&self) -> Result<String, CodecError> {
        format_retained(self.ctx, format_args!("iges:presentation:annotation#D{}", self.sequence), "iges native annotation id")
    }

    fn source_entity(&self) -> Result<String, CodecError> {
        format_retained(self.ctx, format_args!("iges:entity:directory#{}", self.sequence), "iges native annotation source")
    }

    fn annotation_link_id(&self, sequence: u32) -> Result<String, CodecError> {
        format_retained(self.ctx, format_args!("iges:presentation:annotation#D{sequence}"), "iges native annotation link")
    }

    fn entity_link_id(&self, sequence: u32) -> Result<String, CodecError> {
        format_retained(self.ctx, format_args!("iges:entity:directory#{sequence}"), "iges native annotation entity link")
    }

    fn counted_tail(
        &self,
        count_index: usize,
        stride: usize,
        overdeclared: &mut OverdeclaredCounts,
    ) -> usize {
        overdeclared.counted_tail(
            self.sequence,
            self.record,
            self.primary_end,
            count_index,
            stride,
        )
    }

    fn counted_tail_at(
        &self,
        count_index: usize,
        item_start: usize,
        stride: usize,
        overdeclared: &mut OverdeclaredCounts,
    ) -> usize {
        overdeclared.counted_tail_at(
            self.sequence,
            self.record,
            self.primary_end,
            count_index,
            item_start,
            stride,
        )
    }

    fn text_run(&self, start: usize) -> Result<NativeTextRun, CodecError> {
        let record = self.record;
        let font_code = record.and_then(|record| record.integer(start + 3));
        let text = record.and_then(|record| record.string(start + 11));
        let v5_null_string = self.v5_null_string_rule
            && record.and_then(|record| record.integer(1)) == Some(1)
            && record.and_then(|record| record.integer(start)) == Some(1)
            && text.is_some_and(|value| value == b" ");
        let font_definition = match font_code.filter(|value| *value < 0) {
            Some(value) => self
                .parameter_resolver
                .resolve_negative_type(self.sequence, start + 3, value, 310, &[0])?
                .map(|sequence| format_retained(self.ctx, format_args!("iges:presentation:text-font#D{sequence}"), "iges native text run font"))
                .transpose()?,
            None => None,
        };
        Ok(NativeTextRun {
            declared_character_count: record.and_then(|record| record.integer(start)),
            text: (!v5_null_string)
                .then_some(text)
                .flatten()
                .map(|bytes| self.ctx.copy_retained(bytes, "iges native text run bytes"))
                .transpose()?,
            box_size: [
                record.and_then(|record| record.number(start + 1)),
                record.and_then(|record| record.number(start + 2)),
            ],
            font_code,
            font_definition,
            slant_angle: record.and_then(|record| record.number(start + 4)),
            rotation_angle: record.and_then(|record| record.number(start + 5)),
            mirror: record.and_then(|record| record.integer(start + 6)),
            vertical: record.and_then(|record| record.integer(start + 7)),
            start: [
                record.and_then(|record| record.number(start + 8)),
                record.and_then(|record| record.number(start + 9)),
                record.and_then(|record| record.number(start + 10)),
            ],
        })
    }

    fn note_link(&self, index: usize) -> Result<Option<String>, CodecError> {
        let Some(sequence) = self.record.and_then(|record| record.integer(index)) else {
            return Ok(None);
        };
        self
            .parameter_resolver
            .resolve_type(self.sequence, index, sequence, 212, &[0])?
            .map(|sequence| self.annotation_link_id(sequence))
            .transpose()
    }

    fn leader_link(&self, index: usize) -> Result<Option<String>, CodecError> {
        let Some(sequence) = self.record.and_then(|record| record.integer(index)) else {
            return Ok(None);
        };
        self
            .parameter_resolver
            .resolve(
                self.sequence,
                index,
                sequence,
                ReferenceExpectation::Named(ExpectationLabel::Type214Form1Through12),
                |target| target.entity_type == 214 && matches!(target.form, 1..=12),
            )?
            .map(|sequence| self.annotation_link_id(sequence))
            .transpose()
    }

    fn leader_list(
        &self,
        count_index: usize,
        leader_start: usize,
        overdeclared: &mut OverdeclaredCounts,
    ) -> Result<Vec<Option<String>>, CodecError> {
        let count = self.counted_tail_at(count_index, leader_start, 1, overdeclared);
        collect_result_vec(self.ctx, count, "iges native annotation leader slots", |offset| self.leader_link(leader_start + offset))
    }

    fn witness_link(&self, index: usize) -> Result<Option<String>, CodecError> {
        let Some(sequence) = self.record.and_then(|record| record.integer(index)) else {
            return Ok(None);
        };
        self
            .parameter_resolver
            .resolve_type(self.sequence, index, sequence, 106, &[40])?
            .map(|sequence| self.entity_link_id(sequence))
            .transpose()
    }

    fn curve_link(
        &self,
        index: usize,
        global_table: GlobalTable,
    ) -> Result<Option<String>, CodecError> {
        let Some(sequence) = self.record.and_then(|record| record.integer(index)) else {
            return Ok(None);
        };
        self
            .parameter_resolver
            .resolve(
                self.sequence,
                index,
                sequence,
                ReferenceExpectation::Named(ExpectationLabel::ParameterizedCurve),
                |target| {
                    parameterized_curve_type(target)
                        && target.status.is_physically_dependent()
                        && target.status.use_flag(global_table) == Some(UseFlag::Annotation)
                },
            )?
            .map(|sequence| self.entity_link_id(sequence))
            .transpose()
    }

    fn ordinate_link(&self, index: usize) -> Result<Option<String>, CodecError> {
        let Some(sequence) = self.record.and_then(|record| record.integer(index)) else {
            return Ok(None);
        };
        self
            .parameter_resolver
            .resolve(
                self.sequence,
                index,
                sequence,
                ReferenceExpectation::Named(ExpectationLabel::Type106Form40OrLeader),
                |target| {
                    (target.entity_type == 106 && target.form == 40)
                        || (target.entity_type == 214 && matches!(target.form, 1..=12))
                },
            )?
            .map(|sequence| {
                self.entries
                    .get(&sequence)
                    .filter(|target| target.entity_type == 214)
                    .map_or_else(|| self.entity_link_id(sequence), |_| self.annotation_link_id(sequence))
            })
            .transpose()
    }

    fn enclosure_link(
        &self,
        index: usize,
        global_table: GlobalTable,
    ) -> Result<Option<String>, CodecError> {
        let Some(sequence) = self.record.and_then(|record| record.integer(index)) else {
            return Ok(None);
        };
        self
            .parameter_resolver
            .resolve(
                self.sequence,
                index,
                sequence,
                ReferenceExpectation::Named(ExpectationLabel::PointDimensionEnclosure),
                |target| {
                    matches!(
                        (target.entity_type, target.form),
                        (100 | 102, 0) | (106, 63)
                    ) && target.status.is_physically_dependent()
                        && target.status.use_flag(global_table) == Some(UseFlag::Annotation)
                },
            )?
            .map(|sequence| self.entity_link_id(sequence))
            .transpose()
    }

    fn geometry_link(
        &self,
        index: usize,
        global_table: GlobalTable,
    ) -> Result<Option<String>, CodecError> {
        let Some(sequence) = self.record.and_then(|record| record.integer(index)) else {
            return Ok(None);
        };
        self
            .parameter_resolver
            .resolve(
                self.sequence,
                index,
                sequence,
                ReferenceExpectation::Named(ExpectationLabel::SubordinateAnnotationGeometry),
                |target| {
                    target.status.is_physically_dependent()
                        && target.status.use_flag(global_table) == Some(UseFlag::Annotation)
                },
            )?
            .map(|sequence| self.entity_link_id(sequence))
            .transpose()
    }

    fn section_boundary_link(&self, index: usize) -> Result<Option<String>, CodecError> {
        let Some(sequence) = self.record.and_then(|record| record.integer(index)) else {
            return Ok(None);
        };
        self
            .parameter_resolver
            .resolve(
                self.sequence,
                index,
                sequence,
                ReferenceExpectation::Named(ExpectationLabel::SectionBoundaryEntity),
                section_boundary_type,
            )?
            .map(|sequence| self.entity_link_id(sequence))
            .transpose()
    }
}

fn general_note(
    subject: &Subject<'_, '_>,
    transformation: Option<String>,
    overdeclared: &mut OverdeclaredCounts,
) -> Result<NativeAnnotation, CodecError> {
    let record = subject.record;
    let count = subject.counted_tail(1, 12, overdeclared);
    Ok(NativeAnnotation::GeneralNote {
        id: subject.id()?,
        source_entity: subject.source_entity()?,
        form: subject.form,
        declared_string_count: record.and_then(|record| record.integer(1)),
        strings: collect_result_vec(subject.ctx, count, "iges native general note text run slots", |index| subject.text_run(2 + index * 12))?,
        transformation,
    })
}

fn new_general_note(
    subject: &Subject<'_, '_>,
    transformation: Option<String>,
    overdeclared: &mut OverdeclaredCounts,
) -> Result<NativeAnnotation, CodecError> {
    let record = subject.record;
    let count = subject.counted_tail(12, 20, overdeclared);
    Ok(NativeAnnotation::NewGeneralNote {
        id: subject.id()?,
        source_entity: subject.source_entity()?,
        containment_size: [
            record.and_then(|record| record.number(1)),
            record.and_then(|record| record.number(2)),
        ],
        justification: record.and_then(|record| record.integer(3)),
        containment_origin: [
            record.and_then(|record| record.number(4)),
            record.and_then(|record| record.number(5)),
            record.and_then(|record| record.number(6)),
        ],
        containment_angle: record.and_then(|record| record.number(7)),
        baseline_origin: [
            record.and_then(|record| record.number(8)),
            record.and_then(|record| record.number(9)),
            record.and_then(|record| record.number(10)),
        ],
        normal_interline_spacing: record.and_then(|record| record.number(11)),
        declared_string_count: record.and_then(|record| record.integer(12)),
        strings: collect_result_vec(subject.ctx, count, "iges native new note text run slots", |index| -> Result<NativeNewTextRun, CodecError> {
                let start = 13 + index * 20;
                Ok(NativeNewTextRun {
                    fixed_or_variable: record.and_then(|record| record.integer(start)),
                    character_size: [
                        record.and_then(|record| record.number(start + 1)),
                        record.and_then(|record| record.number(start + 2)),
                    ],
                    character_spacing: record.and_then(|record| record.number(start + 3)),
                    line_spacing: record.and_then(|record| record.number(start + 4)),
                    font_style: record.and_then(|record| record.integer(start + 5)),
                    character_angle: record.and_then(|record| record.number(start + 6)),
                    control_codes: record
                        .and_then(|record| record.string(start + 7))
                        .map(|bytes| subject.ctx.copy_retained(bytes, "iges native new note control codes"))
                        .transpose()?,
                    // A 213 text block is the 212 layout shifted by its
                    // eight-token prefix.
                    text: subject.text_run(start + 8)?,
                })
            })?,
        transformation,
    })
}

fn leader(
    subject: &Subject<'_, '_>,
    transformation: Option<String>,
    overdeclared: &mut OverdeclaredCounts,
) -> Result<NativeAnnotation, CodecError> {
    let record = subject.record;
    let count = subject.counted_tail_at(1, 7, 2, overdeclared);
    let z = record.and_then(|record| record.number(4));
    Ok(NativeAnnotation::Leader {
        id: subject.id()?,
        source_entity: subject.source_entity()?,
        form: subject.form,
        declared_segment_count: record.and_then(|record| record.integer(1)),
        arrowhead_size: [
            record.and_then(|record| record.number(2)),
            record.and_then(|record| record.number(3)),
        ],
        arrowhead: [
            record.and_then(|record| record.number(5)),
            record.and_then(|record| record.number(6)),
            z,
        ],
        segment_tails: collect_result_vec(subject.ctx, count, "iges native leader segment tail slots", |index| {
                Ok([
                    record.and_then(|record| record.number(7 + index * 2)),
                    record.and_then(|record| record.number(8 + index * 2)),
                    z,
                ])
            })?,
        transformation,
    })
}

fn flag_note(
    subject: &Subject<'_, '_>,
    transformation: Option<String>,
    overdeclared: &mut OverdeclaredCounts,
) -> Result<NativeAnnotation, CodecError> {
    let record = subject.record;
    let leaders = subject.leader_list(6, 7, overdeclared)?;
    Ok(NativeAnnotation::FlagNote {
        id: subject.id()?,
        source_entity: subject.source_entity()?,
        origin: [
            record.and_then(|record| record.number(1)),
            record.and_then(|record| record.number(2)),
            record.and_then(|record| record.number(3)),
        ],
        rotation: record.and_then(|record| record.number(4)),
        note: subject.note_link(5)?,
        declared_leader_count: record.and_then(|record| record.integer(6)),
        leaders,
        transformation,
    })
}

fn general_label(
    subject: &Subject<'_, '_>,
    transformation: Option<String>,
    overdeclared: &mut OverdeclaredCounts,
) -> Result<NativeAnnotation, CodecError> {
    let record = subject.record;
    let leaders = subject.leader_list(2, 3, overdeclared)?;
    Ok(NativeAnnotation::GeneralLabel {
        id: subject.id()?,
        source_entity: subject.source_entity()?,
        note: subject.note_link(1)?,
        declared_leader_count: record.and_then(|record| record.integer(2)),
        leaders,
        transformation,
    })
}

fn general_symbol(
    subject: &Subject<'_, '_>,
    transformation: Option<String>,
    global_table: GlobalTable,
) -> Result<NativeAnnotation, CodecError> {
    let record = subject.record;
    let end = subject.primary_end;
    let declared_geometry_count = record.and_then(|record| record.integer(2));
    // The leader count follows the declared geometry span, even when that
    // span cannot be admitted. This preserves the second declaration without
    // allowing an invalid count to alias a geometry pointer.
    let declared_leader_count_index = declared_geometry_count
        .and_then(|count| usize::try_from(count).ok())
        .and_then(|count| 3_usize.checked_add(count));
    let declared_leader_count = declared_leader_count_index
        .and_then(|index| record.and_then(|record| record.integer(index)));
    // On any checked failure the tuple defaults to (0, 0, 0), so
    // leader_count_index is read only when leader_count > 0 admitted it.
    let (geometry_count, leader_count_index, leader_count) = record
        .and_then(|record| record.count_with_stride_before(2, 1, end))
        .and_then(|geometry_count| {
            let leader_count_index = 3_usize.checked_add(geometry_count)?;
            let leader_count = record
                .and_then(|record| record.count_with_stride_before(leader_count_index, 1, end))?;
            let finish = leader_count_index
                .checked_add(1)?
                .checked_add(leader_count)?;
            (finish <= end).then_some((geometry_count, leader_count_index, leader_count))
        })
        .unwrap_or_default();
    Ok(NativeAnnotation::GeneralSymbol {
        id: subject.id()?,
        source_entity: subject.source_entity()?,
        form: subject.form,
        note: subject.note_link(1)?,
        declared_geometry_count,
        geometry: collect_result_vec(subject.ctx, geometry_count, "iges native symbol geometry slots", |offset| subject.geometry_link(3 + offset, global_table))?,
        declared_leader_count,
        leaders: collect_result_vec(subject.ctx, leader_count, "iges native symbol leader slots", |offset| subject.leader_link(leader_count_index + 1 + offset))?,
        transformation,
    })
}

fn sectioned_area(
    subject: &Subject<'_, '_>,
    transformation: Option<String>,
    overdeclared: &mut OverdeclaredCounts,
) -> Result<NativeAnnotation, CodecError> {
    let record = subject.record;
    let island_count = subject.counted_tail_at(8, 9, 1, overdeclared);
    Ok(NativeAnnotation::SectionedArea {
        id: subject.id()?,
        source_entity: subject.source_entity()?,
        form: subject.form,
        boundary: subject.section_boundary_link(1)?,
        fill_pattern: record.and_then(|record| record.integer(2)),
        pattern_anchor: [
            record.and_then(|record| record.number(3)),
            record.and_then(|record| record.number(4)),
            record.and_then(|record| record.number(5)),
        ],
        pattern_spacing: record.and_then(|record| record.number(6)),
        pattern_angle: record.and_then(|record| record.number(7)),
        declared_island_count: record.and_then(|record| record.integer(8)),
        islands: collect_result_vec(subject.ctx, island_count, "iges native section island slots", |offset| subject.section_boundary_link(9 + offset))?,
        transformation,
    })
}

pub(super) fn build(
    directory: &[DirectoryEntry],
    by_directory: &BTreeMap<u32, &ParameterRecord>,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    parameter_resolver: &ParameterResolver<'_, '_>,
    clamped_primary_end: &impl Fn(u32, &ParameterRecord) -> usize,
    overdeclared_counts: &mut OverdeclaredCounts,
    global_table: GlobalTable,
    ctx: &DecodeContext<'_>,
) -> Result<Vec<NativeAnnotation>, CodecError> {
    collect_native_items(
        ctx,
        directory.iter().filter_map(|entry| classify(entry.entity_type, entry.form).map(|kind| (entry, kind))),
        "iges native annotation slots",
        |(entry, kind)| -> Result<NativeAnnotation, CodecError> {
            let record = by_directory.get(&entry.sequence).copied();
            let subject = Subject {
                sequence: entry.sequence,
                form: entry.form,
                record,
                primary_end: record.map_or(0, |record| clamped_primary_end(entry.sequence, record)),
                entries,
                parameter_resolver,
                ctx,
                v5_null_string_rule: global_table == GlobalTable::V5_0
                    && matches!(kind, AnnotationKind::GeneralNote),
            };
            let transformation = (entry.transform > 0)
                .then(|| format_retained(ctx, format_args!("iges:native:transformation#D{}", entry.transform), "iges native annotation transformation"))
                .transpose()?;
            Ok(match kind {
                AnnotationKind::GeneralNote => {
                    general_note(&subject, transformation, overdeclared_counts)?
                }
                AnnotationKind::NewGeneralNote => {
                    new_general_note(&subject, transformation, overdeclared_counts)?
                }
                AnnotationKind::Leader => leader(&subject, transformation, overdeclared_counts)?,
                AnnotationKind::FlagNote => {
                    flag_note(&subject, transformation, overdeclared_counts)?
                }
                AnnotationKind::GeneralLabel => {
                    general_label(&subject, transformation, overdeclared_counts)?
                }
                AnnotationKind::GeneralSymbol => {
                    general_symbol(&subject, transformation, global_table)?
                }
                AnnotationKind::SectionedArea => {
                    sectioned_area(&subject, transformation, overdeclared_counts)?
                }
                AnnotationKind::AngularDimension => NativeAnnotation::AngularDimension {
                    id: subject.id()?,
                    source_entity: subject.source_entity()?,
                    note: subject.note_link(1)?,
                    witnesses: [subject.witness_link(2)?, subject.witness_link(3)?],
                    vertex: [
                        record.and_then(|record| record.number(4)),
                        record.and_then(|record| record.number(5)),
                    ],
                    radius: record.and_then(|record| record.number(6)),
                    leaders: [subject.leader_link(7)?, subject.leader_link(8)?],
                    transformation,
                },
                AnnotationKind::CurveDimension => NativeAnnotation::CurveDimension {
                    id: subject.id()?,
                    source_entity: subject.source_entity()?,
                    note: subject.note_link(1)?,
                    curves: [
                        subject.curve_link(2, global_table)?,
                        subject.curve_link(3, global_table)?,
                    ],
                    leaders: [subject.leader_link(4)?, subject.leader_link(5)?],
                    witnesses: [subject.witness_link(6)?, subject.witness_link(7)?],
                    transformation,
                },
                AnnotationKind::DiameterDimension => NativeAnnotation::DiameterDimension {
                    id: subject.id()?,
                    source_entity: subject.source_entity()?,
                    note: subject.note_link(1)?,
                    leaders: [subject.leader_link(2)?, subject.leader_link(3)?],
                    center: [
                        record.and_then(|record| record.number(4)),
                        record.and_then(|record| record.number(5)),
                    ],
                    transformation,
                },
                AnnotationKind::LinearDimension => NativeAnnotation::LinearDimension {
                    id: subject.id()?,
                    source_entity: subject.source_entity()?,
                    form: subject.form,
                    note: subject.note_link(1)?,
                    leaders: [subject.leader_link(2)?, subject.leader_link(3)?],
                    witnesses: [subject.witness_link(4)?, subject.witness_link(5)?],
                    transformation,
                },
                AnnotationKind::OrdinateDimension => NativeAnnotation::OrdinateDimension {
                    id: subject.id()?,
                    source_entity: subject.source_entity()?,
                    form: subject.form,
                    note: subject.note_link(1)?,
                    ordinate: subject.ordinate_link(2)?,
                    supplemental_leader: if subject.form == 1 {
                        subject.leader_link(3)?
                    } else {
                        None
                    },
                    transformation,
                },
                AnnotationKind::PointDimension => NativeAnnotation::PointDimension {
                    id: subject.id()?,
                    source_entity: subject.source_entity()?,
                    note: subject.note_link(1)?,
                    leader: subject.leader_link(2)?,
                    enclosure: subject.enclosure_link(3, global_table)?,
                    transformation,
                },
                AnnotationKind::RadiusDimension => NativeAnnotation::RadiusDimension {
                    id: subject.id()?,
                    source_entity: subject.source_entity()?,
                    form: subject.form,
                    note: subject.note_link(1)?,
                    leaders: [
                        subject.leader_link(2)?,
                        if subject.form == 1 {
                            subject.leader_link(5)?
                        } else {
                            None
                        },
                    ],
                    center: [
                        record.and_then(|record| record.number(3)),
                        record.and_then(|record| record.number(4)),
                    ],
                    transformation,
                },
            })
        },
    )
}
