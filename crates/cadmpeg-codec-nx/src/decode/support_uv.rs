// SPDX-License-Identifier: Apache-2.0
//! EXT11 support-UV assignment, completion, and equivalent-parameter transfer.

use super::blend::{
    blend_boundary_parameter_from_support_pcurve, blend_surface_definition,
    blend_surface_parameter_grid_with_index, blend_surface_parameters_for_fit_with_grid,
    blend_surface_parameters_from_grid_for_fit, decoded_surface_point_inner, BlendParameterGrid,
    BoundaryInverseTarget,
};
use super::offset::{
    continue_surface_intersection_parameters_with_seeds,
    offset_surface_parameters_with_tolerance_with_index, point_distance, surface_parameters,
};
use super::pcurves::pcurve_matches_edge_range_with_index;
use super::MISSING_TOLERANCE;
use crate::topology::Graph;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::eval::{
    analytic_surface_parameters, nurbs_surface_parameter_within_tolerance, pcurve_uv,
};
use cadmpeg_ir::geometry::{
    CurveGeometry, Pcurve, PcurveGeometry, ProceduralCurveDefinition, ProceduralSurface,
    ProceduralSurfaceDefinition, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, PcurveId, ProceduralCurveId, SurfaceId};
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::AnnotationBuilder;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn linear_knots(parameters: &[f64]) -> Vec<f64> {
    let mut knots = Vec::with_capacity(parameters.len() + 2);
    knots.push(parameters[0]);
    knots.extend_from_slice(parameters);
    knots.push(*parameters.last().expect("non-empty chart parameters"));
    knots
}

pub(crate) fn assign_ext11_support_uv(
    ir: &CadIr,
    surfaces_by_xmt: &BTreeMap<u32, SurfaceId>,
    supports: [u32; 2],
    points: &[Point3],
    fit_tolerance: f64,
    lanes: &[Option<Vec<[f64; 2]>>; 2],
) -> Option<[Option<Vec<[f64; 2]>>; 2]> {
    let surface_ids = supports.map(|support| surfaces_by_xmt.get(&support).cloned());
    let [Some(first_surface), Some(second_surface)] = surface_ids else {
        return None;
    };
    assign_ext11_support_uv_to_surfaces(
        ir,
        [&first_surface, &second_surface],
        points,
        fit_tolerance,
        lanes,
    )
}

pub(crate) fn validate_serialized_support_uv(
    ir: &CadIr,
    surfaces_by_xmt: &BTreeMap<u32, SurfaceId>,
    supports: [u32; 2],
    points: &[Point3],
    fit_tolerance: f64,
    lanes: &[Option<Vec<[f64; 2]>>; 2],
) -> [Option<Vec<[f64; 2]>>; 2] {
    let index = cadmpeg_ir::index::ModelIndex::new(ir);
    std::array::from_fn(|side| {
        let surface = surfaces_by_xmt.get(&supports[side])?;
        let values = lanes[side].as_deref()?;
        let tolerance = blend_spine_cache_fit_tolerance(ir, surface, fit_tolerance);
        support_uv_lane_matches_surface(ir, &index, surface, points, tolerance, Some(values))
            .then(|| values.to_vec())
    })
}

pub(crate) fn support_uv_lane_matches_surface(
    ir: &CadIr,
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    points: &[Point3],
    fit_tolerance: f64,
    values: Option<&[[f64; 2]]>,
) -> bool {
    let Some(values) = values.filter(|values| values.len() == points.len()) else {
        return false;
    };
    let Some(geometry) = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| &candidate.id == surface)
        .map(|surface| &surface.geometry)
    else {
        return false;
    };
    values.iter().zip(points).all(|(uv, point)| {
        if uv
            .iter()
            .any(|value| !value.is_finite() || missing_support_parameter(*value))
        {
            return false;
        }
        let Some(uv) = surface_parameters(geometry, *uv) else {
            return false;
        };
        decoded_surface_point_inner(index, surface, uv.u, uv.v, 0)
            .is_some_and(|candidate| point_distance(candidate, *point) <= fit_tolerance)
    })
}

