// SPDX-License-Identifier: Apache-2.0

//! Native retention of annotation records: the arena types, the text-run
//! builder, and the pass that fills the `annotations` arena while recording
//! counted-tail verdicts.

use super::OverdeclaredCounts;
use crate::directory::DirectoryEntry;
use crate::entities::annotation::{
    classify, parameterized_curve_type, section_boundary_type, AnnotationKind,
};
use crate::global::GlobalTable;
use crate::graph::expectation::ReferenceExpectation;
use crate::graph::ParameterResolver;
use crate::parameter::ParameterRecord;
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
struct Subject<'a> {
    sequence: u32,
    form: i64,
    record: Option<&'a ParameterRecord>,
    primary_end: usize,
    entries: &'a BTreeMap<u32, &'a DirectoryEntry>,
    parameter_resolver: &'a ParameterResolver<'a>,
    v5_null_string_rule: bool,
}

impl Subject<'_> {
    fn id(&self) -> String {
        format!("iges:presentation:annotation#D{}", self.sequence)
    }

    fn source_entity(&self) -> String {
        format!("iges:entity:directory#{}", self.sequence)
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

    fn text_run(&self, start: usize) -> NativeTextRun {
        let record = self.record;
        let font_code = record.and_then(|record| record.integer(start + 3));
        let text = record.and_then(|record| record.string(start + 11));
        let v5_null_string = self.v5_null_string_rule
            && record.and_then(|record| record.integer(1)) == Some(1)
            && record.and_then(|record| record.integer(start)) == Some(1)
            && text.is_some_and(|value| value == b" ");
        NativeTextRun {
            declared_character_count: record.and_then(|record| record.integer(start)),
            text: (!v5_null_string)
                .then(|| text.map(<[u8]>::to_vec))
                .flatten(),
            box_size: [
                record.and_then(|record| record.number(start + 1)),
                record.and_then(|record| record.number(start + 2)),
            ],
            font_code,
            font_definition: font_code
                .filter(|value| *value < 0)
                .and_then(|value| {
                    self.parameter_resolver.resolve_negative(
                        self.sequence,
                        start + 3,
                        value,
                        ReferenceExpectation::Type310Form0,
                        |target| target.entity_type == 310 && target.form == 0,
                    )
                })
                .map(|sequence| format!("iges:presentation:text-font#D{sequence}")),
            slant_angle: record.and_then(|record| record.number(start + 4)),
            rotation_angle: record.and_then(|record| record.number(start + 5)),
            mirror: record.and_then(|record| record.integer(start + 6)),
            vertical: record.and_then(|record| record.integer(start + 7)),
            start: [
                record.and_then(|record| record.number(start + 8)),
                record.and_then(|record| record.number(start + 9)),
                record.and_then(|record| record.number(start + 10)),
            ],
        }
    }

    fn note_link(&self, index: usize) -> Option<String> {
        self.record
            .and_then(|record| record.integer(index))
            .and_then(|sequence| {
                self.parameter_resolver
                    .resolve_type(self.sequence, index, sequence, 212, &[0])
            })
            .map(|sequence| format!("iges:presentation:annotation#D{sequence}"))
    }

    fn leader_link(&self, index: usize) -> Option<String> {
        self.record
            .and_then(|record| record.integer(index))
            .and_then(|sequence| {
                self.parameter_resolver.resolve(
                    self.sequence,
                    index,
                    sequence,
                    ReferenceExpectation::Type214Form1Through12,
                    |target| target.entity_type == 214 && matches!(target.form, 1..=12),
                )
            })
            .map(|sequence| format!("iges:presentation:annotation#D{sequence}"))
    }

    fn leader_list(
        &self,
        count_index: usize,
        leader_start: usize,
        overdeclared: &mut OverdeclaredCounts,
    ) -> Vec<Option<String>> {
        (0..self.counted_tail_at(count_index, leader_start, 1, overdeclared))
            .map(|offset| self.leader_link(leader_start + offset))
            .collect()
    }

    fn witness_link(&self, index: usize) -> Option<String> {
        self.record
            .and_then(|record| record.integer(index))
            .and_then(|sequence| {
                self.parameter_resolver
                    .resolve_type(self.sequence, index, sequence, 106, &[40])
            })
            .map(|sequence| format!("iges:entity:directory#{sequence}"))
    }

    fn curve_link(&self, index: usize) -> Option<String> {
        self.record
            .and_then(|record| record.integer(index))
            .and_then(|sequence| {
                self.parameter_resolver.resolve(
                    self.sequence,
                    index,
                    sequence,
                    ReferenceExpectation::ParameterizedCurve,
                    |target| {
                        parameterized_curve_type(target)
                            && target.status.is_physically_dependent()
                            && target.status.use_flag == 1
                    },
                )
            })
            .map(|sequence| format!("iges:entity:directory#{sequence}"))
    }

    fn ordinate_link(&self, index: usize) -> Option<String> {
        self.record
            .and_then(|record| record.integer(index))
            .and_then(|sequence| {
                self.parameter_resolver.resolve(
                    self.sequence,
                    index,
                    sequence,
                    ReferenceExpectation::Type106Form40OrLeader,
                    |target| {
                        (target.entity_type == 106 && target.form == 40)
                            || (target.entity_type == 214 && matches!(target.form, 1..=12))
                    },
                )
            })
            .map(|sequence| {
                self.entries
                    .get(&sequence)
                    .filter(|target| target.entity_type == 214)
                    .map_or_else(
                        || format!("iges:entity:directory#{sequence}"),
                        |_| format!("iges:presentation:annotation#D{sequence}"),
                    )
            })
    }

    fn enclosure_link(&self, index: usize) -> Option<String> {
        self.record
            .and_then(|record| record.integer(index))
            .and_then(|sequence| {
                self.parameter_resolver.resolve(
                    self.sequence,
                    index,
                    sequence,
                    ReferenceExpectation::PointDimensionEnclosure,
                    |target| {
                        matches!(
                            (target.entity_type, target.form),
                            (100 | 102, 0) | (106, 63)
                        ) && target.status.is_physically_dependent()
                            && target.status.use_flag == 1
                    },
                )
            })
            .map(|sequence| format!("iges:entity:directory#{sequence}"))
    }

    fn geometry_link(&self, index: usize) -> Option<String> {
        self.record
            .and_then(|record| record.integer(index))
            .and_then(|sequence| {
                self.parameter_resolver.resolve(
                    self.sequence,
                    index,
                    sequence,
                    ReferenceExpectation::SubordinateAnnotationGeometry,
                    |target| target.status.is_physically_dependent() && target.status.use_flag == 1,
                )
            })
            .map(|sequence| format!("iges:entity:directory#{sequence}"))
    }

    fn section_boundary_link(&self, index: usize) -> Option<String> {
        self.record
            .and_then(|record| record.integer(index))
            .and_then(|sequence| {
                self.parameter_resolver.resolve(
                    self.sequence,
                    index,
                    sequence,
                    ReferenceExpectation::SectionBoundaryEntity,
                    section_boundary_type,
                )
            })
            .map(|sequence| format!("iges:entity:directory#{sequence}"))
    }
}

