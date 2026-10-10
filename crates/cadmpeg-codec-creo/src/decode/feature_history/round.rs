// SPDX-License-Identifier: Apache-2.0
//! Round and chamfer radius reconstruction from support geometry.

use super::super::surfaces::prototypes::{prototype_scalar, unique_surface_prototype_associations};
use super::selections::agreed_feature_geometry_ids;
use crate::container::ContainerScan;
use crate::decode::analytic::equations::{
    circular_cone, solve_planes, ConeEquation, CylinderEquation, PlaneEquation,
};
use crate::decode::analytic::planes::{placed_planes, reconciled_model_plane};
use crate::legacy_feature::LegacyRoundRadius;
use crate::surface::{SurfaceParameterRecord, Type24RoundEnvelope};
use crate::vecmath::normalize;
use crate::vecmath::{cross, dot};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::scalar::{PositiveLength, PositiveReal};
use std::collections::BTreeSet;

const EPS_CYLINDER_FIT: f64 = 1.0e-8;
const EPS_GEOMETRY_AGREEMENT: f64 = 1.0e-9;
const EPS_NORMAL_ALIGNMENT: f64 = 1.0e-10;
const EPS_RADIUS_NONZERO: f64 = 1.0e-12;
const EPS_CONE_ANGLE: f64 = 1.0e-10;
const EPS_DENOMINATOR_ALIGNMENT: f64 = 1.0e-10;
const EPS_SETBACK_NONZERO: f64 = 1.0e-12;
const EPS_ROUND_CAP_GAP: f64 = 1.0e-9;
const EPS_ROUND_CAP_PARALLEL: f64 = 1.0e-10;
const EPS_ROUND_RADIUS_RECONCILIATION: f64 = 1.0e-9;
const EPS_ROUND_SUPPORT_ORTHOGONAL: f64 = 1.0e-9;

#[cfg(test)]
pub(in super::super) fn parallel_support_radius<T>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    sources: &[T],
    mut resolve_plane: impl FnMut(&T) -> Result<Option<PlaneEquation>, cadmpeg_core::CodecError>,
) -> Result<Option<f64>, cadmpeg_core::CodecError> {
    if sources.len() < 2 {
        return Ok(None);
    }
    let mut scratch = ctx.reserve_scoped(0, "creo resolved support planes")?;
    let mut planes = Vec::new();
    let mut source_rows = sources.iter();
    while let Some(source) =
        ctx.next_charged(&mut source_rows, "creo round support plane resolution")?
    {
        let Some(plane) = resolve_plane(source)? else {
            return Ok(None);
        };
        let Some(normal) = normalize(plane.normal) else {
            return Ok(None);
        };
        scratch.with_storage(|| {
            ctx.push_vec(
                &mut planes,
                (plane, normal),
                "creo resolved support plane rows",
            )
        })?;
    }
    parallel_plane_radius(ctx, &planes)
}

fn parallel_plane_radius(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    planes: &[(PlaneEquation, [f64; 3])],
) -> Result<Option<f64>, cadmpeg_core::CodecError> {
    let mut first_radius: Option<f64> = None;
    let mut pairs = planes.iter().enumerate();
    while let Some((first_index, (first_plane, first_normal))) =
        ctx.next_charged(&mut pairs, "creo round support plane pairs")?
    {
        let mut partners = planes[first_index + 1..].iter();
        while let Some((second_plane, second_normal)) =
            ctx.next_charged(&mut partners, "creo round support plane pairs")?
        {
            let alignment = first_normal
                .iter()
                .zip(*second_normal)
                .map(|(first, second)| first * second)
                .sum::<f64>();
            if alignment.abs() < 1.0 - EPS_GEOMETRY_AGREEMENT {
                continue;
            }
            let gap = second_plane
                .origin
                .iter()
                .zip(first_plane.origin)
                .zip(*first_normal)
                .map(|((second, first), normal)| (second - first) * normal)
                .sum::<f64>()
                .abs();
            let scale = first_plane
                .origin
                .iter()
                .chain(&second_plane.origin)
                .map(|value| value.abs())
                .fold(1.0, f64::max);
            if gap > EPS_GEOMETRY_AGREEMENT * scale {
                let candidate = 0.5 * gap;
                if let Some(radius) = first_radius {
                    if (candidate - radius).abs() > EPS_GEOMETRY_AGREEMENT * radius.abs().max(1.0) {
                        return Ok(None);
                    }
                } else {
                    first_radius = Some(candidate);
                }
            }
        }
    }
    Ok(first_radius.filter(|radius| radius.is_finite()))
}

