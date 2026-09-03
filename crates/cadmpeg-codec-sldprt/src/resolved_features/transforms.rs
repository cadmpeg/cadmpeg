//! Marker-to-sketch transform selection.

use crate::records::SketchInputEntity;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{SketchEntity, SketchEntityId, SketchGeometry, SketchLocus};
use std::collections::{HashMap, HashSet};

#[cfg(test)]
use super::axes::{
    compact_line_reference_direction, declared_line_reference_directions, line_reference_direction,
    linear_pattern_display_directions,
};
#[cfg(test)]
use super::bindings::{bind_pattern_inputs, bind_sweep_adjacent_profiles};
#[cfg(test)]
use super::component_paths::project_dissected_sketches;
#[cfg(test)]
use super::curves::{closed_marker_profiles, fitted_marker_circle, resolve_connected_marker_arcs};
#[cfg(test)]
use super::dimensions::{project_dimensioned_sketch_geometry, project_marker_dimensioned_circles};
#[cfg(test)]
use super::endpoints::inferred_point_coordinates_by_index;
#[cfg(test)]
use super::profiles::{
    bind_sketch_profiles, nested_profile_contains_declared_circular_carriers,
    project_marker_backed_sketches,
};
#[cfg(test)]
use super::projections::{
    bind_circular_profile_by_dimension, project_compact_edge_selections,
    type_display_relation_parameters,
};
#[cfg(test)]
use super::relation_geometry::{
    implicit_circle_marker, owned_relation_parameters, project_relation_bindings,
    project_relation_point_geometry, project_relation_solved_line_geometry,
    project_relation_solved_point_geometry, relation_parameter_by_display_name,
};
#[cfg(test)]
use super::relation_loci::{
    doubled_profile_distance_loci, marker_accepts_locus, marker_point_locus,
    profile_loci_by_marker, qualified_point_marker_key, relation_constraint_is_inactive,
    relation_operand_loci, relation_operand_marker, resolved_marker_locus,
    single_marker_line_entity, typed_relation_definition, unique_linked_endpoint_locus,
    unique_profile_axis_distance_locus, unique_profile_axis_distance_pair,
    unique_profile_distance_loci_pair, unique_profile_distance_locus,
    unique_profile_line_angle_entity, unique_profile_line_angle_pair,
    unique_profile_line_distance_entity, unique_profile_line_distance_pair,
    unique_profile_line_point_locus, unique_profile_point_line_entity,
    unique_profile_point_line_pair, unique_repaired_profile_line_angle_pair,
    unique_repaired_profile_line_distance_pair, unique_repaired_profile_point_line_pair,
};
#[cfg(test)]
use super::relation_records::{bind_circle_dimension_centers, bind_detached_relation_drivers};
#[cfg(test)]
use super::selections::{input_owned_edge_selections, COMPACT_EDGE_VECTOR_MARKER};
#[cfg(test)]
use super::typed_relations::{
    binary_relation_matches_evaluated_geometry, legacy_terminal_profile_indexed_endpoints,
    line_endpoint_markers, marker_owns_constraint, marker_relation_is_inactive,
    relation_owner_markers, typed_marker_relation_definition,
    typed_marker_relation_definition_in_sketch, unique_axis_aligned_linked_loci,
};
#[cfg(test)]
use super::{LEGACY_EXTENDED_SKETCH_MARKER, LEGACY_SKETCH_MARKER, SKETCH_MARKER};

