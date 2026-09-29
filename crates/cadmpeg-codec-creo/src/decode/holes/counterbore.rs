// SPDX-License-Identifier: Apache-2.0
//! Counterbore dimensions, axis placement, and source cylinder geometry.

use crate::decode::axis::Axis;
use crate::vecmath::normalize;
use crate::vecmath::unit_length;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::LinearTermination;
use cadmpeg_ir::geometry::analytic::CylinderSurface;
use cadmpeg_ir::geometry::{SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::scalar::PositiveLength;

use crate::container::ContainerScan;

use super::super::feature_history::dimensions::feature_dimension_table_complete;
use super::super::feature_history::round::unique_surface_parameter_record;
use super::super::sketch::equations_coordinate::approximately_equal;
use super::super::uniqueness::exactly_one;
use super::drilled::paired_corner_envelope_axis_spans;
use crate::decode::analytic::planes::{placed_planes, reconciled_model_plane};

/// General reconstructed counterbore geometry tolerance.
const EPS_COUNTERBORE_GEOMETRY: f64 = 1.0e-9;
/// Exact-geometry threshold for degenerate counterbore lengths.
const EPS_COUNTERBORE_EXACT_GEOMETRY: f64 = 1.0e-12;

fn unique_model_surface_geometries(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
) -> Result<Option<BTreeMap<u32, SurfaceGeometry>>, CodecError> {
    let mut geometries = BTreeMap::new();
    for surface in &ir.model.surfaces {
        let Some(surface_id) = surface
            .id
            .as_str()
            .strip_prefix("creo:visibgeom:surface#")
            .and_then(|id| id.parse::<u32>().ok())
        else {
            continue;
        };
        if geometries.contains_key(&surface_id) {
            return Ok(None);
        }
        ctx.charge_collection_items(1, "creo counterbore model surface nodes")?;
        let geometry = surface.geometry.copy_admitted(ctx, "creo counterbore model geometry")?;
        geometries.insert(surface_id, geometry);
    }
    Ok(Some(geometries))
}

pub(in crate::decode) fn counterbore_dimensions(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> Result<Option<(f64, f64, f64)>, CodecError> {
    let Some(table) = counterbore_entity_table(scan, feature_id) else {
        return Ok(None);
    };
    let mut generated_cylinders = BTreeSet::new();
    for surface_id in table
        .surface_ids_iter()
        .filter(|surface_id| {
            crate::surface::unique_surface_row(&scan.surfaces.rows, *surface_id).is_some_and(
                |row| {
                    row.feature_id == feature_id
                        && row.kind == crate::surface::SurfaceKind::Cylinder
                },
            )
        })
    {
        if !generated_cylinders.contains(&surface_id) {
            ctx.charge_collection_items(1, "creo counterbore generated cylinder nodes")?;
            generated_cylinders.insert(surface_id);
        }
    }
    let Some(existing_geometries) = unique_model_surface_geometries(ctx, ir)? else {
        return Ok(None);
    };
    let mut generated_radii = Vec::new();
    for radius in existing_geometries
        .into_iter()
        .filter_map(|(surface_id, geometry)| {
            generated_cylinders.contains(&surface_id).then_some(())?;
            let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) =
                geometry
            else {
                return None;
            };
            let radius = cylinder_surface.radius().get();
            Some(radius)
        })
    {
        ctx.try_reserve_items(&mut generated_radii, 1, "creo counterbore generated radii")?;
        generated_radii.push(radius);
    }
    let dimension_tables = || {
        scan.features
            .definitions
            .iter()
            .filter(|definition| definition.identity.id() == 911)
            .filter_map(|definition| definition.dimensions.as_ref())
    };
    if let Some(dimensions) = counterbore_dimension_values(dimension_tables(), &generated_radii) {
        return Ok(Some(dimensions));
    }
    let Some(sources) = counterbore_cylinder_sources(ctx, scan, feature_id)? else {
        return Ok(None);
    };
    let [first_source, second_source] = sources.as_slice() else {
        return Ok(None);
    };
    let source_span = |ids: &Vec<u32>| {
        let [first_id, second_id] = ids.as_slice() else {
            return None;
        };
        let envelope = |id: &u32| {
            let row = crate::surface::unique_surface_row(&scan.surfaces.rows, *id)?;
            unique_surface_parameter_record(scan, row)?.type24_terminal_corner_envelope()
        };
        paired_corner_envelope_axis_spans(envelope(first_id)?, envelope(second_id)?)
    };
    let source_spans = [source_span(first_source), source_span(second_source)];
    if source_spans.iter().any(Option::is_some) {
        Ok(counterbore_envelope_dimension_values(dimension_tables(), &source_spans))
    } else {
        Ok(counterbore_unenveloped_dimension_values(dimension_tables()))
    }
}

/// Check whether a cylinder radius is one of the two radii declared by a
/// complete counterbore dimension tuple.
pub(in crate::decode) fn counterbore_dimension_tuple_matches_radius(
    (bore_diameter, counterbore_diameter, _): (f64, f64, f64),
    radius: f64,
) -> bool {
    [0.5 * bore_diameter, 0.5 * counterbore_diameter]
        .into_iter()
        .any(|expected| {
            (FiniteReal::new(radius))
                .zip(FiniteReal::new(expected))
                .is_some_and(|(first, second)| approximately_equal(first, second))
        })
}

pub(in crate::decode) fn counterbore_dimension_values<'a>(
    tables: impl Iterator<Item = &'a crate::feature::definitions::FeatureDimensionTable>,
    generated_radii: &[f64],
) -> Option<(f64, f64, f64)> {
    let mut first = None;
    for table in tables {
        if usize::try_from(table.declared_count).ok() != Some(table.rows.len())
            || table.rows.len() != 4
        {
            continue;
        }
        let value = |external_id, dimension_type| {
            let row = exactly_one(table.rows.iter().filter(|row| {
                row.external_id == external_id && row.dimension_type == dimension_type
            }))?;
            row.value.resolved().filter(|value| value.is_finite())
        };
        let (Some(bore_radius), Some(_placement_distance), Some(depth), Some(counterbore_radius)) =
            (value(0, 2), value(1, 2), value(2, 1), value(3, 2))
        else {
            continue;
        };
        if bore_radius <= 0.0
            || depth == 0.0
            || counterbore_radius <= bore_radius
            || !generated_radii.iter().any(|radius| {
                (*radius - counterbore_radius).abs()
                    <= EPS_COUNTERBORE_GEOMETRY
                        * radius.abs().max(counterbore_radius.abs()).max(1.0)
            })
        {
            continue;
        }
        let bore_diameter = PositiveLength::new(2.0 * bore_radius)?;
        let counterbore_diameter = PositiveLength::new(2.0 * counterbore_radius)?;
        let candidate = (bore_diameter.get(), counterbore_diameter.get(), depth.abs());
        if let Some(first) = first {
            if !counterbore_values_agree(candidate, first) {
                return None;
            }
        } else {
            first = Some(candidate);
        }
    }
    first
}

