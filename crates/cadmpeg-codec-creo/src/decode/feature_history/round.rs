// SPDX-License-Identifier: Apache-2.0
//! Round and chamfer radius reconstruction from support geometry.

use super::super::analytic::{
    circular_cone, cross, dot, placed_planes, reconciled_model_plane, solve_planes, ConeEquation,
    CylinderEquation, PlaneEquation,
};
use super::super::sketch::normalized;
use super::super::surfaces::{prototype_scalar, unique_surface_prototype_associations};
use super::super::uniqueness::exactly_one;
use super::agreed_feature_geometry_ids;
use crate::container::ContainerScan;
use crate::legacy_feature::LegacyRoundRadius;
use crate::surface::Type24RoundEnvelope;
use cadmpeg_core::decode::alloc_filled;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::ids::SurfaceId;
use std::collections::BTreeSet;

const EPS_CYLINDER_FIT: f64 = 1e-8;
const EPS_ROUND_CAP_GAP: f64 = 1e-9;
const EPS_ROUND_CAP_PARALLEL: f64 = 1e-10;
const EPS_ROUND_RADIUS_RECONCILIATION: f64 = 1e-9;
const EPS_ROUND_SUPPORT_ORTHOGONAL: f64 = 1e-9;

pub(in super::super) fn parallel_support_radius(
    planes: impl IntoIterator<Item = ([f64; 3], [f64; 3])>,
) -> Option<f64> {
    let planes = planes.into_iter().collect::<Vec<_>>();
    let mut radii = Vec::new();
    for first in 0..planes.len() {
        for second in first + 1..planes.len() {
            let first_normal = normalized(planes[first].1)?;
            let second_normal = normalized(planes[second].1)?;
            let alignment = first_normal
                .iter()
                .zip(second_normal)
                .map(|(first, second)| first * second)
                .sum::<f64>();
            if alignment.abs() < 1.0 - 1e-9 {
                continue;
            }
            let gap = planes[second]
                .0
                .iter()
                .zip(planes[first].0)
                .zip(first_normal)
                .map(|((second, first), normal)| (second - first) * normal)
                .sum::<f64>()
                .abs();
            let scale = planes[first]
                .0
                .iter()
                .chain(&planes[second].0)
                .map(|value| value.abs())
                .fold(1.0, f64::max);
            if gap > 1e-9 * scale {
                radii.push(0.5 * gap);
            }
        }
    }
    let radius = *radii.first()?;
    let scale = radius.abs().max(1.0);
    radii
        .iter()
        .all(|candidate| (candidate - radius).abs() <= 1e-9 * scale)
        .then_some(radius)
}

pub(in super::super) fn slot_fillet_cylinder(
    cap_planes: [PlaneEquation; 2],
    support_planes: &[PlaneEquation],
) -> Option<CylinderEquation> {
    let axis = normalized(cap_planes[0].normal)?;
    let second_cap_normal = normalized(cap_planes[1].normal)?;
    if (dot(axis, second_cap_normal).abs() - 1.0).abs() > 1e-10 {
        return None;
    }
    let cap_gap = dot(
        axis,
        std::array::from_fn(|index| cap_planes[1].origin[index] - cap_planes[0].origin[index]),
    )
    .abs();
    if cap_gap <= 1e-9 {
        return None;
    }
    let mut midplanes = Vec::<(PlaneEquation, f64)>::new();
    for first in 0..support_planes.len() {
        let first_normal = normalized(support_planes[first].normal)?;
        if dot(first_normal, axis).abs() > 1e-9 {
            return None;
        }
        for second in first + 1..support_planes.len() {
            let second_normal = normalized(support_planes[second].normal)?;
            if (dot(first_normal, second_normal).abs() - 1.0).abs() > 1e-10 {
                continue;
            }
            let gap = dot(
                first_normal,
                std::array::from_fn(|index| {
                    support_planes[second].origin[index] - support_planes[first].origin[index]
                }),
            )
            .abs();
            if gap <= 1e-9 {
                continue;
            }
            midplanes.push((
                PlaneEquation {
                    origin: std::array::from_fn(|index| {
                        0.5 * (support_planes[first].origin[index]
                            + support_planes[second].origin[index])
                    }),
                    normal: first_normal,
                },
                0.5 * gap,
            ));
        }
    }
    let mut candidates = Vec::<CylinderEquation>::new();
    for first in 0..midplanes.len() {
        for second in first + 1..midplanes.len() {
            let radius = midplanes[first].1;
            let scale = radius.max(midplanes[second].1).max(1.0);
            if (midplanes[second].1 - radius).abs() > 1e-9 * scale
                || dot(midplanes[first].0.normal, midplanes[second].0.normal).abs() > 1.0 - 1e-9
            {
                continue;
            }
            let Some(origin) =
                solve_planes(&[cap_planes[0], midplanes[first].0, midplanes[second].0])
            else {
                continue;
            };
            let tangent_to_all = support_planes.iter().all(|plane| {
                let Some(normal) = normalized(plane.normal) else {
                    return false;
                };
                let distance = dot(
                    normal,
                    std::array::from_fn(|index| origin[index] - plane.origin[index]),
                )
                .abs();
                (distance - radius).abs() <= EPS_CYLINDER_FIT * scale
            });
            if tangent_to_all {
                candidates.push(CylinderEquation {
                    origin,
                    axis,
                    ref_direction: midplanes[first].0.normal,
                    radius,
                });
            }
        }
    }
    let first = *candidates.first()?;
    let scale = first.radius.max(1.0);
    candidates
        .iter()
        .all(|candidate| {
            let origin_delta: [f64; 3] =
                std::array::from_fn(|index| candidate.origin[index] - first.origin[index]);
            (candidate.radius - first.radius).abs() <= 1e-9 * scale
                && (dot(candidate.axis, first.axis).abs() - 1.0).abs() <= 1e-10
                && dot(
                    cross(origin_delta, first.axis),
                    cross(origin_delta, first.axis),
                )
                .sqrt()
                    <= EPS_CYLINDER_FIT * scale
        })
        .then_some(first)
}

