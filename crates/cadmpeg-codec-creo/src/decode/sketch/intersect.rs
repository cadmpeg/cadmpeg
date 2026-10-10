// SPDX-License-Identifier: Apache-2.0
//! Section carrier intersection, trim vertices, and coordinate reconciliation.

use super::axis::SectionAxis;

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::features::FiniteVector3;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::scalar::Angle;
use cadmpeg_ir::sketches::{SketchGeometry, SketchGeometryDefinition};

use super::geometry::{
    resolved_section_segment_geometry_with_missing_line, saved_section_arc_carrier,
    saved_section_arc_record, saved_section_missing_line_geometry,
};
use super::radii::{
    section_arc_carrier, section_segment_intersection_carrier_with_missing_line, trim_segment_ids,
};
use super::skamp::section_line_entity_fixed_coordinate_with_unique_rows;

/// General reconstructed sketch-intersection geometry tolerance.
const EPS_SKETCH_INTERSECTION_GEOMETRY: f64 = 1.0e-9;
/// Threshold for degenerate sketch-intersection configurations.
const EPS_SKETCH_INTERSECTION_DEGENERATE: f64 = 1.0e-10;
/// Exact-geometry threshold for sketch-intersection calculations.
const EPS_SKETCH_INTERSECTION_EXACT_GEOMETRY: f64 = 1.0e-12;

fn section_line_origin_direction(geometry: &SketchGeometry) -> Option<(Point2, Point2)> {
    match geometry.definition() {
        SketchGeometryDefinition::Line { start, end } => {
            Some((start.get(), Point2::new(end.u - start.u, end.v - start.v)))
        }
        SketchGeometryDefinition::ReferenceLine { origin, direction } => {
            Some((origin.get(), direction.get()))
        }
        _ => None,
    }
}

pub(in crate::decode) fn intersect_section_lines(
    first: &SketchGeometry,
    second: &SketchGeometry,
) -> Option<[f64; 2]> {
    let (first_origin, first_span) = section_line_origin_direction(first)?;
    let (second_origin, second_span) = section_line_origin_direction(second)?;
    let first_direction = FiniteVector3::new(cadmpeg_ir::math::Vector3::new(
        first_span.u,
        first_span.v,
        0.0,
    ))?
    .unit_nonzero()?;
    let second_direction = FiniteVector3::new(cadmpeg_ir::math::Vector3::new(
        second_span.u,
        second_span.v,
        0.0,
    ))?
    .unit_nonzero()?;
    let determinant = first_direction
        .x
        .mul_add(second_direction.y, -first_direction.y * second_direction.x);
    if determinant.abs() <= EPS_SKETCH_INTERSECTION_EXACT_GEOMETRY {
        return None;
    }
    let delta = Point2::new(
        second_origin.u - first_origin.u,
        second_origin.v - first_origin.v,
    );
    let parameter = delta
        .u
        .mul_add(second_direction.y, -delta.v * second_direction.x)
        / determinant;
    let point = [
        first_origin.u + parameter * first_direction.x,
        first_origin.v + parameter * first_direction.y,
    ];
    point.iter().all(|value| value.is_finite()).then_some(point)
}

pub(in crate::decode) fn intersect_section_line_arc(
    first: &SketchGeometry,
    second: &SketchGeometry,
) -> Option<[f64; 2]> {
    let (
        (line @ SketchGeometryDefinition::Line { .. }, arc @ SketchGeometryDefinition::Arc { .. })
        | (arc @ SketchGeometryDefinition::Arc { .. }, line @ SketchGeometryDefinition::Line { .. }),
    ) = ((first.definition(), second.definition()),)
    else {
        return None;
    };
    let SketchGeometryDefinition::Line { start, end } = line else {
        return None;
    };
    let SketchGeometryDefinition::Arc { center, radius, .. } = arc else {
        return None;
    };
    if (end.u - start.u).hypot(end.v - start.v) <= EPS_SKETCH_INTERSECTION_EXACT_GEOMETRY
        || radius.get() <= EPS_SKETCH_INTERSECTION_EXACT_GEOMETRY
    {
        return None;
    }
    let intersections = cadmpeg_ir::math::planar::line_circle_intersections(
        start.get(),
        end.get(),
        center.get(),
        radius.get(),
    )?;
    let endpoint_tolerance = EPS_SKETCH_INTERSECTION_DEGENERATE * radius.get();
    let mut inside = intersections.into_iter().filter(|(_, point)| {
        cadmpeg_ir::math::planar::point_segment_distance(*point, *start, *end) <= endpoint_tolerance
    });
    let (_, point) = inside.next()?;
    if inside.next().is_some_and(|(_, other)| other != point) {
        return None;
    }
    let radial = (point.u - center.u).hypot(point.v - center.v);
    ((radial - radius.get()).abs() <= endpoint_tolerance).then_some([point.u, point.v])
}

pub(in crate::decode) fn intersect_tangent_section_arcs(
    first: &SketchGeometry,
    second: &SketchGeometry,
) -> Option<[f64; 2]> {
    let (
        SketchGeometryDefinition::Arc {
            center: first_center,
            radius: first_radius,
            ..
        },
        SketchGeometryDefinition::Arc {
            center: second_center,
            radius: second_radius,
            ..
        },
    ) = (first.definition(), second.definition())
    else {
        return None;
    };
    if first_radius.get() <= EPS_SKETCH_INTERSECTION_EXACT_GEOMETRY
        || second_radius.get() <= EPS_SKETCH_INTERSECTION_EXACT_GEOMETRY
    {
        return None;
    }
    let delta = [
        second_center.u - first_center.u,
        second_center.v - first_center.v,
    ];
    let distance = delta[0].hypot(delta[1]);
    let scale = distance.max(first_radius.get()).max(second_radius.get());
    if !scale.is_finite() || distance <= EPS_SKETCH_INTERSECTION_EXACT_GEOMETRY * scale {
        return None;
    }
    let d = distance / scale;
    let r = first_radius.get() / scale;
    let s = second_radius.get() / scale;
    let offset = 0.5 * (d + (r - s) * (r + s) / d);
    let height_squared = (r - offset) * (r + offset);
    if !height_squared.is_finite() || height_squared.abs() > EPS_SKETCH_INTERSECTION_GEOMETRY {
        return None;
    }
    let point = [
        first_center.u + (offset * (delta[0] / distance)) * scale,
        first_center.v + (offset * (delta[1] / distance)) * scale,
    ];
    let on_circle = |center: &Point2, radius: f64| {
        ((point[0] - center.u).hypot(point[1] - center.v) - radius).abs()
            <= EPS_SKETCH_INTERSECTION_GEOMETRY * radius
    };
    (point.iter().all(|value| value.is_finite())
        && on_circle(first_center, first_radius.get())
        && on_circle(second_center, second_radius.get()))
    .then_some(point)
}