fn counterbore_values_agree(candidate: (f64, f64, f64), first: (f64, f64, f64)) -> bool {
    [
        candidate.0 - first.0,
        candidate.1 - first.1,
        candidate.2 - first.2,
    ]
    .iter()
    .all(|delta| delta.abs() <= EPS_COUNTERBORE_GEOMETRY)
}

pub(in crate::decode) fn counterbore_envelope_dimension_values<'a>(
    tables: impl Iterator<Item = &'a crate::feature::definitions::FeatureDimensionTable>,
    source_spans: &[Option<[[Option<PositiveLength>; 2]; 3]>],
) -> Option<(f64, f64, f64)> {
    let [first_source, second_source] = source_spans else {
        return None;
    };
    let cylinder_diameter_matches = |diameter: f64, spans: [[Option<PositiveLength>; 2]; 3]| {
        (0..3)
            .filter(|axis| {
                spans[*axis].into_iter().flatten().any(|span| {
                    (FiniteReal::new(span.get()))
                        .zip(FiniteReal::new(diameter))
                        .is_some_and(|(first, second)| approximately_equal(first, second))
                })
            })
            .count()
            == 2
    };
    let counterbore_matches =
        |diameter: f64, depth: f64, spans: [[Option<PositiveLength>; 2]; 3]| {
            let mut diameter_axes = (0..3).filter(|axis| {
                spans[*axis].into_iter().flatten().any(|span| {
                    (FiniteReal::new(span.get()))
                        .zip(FiniteReal::new(diameter))
                        .is_some_and(|(first, second)| approximately_equal(first, second))
                })
            });
            let (Some(first_axis), Some(second_axis), None) = (
                diameter_axes.next(),
                diameter_axes.next(),
                diameter_axes.next(),
            ) else {
                return false;
            };
            (0..3)
                .find(|axis| *axis != first_axis && *axis != second_axis)
                .is_some_and(|axis| {
                    spans[axis].into_iter().flatten().any(|span| {
                        (FiniteReal::new(span.get()))
                            .zip(FiniteReal::new(depth))
                            .is_some_and(|(first, second)| approximately_equal(first, second))
                    })
                })
        };
    let candidates = tables
        .filter_map(|table| {
            let (bore_diameter, counterbore_diameter, counterbore_depth) =
                counterbore_envelope_dimension_tuple(table)?;
            let matches = match (first_source, second_source) {
                (Some(first), Some(second)) => {
                    [
                        cylinder_diameter_matches(bore_diameter, *first)
                            && counterbore_matches(
                                counterbore_diameter,
                                counterbore_depth,
                                *second,
                            ),
                        cylinder_diameter_matches(bore_diameter, *second)
                            && counterbore_matches(counterbore_diameter, counterbore_depth, *first),
                    ]
                    .into_iter()
                    .filter(|matches| *matches)
                    .count()
                        == 1
                }
                (Some(spans), None) | (None, Some(spans)) => {
                    cylinder_diameter_matches(bore_diameter, *spans)
                        != counterbore_matches(counterbore_diameter, counterbore_depth, *spans)
                }
                (None, None) => false,
            };
            matches.then_some((bore_diameter, counterbore_diameter, counterbore_depth))
        });
    unique_counterbore_dimension_tuple(candidates)
}