fn general_note(
    subject: &Subject<'_>,
    transformation: Option<String>,
    overdeclared: &mut OverdeclaredCounts,
) -> NativeAnnotation {
    let record = subject.record;
    let count = subject.counted_tail(1, 12, overdeclared);
    NativeAnnotation::GeneralNote {
        id: subject.id(),
        source_entity: subject.source_entity(),
        form: subject.form,
        declared_string_count: record.and_then(|record| record.integer(1)),
        strings: (0..count)
            .map(|index| subject.text_run(2 + index * 12))
            .collect(),
        transformation,
    }
}

fn new_general_note(
    subject: &Subject<'_>,
    transformation: Option<String>,
    overdeclared: &mut OverdeclaredCounts,
) -> NativeAnnotation {
    let record = subject.record;
    let count = subject.counted_tail(12, 20, overdeclared);
    NativeAnnotation::NewGeneralNote {
        id: subject.id(),
        source_entity: subject.source_entity(),
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
        strings: (0..count)
            .map(|index| {
                let start = 13 + index * 20;
                NativeNewTextRun {
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
                        .map(<[u8]>::to_vec),
                    // A 213 text block is the 212 layout shifted by its
                    // eight-token prefix.
                    text: subject.text_run(start + 8),
                }
            })
            .collect(),
        transformation,
    }
}

