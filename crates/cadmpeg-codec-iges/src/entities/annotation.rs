// SPDX-License-Identifier: Apache-2.0
//! Text annotation entities.

use super::geometry::{entity_loss, resolve_transform, ProjectionOutcome};
use super::presentation::{
    general_note_font_valid_for_dialect, new_general_note_charset_valid,
    new_general_note_font_valid,
};
use crate::directory::DirectoryEntry;
use crate::global::{Dialect, ProjectedGlobal};
use crate::parameter::{DefaultTailCount, ParameterRecord};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::geometry::CurveGeometry;
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};

/// One admitted annotation shape per variant.
///
/// [`classify`] is the single owner of annotation admission: the native
/// retention pass and the semantic projection both dispatch on its result,
/// so a new form is admitted by adding one `classify` arm and handling the
/// variant everywhere the compiler then requires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AnnotationKind {
    AngularDimension,
    CurveDimension,
    DiameterDimension,
    FlagNote,
    GeneralLabel,
    GeneralNote,
    NewGeneralNote,
    Leader,
    LinearDimension,
    OrdinateDimension,
    PointDimension,
    RadiusDimension,
    GeneralSymbol,
    SectionedArea,
}

const SECTION_COPLANAR_NORMAL_EPSILON: f64 = 1.0e-10;

fn finite_point(point: Point3) -> bool {
    point.x.is_finite() && point.y.is_finite() && point.z.is_finite()
}

fn finite_vector(vector: Vector3) -> bool {
    vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite()
}

fn normalized(vector: Vector3) -> Option<Vector3> {
    let norm = vector.norm();
    (finite_vector(vector) && norm.is_finite() && norm > 0.0).then(|| vector.scale(1.0 / norm))
}

fn point_on_plane(point: Point3, plane: (Point3, Vector3), resolution: f64) -> bool {
    let distance = point.vector_from(plane.0).dot(plane.1).abs();
    distance.is_finite() && distance <= resolution
}

fn normal_matches_plane(normal: Vector3, plane_normal: Vector3) -> bool {
    normalized(normal)
        .is_some_and(|normal| normal.cross(plane_normal).norm() <= SECTION_COPLANAR_NORMAL_EPSILON)
}

fn direction_in_plane(direction: Vector3, plane_normal: Vector3) -> bool {
    normalized(direction)
        .zip(normalized(plane_normal))
        .is_some_and(|(direction, plane_normal)| {
            direction.dot(plane_normal).abs() <= SECTION_COPLANAR_NORMAL_EPSILON
        })
}

fn curve_geometry_coplanar(
    geometry: &CurveGeometry,
    index: &ModelIndex<'_>,
    transform: Transform,
    plane: (Point3, Vector3),
    resolution: f64,
    active: &mut BTreeSet<CurveId>,
) -> bool {
    let point_valid =
        |point: Point3| point_on_plane(transform.apply_point(point), plane, resolution);
    let normal_valid = |normal: Vector3| {
        transform
            .apply_normal(normal)
            .is_some_and(|normal| normal_matches_plane(normal, plane.1))
    };
    let direction_valid =
        |direction: Vector3| direction_in_plane(transform.apply_vector(direction), plane.1);
    match geometry {
        CurveGeometry::Line { origin, direction } => {
            point_valid(*origin) && direction_valid(*direction)
        }
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            ..
        } => point_valid(*center) && normal_valid(*axis) && direction_valid(*ref_direction),
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            ..
        } => point_valid(*center) && normal_valid(*axis) && direction_valid(*major_direction),
        CurveGeometry::Parabola {
            vertex,
            axis,
            major_direction,
            ..
        } => point_valid(*vertex) && normal_valid(*axis) && direction_valid(*major_direction),
        CurveGeometry::Hyperbola {
            center,
            axis,
            major_direction,
            ..
        } => point_valid(*center) && normal_valid(*axis) && direction_valid(*major_direction),
        CurveGeometry::Degenerate { point } => point_valid(*point),
        CurveGeometry::Nurbs(curve) => curve.control_points.iter().copied().all(point_valid),
        CurveGeometry::Polyline { points, .. } => points.iter().copied().all(point_valid),
        CurveGeometry::Composite { segments, .. } => segments.iter().all(|segment| {
            let Some(curve) = index.curves(&segment.curve.0) else {
                return false;
            };
            if !active.insert(segment.curve.clone()) {
                return false;
            }
            let valid = curve_geometry_coplanar(
                &curve.geometry,
                index,
                transform,
                plane,
                resolution,
                active,
            );
            active.remove(&segment.curve);
            valid
        }),
        CurveGeometry::Transformed {
            basis,
            transform: map,
        } => curve_geometry_coplanar(
            basis,
            index,
            transform.compose(*map),
            plane,
            resolution,
            active,
        ),
        CurveGeometry::Procedural { .. } | CurveGeometry::Unknown { .. } => false,
    }
}