pub(in crate::decode) fn counterbore_unenveloped_dimension_values<'a>(
    tables: impl Iterator<Item = &'a crate::feature::definitions::FeatureDimensionTable>,
) -> Option<(f64, f64, f64)> {
    let mut candidates = tables
        .filter(|table| {
            feature_dimension_table_complete(table) && matches!(table.rows.len(), 4 | 5)
        })
        .map(counterbore_envelope_dimension_tuple);
    let first = candidates.next()??;
    for candidate in candidates {
        if !counterbore_tuples_approximately_equal(candidate?, first) {
            return None;
        }
    }
    Some(first)
}

fn counterbore_envelope_dimension_tuple(
    table: &crate::feature::definitions::FeatureDimensionTable,
) -> Option<(f64, f64, f64)> {
    (feature_dimension_table_complete(table) && matches!(table.rows.len(), 4 | 5)).then_some(())?;
    let value = |external_id, dimension_type| {
        let row = exactly_one(table.rows.iter().filter(|row| {
            row.external_id == external_id && row.dimension_type == dimension_type
        }))?;
        row.value.resolved().filter(|value| value.is_finite())
    };
    let signed_counterbore_depth = value(0, 1)?;
    let bore_radius = value(1, 2)?;
    let (counterbore_radius, _placement_distance) = if table.rows.len() == 4 {
        let shifted = value(2, 2).zip(value(3, 2));
        let retained = value(3, 2).zip(value(4, 2));
        match (shifted, retained) {
            (Some(layout), None) | (None, Some(layout)) => layout,
            _ => return None,
        }
    } else {
        let drill_point_angle = value(2, 10)?;
        (drill_point_angle > 0.0 && drill_point_angle < std::f64::consts::PI).then_some(())?;
        (value(3, 2)?, value(4, 2)?)
    };
    (signed_counterbore_depth != 0.0 && bore_radius > 0.0 && counterbore_radius > bore_radius)
        .then_some(())?;
    Some((
        PositiveLength::new(2.0 * bore_radius)?.get(),
        PositiveLength::new(2.0 * counterbore_radius)?.get(),
        signed_counterbore_depth.abs(),
    ))
}

fn unique_counterbore_dimension_tuple(
    candidates: impl Iterator<Item = (f64, f64, f64)>,
) -> Option<(f64, f64, f64)> {
    let mut candidates = candidates;
    let first = candidates.next()?;
    candidates
        .all(|candidate| counterbore_tuples_approximately_equal(candidate, first))
        .then_some(first)
}

fn counterbore_tuples_approximately_equal(
    candidate: (f64, f64, f64),
    first: (f64, f64, f64),
) -> bool {
    [candidate.0, candidate.1, candidate.2]
        .into_iter()
        .zip([first.0, first.1, first.2])
        .all(|(candidate, first)| {
            (FiniteReal::new(candidate))
                .zip(FiniteReal::new(first))
                .is_some_and(|(first, second)| approximately_equal(first, second))
        })
}

pub(in crate::decode) fn counterbore_patch_geometries<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan<'_>,
    ir: &CadIr,
    feature_id: u32,
) -> Result<Option<Vec<(&'a crate::surface::SurfaceRow, CylinderSurface)>>, CodecError> {
    let resolve_rows = |geometries: Vec<(u32, CylinderSurface)>| -> Result<_, CodecError> {
        let mut rows = Vec::new();
        for (id, geometry) in geometries {
            let Some(row) = crate::surface::unique_surface_row(&scan.surfaces.rows, id) else {
                return Ok(None);
            };
            ctx.try_reserve_items(&mut rows, 1, "creo counterbore patch rows")?;
            rows.push((row, geometry));
        }
        Ok(Some(rows))
    };
    let Some((bore_diameter, counterbore_diameter, counterbore_depth)) =
        counterbore_dimensions(ctx, scan, ir, feature_id)? else {
        return Ok(None);
    };
    let Some(cylinder_sources) = counterbore_cylinder_sources(ctx, scan, feature_id)? else {
        return Ok(None);
    };
    let Some(existing_geometries) = unique_model_surface_geometries(ctx, ir)? else {
        return Ok(None);
    };
    if let Some(geometries) = counterbore_source_patch_geometries(
        ctx,
        &cylinder_sources,
        &existing_geometries,
        bore_diameter,
        counterbore_diameter,
    )? {
        return resolve_rows(geometries);
    }
    if cylinder_sources
        .iter()
        .flatten()
        .any(|id| existing_geometries.contains_key(id))
    {
        return Ok(None);
    }
    let Some(source_corners) = counterbore_source_corner_envelopes(ctx, scan, &cylinder_sources)? else {
        return Ok(None);
    };
    let Some(geometries) = counterbore_source_corner_patch_geometries(
        ctx,
        &cylinder_sources,
        &source_corners,
        bore_diameter,
        counterbore_diameter,
        counterbore_depth,
    )? else {
        return Ok(None);
    };
    resolve_rows(geometries)
}