pub(in super::super) fn slot_fillet_cylinder(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    cap_planes: [PlaneEquation; 2],
    support_planes: &[PlaneEquation],
) -> Result<Option<CylinderEquation>, cadmpeg_core::CodecError> {
    let Some(axis) = normalize(cap_planes[0].normal) else {
        return Ok(None);
    };
    let Some(second_cap_normal) = normalize(cap_planes[1].normal) else {
        return Ok(None);
    };
    if (dot(axis, second_cap_normal).abs() - 1.0).abs() > EPS_NORMAL_ALIGNMENT {
        return Ok(None);
    }
    let cap_gap = dot(
        axis,
        std::array::from_fn(|index| cap_planes[1].origin[index] - cap_planes[0].origin[index]),
    )
    .abs();
    if cap_gap <= EPS_GEOMETRY_AGREEMENT {
        return Ok(None);
    }
    let mut scratch = ctx.reserve_scoped(0, "creo slot fillet evidence")?;
    let mut midplanes = Vec::<(PlaneEquation, f64)>::new();
    let mut items = support_planes.iter().enumerate();
    while let Some((first_index, first_plane)) =
        ctx.next_charged(&mut items, "creo slot fillet support plane pairs")?
    {
        let Some(first_normal) = normalize(first_plane.normal) else {
            return Ok(None);
        };
        if dot(first_normal, axis).abs() > EPS_GEOMETRY_AGREEMENT {
            return Ok(None);
        }
        let mut second_plane_iter = support_planes[first_index + 1..].iter();
        while let Some(second_plane) = ctx.next_charged(
            &mut second_plane_iter,
            "creo slot fillet support plane pairs",
        )? {
            let Some(second_normal) = normalize(second_plane.normal) else {
                return Ok(None);
            };
            if (dot(first_normal, second_normal).abs() - 1.0).abs() > EPS_NORMAL_ALIGNMENT {
                continue;
            }
            let gap = dot(
                first_normal,
                std::array::from_fn(|index| second_plane.origin[index] - first_plane.origin[index]),
            )
            .abs();
            if gap <= EPS_GEOMETRY_AGREEMENT {
                continue;
            }
            scratch.with_storage(|| {
                ctx.reserve_vec(&mut midplanes, 1, "creo slot fillet midplanes")
            })?;
            midplanes.push((
                PlaneEquation {
                    origin: std::array::from_fn(|index| {
                        0.5 * (first_plane.origin[index] + second_plane.origin[index])
                    }),
                    normal: first_normal,
                },
                0.5 * gap,
            ));
        }
    }
    let mut first_candidate: Option<CylinderEquation> = None;
    let mut items = midplanes.iter().enumerate();
    while let Some((first_index, first)) =
        ctx.next_charged(&mut items, "creo slot fillet midplane pairs")?
    {
        let mut second_iter = midplanes[first_index + 1..].iter();
        while let Some(second) =
            ctx.next_charged(&mut second_iter, "creo slot fillet midplane pairs")?
        {
            let radius = first.1;
            let scale = radius.max(second.1);
            if (second.1 - radius).abs() > EPS_GEOMETRY_AGREEMENT * scale
                || dot(first.0.normal, second.0.normal).abs() > 1.0 - EPS_GEOMETRY_AGREEMENT
            {
                continue;
            }
            let Some(origin) = solve_planes(ctx, &[cap_planes[0], first.0, second.0])? else {
                continue;
            };
            let tangent_to_all = ctx.all_by(
                support_planes,
                |plane| {
                    let Some(normal) = normalize(plane.normal) else {
                        return Ok(false);
                    };
                    let distance = dot(
                        normal,
                        std::array::from_fn(|index| origin[index] - plane.origin[index]),
                    )
                    .abs();
                    Ok((distance - radius).abs() <= EPS_CYLINDER_FIT * scale)
                },
                "creo slot fillet support planes",
            )?;
            if tangent_to_all {
                let candidate = CylinderEquation {
                    origin,
                    axis,
                    ref_direction: first.0.normal,
                    radius,
                };
                if let Some(first) = first_candidate {
                    let scale = first.radius;
                    let origin_delta: [f64; 3] =
                        std::array::from_fn(|index| candidate.origin[index] - first.origin[index]);
                    if !((candidate.radius - first.radius).abs() <= EPS_GEOMETRY_AGREEMENT * scale
                        && (dot(candidate.axis, first.axis).abs() - 1.0).abs()
                            <= EPS_NORMAL_ALIGNMENT
                        && dot(
                            cross(origin_delta, first.axis),
                            cross(origin_delta, first.axis),
                        )
                        .sqrt()
                            <= EPS_CYLINDER_FIT * scale)
                    {
                        return Ok(None);
                    }
                } else {
                    first_candidate = Some(candidate);
                }
            }
        }
    }
    let Some(first) = first_candidate else {
        return Ok(None);
    };
    let finite_origin = first.origin.iter().all(|value| value.is_finite());
    Ok((first.radius.is_finite()
        && finite_origin
        && (dot(first.axis, first.axis).abs() - 1.0).abs() <= EPS_NORMAL_ALIGNMENT)
        .then_some(first))
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
        .filter(|(first, second)| {
            ((*second - *first).abs() - radius).abs() <= EPS_GEOMETRY_AGREEMENT * scale
        })
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
    let close = |left: f64, right: f64| (left - right).abs() <= EPS_GEOMETRY_AGREEMENT * scale;
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
    let close = |left: f64, right: f64| (left - right).abs() <= EPS_GEOMETRY_AGREEMENT * scale;
    close(a1, b0)
        && coordinate_pair_proves_torus_radii([a1, a2], [b1, b2], major_radius, minor_radius)
}

pub(in super::super) fn paired_five_coordinate_sphere_center(
    envelopes: [crate::surface::Type26FiveCoordinateEnvelope; 2],
    radius: f64,
) -> Option<[f64; 3]> {
    if !(radius.is_finite() && radius > 0.0) {
        return None;
    }
    let mut scale = radius.max(1.0);
    for envelope in &envelopes {
        for value in &envelope.values {
            scale = scale.max(value.abs());
        }
    }
    let mut decoded = [None, None];
    for (index, envelope) in envelopes.iter().enumerate() {
        let [x_min, z0, y_min, radial_max, z1] = envelope.values;
        let close = |left: f64, right: f64| (left - right).abs() <= EPS_GEOMETRY_AGREEMENT * scale;
        decoded[index] = (close(x_min, y_min)
            && close(radial_max - x_min, 2.0 * radius)
            && close((z1 - z0).abs(), radius))
        .then_some(([x_min, radial_max], [z0, z1]));
    }
    let close = |left: f64, right: f64| (left - right).abs() <= EPS_GEOMETRY_AGREEMENT * scale;
    let [Some((first_radial, first_axial)), Some((second_radial, second_axial))] = decoded else {
        return None;
    };
    if !(close(first_radial[0], second_radial[0]) && close(first_radial[1], second_radial[1])) {
        return None;
    }
    let candidates = [first_axial[0], first_axial[1]];
    let other_axial = [second_axial[0], second_axial[1]];
    let mut center_z = None;
    for candidate in &candidates {
        if other_axial.iter().any(|other| close(*candidate, *other))
            && center_z.replace(*candidate).is_some()
        {
            return None;
        }
    }
    let center_z = center_z?;
    let axial_values = [
        first_axial[0],
        first_axial[1],
        second_axial[0],
        second_axial[1],
    ];
    let axial_min = axial_values.iter().copied().fold(f64::INFINITY, f64::min);
    let axial_max = axial_values
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    (close(axial_max - axial_min, 2.0 * radius)
        && close(center_z - axial_min, radius)
        && close(axial_max - center_z, radius))
    .then_some([
        0.5 * (first_radial[0] + first_radial[1]),
        0.5 * (first_radial[0] + first_radial[1]),
        center_z,
    ])
}

pub(in super::super) fn unique_surface_parameter_record<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &'a ContainerScan,
    row: &crate::surface::SurfaceRow,
) -> Result<Option<&'a crate::surface::SurfaceParameterRecord>, cadmpeg_core::CodecError> {
    crate::decode::uniqueness::exactly_one_by(
        ctx,
        &scan.surfaces.parameters,
        |record| Ok(record.offset == row.offset),
        "creo surface parameter records",
    )
}

