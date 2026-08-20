// SPDX-License-Identifier: Apache-2.0

use super::OverdeclaredCount;
use crate::directory::DirectoryEntry;
use crate::entities::annotation::{parameterized_curve_type, section_boundary_type};
use crate::graph::ParameterResolver;
use crate::parameter::{DefaultTailCount, ParameterRecord};
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
        note: Option<String>,
        geometry: Vec<Option<String>>,
        leaders: Vec<Option<String>>,
        transformation: Option<String>,
    },
    SectionedArea {
        id: String,
        source_entity: String,
        boundary: Option<String>,
        fill_pattern: Option<i64>,
        pattern_anchor: [Option<f64>; 3],
        pattern_spacing: Option<f64>,
        pattern_angle: Option<f64>,
        islands: Vec<Option<String>>,
        transformation: Option<String>,
    },
}

fn counted_tail_items(
    sequence: u32,
    verdict: DefaultTailCount,
    overdeclared: &mut Vec<OverdeclaredCount>,
) -> usize {
    match verdict {
        DefaultTailCount::Held(count) => count,
        DefaultTailCount::Overdeclared { declared, present } => {
            overdeclared.push(OverdeclaredCount {
                sequence,
                declared,
                present,
            });
            0
        }
        DefaultTailCount::Unreadable => 0,
    }
}