fn sectioned_area_pattern_plane(
    record: &ParameterRecord,
    transform: Transform,
    length_factor: f64,
) -> Option<(Point3, Vector3)> {
    let z = record.number_or(5, 0.0)? * length_factor;
    if !z.is_finite() || !length_factor.is_finite() {
        return None;
    }
    let point = transform.apply_point(Point3::new(0.0, 0.0, z));
    let normal = transform
        .apply_normal(Vector3::new(0.0, 0.0, 1.0))
        .and_then(normalized)?;
    finite_point(point).then_some((point, normal))
}

fn sectioned_area_curves_coplanar(
    ir: &CadIr,
    sequences: &[u32],
    pattern_plane: (Point3, Vector3),
    resolution: f64,
) -> bool {
    if !resolution.is_finite() || resolution < 0.0 {
        return false;
    }
    let index = ModelIndex::new(ir);
    let identity = Transform::identity();
    let mut active = BTreeSet::new();
    sequences.iter().all(|sequence| {
        let curve_id = CurveId(format!("iges:model:curve#D{sequence}"));
        let Some(curve) = index.curves(&curve_id.0) else {
            return false;
        };
        if !active.insert(curve_id.clone()) {
            return false;
        }
        let valid = curve_geometry_coplanar(
            &curve.geometry,
            &index,
            identity,
            pattern_plane,
            resolution,
            &mut active,
        );
        active.remove(&curve_id);
        valid
    })
}

/// Maps a directory entry's type and form to its annotation kind, or `None`
/// for every shape the annotation concern does not admit.
pub(crate) fn classify(entity_type: i64, form: i64) -> Option<AnnotationKind> {
    match (entity_type, form) {
        (202, 0) => Some(AnnotationKind::AngularDimension),
        (204, 0) => Some(AnnotationKind::CurveDimension),
        (206, 0) => Some(AnnotationKind::DiameterDimension),
        (208, 0) => Some(AnnotationKind::FlagNote),
        (210, 0) => Some(AnnotationKind::GeneralLabel),
        (212, form) if crate::profile::general_note_form_admitted(form) => {
            Some(AnnotationKind::GeneralNote)
        }
        (213, 0) => Some(AnnotationKind::NewGeneralNote),
        (214, 1..=12) => Some(AnnotationKind::Leader),
        (216, 0..=2) => Some(AnnotationKind::LinearDimension),
        (218, 0..=1) => Some(AnnotationKind::OrdinateDimension),
        (220, 0) => Some(AnnotationKind::PointDimension),
        (222, 0..=1) => Some(AnnotationKind::RadiusDimension),
        (228, 0) => Some(AnnotationKind::GeneralSymbol),
        (230, 0) => Some(AnnotationKind::SectionedArea),
        _ => None,
    }
}

fn finite(record: &ParameterRecord, index: usize) -> bool {
    record.number(index).is_some_and(f64::is_finite)
}

fn exact_parameter_count(record: &ParameterRecord, expected: usize) -> bool {
    record.parameter_end() == expected
}

fn justification_valid(value: i64) -> bool {
    matches!(value, 0..=3)
}

fn fixed_or_variable_valid(value: i64) -> bool {
    matches!(value, 0..=1)
}

fn mirror_flag_valid(value: i64) -> bool {
    matches!(value, 0..=2)
}

fn vertical_text_flag_valid(value: i64) -> bool {
    matches!(value, 0..=1)
}

fn general_note_valid_for_dialect(
    record: &ParameterRecord,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    dialect: Dialect,
    form: i64,
) -> bool {
    let parameter_end = crate::parameter::entity_primary_end(record, entries)
        .unwrap_or_else(|| record.parameter_end());
    if !general_note_suffix_structurally_valid(record, parameter_end) {
        return false;
    }
    let count = match record.count_with_stride_before_default_tail(1, 12, parameter_end) {
        DefaultTailCount::Held(count)
            if crate::profile::general_note_form_admitted(form)
                && general_note_string_count_valid(form, count) =>
        {
            count
        }
        _ => return false,
    };
    parameter_end <= 2 + count * 12
        && (0..count).all(|index| {
            let start = 2 + index * 12;
            let text = record.string_or_empty(start + 11);
            record
                .integer(start)
                .and_then(|value| usize::try_from(value).ok())
                .zip(text)
                .is_some_and(|(declared, text)| declared == text.len())
                && (start + 1..=start + 2).all(|field| {
                    record
                        .number_or(field, 0.0)
                        .is_some_and(|value| value.is_finite() && value >= 0.0)
                })
                && record.integer_or(start + 3, 1).is_some_and(|value| {
                    general_note_font_valid_for_dialect(value, entries, dialect)
                })
                && record
                    .number_or(start + 4, std::f64::consts::FRAC_PI_2)
                    .is_some_and(f64::is_finite)
                && record.number_or(start + 5, 0.0).is_some_and(f64::is_finite)
                && record
                    .integer_or(start + 6, 0)
                    .is_some_and(mirror_flag_valid)
                && record
                    .integer_or(start + 7, 0)
                    .is_some_and(vertical_text_flag_valid)
                && (start + 8..=start + 10)
                    .all(|field| record.number_or(field, 0.0).is_some_and(f64::is_finite))
        })
}