fn unique_section_torus_minor_radius(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    row: &crate::surface::SurfaceRow,
) -> Result<Option<f64>, cadmpeg_core::CodecError> {
    let Some(section) = ctx.find_by(
        &scan.framing.sections,
        |section| Ok(section.contains(row.offset)),
        "creo torus section lookup",
    )?
    else {
        return Ok(None);
    };
    let prototype = crate::decode::uniqueness::exactly_one_by(
        ctx,
        &scan.surfaces.prototype_records,
        |prototype| {
            Ok(matches!(
                prototype.family,
                crate::surface::SurfacePrototypeFamily::Torus(_)
            ) && section.contains(prototype.offset))
        },
        "creo torus prototype records",
    )?;
    let radius = match prototype {
        Some(prototype) => prototype_scalar(ctx, prototype, "radius2")?,
        None => None,
    };
    Ok(radius.filter(|radius| radius.is_finite() && *radius > 0.0))
}

pub(in super::super) fn replayed_torus_minor_radius(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    row: &crate::surface::SurfaceRow,
    record: &crate::surface::SurfaceParameterRecord,
) -> Result<Option<f64>, cadmpeg_core::CodecError> {
    let Some(prototype_minor_radius) = unique_section_torus_minor_radius(ctx, scan, row)? else {
        return Ok(None);
    };
    record.type26_replayed_minor_radius_checked(ctx, prototype_minor_radius)
}

fn prototype_round_radius(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    rows: &[&crate::surface::SurfaceRow],
) -> Result<Option<f64>, cadmpeg_core::CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo round radius evidence")?;
    let Some(first) = rows.first() else {
        return Ok(None);
    };
    let feature_id = first.feature_id;
    let associations = scratch.with_storage(|| unique_surface_prototype_associations(ctx, scan))?;
    let mut radii = None;
    let mut items = associations.iter();
    while let Some((record, row, _)) =
        ctx.next_charged(&mut items, "creo round prototype associations")?
    {
        if !matches!(
            record.record().family,
            crate::surface::SurfacePrototypeFamily::Torus(_)
        ) || row.feature_id != feature_id
        {
            continue;
        }
        let matched_row = ctx.any_by(
            rows,
            |candidate| Ok(candidate.offset == row.offset),
            "creo round prototype surface rows",
        )?;
        if !matched_row {
            continue;
        }
        let Some(radius1) = prototype_scalar(ctx, record.record(), "radius1")? else {
            continue;
        };
        let Some(radius2) = prototype_scalar(ctx, record.record(), "radius2")? else {
            continue;
        };
        if radii.replace((radius1, radius2)).is_some() {
            return Ok(None);
        }
    }
    let Some((radius1, radius2)) = radii else {
        return Ok(None);
    };
    if !(radius1.is_finite() && radius1 >= 0.0 && radius2.is_finite() && radius2 > 0.0) {
        return Ok(None);
    }
    let mut row_iter = rows.iter();
    while let Some(row) = ctx.next_charged(&mut row_iter, "creo round prototype surface rows")? {
        let Some(record) = unique_surface_parameter_record(ctx, scan, row)? else {
            return Ok(None);
        };
        if record.torus_radius_overrides_checked(ctx)?.is_some() {
            return Ok(None);
        }
        let replayed_matches = replayed_torus_minor_radius(ctx, scan, row, record)?
            .is_some_and(|radius| radius.to_bits() == radius2.to_bits());
        let outline_matches = if replayed_matches {
            false
        } else if let Some(frame) = record.torus_outline_frame_checked(ctx)? {
            outline_has_unique_radius_delta(frame, radius2)
        } else {
            false
        };
        let five_coordinate_matches = if replayed_matches || outline_matches {
            false
        } else {
            record
                .type26_five_coordinate_envelope()
                .is_some_and(|envelope| {
                    five_coordinate_envelope_proves_torus_radii(envelope, radius1, radius2)
                })
        };
        let split_coordinate_matches =
            if replayed_matches || outline_matches || five_coordinate_matches {
                false
            } else {
                record
                    .type26_split_coordinate_envelope()
                    .is_some_and(|envelope| {
                        let [a1, a2, b1, b2] = envelope.values;
                        coordinate_pair_proves_torus_radii([a1, a2], [b1, b2], radius1, radius2)
                    })
            };
        if !(replayed_matches
            || outline_matches
            || five_coordinate_matches
            || split_coordinate_matches)
        {
            return Ok(None);
        }
    }
    Ok(Some(radius2))
}