pub(in super::super) fn outline_has_unique_radius_delta(
    frame: crate::surface::TorusOutlineFrame,
    radius: f64,
) -> bool {
    let scale = frame
        .values
        .iter()
        .map(|value| value.abs())
        .fold(radius.abs().max(1.0), f64::max);
    frame.values[..3]
        .iter()
        .zip(&frame.values[3..])
        .filter(|(first, second)| ((*second - *first).abs() - radius).abs() <= 1e-9 * scale)
        .count()
        == 1
}

pub(in super::super) fn coordinate_pair_proves_torus_radii(
    first: [f64; 2],
    second: [f64; 2],
    major_radius: f64,
    minor_radius: f64,
) -> bool {
    let scale = first.iter().chain(&second).map(|value| value.abs()).fold(
        major_radius.abs().max(minor_radius.abs()).max(1.0),
        f64::max,
    );
    let close = |left: f64, right: f64| (left - right).abs() <= 1e-9 * scale;
    let proves = |outer: f64, minor: f64| {
        close(outer.abs(), 2.0 * (major_radius + minor_radius)) && close(minor.abs(), minor_radius)
    };
    let direct = proves(second[0] - first[0], second[1] - first[1]);
    let swapped = proves(second[1] - first[0], second[0] - first[1]);
    direct ^ swapped
}

pub(in super::super) fn five_coordinate_envelope_proves_torus_radii(
    envelope: crate::surface::Type26FiveCoordinateEnvelope,
    major_radius: f64,
    minor_radius: f64,
) -> bool {
    let [a1, a2, b0, b1, b2] = envelope.values;
    let scale = envelope.values.iter().map(|value| value.abs()).fold(
        major_radius.abs().max(minor_radius.abs()).max(1.0),
        f64::max,
    );
    let close = |left: f64, right: f64| (left - right).abs() <= 1e-9 * scale;
    close(a1, b0)
        && coordinate_pair_proves_torus_radii([a1, a2], [b1, b2], major_radius, minor_radius)
}

pub(in super::super) fn paired_five_coordinate_sphere_center(
    envelopes: [crate::surface::Type26FiveCoordinateEnvelope; 2],
    radius: f64,
) -> Option<[f64; 3]> {
    (radius.is_finite() && radius > 0.0).then_some(())?;
    let scale = envelopes
        .iter()
        .flat_map(|envelope| envelope.values)
        .map(f64::abs)
        .fold(radius.max(1.0), f64::max);
    let close = |left: f64, right: f64| (left - right).abs() <= 1e-9 * scale;
    let decoded = envelopes.map(|envelope| {
        let [x_min, z0, y_min, radial_max, z1] = envelope.values;
        (close(x_min, y_min)
            && close(radial_max - x_min, 2.0 * radius)
            && close((z1 - z0).abs(), radius))
        .then_some(([x_min, radial_max], [z0, z1]))
    });
    let [Some((first_radial, first_axial)), Some((second_radial, second_axial))] = decoded else {
        return None;
    };
    (close(first_radial[0], second_radial[0]) && close(first_radial[1], second_radial[1]))
        .then_some(())?;
    let shared = [first_axial[0], first_axial[1]]
        .into_iter()
        .filter(|candidate| {
            [second_axial[0], second_axial[1]]
                .into_iter()
                .any(|other| close(*candidate, other))
        })
        .collect::<Vec<_>>();
    let [center_z] = shared.as_slice() else {
        return None;
    };
    let axial_min = first_axial
        .into_iter()
        .chain(second_axial)
        .fold(f64::INFINITY, f64::min);
    let axial_max = first_axial
        .into_iter()
        .chain(second_axial)
        .fold(f64::NEG_INFINITY, f64::max);
    (close(axial_max - axial_min, 2.0 * radius)
        && close(*center_z - axial_min, radius)
        && close(axial_max - *center_z, radius))
    .then_some([
        0.5 * (first_radial[0] + first_radial[1]),
        0.5 * (first_radial[0] + first_radial[1]),
        *center_z,
    ])
}