fn leader(
    subject: &Subject<'_>,
    transformation: Option<String>,
    overdeclared: &mut OverdeclaredCounts,
) -> NativeAnnotation {
    let record = subject.record;
    let count = subject.counted_tail_at(1, 7, 2, overdeclared);
    let z = record.and_then(|record| record.number(4));
    NativeAnnotation::Leader {
        id: subject.id(),
        source_entity: subject.source_entity(),
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
        segment_tails: (0..count)
            .map(|index| {
                [
                    record.and_then(|record| record.number(7 + index * 2)),
                    record.and_then(|record| record.number(8 + index * 2)),
                    z,
                ]
            })
            .collect(),
        transformation,
    }
}

fn flag_note(
    subject: &Subject<'_>,
    transformation: Option<String>,
    overdeclared: &mut OverdeclaredCounts,
) -> NativeAnnotation {
    let record = subject.record;
    let leaders = subject.leader_list(6, 7, overdeclared);
    NativeAnnotation::FlagNote {
        id: subject.id(),
        source_entity: subject.source_entity(),
        origin: [
            record.and_then(|record| record.number(1)),
            record.and_then(|record| record.number(2)),
            record.and_then(|record| record.number(3)),
        ],
        rotation: record.and_then(|record| record.number(4)),
        note: subject.note_link(5),
        declared_leader_count: record.and_then(|record| record.integer(6)),
        leaders,
        transformation,
    }
}

fn general_label(
    subject: &Subject<'_>,
    transformation: Option<String>,
    overdeclared: &mut OverdeclaredCounts,
) -> NativeAnnotation {
    let record = subject.record;
    let leaders = subject.leader_list(2, 3, overdeclared);
    NativeAnnotation::GeneralLabel {
        id: subject.id(),
        source_entity: subject.source_entity(),
        note: subject.note_link(1),
        declared_leader_count: record.and_then(|record| record.integer(2)),
        leaders,
        transformation,
    }
}

fn general_symbol(subject: &Subject<'_>, transformation: Option<String>) -> NativeAnnotation {
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
    NativeAnnotation::GeneralSymbol {
        id: subject.id(),
        source_entity: subject.source_entity(),
        form: subject.form,
        note: subject.note_link(1),
        declared_geometry_count,
        geometry: (0..geometry_count)
            .map(|offset| subject.geometry_link(3 + offset))
            .collect(),
        declared_leader_count,
        leaders: (0..leader_count)
            .map(|offset| subject.leader_link(leader_count_index + 1 + offset))
            .collect(),
        transformation,
    }
}

fn sectioned_area(
    subject: &Subject<'_>,
    transformation: Option<String>,
    overdeclared: &mut OverdeclaredCounts,
) -> NativeAnnotation {
    let record = subject.record;
    let island_count = subject.counted_tail_at(8, 9, 1, overdeclared);
    NativeAnnotation::SectionedArea {
        id: subject.id(),
        source_entity: subject.source_entity(),
        form: subject.form,
        boundary: subject.section_boundary_link(1),
        fill_pattern: record.and_then(|record| record.integer(2)),
        pattern_anchor: [
            record.and_then(|record| record.number(3)),
            record.and_then(|record| record.number(4)),
            record.and_then(|record| record.number(5)),
        ],
        pattern_spacing: record.and_then(|record| record.number(6)),
        pattern_angle: record.and_then(|record| record.number(7)),
        declared_island_count: record.and_then(|record| record.integer(8)),
        islands: (0..island_count)
            .map(|offset| subject.section_boundary_link(9 + offset))
            .collect(),
        transformation,
    }
}