pub(in super::super) fn round_constant_radius(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    feature_id: u32,
) -> Result<Option<f64>, cadmpeg_core::CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo round radius evidence")?;
    let legacy_radius = ctx
        .find_by(
            &scan.features.legacy_rounds,
            |round| Ok(round.feature_id == feature_id),
            "creo legacy round records",
        )?
        .map(|round| round.radius);
    match legacy_radius {
        Some(LegacyRoundRadius::Constant(radius)) => {
            if !legacy_round_radius_agrees(ctx, scan, ir, source_carriers, feature_id, radius)? {
                return Ok(None);
            }
            return Ok(Some(radius.get()));
        }
        Some(LegacyRoundRadius::Ambiguous) => return Ok(None),
        Some(LegacyRoundRadius::NotPresent) | None => {}
    }
    let (generated_count, observed_radii) =
        scratch.with_storage(|| round_observed_radius_rows(ctx, scan, feature_id))?;
    let direct_radius = if generated_count > 0 && observed_radii.len() == generated_count {
        unique_positive_length(ctx, &observed_radii)?
    } else {
        None
    };
    if let Some(radius) = direct_radius {
        if complete_direct_placed_cylinder_radius_agreement(
            ctx,
            scan,
            ir,
            source_carriers,
            feature_id,
        )?
        .is_some_and(|agrees| !agrees)
        {
            return Ok(None);
        }
        return Ok(Some(radius.get()));
    }
    if generated_count == 0 {
        return round_support_radius(ctx, scan, ir, source_carriers, feature_id);
    }
    if let Some(radius) =
        round_replay_radius(ctx, scan, ir, source_carriers, feature_id, &observed_radii)?
    {
        return Ok(Some(radius));
    }
    // Unequal decoded rolling-radius samples identify a variable-radius
    // round even when another generated row has no radius proof. A support
    // plane fallback must not turn that incomplete, unequal sample set into
    // a false constant radius.
    if differing_positive_lengths(ctx, &observed_radii)? {
        return Ok(None);
    }
    let mut generated_rows = Vec::new();
    let mut cylinder_count = 0;
    let mut only_cylinders_and_tori = true;
    let mut only_cylinders_and_planes = true;
    for row in ctx
        .admit_iter(&*scan.surfaces.rows, "creo generated round surface rows")?
        .filter(|row| row.feature_id == feature_id)
    {
        cylinder_count += usize::from(row.kind == crate::surface::SurfaceKind::Cylinder);
        only_cylinders_and_tori &= matches!(
            row.kind,
            crate::surface::SurfaceKind::Cylinder | crate::surface::SurfaceKind::TorusOrSphere
        );
        only_cylinders_and_planes &= matches!(
            row.kind,
            crate::surface::SurfaceKind::Cylinder | crate::surface::SurfaceKind::Plane
        );
        scratch.with_storage(|| {
            ctx.reserve_vec(&mut generated_rows, 1, "creo generated round rows")
        })?;
        generated_rows.push(row);
    }
    if cylinder_count == 0 {
        if !only_cylinders_and_tori {
            return Ok(None);
        }
        return prototype_round_radius(ctx, scan, &generated_rows);
    }
    if cylinder_count != generated_rows.len() && only_cylinders_and_tori {
        if let Some(radii) = scratch.with_storage(|| {
            mixed_round_radius_samples(ctx, scan, ir, source_carriers, &generated_rows)
        })? {
            return Ok(unique_positive_length(ctx, &radii)?.map(PositiveLength::get));
        }
    }
    let cylinder_radii = scratch
        .with_storage(|| round_placed_cylinder_radii(ctx, scan, ir, source_carriers, feature_id))?;
    if differing_positive_lengths(ctx, &cylinder_radii)? {
        // Independent placed cylinder samples remain decisive when an
        // unresolved toroidal sibling prevents the complete mixed-family
        // witness from being assembled.
        return Ok(None);
    }
    // A complete placed set of generated cylinder carriers is an independent
    // radius witness when the remaining generated rows are cap or support
    // planes. A toroidal or other rolling carrier still needs its own family
    // proof, so it must not be hidden by the cylinder subset.
    if cylinder_radii.len() == cylinder_count && only_cylinders_and_planes {
        return Ok(unique_positive_length(ctx, &cylinder_radii)?.map(PositiveLength::get));
    }
    round_support_radius(ctx, scan, ir, source_carriers, feature_id)
}

fn round_replay_radius(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    feature_id: u32,
    observed: &[f64],
) -> Result<Option<f64>, cadmpeg_core::CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo round radius evidence")?;
    let placed = scratch
        .with_storage(|| round_placed_cylinder_radii(ctx, scan, ir, source_carriers, feature_id))?;
    let Some((radius, scale, difference)) = positive_length_spread(ctx, observed, &placed)? else {
        return Ok(None);
    };
    if difference > EPS_GEOMETRY_AGREEMENT * scale {
        return Ok(None);
    }
    let radius = radius.get();
    let scale = radius.abs().max(1.0);
    Ok(ctx
        .any_by(
            &scan.features.round_replay_scalars,
            |candidate| {
                Ok(candidate.feature_id == feature_id
                    && candidate.value.get() > 0.0
                    && (candidate.value.get() - radius).abs()
                        <= EPS_ROUND_RADIUS_RECONCILIATION * scale)
            },
            "creo round replay scalar records",
        )?
        .then_some(radius))
}

fn legacy_round_radius_agrees(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    feature_id: u32,
    radius: PositiveReal,
) -> Result<bool, cadmpeg_core::CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo round radius evidence")?;
    let radius = radius.get();
    let samples = scratch.with_storage(|| round_observed_radii(ctx, scan, feature_id))?;
    let placed = scratch
        .with_storage(|| round_placed_cylinder_radii(ctx, scan, ir, source_carriers, feature_id))?;
    let mut scale = radius.abs().max(1.0);
    let mut difference: f64 = 0.0;
    let mut samples = samples.iter().chain(&placed);
    while let Some(&sample) = ctx.next_charged(&mut samples, "creo legacy round radius samples")? {
        if !sample.is_finite() || sample <= 0.0 {
            return Ok(false);
        }
        scale = scale.max(sample);
        difference = difference.max((sample - radius).abs());
    }
    Ok(difference <= EPS_ROUND_RADIUS_RECONCILIATION * scale)
}

fn complete_direct_placed_cylinder_radius_agreement(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    feature_id: u32,
) -> Result<Option<bool>, cadmpeg_core::CodecError> {
    let mut agrees = true;
    let mut row_iter = scan.surfaces.rows.iter();
    while let Some(row) =
        ctx.next_charged(&mut row_iter, "creo direct placed round surface rows")?
    {
        if row.feature_id != feature_id || row.kind != crate::surface::SurfaceKind::Cylinder {
            continue;
        }
        let Some(direct) = unique_surface_parameter_record(ctx, scan, row)?
            .and_then(SurfaceParameterRecord::type24_generated_round_radius)
        else {
            return Ok(None);
        };
        let Some(placed) = round_placed_cylinder_radius(ctx, ir, row, source_carriers)? else {
            return Ok(None);
        };
        let scale = direct.abs().max(placed.abs()).max(1.0);
        agrees &= direct.is_finite()
            && direct > 0.0
            && placed.is_finite()
            && placed > 0.0
            && (direct - placed).abs() <= EPS_ROUND_RADIUS_RECONCILIATION * scale;
    }
    Ok(Some(agrees))
}