pub(crate) fn assign_ext11_support_uv_to_surfaces(
    ir: &CadIr,
    surfaces: [&SurfaceId; 2],
    points: &[Point3],
    fit_tolerance: f64,
    lanes: &[Option<Vec<[f64; 2]>>; 2],
) -> Option<[Option<Vec<[f64; 2]>>; 2]> {
    let index = cadmpeg_ir::index::ModelIndex::new(ir);
    let lane_matches_surface = |surface: &SurfaceId, lane: usize| {
        support_uv_lane_matches_surface(
            ir,
            &index,
            surface,
            points,
            fit_tolerance,
            lanes[lane].as_deref(),
        )
    };
    let matches = [
        [
            lane_matches_surface(surfaces[0], 0),
            lane_matches_surface(surfaces[0], 1),
        ],
        [
            lane_matches_surface(surfaces[1], 0),
            lane_matches_surface(surfaces[1], 1),
        ],
    ];
    let mut assigned = [None, None];
    let mut assigned_lanes = [None, None];
    for lane in 0..2 {
        let support_matches = [matches[0][lane], matches[1][lane]];
        let Some(support) = support_matches
            .iter()
            .position(|matches| *matches)
            .filter(|_| support_matches.iter().filter(|matches| **matches).count() == 1)
        else {
            continue;
        };
        if assigned[support].is_some() {
            return None;
        }
        assigned[support].clone_from(&lanes[lane]);
        assigned_lanes[support] = Some(lane);
    }
    if surfaces[0] != surfaces[1] && assigned.iter().filter(|lane| lane.is_some()).count() == 1 {
        let assigned_support = assigned.iter().position(Option::is_some)?;
        let assigned_lane = assigned_lanes[assigned_support]?;
        let other_support = 1 - assigned_support;
        let other_lane = 1 - assigned_lane;
        if lane_matches_surface(surfaces[other_support], other_lane) {
            assigned[other_support].clone_from(&lanes[other_lane]);
        }
    }
    assigned.iter().any(Option::is_some).then_some(assigned)
}

pub(crate) type PendingExt11SupportUv = (
    ProceduralCurveId,
    Vec<Point3>,
    Vec<f64>,
    f64,
    [Option<Vec<[f64; 2]>>; 2],
);

pub(crate) fn missing_support_parameter(value: f64) -> bool {
    value.to_bits() == MISSING_TOLERANCE.to_bits()
}

pub(crate) fn pcurve_requires_completion(pcurve: Option<&PcurveGeometry>) -> bool {
    match pcurve {
        None => true,
        Some(PcurveGeometry::Nurbs { control_points, .. }) => control_points.iter().any(|point| {
            !point.u.is_finite()
                || !point.v.is_finite()
                || missing_support_parameter(point.u)
                || missing_support_parameter(point.v)
        }),
        Some(PcurveGeometry::Line { origin, direction }) => [origin, direction]
            .into_iter()
            .any(|point| !point.u.is_finite() || !point.v.is_finite()),
        Some(_) => false,
    }
}

pub(crate) fn pcurve_control_point_seed(
    pcurve: Option<&PcurveGeometry>,
    index: usize,
) -> Option<Point2> {
    let PcurveGeometry::Nurbs { control_points, .. } = pcurve? else {
        return None;
    };
    control_points.get(index).copied().filter(|point| {
        point.u.is_finite()
            && point.v.is_finite()
            && !missing_support_parameter(point.u)
            && !missing_support_parameter(point.v)
    })
}