const EPS_TRANSFORMS_AXIS_ALIGNED_SKETCH_FRAME_MARKER_TRANSFORM_E8: f64 = 1.0e-8;
const EPS_TRANSFORMS_AFFINE_SKETCH_FRAME_MARKER_TRANSFORM_E8: f64 = 1.0e-8;
const EPS_TRANSFORMS_DIMENSIONED_CIRCLE_SURFACE_TRANSFORMS_E8: f64 = 1.0e-8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct MarkerTransform {
    swap: bool,
    u_sign: i8,
    v_sign: i8,
    affine_matrix: Option<[i64; 4]>,
    translation: (i64, i64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ProfileAxis {
    U,
    V,
}

impl MarkerTransform {
    pub(super) fn apply_axes(self, point: (i64, i64)) -> Option<(i64, i64)> {
        if let Some([uu, uv, vu, vv]) = self.affine_matrix {
            const SCALE: i128 = 1_000_000_000_000;
            let u = i128::from(uu) * i128::from(point.0) + i128::from(uv) * i128::from(point.1);
            let v = i128::from(vu) * i128::from(point.0) + i128::from(vv) * i128::from(point.1);
            let rounded = |value: i128| {
                let adjustment = if value < 0 { -(SCALE / 2) } else { SCALE / 2 };
                i64::try_from((value + adjustment) / SCALE).ok()
            };
            return Some((rounded(u)?, rounded(v)?));
        }
        let (u, v) = if self.swap { (point.1, point.0) } else { point };
        Some((
            i64::try_from(i128::from(u) * i128::from(self.u_sign)).ok()?,
            i64::try_from(i128::from(v) * i128::from(self.v_sign)).ok()?,
        ))
    }

    pub(super) fn apply(self, point: (i64, i64)) -> Option<(i64, i64)> {
        let point = self.apply_axes(point)?;
        Some((
            point.0.checked_add(self.translation.0)?,
            point.1.checked_add(self.translation.1)?,
        ))
    }

    pub(super) fn profile_axis_for_native(self, native_axis: usize) -> Option<ProfileAxis> {
        if native_axis > 1 {
            return None;
        }
        if let Some([uu, uv, vu, vv]) = self.affine_matrix {
            const SCALE: i64 = 1_000_000_000_000;
            let (u, v) = match native_axis {
                0 => (uu, vu),
                1 => (uv, vv),
                _ => unreachable!("native axis was bounded above"),
            };
            return match (u, v) {
                (u, 0) if u.abs() == SCALE => Some(ProfileAxis::U),
                (0, v) if v.abs() == SCALE => Some(ProfileAxis::V),
                _ => None,
            };
        }
        Some(match (self.swap, native_axis) {
            (false, 0) | (true, 1) => ProfileAxis::U,
            (false, 1) | (true, 0) => ProfileAxis::V,
            _ => unreachable!("native axis was bounded above"),
        })
    }
}

pub(super) fn sketch_frame_marker_transform(
    sketch: &cadmpeg_ir::sketches::Sketch,
    quantum: f64,
) -> Option<MarkerTransform> {
    if sketch.placement == cadmpeg_ir::sketches::SketchPlacement::Unresolved {
        return Some(MarkerTransform {
            swap: false,
            u_sign: 1,
            v_sign: 1,
            affine_matrix: None,
            translation: (0, 0),
        });
    }
    axis_aligned_sketch_frame_marker_transform(sketch, quantum)
        .or_else(|| affine_sketch_frame_marker_transform(sketch, quantum))
}

fn axis_aligned_sketch_frame_marker_transform(
    sketch: &cadmpeg_ir::sketches::Sketch,
    quantum: f64,
) -> Option<MarkerTransform> {
    let (origin, normal, u_axis) = sketch.resolved_placement()?;
    let normal = [normal.x, normal.y, normal.z];
    let u_axis = [u_axis.x, u_axis.y, u_axis.z];
    let v_axis = [
        normal[1] * u_axis[2] - normal[2] * u_axis[1],
        normal[2] * u_axis[0] - normal[0] * u_axis[2],
        normal[0] * u_axis[1] - normal[1] * u_axis[0],
    ];
    let origin = [origin.x, origin.y, origin.z];
    let axis = |vector: [f64; 3]| {
        let matches = vector
            .iter()
            .enumerate()
            .filter(|(_, value)| {
                (value.abs() - 1.0).abs()
                    <= EPS_TRANSFORMS_AXIS_ALIGNED_SKETCH_FRAME_MARKER_TRANSFORM_E8
            })
            .map(|(index, value)| (index, if *value < 0.0 { -1 } else { 1 }))
            .collect::<Vec<_>>();
        let [(index, sign)] = matches.as_slice() else {
            return None;
        };
        vector
            .iter()
            .enumerate()
            .all(|(candidate, value)| {
                candidate == *index
                    || value.abs() <= EPS_TRANSFORMS_AXIS_ALIGNED_SKETCH_FRAME_MARKER_TRANSFORM_E8
            })
            .then_some((*index, *sign))
    };
    let (normal_axis, _) = axis(normal)?;
    let native_axes = (0..3)
        .filter(|candidate| *candidate != normal_axis)
        .collect::<Vec<_>>();
    let [first_native_axis, second_native_axis] = native_axes.as_slice() else {
        return None;
    };
    let (u_axis_index, u_sign) = axis(u_axis)?;
    let (v_axis_index, v_sign) = axis(v_axis)?;
    if u_axis_index == normal_axis || v_axis_index == normal_axis || u_axis_index == v_axis_index {
        return None;
    }
    let swap = match (u_axis_index, v_axis_index) {
        (u, v) if u == *first_native_axis && v == *second_native_axis => false,
        (u, v) if u == *second_native_axis && v == *first_native_axis => true,
        _ => return None,
    };
    Some(MarkerTransform {
        swap,
        u_sign,
        v_sign,
        affine_matrix: None,
        translation: (
            (-origin[u_axis_index] * f64::from(u_sign) / quantum).round() as i64,
            (-origin[v_axis_index] * f64::from(v_sign) / quantum).round() as i64,
        ),
    })
}

fn affine_sketch_frame_marker_transform(
    sketch: &cadmpeg_ir::sketches::Sketch,
    quantum: f64,
) -> Option<MarkerTransform> {
    const SCALE: f64 = 1_000_000_000_000.0;
    let (origin, normal, u_axis) = sketch.resolved_placement()?;
    let normal = [normal.x, normal.y, normal.z];
    let u_axis = [u_axis.x, u_axis.y, u_axis.z];
    let v_axis = [
        normal[1] * u_axis[2] - normal[2] * u_axis[1],
        normal[2] * u_axis[0] - normal[0] * u_axis[2],
        normal[0] * u_axis[1] - normal[1] * u_axis[0],
    ];
    let origin = [origin.x, origin.y, origin.z];
    if !(normal
        .into_iter()
        .chain(u_axis)
        .chain(v_axis)
        .chain(origin)
        .all(f64::is_finite)
        && quantum.is_finite()
        && quantum > 0.0)
    {
        return None;
    }
    let normal_axis =
        (0..3).max_by(|left, right| normal[*left].abs().total_cmp(&normal[*right].abs()))?;
    if normal[normal_axis].abs() <= EPS_TRANSFORMS_AFFINE_SKETCH_FRAME_MARKER_TRANSFORM_E8 {
        return None;
    }
    let native_axes = (0..3)
        .filter(|candidate| *candidate != normal_axis)
        .collect::<Vec<_>>();
    let [first_axis, second_axis] = native_axes.as_slice() else {
        return None;
    };
    let tangent = |axis: usize| {
        let mut value = [0.0; 3];
        value[axis] = 1.0;
        value[normal_axis] = -normal[axis] / normal[normal_axis];
        value
    };
    let dot = |left: [f64; 3], right: [f64; 3]| {
        left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
    };
    let first = tangent(*first_axis);
    let second = tangent(*second_axis);
    let matrix = [
        (dot(first, u_axis) * SCALE).round() as i64,
        (dot(second, u_axis) * SCALE).round() as i64,
        (dot(first, v_axis) * SCALE).round() as i64,
        (dot(second, v_axis) * SCALE).round() as i64,
    ];
    let mut zero_world_delta = [0.0; 3];
    zero_world_delta[*first_axis] = -origin[*first_axis];
    zero_world_delta[*second_axis] = -origin[*second_axis];
    zero_world_delta[normal_axis] = -(normal[*first_axis] * zero_world_delta[*first_axis]
        + normal[*second_axis] * zero_world_delta[*second_axis])
        / normal[normal_axis];
    Some(MarkerTransform {
        swap: false,
        u_sign: 1,
        v_sign: 1,
        affine_matrix: Some(matrix),
        translation: (
            (dot(zero_world_delta, u_axis) / quantum).round() as i64,
            (dot(zero_world_delta, v_axis) / quantum).round() as i64,
        ),
    })
}

pub(super) fn marker_transforms_with_frame_fallback(
    candidates: &[MarkerTransform],
    sketch: &cadmpeg_ir::sketches::Sketch,
    quantum: f64,
) -> Vec<MarkerTransform> {
    if candidates.is_empty() {
        sketch_frame_marker_transform(sketch, quantum)
            .into_iter()
            .collect()
    } else {
        candidates.to_vec()
    }
}

pub(super) fn dimensioned_circle_surface_transforms(
    sketch: &cadmpeg_ir::sketches::Sketch,
    surfaces: &[cadmpeg_ir::geometry::Surface],
    circles: &[((i64, i64), i64)],
    quantum: f64,
) -> Vec<MarkerTransform> {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    if circles.is_empty() {
        return Vec::new();
    }
    let Some((frame_origin, normal, u_axis)) = sketch.resolved_placement() else {
        return Vec::new();
    };
    let v_axis = cadmpeg_ir::math::Vector3::new(
        normal.y * u_axis.z - normal.z * u_axis.y,
        normal.z * u_axis.x - normal.x * u_axis.z,
        normal.x * u_axis.y - normal.y * u_axis.x,
    );
    let mut targets_by_radius = HashMap::<i64, HashSet<(i64, i64)>>::new();
    for surface in surfaces {
        let SurfaceGeometry::Cylinder {
            origin,
            axis,
            radius,
            ..
        } = &surface.geometry
        else {
            continue;
        };
        let alignment = axis.x * normal.x + axis.y * normal.y + axis.z * normal.z;
        if !alignment.is_finite()
            || (alignment.abs() - 1.0).abs()
                > EPS_TRANSFORMS_DIMENSIONED_CIRCLE_SURFACE_TRANSFORMS_E8
        {
            continue;
        }
        let radius_key = (radius / quantum).round() as i64;
        if !circles
            .iter()
            .any(|(_, candidate)| *candidate == radius_key)
        {
            continue;
        }
        let delta = cadmpeg_ir::math::Vector3::new(
            origin.x - frame_origin.x,
            origin.y - frame_origin.y,
            origin.z - frame_origin.z,
        );
        let center = Point2::new(
            delta.x * u_axis.x + delta.y * u_axis.y + delta.z * u_axis.z,
            delta.x * v_axis.x + delta.y * v_axis.y + delta.z * v_axis.z,
        );
        targets_by_radius
            .entry(radius_key)
            .or_default()
            .insert(quantize(center, quantum));
    }
    let compatible = circles
        .iter()
        .filter_map(|(center, radius)| Some((*center, targets_by_radius.get(radius)?.clone())))
        .collect::<HashMap<_, _>>();
    if compatible.len() != circles.len() {
        return Vec::new();
    }
    let candidates = compatible_marker_transform_candidates(&compatible);
    candidates
        .into_iter()
        .filter(|transform| {
            let mut used = HashSet::new();
            circles.iter().all(|(center, radius)| {
                transform.apply(*center).is_some_and(|center| {
                    targets_by_radius
                        .get(radius)
                        .is_some_and(|targets| targets.contains(&center))
                        && used.insert((*radius, center))
                })
            })
        })
        .collect()
}

pub(super) fn dimensioned_circle_transform(
    candidates: &[MarkerTransform],
    circles: &[((i64, i64), i64)],
) -> Option<MarkerTransform> {
    let signature = |transform: MarkerTransform| {
        let mut transformed = circles
            .iter()
            .filter_map(|(center, radius)| {
                let center = transform.apply(*center)?;
                Some((center.0, center.1, *radius))
            })
            .collect::<Vec<_>>();
        transformed.sort_unstable();
        (transformed.len() == circles.len() && !transformed.is_empty()).then_some(transformed)
    };
    let first_signature = signature(*candidates.first()?)?;
    if candidates
        .iter()
        .skip(1)
        .any(|transform| signature(*transform).as_ref() != Some(&first_signature))
    {
        return None;
    }
    candidates.iter().copied().min_by_key(|transform| {
        (
            transform.swap,
            transform.u_sign,
            transform.v_sign,
            transform.affine_matrix,
            transform.translation,
        )
    })
}

#[cfg(test)]
fn unique_marker_transform(
    marker_points: &HashSet<(i64, i64)>,
    locus_points: &HashSet<(i64, i64)>,
) -> Option<MarkerTransform> {
    let identity = MarkerTransform {
        swap: false,
        u_sign: 1,
        v_sign: 1,
        affine_matrix: None,
        translation: (0, 0),
    };
    if let Some(transform) = unique_transform_translation(identity, marker_points, locus_points) {
        return Some(transform);
    }
    let mut scored = Vec::new();
    for swap in [false, true] {
        for u_sign in [-1, 1] {
            for v_sign in [-1, 1] {
                if !swap && u_sign == 1 && v_sign == 1 {
                    continue;
                }
                let transform = MarkerTransform {
                    swap,
                    u_sign,
                    v_sign,
                    affine_matrix: None,
                    translation: (0, 0),
                };
                let transformed = marker_points
                    .iter()
                    .filter_map(|point| transform.apply_axes(*point))
                    .collect::<HashSet<_>>();
                let mut translations = HashMap::<(i64, i64), usize>::new();
                for marker in &transformed {
                    for locus in locus_points {
                        let Some(translation) = locus
                            .0
                            .checked_sub(marker.0)
                            .zip(locus.1.checked_sub(marker.1))
                        else {
                            continue;
                        };
                        *translations.entry(translation).or_default() += 1;
                    }
                }
                scored.extend(translations.into_iter().map(|(translation, count)| {
                    (
                        MarkerTransform {
                            translation,
                            ..transform
                        },
                        count,
                    )
                }));
            }
        }
    }
    let maximum = scored
        .iter()
        .map(|(_, count)| *count)
        .max()
        .filter(|count| *count >= 2)?;
    let candidates = scored
        .into_iter()
        .filter_map(|(transform, count)| (count == maximum).then_some(transform))
        .collect::<Vec<_>>();
    if let [transform] = candidates.as_slice() {
        return Some(*transform);
    }
    let mut zero_translation = candidates
        .iter()
        .copied()
        .filter(|transform| transform.translation == (0, 0));
    let first = zero_translation.next()?;
    zero_translation.next().is_none().then_some(first)
}

#[cfg(test)]
fn unique_compatible_marker_transform(
    compatible_locus_points: &HashMap<(i64, i64), HashSet<(i64, i64)>>,
) -> Option<MarkerTransform> {
    let candidates = compatible_marker_transform_candidates(compatible_locus_points);
    let [transform] = candidates.as_slice() else {
        return None;
    };
    Some(*transform)
}

pub(super) fn compatible_marker_transform_candidates(
    compatible_locus_points: &HashMap<(i64, i64), HashSet<(i64, i64)>>,
) -> Vec<MarkerTransform> {
    let score = |axes: MarkerTransform| {
        let mut translations = HashMap::<(i64, i64), usize>::new();
        for (marker, loci) in compatible_locus_points {
            let Some(marker) = axes.apply_axes(*marker) else {
                continue;
            };
            for locus in loci {
                let Some(translation) = locus
                    .0
                    .checked_sub(marker.0)
                    .zip(locus.1.checked_sub(marker.1))
                else {
                    continue;
                };
                *translations.entry(translation).or_default() += 1;
            }
        }
        translations
    };
    let identity = MarkerTransform {
        swap: false,
        u_sign: 1,
        v_sign: 1,
        affine_matrix: None,
        translation: (0, 0),
    };
    if let Some(transform) = unique_scored_transform(identity, score(identity)) {
        return vec![transform];
    }
    let mut scored = Vec::new();
    for swap in [false, true] {
        for u_sign in [-1, 1] {
            for v_sign in [-1, 1] {
                if !swap && u_sign == 1 && v_sign == 1 {
                    continue;
                }
                let axes = MarkerTransform {
                    swap,
                    u_sign,
                    v_sign,
                    affine_matrix: None,
                    translation: (0, 0),
                };
                scored.extend(score(axes).into_iter().map(|(translation, count)| {
                    (
                        MarkerTransform {
                            translation,
                            ..axes
                        },
                        count,
                    )
                }));
            }
        }
    }
    let Some(maximum) = scored
        .iter()
        .map(|(_, count)| *count)
        .max()
        .filter(|count| *count >= 2)
    else {
        return Vec::new();
    };
    let candidates = scored
        .into_iter()
        .filter_map(|(transform, count)| (count == maximum).then_some(transform))
        .collect::<Vec<_>>();
    if let [transform] = candidates.as_slice() {
        return vec![*transform];
    }
    let zero_translation = candidates
        .iter()
        .copied()
        .filter(|transform| transform.translation == (0, 0))
        .collect::<Vec<_>>();
    if !zero_translation.is_empty() {
        return zero_translation;
    }
    candidates
}

fn unique_scored_transform(
    axes: MarkerTransform,
    translations: HashMap<(i64, i64), usize>,
) -> Option<MarkerTransform> {
    let maximum = translations
        .values()
        .copied()
        .max()
        .filter(|count| *count >= 2)?;
    let mut candidates = translations
        .into_iter()
        .filter_map(|(translation, count)| (count == maximum).then_some(translation));
    let translation = candidates.next()?;
    candidates.next().is_none().then_some(MarkerTransform {
        translation,
        ..axes
    })
}

#[cfg(test)]
fn unique_transform_translation(
    transform: MarkerTransform,
    marker_points: &HashSet<(i64, i64)>,
    locus_points: &HashSet<(i64, i64)>,
) -> Option<MarkerTransform> {
    let transformed = marker_points
        .iter()
        .filter_map(|point| transform.apply_axes(*point))
        .collect::<HashSet<_>>();
    let mut translations = HashMap::<(i64, i64), usize>::new();
    for marker in &transformed {
        for locus in locus_points {
            let Some(translation) = locus
                .0
                .checked_sub(marker.0)
                .zip(locus.1.checked_sub(marker.1))
            else {
                continue;
            };
            *translations.entry(translation).or_default() += 1;
        }
    }
    let maximum = translations
        .values()
        .copied()
        .max()
        .filter(|count| *count >= 2)?;
    let mut candidates = translations
        .into_iter()
        .filter_map(|(translation, count)| (count == maximum).then_some(translation));
    let translation = candidates.next()?;
    candidates.next().is_none().then_some(MarkerTransform {
        translation,
        ..transform
    })
}

pub(super) fn quantize(point: Point2, quantum: f64) -> (i64, i64) {
    (
        (point.u / quantum).round() as i64,
        (point.v / quantum).round() as i64,
    )
}

pub(super) fn sketch_entity_loci(entity: &SketchEntity) -> Vec<(Point2, SketchLocus)> {
    let locus = |point, locus| (point, locus);
    match &entity.geometry {
        SketchGeometry::Point { position } => {
            vec![locus(*position, SketchLocus::Entity(entity.id.clone()))]
        }
        SketchGeometry::Line { start, end } => vec![
            locus(*start, SketchLocus::Start(entity.id.clone())),
            locus(*end, SketchLocus::End(entity.id.clone())),
        ],
        SketchGeometry::ReferenceLine { .. } => Vec::new(),
        SketchGeometry::Circle { center, .. } => {
            vec![locus(*center, SketchLocus::Center(entity.id.clone()))]
        }
        SketchGeometry::Ellipse {
            center,
            major_angle,
            major_radius,
            minor_radius,
            bounds,
        } => {
            let mut loci = vec![locus(*center, SketchLocus::Center(entity.id.clone()))];
            if let Some([start, end]) = bounds {
                let point = |parameter: f64| {
                    Point2::new(
                        center.u + major_angle.0.cos() * major_radius.0 * parameter.cos()
                            - major_angle.0.sin() * minor_radius.0 * parameter.sin(),
                        center.v
                            + major_angle.0.sin() * major_radius.0 * parameter.cos()
                            + major_angle.0.cos() * minor_radius.0 * parameter.sin(),
                    )
                };
                loci.push(locus(point(start.0), SketchLocus::Start(entity.id.clone())));
                loci.push(locus(point(end.0), SketchLocus::End(entity.id.clone())));
            }
            loci
        }
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => vec![
            locus(*center, SketchLocus::Center(entity.id.clone())),
            locus(
                Point2::new(
                    center.u + radius.0 * start_angle.0.cos(),
                    center.v + radius.0 * start_angle.0.sin(),
                ),
                SketchLocus::Start(entity.id.clone()),
            ),
            locus(
                Point2::new(
                    center.u + radius.0 * end_angle.0.cos(),
                    center.v + radius.0 * end_angle.0.sin(),
                ),
                SketchLocus::End(entity.id.clone()),
            ),
        ],
        SketchGeometry::Hyperbola {
            center,
            major_angle,
            major_radius,
            minor_radius,
            bounds,
        } => {
            let mut loci = vec![locus(*center, SketchLocus::Center(entity.id.clone()))];
            let point = |parameter: f64| {
                let x = major_radius.0 * parameter.cosh();
                let y = minor_radius.0 * parameter.sinh();
                Point2::new(
                    center.u + x * major_angle.0.cos() - y * major_angle.0.sin(),
                    center.v + x * major_angle.0.sin() + y * major_angle.0.cos(),
                )
            };
            if let Some([start, end]) = bounds {
                loci.push(locus(point(*start), SketchLocus::Start(entity.id.clone())));
                loci.push(locus(point(*end), SketchLocus::End(entity.id.clone())));
            }
            loci
        }
        SketchGeometry::Parabola {
            vertex,
            axis_angle,
            focal_length,
            bounds,
        } => {
            let point = |parameter: f64| {
                let x = parameter * parameter / (4.0 * focal_length.0);
                Point2::new(
                    vertex.u + x * axis_angle.0.cos() - parameter * axis_angle.0.sin(),
                    vertex.v + x * axis_angle.0.sin() + parameter * axis_angle.0.cos(),
                )
            };
            match bounds {
                Some([start, end]) => vec![
                    locus(point(*start), SketchLocus::Start(entity.id.clone())),
                    locus(point(*end), SketchLocus::End(entity.id.clone())),
                ],
                None => Vec::new(),
            }
        }
        SketchGeometry::Nurbs { control_points, .. } if !control_points.is_empty() => vec![
            locus(control_points[0], SketchLocus::Start(entity.id.clone())),
            locus(
                control_points[control_points.len() - 1],
                SketchLocus::End(entity.id.clone()),
            ),
        ],
        SketchGeometry::Nurbs { .. }
        | SketchGeometry::Text { .. }
        | SketchGeometry::ExternalReference { .. }
        | SketchGeometry::Native { .. } => Vec::new(),
    }
}

pub(super) fn locus_key(locus: &SketchLocus) -> (&str, u8) {
    match locus {
        SketchLocus::Entity(entity) => (&entity.0, 0),
        SketchLocus::Start(entity) => (&entity.0, 1),
        SketchLocus::End(entity) => (&entity.0, 2),
        SketchLocus::Center(entity) => (&entity.0, 3),
    }
}

pub(super) fn locus_entity(locus: &SketchLocus) -> SketchEntityId {
    match locus {
        SketchLocus::Entity(entity)
        | SketchLocus::Start(entity)
        | SketchLocus::End(entity)
        | SketchLocus::Center(entity) => entity.clone(),
    }
}

pub(super) fn marker_entities(
    marker_id: &str,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Vec<SketchEntityId> {
    marker_entities_inner(
        marker_id,
        markers_by_id,
        loci_by_marker,
        &mut HashSet::new(),
    )
}

fn marker_entities_inner(
    marker_id: &str,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    visited: &mut HashSet<String>,
) -> Vec<SketchEntityId> {
    let direct = loci_by_marker.get(marker_id).map(|loci| {
        loci.iter()
            .map(locus_entity)
            .collect::<HashSet<SketchEntityId>>()
    });
    if direct.as_ref().is_some_and(|entities| entities.len() == 1) {
        return direct.into_iter().flatten().collect();
    }
    if !visited.insert(marker_id.to_string()) {
        return Vec::new();
    }
    let Some(marker) = markers_by_id.get(marker_id) else {
        return direct.into_iter().flatten().collect();
    };
    let mut linked = marker
        .links
        .iter()
        .filter(|link| link.entity_ref != marker_id)
        .map(|link| {
            marker_entities_inner(
                &link.entity_ref,
                markers_by_id,
                loci_by_marker,
                &mut visited.clone(),
            )
            .into_iter()
            .collect::<HashSet<_>>()
        })
        .filter(|entities| !entities.is_empty());
    let mut entities = if let Some(direct) = direct {
        direct
    } else if let Some(linked) = linked.next() {
        linked
    } else {
        return Vec::new();
    };
    for candidates in linked {
        entities.retain(|entity| candidates.contains(entity));
    }
    let mut entities = entities.into_iter().collect::<Vec<_>>();
    entities.sort();
    entities
}

#[cfg(test)]
mod tests;