fn text_run(
    parameter_resolver: &ParameterResolver<'_>,
    source: u32,
    record: Option<&ParameterRecord>,
    start: usize,
) -> NativeTextRun {
    let font_code = record.and_then(|record| record.integer(start + 3));
    NativeTextRun {
        declared_character_count: record.and_then(|record| record.integer(start)),
        text: record
            .and_then(|record| record.string(start + 11))
            .map(<[u8]>::to_vec),
        box_size: [
            record.and_then(|record| record.number(start + 1)),
            record.and_then(|record| record.number(start + 2)),
        ],
        font_code,
        font_definition: font_code
            .filter(|value| *value < 0)
            .and_then(|value| {
                parameter_resolver.resolve_negative(
                    source,
                    start + 3,
                    value,
                    "type-310-form-0",
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

pub(super) fn build(
    directory: &[DirectoryEntry],
    by_directory: &BTreeMap<u32, &ParameterRecord>,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    parameter_resolver: &ParameterResolver<'_>,
    primary_end: &impl Fn(u32, &ParameterRecord) -> usize,
) -> (Vec<NativeAnnotation>, Vec<OverdeclaredCount>) {
    let mut overdeclared_counts = Vec::new();
    let annotations = directory
        .iter()
        .filter(|entry| {
            (matches!(entry.entity_type, 202 | 204 | 206 | 208 | 210 | 212 | 213)
                && entry.form == 0)
                || (entry.entity_type == 214 && matches!(entry.form, 1..=12))
                || matches!(
                    (entry.entity_type, entry.form),
                    (216, 0..=2) | (218 | 222, 0..=1) | (220, 0)
                )
                || matches!((entry.entity_type, entry.form), (228 | 230, 0))
        })
        .filter_map(|entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let transformation = (entry.transform > 0)
                .then(|| format!("iges:native:transformation#D{}", entry.transform));
            Some(if entry.entity_type == 212 {
                let verdict = record.map_or(DefaultTailCount::Unreadable, |record| {
                    record.count_with_stride_before_default_tail(
                        1,
                        12,
                        primary_end(entry.sequence, record),
                    )
                });
                let count = counted_tail_items(entry.sequence, verdict, &mut overdeclared_counts);
                NativeAnnotation::GeneralNote {
                    id: format!("iges:presentation:annotation#D{}", entry.sequence),
                    source_entity: format!("iges:entity:directory#{}", entry.sequence),
                    declared_string_count: record.and_then(|record| record.integer(1)),
                    strings: (0..count)
                        .map(|index| {
                            text_run(parameter_resolver, entry.sequence, record, 2 + index * 12)
                        })
                        .collect(),
                    transformation,
                }
            } else if entry.entity_type == 213 {
                let verdict = record.map_or(DefaultTailCount::Unreadable, |record| {
                    record.count_with_stride_before_default_tail(
                        12,
                        20,
                        primary_end(entry.sequence, record),
                    )
                });
                let count = counted_tail_items(entry.sequence, verdict, &mut overdeclared_counts);
                NativeAnnotation::NewGeneralNote {
                    id: format!("iges:presentation:annotation#D{}", entry.sequence),
                    source_entity: format!("iges:entity:directory#{}", entry.sequence),
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
                            let font_code = record.and_then(|record| record.integer(start + 11));
                            NativeNewTextRun {
                                fixed_or_variable: record.and_then(|record| record.integer(start)),
                                character_size: [
                                    record.and_then(|record| record.number(start + 1)),
                                    record.and_then(|record| record.number(start + 2)),
                                ],
                                character_spacing: record
                                    .and_then(|record| record.number(start + 3)),
                                line_spacing: record.and_then(|record| record.number(start + 4)),
                                font_style: record.and_then(|record| record.integer(start + 5)),
                                character_angle: record.and_then(|record| record.number(start + 6)),
                                control_codes: record
                                    .and_then(|record| record.string(start + 7))
                                    .map(<[u8]>::to_vec),
                                text: NativeTextRun {
                                    declared_character_count: record
                                        .and_then(|record| record.integer(start + 8)),
                                    text: record
                                        .and_then(|record| record.string(start + 19))
                                        .map(<[u8]>::to_vec),
                                    box_size: [
                                        record.and_then(|record| record.number(start + 9)),
                                        record.and_then(|record| record.number(start + 10)),
                                    ],
                                    font_code,
                                    font_definition: font_code
                                        .filter(|value| *value < 0)
                                        .and_then(|value| {
                                            parameter_resolver.resolve_negative(
                                                entry.sequence,
                                                start + 11,
                                                value,
                                                "type-310-form-0",
                                                |target| {
                                                    target.entity_type == 310 && target.form == 0
                                                },
                                            )
                                        })
                                        .map(|sequence| {
                                            format!("iges:presentation:text-font#D{sequence}")
                                        }),
                                    slant_angle: record
                                        .and_then(|record| record.number(start + 12)),
                                    rotation_angle: record
                                        .and_then(|record| record.number(start + 13)),
                                    mirror: record.and_then(|record| record.integer(start + 14)),
                                    vertical: record.and_then(|record| record.integer(start + 15)),
                                    start: [
                                        record.and_then(|record| record.number(start + 16)),
                                        record.and_then(|record| record.number(start + 17)),
                                        record.and_then(|record| record.number(start + 18)),
                                    ],
                                },
                            }
                        })
                        .collect(),
                    transformation,
                }
            } else if entry.entity_type == 214 {
                let count = record
                    .and_then(|record| {
                        record.count_with_stride_before(1, 2, primary_end(entry.sequence, record))
                    })
                    .unwrap_or_default();
                let z = record.and_then(|record| record.number(4));
                NativeAnnotation::Leader {
                    id: format!("iges:presentation:annotation#D{}", entry.sequence),
                    source_entity: format!("iges:entity:directory#{}", entry.sequence),
                    form: entry.form,
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
            } else {
                let note_link = |index| {
                    record
                        .and_then(|record| record.integer(index))
                        .filter(|sequence| *sequence != 0)
                        .and_then(|sequence| {
                            parameter_resolver.resolve_type(
                                entry.sequence,
                                index,
                                sequence,
                                212,
                                &[0],
                            )
                        })
                        .map(|sequence| format!("iges:presentation:annotation#D{sequence}"))
                };
                let leader_link = |index| {
                    record
                        .and_then(|record| record.integer(index))
                        .filter(|sequence| *sequence != 0)
                        .and_then(|sequence| {
                            parameter_resolver.resolve(
                                entry.sequence,
                                index,
                                sequence,
                                "type-214-form-1-through-12",
                                |target| target.entity_type == 214 && matches!(target.form, 1..=12),
                            )
                        })
                        .map(|sequence| format!("iges:presentation:annotation#D{sequence}"))
                };
                let witness_link = |index| {
                    record
                        .and_then(|record| record.integer(index))
                        .filter(|sequence| *sequence != 0)
                        .and_then(|sequence| {
                            parameter_resolver.resolve_type(
                                entry.sequence,
                                index,
                                sequence,
                                106,
                                &[40],
                            )
                        })
                        .map(|sequence| format!("iges:entity:directory#{sequence}"))
                };
                let curve_link = |index| {
                    record
                        .and_then(|record| record.integer(index))
                        .filter(|sequence| *sequence != 0)
                        .and_then(|sequence| {
                            parameter_resolver.resolve(
                                entry.sequence,
                                index,
                                sequence,
                                "parameterized-curve",
                                |target| {
                                    parameterized_curve_type(target)
                                        && target.status.is_physically_dependent()
                                        && target.status.use_flag == 1
                                },
                            )
                        })
                        .map(|sequence| format!("iges:entity:directory#{sequence}"))
                };
                let ordinate_link = |index| {
                    record
                        .and_then(|record| record.integer(index))
                        .filter(|sequence| *sequence != 0)
                        .and_then(|sequence| {
                            parameter_resolver.resolve(
                                entry.sequence,
                                index,
                                sequence,
                                "type-106-form-40-or-leader",
                                |target| {
                                    (target.entity_type == 106 && target.form == 40)
                                        || (target.entity_type == 214
                                            && matches!(target.form, 1..=12))
                                },
                            )
                        })
                        .map(|sequence| {
                            entries
                                .get(&sequence)
                                .filter(|target| target.entity_type == 214)
                                .map_or_else(
                                    || format!("iges:entity:directory#{sequence}"),
                                    |_| format!("iges:presentation:annotation#D{sequence}"),
                                )
                        })
                };
                let id = format!("iges:presentation:annotation#D{}", entry.sequence);
                let source_entity = format!("iges:entity:directory#{}", entry.sequence);
                match entry.entity_type {
                    202 => NativeAnnotation::AngularDimension {
                        id,
                        source_entity,
                        note: note_link(1),
                        witnesses: [witness_link(2), witness_link(3)],
                        vertex: [
                            record.and_then(|record| record.number(4)),
                            record.and_then(|record| record.number(5)),
                        ],
                        radius: record.and_then(|record| record.number(6)),
                        leaders: [leader_link(7), leader_link(8)],
                        transformation,
                    },
                    204 => NativeAnnotation::CurveDimension {
                        id,
                        source_entity,
                        note: note_link(1),
                        curves: [curve_link(2), curve_link(3)],
                        leaders: [leader_link(4), leader_link(5)],
                        witnesses: [witness_link(6), witness_link(7)],
                        transformation,
                    },
                    206 => NativeAnnotation::DiameterDimension {
                        id,
                        source_entity,
                        note: note_link(1),
                        leaders: [leader_link(2), leader_link(3)],
                        center: [
                            record.and_then(|record| record.number(4)),
                            record.and_then(|record| record.number(5)),
                        ],
                        transformation,
                    },
                    208 | 210 => {
                        let (note_index, count_index, leader_start) = if entry.entity_type == 208 {
                            (5, 6, 7)
                        } else {
                            (1, 2, 3)
                        };
                        let leader_count = record
                            .and_then(|record| {
                                record.count_with_stride_before(
                                    count_index,
                                    1,
                                    primary_end(entry.sequence, record),
                                )
                            })
                            .unwrap_or_default();
                        let leaders = (0..leader_count)
                            .map(|offset| leader_link(leader_start + offset))
                            .collect();
                        if entry.entity_type == 208 {
                            NativeAnnotation::FlagNote {
                                id,
                                source_entity,
                                origin: [
                                    record.and_then(|record| record.number(1)),
                                    record.and_then(|record| record.number(2)),
                                    record.and_then(|record| record.number(3)),
                                ],
                                rotation: record.and_then(|record| record.number(4)),
                                note: note_link(note_index),
                                declared_leader_count: record
                                    .and_then(|record| record.integer(count_index)),
                                leaders,
                                transformation,
                            }
                        } else {
                            NativeAnnotation::GeneralLabel {
                                id,
                                source_entity,
                                note: note_link(note_index),
                                declared_leader_count: record
                                    .and_then(|record| record.integer(count_index)),
                                leaders,
                                transformation,
                            }
                        }
                    }
                    216 => NativeAnnotation::LinearDimension {
                        id,
                        source_entity,
                        form: entry.form,
                        note: note_link(1),
                        leaders: [leader_link(2), leader_link(3)],
                        witnesses: [witness_link(4), witness_link(5)],
                        transformation,
                    },
                    218 => NativeAnnotation::OrdinateDimension {
                        id,
                        source_entity,
                        form: entry.form,
                        note: note_link(1),
                        ordinate: ordinate_link(2),
                        supplemental_leader: (entry.form == 1).then(|| leader_link(3)).flatten(),
                        transformation,
                    },
                    220 => NativeAnnotation::PointDimension {
                        id,
                        source_entity,
                        note: note_link(1),
                        leader: leader_link(2),
                        enclosure: record
                            .and_then(|record| record.integer(3))
                            .filter(|sequence| *sequence != 0)
                            .and_then(|sequence| {
                                parameter_resolver.resolve(
                                    entry.sequence,
                                    3,
                                    sequence,
                                    "point-dimension-enclosure",
                                    |target| {
                                        matches!(
                                            (target.entity_type, target.form),
                                            (100 | 102, 0) | (106, 63)
                                        ) && target.status.is_physically_dependent()
                                            && target.status.use_flag == 1
                                    },
                                )
                            })
                            .map(|sequence| format!("iges:entity:directory#{sequence}")),
                        transformation,
                    },
                    222 => NativeAnnotation::RadiusDimension {
                        id,
                        source_entity,
                        form: entry.form,
                        note: note_link(1),
                        leaders: [
                            leader_link(2),
                            (entry.form == 1).then(|| leader_link(5)).flatten(),
                        ],
                        center: [
                            record.and_then(|record| record.number(3)),
                            record.and_then(|record| record.number(4)),
                        ],
                        transformation,
                    },
                    228 => {
                        let end = record.map_or(0, |record| primary_end(entry.sequence, record));
                        let counts = record
                            .and_then(|record| record.count_with_stride_before(2, 1, end))
                            .and_then(|geometry_count| {
                                let leader_count_index = 3_usize.checked_add(geometry_count)?;
                                let leader_count = record.and_then(|record| {
                                    record.count_with_stride_before(leader_count_index, 1, end)
                                })?;
                                let finish = leader_count_index
                                    .checked_add(1)?
                                    .checked_add(leader_count)?;
                                (finish <= end).then_some((geometry_count, leader_count))
                            })
                            .unwrap_or_default();
                        let (geometry_count, leader_count) = counts;
                        let leader_count_index = 3 + geometry_count;
                        NativeAnnotation::GeneralSymbol {
                            id,
                            source_entity,
                            note: note_link(1),
                            geometry: (0..geometry_count)
                                .map(|offset| {
                                    let index = 3 + offset;
                                    record
                                        .and_then(|record| record.integer(index))
                                        .and_then(|sequence| {
                                            parameter_resolver.resolve(
                                                entry.sequence,
                                                index,
                                                sequence,
                                                "subordinate-annotation-geometry",
                                                |target| {
                                                    target.status.is_physically_dependent()
                                                        && target.status.use_flag == 1
                                                },
                                            )
                                        })
                                        .map(|sequence| format!("iges:entity:directory#{sequence}"))
                                })
                                .collect(),
                            leaders: (0..leader_count)
                                .map(|offset| leader_link(leader_count_index + 1 + offset))
                                .collect(),
                            transformation,
                        }
                    }
                    230 => {
                        let island_count = record
                            .and_then(|record| {
                                record.count_with_stride_before(
                                    8,
                                    1,
                                    primary_end(entry.sequence, record),
                                )
                            })
                            .unwrap_or_default();
                        NativeAnnotation::SectionedArea {
                            id,
                            source_entity,
                            boundary: record
                                .and_then(|record| record.integer(1))
                                .and_then(|sequence| {
                                    parameter_resolver.resolve(
                                        entry.sequence,
                                        1,
                                        sequence,
                                        "section-boundary-entity",
                                        section_boundary_type,
                                    )
                                })
                                .map(|sequence| format!("iges:entity:directory#{sequence}")),
                            fill_pattern: record.and_then(|record| record.integer(2)),
                            pattern_anchor: [
                                record.and_then(|record| record.number(3)),
                                record.and_then(|record| record.number(4)),
                                record.and_then(|record| record.number(5)),
                            ],
                            pattern_spacing: record.and_then(|record| record.number(6)),
                            pattern_angle: record.and_then(|record| record.number(7)),
                            islands: (0..island_count)
                                .map(|offset| {
                                    let index = 9 + offset;
                                    record
                                        .and_then(|record| record.integer(index))
                                        .and_then(|sequence| {
                                            parameter_resolver.resolve(
                                                entry.sequence,
                                                index,
                                                sequence,
                                                "section-boundary-entity",
                                                section_boundary_type,
                                            )
                                        })
                                        .map(|sequence| format!("iges:entity:directory#{sequence}"))
                                })
                                .collect(),
                            transformation,
                        }
                    }
                    _ => return None,
                }
            })
        })
        .collect::<Vec<_>>();
    (annotations, overdeclared_counts)
}