fn intersect_section_carriers(first: &SketchGeometry, second: &SketchGeometry) -> Option<[f64; 2]> {
    let line_arc_is_bounded = matches!(
        (first.definition(), second.definition()),
        (
            SketchGeometryDefinition::Line { .. },
            SketchGeometryDefinition::Arc { .. }
        ) | (
            SketchGeometryDefinition::Arc { .. },
            SketchGeometryDefinition::Line { .. }
        )
    );
    intersect_section_lines(first, second)
        .or_else(|| {
            line_arc_is_bounded
                .then(|| intersect_section_line_arc(first, second))
                .flatten()
        })
        .or_else(|| intersect_tangent_section_arcs(first, second))
}

pub(in crate::decode) fn intersect_incident_section_carriers<
    T: std::borrow::Borrow<SketchGeometry>,
>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    carriers: &[T],
) -> Result<Option<[f64; 2]>, cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    if carriers.len() < 2 {
        return Ok(None);
    }
    let mut first_coordinate: Option<[f64; 2]> = None;
    let mut scale = 1.0_f64;
    let mut maximum_distance = 0.0_f64;
    let mut first_carriers = carriers[..carriers.len() - 1].iter().enumerate();
    while first_carriers.len() != 0 {
        let Some((first_index, first)) =
            ctx.next_charged(&mut first_carriers, "creo incident section first carriers")?
        else {
            break;
        };
        let mut second_carriers = carriers[first_index + 1..].iter();
        while second_carriers.len() != 0 {
            let Some(second) = ctx.next_charged(
                &mut second_carriers,
                "creo incident section second carriers",
            )? else {
                break;
            };
            let Some(coordinate) = intersect_section_carriers(first.borrow(), second.borrow())
            else {
                return Ok(None);
            };
            scale = scale.max(coordinate[0].abs()).max(coordinate[1].abs());
            if let Some(first_coordinate) = first_coordinate {
                maximum_distance = maximum_distance.max(
                    (coordinate[0] - first_coordinate[0])
                        .hypot(coordinate[1] - first_coordinate[1]),
                );
            } else {
                first_coordinate = Some(coordinate);
            }
        }
    }
    Ok(
        (maximum_distance <= EPS_SKETCH_INTERSECTION_GEOMETRY * scale)
            .then_some(first_coordinate)
            .flatten(),
    )
}