pub(crate) fn complete_ext11_support_uv(ir: &mut CadIr, pending: &[PendingExt11SupportUv]) {
    for (procedural_id, points, parameters, fit_tolerance, lanes) in pending {
        let Some(procedural_index) = ir
            .model
            .procedural_curves
            .iter()
            .position(|procedural| &procedural.id == procedural_id)
        else {
            continue;
        };
        let (surfaces, missing) = match &ir.model.procedural_curves[procedural_index].definition {
            ProceduralCurveDefinition::Intersection { context, .. } => {
                let [Some(first), Some(second)] = &context.sides.clone().map(|side| side.surface)
                else {
                    continue;
                };
                (
                    [first.clone(), second.clone()],
                    context
                        .sides
                        .each_ref()
                        .map(|side| pcurve_requires_completion(side.pcurve.as_ref())),
                )
            }
            _ => continue,
        };
        if !missing.into_iter().any(|missing| missing) {
            continue;
        }
        let Some(assigned) = assign_ext11_support_uv_to_surfaces(
            ir,
            [&surfaces[0], &surfaces[1]],
            points,
            *fit_tolerance,
            lanes,
        ) else {
            continue;
        };
        let replacements: [Option<PcurveGeometry>; 2] = std::array::from_fn(|side| {
            if !missing[side] {
                return None;
            }
            let surface_geometry = ir
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == surfaces[side])
                .map(|surface| &surface.geometry)?;
            let values = assigned[side].as_ref()?;
            if values
                .iter()
                .flatten()
                .any(|value| !value.is_finite() || missing_support_parameter(*value))
            {
                return None;
            }
            let control_points = values
                .iter()
                .map(|uv| surface_parameters(surface_geometry, *uv))
                .collect::<Option<Vec<_>>>()?;
            Some(PcurveGeometry::Nurbs {
                degree: 1,
                knots: linear_knots(parameters),
                control_points,
                weights: None,
                periodic: false,
            })
        });
        let ProceduralCurveDefinition::Intersection { context, .. } =
            &mut ir.model.procedural_curves[procedural_index].definition
        else {
            unreachable!("definition checked above");
        };
        for (side, replacement) in replacements.into_iter().enumerate() {
            if let Some(replacement) = replacement {
                context.sides[side].pcurve = Some(replacement);
            }
        }
    }
}

pub(crate) fn complete_support_uv(ir: &mut CadIr, pending: &[PendingExt11SupportUv]) {
    loop {
        let before = pending_support_lanes_requiring_completion(ir, pending);
        complete_support_uv_wave(ir, pending);
        let after = pending_support_lanes_requiring_completion(ir, pending);
        if after >= before {
            break;
        }
    }
}

pub(crate) fn invalidate_inconsistent_support_uv(
    ir: &mut CadIr,
    pending: &[PendingExt11SupportUv],
) {
    let invalid = {
        let index = cadmpeg_ir::index::ModelIndex::new(ir);
        let mut invalid = Vec::new();
        for (procedural_id, points, parameters, fit_tolerance, _) in pending {
            let Some(procedural_index) = ir
                .model
                .procedural_curves
                .iter()
                .position(|procedural| &procedural.id == procedural_id)
            else {
                continue;
            };
            let ProceduralCurveDefinition::Intersection { context, .. } =
                &ir.model.procedural_curves[procedural_index].definition
            else {
                continue;
            };
            for (side, support) in context.sides.iter().enumerate() {
                let (Some(surface), Some(pcurve)) = (&support.surface, &support.pcurve) else {
                    continue;
                };
                let tolerance = blend_spine_cache_fit_tolerance(ir, surface, *fit_tolerance);
                let inconsistent = parameters
                    .iter()
                    .zip(points)
                    .filter_map(|(parameter, point)| {
                        let uv = pcurve_uv(pcurve, *parameter)?;
                        decoded_surface_point_inner(&index, surface, uv.u, uv.v, 0)
                            .map(|actual| point_distance(actual, *point) > tolerance)
                    })
                    .any(|inconsistent| inconsistent);
                if inconsistent {
                    invalid.push((procedural_index, side));
                }
            }
        }
        invalid
    };
    for (procedural_index, side) in invalid {
        let ProceduralCurveDefinition::Intersection { context, .. } =
            &mut ir.model.procedural_curves[procedural_index].definition
        else {
            unreachable!("definition selected above");
        };
        context.sides[side].pcurve = None;
    }
}

pub(crate) fn pending_support_lanes_requiring_completion(
    ir: &CadIr,
    pending: &[PendingExt11SupportUv],
) -> usize {
    pending
        .iter()
        .filter_map(|(procedural_id, ..)| {
            ir.model
                .procedural_curves
                .iter()
                .find(|procedural| &procedural.id == procedural_id)
        })
        .filter_map(|procedural| {
            let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition
            else {
                return None;
            };
            Some(
                context
                    .sides
                    .iter()
                    .filter(|side| pcurve_requires_completion(side.pcurve.as_ref()))
                    .count(),
            )
        })
        .sum()
}