fn mixed_round_radius_samples(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    rows: &[&crate::surface::SurfaceRow],
) -> Result<Option<Vec<f64>>, cadmpeg_core::CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo round radius evidence")?;
    let mut torus_rows = Vec::new();
    for row in ctx
        .admit_iter(rows, "creo mixed round surface rows")?
        .copied()
        .filter(|row| row.kind == crate::surface::SurfaceKind::TorusOrSphere)
    {
        scratch.with_storage(|| ctx.reserve_vec(&mut torus_rows, 1, "creo mixed torus rows"))?;
        torus_rows.push(row);
    }
    if torus_rows.is_empty() || torus_rows.len() == rows.len() {
        return Ok(None);
    }

    let mut cylinder_radii = Vec::new();
    let mut row_iter = rows.iter().copied();
    while let Some(row) = ctx.next_charged(&mut row_iter, "creo mixed round surface rows")? {
        if row.kind != crate::surface::SurfaceKind::Cylinder {
            continue;
        }
        let Some(radius) = round_cylinder_radius(ctx, scan, ir, source_carriers, row)? else {
            return Ok(None);
        };
        ctx.reserve_vec(&mut cylinder_radii, 1, "creo mixed cylinder radii")?;
        cylinder_radii.push(radius);
    }
    let Some(torus_radii) = mixed_torus_radius_samples(ctx, scan, &torus_rows)? else {
        return Ok(None);
    };
    ctx.extend_vec(&mut cylinder_radii, torus_radii, "creo mixed round samples")?;
    Ok(Some(cylinder_radii))
}

fn mixed_torus_radius_samples(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    rows: &[&crate::surface::SurfaceRow],
) -> Result<Option<Vec<f64>>, cadmpeg_core::CodecError> {
    let mut all_overrides = true;
    let mut any_overrides = false;
    let mut row_iter = rows.iter();
    while let Some(row) = ctx.next_charged(&mut row_iter, "creo mixed torus surface rows")? {
        let Some(record) = unique_surface_parameter_record(ctx, scan, row)? else {
            return Ok(None);
        };
        let overridden = record.torus_radius_overrides_checked(ctx)?.is_some();
        all_overrides &= overridden;
        any_overrides |= overridden;
    }
    if all_overrides {
        let mut radii = Vec::new();
        let mut row_iter = rows.iter();
        while let Some(row) = ctx.next_charged(&mut row_iter, "creo mixed torus surface rows")? {
            let Some(record) = unique_surface_parameter_record(ctx, scan, row)? else {
                return Ok(None);
            };
            let Some(overrides) = record.torus_radius_overrides_checked(ctx)? else {
                return Ok(None);
            };
            ctx.reserve_vec(&mut radii, 1, "creo torus override samples")?;
            radii.push(overrides.radius2);
        }
        return Ok(Some(radii));
    }
    if any_overrides {
        return Ok(None);
    }
    match prototype_round_radius(ctx, scan, rows)? {
        Some(radius) => Ok(Some(ctx.alloc_filled(
            rows.len(),
            radius,
            "creo_torus_radius_samples",
        )?)),
        None => Ok(None),
    }
}

fn round_cylinder_radius(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    row: &crate::surface::SurfaceRow,
) -> Result<Option<f64>, cadmpeg_core::CodecError> {
    match unique_surface_parameter_record(ctx, scan, row)?
        .and_then(SurfaceParameterRecord::type24_generated_round_radius)
    {
        Some(radius) => Ok(Some(radius)),
        None => round_placed_cylinder_radius(ctx, ir, row, source_carriers),
    }
}

pub(in super::super) fn round_support_radius(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    feature_id: u32,
) -> Result<Option<f64>, cadmpeg_core::CodecError> {
    let Some(affected_ids) = agreed_feature_geometry_ids(
        ctx,
        &scan.features.affected_ids,
        &scan.features.replay_affected_ids,
        feature_id,
    )?
    else {
        return Ok(None);
    };
    let [first_cap_id, second_cap_id, support_ids @ ..] = affected_ids else {
        return Ok(None);
    };
    if first_cap_id == second_cap_id {
        return Ok(None);
    }
    let mut scratch = ctx.reserve_scoped(0, "creo round support evidence")?;
    let local_planes = scratch.with_storage(|| placed_planes(ctx, scan))?;
    let mut planes = Vec::new();
    let Some(first_cap) =
        reconciled_model_plane(ctx, &local_planes, ir, source_carriers, *first_cap_id)?
    else {
        return Ok(None);
    };
    let Some(second_cap) =
        reconciled_model_plane(ctx, &local_planes, ir, source_carriers, *second_cap_id)?
    else {
        return Ok(None);
    };
    let Some(first_cap_normal) = normalize(first_cap.normal) else {
        return Ok(None);
    };
    let Some(second_cap_normal) = normalize(second_cap.normal) else {
        return Ok(None);
    };
    if (dot(first_cap_normal, second_cap_normal).abs() - 1.0).abs() > EPS_ROUND_CAP_PARALLEL {
        return Ok(None);
    }
    let cap_gap = dot(
        first_cap_normal,
        std::array::from_fn(|index| second_cap.origin[index] - first_cap.origin[index]),
    )
    .abs();
    if cap_gap <= EPS_ROUND_CAP_GAP {
        return Ok(None);
    }
    let mut id_iter = support_ids.iter();
    while let Some(id) = ctx.next_charged(&mut id_iter, "creo round support plane IDs")? {
        let Some(plane) = reconciled_model_plane(ctx, &local_planes, ir, source_carriers, *id)?
        else {
            return Ok(None);
        };
        let Some(normal) = normalize(plane.normal) else {
            return Ok(None);
        };
        if !crate::vecmath::within(
            dot(first_cap_normal, normal).abs(),
            EPS_ROUND_SUPPORT_ORTHOGONAL,
        ) {
            return Ok(None);
        }
        scratch.with_storage(|| {
            ctx.push_vec(
                &mut planes,
                (plane, normal),
                "creo validated support plane rows",
            )
        })?;
    }
    parallel_plane_radius(ctx, &planes)
}