pub(in crate::decode) fn counterbore_cylinder_sources(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    feature_id: u32,
) -> Result<Option<Vec<Vec<u32>>>, CodecError> {
    let Some(table) = counterbore_entity_table(scan, feature_id) else {
        return Ok(None);
    };
    let mut cylinders_by_source = BTreeMap::<u32, Vec<u32>>::new();
    for entry in table.entries.iter().filter(|entry| entry.class_id() == 200) {
        if !table.contains_surface_id(entry.entity_id) {
            continue;
        }
        let Some(source_id) = entry.source_entity_id() else {
            return Ok(None);
        };
        let Some(row) = crate::surface::unique_surface_row(&scan.surfaces.rows, entry.entity_id)
        else {
            continue;
        };
        if row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Cylinder {
            if !cylinders_by_source.contains_key(&source_id) {
                ctx.charge_collection_items(1, "creo counterbore source nodes")?;
            }
            let ids = cylinders_by_source.entry(source_id).or_default();
            ctx.try_reserve_items(ids, 1, "creo counterbore source cylinder IDs")?;
            ids.push(entry.entity_id);
        }
    }
    let mut sources = Vec::new();
    for (_, ids) in cylinders_by_source {
        if ids.len() == 2 {
            ctx.try_reserve_items(&mut sources, 1, "creo counterbore source groups")?;
            sources.push(ids);
        }
    }
    Ok(Some(sources))
}

fn counterbore_source_corner_envelopes(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    sources: &[Vec<u32>],
) -> Result<Option<Vec<[[[f64; 3]; 2]; 2]>>, CodecError> {
    let mut envelopes = Vec::new();
    for ids in sources {
        let candidate = (|| {
            let [first_id, second_id] = ids.as_slice() else {
                return None;
            };
            let envelope = |id| {
                let row = crate::surface::unique_surface_row(&scan.surfaces.rows, id)?;
                unique_surface_parameter_record(scan, row)?.type24_terminal_corner_envelope()
            };
            Some([envelope(*first_id)?, envelope(*second_id)?])
        })();
        let Some(candidate) = candidate else {
            return Ok(None);
        };
        ctx.try_reserve_items(&mut envelopes, 1, "creo counterbore source corner envelopes")?;
        envelopes.push(candidate);
    }
    Ok(Some(envelopes))
}

fn counterbore_entity_table<'a>(
    scan: &'a ContainerScan<'_>,
    feature_id: u32,
) -> Option<&'a crate::feature::entity::FeatureEntityTable> {
    exactly_one(scan
        .features
        .entity_tables
        .iter()
        .filter(|table| table.feature_id == feature_id && table.table_class_id == 29)
        .filter(|table| {
            table.entries.iter().any(|entry| {
                entry.source_entity_id().is_some()
                    && table.contains_surface_id(entry.entity_id)
                    && crate::surface::unique_surface_row(&scan.surfaces.rows, entry.entity_id)
                        .is_some_and(|row| {
                            row.feature_id == feature_id
                                && row.kind == crate::surface::SurfaceKind::Cylinder
                        })
            })
        }))
}