pub(crate) fn complete_support_uv_wave(ir: &mut CadIr, pending: &[PendingExt11SupportUv]) {
    let mut replacements = Vec::new();
    let mut blend_parameter_grids = BTreeMap::<SurfaceId, Option<Vec<(Point2, Point3)>>>::new();
    let model_index = cadmpeg_ir::index::ModelIndex::new(ir);
    for (procedural_id, points, parameters, fit_tolerance, _) in pending {
        let Some(procedural) = ir
            .model
            .procedural_curves
            .iter()
            .find(|procedural| &procedural.id == procedural_id)
        else {
            continue;
        };
        let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition else {
            continue;
        };
        for side in 0..2 {
            if !pcurve_requires_completion(context.sides[side].pcurve.as_ref()) {
                continue;
            }
            let Some(surface_id) = &context.sides[side].surface else {
                continue;
            };
            let Some(surface) = ir
                .model
                .surfaces
                .iter()
                .find(|surface| &surface.id == surface_id)
            else {
                continue;
            };
            let effective_fit_tolerance =
                blend_spine_cache_fit_tolerance(ir, surface_id, *fit_tolerance);
            let mut uv = Vec::with_capacity(points.len());
            for (point_index, point) in points.iter().enumerate() {
                let seed =
                    pcurve_control_point_seed(context.sides[side].pcurve.as_ref(), point_index)
                        .or_else(|| uv.last().copied());
                let parameters = match &surface.geometry {
                    SurfaceGeometry::Nurbs(nurbs) => nurbs_surface_parameter_within_tolerance(
                        nurbs,
                        *point,
                        seed,
                        effective_fit_tolerance,
                    ),
                    SurfaceGeometry::Procedural { .. } => {
                        let other_side = &context.sides[1 - side];
                        other_side
                            .surface
                            .as_ref()
                            .zip(other_side.pcurve.as_ref())
                            .and_then(|(other_surface, other_pcurve)| {
                                blend_boundary_parameter_from_support_pcurve(
                                    &model_index,
                                    ir,
                                    surface_id,
                                    other_surface,
                                    other_pcurve,
                                    parameters[point_index],
                                    BoundaryInverseTarget {
                                        point: *point,
                                        seed,
                                        tolerance: effective_fit_tolerance,
                                    },
                                )
                            })
                            .or_else(|| {
                                offset_surface_parameters_with_tolerance_with_index(
                                    &model_index,
                                    surface_id,
                                    *point,
                                    seed,
                                    Some(effective_fit_tolerance),
                                )
                            })
                            .or_else(|| {
                                blend_surface_parameters_for_fit_with_grid(
                                    &model_index,
                                    surface_id,
                                    *point,
                                    seed,
                                    effective_fit_tolerance,
                                    BlendParameterGrid::Disabled,
                                )
                            })
                            .or_else(|| {
                                let blend_grid = blend_parameter_grids
                                    .entry(surface_id.clone())
                                    .or_insert_with(|| {
                                        blend_surface_parameter_grid_with_index(
                                            &model_index,
                                            surface_id,
                                            0,
                                        )
                                    });
                                blend_surface_parameters_from_grid_for_fit(
                                    &model_index,
                                    surface_id,
                                    *point,
                                    effective_fit_tolerance,
                                    blend_grid.as_deref()?,
                                )
                            })
                    }
                    geometry => analytic_surface_parameters(geometry, *point),
                };
                let Some(parameters) = parameters else {
                    uv.clear();
                    break;
                };
                uv.push(parameters);
            }
            if uv.len() != points.len() {
                continue;
            }
            if matches!(
                surface.geometry,
                SurfaceGeometry::Cylinder { .. }
                    | SurfaceGeometry::Cone { .. }
                    | SurfaceGeometry::Sphere { .. }
                    | SurfaceGeometry::Torus { .. }
            ) {
                for index in 1..uv.len() {
                    let turns = ((uv[index - 1].u - uv[index].u) / std::f64::consts::TAU).round();
                    uv[index].u += turns * std::f64::consts::TAU;
                }
            }
            let reproduces_chart = uv.iter().zip(points).all(|(uv, point)| {
                decoded_surface_point_inner(&model_index, surface_id, uv.u, uv.v, 0)
                    .is_some_and(|actual| point_distance(actual, *point) <= effective_fit_tolerance)
            });
            if reproduces_chart {
                replacements.push((
                    procedural_id.clone(),
                    side,
                    PcurveGeometry::Nurbs {
                        degree: 1,
                        knots: linear_knots(parameters),
                        control_points: uv,
                        weights: None,
                        periodic: false,
                    },
                    effective_fit_tolerance,
                ));
            }
        }
    }
    let cache_backed_curves = ir
        .model
        .curves
        .iter()
        .filter(|curve| !matches!(&curve.geometry, CurveGeometry::Procedural { .. }))
        .map(|curve| curve.id.clone())
        .collect::<BTreeSet<_>>();
    for (procedural_id, side, pcurve, effective_fit_tolerance) in replacements {
        let Some(procedural) = ir
            .model
            .procedural_curves
            .iter_mut()
            .find(|procedural| procedural.id == procedural_id)
        else {
            continue;
        };
        let ProceduralCurveDefinition::Intersection { context, .. } = &mut procedural.definition
        else {
            continue;
        };
        if pcurve_requires_completion(context.sides[side].pcurve.as_ref()) {
            context.sides[side].pcurve = Some(pcurve);
            if cache_backed_curves.contains(&procedural.curve) {
                procedural.cache_fit_tolerance = Some(
                    procedural
                        .cache_fit_tolerance
                        .unwrap_or(0.0)
                        .max(effective_fit_tolerance),
                );
            }
        }
    }
    complete_coupled_support_uv(ir, pending);
}

