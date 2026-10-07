//! Marker-to-sketch transform selection.

use super::grid::{quantize, GridCoordinate, GridPoint};

use crate::records::{SketchInputEntity, SketchInputKind};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{SketchEntity, SketchEntityId, SketchGeometryDefinition, SketchLocus};
use std::borrow::Borrow;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

const EPS_TRANSFORMS_AXIS_ALIGNED_SKETCH_FRAME_MARKER_TRANSFORM_E8: f64 = 1.0e-8;
const EPS_TRANSFORMS_AFFINE_SKETCH_FRAME_MARKER_TRANSFORM_E8: f64 = 1.0e-8;
const EPS_TRANSFORMS_DIMENSIONED_CIRCLE_SURFACE_TRANSFORMS_E8: f64 = 1.0e-8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct MarkerTransform {
    axes: Axes,
    translation: (i64, i64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sign {
    Negative,
    Positive,
}

impl Sign {
    fn value(self) -> i8 {
        match self {
            Self::Negative => -1,
            Self::Positive => 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Axes {
    Aligned { swap: bool, u: Sign, v: Sign },
    Affine([i64; 4]),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ProfileAxis {
    U,
    V,
}

impl MarkerTransform {
    pub(super) fn apply_axes(self, point: impl Into<GridPoint>) -> Option<(i64, i64)> {
        let point = point.into().cells()?;
        match self.axes {
            Axes::Affine([uu, uv, vu, vv]) => {
                const SCALE: i128 = 1_000_000_000_000;
                let u = i128::from(uu) * i128::from(point.0) + i128::from(uv) * i128::from(point.1);
                let v = i128::from(vu) * i128::from(point.0) + i128::from(vv) * i128::from(point.1);
                let rounded = |value: i128| {
                    let adjustment = if value < 0 { -(SCALE / 2) } else { SCALE / 2 };
                    i64::try_from((value + adjustment) / SCALE).ok()
                };
                Some((rounded(u)?, rounded(v)?))
            }
            Axes::Aligned { swap, u, v } => {
                let point = if swap { (point.1, point.0) } else { point };
                Some((
                    i64::try_from(i128::from(point.0) * i128::from(u.value())).ok()?,
                    i64::try_from(i128::from(point.1) * i128::from(v.value())).ok()?,
                ))
            }
        }
    }

    pub(super) fn apply(self, point: impl Into<GridPoint>) -> Option<(i64, i64)> {
        let point = self.apply_axes(point)?;
        Some((
            point.0.checked_add(self.translation.0)?,
            point.1.checked_add(self.translation.1)?,
        ))
    }

    pub(super) fn profile_axis_for_native(self, native_axis: usize) -> Option<ProfileAxis> {
        let native_axis = match native_axis {
            0 => ProfileAxis::U,
            1 => ProfileAxis::V,
            _ => return None,
        };
        match self.axes {
            Axes::Affine([uu, uv, vu, vv]) => {
                const SCALE: i64 = 1_000_000_000_000;
                let (u, v) = match native_axis {
                    ProfileAxis::U => (uu, vu),
                    ProfileAxis::V => (uv, vv),
                };
                match (u, v) {
                    (u, 0) if u.abs() == SCALE => Some(ProfileAxis::U),
                    (0, v) if v.abs() == SCALE => Some(ProfileAxis::V),
                    _ => None,
                }
            }
            Axes::Aligned { swap, .. } => Some(match (swap, native_axis) {
                (false, ProfileAxis::U) | (true, ProfileAxis::V) => ProfileAxis::U,
                (false, ProfileAxis::V) | (true, ProfileAxis::U) => ProfileAxis::V,
            }),
        }
    }
}

pub(super) fn sketch_frame_marker_transform(
    sketch: &cadmpeg_ir::sketches::Sketch,
    quantum: f64,
) -> Option<MarkerTransform> {
    if matches!(
        sketch.placement,
        cadmpeg_ir::sketches::SketchPlacement::Unresolved { .. }
    ) {
        return Some(MarkerTransform {
            axes: Axes::Aligned {
                swap: false,
                u: Sign::Positive,
                v: Sign::Positive,
            },
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
        let mut matches = vector
            .iter()
            .enumerate()
            .filter(|(_, value)| {
                (value.abs() - 1.0).abs()
                    <= EPS_TRANSFORMS_AXIS_ALIGNED_SKETCH_FRAME_MARKER_TRANSFORM_E8
            })
            .map(|(index, value)| {
                (
                    index,
                    if *value < 0.0 {
                        Sign::Negative
                    } else {
                        Sign::Positive
                    },
                )
            });
        let (index, sign) = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        vector
            .iter()
            .enumerate()
            .all(|(candidate, value)| {
                candidate == index
                    || value.abs() <= EPS_TRANSFORMS_AXIS_ALIGNED_SKETCH_FRAME_MARKER_TRANSFORM_E8
            })
            .then_some((index, sign))
    };
    let (normal_axis, _) = axis(normal)?;
    let mut native_axes = (0..3).filter(|candidate| *candidate != normal_axis);
    let first_native_axis = native_axes.next()?;
    let second_native_axis = native_axes.next()?;
    let (u_axis_index, u_sign) = axis(u_axis)?;
    let (v_axis_index, v_sign) = axis(v_axis)?;
    if u_axis_index == normal_axis || v_axis_index == normal_axis || u_axis_index == v_axis_index {
        return None;
    }
    let swap = match (u_axis_index, v_axis_index) {
        (u, v) if u == first_native_axis && v == second_native_axis => false,
        (u, v) if u == second_native_axis && v == first_native_axis => true,
        _ => return None,
    };
    Some(MarkerTransform {
        axes: Axes::Aligned {
            swap,
            u: u_sign,
            v: v_sign,
        },
        translation: quantize(
            Point2::new(
                -origin[u_axis_index] * f64::from(u_sign.value()),
                -origin[v_axis_index] * f64::from(v_sign.value()),
            ),
            quantum,
        )
        .cells()?,
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
    // The resolved frame holds finite axes and origin; the product axis is
    // computed and can overflow.
    if !(v_axis.into_iter().all(f64::is_finite) && quantum.is_finite() && quantum > 0.0) {
        return None;
    }
    let normal_axis =
        (0..3).max_by(|left, right| normal[*left].abs().total_cmp(&normal[*right].abs()))?;
    if normal[normal_axis].abs() <= EPS_TRANSFORMS_AFFINE_SKETCH_FRAME_MARKER_TRANSFORM_E8 {
        return None;
    }
    let mut native_axes = (0..3).filter(|candidate| *candidate != normal_axis);
    let first_axis = native_axes.next()?;
    let second_axis = native_axes.next()?;
    let tangent = |axis: usize| {
        let mut value = [0.0; 3];
        value[axis] = 1.0;
        value[normal_axis] = -normal[axis] / normal[normal_axis];
        value
    };
    let dot = |left: [f64; 3], right: [f64; 3]| {
        left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
    };
    let first = tangent(first_axis);
    let second = tangent(second_axis);
    let first_row = quantize(
        Point2::new(dot(first, u_axis) * SCALE, dot(second, u_axis) * SCALE),
        1.0,
    )
    .cells()?;
    let second_row = quantize(
        Point2::new(dot(first, v_axis) * SCALE, dot(second, v_axis) * SCALE),
        1.0,
    )
    .cells()?;
    let matrix = [first_row.0, first_row.1, second_row.0, second_row.1];
    let mut zero_world_delta = [0.0; 3];
    zero_world_delta[first_axis] = -origin[first_axis];
    zero_world_delta[second_axis] = -origin[second_axis];
    zero_world_delta[normal_axis] = -(normal[first_axis] * zero_world_delta[first_axis]
        + normal[second_axis] * zero_world_delta[second_axis])
        / normal[normal_axis];
    Some(MarkerTransform {
        axes: Axes::Affine(matrix),
        translation: quantize(
            Point2::new(dot(zero_world_delta, u_axis), dot(zero_world_delta, v_axis)),
            quantum,
        )
        .cells()?,
    })
}

pub(super) fn marker_transforms_with_frame_fallback(
    candidates: Vec<MarkerTransform>,
    sketch: &cadmpeg_ir::sketches::Sketch,
    quantum: f64,
) -> Vec<MarkerTransform> {
    if candidates.is_empty() {
        sketch_frame_marker_transform(sketch, quantum)
            .into_iter()
            .collect()
    } else {
        candidates
    }
}

pub(super) fn dimensioned_circle_surface_transforms(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    sketch: &cadmpeg_ir::sketches::Sketch,
    surfaces: &[cadmpeg_ir::geometry::Surface],
    circles: &[(impl Copy + Into<GridPoint>, GridCoordinate)],
    quantum: f64,
) -> Result<Vec<MarkerTransform>, cadmpeg_core::CodecError> {
    use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
    const OPERATION: &str = "find SLDPRT dimensioned circle surface transforms";

    if circles.is_empty() {
        return Ok(Vec::new());
    }
    let Some((frame_origin, normal, u_axis)) = sketch.resolved_placement() else {
        return Ok(Vec::new());
    };
    let v_axis = cadmpeg_ir::math::Vector3::new(
        normal.y * u_axis.z - normal.z * u_axis.y,
        normal.z * u_axis.x - normal.x * u_axis.z,
        normal.x * u_axis.y - normal.y * u_axis.x,
    );
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let radii = storage.with_storage(|| {
        ctx.collect_btree_set(circles.iter().map(|(_, radius)| *radius), OPERATION)
    })?;
    let mut targets_by_radius = BTreeMap::<GridCoordinate, BTreeSet<GridPoint>>::new();
    for surface in ctx.admit_iter(surfaces, OPERATION)? {
        let Some(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) = surface.geometry.solved()
        else {
            continue;
        };
        let origin = cylinder_surface.origin().get();
        let axis = cylinder_surface.frame().axis().as_raw();
        let radius = cylinder_surface.radius().get();
        let alignment = axis.x * normal.x + axis.y * normal.y + axis.z * normal.z;
        if !alignment.is_finite()
            || (alignment.abs() - 1.0).abs()
                > EPS_TRANSFORMS_DIMENSIONED_CIRCLE_SURFACE_TRANSFORMS_E8
        {
            continue;
        }
        let radius_key = GridCoordinate::new(radius, quantum);
        if !ctx.contains_btree_set(&radii, &radius_key, OPERATION)? {
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
        storage.with_storage(|| {
            ctx.insert_btree_group_set(
                &mut targets_by_radius,
                radius_key,
                quantize(center, quantum),
                OPERATION,
                OPERATION,
            )
        })?;
    }
    let mut compatible = BTreeMap::new();
    for (center, radius) in ctx.admit_iter(circles, OPERATION)? {
        let Some(targets) = ctx.get_btree_map(&targets_by_radius, radius, OPERATION)? else {
            continue;
        };
        let center = (*center).into();
        storage
            .with_storage(|| ctx.insert_btree_map(&mut compatible, center, targets, OPERATION))?;
    }
    if compatible.len() != circles.len() {
        return Ok(Vec::new());
    }
    let candidates = compatible_marker_transform_candidates(ctx, &compatible)?;
    let mut result = Vec::new();
    for transform in ctx.admit_iter(candidates, OPERATION)? {
        let mut used = HashSet::new();
        let complete = ctx.all_by(
            circles,
            |(center, radius)| {
                let Some(center) = transform.apply(*center) else {
                    return Ok(false);
                };
                let hit = match ctx.get_btree_map(&targets_by_radius, radius, OPERATION)? {
                    Some(targets) => {
                        ctx.contains_btree_set(targets, &GridPoint::from(center), OPERATION)?
                    }
                    None => false,
                };
                Ok(hit
                    && storage.with_storage(|| {
                        ctx.insert_hash_set(&mut used, (*radius, center), OPERATION)
                    })?)
            },
            OPERATION,
        )?;
        if complete {
            ctx.push_vec(&mut result, transform, OPERATION)?;
        }
    }
    Ok(result)
}

pub(super) fn dimensioned_circle_transform(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    candidates: &[MarkerTransform],
    circles: &[(impl Copy + Into<GridPoint>, GridCoordinate)],
) -> Result<Option<MarkerTransform>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "select SLDPRT dimensioned circle transform";
    let signature =
        |transform: MarkerTransform| -> Result<Option<Vec<_>>, cadmpeg_core::CodecError> {
            let mut transformed = Vec::new();
            let mut remaining = circles.iter();
            while let Some((center, radius)) = ctx.next_charged(&mut remaining, OPERATION)? {
                let Some(center) = transform.apply(*center) else {
                    return Ok(None);
                };
                ctx.push_vec(&mut transformed, (center.0, center.1, *radius), OPERATION)?;
            }
            ctx.sort_unstable_by(&mut transformed, |value| value, Ord::cmp, OPERATION)?;
            Ok(
                (transformed.len() == circles.len() && !transformed.is_empty())
                    .then_some(transformed),
            )
        };
    let Some((first, rest)) = candidates.split_first() else {
        return Ok(None);
    };
    let (first_signature, _first_storage) =
        ctx.with_scoped_storage(OPERATION, || signature(*first))?;
    let Some(first_signature) = first_signature else {
        return Ok(None);
    };
    let mut remaining = rest.iter();
    while let Some(transform) = ctx.next_charged(&mut remaining, OPERATION)? {
        let (other, _other_storage) =
            ctx.with_scoped_storage(OPERATION, || signature(*transform))?;
        let Some(other) = other else {
            return Ok(None);
        };
        if !ctx.equal(&other, &first_signature, OPERATION)? {
            return Ok(None);
        }
    }
    Ok(ctx
        .min_by_key(
            candidates,
            |transform| {
                let (swap, u, v, matrix) = match transform.axes {
                    Axes::Aligned { swap, u, v } => (swap, u.value(), v.value(), None),
                    Axes::Affine(matrix) => (false, 1, 1, Some(matrix)),
                };
                Ok((swap, u, v, matrix, transform.translation))
            },
            |left, right| Ok(left.cmp(right)),
            OPERATION,
        )?
        .copied())
}

#[cfg(test)]
fn unique_marker_transform(
    marker_points: &HashSet<(i64, i64)>,
    locus_points: &HashSet<(i64, i64)>,
) -> Option<MarkerTransform> {
    let identity = MarkerTransform {
        axes: Axes::Aligned {
            swap: false,
            u: Sign::Positive,
            v: Sign::Positive,
        },
        translation: (0, 0),
    };
    if let Some(transform) = unique_transform_translation(identity, marker_points, locus_points) {
        return Some(transform);
    }
    let mut scored = Vec::new();
    for swap in [false, true] {
        for u_sign in [Sign::Negative, Sign::Positive] {
            for v_sign in [Sign::Negative, Sign::Positive] {
                if !swap && u_sign == Sign::Positive && v_sign == Sign::Positive {
                    continue;
                }
                let transform = MarkerTransform {
                    axes: Axes::Aligned {
                        swap,
                        u: u_sign,
                        v: v_sign,
                    },
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    compatible_locus_points: &HashMap<(i64, i64), HashSet<(i64, i64)>>,
) -> Result<Option<MarkerTransform>, cadmpeg_core::CodecError> {
    let candidates = compatible_marker_transform_candidates(
        ctx,
        &compatible_locus_points
            .iter()
            .map(|(point, loci)| {
                (
                    GridPoint::from(*point),
                    loci.iter()
                        .copied()
                        .map(GridPoint::from)
                        .collect::<BTreeSet<_>>(),
                )
            })
            .collect(),
    )?;
    let [transform] = candidates.as_slice() else {
        return Ok(None);
    };
    Ok(Some(*transform))
}

pub(super) fn compatible_marker_transform_candidates<V>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    compatible_locus_points: &BTreeMap<GridPoint, V>,
) -> Result<Vec<MarkerTransform>, cadmpeg_core::CodecError>
where
    V: Borrow<BTreeSet<GridPoint>>,
{
    const OPERATION: &str = "score SLDPRT compatible marker transforms";
    let score =
        |axes: MarkerTransform| -> Result<BTreeMap<(i64, i64), usize>, cadmpeg_core::CodecError> {
            let mut translations = BTreeMap::<(i64, i64), usize>::new();
            for (marker, loci) in ctx.admit_iter(compatible_locus_points, OPERATION)? {
                let Some(marker) = axes.apply_axes(*marker) else {
                    continue;
                };
                for locus in ctx.admit_iter(loci.borrow(), OPERATION)? {
                    let Some(locus) = locus.cells() else {
                        continue;
                    };
                    let Some(translation) = locus
                        .0
                        .checked_sub(marker.0)
                        .zip(locus.1.checked_sub(marker.1))
                    else {
                        continue;
                    };
                    let count = ctx
                        .entry_btree_map(&mut translations, translation, OPERATION)?
                        .or_insert(0_usize);
                    // A count never exceeds the number of compatible pairs.
                    *count += 1;
                }
            }
            Ok(translations)
        };
    let identity = MarkerTransform {
        axes: Axes::Aligned {
            swap: false,
            u: Sign::Positive,
            v: Sign::Positive,
        },
        translation: (0, 0),
    };
    let first = {
        let (translations, _storage) = ctx.with_scoped_storage(OPERATION, || score(identity))?;
        unique_scored_transform(ctx, identity, translations)?
    };
    if let Some(transform) = first {
        let mut result = Vec::new();
        ctx.push_vec(&mut result, transform, OPERATION)?;
        return Ok(result);
    }
    let mut scored = Vec::new();
    for swap in [false, true] {
        for u_sign in [Sign::Negative, Sign::Positive] {
            for v_sign in [Sign::Negative, Sign::Positive] {
                if !swap && u_sign == Sign::Positive && v_sign == Sign::Positive {
                    continue;
                }
                let axes = MarkerTransform {
                    axes: Axes::Aligned {
                        swap,
                        u: u_sign,
                        v: v_sign,
                    },
                    translation: (0, 0),
                };
                let (translations, _storage) =
                    ctx.with_scoped_storage(OPERATION, || score(axes))?;
                for (translation, count) in ctx.admit_iter(translations, OPERATION)? {
                    ctx.push_vec(
                        &mut scored,
                        (
                            MarkerTransform {
                                translation,
                                ..axes
                            },
                            count,
                        ),
                        OPERATION,
                    )?;
                }
            }
        }
    }
    let Some(maximum) = ctx
        .max_by_key(
            &scored,
            |(_, count)| Ok(*count),
            |left, right| Ok(left.cmp(right)),
            OPERATION,
        )?
        .map(|(_, count)| *count)
        .filter(|count| *count >= 2)
    else {
        return Ok(Vec::new());
    };
    let mut candidates = Vec::new();
    for (transform, count) in ctx.admit_iter(scored, OPERATION)? {
        if count == maximum {
            ctx.push_vec(&mut candidates, transform, OPERATION)?;
        }
    }
    if candidates.len() == 1 {
        return Ok(candidates);
    }
    if ctx.any_by(
        &candidates,
        |transform| Ok(transform.translation == (0, 0)),
        OPERATION,
    )? {
        ctx.retain_vec(
            &mut candidates,
            |transform| Ok(transform.translation == (0, 0)),
            OPERATION,
        )?;
    }
    Ok(candidates)
}

fn unique_scored_transform(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    axes: MarkerTransform,
    translations: BTreeMap<(i64, i64), usize>,
) -> Result<Option<MarkerTransform>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "score SLDPRT compatible marker transforms";
    let mut maximum = 0;
    for count in ctx
        .admit_iter(&translations, OPERATION)?
        .map(|(_, count)| *count)
    {
        maximum = maximum.max(count);
    }
    if maximum < 2 {
        return Ok(None);
    }
    let mut remaining = translations.into_iter();
    let Some(translation) = ctx.find_map(
        &mut remaining,
        |(translation, count)| Ok((count == maximum).then_some(translation)),
        OPERATION,
    )?
    else {
        return Ok(None);
    };
    Ok(
        (!ctx.any_by(remaining, |(_, count)| Ok(count == maximum), OPERATION)?).then_some(
            MarkerTransform {
                translation,
                ..axes
            },
        ),
    )
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

/// A sketch entity's marker loci beside the marker kind they are written as.
///
/// Minted only by [`sketch_entity_marker_loci`], which returns nothing for an
/// entity that has no marker loci, so an empty locus list has no spelling.
pub(super) struct SketchEntityMarkerLoci {
    kind: SketchInputKind,
    loci: Vec<(Point2, SketchLocus)>,
}

impl SketchEntityMarkerLoci {
    /// The marker kind every locus in this set is written as.
    pub(super) const fn kind(&self) -> SketchInputKind {
        self.kind
    }

    /// The marker loci, at least one.
    pub(super) fn loci(&self) -> &[(Point2, SketchLocus)] {
        &self.loci
    }
}

/// The marker loci of `entity`, absent when it contributes no marker.
pub(super) fn sketch_entity_marker_loci(entity: &SketchEntity) -> Option<SketchEntityMarkerLoci> {
    let kind = match entity.geometry.definition() {
        SketchGeometryDefinition::Point { .. } => SketchInputKind::Point,
        SketchGeometryDefinition::Arc { .. } => SketchInputKind::Arc,
        SketchGeometryDefinition::Line { .. }
        | SketchGeometryDefinition::ReferenceLine { .. }
        | SketchGeometryDefinition::Circle { .. }
        | SketchGeometryDefinition::Ellipse { .. }
        | SketchGeometryDefinition::Hyperbola { .. }
        | SketchGeometryDefinition::Parabola { .. }
        | SketchGeometryDefinition::Nurbs { .. }
        | SketchGeometryDefinition::ExternalReference { .. }
        | SketchGeometryDefinition::Native { .. }
        | SketchGeometryDefinition::Text { .. } => SketchInputKind::LineOrCircle,
    };
    let loci = sketch_entity_loci(entity);
    (!loci.is_empty()).then_some(SketchEntityMarkerLoci { kind, loci })
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum SketchLocusRole {
    Entity,
    Start,
    End,
    Center,
}

impl SketchLocusRole {
    pub(super) const fn of_locus(locus: &SketchLocus) -> Self {
        match locus {
            SketchLocus::Entity(_) => Self::Entity,
            SketchLocus::Start(_) => Self::Start,
            SketchLocus::End(_) => Self::End,
            SketchLocus::Center(_) => Self::Center,
        }
    }

    pub(super) fn matches(self, locus: &SketchLocus) -> bool {
        self == Self::of_locus(locus)
    }
}

pub(super) fn copy_sketch_entity_identity(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    entity: &SketchEntityId,
    operation: &'static str,
) -> Result<SketchEntityId, cadmpeg_core::CodecError> {
    entity.try_clone_for_decode(ctx, operation)
}

pub(super) fn sketch_entity_loci(entity: &SketchEntity) -> Vec<(Point2, SketchLocus)> {
    sketch_entity_locus_points(entity)
        .into_iter()
        .flatten()
        .map(|(point, role)| {
            let id = entity.id().clone();
            let locus = match role {
                SketchLocusRole::Entity => SketchLocus::Entity(id),
                SketchLocusRole::Start => SketchLocus::Start(id),
                SketchLocusRole::End => SketchLocus::End(id),
                SketchLocusRole::Center => SketchLocus::Center(id),
            };
            (point, locus)
        })
        .collect()
}

pub(super) fn sketch_entity_locus_points(
    entity: &SketchEntity,
) -> [Option<(Point2, SketchLocusRole)>; 3] {
    let locus = |point: Point2, role| Some((point, role));
    match entity.geometry.definition() {
        SketchGeometryDefinition::Point { position } => {
            [locus(position.get(), SketchLocusRole::Entity), None, None]
        }
        SketchGeometryDefinition::Line { start, end } => [
            locus(start.get(), SketchLocusRole::Start),
            locus(end.get(), SketchLocusRole::End),
            None,
        ],
        SketchGeometryDefinition::ReferenceLine { .. } => [None, None, None],
        SketchGeometryDefinition::Circle { center, .. } => {
            [locus(center.get(), SketchLocusRole::Center), None, None]
        }
        SketchGeometryDefinition::Ellipse {
            center,
            major_angle,
            radii,
            bounds,
        } => {
            let major_radius = radii.major();
            let minor_radius = radii.minor();
            let mut loci = [locus(center.get(), SketchLocusRole::Center), None, None];
            if let Some([start, end]) = bounds {
                let point = |parameter: f64| {
                    Point2::new(
                        center.u + major_angle.get().cos() * major_radius.get() * parameter.cos()
                            - major_angle.get().sin() * minor_radius.get() * parameter.sin(),
                        center.v
                            + major_angle.get().sin() * major_radius.get() * parameter.cos()
                            + major_angle.get().cos() * minor_radius.get() * parameter.sin(),
                    )
                };
                loci[1] = locus(point(start.get()), SketchLocusRole::Start);
                loci[2] = locus(point(end.get()), SketchLocusRole::End);
            }
            loci
        }
        SketchGeometryDefinition::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => [
            locus(center.get(), SketchLocusRole::Center),
            locus(
                Point2::new(
                    center.u + radius.get() * start_angle.get().cos(),
                    center.v + radius.get() * start_angle.get().sin(),
                ),
                SketchLocusRole::Start,
            ),
            locus(
                Point2::new(
                    center.u + radius.get() * end_angle.get().cos(),
                    center.v + radius.get() * end_angle.get().sin(),
                ),
                SketchLocusRole::End,
            ),
        ],
        SketchGeometryDefinition::Hyperbola {
            center,
            major_angle,
            major_radius,
            minor_radius,
            bounds,
        } => {
            let mut loci = [locus(center.get(), SketchLocusRole::Center), None, None];
            let point = |parameter: cadmpeg_ir::scalar::FiniteReal| {
                let (_, x) =
                    cadmpeg_ir::math::scaled_sinh_cosh(major_radius.magnitude(), parameter).ok()?;
                let y =
                    match cadmpeg_ir::math::scaled_sinh_cosh(minor_radius.magnitude(), parameter) {
                        Ok((sinh, _)) => Some(sinh),
                        Err((sinh, _)) => cadmpeg_ir::scalar::FiniteReal::new(sinh),
                    }?;
                let (x, y) = (x.get(), y.get());
                let (sine, cosine) = major_angle.get().sin_cos();
                let point = Point2::new(
                    center.u + x * cosine - y * sine,
                    center.v + x * sine + y * cosine,
                );
                point.is_finite().then_some(point)
            };
            if let Some([start, end]) = bounds {
                if let (Some(start), Some(end)) = (point(*start), point(*end)) {
                    loci[1] = locus(start, SketchLocusRole::Start);
                    loci[2] = locus(end, SketchLocusRole::End);
                }
            }
            loci
        }
        SketchGeometryDefinition::Parabola {
            vertex,
            axis_angle,
            focal_length,
            bounds,
        } => {
            let point = |parameter: f64| {
                let x = cadmpeg_ir::math::product_quotient(
                    [parameter, parameter],
                    [4.0, focal_length.get()],
                )?
                .get();
                let point = Point2::new(
                    vertex.u + x * axis_angle.get().cos() - parameter * axis_angle.get().sin(),
                    vertex.v + x * axis_angle.get().sin() + parameter * axis_angle.get().cos(),
                );
                point.is_finite().then_some(point)
            };
            match bounds {
                Some([start, end]) => match (point(start.get()), point(end.get())) {
                    (Some(start), Some(end)) => [
                        locus(start, SketchLocusRole::Start),
                        locus(end, SketchLocusRole::End),
                        None,
                    ],
                    _ => [None, None, None],
                },
                None => [None, None, None],
            }
        }
        SketchGeometryDefinition::Nurbs { curve } => {
            let control_points = curve.control_points();
            [
                locus(control_points[0].get(), SketchLocusRole::Start),
                locus(
                    control_points[control_points.len() - 1].get(),
                    SketchLocusRole::End,
                ),
                None,
            ]
        }
        SketchGeometryDefinition::Text { .. }
        | SketchGeometryDefinition::ExternalReference { .. }
        | SketchGeometryDefinition::Native { .. } => [None, None, None],
    }
}

pub(super) fn locus_key(locus: &SketchLocus) -> (&str, u8) {
    match locus {
        SketchLocus::Entity(entity) => (entity.as_str(), 0),
        SketchLocus::Start(entity) => (entity.as_str(), 1),
        SketchLocus::End(entity) => (entity.as_str(), 2),
        SketchLocus::Center(entity) => (entity.as_str(), 3),
    }
}

pub(super) fn locus_entity(locus: &SketchLocus) -> &SketchEntityId {
    match locus {
        SketchLocus::Entity(entity)
        | SketchLocus::Start(entity)
        | SketchLocus::End(entity)
        | SketchLocus::Center(entity) => entity,
    }
}

#[derive(Clone, Copy)]
pub(super) enum MarkerEntityFilter<'a> {
    All,
    Lines(&'a [SketchEntity]),
}

/// The sketch entities a marker resolves to, in identity order.
pub(super) fn marker_entities<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    marker_id: &'a str,
    markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    filter: MarkerEntityFilter<'_>,
) -> Result<Vec<SketchEntityId>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "resolve SLDPRT marker entities";
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let identities = marker_entities_inner(
        ctx,
        &mut storage,
        marker_id,
        markers_by_id,
        loci_by_marker,
        filter,
        &mut HashSet::new(),
    )?;
    let mut result = Vec::new();
    for identity in ctx.admit_iter(identities, OPERATION)? {
        let identity = copy_sketch_entity_identity(ctx, identity, OPERATION)?;
        ctx.push_vec(&mut result, identity, OPERATION)?;
    }
    Ok(result)
}

fn marker_entities_inner<'a, 'loci>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    marker_id: &'a str,
    markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
    loci_by_marker: &'loci HashMap<String, Vec<SketchLocus>>,
    filter: MarkerEntityFilter<'_>,
    visited: &mut HashSet<&'a str>,
) -> Result<BTreeSet<&'loci SketchEntityId>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "resolve SLDPRT linked marker entities";
    let _nesting = ctx.enter_nested(OPERATION)?;
    let mut direct = None;
    if let Some(loci) = ctx.get_hash_map(loci_by_marker, marker_id, OPERATION)? {
        let mut identities = BTreeSet::new();
        for locus in ctx.admit_iter(loci, OPERATION)? {
            let identity = locus_entity(locus);
            if let MarkerEntityFilter::Lines(entities) = filter {
                let Some(entity) =
                    super::relation_loci::find_profile_entity(ctx, entities, identity, OPERATION)?
                else {
                    continue;
                };
                if !matches!(
                    entity.geometry.definition(),
                    SketchGeometryDefinition::Line { .. }
                ) {
                    continue;
                }
            }
            storage.with_storage(|| ctx.insert_btree_set(&mut identities, identity, OPERATION))?;
        }
        if matches!(filter, MarkerEntityFilter::Lines(_)) || identities.len() == 1 {
            return Ok(identities);
        }
        direct = Some(identities);
    }
    if !storage.with_storage(|| ctx.insert_hash_set(visited, marker_id, OPERATION))? {
        return Ok(BTreeSet::new());
    }
    let mut selected = direct;
    if let Some(marker) = ctx.get_hash_map(markers_by_id, marker_id, OPERATION)? {
        for link in ctx.admit_iter(marker.links(), OPERATION)? {
            if ctx.equal(link.entity_ref.as_str(), marker_id, OPERATION)?
                || (matches!(filter, MarkerEntityFilter::Lines(_))
                    && matches!(marker.kind(), SketchInputKind::Relation(_))
                    && super::typed_relations::relation_link_identifies_owner(marker, link))
            {
                continue;
            }
            let candidates = marker_entities_inner(
                ctx,
                storage,
                &link.entity_ref,
                markers_by_id,
                loci_by_marker,
                filter,
                visited,
            )?;
            if candidates.is_empty() {
                continue;
            }
            let Some(entities) = selected.as_mut() else {
                selected = Some(candidates);
                continue;
            };
            ctx.retain_btree_set(
                entities,
                |identity| ctx.contains_btree_set(&candidates, identity, OPERATION),
                OPERATION,
            )?;
        }
    }
    ctx.remove_hash_set(visited, marker_id, OPERATION)?;
    Ok(selected.unwrap_or_default())
}

pub(super) fn sort_marker_entity_ids(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    entities: &mut Vec<SketchEntityId>,
    operation: &'static str,
) -> Result<(), cadmpeg_core::CodecError> {
    ctx.sort_unstable_by(entities, |value| value, Ord::cmp, operation)?;
    ctx.dedup_vec(entities, "deduplicate SLDPRT marker entity identities")?;
    Ok(())
}

#[cfg(test)]
pub(in crate::resolved_features) mod tests;

#[cfg(test)]
mod numerical_range_tests;