pub(in super::super) fn unique_surface_parameter_record<'a>(
    scan: &'a ContainerScan,
    row: &crate::surface::SurfaceRow,
) -> Option<&'a crate::surface::SurfaceParameterRecord> {
    exactly_one(
        scan.surfaces
            .parameters
            .iter()
            .filter(|record| record.offset == row.offset),
    )
}

pub(in super::super) fn unique_section_torus_minor_radius(
    scan: &ContainerScan,
    row: &crate::surface::SurfaceRow,
) -> Option<f64> {
    let section = scan.framing.sections.iter().find(|section| {
        row.offset >= section.offset && row.offset < section.offset.saturating_add(section.length)
    })?;
    let prototype = exactly_one(scan.surfaces.prototype_records.iter().filter(|prototype| {
        prototype.family == crate::surface::SurfacePrototypeFamily::Torus
            && prototype.offset >= section.offset
            && prototype.offset < section.offset.saturating_add(section.length)
    }))?;
    prototype_scalar(prototype, "radius2").filter(|radius| radius.is_finite() && *radius > 0.0)
}

pub(in super::super) fn replayed_torus_minor_radius(
    scan: &ContainerScan,
    row: &crate::surface::SurfaceRow,
    record: &crate::surface::SurfaceParameterRecord,
) -> Option<f64> {
    let prototype_minor_radius = unique_section_torus_minor_radius(scan, row)?;
    record.type26_replayed_minor_radius(row.type_byte, prototype_minor_radius)
}

pub(in super::super) fn prototype_round_radius(
    scan: &ContainerScan,
    rows: &[&crate::surface::SurfaceRow],
) -> Option<f64> {
    let feature_id = rows.first()?.feature_id;
    let (radius1, radius2) = exactly_one(
        unique_surface_prototype_associations(scan)
            .into_iter()
            .filter(|(record, row, _)| {
                record.family == crate::surface::SurfacePrototypeFamily::Torus
                    && row.feature_id == feature_id
                    && rows.iter().any(|candidate| candidate.offset == row.offset)
            })
            .filter_map(|(record, _, _)| {
                Some((
                    prototype_scalar(record, "radius1")?,
                    prototype_scalar(record, "radius2")?,
                ))
            }),
    )?;
    (radius1.is_finite() && radius1 >= 0.0 && radius2.is_finite() && radius2 > 0.0).then_some(())?;
    rows.iter()
        .all(|row| {
            let Some(record) = unique_surface_parameter_record(scan, row) else {
                return false;
            };
            record.torus_radius_overrides(row.type_byte).is_none()
                && (replayed_torus_minor_radius(scan, row, record)
                    .is_some_and(|radius| radius.to_bits() == radius2.to_bits())
                    || record
                        .torus_outline_frame(row.type_byte)
                        .is_some_and(|frame| outline_has_unique_radius_delta(frame, radius2))
                    || record
                        .type26_five_coordinate_envelope(row.type_byte)
                        .is_some_and(|envelope| {
                            five_coordinate_envelope_proves_torus_radii(envelope, radius1, radius2)
                        })
                    || record
                        .type26_split_coordinate_envelope(row.type_byte)
                        .is_some_and(|envelope| {
                            let [a1, a2, b1, b2] = envelope.values;
                            coordinate_pair_proves_torus_radii([a1, a2], [b1, b2], radius1, radius2)
                        }))
        })
        .then_some(radius2)
}