pub(in super::super) fn round_support_envelope_cylinder(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    feature_id: u32,
    envelope: Type24RoundEnvelope,
) -> Result<Option<crate::surface::PositionalCylinderFrame>, cadmpeg_core::CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo round envelope support evidence")?;
    let Some(RoundSupportPlanes {
        caps: [first_cap, second_cap],
        support_planes,
    }) = scratch.with_storage(|| {
        resolved_round_support_planes(ctx, scan, ir, source_carriers, feature_id)
    })?
    else {
        return Ok(None);
    };
    let Some(axis) = normalize(first_cap.normal) else {
        return Ok(None);
    };
    let Some(second_cap_normal) = normalize(second_cap.normal) else {
        return Ok(None);
    };
    if (dot(axis, second_cap_normal).abs() - 1.0).abs() > EPS_ROUND_CAP_PARALLEL {
        return Ok(None);
    }
    let cap_gap = dot(
        axis,
        std::array::from_fn(|index| second_cap.origin[index] - first_cap.origin[index]),
    )
    .abs();
    if cap_gap <= EPS_ROUND_CAP_GAP {
        return Ok(None);
    }

    let mut agreed_pair: Option<([f64; 3], f64, f64)> = None;
    let mut pairs = support_planes.iter().enumerate();
    while let Some((first_index, (first, first_normal))) =
        ctx.next_charged(&mut pairs, "creo round support plane pairs")?
    {
        let first_normal = *first_normal;
        let mut partners = support_planes[first_index + 1..].iter();
        while let Some((second, second_normal)) =
            ctx.next_charged(&mut partners, "creo round support plane pairs")?
        {
            let second_normal = *second_normal;
            if (dot(first_normal, second_normal).abs() - 1.0).abs() > EPS_ROUND_CAP_PARALLEL {
                continue;
            }
            let gap = dot(
                first_normal,
                std::array::from_fn(|index| second.origin[index] - first.origin[index]),
            )
            .abs();
            if gap <= EPS_ROUND_CAP_GAP {
                continue;
            }
            if dot(first_normal, axis).abs() > EPS_ROUND_SUPPORT_ORTHOGONAL {
                return Ok(None);
            }
            let first_offset = dot(first_normal, first.origin);
            let second_offset = dot(first_normal, second.origin);
            let pair = (
                first_normal,
                0.5 * gap,
                0.5 * (first_offset + second_offset),
            );
            if let Some((support_normal, radius, support_midpoint)) = agreed_pair {
                let scale = radius.max(cap_gap).max(1.0);
                if !((pair.1 - radius).abs() <= EPS_ROUND_RADIUS_RECONCILIATION * scale
                    && (dot(pair.0, support_normal).abs() - 1.0).abs() <= EPS_ROUND_CAP_PARALLEL
                    && (pair.2 - support_midpoint).abs() <= EPS_ROUND_RADIUS_RECONCILIATION * scale)
                {
                    return Ok(None);
                }
            } else {
                agreed_pair = Some(pair);
            }
        }
    }
    let Some((support_normal, radius, support_midpoint)) = agreed_pair else {
        return Ok(None);
    };
    let scale = radius.max(cap_gap).max(1.0);
    if !(radius.is_finite()
        && (dot(support_normal, support_normal).abs() - 1.0).abs() <= EPS_ROUND_CAP_PARALLEL
        && support_midpoint.is_finite())
    {
        return Ok(None);
    }

    let [first_extent, second_extent] = envelope.extent_endpoints;
    let extent_delta =
        std::array::from_fn::<_, 3, _>(|index| second_extent[index] - first_extent[index]);
    let radial_span = dot(support_normal, extent_delta).abs();
    let axial_span = dot(axis, extent_delta).abs();
    if !((radial_span - 2.0 * radius).abs() <= EPS_ROUND_RADIUS_RECONCILIATION * scale
        && (axial_span - cap_gap).abs() <= EPS_ROUND_RADIUS_RECONCILIATION * scale
        && radial_span > EPS_ROUND_CAP_GAP
        && axial_span > EPS_ROUND_CAP_GAP)
    {
        return Ok(None);
    }

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
        _ => return Ok(None),
    };
    let start_offset = dot(support_normal, start);
    let origin = std::array::from_fn(|index| {
        start[index] + support_normal[index] * (support_midpoint - start_offset)
    });
    Ok(crate::surface::PositionalCylinderFrame::new(
        origin,
        axis,
        support_normal,
        radius,
        Some(cap_gap),
    ))
}

struct RoundSupportPlanes {
    caps: [PlaneEquation; 2],
    support_planes: Vec<(PlaneEquation, [f64; 3])>,
}

fn resolved_round_support_planes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    feature_id: u32,
) -> Result<Option<RoundSupportPlanes>, cadmpeg_core::CodecError> {
    let Some(affected_ids) = agreed_feature_geometry_ids(
        ctx,
        &scan.features.affected_ids,
        &scan.features.replay_affected_ids,
        feature_id,
    )?
    else {
        return Ok(None);
    };
    let [first_cap_id, second_cap_id, support_ids @ ..] = affected_ids else {
        return Ok(None);
    };
    if first_cap_id == second_cap_id {
        return Ok(None);
    }
    let mut scratch = ctx.reserve_scoped(0, "creo round support plane index")?;
    let local_planes = scratch.with_storage(|| placed_planes(ctx, scan))?;
    let Some(first_cap) =
        reconciled_model_plane(ctx, &local_planes, ir, source_carriers, *first_cap_id)?
    else {
        return Ok(None);
    };
    let Some(second_cap) =
        reconciled_model_plane(ctx, &local_planes, ir, source_carriers, *second_cap_id)?
    else {
        return Ok(None);
    };
    let caps = [first_cap, second_cap];
    let Some(first_cap_normal) = normalize(caps[0].normal) else {
        return Ok(None);
    };
    let Some(second_cap_normal) = normalize(caps[1].normal) else {
        return Ok(None);
    };
    if (dot(first_cap_normal, second_cap_normal).abs() - 1.0).abs() > EPS_ROUND_CAP_PARALLEL {
        return Ok(None);
    }
    let cap_gap = dot(
        first_cap_normal,
        std::array::from_fn(|index| caps[1].origin[index] - caps[0].origin[index]),
    )
    .abs();
    if cap_gap <= EPS_ROUND_CAP_GAP {
        return Ok(None);
    }
    let mut support_planes = Vec::new();
    let mut id_iter = support_ids.iter();
    while let Some(id) = ctx.next_charged(&mut id_iter, "creo round support plane IDs")? {
        let Some(plane) = reconciled_model_plane(ctx, &local_planes, ir, source_carriers, *id)?
        else {
            continue;
        };
        let Some(normal) = normalize(plane.normal) else {
            return Ok(None);
        };
        if !crate::vecmath::within(
            dot(first_cap_normal, normal).abs(),
            EPS_ROUND_SUPPORT_ORTHOGONAL,
        ) {
            return Ok(None);
        }
        ctx.push_vec(
            &mut support_planes,
            (plane, normal),
            "creo resolved envelope support planes",
        )?;
    }
    if support_planes.len() < 2 {
        return Ok(None);
    }
    Ok(Some(RoundSupportPlanes {
        caps,
        support_planes,
    }))
}