pub(crate) fn blend_spine_cache_fit_tolerance(
    ir: &CadIr,
    surface: &SurfaceId,
    fit_tolerance: f64,
) -> f64 {
    blend_surface_definition(ir, surface)
        .and_then(|(_, spine, _, _)| {
            ir.model
                .procedural_curves
                .iter()
                .find(|procedural| procedural.curve == spine)
                .and_then(|procedural| procedural.cache_fit_tolerance)
        })
        .filter(|tolerance| tolerance.is_finite() && *tolerance > 0.0)
        .map_or(fit_tolerance, |tolerance| fit_tolerance + tolerance)
}

pub(crate) fn complete_coupled_support_uv(ir: &mut CadIr, pending: &[PendingExt11SupportUv]) {
    let mut replacements = Vec::new();
    for (procedural_id, points, parameters, fit_tolerance, _) in pending {
        let Some(procedural) = ir
            .model
            .procedural_curves
            .iter()
            .find(|procedural| &procedural.id == procedural_id)
        else {
            continue;
        };
        let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition else {
            continue;
        };
        let missing = context
            .sides
            .each_ref()
            .map(|side| pcurve_requires_completion(side.pcurve.as_ref()));
        let [Some(first_surface), Some(second_surface)] =
            context.sides.each_ref().map(|side| side.surface.as_ref())
        else {
            continue;
        };
        let surfaces = [first_surface, second_surface];
        let unresolved_procedural_support = (0..2).any(|side| {
            missing[side]
                && pcurve_control_point_seed(context.sides[side].pcurve.as_ref(), 0).is_some()
                && ir.model.surfaces.iter().any(|surface| {
                    &surface.id == surfaces[side]
                        && matches!(surface.geometry, SurfaceGeometry::Procedural { .. })
                })
        });
        if !unresolved_procedural_support {
            continue;
        }
        let seeds = context
            .sides
            .each_ref()
            .map(|side| pcurve_control_point_seed(side.pcurve.as_ref(), 0));
        let Some(lanes) = continue_surface_intersection_parameters_with_seeds(
            ir,
            surfaces,
            points,
            *fit_tolerance,
            seeds,
        ) else {
            continue;
        };
        for side in 0..2 {
            if missing[side] {
                replacements.push((
                    procedural_id.clone(),
                    side,
                    PcurveGeometry::Nurbs {
                        degree: 1,
                        knots: linear_knots(parameters),
                        control_points: lanes[side].clone(),
                        weights: None,
                        periodic: false,
                    },
                ));
            }
        }
    }
    for (procedural_id, side, pcurve) in replacements {
        let Some(procedural) = ir
            .model
            .procedural_curves
            .iter_mut()
            .find(|procedural| procedural.id == procedural_id)
        else {
            continue;
        };
        let ProceduralCurveDefinition::Intersection { context, .. } = &mut procedural.definition
        else {
            continue;
        };
        if pcurve_requires_completion(context.sides[side].pcurve.as_ref()) {
            context.sides[side].pcurve = Some(pcurve);
        }
    }
}