fn general_note_suffix_structurally_valid(record: &ParameterRecord, primary_end: usize) -> bool {
    // A malformed pointer target must not invalidate the note's own primary
    // fields, but arbitrary tokens after that primary span are not a suffix.
    // Check the two counted group shapes without requiring their targets to
    // resolve; reference validation owns that separate decision.
    if record.tokens.len() == primary_end || record.parameter_end() == primary_end {
        return true;
    }
    let Some(first_count) = record
        .tokens
        .get(primary_end)
        .and_then(|token| match &token.value {
            crate::parameter::TokenValue::Integer(value) => usize::try_from(*value).ok(),
            crate::parameter::TokenValue::Omitted
            | crate::parameter::TokenValue::Real(_)
            | crate::parameter::TokenValue::String(_) => None,
        })
    else {
        return false;
    };
    let Some(first_end) = primary_end
        .checked_add(1)
        .and_then(|end| end.checked_add(first_count))
    else {
        return false;
    };
    if first_end > record.tokens.len() {
        return false;
    }
    if first_end == record.tokens.len() {
        return true;
    }
    let Some(second_count) = record
        .tokens
        .get(first_end)
        .and_then(|token| match &token.value {
            crate::parameter::TokenValue::Integer(value) => usize::try_from(*value).ok(),
            crate::parameter::TokenValue::Omitted
            | crate::parameter::TokenValue::Real(_)
            | crate::parameter::TokenValue::String(_) => None,
        })
    else {
        return false;
    };
    first_end
        .checked_add(1)
        .and_then(|end| end.checked_add(second_count))
        .is_some_and(|second_end| second_end == record.tokens.len())
}

fn general_note_string_count_valid(form: i64, count: usize) -> bool {
    let minimum = match form {
        0 | 6..=8 => 1,
        1..=4 => 2,
        5 => 3,
        100 => 4,
        101 => 8,
        102 => 9,
        105 => 12,
        _ => return false,
    };
    count >= minimum
}

fn new_general_note_valid(
    record: &ParameterRecord,
    entries: &BTreeMap<u32, &DirectoryEntry>,
) -> bool {
    let parameter_end = record.parameter_end();
    let count = match record.count_with_stride_before_default_tail(12, 20, parameter_end) {
        DefaultTailCount::Held(count) if count > 0 => count,
        _ => return false,
    };
    parameter_end <= 13 + count * 20
        && (1..=2).all(|index| {
            record
                .number_or(index, 0.0)
                .is_some_and(|value| value.is_finite() && value >= 0.0)
        })
        && record.integer_or(3, 0).is_some_and(justification_valid)
        && (4..=11).all(|index| record.number_or(index, 0.0).is_some_and(f64::is_finite))
        && (0..count).all(|index| {
            let start = 13 + index * 20;
            let fixed = record.integer_or(start, 0);
            let character_width = record.number_or(start + 1, 0.0);
            let character_height = record.number_or(start + 2, 0.0);
            // PS-01: variable-width CSPACE has an explicit default of one;
            // fixed-width CSPACE uses the generic real default of zero.
            let spacing_default = if fixed == Some(1) { 1.0 } else { 0.0 };
            let spacing = record.number_or(start + 3, spacing_default);
            let text = record.string_or_empty(start + 19);
            // PS-01: Type 213 FONT has no explicit default; the generic
            // integer default is zero.
            let font_style = record.integer_or(start + 5, 0);
            // PS-04: CHRSET has an entity-specific default of standard ASCII.
            let character_set = record.integer_or(start + 11, 1);
            let metrics_valid = character_width
                .zip(character_height)
                .zip(spacing)
                .is_some_and(|((width, height), spacing)| {
                    width.is_finite()
                        && width > 0.0
                        && height.is_finite()
                        && height > 0.0
                        && spacing.is_finite()
                        && match fixed {
                            Some(0) => spacing >= -width,
                            Some(1) => spacing >= 0.0,
                            _ => false,
                        }
                })
                && fixed.is_some_and(fixed_or_variable_valid);
            metrics_valid
                && record.number_or(start + 4, 0.0).is_some_and(f64::is_finite)
                && record.number_or(start + 6, 0.0).is_some_and(|value| {
                    value.is_finite() && (0.0..=std::f64::consts::TAU).contains(&value)
                })
                && record.string_or_empty(start + 7).is_some()
                && record
                    .integer(start + 8)
                    .and_then(|value| usize::try_from(value).ok())
                    .zip(text)
                    .is_some_and(|(declared, text)| declared == text.len())
                && (start + 9..=start + 10).all(|field| {
                    record
                        .number_or(field, 0.0)
                        .is_some_and(|value| value.is_finite() && value >= 0.0)
                })
                && font_style.is_some_and(new_general_note_font_valid)
                && character_set.is_some_and(|value| new_general_note_charset_valid(value, entries))
                && record
                    .number_or(start + 12, std::f64::consts::FRAC_PI_2)
                    .is_some_and(f64::is_finite)
                && record
                    .number_or(start + 13, 0.0)
                    .is_some_and(f64::is_finite)
                && record
                    .integer_or(start + 14, 0)
                    .is_some_and(mirror_flag_valid)
                && record
                    .integer_or(start + 15, 0)
                    .is_some_and(vertical_text_flag_valid)
                && (start + 16..=start + 18)
                    .all(|field| record.number_or(field, 0.0).is_some_and(f64::is_finite))
        })
}