pub(in super::super) fn round_placed_cylinder_radii(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    feature_id: u32,
) -> Result<Vec<f64>, cadmpeg_core::CodecError> {
    let mut radii = Vec::new();
    for row in ctx
        .admit_iter(&*scan.surfaces.rows, "creo placed round surface rows")?
        .filter(|row| {
            row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Cylinder
        })
    {
        if let Some(radius) = round_placed_cylinder_radius(ctx, ir, row, source_carriers)? {
            ctx.reserve_vec(&mut radii, 1, "creo placed round radii")?;
            radii.push(radius);
        }
    }
    Ok(radii)
}

fn round_placed_cylinder_radius(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    row: &crate::surface::SurfaceRow,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<Option<f64>, cadmpeg_core::CodecError> {
    let surface = crate::decode::uniqueness::exactly_one_by(
        ctx,
        &ir.model.surfaces,
        |surface| {
            Ok(crate::identity::matches_numbered_identity(
                surface.id.as_str(),
                "creo:visibgeom:surface#",
                row.id,
            ))
        },
        "creo round placed cylinder surface",
    )?;
    let geometry = surface
        .map(|surface| source_carriers.surface_geometry(surface))
        .transpose()?;
    Ok(match geometry {
        Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder))) => {
            Some(cylinder.radius().get())
        }
        _ => None,
    })
}

pub(in super::super) fn round_observed_radii(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    feature_id: u32,
) -> Result<Vec<f64>, cadmpeg_core::CodecError> {
    Ok(round_observed_radius_rows(ctx, scan, feature_id)?.1)
}

fn round_observed_radius_rows(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    feature_id: u32,
) -> Result<(usize, Vec<f64>), cadmpeg_core::CodecError> {
    let mut generated_count = 0;
    let mut radii = Vec::new();
    for row in ctx
        .admit_iter(&*scan.surfaces.rows, "creo observed round surface rows")?
        .filter(|row| row.feature_id == feature_id)
    {
        generated_count += 1;
        let Some(parameters) = unique_surface_parameter_record(ctx, scan, row)? else {
            continue;
        };
        let radius = match row.kind {
            crate::surface::SurfaceKind::Cylinder => parameters.type24_generated_round_radius(),
            crate::surface::SurfaceKind::TorusOrSphere => {
                if let Some(overrides) = parameters.torus_radius_overrides_checked(ctx)? {
                    Some(overrides.radius2)
                } else {
                    replayed_torus_minor_radius(ctx, scan, row, parameters)?
                }
            }
            _ => None,
        };
        if let Some(radius) = radius {
            ctx.reserve_vec(&mut radii, 1, "creo observed round radii")?;
            radii.push(radius);
        }
    }
    Ok((generated_count, radii))
}

fn positive_length_spread(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    values: &[f64],
    additional: &[f64],
) -> Result<Option<(PositiveLength, f64, f64)>, cadmpeg_core::CodecError> {
    let Some(first) = values
        .first()
        .or_else(|| additional.first())
        .copied()
        .and_then(PositiveLength::new)
    else {
        return Ok(None);
    };
    let reference = first.get();
    let mut scale = reference.max(1.0);
    let mut difference: f64 = 0.0;
    let mut samples = values.iter().chain(additional);
    while let Some(&sample) = ctx.next_charged(&mut samples, "creo positive length agreement")? {
        if !sample.is_finite() || sample <= 0.0 {
            return Ok(None);
        }
        scale = scale.max(sample);
        difference = difference.max((sample - reference).abs());
    }
    Ok(Some((first, scale, difference)))
}

pub(in super::super) fn differing_positive_lengths(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    values: &[f64],
) -> Result<bool, cadmpeg_core::CodecError> {
    Ok(positive_length_spread(ctx, values, &[])?
        .is_some_and(|(_, scale, difference)| difference > EPS_GEOMETRY_AGREEMENT * scale))
}

pub(in super::super) fn unique_positive_length(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    values: &[f64],
) -> Result<Option<PositiveLength>, cadmpeg_core::CodecError> {
    Ok(
        positive_length_spread(ctx, values, &[])?.and_then(|(value, scale, difference)| {
            (difference <= EPS_GEOMETRY_AGREEMENT * scale).then_some(value)
        }),
    )
}

pub(super) fn differing_positive_length_sets(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    first: &[f64],
    second: &[f64],
) -> Result<bool, cadmpeg_core::CodecError> {
    Ok(positive_length_spread(ctx, first, second)?
        .is_some_and(|(_, scale, difference)| difference > EPS_GEOMETRY_AGREEMENT * scale))
}

fn equal_distance_chamfer_setback(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    cones: &[ConeEquation],
    support_planes: &[PlaneEquation],
) -> Result<Option<f64>, cadmpeg_core::CodecError> {
    if cones.is_empty() || support_planes.is_empty() {
        return Ok(None);
    }
    let mut first_setback: Option<f64> = None;
    let mut scale: f64 = 1.0;
    let mut max_difference: f64 = 0.0;
    let mut cone_iter = cones.iter();
    while let Some(cone) = ctx.next_charged(&mut cone_iter, "creo chamfer cone support pairs")? {
        let Some(axis) = normalize(cone.axis()) else {
            return Ok(None);
        };
        if !(circular_cone(*cone)
            && cone.radius().abs() <= EPS_RADIUS_NONZERO
            && (cone.half_angle() - std::f64::consts::FRAC_PI_4).abs() <= EPS_CONE_ANGLE)
        {
            return Ok(None);
        }
        let mut nearest: Option<f64> = None;
        for plane in ctx.admit_iter(support_planes, "creo chamfer support planes")? {
            let Some(normal) = normalize(plane.normal) else {
                continue;
            };
            let denominator = dot(axis, normal);
            if !crate::vecmath::within(1.0 - EPS_DENOMINATOR_ALIGNMENT, denominator.abs()) {
                continue;
            }
            let displacement = [
                plane.origin[0] - cone.origin()[0],
                plane.origin[1] - cone.origin()[1],
                plane.origin[2] - cone.origin()[2],
            ];
            let setback = dot(displacement, normal) / denominator;
            if !(setback.is_finite() && setback > EPS_SETBACK_NONZERO) {
                continue;
            }
            let replaces_nearest = match nearest {
                None => true,
                Some(current) => setback.total_cmp(&current).is_lt(),
            };
            if replaces_nearest {
                nearest = Some(setback);
            }
        }
        let Some(setback) = nearest else {
            return Ok(None);
        };
        if let Some(first) = first_setback {
            max_difference = max_difference.max((setback - first).abs());
        } else {
            first_setback = Some(setback);
        }
        scale = scale.max(setback.abs());
    }
    let Some(setback) = first_setback.and_then(PositiveLength::new) else {
        return Ok(None);
    };
    Ok((max_difference <= EPS_GEOMETRY_AGREEMENT * scale).then_some(setback.get()))
}