pub(crate) fn complete_parameterization_equivalent_support_uv(ir: &mut CadIr) {
    let replacements = ir
        .model
        .procedural_curves
        .iter()
        .enumerate()
        .filter_map(|(procedural_index, procedural)| {
            let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition
            else {
                return None;
            };
            let missing = context
                .sides
                .each_ref()
                .map(|side| pcurve_requires_completion(side.pcurve.as_ref()));
            let target = match missing {
                [true, false] => 0,
                [false, true] => 1,
                _ => return None,
            };
            let source = 1 - target;
            let (Some(target_surface), Some(source_surface), Some(source_pcurve)) = (
                context.sides[target].surface.as_ref(),
                context.sides[source].surface.as_ref(),
                context.sides[source].pcurve.as_ref(),
            ) else {
                return None;
            };
            parameterization_equivalent_surfaces(ir, target_surface, source_surface)
                .then(|| (procedural_index, target, source_pcurve.clone()))
        })
        .collect::<Vec<_>>();
    for (procedural_index, side, pcurve) in replacements {
        let ProceduralCurveDefinition::Intersection { context, .. } =
            &mut ir.model.procedural_curves[procedural_index].definition
        else {
            unreachable!("definition selected above");
        };
        if pcurve_requires_completion(context.sides[side].pcurve.as_ref()) {
            context.sides[side].pcurve = Some(pcurve);
        }
    }
}

pub(crate) fn parameterization_equivalent_surfaces(
    ir: &CadIr,
    first: &SurfaceId,
    second: &SurfaceId,
) -> bool {
    fn equivalent(
        ir: &CadIr,
        first: &SurfaceId,
        second: &SurfaceId,
        visited: &mut BTreeSet<(SurfaceId, SurfaceId)>,
    ) -> bool {
        if first == second {
            return true;
        }
        if !visited.insert((first.clone(), second.clone())) {
            return false;
        }
        let geometry = |id: &SurfaceId| {
            ir.model
                .surfaces
                .iter()
                .find(|surface| &surface.id == id)
                .map(|surface| &surface.geometry)
        };
        let (Some(first_geometry), Some(second_geometry)) = (geometry(first), geometry(second))
        else {
            return false;
        };
        if first_geometry == second_geometry {
            return true;
        }
        let (
            Some(ProceduralSurfaceDefinition::Offset {
                support: first_support,
                distance: first_distance,
                u_sense: first_u_sense,
                v_sense: first_v_sense,
                extension_flags: first_extensions,
                ..
            }),
            Some(ProceduralSurfaceDefinition::Offset {
                support: second_support,
                distance: second_distance,
                u_sense: second_u_sense,
                v_sense: second_v_sense,
                extension_flags: second_extensions,
                ..
            }),
        ) = (
            procedural_surface_for_carrier(ir, first).map(|surface| &surface.definition),
            procedural_surface_for_carrier(ir, second).map(|surface| &surface.definition),
        )
        else {
            return false;
        };
        first_distance.to_bits() == second_distance.to_bits()
            && first_u_sense == second_u_sense
            && first_v_sense == second_v_sense
            && first_extensions == second_extensions
            && equivalent(ir, first_support, second_support, visited)
    }

    equivalent(ir, first, second, &mut BTreeSet::new())
}

pub(crate) fn procedural_surface_for_carrier<'a>(
    ir: &'a CadIr,
    surface: &SurfaceId,
) -> Option<&'a ProceduralSurface> {
    let carrier = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| &candidate.id == surface)?;
    if let SurfaceGeometry::Procedural { construction } = &carrier.geometry {
        return ir
            .model
            .procedural_surfaces
            .iter()
            .find(|candidate| candidate.id == *construction && candidate.surface == *surface);
    }
    let mut producers = ir.model.procedural_surfaces.iter().filter(|candidate| {
        candidate.surface == *surface && candidate.cache_fit_tolerance.is_some()
    });
    let producer = producers.next()?;
    producers.next().is_none().then_some(producer)
}

