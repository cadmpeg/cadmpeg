// SPDX-License-Identifier: Apache-2.0
//! Round and chamfer radius reconstruction from support geometry.

use super::super::surfaces::prototypes::{prototype_scalar, unique_surface_prototype_associations};
use super::super::uniqueness::exactly_one;
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

pub(in super::super) fn parallel_support_radius<T>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    sources: &[T],
    mut resolve_plane: impl FnMut(&T) -> Result<Option<PlaneEquation>, cadmpeg_core::CodecError>,
) -> Result<Option<f64>, cadmpeg_core::CodecError> {
    let mut first_radius: Option<f64> = None;
    let mut agrees = true;
    for (first_index, first) in ctx
        .admit_iter(sources, "creo round support plane pairs")?
        .enumerate()
    {
        let first_plane = resolve_plane(first)?;
        for (second_index, second) in ctx
            .admit_iter(sources, "creo round support plane pairs")?
            .enumerate()
        {
            let second_plane = resolve_plane(second)?;
            if second_index <= first_index {
                continue;
            }
            let Some(first_plane) = first_plane else {
                return Ok(None);
            };
            let Some(second_plane) = second_plane else {
                return Ok(None);
            };
            let Some(first_normal) = normalize(first_plane.normal) else {
                return Ok(None);
            };
            let Some(second_normal) = normalize(second_plane.normal) else {
                return Ok(None);
            };
            let alignment = first_normal
                .iter()
                .zip(second_normal)
                .map(|(first, second)| first * second)
                .sum::<f64>();
            if alignment.abs() < 1.0 - EPS_GEOMETRY_AGREEMENT {
                continue;
            }
            let gap = second_plane
                .origin
                .iter()
                .zip(first_plane.origin)
                .zip(first_normal)
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
                    agrees &= (candidate - radius).abs()
                        <= EPS_GEOMETRY_AGREEMENT * radius.abs().max(1.0);
                } else {
                    first_radius = Some(candidate);
                }
            }
        }
    }
    Ok(first_radius.filter(|radius| agrees && radius.is_finite()))
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
    let mut midplanes = Vec::<(PlaneEquation, f64)>::new();
    for (first_index, first_plane) in ctx
        .admit_iter(support_planes, "creo slot fillet support plane pairs")?
        .enumerate()
    {
        let Some(first_normal) = normalize(first_plane.normal) else {
            return Ok(None);
        };
        if dot(first_normal, axis).abs() > EPS_GEOMETRY_AGREEMENT {
            return Ok(None);
        }
        for second_plane in ctx.admit_iter(
            &support_planes[first_index + 1..],
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
            ctx.reserve_vec(&mut midplanes, 1, "creo slot fillet midplanes")?;
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
    for (first_index, first) in ctx
        .admit_iter(&midplanes, "creo slot fillet midplane pairs")?
        .enumerate()
    {
        for second in ctx.admit_iter(
            &midplanes[first_index + 1..],
            "creo slot fillet midplane pairs",
        )? {
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    frame: crate::surface::TorusOutlineFrame,
    radius: f64,
) -> Result<bool, cadmpeg_core::CodecError> {
    let scale = frame.values;
    let scale = ctx
        .admit_iter(&scale, "creo torus outline frame scale")?
        .map(|value| value.abs())
        .fold(radius.abs().max(1.0), f64::max);
    Ok(ctx
        .admit_iter(&frame.values[..3], "creo torus outline radius candidates")?
        .zip(ctx.admit_iter(&frame.values[3..], "creo torus outline radius candidates")?)
        .filter(|(first, second)| {
            ((*second - *first).abs() - radius).abs() <= EPS_GEOMETRY_AGREEMENT * scale
        })
        .count()
        == 1)
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    envelopes: [crate::surface::Type26FiveCoordinateEnvelope; 2],
    radius: f64,
) -> Result<Option<[f64; 3]>, cadmpeg_core::CodecError> {
    if !(radius.is_finite() && radius > 0.0) {
        return Ok(None);
    }
    let mut scale = radius.max(1.0);
    for envelope in ctx.admit_iter(&envelopes, "creo paired sphere envelopes")? {
        for value in ctx.admit_iter(&envelope.values, "creo paired sphere coordinates")? {
            scale = scale.max(value.abs());
        }
    }
    let mut decoded = [None, None];
    for (index, envelope) in ctx
        .admit_iter(&envelopes, "creo paired sphere envelopes")?
        .enumerate()
    {
        let [x_min, z0, y_min, radial_max, z1] = envelope.values;
        let close = |left: f64, right: f64| (left - right).abs() <= EPS_GEOMETRY_AGREEMENT * scale;
        decoded[index] = (close(x_min, y_min)
            && close(radial_max - x_min, 2.0 * radius)
            && close((z1 - z0).abs(), radius))
        .then_some(([x_min, radial_max], [z0, z1]));
    }
    let close = |left: f64, right: f64| (left - right).abs() <= EPS_GEOMETRY_AGREEMENT * scale;
    let [Some((first_radial, first_axial)), Some((second_radial, second_axial))] = decoded else {
        return Ok(None);
    };
    if !(close(first_radial[0], second_radial[0]) && close(first_radial[1], second_radial[1])) {
        return Ok(None);
    }
    let candidates = [first_axial[0], first_axial[1]];
    let other_axial = [second_axial[0], second_axial[1]];
    let mut center_z = None;
    for candidate in ctx.admit_iter(&candidates, "creo paired sphere center candidates")? {
        if ctx.any_by(
            &other_axial,
            |other| Ok(close(*candidate, *other)),
            "creo paired sphere matching axial coordinates",
        )? && center_z.replace(*candidate).is_some()
        {
            return Ok(None);
        }
    }
    let Some(center_z) = center_z else {
        return Ok(None);
    };
    let axial_values = [
        first_axial[0],
        first_axial[1],
        second_axial[0],
        second_axial[1],
    ];
    let axial_min = ctx
        .admit_iter(&axial_values, "creo paired sphere axial extent")?
        .copied()
        .fold(f64::INFINITY, f64::min);
    let axial_max = ctx
        .admit_iter(&axial_values, "creo paired sphere axial extent")?
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    Ok((close(axial_max - axial_min, 2.0 * radius)
        && close(center_z - axial_min, radius)
        && close(axial_max - center_z, radius))
    .then_some([
        0.5 * (first_radial[0] + first_radial[1]),
        0.5 * (first_radial[0] + first_radial[1]),
        center_z,
    ]))
}

pub(in super::super) fn unique_surface_parameter_record<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &'a ContainerScan,
    row: &crate::surface::SurfaceRow,
) -> Result<Option<&'a crate::surface::SurfaceParameterRecord>, cadmpeg_core::CodecError> {
    Ok(exactly_one(
        ctx.admit_iter(&*scan.surfaces.parameters, "creo surface parameter records")?
            .filter(|record| record.offset == row.offset),
    ))
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
    let prototype = exactly_one(
        ctx.admit_iter(
            &scan.surfaces.prototype_records,
            "creo torus prototype records",
        )?
        .filter(|prototype| {
            matches!(
                prototype.family,
                crate::surface::SurfacePrototypeFamily::Torus(_)
            ) && section.contains(prototype.offset)
        }),
    );
    Ok(prototype
        .and_then(|prototype| prototype_scalar(prototype, "radius2"))
        .filter(|radius| radius.is_finite() && *radius > 0.0))
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
    Ok(record.type26_replayed_minor_radius(prototype_minor_radius))
}

fn prototype_round_radius(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    rows: &[&crate::surface::SurfaceRow],
) -> Result<Option<f64>, cadmpeg_core::CodecError> {
    let Some(first) = rows.first() else {
        return Ok(None);
    };
    let feature_id = first.feature_id;
    let associations = unique_surface_prototype_associations(ctx, scan)?;
    let mut radii = None;
    for (record, row, _) in ctx.admit_iter(&associations, "creo round prototype associations")? {
        if !matches!(
            record.record().family,
            crate::surface::SurfacePrototypeFamily::Torus(_)
        ) || row.feature_id != feature_id
        {
            continue;
        }
        let mut matched_row = false;
        for candidate in ctx.admit_iter(rows, "creo round prototype surface rows")? {
            if candidate.offset == row.offset {
                matched_row = true;
                break;
            }
        }
        if !matched_row {
            continue;
        }
        let Some(radius1) = prototype_scalar(record.record(), "radius1") else {
            continue;
        };
        let Some(radius2) = prototype_scalar(record.record(), "radius2") else {
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
    for row in ctx.admit_iter(rows, "creo round prototype surface rows")? {
        let Some(record) = unique_surface_parameter_record(ctx, scan, row)? else {
            return Ok(None);
        };
        if record.torus_radius_overrides().is_some() {
            return Ok(None);
        }
        let replayed_matches = replayed_torus_minor_radius(ctx, scan, row, record)?
            .is_some_and(|radius| radius.to_bits() == radius2.to_bits());
        let outline_matches = if replayed_matches {
            false
        } else if let Some(frame) = record.torus_outline_frame() {
            outline_has_unique_radius_delta(ctx, frame, radius2)?
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
    let direct_radii = round_direct_radii(ctx, scan, feature_id)?;
    if let Some(radius) = match direct_radii.as_deref() {
        Some(values) => unique_positive_length(ctx, values)?,
        None => None,
    } {
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
    let mut generated_rows = Vec::new();
    for row in ctx
        .admit_iter(&*scan.surfaces.rows, "creo generated round surface rows")?
        .filter(|row| row.feature_id == feature_id)
    {
        ctx.reserve_vec(&mut generated_rows, 1, "creo generated round rows")?;
        generated_rows.push(row);
    }
    if generated_rows.is_empty() {
        return round_support_radius(ctx, scan, ir, source_carriers, feature_id);
    }
    if let Some(radius) = round_replay_radius(ctx, scan, ir, source_carriers, feature_id)? {
        return Ok(Some(radius));
    }
    // Unequal decoded rolling-radius samples identify a variable-radius
    // round even when another generated row has no radius proof. A support
    // plane fallback must not turn that incomplete, unequal sample set into
    // a false constant radius.
    if differing_positive_lengths(ctx, &round_observed_radii(ctx, scan, feature_id)?)? {
        return Ok(None);
    }
    let cylinder_count = ctx
        .admit_iter(&generated_rows, "creo generated round surface rows")?
        .filter(|row| row.kind == crate::surface::SurfaceKind::Cylinder)
        .count();
    if cylinder_count == 0 {
        if ctx.any_by(
            &generated_rows,
            |row| Ok(row.kind != crate::surface::SurfaceKind::TorusOrSphere),
            "creo generated round surface rows",
        )? {
            return Ok(None);
        }
        return prototype_round_radius(ctx, scan, &generated_rows);
    }
    if cylinder_count != generated_rows.len()
        && ctx.all_by(
            &generated_rows,
            |row| {
                Ok(matches!(
                    row.kind,
                    crate::surface::SurfaceKind::Cylinder
                        | crate::surface::SurfaceKind::TorusOrSphere
                ))
            },
            "creo generated round surface rows",
        )?
    {
        if let Some(radii) =
            mixed_round_radius_samples(ctx, scan, ir, source_carriers, &generated_rows)?
        {
            return Ok(unique_positive_length(ctx, &radii)?.map(PositiveLength::get));
        }
    }
    let cylinder_radii = round_placed_cylinder_radii(ctx, scan, ir, source_carriers, feature_id)?;
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
    let non_radius_rows_are_planes = ctx.all_by(
        &generated_rows,
        |row| {
            Ok(matches!(
                row.kind,
                crate::surface::SurfaceKind::Cylinder | crate::surface::SurfaceKind::Plane
            ))
        },
        "creo generated round surface rows",
    )?;
    if cylinder_radii.len() == cylinder_count && non_radius_rows_are_planes {
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
) -> Result<Option<f64>, cadmpeg_core::CodecError> {
    let mut samples = round_observed_radii(ctx, scan, feature_id)?;
    let placed = round_placed_cylinder_radii(ctx, scan, ir, source_carriers, feature_id)?;
    ctx.reserve_vec(&mut samples, placed.len(), "creo round replay samples")?;
    samples.extend(placed);
    let Some(radius) = unique_positive_length(ctx, &samples)? else {
        return Ok(None);
    };
    let radius = radius.get();
    let scale = radius.abs().max(1.0);
    Ok(ctx
        .admit_iter(
            &scan.features.round_replay_scalars,
            "creo round replay scalar records",
        )?
        .filter(|candidate| candidate.feature_id == feature_id)
        .any(|candidate| {
            candidate.value.get() > 0.0
                && (candidate.value.get() - radius).abs() <= EPS_ROUND_RADIUS_RECONCILIATION * scale
        })
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
    let radius = radius.get();
    let mut samples = round_observed_radii(ctx, scan, feature_id)?;
    let placed = round_placed_cylinder_radii(ctx, scan, ir, source_carriers, feature_id)?;
    ctx.reserve_vec(&mut samples, placed.len(), "creo legacy round samples")?;
    samples.extend(placed);
    if ctx.any_by(
        &samples,
        |sample| Ok(!sample.is_finite() || *sample <= 0.0),
        "creo legacy round radius samples",
    )? {
        return Ok(false);
    }
    let scale = ctx
        .admit_iter(&samples, "creo legacy round radius samples")?
        .copied()
        .map(f64::abs)
        .chain(std::iter::once(radius.abs()))
        .fold(1.0, f64::max);
    ctx.all_by(
        &samples,
        |sample| Ok((sample - radius).abs() <= EPS_ROUND_RADIUS_RECONCILIATION * scale),
        "creo legacy round radius samples",
    )
}

fn complete_direct_placed_cylinder_radius_agreement(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    feature_id: u32,
) -> Result<Option<bool>, cadmpeg_core::CodecError> {
    let mut agrees = true;
    for row in ctx
        .admit_iter(
            &*scan.surfaces.rows,
            "creo direct placed round surface rows",
        )?
        .filter(|row| {
            row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Cylinder
        })
    {
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
    let mut torus_rows = Vec::new();
    for row in ctx
        .admit_iter(rows, "creo mixed round surface rows")?
        .copied()
        .filter(|row| row.kind == crate::surface::SurfaceKind::TorusOrSphere)
    {
        ctx.reserve_vec(&mut torus_rows, 1, "creo mixed torus rows")?;
        torus_rows.push(row);
    }
    if torus_rows.is_empty() || torus_rows.len() == rows.len() {
        return Ok(None);
    }

    let mut cylinder_radii = Vec::new();
    for row in ctx
        .admit_iter(rows, "creo mixed round surface rows")?
        .copied()
        .filter(|row| row.kind == crate::surface::SurfaceKind::Cylinder)
    {
        let Some(radius) = round_cylinder_radius(ctx, scan, ir, source_carriers, row)? else {
            return Ok(None);
        };
        ctx.reserve_vec(&mut cylinder_radii, 1, "creo mixed cylinder radii")?;
        cylinder_radii.push(radius);
    }
    let Some(torus_radii) = mixed_torus_radius_samples(ctx, scan, &torus_rows)? else {
        return Ok(None);
    };
    ctx.reserve_vec(
        &mut cylinder_radii,
        torus_radii.len(),
        "creo mixed round samples",
    )?;
    cylinder_radii.extend(torus_radii);
    Ok(Some(cylinder_radii))
}

fn mixed_torus_radius_samples(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    rows: &[&crate::surface::SurfaceRow],
) -> Result<Option<Vec<f64>>, cadmpeg_core::CodecError> {
    let mut all_overrides = true;
    let mut any_overrides = false;
    for row in ctx.admit_iter(rows, "creo mixed torus surface rows")? {
        let Some(record) = unique_surface_parameter_record(ctx, scan, row)? else {
            return Ok(None);
        };
        let overridden = record.torus_radius_overrides().is_some();
        all_overrides &= overridden;
        any_overrides |= overridden;
    }
    if all_overrides {
        let mut radii = Vec::new();
        for row in ctx.admit_iter(rows, "creo mixed torus surface rows")? {
            let Some(record) = unique_surface_parameter_record(ctx, scan, row)? else {
                return Ok(None);
            };
            let Some(overrides) = record.torus_radius_overrides() else {
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
    let local_planes = placed_planes(ctx, scan)?;
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
    for id in ctx.admit_iter(support_ids, "creo round support plane IDs")? {
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
    }
    parallel_support_radius(ctx, support_ids, |id| {
        reconciled_model_plane(ctx, &local_planes, ir, source_carriers, *id)
    })
}

pub(in super::super) fn round_support_envelope_cylinder(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    feature_id: u32,
    envelope: Type24RoundEnvelope,
) -> Result<Option<crate::surface::PositionalCylinderFrame>, cadmpeg_core::CodecError> {
    let Some(RoundSupportPlanes {
        caps: [first_cap, second_cap],
        support_ids,
        local_planes,
    }) = resolved_round_support_planes(ctx, scan, ir, source_carriers, feature_id)?
    else {
        return Ok(None);
    };
    (|| -> Result<Option<crate::surface::PositionalCylinderFrame>, cadmpeg_core::CodecError> {
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
        for (first_index, first) in ctx
            .admit_iter(support_ids, "creo round support plane pairs")?
            .enumerate()
        {
            let Some(first) =
                reconciled_model_plane(ctx, &local_planes, ir, source_carriers, *first)?
            else {
                continue;
            };
            let Some(first_normal) = normalize(first.normal) else {
                return Ok(None);
            };
            for second in ctx
                .admit_iter(support_ids, "creo round support plane pairs")?
                .skip(first_index + 1)
            {
                let Some(second) =
                    reconciled_model_plane(ctx, &local_planes, ir, source_carriers, *second)?
                else {
                    continue;
                };
                let Some(second_normal) = normalize(second.normal) else {
                    return Ok(None);
                };
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
                        && (dot(pair.0, support_normal).abs() - 1.0).abs()
                            <= EPS_ROUND_CAP_PARALLEL
                        && (pair.2 - support_midpoint).abs()
                            <= EPS_ROUND_RADIUS_RECONCILIATION * scale)
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
    })()
}

struct RoundSupportPlanes<'a> {
    caps: [PlaneEquation; 2],
    support_ids: &'a [u32],
    local_planes: std::collections::BTreeMap<u32, PlaneEquation>,
}

fn resolved_round_support_planes<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &'a ContainerScan<'_>,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    feature_id: u32,
) -> Result<Option<RoundSupportPlanes<'a>>, cadmpeg_core::CodecError> {
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
    let local_planes = placed_planes(ctx, scan)?;
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
    let mut resolved_count = 0usize;
    for id in ctx.admit_iter(support_ids, "creo round support plane IDs")? {
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
        resolved_count = resolved_count.checked_add(1).ok_or_else(|| {
            ctx.refuse_codec_limit(
                "creo resolved round support plane count",
                u64::MAX,
                u64::MAX,
            )
        })?;
    }
    if resolved_count < 2 {
        return Ok(None);
    }
    Ok(Some(RoundSupportPlanes {
        caps,
        support_ids,
        local_planes,
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
    Ok(
        surface.and_then(|surface| match source_carriers.surface_geometry(surface) {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) => {
                let radius = cylinder_surface.radius().get();
                Some(radius)
            }
            _ => None,
        }),
    )
}

fn round_direct_radii(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    feature_id: u32,
) -> Result<Option<Vec<f64>>, cadmpeg_core::CodecError> {
    let generated_count = ctx
        .admit_iter(&*scan.surfaces.rows, "creo direct round surface rows")?
        .filter(|row| row.feature_id == feature_id)
        .count();
    if generated_count == 0 {
        return Ok(None);
    }
    let radii = round_observed_radii(ctx, scan, feature_id)?;
    Ok((radii.len() == generated_count).then_some(radii))
}

pub(in super::super) fn round_observed_radii(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    feature_id: u32,
) -> Result<Vec<f64>, cadmpeg_core::CodecError> {
    let mut radii = Vec::new();
    for row in ctx
        .admit_iter(&*scan.surfaces.rows, "creo observed round surface rows")?
        .filter(|row| row.feature_id == feature_id)
    {
        let Some(parameters) = unique_surface_parameter_record(ctx, scan, row)? else {
            continue;
        };
        let radius = match row.kind {
            crate::surface::SurfaceKind::Cylinder => parameters.type24_generated_round_radius(),
            crate::surface::SurfaceKind::TorusOrSphere => {
                if let Some(overrides) = parameters.torus_radius_overrides() {
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
    Ok(radii)
}

pub(in super::super) fn differing_positive_lengths(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    values: &[f64],
) -> Result<bool, cadmpeg_core::CodecError> {
    let Some(&first) = values.first() else {
        return Ok(false);
    };
    if ctx.any_by(
        values,
        |value| Ok(!value.is_finite() || *value <= 0.0),
        "creo positive length validation",
    )? {
        return Ok(false);
    }
    let scale = ctx
        .admit_iter(values, "creo positive length scale")?
        .copied()
        .map(f64::abs)
        .fold(first.abs().max(1.0), f64::max);
    ctx.any_by(
        values,
        |value| Ok((*value - first).abs() > EPS_GEOMETRY_AGREEMENT * scale),
        "creo positive length agreement",
    )
}

pub(in super::super) fn unique_positive_length(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    values: &[f64],
) -> Result<Option<PositiveLength>, cadmpeg_core::CodecError> {
    let Some(first) = values.first() else {
        return Ok(None);
    };
    let Some(value) = PositiveLength::new(*first) else {
        return Ok(None);
    };
    let value_raw = value.get();
    let scale = ctx
        .admit_iter(values, "creo positive length scale")?
        .copied()
        .map(f64::abs)
        .fold(value_raw.abs().max(1.0), f64::max);
    Ok(ctx
        .all_by(
            values,
            |candidate| {
                Ok(candidate.is_finite()
                    && *candidate > 0.0
                    && (*candidate - value_raw).abs() <= EPS_GEOMETRY_AGREEMENT * scale)
            },
            "creo positive length agreement",
        )?
        .then_some(value))
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
    for cone in ctx.admit_iter(cones, "creo chamfer cone support pairs")? {
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
    scan: &ContainerScan,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    row: &crate::surface::SurfaceRow,
) -> Option<ConeEquation> {
    let mut parameter_records = scan
        .surfaces
        .parameters
        .iter()
        .filter(|record| record.offset == row.offset);
    let parameter_record = parameter_records.next();
    if parameter_records.next().is_some() {
        return None;
    }
    if let Some(frame) =
        parameter_record.and_then(crate::surface::SurfaceParameterRecord::positional_cone_frame)
    {
        return ConeEquation::new(
            frame.frame().origin(),
            frame.frame().axis(),
            frame.frame().ref_direction(),
            0.0,
            1.0,
            frame.half_angle().get().get(),
        );
    }
    let surface = exactly_one(ir.model.surfaces.iter().filter(|surface| {
        crate::identity::matches_numbered_identity(
            surface.id.as_str(),
            "creo:visibgeom:surface#",
            row.id,
        )
    }))?;
    let Some(SolvedSurfaceGeometry::Cone(cone_surface)) =
        source_carriers.surface_geometry(surface).solved()
    else {
        return None;
    };
    let origin = cone_surface.origin().get();
    let axis = cone_surface.frame().axis().as_raw();
    let ref_direction = cone_surface.frame().reference().as_raw();
    let radius = cone_surface.radius().get();
    let ratio = cone_surface.ratio().get();
    let half_angle = cone_surface.half_angle().get();
    ConeEquation::new(
        [origin.x, origin.y, origin.z],
        [axis.x, axis.y, axis.z],
        [ref_direction.x, ref_direction.y, ref_direction.z],
        radius,
        ratio,
        half_angle,
    )
}

pub(in super::super) fn chamfer_constant_distance(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    feature_id: u32,
) -> Result<Option<f64>, cadmpeg_core::CodecError> {
    let mut candidates = ctx
        .admit_iter(&*scan.surfaces.rows, "creo chamfer generated surface rows")?
        .filter(|row| row.feature_id == feature_id);
    let Some(first) = candidates.next() else {
        return Ok(None);
    };
    if first.kind != crate::surface::SurfaceKind::Cone
        || candidates.any(|row| row.kind != crate::surface::SurfaceKind::Cone)
    {
        return Ok(None);
    }
    let mut cones = Vec::new();
    for row in ctx
        .admit_iter(&*scan.surfaces.rows, "creo chamfer generated surface rows")?
        .filter(|row| row.feature_id == feature_id)
    {
        let Some(cone) = chamfer_cone_equation(scan, ir, source_carriers, row) else {
            return Ok(None);
        };
        ctx.reserve_vec(&mut cones, 1, "creo chamfer cone witnesses")?;
        cones.push(cone);
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
    let local_planes = placed_planes(ctx, scan)?;
    let mut support_planes = Vec::new();
    let mut support_plane_ids = BTreeSet::new();
    for id in ctx.admit_iter(affected_ids, "creo chamfer support plane IDs")? {
        let mut rows = ctx
            .admit_iter(&*scan.surfaces.rows, "creo chamfer support surface rows")?
            .filter(|row| row.id == *id);
        let first_row = rows.next();
        let second_row = rows.next();
        let is_support_plane = match (first_row, second_row) {
            (None, None) => {
                let mut model_surfaces = ctx
                    .admit_iter(&ir.model.surfaces, "creo chamfer support model surfaces")?
                    .filter(|surface| {
                        crate::identity::matches_numbered_identity(
                            surface.id.as_str(),
                            "creo:visibgeom:surface#",
                            *id,
                        )
                    });
                let first = model_surfaces.next();
                let second = model_surfaces.next();
                match (first, second) {
                    (None, None) => false,
                    (Some(surface), None) => matches!(
                        source_carriers.surface_geometry(surface).solved(),
                        Some(SolvedSurfaceGeometry::Plane(_))
                    ),
                    _ => return Ok(None),
                }
            }
            (Some(row), None) => row.kind == crate::surface::SurfaceKind::Plane,
            (Some(first), Some(second)) => {
                if first.kind == crate::surface::SurfaceKind::Plane
                    || second.kind == crate::surface::SurfaceKind::Plane
                    || rows.any(|row| row.kind == crate::surface::SurfaceKind::Plane)
                {
                    return Ok(None);
                }
                continue;
            }
            _ => continue,
        };
        if !is_support_plane || ctx.contains_btree_set(&support_plane_ids, id, "creo chamfer support plane identity lookup")? {
            continue;
        }
        ctx.insert_btree_set(
            &mut support_plane_ids,
            *id,
            "creo chamfer support plane IDs",
        )?;
        let Some(plane) = reconciled_model_plane(ctx, &local_planes, ir, source_carriers, *id)?
        else {
            return Ok(None);
        };
        ctx.reserve_vec(&mut support_planes, 1, "creo chamfer support planes")?;
        support_planes.push(plane);
    }
    equal_distance_chamfer_setback(ctx, &cones, &support_planes)
}

#[cfg(test)]
mod tests;