pub(in crate::decode) fn resolved_trim_vertex_coordinates(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    points: &BTreeMap<u32, [f64; 2]>,
    radii: &BTreeMap<u32, f64>,
) -> Result<BTreeMap<u32, [f64; 2]>, cadmpeg_core::CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo resolved trim vertex coordinates scratch")?;
    let Some(segments) = &definition.segments else {
        return Ok(BTreeMap::new());
    };
    let missing_line =
        scratch.with_storage(|| saved_section_missing_line_geometry(ctx, definition))?;
    let trim_ids = scratch.with_storage(|| trim_segment_ids(ctx, definition))?;
    let variable_points = match definition.variables.as_ref() {
        Some(variables) => {
            scratch
                .with_storage(|| variables.reconciled_points(ctx))?
                .points
        }
        None => BTreeMap::new(),
    };
    let mut seen_vertex_ids = BTreeSet::new();
    let mut duplicate_vertex_ids = BTreeSet::new();
    let mut coordinate_candidates = Vec::new();
    let framed_trim_vertices = match definition.trim_vertices.as_ref() {
        Some(table) if table.has_complete_bucket_frame(ctx)? => Some(table),
        _ => None,
    };
    if let Some(table) = framed_trim_vertices {
        for vertex in ctx.admit_iter(&table.rows, "creo sketch trim vertex rows")? {
            if ctx.contains_btree_set(
                &seen_vertex_ids,
                &vertex.vertex_id,
                "creo sketch seen trim vertex lookup",
            )? {
                scratch.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut duplicate_vertex_ids,
                        vertex.vertex_id,
                        "creo sketch duplicate trim vertex nodes",
                    )
                })?;
            } else {
                scratch.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut seen_vertex_ids,
                        vertex.vertex_id,
                        "creo sketch seen trim vertex nodes",
                    )
                })?;
            }
            if let Some(point) = vertex
                .section_coordinates
                .map(cadmpeg_ir::units::FinitePoint2::get)
            {
                scratch.with_storage(|| {
                    ctx.reserve_vec(
                        &mut coordinate_candidates,
                        1,
                        "creo sketch trim coordinate candidates",
                    )
                })?;
                coordinate_candidates.push((vertex.vertex_id, [point.u, point.v]));
            }
        }
    }
    if let Some(trim_entities) = definition.trim_entities.as_ref() {
        for (trim, external_id) in ctx
            .admit_iter(&trim_entities.rows, "creo sketch trim entity rows")?
            .zip(trim_ids.iter().copied())
        {
            let Some(external_id) = external_id else {
                continue;
            };
            let Some(segment) = segments.unique_segment(external_id) else {
                continue;
            };
            let Some(carrier) = saved_section_arc_carrier(ctx, definition, segment)? else {
                continue;
            };
            let ([center_u, center_v], radius) = carrier.raw();
            let Some(arc) = saved_section_arc_record(ctx, definition, segment)? else {
                continue;
            };
            for (vertex, endpoint) in trim.vertices.into_iter().zip(arc.endpoints) {
                let [Some(u), Some(v), _] = endpoint else {
                    continue;
                };
                let candidate = [u, v];
                let candidate_radius = (u - center_u).hypot(v - center_v);
                let radial_scale = radius.max(candidate_radius);
                if !candidate_radius.is_finite()
                    || (candidate_radius - radius).abs() / radial_scale
                        > EPS_SKETCH_INTERSECTION_GEOMETRY
                {
                    continue;
                }
                scratch.with_storage(|| {
                    ctx.reserve_vec(
                        &mut coordinate_candidates,
                        1,
                        "creo sketch trim coordinate candidates",
                    )
                })?;
                coordinate_candidates.push((vertex, candidate));
            }
        }
    }
    let mut incident = BTreeMap::<u32, Vec<u32>>::new();
    if let Some(trim_entities) = definition.trim_entities.as_ref() {
        for (entity, external_id) in ctx
            .admit_iter(&trim_entities.rows, "creo sketch incident trim entity rows")?
            .zip(trim_ids.iter().copied())
        {
            let Some(external_id) = external_id else {
                continue;
            };
            for vertex in entity.vertices {
                let entities = scratch
                    .with_storage(|| {
                        ctx.entry_btree_map(
                            &mut incident,
                            vertex,
                            "creo sketch incident vertex nodes",
                        )
                    })?
                    .or_default();
                scratch.with_storage(|| {
                    ctx.reserve_vec(entities, 1, "creo sketch incident vertex entities")
                })?;
                entities.push(external_id);
            }
        }
    }
    // Trim entity rows by external identifier; a repeated identifier keeps a
    // `None` marker so the explicit lookup below needs no row scan.
    let mut trim_entities_by_id = BTreeMap::<u32, Option<usize>>::new();
    if let Some(trim_entities) = definition.trim_entities.as_ref() {
        for (index, entity) in ctx
            .admit_iter(&trim_entities.rows, "creo explicit trim entity index")?
            .enumerate()
        {
            match scratch.with_storage(|| {
                ctx.entry_btree_map(
                    &mut trim_entities_by_id,
                    entity.external_id,
                    "creo explicit trim entity index nodes",
                )
            })? {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(Some(index));
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    *entry.get_mut() = None;
                }
            }
        }
    }
    let explicit_incident = framed_trim_vertices
        .map(|table| {
            let mut result = BTreeMap::<u32, Vec<u32>>::new();
            for vertex in ctx.admit_iter(&table.rows, "creo explicit trim vertex rows")? {
                let mut vertex_storage = ctx.reserve_scoped(0, "creo explicit vertex scratch")?;
                let mut resolved = Vec::new();
                for entity_id in
                    ctx.admit_iter(&vertex.entities, "creo explicit vertex entity IDs")?
                {
                    let indexed = ctx.get_btree_map(
                        &trim_entities_by_id,
                        entity_id,
                        "creo explicit trim entity lookup",
                    )?;
                    let external_id = match (indexed, definition.trim_entities.as_ref()) {
                        (Some(Some(index)), Some(_)) => trim_ids.get(*index).copied().flatten(),
                        (Some(_), _) => None,
                        (None, _) => segments
                            .unique_segment(*entity_id)
                            .map(|segment| segment.external_id),
                    };
                    if let Some(external_id) = external_id {
                        vertex_storage.with_storage(|| {
                            ctx.reserve_vec(
                                &mut resolved,
                                1,
                                "creo sketch explicit incident entities",
                            )
                        })?;
                        resolved.push(external_id);
                    }
                }
                ctx.sort_unstable_by(
                    &mut resolved,
                    |value| value,
                    Ord::cmp,
                    "creo sketch explicit incident entities sort",
                )?;
                if resolved.len() == vertex.entities.len() {
                    let entities = scratch
                        .with_storage(|| {
                            ctx.entry_btree_map(
                                &mut result,
                                vertex.vertex_id,
                                "creo sketch explicit incident nodes",
                            )
                        })?
                        .or_default();
                    scratch.with_storage(|| {
                        ctx.reserve_vec(
                            entities,
                            resolved.len(),
                            "creo sketch explicit incident entities",
                        )
                    })?;
                    entities.extend(
                        ctx.admit_iter(resolved, "creo sketch explicit incident entity moves")?,
                    );
                }
            }
            Ok::<_, cadmpeg_core::CodecError>(result)
        })
        .transpose()?;
    if let Some(explicit) = &explicit_incident {
        for (vertex, entities) in ctx.admit_iter(explicit, "creo explicit incident vertices")? {
            let mut comparison_storage =
                ctx.reserve_scoped(0, "creo incident comparison scratch")?;
            if entities.len() < 2
                || ctx.any_by(
                    entities.windows(2),
                    |pair| Ok(pair[0] == pair[1]),
                    "creo explicit incident entity IDs",
                )?
            {
                continue;
            }
            let mut derived =
                match ctx.get_btree_map(&incident, vertex, "creo sketch incident vertex lookup")? {
                    Some(rows) => comparison_storage.with_storage(|| {
                        ctx.collect_vec(
                            ctx.admit_iter(rows, "creo sketch incident comparison source")?
                                .copied(),
                            "creo sketch incident comparison copy",
                        )
                    })?,
                    None => Vec::new(),
                };
            ctx.sort_unstable_by(
                &mut derived,
                |value| value,
                Ord::cmp,
                "creo sketch incident comparison sort",
            )?;
            ctx.dedup_vec(&mut derived, "creo sketch incident comparison dedup")?;
            if !ctx.equal(&derived, entities, "creo incident derived entity agreement")? {
                continue;
            }
            let copied = scratch.with_storage(|| {
                ctx.collect_vec(
                    entities.iter().copied(),
                    "creo sketch explicit incident copy",
                )
            })?;
            scratch.with_storage(|| {
                ctx.insert_btree_map(
                    &mut incident,
                    *vertex,
                    copied,
                    "creo sketch explicit incident nodes",
                )
            })?;
        }
    }
    let mut unique_carrier_ids = BTreeSet::new();
    for (_, entity_ids) in ctx.admit_iter(&incident, "creo sketch incident vertices")? {
        for external_id in ctx.admit_iter(entity_ids, "creo sketch incident entity IDs")? {
            scratch.with_storage(|| {
                ctx.insert_btree_set(
                    &mut unique_carrier_ids,
                    *external_id,
                    "creo sketch intersection carrier ID nodes",
                )
            })?;
        }
    }
    let mut intersection_carriers = BTreeMap::new();
    for &external_id in ctx.admit_iter(&unique_carrier_ids, "creo sketch unique carrier IDs")? {
        let Some(segment) = segments.unique_segment(external_id) else {
            continue;
        };
        let Some(carrier) = scratch.with_storage(|| {
            section_segment_intersection_carrier_with_missing_line(
                ctx,
                definition,
                radii,
                points,
                segment,
                missing_line.as_ref(),
                &variable_points,
            )
        })?
        else {
            continue;
        };
        scratch.with_storage(|| {
            ctx.insert_btree_map(
                &mut intersection_carriers,
                external_id,
                carrier,
                "creo sketch intersection carrier nodes",
            )
        })?;
    }
    for (vertex, mut entities) in ctx.admit_iter(incident, "creo sketch incident vertex rows")? {
        let mut carrier_storage = ctx.reserve_scoped(0, "creo incident carrier scratch")?;
        ctx.sort_unstable_by(
            &mut entities,
            |value| value,
            Ord::cmp,
            "creo sketch incident entities sort",
        )?;
        if entities.len() < 2
            || ctx.any_by(
                entities.windows(2),
                |pair| Ok(pair[0] == pair[1]),
                "creo incident entity IDs",
            )?
        {
            continue;
        }
        if let Some(explicit) = &explicit_incident {
            let Some(explicit_entities) =
                ctx.get_btree_map(explicit, &vertex, "creo explicit incident vertex lookup")?
            else {
                continue;
            };
            if !ctx.equal(
                explicit_entities,
                &entities,
                "creo explicit incident entity agreement",
            )? {
                continue;
            }
        }
        // A unique shared endpoint coordinate is a trim witness even when a
        // complete carrier cannot be evaluated from the remaining points.
        let mut common_point = None;
        let mut multiple_common_points = false;
        if let Some(first) = ctx.find_map(
            &entities,
            |id| Ok(segments.unique_segment(*id)),
            "creo incident first segment IDs",
        )? {
            for point_id in first.point_ids() {
                if ctx.all_by(
                    &entities,
                    |id| {
                        Ok(segments
                            .unique_segment(*id)
                            .is_none_or(|segment| segment.point_ids().contains(&point_id)))
                    },
                    "creo incident shared point segment IDs",
                )? && common_point != Some(point_id)
                {
                    if common_point.is_some() {
                        multiple_common_points = true;
                        break;
                    }
                    common_point = Some(point_id);
                }
            }
        }
        if let Some(point_id) = common_point.filter(|_| !multiple_common_points) {
            if let Some(coordinate) =
                ctx.get_btree_map(points, &point_id, "creo sketch shared point lookup")?
            {
                scratch.with_storage(|| {
                    ctx.reserve_vec(
                        &mut coordinate_candidates,
                        1,
                        "creo sketch trim coordinate candidates",
                    )
                })?;
                coordinate_candidates.push((vertex, *coordinate));
            }
        }
        let mut carriers = Vec::new();
        let mut complete = true;
        let mut carrier_ids = entities.iter();
        while carrier_ids.len() != 0 {
            let Some(external_id) =
                ctx.next_charged(&mut carrier_ids, "creo incident carrier IDs")?
            else {
                break;
            };
            let Some(carrier) = ctx.get_btree_map(
                &intersection_carriers,
                external_id,
                "creo sketch intersection carrier lookup",
            )?
            else {
                complete = false;
                break;
            };
            carrier_storage.with_storage(|| {
                ctx.reserve_vec(&mut carriers, 1, "creo sketch incident carriers")
            })?;
            carriers.push(carrier);
        }
        if !complete {
            continue;
        }
        if let Some(coordinate) = intersect_incident_section_carriers(ctx, &carriers)? {
            scratch.with_storage(|| {
                ctx.reserve_vec(
                    &mut coordinate_candidates,
                    1,
                    "creo sketch trim coordinate candidates",
                )
            })?;
            coordinate_candidates.push((vertex, coordinate));
        }
    }
    let crate::feature::definitions::ReconciledPoints {
        points: mut coordinates,
        ambiguous: mut ambiguous_vertices,
    } = reconciled_section_coordinates(ctx, &coordinate_candidates, &mut scratch)?;
    for &vertex in ctx.admit_iter(&duplicate_vertex_ids, "creo duplicate trim vertex IDs")? {
        scratch.with_storage(|| {
            ctx.insert_btree_set(
                &mut ambiguous_vertices,
                vertex,
                "creo sketch ambiguous coordinate nodes",
            )
        })?;
    }
    ctx.retain_btree_map(
        &mut coordinates,
        |vertex, _| {
            Ok::<_, cadmpeg_core::CodecError>(!ctx.contains_btree_set(
                &ambiguous_vertices,
                vertex,
                "creo sketch ambiguous vertex lookup",
            )?)
        },
        "creo sketch unambiguous coordinates",
    )?;
    loop {
        let mut pass_storage = ctx.reserve_scoped(0, "creo propagated trim pass scratch")?;
        ctx.charge_work(1, "creo propagated trim fixed-point passes")?;
        let mut additions = Vec::new();
        if let Some(trim_entities) = definition.trim_entities.as_ref() {
            for (trim, external_id) in ctx
                .admit_iter(&trim_entities.rows, "creo propagated trim entity rows")?
                .zip(trim_ids.iter().copied())
            {
                let Some(external_id) = external_id else {
                    continue;
                };
                let Some(segment) = segments.unique_segment(external_id) else {
                    continue;
                };
                let Some(SketchGeometryDefinition::Line { start, end }) =
                    (resolved_section_segment_geometry_with_missing_line(
                        ctx,
                        definition,
                        points,
                        segment,
                        missing_line.as_ref(),
                    )?)
                    .map(SketchGeometry::into_definition)
                else {
                    continue;
                };
                let stored = [[start.u, start.v], [end.u, end.v]];
                let known = [
                    ctx.get_btree_map(
                        &coordinates,
                        &trim.vertices[0],
                        "creo trim coordinate lookup",
                    )?
                    .copied(),
                    ctx.get_btree_map(
                        &coordinates,
                        &trim.vertices[1],
                        "creo trim coordinate lookup",
                    )?
                    .copied(),
                ];
                let (known_point, missing_index) = match known {
                    [Some(point), None] => (point, 1),
                    [None, Some(point)] => (point, 0),
                    _ => continue,
                };
                let distances = stored
                    .map(|point| (point[0] - known_point[0]).hypot(point[1] - known_point[1]));
                let scale = stored
                    .iter()
                    .flatten()
                    .map(|value| value.abs())
                    .fold(1.0, f64::max);
                let matched = if distances[0] <= EPS_SKETCH_INTERSECTION_GEOMETRY * scale
                    && distances[1] > EPS_SKETCH_INTERSECTION_GEOMETRY * scale
                {
                    0
                } else if distances[1] <= EPS_SKETCH_INTERSECTION_GEOMETRY * scale
                    && distances[0] > EPS_SKETCH_INTERSECTION_GEOMETRY * scale
                {
                    1
                } else {
                    continue;
                };
                pass_storage.with_storage(|| {
                    ctx.reserve_vec(&mut additions, 1, "creo sketch propagated trim coordinates")
                })?;
                additions.push((trim.vertices[missing_index], stored[1 - matched]));
            }
        }
        let mut conflict_storage = ctx.reserve_scoped(0, "creo trim conflict scratch")?;
        let crate::feature::definitions::ReconciledPoints {
            points: additions,
            ambiguous: conflicts,
        } = pass_storage.with_storage(|| {
            reconciled_section_coordinates(ctx, &additions, &mut conflict_storage)
        })?;
        for &vertex in ctx.admit_iter(&conflicts, "creo conflicting trim vertex IDs")? {
            scratch.with_storage(|| {
                ctx.insert_btree_set(
                    &mut ambiguous_vertices,
                    vertex,
                    "creo sketch ambiguous coordinate nodes",
                )
            })?;
        }
        let mut changed = false;
        for (&vertex, &coordinate) in
            ctx.admit_iter(&additions, "creo trim coordinate additions")?
        {
            if ctx.contains_btree_set(
                &ambiguous_vertices,
                &vertex,
                "creo sketch ambiguous vertex lookup",
            )? {
                continue;
            }
            if !ctx.contains_key_btree_map(&coordinates, &vertex, "creo trim coordinate lookup")? {
                ctx.insert_btree_map(
                    &mut coordinates,
                    vertex,
                    coordinate,
                    "creo sketch propagated coordinate nodes",
                )?;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    Ok(coordinates)
}

fn reconciled_section_coordinates(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    candidates: &[(u32, [f64; 2])],
    ambiguous_storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
) -> Result<crate::feature::definitions::ReconciledPoints<[f64; 2]>, cadmpeg_core::CodecError> {
    struct CoordinateSamples {
        first: [f64; 2],
        scale: f64,
        maximum_distance: f64,
        invalid: bool,
    }
    let mut scratch = ctx.reserve_scoped(0, "creo reconciled section coordinates scratch")?;
    let mut grouped = BTreeMap::<u32, CoordinateSamples>::new();
    for &(vertex, coordinate) in ctx.admit_iter(candidates, "creo sketch coordinate candidates")? {
        let group = scratch
            .with_storage(|| {
                ctx.entry_btree_map(
                    &mut grouped,
                    vertex,
                    "creo sketch reconciliation group nodes",
                )
            })?
            .or_insert(CoordinateSamples {
                first: coordinate,
                scale: 1.0,
                maximum_distance: 0.0,
                invalid: false,
            });
        group.scale = group
            .scale
            .max(coordinate[0].abs())
            .max(coordinate[1].abs());
        let distance = (coordinate[0] - group.first[0]).hypot(coordinate[1] - group.first[1]);
        group.invalid |= distance.is_nan();
        group.maximum_distance = group.maximum_distance.max(distance);
    }
    let mut coordinates = BTreeMap::new();
    let mut ambiguous = BTreeSet::new();
    for (&vertex, group) in ctx.admit_iter(&grouped, "creo sketch coordinate groups")? {
        if !group.invalid
            && group.maximum_distance <= EPS_SKETCH_INTERSECTION_GEOMETRY * group.scale
        {
            ctx.insert_btree_map(
                &mut coordinates,
                vertex,
                group.first,
                "creo sketch reconciled coordinate nodes",
            )?;
        } else {
            ambiguous_storage.with_storage(|| {
                ctx.insert_btree_set(
                    &mut ambiguous,
                    vertex,
                    "creo sketch ambiguous coordinate nodes",
                )
            })?;
        }
    }
    Ok(crate::feature::definitions::ReconciledPoints {
        points: coordinates,
        ambiguous,
    })
}

pub(in crate::decode) fn trimmed_section_segment_geometry_with_missing_line(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    points: &BTreeMap<u32, [f64; 2]>,
    radii: &BTreeMap<u32, f64>,
    trim_vertices: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::definitions::FeatureSegment,
    missing_line: Option<&(usize, SketchGeometry)>,
) -> Result<Option<SketchGeometry>, cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let Some(trim_entities) = definition.trim_entities.as_ref() else {
        return Ok(None);
    };
    let mut trim_storage = ctx.reserve_scoped(0, "creo trimmed geometry ID scratch")?;
    let trim_ids = trim_storage.with_storage(|| trim_segment_ids(ctx, definition))?;
    let Some(position) = ctx.position_by(
        &trim_ids,
        |id| Ok(*id == Some(segment.external_id)),
        "creo trimmed segment rows",
    )?
    else {
        return Ok(None);
    };
    let trim = &trim_entities.rows[position];
    let Some(start) =
        ctx.get_btree_map(trim_vertices, &trim.vertices[0], "creo trim vertex lookup")?
    else {
        return Ok(None);
    };
    let Some(end) =
        ctx.get_btree_map(trim_vertices, &trim.vertices[1], "creo trim vertex lookup")?
    else {
        return Ok(None);
    };
    if let Some(SketchGeometryDefinition::Line {
        start: carrier_start,
        end: carrier_end,
    }) = (resolved_section_segment_geometry_with_missing_line(
        ctx,
        definition,
        points,
        segment,
        missing_line,
    )?)
    .map(SketchGeometry::into_definition)
    {
        let scale = [
            carrier_start.u,
            carrier_start.v,
            carrier_end.u,
            carrier_end.v,
            start[0],
            start[1],
            end[0],
            end[1],
        ]
        .into_iter()
        .map(f64::abs)
        .fold(1.0, f64::max);
        let direction = [
            carrier_end.u / scale - carrier_start.u / scale,
            carrier_end.v / scale - carrier_start.v / scale,
        ];
        let direction_norm = direction[0].hypot(direction[1]);
        if direction_norm <= EPS_SKETCH_INTERSECTION_EXACT_GEOMETRY
            || [start, end].into_iter().any(|point| {
                let offset = [
                    point[0] / scale - carrier_start.u / scale,
                    point[1] / scale - carrier_start.v / scale,
                ];
                (offset[0] * direction[1] - offset[1] * direction[0]).abs()
                    > EPS_SKETCH_INTERSECTION_GEOMETRY * direction_norm
            })
        {
            return Ok(None);
        }
    } else if let Some(carrier) = match section_arc_carrier(ctx, radii, points, segment)? {
        Some(carrier) => Some(carrier),
        None => saved_section_arc_carrier(ctx, definition, segment)?,
    } {
        let ([center_u, center_v], radius) = carrier.raw();
        let first = [start[0] - center_u, start[1] - center_v];
        let second = [end[0] - center_u, end[1] - center_v];
        let first_radius = first[0].hypot(first[1]);
        let second_radius = second[0].hypot(second[1]);
        let scale = radius.max(first_radius).max(second_radius);
        if !first_radius.is_finite()
            || !second_radius.is_finite()
            || (first_radius - radius).abs() / scale > EPS_SKETCH_INTERSECTION_GEOMETRY
            || (second_radius - radius).abs() / scale > EPS_SKETCH_INTERSECTION_GEOMETRY
        {
            return Ok(None);
        }
        let start_angle = second[1].atan2(second[0]);
        let mut end_angle = first[1].atan2(first[0]);
        // `atan2` lies in [-pi, pi], so at most two turns bring the end past the start.
        while end_angle <= start_angle {
            end_angle += std::f64::consts::TAU;
        }
        let Some(start_angle) = Angle::new(start_angle) else {
            return Ok(None);
        };
        let Some(end_angle) = Angle::new(end_angle) else {
            return Ok(None);
        };
        return Ok(SketchGeometry::from_parts(SketchGeometryDefinition::Arc {
            center: carrier.center,
            radius: carrier.radius,
            start_angle,
            end_angle,
        })
        .ok());
    } else {
        let scale = start
            .iter()
            .chain(end)
            .map(|value| value.abs())
            .fold(1.0, f64::max);
        let orientation_matches = match section_line_entity_fixed_coordinate_with_unique_rows(
            ctx,
            definition,
            segment.external_id,
        )? {
            Some(SectionAxis::U) => {
                (start[0] - end[0]).abs() <= EPS_SKETCH_INTERSECTION_GEOMETRY * scale
            }
            Some(SectionAxis::V) => {
                (start[1] - end[1]).abs() <= EPS_SKETCH_INTERSECTION_GEOMETRY * scale
            }
            _ => false,
        };
        if !orientation_matches {
            return Ok(None);
        }
    }
    Ok(SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: cadmpeg_ir::math::Point2::new(start[0], start[1]),
        end: cadmpeg_ir::math::Point2::new(end[0], end[1]),
    })
    .ok())
}