pub(crate) fn attach_completed_intersection_pcurves(
    ir: &mut CadIr,
    graph: &Graph,
    prefix: &str,
    source_stream: cadmpeg_ir::annotations::StreamHandle,
    annotations: &mut AnnotationBuilder,
) {
    let loop_faces = ir
        .model
        .loops
        .iter()
        .map(|loop_| (&loop_.id, &loop_.face))
        .collect::<BTreeMap<_, _>>();
    let face_surfaces = ir
        .model
        .faces
        .iter()
        .map(|face| (&face.id, &face.surface))
        .collect::<BTreeMap<_, _>>();
    let edge_curves = ir
        .model
        .edges
        .iter()
        .filter_map(|edge| Some((&edge.id, edge.curve.as_ref()?)))
        .collect::<BTreeMap<_, _>>();
    let mut candidates =
        BTreeMap::<(CurveId, SurfaceId), Vec<(PcurveGeometry, [f64; 2], Option<f64>)>>::new();
    for procedural in &ir.model.procedural_curves {
        let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition else {
            continue;
        };
        for side in &context.sides {
            let (Some(surface), Some(pcurve)) = (&side.surface, &side.pcurve) else {
                continue;
            };
            let values = candidates
                .entry((procedural.curve.clone(), surface.clone()))
                .or_default();
            let candidate = (
                pcurve.clone(),
                context.parameter_range,
                procedural.cache_fit_tolerance,
            );
            if !values.contains(&candidate) {
                values.push(candidate);
            }
        }
    }

    let replacements = {
        let model_index = cadmpeg_ir::index::ModelIndex::new(ir);
        ir.model
            .coedges
            .iter()
            .filter(|coedge| coedge.pcurves.is_empty() && coedge.id.0.starts_with(prefix))
            .filter_map(|coedge| {
                let surface = loop_faces
                    .get(&coedge.owner_loop)
                    .and_then(|face| face_surfaces.get(*face))?;
                let curve = edge_curves.get(&coedge.edge)?;
                let [candidate] = candidates
                    .get(&((*curve).clone(), (*surface).clone()))?
                    .as_slice()
                else {
                    return None;
                };
                let fit_tolerance = candidate.2.or_else(|| {
                    ir.model
                        .edges
                        .iter()
                        .find(|edge| edge.id == coedge.edge)
                        .and_then(|edge| edge.tolerance)
                });
                pcurve_matches_edge_range_with_index(
                    ir,
                    &model_index,
                    &coedge.edge,
                    surface,
                    &candidate.0,
                    None,
                    fit_tolerance,
                )
                .then(|| {
                    (
                        coedge.id.clone(),
                        (candidate.0.clone(), candidate.1, fit_tolerance),
                    )
                })
            })
            .collect::<Vec<_>>()
    };
    for (coedge_id, (geometry, parameter_range, fit_tolerance)) in replacements {
        let Some(fin_xmt) = coedge_id
            .0
            .rsplit_once('#')
            .and_then(|(_, value)| value.parse::<u32>().ok())
        else {
            continue;
        };
        let pcurve_id = PcurveId(format!("{prefix}:intersection-pcurve-completed#{fin_xmt}"));
        if ir.model.pcurves.iter().any(|pcurve| pcurve.id == pcurve_id) {
            continue;
        }
        let source_offset = graph.get(17, fin_xmt).map_or(0, |node| node.pos as u64);
        annotations
            .note(&pcurve_id, source_stream, source_offset)
            .tag("INTERSECTION_PCURVE");
        annotations.derived(&pcurve_id, "geometry");
        annotations.derived(&pcurve_id, "parameter_range");
        if fit_tolerance.is_some() {
            annotations.derived(&pcurve_id, "fit_tolerance");
        }
        ir.model.pcurves.push(Pcurve {
            id: pcurve_id.clone(),
            geometry,
            wrapper_reversed: None,
            native_tail_flags: None,
            parameter_range: Some(parameter_range),
            fit_tolerance,
        });
        if let Some(coedge) = ir
            .model
            .coedges
            .iter_mut()
            .find(|coedge| coedge.id == coedge_id && coedge.pcurves.is_empty())
        {
            coedge.pcurves.push(cadmpeg_ir::topology::PcurveUse {
                pcurve: pcurve_id,
                isoparametric: None,
                parameter_range: None,
            });
        }
    }
}