fn leader_valid_for_dialect(
    entry: &DirectoryEntry,
    record: &ParameterRecord,
    dialect: Dialect,
) -> bool {
    let Some(count) = record
        .count_with_stride_at(1, 7, 2, record.parameter_end())
        .filter(|count| *count > 0)
    else {
        return false;
    };
    let dimensions_valid = record
        .number(2)
        .zip(record.number(3))
        .is_some_and(|(height, width)| {
            height.is_finite()
                && width.is_finite()
                && match dialect {
                    Dialect::V4_0 => matches!(entry.form, 1..=12),
                    _ => match entry.form {
                        4 => height == 0.0 && width == 0.0,
                        5 | 6 | 12 => height > 0.0 && height == width,
                        1..=3 | 7..=11 => height > 0.0 && width > 0.0,
                        _ => false,
                    },
                }
        });
    exact_parameter_count(record, 7 + count * 2)
        && dimensions_valid
        && (4..=6 + count * 2).all(|index| finite(record, index))
}

fn pointer(
    record: &ParameterRecord,
    index: usize,
    entries: &BTreeMap<u32, &DirectoryEntry>,
) -> Option<u32> {
    record
        .integer(index)
        .and_then(|value| u32::try_from(value).ok())
        .filter(|sequence| sequence % 2 == 1)
        .filter(|sequence| entries.contains_key(sequence))
}

fn child_valid(
    sequence: u32,
    entity_type: i64,
    forms: impl Fn(i64) -> bool,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    dialect: Dialect,
) -> bool {
    entries.get(&sequence).is_some_and(|entry| {
        entry.entity_type == entity_type
            && forms(entry.form)
            && entry.status.is_physically_dependent()
            && entry.status.use_flag == 1
            && records
                .get(&sequence)
                .is_some_and(|record| match entity_type {
                    212 => general_note_valid_for_dialect(record, entries, dialect, entry.form),
                    214 => leader_valid_for_dialect(entry, record, dialect),
                    106 => witness_valid(record),
                    _ => false,
                })
    })
}

fn general_note_child_valid(
    sequence: u32,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    dialect: Dialect,
) -> bool {
    entries.get(&sequence).is_some_and(|entry| {
        entry.entity_type == 212
            && crate::profile::general_note_form_admitted(entry.form)
            && entry.status.is_physically_dependent()
            && entry.status.use_flag == 1
            && records.get(&sequence).is_some_and(|record| {
                general_note_valid_for_dialect(record, entries, dialect, entry.form)
            })
    })
}

fn general_symbol_note_valid(
    record: &ParameterRecord,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    dialect: Dialect,
) -> bool {
    match record.integer(1) {
        Some(0) => !matches!(dialect, Dialect::V4_0),
        Some(_) => pointer(record, 1, entries)
            .is_some_and(|sequence| general_note_child_valid(sequence, entries, records, dialect)),
        None => false,
    }
}

fn dimension_enclosure_type_allowed(entity_type: i64, form: i64, dialect: Dialect) -> bool {
    matches!((entity_type, form), (100 | 102, 0))
        || (!matches!(dialect, Dialect::V4_0) && (entity_type, form) == (106, 63))
}

fn dimension_children_valid(
    parent: &DirectoryEntry,
    children: &[u32],
    entries: &BTreeMap<u32, &DirectoryEntry>,
) -> bool {
    let Some(first_transform) = children
        .first()
        .and_then(|sequence| entries.get(sequence))
        .map(|entry| entry.transform)
    else {
        return false;
    };
    (parent.transform == 0 || first_transform == 0)
        && children.iter().all(|sequence| {
            entries
                .get(sequence)
                .is_some_and(|entry| entry.transform == first_transform)
        })
}