pub(in super::super) fn round_constant_radius(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> Option<f64> {
    match scan
        .features
        .legacy_rounds
        .iter()
        .find(|round| round.feature_id == feature_id)
        .map(|round| round.radius)
    {
        Some(LegacyRoundRadius::Constant(radius)) => {
            if !legacy_round_radius_agrees(scan, ir, feature_id, radius) {
                return None;
            }
            return Some(radius);
        }
        Some(LegacyRoundRadius::Ambiguous) => return None,
        Some(LegacyRoundRadius::NotPresent) | None => {}
    }
    if let Some(radius) = round_direct_radii(scan, feature_id)
        .as_deref()
        .and_then(unique_positive_length)
    {
        if complete_direct_placed_cylinder_radius_agreement(scan, ir, feature_id)
            .is_some_and(|agrees| !agrees)
        {
            return None;
        }
        return Some(radius);
    }
    let generated_rows = scan
        .surfaces
        .rows
        .iter()
        .filter(|row| row.feature_id == feature_id)
        .collect::<Vec<_>>();
    if generated_rows.is_empty() {
        return round_support_radius(scan, ir, feature_id);
    }
    // Unequal decoded rolling-radius samples identify a variable-radius
    // round even when another generated row has no radius proof. A support
    // plane fallback must not turn that incomplete, unequal sample set into
    // a false constant radius.
    if differing_positive_lengths(&round_observed_radii(scan, feature_id)) {
        return None;
    }
    let cylinder_rows = generated_rows
        .iter()
        .filter(|row| row.kind == crate::surface::SurfaceKind::Cylinder)
        .copied()
        .collect::<Vec<_>>();
    if cylinder_rows.is_empty() {
        if generated_rows
            .iter()
            .any(|row| row.kind != crate::surface::SurfaceKind::TorusOrSphere)
        {
            return None;
        }
        return prototype_round_radius(scan, &generated_rows);
    }
    if cylinder_rows.len() != generated_rows.len()
        && generated_rows.iter().all(|row| {
            matches!(
                row.kind,
                crate::surface::SurfaceKind::Cylinder | crate::surface::SurfaceKind::TorusOrSphere
            )
        })
    {
        if let Some(radii) = mixed_round_radius_samples(scan, ir, &generated_rows) {
            return unique_positive_length(&radii);
        }
    }
    let cylinder_radii = round_placed_cylinder_radii(scan, ir, feature_id);
    if differing_positive_lengths(&cylinder_radii) {
        // Independent placed cylinder samples remain decisive when an
        // unresolved toroidal sibling prevents the complete mixed-family
        // witness from being assembled.
        return None;
    }
    // A complete placed set of generated cylinder carriers is an independent
    // radius witness when the remaining generated rows are cap or support
    // planes. A toroidal or other rolling carrier still needs its own family
    // proof, so it must not be hidden by the cylinder subset.
    let non_radius_rows_are_planes = generated_rows.iter().all(|row| {
        matches!(
            row.kind,
            crate::surface::SurfaceKind::Cylinder | crate::surface::SurfaceKind::Plane
        )
    });
    if cylinder_radii.len() == cylinder_rows.len() && non_radius_rows_are_planes {
        return unique_positive_length(&cylinder_radii);
    }
    round_support_radius(scan, ir, feature_id)
}

fn legacy_round_radius_agrees(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
    radius: f64,
) -> bool {
    if !radius.is_finite() || radius <= 0.0 {
        return false;
    }
    let mut samples = round_observed_radii(scan, feature_id);
    samples.extend(round_placed_cylinder_radii(scan, ir, feature_id));
    if samples
        .iter()
        .any(|sample| !sample.is_finite() || *sample <= 0.0)
    {
        return false;
    }
    let scale = samples
        .iter()
        .copied()
        .map(f64::abs)
        .chain(std::iter::once(radius.abs()))
        .fold(1.0, f64::max);
    samples
        .iter()
        .all(|sample| (sample - radius).abs() <= EPS_ROUND_RADIUS_RECONCILIATION * scale)
}

fn complete_direct_placed_cylinder_radius_agreement(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> Option<bool> {
    let cylinder_rows = scan
        .surfaces
        .rows
        .iter()
        .filter(|row| {
            row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Cylinder
        })
        .collect::<Vec<_>>();
    let direct_radii = cylinder_rows
        .iter()
        .map(|row| {
            unique_surface_parameter_record(scan, row)
                .and_then(|record| record.type24_generated_round_radius(row.type_byte))
        })
        .collect::<Option<Vec<_>>>()?;
    let placed_radii = cylinder_rows
        .iter()
        .map(|row| round_placed_cylinder_radius(ir, row))
        .collect::<Option<Vec<_>>>()?;
    Some(
        direct_radii
            .iter()
            .zip(placed_radii)
            .all(|(direct, placed)| {
                let scale = direct.abs().max(placed.abs()).max(1.0);
                direct.is_finite()
                    && *direct > 0.0
                    && placed.is_finite()
                    && placed > 0.0
                    && (direct - placed).abs() <= EPS_ROUND_RADIUS_RECONCILIATION * scale
            }),
    )
}

pub(in super::super) fn mixed_round_radius_samples(
    scan: &ContainerScan,
    ir: &CadIr,
    rows: &[&crate::surface::SurfaceRow],
) -> Option<Vec<f64>> {
    let cylinder_rows = rows
        .iter()
        .copied()
        .filter(|row| row.kind == crate::surface::SurfaceKind::Cylinder)
        .collect::<Vec<_>>();
    let torus_rows = rows
        .iter()
        .copied()
        .filter(|row| row.kind == crate::surface::SurfaceKind::TorusOrSphere)
        .collect::<Vec<_>>();
    (!cylinder_rows.is_empty() && !torus_rows.is_empty()).then_some(())?;

    let cylinder_radii = cylinder_rows
        .iter()
        .map(|row| round_cylinder_radius(scan, ir, row))
        .collect::<Option<Vec<_>>>()?;
    let torus_radii = mixed_torus_radius_samples(scan, &torus_rows)?;
    Some(cylinder_radii.into_iter().chain(torus_radii).collect())
}

pub(in super::super) fn mixed_torus_radius_samples(
    scan: &ContainerScan,
    rows: &[&crate::surface::SurfaceRow],
) -> Option<Vec<f64>> {
    let parameters = rows
        .iter()
        .map(|row| Some((row.type_byte, unique_surface_parameter_record(scan, row)?)))
        .collect::<Option<Vec<_>>>()?;
    if parameters
        .iter()
        .all(|(type_byte, record)| record.torus_radius_overrides(*type_byte).is_some())
    {
        return Some(
            parameters
                .iter()
                .filter_map(|(type_byte, record)| record.torus_radius_overrides(*type_byte))
                .map(|overrides| overrides.radius2)
                .collect(),
        );
    }
    if parameters
        .iter()
        .any(|(type_byte, record)| record.torus_radius_overrides(*type_byte).is_some())
    {
        return None;
    }
    prototype_round_radius(scan, rows)
        .and_then(|radius| alloc_filled(rows.len(), radius, "creo_torus_radius_samples").ok())
}

pub(in super::super) fn round_cylinder_radius(
    scan: &ContainerScan,
    ir: &CadIr,
    row: &crate::surface::SurfaceRow,
) -> Option<f64> {
    unique_surface_parameter_record(scan, row)
        .and_then(|record| record.type24_generated_round_radius(row.type_byte))
        .or_else(|| round_placed_cylinder_radius(ir, row))
}

pub(in super::super) fn round_support_radius(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> Option<f64> {
    let affected_ids = agreed_feature_geometry_ids(
        &scan.features.affected_ids,
        &scan.features.replay_affected_ids,
        feature_id,
    )?;
    let [first_cap_id, second_cap_id, support_ids @ ..] = affected_ids else {
        return None;
    };
    if first_cap_id == second_cap_id {
        return None;
    }
    let local_planes = placed_planes(scan);
    let first_cap = reconciled_model_plane(&local_planes, ir, *first_cap_id)?;
    let second_cap = reconciled_model_plane(&local_planes, ir, *second_cap_id)?;
    let first_cap_normal = normalized(first_cap.normal)?;
    let second_cap_normal = normalized(second_cap.normal)?;
    if (dot(first_cap_normal, second_cap_normal).abs() - 1.0).abs() > EPS_ROUND_CAP_PARALLEL {
        return None;
    }
    let cap_gap = dot(
        first_cap_normal,
        std::array::from_fn(|index| second_cap.origin[index] - first_cap.origin[index]),
    )
    .abs();
    if cap_gap <= EPS_ROUND_CAP_GAP {
        return None;
    }
    let support_planes = support_ids
        .iter()
        .map(|id| reconciled_model_plane(&local_planes, ir, *id))
        .collect::<Option<Vec<_>>>()?;
    support_planes
        .iter()
        .all(|plane| {
            normalized(plane.normal).is_some_and(|normal| {
                dot(first_cap_normal, normal).abs() <= EPS_ROUND_SUPPORT_ORTHOGONAL
            })
        })
        .then_some(())?;
    parallel_support_radius(
        support_planes
            .into_iter()
            .map(|plane| (plane.origin, plane.normal)),
    )
}

pub(in super::super) fn round_support_envelope_cylinder(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
    envelope: Type24RoundEnvelope,
) -> Option<crate::surface::PositionalCylinderFrame> {
    let ([first_cap, second_cap], support_planes) =
        resolved_round_support_planes(scan, ir, feature_id)?;
    let axis = normalized(first_cap.normal)?;
    let second_cap_normal = normalized(second_cap.normal)?;
    if (dot(axis, second_cap_normal).abs() - 1.0).abs() > EPS_ROUND_CAP_PARALLEL {
        return None;
    }
    let cap_gap = dot(
        axis,
        std::array::from_fn(|index| second_cap.origin[index] - first_cap.origin[index]),
    )
    .abs();
    if cap_gap <= EPS_ROUND_CAP_GAP {
        return None;
    }

    let mut support_pairs = Vec::new();
    for first in 0..support_planes.len() {
        let first_normal = normalized(support_planes[first].normal)?;
        for second in first + 1..support_planes.len() {
            let second_normal = normalized(support_planes[second].normal)?;
            if (dot(first_normal, second_normal).abs() - 1.0).abs() > EPS_ROUND_CAP_PARALLEL {
                continue;
            }
            let gap = dot(
                first_normal,
                std::array::from_fn(|index| {
                    support_planes[second].origin[index] - support_planes[first].origin[index]
                }),
            )
            .abs();
            if gap <= EPS_ROUND_CAP_GAP {
                continue;
            }
            if dot(first_normal, axis).abs() > EPS_ROUND_SUPPORT_ORTHOGONAL {
                return None;
            }
            let first_offset = dot(first_normal, support_planes[first].origin);
            let second_offset = dot(first_normal, support_planes[second].origin);
            support_pairs.push((
                first_normal,
                0.5 * gap,
                0.5 * (first_offset + second_offset),
            ));
        }
    }
    let (support_normal, radius, support_midpoint) = *support_pairs.first()?;
    let scale = radius.max(cap_gap).max(1.0);
    support_pairs
        .iter()
        .all(|(normal, candidate_radius, candidate_midpoint)| {
            (candidate_radius - radius).abs() <= EPS_ROUND_RADIUS_RECONCILIATION * scale
                && (dot(*normal, support_normal).abs() - 1.0).abs() <= EPS_ROUND_CAP_PARALLEL
                && (candidate_midpoint - support_midpoint).abs()
                    <= EPS_ROUND_RADIUS_RECONCILIATION * scale
        })
        .then_some(())?;

    let [first_extent, second_extent] = envelope.extent_endpoints;
    let extent_delta =
        std::array::from_fn::<_, 3, _>(|index| second_extent[index] - first_extent[index]);
    let radial_span = dot(support_normal, extent_delta).abs();
    let axial_span = dot(axis, extent_delta).abs();
    ((radial_span - 2.0 * radius).abs() <= EPS_ROUND_RADIUS_RECONCILIATION * scale
        && (axial_span - cap_gap).abs() <= EPS_ROUND_RADIUS_RECONCILIATION * scale
        && radial_span > EPS_ROUND_CAP_GAP
        && axial_span > EPS_ROUND_CAP_GAP)
        .then_some(())?;

    let cap_residual = |point: [f64; 3], cap: PlaneEquation| {
        dot(
            axis,
            std::array::from_fn(|index| point[index] - cap.origin[index]),
        )
        .abs()
    };
    let first_on_first = cap_residual(first_extent, first_cap) <= EPS_ROUND_CAP_GAP * scale;
    let second_on_first = cap_residual(second_extent, first_cap) <= EPS_ROUND_CAP_GAP * scale;
    let first_on_second = cap_residual(first_extent, second_cap) <= EPS_ROUND_CAP_GAP * scale;
    let second_on_second = cap_residual(second_extent, second_cap) <= EPS_ROUND_CAP_GAP * scale;
    let start = match (
        first_on_first && second_on_second,
        second_on_first && first_on_second,
    ) {
        (true, false) => first_extent,
        (false, true) => second_extent,
        _ => return None,
    };
    let start_offset = dot(support_normal, start);
    let origin = std::array::from_fn(|index| {
        start[index] + support_normal[index] * (support_midpoint - start_offset)
    });
    Some(crate::surface::PositionalCylinderFrame {
        origin,
        axis,
        ref_direction: support_normal,
        radius,
        length: Some(cap_gap),
    })
}

fn resolved_round_support_planes(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> Option<([PlaneEquation; 2], Vec<PlaneEquation>)> {
    let affected_ids = agreed_feature_geometry_ids(
        &scan.features.affected_ids,
        &scan.features.replay_affected_ids,
        feature_id,
    )?;
    let [first_cap_id, second_cap_id, support_ids @ ..] = affected_ids else {
        return None;
    };
    if first_cap_id == second_cap_id {
        return None;
    }
    let local_planes = placed_planes(scan);
    let caps = [
        reconciled_model_plane(&local_planes, ir, *first_cap_id)?,
        reconciled_model_plane(&local_planes, ir, *second_cap_id)?,
    ];
    let first_cap_normal = normalized(caps[0].normal)?;
    let second_cap_normal = normalized(caps[1].normal)?;
    if (dot(first_cap_normal, second_cap_normal).abs() - 1.0).abs() > EPS_ROUND_CAP_PARALLEL {
        return None;
    }
    let cap_gap = dot(
        first_cap_normal,
        std::array::from_fn(|index| caps[1].origin[index] - caps[0].origin[index]),
    )
    .abs();
    if cap_gap <= EPS_ROUND_CAP_GAP {
        return None;
    }
    let support_planes = support_ids
        .iter()
        .filter_map(|id| reconciled_model_plane(&local_planes, ir, *id))
        .collect::<Vec<_>>();
    (support_planes.len() >= 2).then_some(())?;
    support_planes
        .iter()
        .all(|plane| {
            normalized(plane.normal).is_some_and(|normal| {
                dot(first_cap_normal, normal).abs() <= EPS_ROUND_SUPPORT_ORTHOGONAL
            })
        })
        .then_some(())?;
    Some((caps, support_planes))
}

pub(in super::super) fn round_placed_cylinder_radii(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> Vec<f64> {
    scan.surfaces
        .rows
        .iter()
        .filter(|row| {
            row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Cylinder
        })
        .filter_map(|row| round_placed_cylinder_radius(ir, row))
        .collect()
}

pub(in super::super) fn round_placed_cylinder_radius(
    ir: &CadIr,
    row: &crate::surface::SurfaceRow,
) -> Option<f64> {
    let id = SurfaceId(format!("creo:visibgeom:surface#{}", row.id));
    exactly_one(ir.model.surfaces.iter().filter(|surface| surface.id == id)).and_then(|surface| {
        match surface.geometry {
            SurfaceGeometry::Cylinder { radius, .. } => Some(radius),
            _ => None,
        }
    })
}

pub(in super::super) fn round_direct_radii(
    scan: &ContainerScan,
    feature_id: u32,
) -> Option<Vec<f64>> {
    let generated_rows = scan
        .surfaces
        .rows
        .iter()
        .filter(|row| row.feature_id == feature_id)
        .collect::<Vec<_>>();
    (!generated_rows.is_empty()).then_some(())?;
    let radii = round_observed_radii(scan, feature_id);
    (radii.len() == generated_rows.len()).then_some(radii)
}

pub(in super::super) fn round_observed_radii(scan: &ContainerScan, feature_id: u32) -> Vec<f64> {
    scan.surfaces
        .rows
        .iter()
        .filter(|row| row.feature_id == feature_id)
        .filter_map(|row| {
            let parameters = unique_surface_parameter_record(scan, row)?;
            match row.kind {
                crate::surface::SurfaceKind::Cylinder => {
                    parameters.type24_generated_round_radius(row.type_byte)
                }
                crate::surface::SurfaceKind::TorusOrSphere => parameters
                    .torus_radius_overrides(row.type_byte)
                    .map(|overrides| overrides.radius2)
                    .or_else(|| replayed_torus_minor_radius(scan, row, parameters)),
                _ => None,
            }
        })
        .collect()
}

pub(in super::super) fn differing_positive_lengths(values: &[f64]) -> bool {
    let Some(&first) = values.first() else {
        return false;
    };
    if values
        .iter()
        .any(|value| !value.is_finite() || *value <= 0.0)
    {
        return false;
    }
    let scale = values
        .iter()
        .copied()
        .map(f64::abs)
        .fold(first.abs().max(1.0), f64::max);
    values
        .iter()
        .any(|value| (*value - first).abs() > 1e-9 * scale)
}

pub(in super::super) fn unique_positive_length(values: &[f64]) -> Option<f64> {
    let value = *values.first()?;
    if !value.is_finite() || value <= 0.0 {
        return None;
    }
    let scale = values
        .iter()
        .copied()
        .map(f64::abs)
        .fold(value.abs().max(1.0), f64::max);
    values
        .iter()
        .all(|candidate| {
            candidate.is_finite() && *candidate > 0.0 && (*candidate - value).abs() <= 1e-9 * scale
        })
        .then_some(value)
}

pub(in super::super) fn equal_distance_chamfer_setback(
    cones: &[ConeEquation],
    support_planes: &[PlaneEquation],
) -> Option<f64> {
    (!cones.is_empty() && !support_planes.is_empty()).then_some(())?;
    let setbacks = cones
        .iter()
        .map(|cone| {
            let axis = normalized(cone.axis)?;
            (circular_cone(*cone)
                && cone.radius.abs() <= 1e-12
                && (cone.half_angle - std::f64::consts::FRAC_PI_4).abs() <= 1e-10)
                .then_some(())?;
            support_planes
                .iter()
                .filter_map(|plane| {
                    let normal = normalized(plane.normal)?;
                    let denominator = dot(axis, normal);
                    (denominator.abs() >= 1.0 - 1e-10).then_some(())?;
                    let displacement = [
                        plane.origin[0] - cone.origin[0],
                        plane.origin[1] - cone.origin[1],
                        plane.origin[2] - cone.origin[2],
                    ];
                    let setback = dot(displacement, normal) / denominator;
                    (setback.is_finite() && setback > 1e-12).then_some(setback)
                })
                .min_by(f64::total_cmp)
        })
        .collect::<Option<Vec<_>>>()?;
    unique_positive_length(&setbacks)
}

fn chamfer_cone_equation(
    scan: &ContainerScan,
    ir: &CadIr,
    row: &crate::surface::SurfaceRow,
) -> Option<ConeEquation> {
    let parameter_records = scan
        .surfaces
        .parameters
        .iter()
        .filter(|record| record.offset == row.offset)
        .collect::<Vec<_>>();
    if parameter_records.len() > 1 {
        return None;
    }
    if let Some(frame) = parameter_records
        .first()
        .and_then(|record| record.positional_cone_frame)
    {
        return Some(ConeEquation {
            origin: frame.apex,
            axis: frame.axis,
            ref_direction: frame.ref_direction,
            radius: 0.0,
            ratio: 1.0,
            half_angle: frame.half_angle,
        });
    }
    let id = SurfaceId(format!("creo:visibgeom:surface#{}", row.id));
    let surface = exactly_one(ir.model.surfaces.iter().filter(|surface| surface.id == id))?;
    let SurfaceGeometry::Cone {
        origin,
        axis,
        ref_direction,
        radius,
        ratio,
        half_angle,
    } = &surface.geometry
    else {
        return None;
    };
    Some(ConeEquation {
        origin: [origin.x, origin.y, origin.z],
        axis: [axis.x, axis.y, axis.z],
        ref_direction: [ref_direction.x, ref_direction.y, ref_direction.z],
        radius: *radius,
        ratio: *ratio,
        half_angle: *half_angle,
    })
}

pub(in super::super) fn chamfer_constant_distance(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> Option<f64> {
    let rows = scan
        .surfaces
        .rows
        .iter()
        .filter(|row| row.feature_id == feature_id)
        .collect::<Vec<_>>();
    (!rows.is_empty()
        && rows
            .iter()
            .all(|row| row.kind == crate::surface::SurfaceKind::Cone))
    .then_some(())?;
    let cones = rows
        .iter()
        .map(|row| chamfer_cone_equation(scan, ir, row))
        .collect::<Option<Vec<_>>>()?;
    let affected_ids = agreed_feature_geometry_ids(
        &scan.features.affected_ids,
        &scan.features.replay_affected_ids,
        feature_id,
    )?;
    let local_planes = placed_planes(scan);
    let mut support_planes = Vec::new();
    let mut support_plane_ids = BTreeSet::new();
    for id in affected_ids {
        let rows = scan
            .surfaces
            .rows
            .iter()
            .filter(|row| row.id == *id)
            .collect::<Vec<_>>();
        let is_support_plane = match rows.as_slice() {
            [] => {
                let model_id = SurfaceId(format!("creo:visibgeom:surface#{id}"));
                let model_surfaces = ir
                    .model
                    .surfaces
                    .iter()
                    .filter(|surface| surface.id == model_id)
                    .collect::<Vec<_>>();
                match model_surfaces.as_slice() {
                    [] => false,
                    [surface] => matches!(&surface.geometry, SurfaceGeometry::Plane { .. }),
                    _ => return None,
                }
            }
            [row] => row.kind == crate::surface::SurfaceKind::Plane,
            _ if rows
                .iter()
                .any(|row| row.kind == crate::surface::SurfaceKind::Plane) =>
            {
                return None;
            }
            _ => continue,
        };
        if !is_support_plane || !support_plane_ids.insert(*id) {
            continue;
        }
        let plane = reconciled_model_plane(&local_planes, ir, *id)?;
        support_planes.push(plane);
    }
    equal_distance_chamfer_setback(&cones, &support_planes)
}

#[cfg(test)]
mod tests;