pub(super) fn build(
    directory: &[DirectoryEntry],
    by_directory: &BTreeMap<u32, &ParameterRecord>,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    parameter_resolver: &ParameterResolver<'_>,
    clamped_primary_end: &impl Fn(u32, &ParameterRecord) -> usize,
    overdeclared_counts: &mut OverdeclaredCounts,
    global_table: GlobalTable,
) -> Vec<NativeAnnotation> {
    directory
        .iter()
        .filter_map(|entry| classify(entry.entity_type, entry.form).map(|kind| (entry, kind)))
        .map(|(entry, kind)| {
            let record = by_directory.get(&entry.sequence).copied();
            let subject = Subject {
                sequence: entry.sequence,
                form: entry.form,
                record,
                primary_end: record.map_or(0, |record| clamped_primary_end(entry.sequence, record)),
                entries,
                parameter_resolver,
                v5_null_string_rule: global_table == GlobalTable::V5_0
                    && matches!(kind, AnnotationKind::GeneralNote),
            };
            let transformation = (entry.transform > 0)
                .then(|| format!("iges:native:transformation#D{}", entry.transform));
            match kind {
                AnnotationKind::GeneralNote => {
                    general_note(&subject, transformation, overdeclared_counts)
                }
                AnnotationKind::NewGeneralNote => {
                    new_general_note(&subject, transformation, overdeclared_counts)
                }
                AnnotationKind::Leader => leader(&subject, transformation, overdeclared_counts),
                AnnotationKind::FlagNote => {
                    flag_note(&subject, transformation, overdeclared_counts)
                }
                AnnotationKind::GeneralLabel => {
                    general_label(&subject, transformation, overdeclared_counts)
                }
                AnnotationKind::GeneralSymbol => general_symbol(&subject, transformation),
                AnnotationKind::SectionedArea => {
                    sectioned_area(&subject, transformation, overdeclared_counts)
                }
                AnnotationKind::AngularDimension => NativeAnnotation::AngularDimension {
                    id: subject.id(),
                    source_entity: subject.source_entity(),
                    note: subject.note_link(1),
                    witnesses: [subject.witness_link(2), subject.witness_link(3)],
                    vertex: [
                        record.and_then(|record| record.number(4)),
                        record.and_then(|record| record.number(5)),
                    ],
                    radius: record.and_then(|record| record.number(6)),
                    leaders: [subject.leader_link(7), subject.leader_link(8)],
                    transformation,
                },
                AnnotationKind::CurveDimension => NativeAnnotation::CurveDimension {
                    id: subject.id(),
                    source_entity: subject.source_entity(),
                    note: subject.note_link(1),
                    curves: [subject.curve_link(2), subject.curve_link(3)],
                    leaders: [subject.leader_link(4), subject.leader_link(5)],
                    witnesses: [subject.witness_link(6), subject.witness_link(7)],
                    transformation,
                },
                AnnotationKind::DiameterDimension => NativeAnnotation::DiameterDimension {
                    id: subject.id(),
                    source_entity: subject.source_entity(),
                    note: subject.note_link(1),
                    leaders: [subject.leader_link(2), subject.leader_link(3)],
                    center: [
                        record.and_then(|record| record.number(4)),
                        record.and_then(|record| record.number(5)),
                    ],
                    transformation,
                },
                AnnotationKind::LinearDimension => NativeAnnotation::LinearDimension {
                    id: subject.id(),
                    source_entity: subject.source_entity(),
                    form: subject.form,
                    note: subject.note_link(1),
                    leaders: [subject.leader_link(2), subject.leader_link(3)],
                    witnesses: [subject.witness_link(4), subject.witness_link(5)],
                    transformation,
                },
                AnnotationKind::OrdinateDimension => NativeAnnotation::OrdinateDimension {
                    id: subject.id(),
                    source_entity: subject.source_entity(),
                    form: subject.form,
                    note: subject.note_link(1),
                    ordinate: subject.ordinate_link(2),
                    supplemental_leader: (subject.form == 1)
                        .then(|| subject.leader_link(3))
                        .flatten(),
                    transformation,
                },
                AnnotationKind::PointDimension => NativeAnnotation::PointDimension {
                    id: subject.id(),
                    source_entity: subject.source_entity(),
                    note: subject.note_link(1),
                    leader: subject.leader_link(2),
                    enclosure: subject.enclosure_link(3),
                    transformation,
                },
                AnnotationKind::RadiusDimension => NativeAnnotation::RadiusDimension {
                    id: subject.id(),
                    source_entity: subject.source_entity(),
                    form: subject.form,
                    note: subject.note_link(1),
                    leaders: [
                        subject.leader_link(2),
                        (subject.form == 1)
                            .then(|| subject.leader_link(5))
                            .flatten(),
                    ],
                    center: [
                        record.and_then(|record| record.number(3)),
                        record.and_then(|record| record.number(4)),
                    ],
                    transformation,
                },
            }
        })
        .collect()
}