fn witness_valid(record: &ParameterRecord) -> bool {
    let Some(count) = record
        .count_with_stride_at(2, 4, 2, record.parameter_end())
        .filter(|count| *count >= 3 && *count % 2 == 1)
    else {
        return false;
    };
    record.integer(1) == Some(1)
        && exact_parameter_count(record, 4 + count * 2)
        && (3..4 + count * 2).all(|index| finite(record, index))
}

pub(crate) fn parameterized_curve_type(entry: &DirectoryEntry) -> bool {
    matches!(
        entry.entity_type,
        100 | 102 | 104 | 106 | 110 | 112 | 126 | 130 | 142
    )
}

fn dimension_valid(
    entry: &DirectoryEntry,
    record: &ParameterRecord,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    dialect: Dialect,
) -> bool {
    let note = pointer(record, 1, entries);
    let note_valid =
        note.is_some_and(|sequence| general_note_child_valid(sequence, entries, records, dialect));
    let mut children = note.into_iter().collect::<Vec<_>>();
    let fields_valid = match (entry.entity_type, entry.form) {
        (202, 0) => {
            let witnesses = [record.integer(2), record.integer(3)];
            let leaders = [pointer(record, 7, entries), pointer(record, 8, entries)];
            let witnesses_valid = witnesses.iter().enumerate().all(|(offset, raw)| match raw {
                Some(0) => true,
                Some(_) => pointer(record, 2 + offset, entries).is_some_and(|sequence| {
                    child_valid(sequence, 106, |form| form == 40, entries, records, dialect)
                }),
                None => false,
            });
            let leaders_valid = leaders.iter().all(|leader| {
                leader.is_some_and(|sequence| {
                    child_valid(
                        sequence,
                        214,
                        |form| matches!(form, 1..=12),
                        entries,
                        records,
                        dialect,
                    )
                })
            });
            children.extend((2..=3).filter_map(|index| pointer(record, index, entries)));
            children.extend(leaders.into_iter().flatten());
            exact_parameter_count(record, 9)
                && witnesses_valid
                && (4..=5).all(|index| finite(record, index))
                && record
                    .number(6)
                    .is_some_and(|value| value.is_finite() && value > 0.0)
                && leaders_valid
        }
        (204, 0) => {
            let curves = [pointer(record, 2, entries), pointer(record, 3, entries)];
            let curve_entries =
                curves.map(|curve| curve.and_then(|sequence| entries.get(&sequence).copied()));
            let curves_valid = curve_entries[0].is_some_and(|curve| {
                parameterized_curve_type(curve)
                    && curve.status.is_physically_dependent()
                    && curve.status.use_flag == 1
            }) && match record.integer(3) {
                Some(0) => true,
                Some(_) => curve_entries[1].is_some_and(|curve| {
                    parameterized_curve_type(curve)
                        && curve.status.is_physically_dependent()
                        && curve.status.use_flag == 1
                        && !(curve.entity_type == 110
                            && curve_entries[0].is_some_and(|first| first.entity_type == 110))
                }),
                None => false,
            };
            let leaders = [pointer(record, 4, entries), pointer(record, 5, entries)];
            let leaders_valid = leaders.iter().all(|leader| {
                leader.is_some_and(|sequence| {
                    child_valid(
                        sequence,
                        214,
                        |form| matches!(form, 1..=12),
                        entries,
                        records,
                        dialect,
                    )
                })
            });
            let witnesses_valid = (6..=7).all(|index| match record.integer(index) {
                Some(0) => true,
                Some(_) => pointer(record, index, entries).is_some_and(|sequence| {
                    child_valid(sequence, 106, |form| form == 40, entries, records, dialect)
                }),
                None => false,
            });
            children.extend((2..=7).filter_map(|index| pointer(record, index, entries)));
            exact_parameter_count(record, 8) && curves_valid && leaders_valid && witnesses_valid
        }
        (206, 0) => {
            let first = pointer(record, 2, entries);
            let second = pointer(record, 3, entries);
            let leaders_valid = first.is_some_and(|sequence| {
                child_valid(
                    sequence,
                    214,
                    |form| matches!(form, 1..=12),
                    entries,
                    records,
                    dialect,
                )
            }) && match record.integer(3) {
                Some(0) => true,
                Some(_) => second.is_some_and(|sequence| {
                    child_valid(
                        sequence,
                        214,
                        |form| matches!(form, 1..=12),
                        entries,
                        records,
                        dialect,
                    )
                }),
                None => false,
            };
            children.extend(first);
            children.extend(second);
            exact_parameter_count(record, 6)
                && leaders_valid
                && (4..=5).all(|index| finite(record, index))
        }
        (216, 0..=2) => {
            let leaders = [pointer(record, 2, entries), pointer(record, 3, entries)];
            let witnesses = [record.integer(4), record.integer(5)];
            let leaders_valid = leaders.iter().all(|sequence| {
                sequence.is_some_and(|sequence| {
                    child_valid(
                        sequence,
                        214,
                        |form| matches!(form, 1..=12),
                        entries,
                        records,
                        dialect,
                    )
                })
            });
            let witnesses_valid = witnesses.iter().enumerate().all(|(offset, raw)| match raw {
                Some(0) => true,
                Some(_) => pointer(record, 4 + offset, entries).is_some_and(|sequence| {
                    child_valid(sequence, 106, |form| form == 40, entries, records, dialect)
                }),
                None => false,
            });
            children.extend(leaders.into_iter().flatten());
            children.extend((4..=5).filter_map(|index| pointer(record, index, entries)));
            exact_parameter_count(record, 6) && leaders_valid && witnesses_valid
        }
        (218, 0) => {
            let ordinate = pointer(record, 2, entries);
            let valid = ordinate.is_some_and(|sequence| {
                child_valid(sequence, 106, |form| form == 40, entries, records, dialect)
                    || child_valid(
                        sequence,
                        214,
                        |form| matches!(form, 1..=12),
                        entries,
                        records,
                        dialect,
                    )
            });
            children.extend(ordinate);
            exact_parameter_count(record, 3) && valid
        }
        (218, 1) => {
            let witness = pointer(record, 2, entries);
            let leader = pointer(record, 3, entries);
            let valid = witness.is_some_and(|sequence| {
                child_valid(sequence, 106, |form| form == 40, entries, records, dialect)
            }) && leader.is_some_and(|sequence| {
                child_valid(
                    sequence,
                    214,
                    |form| matches!(form, 1..=12),
                    entries,
                    records,
                    dialect,
                )
            });
            children.extend(witness);
            children.extend(leader);
            exact_parameter_count(record, 4) && valid
        }
        (220, 0) => {
            let leader = pointer(record, 2, entries);
            let enclosure_raw = record.integer(3);
            let enclosure = pointer(record, 3, entries);
            let leader_valid = leader.is_some_and(|sequence| {
                child_valid(
                    sequence,
                    214,
                    |form| matches!(form, 1..=12),
                    entries,
                    records,
                    dialect,
                ) && records.get(&sequence).and_then(|record| record.integer(1)) == Some(3)
            });
            let enclosure_valid = match enclosure_raw {
                Some(0) => true,
                Some(_) => enclosure.is_some_and(|sequence| {
                    entries.get(&sequence).is_some_and(|entry| {
                        dimension_enclosure_type_allowed(entry.entity_type, entry.form, dialect)
                            && entry.status.is_physically_dependent()
                            && entry.status.use_flag == 1
                    })
                }),
                None => false,
            };
            children.extend(leader);
            children.extend(enclosure);
            exact_parameter_count(record, 4) && leader_valid && enclosure_valid
        }
        (222, 0..=1) => {
            let first = pointer(record, 2, entries);
            let first_valid = first.is_some_and(|sequence| {
                child_valid(
                    sequence,
                    214,
                    |form| matches!(form, 1..=12),
                    entries,
                    records,
                    dialect,
                )
            });
            let center_valid = finite(record, 3) && finite(record, 4);
            let second_raw = (entry.form == 1).then(|| record.integer(5)).flatten();
            let second = (entry.form == 1)
                .then(|| pointer(record, 5, entries))
                .flatten();
            let second_valid = entry.form == 0
                || match second_raw {
                    Some(0) => true,
                    Some(_) => second.is_some_and(|sequence| {
                        child_valid(
                            sequence,
                            214,
                            |form| matches!(dialect, Dialect::V4_0) || form == 4,
                            entries,
                            records,
                            dialect,
                        )
                    }),
                    None => false,
                };
            children.extend(first);
            children.extend(second);
            exact_parameter_count(record, if entry.form == 0 { 5 } else { 6 })
                && first_valid
                && center_valid
                && second_valid
        }
        _ => false,
    };
    note_valid && fields_valid && dimension_children_valid(entry, &children, entries)
}