pub(in crate::decode) fn counterbore_axis_placement(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> Result<Option<cadmpeg_ir::features::holes::HolePlacement>, CodecError> {
    let cylinder_axis = if let Some((_, counterbore_diameter, _)) =
        counterbore_dimensions(ctx, scan, ir, feature_id)?
    {
        if let Some(sources) = counterbore_cylinder_sources(ctx, scan, feature_id)? {
            unique_model_surface_geometries(ctx, ir)?.and_then(|geometries| {
                counterbore_axis_placement_from_sources(&sources, &geometries, counterbore_diameter)
            })
        } else {
            None
        }
    } else {
        None
    };
    Ok(cylinder_axis.or_else(|| {
        counterbore_support_axis_placement(
            feature_id,
            counterbore_entity_table(scan, feature_id)?,
            &scan.surfaces.rows,
            &scan.planes.local_systems,
        )
    }))
}

pub(in crate::decode) fn counterbore_support_axis_placement(
    feature_id: u32,
    table: &crate::feature::entity::FeatureEntityTable,
    rows: &[crate::surface::SurfaceRow],
    frames: &[crate::surface::PlaneLocalSystem],
) -> Option<cadmpeg_ir::features::holes::HolePlacement> {
    (table.feature_id == feature_id).then_some(())?;
    let plane_id = exactly_one(table
        .surface_ids_iter()
        .filter(|surface_id| {
            crate::surface::unique_surface_row(rows, *surface_id).is_some_and(|row| {
                row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Plane
            })
        }))?;
    let frame = exactly_one(frames
        .iter()
        .filter(|frame| frame.surface_id == plane_id))?;
    let frame = frame.frame();
    let origin = frame.origin?;
    Some(cadmpeg_ir::features::holes::HolePlacement::Axis {
        origin: cadmpeg_ir::features::FinitePoint3::new(Point3::from(origin))?,
        axis: frame.normal?.into(),
    })
}

pub(in crate::decode) fn counterbore_axis_placement_from_sources(
    cylinder_sources: &[Vec<u32>],
    existing_geometries: &BTreeMap<u32, SurfaceGeometry>,
    counterbore_diameter: f64,
) -> Option<cadmpeg_ir::features::holes::HolePlacement> {
    let carrier = exactly_one(cylinder_sources
        .iter()
        .filter_map(|ids| {
            complete_cylinder_source_carrier(ids, existing_geometries, 0.5 * counterbore_diameter)
        }))?;
    Some(cadmpeg_ir::features::holes::HolePlacement::Axis {
        origin: carrier.origin(),
        axis: cadmpeg_ir::features::FeatureDirection3::from(*carrier.frame().axis()),
    })
}

pub(in crate::decode) fn counterbore_directed_placement(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    feature_id: u32,
) -> Result<Option<(Option<u32>, Point3, Vector3, LinearTermination)>, CodecError> {
    let Some((bore_diameter, counterbore_diameter, counterbore_depth)) =
        counterbore_dimensions(ctx, scan, ir, feature_id)? else {
        return Ok(None);
    };
    let Some(sources) = counterbore_cylinder_sources(ctx, scan, feature_id)? else {
        return Ok(None);
    };
    let [first, second] = sources.as_slice() else {
        return Ok(None);
    };
    let boundary = |ids: &[u32], radius: f64| {
        counterbore_source_boundary_circle(ctx, scan, ir, source_carriers, feature_id, ids, radius)
    };
    let bore_radius = 0.5 * bore_diameter;
    let counterbore_radius = 0.5 * counterbore_diameter;
    let boundaries = (
        boundary(first, counterbore_radius)?,
        boundary(first, bore_radius)?,
        boundary(second, counterbore_radius)?,
        boundary(second, bore_radius)?,
    );
    let boundary_placement = match boundaries {
        (Some(counterbore), None, None, Some(bore))
        | (None, Some(bore), Some(counterbore), None) => {
            counterbore_directed_span(counterbore, bore, counterbore_depth).map(
                |(face, position, direction, extent)| (Some(face), position, direction, extent),
            )
        }
        _ => None,
    };
    if boundary_placement.is_some() {
        return Ok(boundary_placement);
    }
    let Some(source_corners) = counterbore_source_corner_envelopes(ctx, scan, &sources)? else {
        return Ok(None);
    };
    Ok(counterbore_placement_from_corner_envelopes(
            &source_corners,
            bore_diameter,
            counterbore_diameter,
            counterbore_depth,
        )
        .map(|(position, direction, extent)| (None, position, direction, extent)))
}

#[derive(Debug, Clone, Copy)]
struct CounterboreEnvelopeLayout {
    axis: Axis,
    center: [f64; 3],
    axial_interval: [f64; 2],
}

#[derive(Debug, Clone, Copy)]
struct CounterboreCornerAssignment {
    bore_source: usize,
    bore: CounterboreEnvelopeLayout,
    position: Point3,
    direction: Vector3,
    length: f64,
}

fn counterbore_source_envelope_layout(
    corners: [[[f64; 3]; 2]; 2],
    diameter: f64,
    axial_depth: Option<f64>,
    scale: f64,
) -> Option<CounterboreEnvelopeLayout> {
    let close = |left: f64, right: f64| (left - right).abs() <= EPS_COUNTERBORE_GEOMETRY * scale;
    let intervals = corners.map(|patch| {
        std::array::from_fn::<_, 3, _>(|axis| {
            [
                patch[0][axis].min(patch[1][axis]),
                patch[0][axis].max(patch[1][axis]),
            ]
        })
    });
    let shared = |axis: Axis| {
        close(intervals[0][axis.index()][0], intervals[1][axis.index()][0])
            && close(intervals[0][axis.index()][1], intervals[1][axis.index()][1])
    };
    let adjacent = |axis: Axis| {
        close(intervals[0][axis.index()][1], intervals[1][axis.index()][0])
            || close(intervals[1][axis.index()][1], intervals[0][axis.index()][0])
    };
    let union = |axis: Axis| {
        [
            intervals[0][axis.index()][0].min(intervals[1][axis.index()][0]),
            intervals[0][axis.index()][1].max(intervals[1][axis.index()][1]),
        ]
    };
    let mut diameter_axes = Axis::ALL
        .into_iter()
        .filter(|axis| {
            let union = union(*axis);
            (shared(*axis) || adjacent(*axis)) && close(union[1] - union[0], diameter)
        });
    let (Some(first_radial), Some(second_radial), None) = (
        diameter_axes.next(),
        diameter_axes.next(),
        diameter_axes.next(),
    ) else {
        return None;
    };
    let axis = Axis::ALL
        .into_iter()
        .find(|axis| *axis != first_radial && *axis != second_radial)?;
    shared(axis).then_some(())?;
    let axial_interval = intervals[0][axis.index()];
    let axial_span = axial_interval[1] - axial_interval[0];
    (axial_span > 0.0 && axial_depth.is_none_or(|depth| close(axial_span, depth))).then_some(())?;
    let mut center = [0.0; 3];
    for radial_axis in [first_radial, second_radial] {
        let bounds = union(radial_axis);
        center[radial_axis.index()] = f64::midpoint(bounds[0], bounds[1]);
    }
    Some(CounterboreEnvelopeLayout {
        axis,
        center,
        axial_interval,
    })
}

pub(in crate::decode) fn counterbore_placement_from_corner_envelopes(
    source_corners: &[[[[f64; 3]; 2]; 2]],
    bore_diameter: f64,
    counterbore_diameter: f64,
    counterbore_depth: f64,
) -> Option<(Point3, Vector3, LinearTermination)> {
    let assignment = counterbore_corner_assignment(
        source_corners,
        bore_diameter,
        counterbore_diameter,
        counterbore_depth,
    )?;
    Some((
        assignment.position,
        assignment.direction,
        LinearTermination::Blind {
            length: cadmpeg_ir::scalar::NonZeroLength::new(assignment.length)?,
        },
    ))
}

fn counterbore_corner_assignment(
    source_corners: &[[[[f64; 3]; 2]; 2]],
    bore_diameter: f64,
    counterbore_diameter: f64,
    counterbore_depth: f64,
) -> Option<CounterboreCornerAssignment> {
    let [first_source, second_source] = source_corners else {
        return None;
    };
    let scale = source_corners
        .iter()
        .flatten()
        .flatten()
        .flatten()
        .chain([&bore_diameter, &counterbore_diameter, &counterbore_depth])
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    source_corners
        .iter()
        .flatten()
        .flatten()
        .flatten()
        .all(|value| value.is_finite())
        .then_some(())?;
    (bore_diameter.is_finite()
        && counterbore_diameter.is_finite()
        && counterbore_depth.is_finite()
        && bore_diameter > 0.0
        && counterbore_diameter > bore_diameter
        && counterbore_depth > 0.0)
        .then_some(())?;
    let close = |left: f64, right: f64| (left - right).abs() <= EPS_COUNTERBORE_GEOMETRY * scale;
    let assignment = exactly_one([
        (
            0,
            counterbore_source_envelope_layout(*first_source, bore_diameter, None, scale),
            counterbore_source_envelope_layout(
                *second_source,
                counterbore_diameter,
                Some(counterbore_depth),
                scale,
            ),
        ),
        (
            1,
            counterbore_source_envelope_layout(*second_source, bore_diameter, None, scale),
            counterbore_source_envelope_layout(
                *first_source,
                counterbore_diameter,
                Some(counterbore_depth),
                scale,
            ),
        ),
    ]
    .into_iter()
    .filter_map(|(bore_source, bore, counterbore)| Some((bore_source, bore?, counterbore?))))?;
    let (bore_source, bore, counterbore) = assignment;
    (bore.axis == counterbore.axis).then_some(())?;
    bore.axis
        .complement()
        .iter()
        .all(|axis| close(bore.center[axis.index()], counterbore.center[axis.index()]))
        .then_some(())?;
    let (entry, direction_sign, length) =
        if close(counterbore.axial_interval[1], bore.axial_interval[0]) {
            (
                counterbore.axial_interval[0],
                1.0,
                bore.axial_interval[1] - counterbore.axial_interval[0],
            )
        } else if close(bore.axial_interval[1], counterbore.axial_interval[0]) {
            (
                counterbore.axial_interval[1],
                -1.0,
                counterbore.axial_interval[1] - bore.axial_interval[0],
            )
        } else {
            return None;
        };
    (length > counterbore_depth && length.is_finite()).then_some(())?;
    let mut position = counterbore.center;
    position[counterbore.axis.index()] = entry;
    let mut direction = [0.0; 3];
    direction[counterbore.axis.index()] = direction_sign;
    Some(CounterboreCornerAssignment {
        bore_source,
        bore,
        position: Point3::from(position),
        direction: Vector3::from(direction),
        length,
    })
}

pub(in crate::decode) fn counterbore_directed_span(
    counterbore: (u32, Point3, [f64; 3]),
    bore: (u32, Point3, [f64; 3]),
    counterbore_depth: f64,
) -> Option<(u32, Point3, Vector3, LinearTermination)> {
    let delta = [
        bore.1.x - counterbore.1.x,
        bore.1.y - counterbore.1.y,
        bore.1.z - counterbore.1.z,
    ];
    let length = delta.iter().map(|value| value * value).sum::<f64>().sqrt();
    let scale = [
        counterbore.1.x,
        counterbore.1.y,
        counterbore.1.z,
        bore.1.x,
        bore.1.y,
        bore.1.z,
        counterbore_depth,
    ]
    .into_iter()
    .map(f64::abs)
    .fold(1.0, f64::max);
    (length.is_finite()
        && length > EPS_COUNTERBORE_EXACT_GEOMETRY * scale
        && counterbore_depth <= length + EPS_COUNTERBORE_GEOMETRY * scale)
        .then_some(())?;
    let direction = delta.map(|value| value / length);
    [counterbore.2, bore.2]
        .iter()
        .all(|axis| {
            let alignment = direction
                .iter()
                .zip(axis)
                .map(|(left, right)| left * right)
                .sum::<f64>()
                .abs();
            (alignment - 1.0).abs() <= EPS_COUNTERBORE_GEOMETRY
        })
        .then_some(())?;
    Some((
        counterbore.0,
        counterbore.1,
        Vector3::from(direction),
        LinearTermination::Blind {
            length: cadmpeg_ir::scalar::NonZeroLength::new(length)?,
        },
    ))
}

fn counterbore_source_boundary_circle(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    feature_id: u32,
    cylinder_ids: &[u32],
    radius: f64,
) -> Result<Option<(u32, Point3, [f64; 3])>, CodecError> {
    let local_planes = placed_planes(ctx, scan)?;
    let unique_edges = crate::identity::uniquely_identified_rows_checked(
        ctx,
        &scan.curves.topology_rows,
        |row| row.id,
    )?;
    Ok((|| {
    let boundary_for = |cylinder_id| {
        exactly_one(unique_edges.iter().copied()
            .filter_map(|edge| {
                (edge.feature_id == feature_id && edge.type_byte == 0).then_some(())?;
                let cylinder = std::num::NonZeroU32::new(cylinder_id)?;
                let other = match edge.faces {
                    [Some(left), Some(right)] if left == cylinder => right.get(),
                    [Some(left), Some(right)] if right == cylinder => left.get(),
                    _ => return None,
                };
                let plane = crate::surface::unique_surface_row(&scan.surfaces.rows, other)?;
                (plane.kind == crate::surface::SurfaceKind::Plane).then_some(())?;
                let curve = exactly_one(ir.model.curves.iter().filter(|curve| {
                    crate::identity::matches_numbered_identity(
                        curve.id.as_str(), "creo:visibgeom:curve#", edge.id,
                    )
                }))?;
                let Some(SolvedCurveGeometry::Circle(circle_curve)) =
                    source_carriers.curve_geometry(curve).solved()
                else {
                    return None;
                };
                let center = circle_curve.center().get();
                let candidate = circle_curve.radius().get();
                ((candidate - radius).abs() <= EPS_COUNTERBORE_GEOMETRY).then_some(())?;
                let axis = unit_length(*circle_curve.frame().axis());
                let plane = reconciled_model_plane(&local_planes, ir, source_carriers, other)?;
                let normal = normalize(plane.normal)?;
                let alignment = axis
                    .iter()
                    .zip(normal)
                    .map(|(left, right)| left * right)
                    .sum::<f64>()
                    .abs();
                let distance = [
                    center.x - plane.origin[0],
                    center.y - plane.origin[1],
                    center.z - plane.origin[2],
                ]
                .iter()
                .zip(normal)
                .map(|(delta, normal)| delta * normal)
                .sum::<f64>()
                .abs();
                let scale = [
                    center.x,
                    center.y,
                    center.z,
                    plane.origin[0],
                    plane.origin[1],
                    plane.origin[2],
                    radius,
                ]
                .into_iter()
                .map(f64::abs)
                .fold(1.0, f64::max);
                ((alignment - 1.0).abs() <= EPS_COUNTERBORE_GEOMETRY
                    && distance <= EPS_COUNTERBORE_GEOMETRY * scale)
                    .then_some(())?;
                Some((other, center, axis))
            }))
    };
    let mut boundaries = cylinder_ids.iter().copied().map(boundary_for);
    let first = boundaries.next()??;
    for candidate in boundaries {
        let candidate = candidate?;
        if !(candidate.0 == first.0
            && candidate.1 == first.1
            && candidate
                .2
                .iter()
                .zip(first.2)
                .map(|(left, right)| left * right)
                .sum::<f64>()
                .abs()
                >= 1.0 - EPS_COUNTERBORE_GEOMETRY) {
            return None;
        }
    }
    Some(first)
    })())
}

pub(in crate::decode) fn counterbore_source_patch_geometries(
    ctx: &DecodeContext<'_>,
    cylinder_sources: &[Vec<u32>],
    existing_geometries: &BTreeMap<u32, SurfaceGeometry>,
    bore_diameter: f64,
    counterbore_diameter: f64,
) -> Result<Option<Vec<(u32, CylinderSurface)>>, CodecError> {
    let candidate = (|| {
        let [first_source, second_source] = cylinder_sources else {
            return None;
        };
    let counterbore_radius = 0.5 * counterbore_diameter;
    let has_observed_geometry =
        |source: &[u32]| source.iter().any(|id| existing_geometries.contains_key(id));
    let (counterbore_source, bore_source, carrier) = match (
        complete_cylinder_source_carrier(first_source, existing_geometries, counterbore_radius),
        complete_cylinder_source_carrier(second_source, existing_geometries, counterbore_radius),
    ) {
        (Some(carrier), None) if !has_observed_geometry(second_source) => {
            (first_source, second_source, carrier)
        }
        (None, Some(carrier)) if !has_observed_geometry(first_source) => {
            (second_source, first_source, carrier)
        }
        _ => return None,
    };
    let geometry = |radius| {
        Some(CylinderSurface::new(
            carrier.origin(),
            *carrier.frame(),
            cadmpeg_ir::scalar::PositiveLength::new(radius)?,
        ))
    };
    let counterbore_geometry = geometry(counterbore_radius)?;
    let bore_geometry = geometry(0.5 * bore_diameter)?;
        Some((counterbore_source, bore_source, counterbore_geometry, bore_geometry))
    })();
    let Some((counterbore_source, bore_source, counterbore_geometry, bore_geometry)) = candidate else {
        return Ok(None);
    };
    let mut patches = Vec::new();
    for (id, geometry) in counterbore_source
        .iter()
        .map(|id| (*id, counterbore_geometry))
        .chain(bore_source.iter().map(|id| (*id, bore_geometry)))
    {
        ctx.try_reserve_items(&mut patches, 1, "creo counterbore source patches")?;
        patches.push((id, geometry));
    }
    Ok(Some(patches))
}

fn counterbore_source_corner_patch_geometries(
    ctx: &DecodeContext<'_>,
    cylinder_sources: &[Vec<u32>],
    source_corners: &[[[[f64; 3]; 2]; 2]],
    bore_diameter: f64,
    counterbore_diameter: f64,
    counterbore_depth: f64,
) -> Result<Option<Vec<(u32, CylinderSurface)>>, CodecError> {
    let candidate = (|| {
        let [first_source, second_source] = cylinder_sources else {
            return None;
        };
    if first_source.len() != 2
        || second_source.len() != 2
        || [first_source[0], first_source[1], second_source[0], second_source[1]]
            .into_iter()
            .enumerate()
            .any(|(index, id)| first_source.iter().chain(second_source).take(index).any(|previous| *previous == id))
    {
        return None;
    }
    let assignment = counterbore_corner_assignment(
        source_corners,
        bore_diameter,
        counterbore_diameter,
        counterbore_depth,
    )?;
    let mut ref_direction = [0.0; 3];
    ref_direction[assignment.bore.axis.complement()[0].index()] = 1.0;
    let geometry = |radius| {
        CylinderSurface::try_new(
            assignment.position,
            assignment.direction,
            Vector3::from(ref_direction),
            radius,
        )
        .ok()
    };
    let bore_geometry = geometry(0.5 * bore_diameter)?;
    let counterbore_geometry = geometry(0.5 * counterbore_diameter)?;
        Some((first_source, second_source, assignment, bore_geometry, counterbore_geometry))
    })();
    let Some((first_source, second_source, assignment, bore_geometry, counterbore_geometry)) = candidate else {
        return Ok(None);
    };
    let mut patches = Vec::new();
    for (id, geometry) in [first_source, second_source]
            .into_iter()
            .enumerate()
            .flat_map(|(source_index, ids)| {
                let geometry = if source_index == assignment.bore_source {
                    bore_geometry
                } else {
                    counterbore_geometry
                };
                ids.iter().copied().map(move |id| (id, geometry))
            })
    {
        ctx.try_reserve_items(&mut patches, 1, "creo counterbore corner patches")?;
        patches.push((id, geometry));
    }
    Ok(Some(patches))
}

fn complete_cylinder_source_carrier(
    ids: &[u32],
    existing_geometries: &BTreeMap<u32, SurfaceGeometry>,
    radius: f64,
) -> Option<CylinderSurface> {
    let mut carriers = ids.iter().map(|id| existing_geometries.get(id));
    let first = carriers.next()??;
    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder)) = first else {
        return None;
    };
    ((cylinder.radius().get() - radius).abs() <= EPS_COUNTERBORE_GEOMETRY
        && carriers.all(|candidate| candidate == Some(first)))
    .then_some(*cylinder)
}

#[cfg(test)]
mod tests;