fn chamfer_cone_equation(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    row: &crate::surface::SurfaceRow,
) -> Result<Option<ConeEquation>, cadmpeg_core::CodecError> {
    let parameters = &*scan.surfaces.parameters;
    let parameter_record = match ctx.position_by(
        parameters,
        |record| Ok(record.offset == row.offset),
        "creo chamfer cone parameter records",
    )? {
        None => None,
        Some(first) => {
            if ctx.any_by(
                &parameters[first + 1..],
                |record| Ok(record.offset == row.offset),
                "creo chamfer cone parameter records",
            )? {
                return Ok(None);
            }
            Some(&parameters[first])
        }
    };
    if let Some(frame) =
        parameter_record.and_then(crate::surface::SurfaceParameterRecord::positional_cone_frame)
    {
        return Ok(ConeEquation::new(
            frame.frame().origin(),
            frame.frame().axis(),
            frame.frame().ref_direction(),
            0.0,
            1.0,
            frame.half_angle().get().get(),
        ));
    }
    let Some(surface) = crate::decode::uniqueness::exactly_one_by(
        ctx,
        &ir.model.surfaces,
        |surface| {
            Ok(crate::identity::matches_numbered_identity(
                surface.id.as_str(),
                "creo:visibgeom:surface#",
                row.id,
            ))
        },
        "creo chamfer cone model surfaces",
    )?
    else {
        return Ok(None);
    };
    let Some(SolvedSurfaceGeometry::Cone(cone_surface)) =
        source_carriers.surface_geometry(surface)?.solved()
    else {
        return Ok(None);
    };
    let origin = cone_surface.origin().get();
    let axis = cone_surface.frame().axis().as_raw();
    let ref_direction = cone_surface.frame().reference().as_raw();
    let radius = cone_surface.radius().get();
    let ratio = cone_surface.ratio().get();
    let half_angle = cone_surface.half_angle().get();
    Ok(ConeEquation::new(
        [origin.x, origin.y, origin.z],
        [axis.x, axis.y, axis.z],
        [ref_direction.x, ref_direction.y, ref_direction.z],
        radius,
        ratio,
        half_angle,
    ))
}

pub(in super::super) fn chamfer_constant_distance(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    feature_id: u32,
) -> Result<Option<f64>, cadmpeg_core::CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo chamfer evidence")?;
    let mut cones = Vec::new();
    let mut rows = scan.surfaces.rows.iter();
    while let Some(row) = ctx.next_charged(&mut rows, "creo chamfer generated surface rows")? {
        if row.feature_id != feature_id {
            continue;
        }
        if row.kind != crate::surface::SurfaceKind::Cone {
            return Ok(None);
        }
        let Some(cone) = chamfer_cone_equation(ctx, scan, ir, source_carriers, row)? else {
            return Ok(None);
        };
        scratch.with_storage(|| ctx.push_vec(&mut cones, cone, "creo chamfer cone witnesses"))?;
    }
    if cones.is_empty() {
        return Ok(None);
    }
    let Some(affected_ids) = agreed_feature_geometry_ids(
        ctx,
        &scan.features.affected_ids,
        &scan.features.replay_affected_ids,
        feature_id,
    )?
    else {
        return Ok(None);
    };
    let local_planes = scratch.with_storage(|| placed_planes(ctx, scan))?;
    let mut support_planes = Vec::new();
    let mut support_plane_ids = BTreeSet::new();
    let mut id_iter = affected_ids.iter();
    while let Some(id) = ctx.next_charged(&mut id_iter, "creo chamfer support plane IDs")? {
        let is_support_plane = match scan.surfaces.rows.unique(*id) {
            Some(row) => row.kind == crate::surface::SurfaceKind::Plane,
            None if scan.surfaces.rows.contains_id(*id) => {
                if ctx.any_by(
                    &*scan.surfaces.rows,
                    |row| Ok(row.id == *id && row.kind == crate::surface::SurfaceKind::Plane),
                    "creo chamfer support surface rows",
                )? {
                    return Ok(None);
                }
                continue;
            }
            None => {
                let surfaces = &ir.model.surfaces;
                let Some(first) = ctx.position_by(
                    surfaces,
                    |surface| {
                        Ok(crate::identity::matches_numbered_identity(
                            surface.id.as_str(),
                            "creo:visibgeom:surface#",
                            *id,
                        ))
                    },
                    "creo chamfer support model surfaces",
                )?
                else {
                    continue;
                };
                if ctx.any_by(
                    &surfaces[first + 1..],
                    |surface| {
                        Ok(crate::identity::matches_numbered_identity(
                            surface.id.as_str(),
                            "creo:visibgeom:surface#",
                            *id,
                        ))
                    },
                    "creo chamfer support model surfaces",
                )? {
                    return Ok(None);
                }
                matches!(
                    source_carriers.surface_geometry(&surfaces[first])?.solved(),
                    Some(SolvedSurfaceGeometry::Plane(_))
                )
            }
        };
        if !is_support_plane
            || ctx.contains_btree_set(
                &support_plane_ids,
                id,
                "creo chamfer support plane identity lookup",
            )?
        {
            continue;
        }
        scratch.with_storage(|| {
            ctx.insert_btree_set(
                &mut support_plane_ids,
                *id,
                "creo chamfer support plane IDs",
            )
        })?;
        let Some(plane) = reconciled_model_plane(ctx, &local_planes, ir, source_carriers, *id)?
        else {
            return Ok(None);
        };
        scratch.with_storage(|| {
            ctx.reserve_vec(&mut support_planes, 1, "creo chamfer support planes")
        })?;
        support_planes.push(plane);
    }
    equal_distance_chamfer_setback(ctx, &cones, &support_planes)
}

#[cfg(test)]
mod tests;