fn flag_or_label_valid(
    entry: &DirectoryEntry,
    record: &ParameterRecord,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    dialect: Dialect,
) -> bool {
    let (note_index, count_index, leader_start) = if entry.entity_type == 208 {
        (5, 6, 7)
    } else {
        (1, 2, 3)
    };
    let note = pointer(record, note_index, entries);
    let note_valid =
        note.is_some_and(|sequence| general_note_child_valid(sequence, entries, records, dialect));
    let count = record.count(count_index);
    let leaders_valid = count.is_some_and(|count| {
        (0..count).all(|offset| {
            pointer(record, leader_start + offset, entries).is_some_and(|sequence| {
                child_valid(
                    sequence,
                    214,
                    |form| matches!(form, 1..=12),
                    entries,
                    records,
                    dialect,
                )
            })
        })
    });
    let shape_valid = if entry.entity_type == 208 {
        count.is_some_and(|count| exact_parameter_count(record, 7 + count))
            && (1..=4).all(|index| finite(record, index))
            && note
                .and_then(|sequence| records.get(&sequence))
                .and_then(|note| note.count(1))
                .is_some_and(|strings| {
                    (0..strings)
                        .map(|offset| {
                            note.and_then(|sequence| records.get(&sequence))
                                .and_then(|note| note.integer(2 + offset * 12))
                                .unwrap_or_default()
                        })
                        .sum::<i64>()
                        <= 10
                })
    } else {
        count.is_some_and(|count| count > 0 && exact_parameter_count(record, 3 + count))
    };
    note_valid && leaders_valid && shape_valid
}