pub(in crate::decode) fn section_point_in_model(
    transform: &crate::placement::FeatureSectionTransform,
    point: [f64; 2],
) -> [f64; 3] {
    std::array::from_fn(|axis| {
        transform.origin()[axis]
            + point[0] * transform.u_axis()[axis]
            + point[1] * transform.v_axis()[axis]
    })
}

pub(in crate::decode) fn section_xyz_in_model(
    transform: &crate::placement::FeatureSectionTransform,
    point: [f64; 3],
) -> [f64; 3] {
    std::array::from_fn(|axis| {
        transform.origin()[axis]
            + point[0] * transform.u_axis()[axis]
            + point[1] * transform.v_axis()[axis]
            + point[2] * transform.normal()[axis]
    })
}

#[cfg(test)]
mod tests {
    mod admission_visits;
    mod incident_carrier_clone;
    mod trimmed_carriers;

    use super::{
        resolved_trim_vertex_coordinates, trimmed_section_segment_geometry_with_missing_line,
    };
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::sketches::{SketchGeometry, SketchGeometryDefinition};
    use std::collections::BTreeMap;

    #[test]
    fn incident_section_carrier_pairs_refuse_before_candidate_geometry() {
        use cadmpeg_core::decode::ResourceDimension;
        let line = SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(-1.0, 0.0),
            end: Point2::new(1.0, 0.0),
        })
        .expect("line geometry");
        let carriers = [line.clone(), line];
        for operation in [
            "creo incident section first carriers",
            "creo incident section second carriers",
        ] {
            let error = crate::test_support::last_refusal_at(
                &[],
                ResourceDimension::WorkUnits,
                operation,
                |ctx| super::intersect_incident_section_carriers(ctx, &carriers),
            );
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
                if resource.dimension == ResourceDimension::WorkUnits && resource.operation == operation)
            );
        }
    }

    #[test]
    fn incident_section_carrier_cardinality_gate_precedes_scan_admission() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let line = SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(-1.0, 0.0),
            end: Point2::new(1.0, 0.0),
        })
        .expect("line geometry");
        assert_eq!(
            super::intersect_incident_section_carriers(&ctx, &[line])
                .expect("one carrier needs no pair search"),
            None
        );
    }

    #[test]
    fn sketch_coordinate_reconciliation_refuses_before_group_node() {
        let error = crate::test_support::last_refusal_at(
            &[0],
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            "creo sketch reconciliation group nodes",
            |ctx| {
                let mut storage = ctx.reserve_scoped(0, "test coordinate ambiguity")?;
                super::reconciled_section_coordinates(ctx, &[(7, [2.0, 3.0])], &mut storage)
            },
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == "creo sketch reconciliation group nodes")
        );
        let (coordinates, ambiguous) = crate::decode::with_test_decode_ctx(|ctx| {
            let mut storage = ctx.reserve_scoped(0, "test coordinate ambiguity")?;
            super::reconciled_section_coordinates(ctx, &[(7, [2.0, 3.0])], &mut storage)
                .map(|result| (result.points, result.ambiguous))
        })
        .expect("test reconciliation");
        assert_eq!(coordinates.get(&7), Some(&[2.0, 3.0]));
        assert!(ambiguous.is_empty());
    }

    #[test]
    fn trim_vertex_propagation_refuses_before_fixed_point_pass() {
        let definition = crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(1),
                owner_feature_id: None,
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: Some(crate::feature::definitions::FeatureSegmentTable {
                declared_count: 0,
                has_elided_prototype: false,
                entity_ref: None,
                rows: crate::feature::segment_rows::SegmentRows::default(),
                offset: 0,
            }),
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 0,
        };
        let coordinates = crate::test_support::assert_work_boundaries(
            &["creo propagated trim fixed-point passes"],
            |ctx| {
                resolved_trim_vertex_coordinates(
                    ctx,
                    &definition,
                    &BTreeMap::new(),
                    &BTreeMap::new(),
                )
            },
        );
        assert!(coordinates.is_empty());
    }

    #[test]
    fn trim_vertex_requires_exact_trim_entity_incidence() {
        let segment = |external_id, point_ids| crate::feature::definitions::FeatureSegment {
            kind: crate::feature::definitions::FeatureSegmentKind::Line(point_ids),
            directions: [None; 3],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id,
            body: Vec::new(),
            offset: 0,
        };
        let definition = crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(1),
                owner_feature_id: None,
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: Some(crate::feature::definitions::FeatureSegmentTable {
                declared_count: 2,
                has_elided_prototype: false,
                entity_ref: None,
                rows: (vec![segment(42, [1, 2]), segment(43, [3, 4])])
                    .into_iter()
                    .map(crate::feature::segment_rows::SegmentRow::Ordinary)
                    .collect(),
                offset: 0,
            }),
            trim_entities: Some(crate::feature::definitions::FeatureTrimEntityTable {
                declared_count: None,
                entity_ref: None,
                entry_ref: None,
                buckets: Vec::new(),
                rows: vec![crate::feature::definitions::FeatureTrimEntity {
                    external_id: 42,
                    mode: None,
                    vertices: [1, 2],
                    kind: crate::feature::definitions::TrimEntityKind::Line,
                    offset: 0,
                }],
                solved_external_ids: vec![42],
                offset: 0,
            }),
            trim_vertices: Some(crate::feature::definitions::FeatureTrimVertexTable {
                declared_count: None,
                entity_ref: None,
                entry_ref: None,
                buckets: Vec::new(),
                rows: vec![crate::feature::definitions::FeatureTrimVertex {
                    vertex_id: 3,
                    entities: vec![42, 43],
                    section_coordinates: None,
                    offset: 0,
                }],
                offset: 0,
            }),
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 0,
        };

        let arena = cadmpeg_core::decode::DecodeArena::new();
        crate::test_support::assert_refusal_order(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            &[
                "creo trim segment IDs",
                "creo sketch seen trim vertex nodes",
            ],
            |cap| {
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                policy.limits.max_collection_items = cap;
                let (limited_ctx, _) =
                    cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
                        .expect("test input admitted");
                resolved_trim_vertex_coordinates(
                    &limited_ctx,
                    &definition,
                    &BTreeMap::new(),
                    &BTreeMap::new(),
                )
            },
        );

        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| {
                let radii = crate::decode::sketch::radii::resolved_section_radii(ctx, &definition)?;
                resolved_trim_vertex_coordinates(
                    ctx,
                    &definition,
                    &BTreeMap::from([
                        (1, [-1.0, 0.0]),
                        (2, [1.0, 0.0]),
                        (3, [0.0, -1.0]),
                        (4, [0.0, 1.0]),
                    ]),
                    &radii,
                )
            })
            .expect("test section geometry"),
            BTreeMap::new()
        );

        let mut shared_point = definition.clone();
        shared_point.trim_vertices = None;
        shared_point
            .segments
            .as_mut()
            .expect("segments")
            .rows
            .edit_ordinary(|rows| {
                rows[1].kind = crate::feature::definitions::FeatureSegmentKind::Line([2, 3]);
            });
        shared_point
            .trim_entities
            .as_mut()
            .expect("trim entities")
            .rows
            .push(crate::feature::definitions::FeatureTrimEntity {
                external_id: 43,
                mode: None,
                vertices: [2, 3],
                kind: crate::feature::definitions::TrimEntityKind::Line,
                offset: 0,
            });
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| {
                let radii =
                    crate::decode::sketch::radii::resolved_section_radii(ctx, &shared_point)?;
                resolved_trim_vertex_coordinates(
                    ctx,
                    &shared_point,
                    &BTreeMap::from([(2, [0.0, 0.0])]),
                    &radii,
                )
            })
            .expect("test section geometry"),
            BTreeMap::from([(2, [0.0, 0.0])])
        );
    }

    #[test]
    fn incomplete_unique_trim_line_uses_stored_orientation() {
        let segment = crate::feature::definitions::FeatureSegment {
            kind: crate::feature::definitions::FeatureSegmentKind::Line([1, 2]),
            directions: [None; 3],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: Some(1),
            radius_ref: None,
            radius2_ref: None,
            external_id: 10,
            body: Vec::new(),
            offset: 0,
        };
        let definition = crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(2),
                owner_feature_id: None,
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: Some(crate::feature::definitions::FeatureSegmentTable {
                declared_count: 2,
                has_elided_prototype: false,
                entity_ref: None,
                rows: (vec![segment.clone()])
                    .into_iter()
                    .map(crate::feature::segment_rows::SegmentRow::Ordinary)
                    .collect(),
                offset: 0,
            }),
            trim_entities: Some(crate::feature::definitions::FeatureTrimEntityTable {
                declared_count: None,
                entity_ref: None,
                entry_ref: None,
                buckets: Vec::new(),
                rows: vec![crate::feature::definitions::FeatureTrimEntity {
                    external_id: 10,
                    mode: None,
                    vertices: [3, 4],
                    kind: crate::feature::definitions::TrimEntityKind::Line,
                    offset: 1,
                }],
                solved_external_ids: vec![10],
                offset: 1,
            }),
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 0,
        };
        let trim_vertices = BTreeMap::from([(3, [0.0, 4.0]), (4, [7.0, 4.0])]);
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| {
                let radii = crate::decode::sketch::radii::resolved_section_radii(ctx, &definition)?;
                trimmed_section_segment_geometry_with_missing_line(
                    ctx,
                    &definition,
                    &BTreeMap::new(),
                    &radii,
                    &trim_vertices,
                    &segment,
                    None,
                )
            })
            .expect("test section geometry"),
            Some(
                cadmpeg_ir::sketches::SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: Point2::new(0.0, 4.0),
                    end: Point2::new(7.0, 4.0),
                })
                .expect("valid test fixture")
            )
        );

        let mut duplicate = definition;
        duplicate.segments.as_mut().expect("segments").rows.insert(
            crate::feature::segment_rows::SegmentRow::Ordinary(
                crate::feature::definitions::FeatureSegment {
                    offset: 2,
                    ..segment
                },
            ),
        );
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| {
                let radii = crate::decode::sketch::radii::resolved_section_radii(ctx, &duplicate)?;
                trimmed_section_segment_geometry_with_missing_line(
                    ctx,
                    &duplicate,
                    &BTreeMap::new(),
                    &radii,
                    &trim_vertices,
                    &duplicate
                        .segments
                        .as_ref()
                        .expect("segments")
                        .rows
                        .ordinary()
                        .cloned()
                        .collect::<Vec<_>>()[0],
                    None,
                )
            })
            .expect("test section geometry"),
            None
        );
    }
    #[test]
    fn small_section_carriers_do_not_acquire_false_tangencies() {
        use cadmpeg_ir::{
            math::Point2,
            scalar::{Angle, Length},
            sketches::{SketchGeometry, SketchGeometryDefinition},
        };
        let line = |a: [f64; 2], b: [f64; 2]| -> SketchGeometry {
            SketchGeometryDefinition::Line {
                start: Point2::new(a[0], a[1]),
                end: Point2::new(b[0], b[1]),
            }
            .try_into()
            .expect("line carrier with distinct endpoints")
        };
        let arc = |x, r| -> SketchGeometry {
            SketchGeometryDefinition::Arc {
                center: Point2::new(x, 0.),
                radius: Length::new(r).expect("positive finite arc radius"),
                start_angle: Angle::new(0.).expect("finite start angle"),
                end_angle: Angle::new(std::f64::consts::TAU).expect("finite end angle"),
            }
            .try_into()
            .expect("arc carrier with positive radius")
        };
        assert_eq!(
            super::intersect_section_line_arc(&line([-1e200, 1.], [1e200, 1.]), &arc(0., 1e-6)),
            None
        );
        assert_eq!(
            super::intersect_tangent_section_arcs(&arc(0., 1e-6), &arc(1e200, 1e200)),
            None
        );
        let r = 1e-6;
        assert_eq!(
            super::intersect_section_line_arc(&line([-r, 2. * r], [r, 2. * r]), &arc(0., r)),
            None
        );
        assert_eq!(
            super::intersect_tangent_section_arcs(&arc(0., r), &arc(r, r)),
            None
        );
        assert_eq!(
            super::intersect_tangent_section_arcs(&arc(0., r), &arc(2. * r, r)),
            Some([r, 0.])
        );
        let r = 1e-7;
        assert_eq!(
            super::intersect_section_lines(&line([-r, 0.], [r, 0.]), &line([0., -r], [0., r])),
            Some([0., 0.])
        );
    }
    #[test]
    fn numerical_0922b_section_unique_crossing() {
        let arc = SketchGeometry::try_from(SketchGeometryDefinition::Arc {
            center: Point2::new(0., 0.),
            radius: cadmpeg_ir::scalar::Length::new(0.001).expect("positive radius"),
            start_angle: cadmpeg_ir::scalar::Angle::new(0.).expect("finite start angle"),
            end_angle: cadmpeg_ir::scalar::Angle::new(std::f64::consts::PI)
                .expect("finite end angle"),
        })
        .expect("valid arc");
        for x in [-1., -1e4, -1e8] {
            let line = SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(x, 0.),
                end: Point2::new(0., 0.),
            })
            .expect("valid line");
            let r = super::intersect_section_line_arc(&line, &arc);
            println!("Creo one-sided line[{x},0],r=.001: {r:?}");
            assert_eq!(r, Some([-0.001, 0.0]));
        }
    }

    #[test]
    fn numerical_audit_line_arc_clips_rounded_endpoint_roots() {
        use cadmpeg_ir::math::Point2;
        use cadmpeg_ir::sketches::{SketchGeometry, SketchGeometryDefinition};
        let arc = SketchGeometry::try_from(SketchGeometryDefinition::Arc {
            center: Point2::new(0., 0.),
            radius: cadmpeg_ir::scalar::Length::new(0.001).expect("positive radius"),
            start_angle: cadmpeg_ir::scalar::Angle::new(0.).expect("finite angle"),
            end_angle: cadmpeg_ir::scalar::Angle::new(std::f64::consts::PI).expect("finite angle"),
        })
        .expect("valid sketch geometry");
        for start in [-1., -1e20] {
            let line = SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(start, 0.),
                end: Point2::new(0., 0.),
            })
            .expect("valid sketch geometry");
            assert_eq!(
                super::intersect_section_line_arc(&line, &arc),
                Some([-0.001, 0.])
            );
        }
    }
}