fn general_symbol_valid(
    record: &ParameterRecord,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    dialect: Dialect,
) -> bool {
    let note_valid = general_symbol_note_valid(record, entries, records, dialect);
    let Some(geometry_count) = record.count(2).filter(|count| *count > 0) else {
        return false;
    };
    let geometry_valid = (0..geometry_count).all(|offset| {
        pointer(record, 3 + offset, entries).is_some_and(|sequence| {
            entries.get(&sequence).is_some_and(|target| {
                target.status.is_physically_dependent() && target.status.use_flag == 1
            })
        })
    });
    let leader_count_index = 3 + geometry_count;
    let Some(leader_count) = record.count(leader_count_index) else {
        return false;
    };
    let leaders_valid = (0..leader_count).all(|offset| {
        pointer(record, leader_count_index + 1 + offset, entries).is_some_and(|sequence| {
            child_valid(
                sequence,
                214,
                |form| matches!(form, 1..=12),
                entries,
                records,
                dialect,
            )
        })
    });
    note_valid
        && geometry_valid
        && leaders_valid
        && exact_parameter_count(record, leader_count_index + 1 + leader_count)
}

pub(crate) fn section_boundary_type(entry: &DirectoryEntry) -> bool {
    matches!(
        (entry.entity_type, entry.form),
        (100 | 102 | 112 | 126, 0) | (104, 1) | (106, 63)
    )
}

fn fill_pattern_valid_for_dialect(pattern: i64, dialect: Dialect) -> bool {
    if matches!(dialect, Dialect::V4_0) {
        return (0..=19).contains(&pattern);
    }
    matches!(
        pattern,
        0..=20 | 22 | 26 | 28..=29 | 32 | 34 | 36 | 38 | 40..=42 | 46 | 50 | 60
            | 70 | 72 | 80 | 82 | 84 | 86 | 90 | 92 | 94 | 110 | 124 | 134 | 136
            | 140 | 142 | 152 | 154 | 156..=159 | 172 | 174 | 178 | 210 | 220 | 224
            | 226 | 234 | 236 | 240 | 244 | 246 | 252 | 254 | 256 | 262 | 264..=266
            | 268
    )
}

fn zero_or_omitted(record: &ParameterRecord, index: usize) -> bool {
    match record.value(index) {
        None | Some(crate::parameter::TokenValue::Omitted) => true,
        _ => record.number(index) == Some(0.0),
    }
}

fn finite_or_omitted(record: &ParameterRecord, index: usize) -> bool {
    match record.value(index) {
        None | Some(crate::parameter::TokenValue::Omitted) => true,
        _ => finite(record, index),
    }
}

fn sectioned_area_valid(
    ir: &CadIr,
    record: &ParameterRecord,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    dialect: Dialect,
    transform: Transform,
    length_factor: f64,
    resolution: f64,
) -> bool {
    let boundary_sequence = pointer(record, 1, entries);
    let boundary_valid = boundary_sequence
        .and_then(|sequence| entries.get(&sequence).copied())
        .is_some_and(section_boundary_type);
    let Some(island_count) = record.count(8) else {
        return false;
    };
    let island_sequences = (0..island_count)
        .map(|offset| pointer(record, 9 + offset, entries))
        .collect::<Option<Vec<_>>>();
    let islands_valid = island_sequences.as_ref().is_some_and(|islands| {
        islands.iter().all(|sequence| {
            entries
                .get(sequence)
                .is_some_and(|entry| section_boundary_type(entry))
        })
    });
    let definition_sequences = boundary_sequence
        .into_iter()
        .chain(island_sequences.iter().flatten().copied())
        .collect::<Vec<_>>();
    let coplanarity_valid = matches!(dialect, Dialect::V4_0)
        || sectioned_area_pattern_plane(record, transform, length_factor).is_some_and(
            |pattern_plane| {
                sectioned_area_curves_coplanar(ir, &definition_sequences, pattern_plane, resolution)
            },
        );
    let pattern = record
        .integer(2)
        .filter(|value| fill_pattern_valid_for_dialect(*value, dialect));
    let pattern_parameters_valid = pattern.is_some_and(|pattern| {
        if matches!(pattern, 0 | 19) || pattern > 19 {
            (3..=7).all(|index| zero_or_omitted(record, index))
        } else {
            finite_or_omitted(record, 3)
                && finite_or_omitted(record, 4)
                && finite(record, 5)
                && record
                    .number(6)
                    .is_some_and(|distance| distance.is_finite() && distance > 0.0)
                && finite_or_omitted(record, 7)
        }
    });
    boundary_valid
        && pattern_parameters_valid
        && islands_valid
        && coplanarity_valid
        && exact_parameter_count(record, 9 + island_count)
}

pub(super) fn project(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    parameters: &[ParameterRecord],
    global: &ProjectedGlobal,
    ctx: Option<&DecodeContext<'_>>,
) -> ProjectionOutcome {
    let records = parameters
        .iter()
        .map(|record| (record.directory_sequence, record))
        .collect::<BTreeMap<_, _>>();
    let entries = directory
        .iter()
        .map(|entry| (entry.sequence, entry))
        .collect::<BTreeMap<_, _>>();
    let mut decoded = BTreeSet::new();
    let mut losses = Vec::new();

    for (entry, kind) in directory
        .iter()
        .filter_map(|entry| classify(entry.entity_type, entry.form).map(|kind| (entry, kind)))
    {
        let valid = records.get(&entry.sequence).is_some_and(|record| {
            let resolved_transform = resolve_transform(
                entry.transform,
                &entries,
                &records,
                global.length_factor_mm(),
                global.real_precision(),
                &mut BTreeSet::new(),
                ctx,
            )
            .ok();
            let transform_valid = resolved_transform.is_some();
            entry.status.use_flag == 1
                && transform_valid
                && match kind {
                    AnnotationKind::AngularDimension
                    | AnnotationKind::CurveDimension
                    | AnnotationKind::DiameterDimension
                    | AnnotationKind::LinearDimension
                    | AnnotationKind::OrdinateDimension
                    | AnnotationKind::PointDimension
                    | AnnotationKind::RadiusDimension => {
                        dimension_valid(entry, record, &entries, &records, global.dialect())
                    }
                    AnnotationKind::FlagNote | AnnotationKind::GeneralLabel => {
                        flag_or_label_valid(entry, record, &entries, &records, global.dialect())
                    }
                    AnnotationKind::GeneralNote => general_note_valid_for_dialect(
                        record,
                        &entries,
                        global.dialect(),
                        entry.form,
                    ),
                    AnnotationKind::NewGeneralNote => new_general_note_valid(record, &entries),
                    AnnotationKind::Leader => {
                        leader_valid_for_dialect(entry, record, global.dialect())
                    }
                    AnnotationKind::GeneralSymbol => {
                        general_symbol_valid(record, &entries, &records, global.dialect())
                    }
                    AnnotationKind::SectionedArea => resolved_transform.is_some_and(|transform| {
                        sectioned_area_valid(
                            ir,
                            record,
                            &entries,
                            global.dialect(),
                            transform.body_transform(),
                            global.length_factor_mm(),
                            global.minimum_resolution_mm(),
                        )
                    }),
                }
        });
        if valid {
            decoded.insert(entry.sequence);
        } else {
            let message = match kind {
                AnnotationKind::AngularDimension
                | AnnotationKind::CurveDimension
                | AnnotationKind::DiameterDimension
                | AnnotationKind::LinearDimension
                | AnnotationKind::OrdinateDimension
                | AnnotationKind::PointDimension
                | AnnotationKind::RadiusDimension => {
                    "dimension components, role types, transforms, or Directory status are invalid"
                }
                AnnotationKind::GeneralSymbol => {
                    "symbol note, defining geometry, or leader list is invalid"
                }
                AnnotationKind::SectionedArea => {
                    "section boundary, fill pattern, hatch geometry, or island list is invalid"
                }
                AnnotationKind::FlagNote
                | AnnotationKind::GeneralLabel
                | AnnotationKind::GeneralNote
                | AnnotationKind::NewGeneralNote
                | AnnotationKind::Leader => "text count, presentation metrics, encoding, placement, or Directory use flag is invalid",
            };
            losses.push(entity_loss(entry, message));
        }
    }

    ProjectionOutcome { decoded, losses }
}

#[cfg(test)]
mod tests;
