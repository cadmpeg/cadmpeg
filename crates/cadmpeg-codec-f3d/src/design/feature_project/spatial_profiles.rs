// SPDX-License-Identifier: Apache-2.0
//! Closed spatial sketch profile construction.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point3,Vector3};
use std::collections::BTreeSet;

fn spatial_sketch_entity_endpoints(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    entity: &cadmpeg_ir::sketches::SpatialSketchEntity,
) -> Result<Option<[Point3; 2]>, CodecError> {
    use cadmpeg_ir::sketches::SpatialSketchGeometryDefinition;

    match entity.geometry.definition() {
        SpatialSketchGeometryDefinition::Line { start, end } => Ok(Some([start.get(), end.get()])),
        SpatialSketchGeometryDefinition::Arc {
            center,
            normal,
            reference_direction,
            radius,
            start_angle,
            end_angle,
        } => {
            let normal = normal.as_raw();
            let reference_direction = reference_direction.as_raw();
            let transverse = normal.cross(*reference_direction);
            let at = |angle: f64| {
                center.translated(
                    reference_direction.scale(angle.cos()) + transverse.scale(angle.sin()),
                    radius.get(),
                )
            };
            Ok(Some([at(start_angle.get()), at(end_angle.get())]))
        }
        SpatialSketchGeometryDefinition::Nurbs { curve } if !curve.periodic() => {
            let Ok(degree) = usize::try_from(curve.degree()) else {
                return Ok(None);
            };
            let start = curve.knots()[degree];
            let end = curve.knots()[curve.pole_count()];
            let Some(first) =
                cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                    cadmpeg_ir::eval::decode::nurbs_curve_point_at(ctx, curve, start),
                )?)?
            else {
                return Ok(None);
            };
            let Some(last) =
                cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                    cadmpeg_ir::eval::decode::nurbs_curve_point_at(ctx, curve, end),
                )?)?
            else {
                return Ok(None);
            };
            Ok(Some([first.get(), last.get()]))
        }
        _ => Ok(None),
    }
}

pub(in crate::design) fn closed_spatial_sketch_profiles(
    ctx: &DecodeContext<'_>,
    sketch: &cadmpeg_ir::sketches::SpatialSketchId,
    entities: &[cadmpeg_ir::sketches::SpatialSketchEntity],
    tolerance: f64,
) -> Result<Vec<cadmpeg_ir::sketches::SpatialSketchProfile>, CodecError> {
    use cadmpeg_ir::sketches::{
        SpatialSketchEntityUse, SpatialSketchGeometryDefinition, SpatialSketchProfile,
    };

    if !tolerance.is_finite() || tolerance <= 0.0 {
        return Ok(Vec::new());
    }
    let mut profiles = Vec::new();
    let mut edge_storage = ctx.reserve_scoped(0, "F3D spatial profile traversal")?;
    let mut edges = Vec::new();
    for entity in ctx.admit_iter(entities, "f3d spatial profile entity scan")? {
        if !ctx.equal(&entity.sketch, sketch, "f3d spatial profile sketch match")?
            || entity.construction
        {
            continue;
        }
        match entity.geometry.definition() {
            SpatialSketchGeometryDefinition::Circle {
                center,
                normal,
                reference_direction,
                ..
            } => {
                let mut boundary = Vec::new();
                ctx.push_vec(
                    &mut boundary,
                    SpatialSketchEntityUse {
                        entity: (entity.id())
                            .try_clone_for_decode(ctx, "f3d spatial profile circle id")?,
                        reversed: false,
                    },
                    "f3d spatial profile boundary use",
                )?;
                let profile = SpatialSketchProfile::try_new(
                    center.get(),
                    *normal.as_raw(),
                    *reference_direction.as_raw(),
                    boundary,
                    ctx,
                    "f3d spatial profile boundary uniqueness",
                )?;
                if let Ok(profile) = profile {
                    ctx.push_vec(&mut profiles, profile, "f3d spatial profile")?;
                }
            }
            _ => {
                if let Some(ends) = spatial_sketch_entity_endpoints(ctx, entity)? {
                    edge_storage.with_storage(|| {
                        ctx.push_vec(&mut edges, (entity, ends), "f3d spatial profile edge")
                    })?;
                }
            }
        }
    }
    let close = |a: Point3, b: Point3| (a.x - b.x).hypot(a.y - b.y).hypot(a.z - b.z) <= tolerance;
    let mut unused = BTreeSet::new();
    for (index, (entity, _)) in ctx
        .admit_iter(&edges, "f3d spatial profile edge index")?
        .enumerate()
    {
        edge_storage.with_storage(|| {
            ctx.insert_btree_set(
                &mut unused,
                (entity.id(), index),
                "f3d spatial profile unused edge",
            )
        })?;
    }
    while let Some(&(id, first)) = unused.first() {
        // discarded-value: the edge was selected from the unused set.
        let _ = ctx.remove_btree_set(
            &mut unused,
            &(id, first),
            "f3d spatial profile edge removal",
        )?;
        let mut use_storage = ctx.reserve_scoped(0, "F3D spatial profile use scratch")?;
        let mut uses = Vec::new();
        use_storage
            .with_storage(|| ctx.push_vec(&mut uses, (first, false), "f3d spatial profile use"))?;
        let start = edges[first].1[0];
        let mut end = edges[first].1[1];
        while !close(end, start) {
            let mut candidate = None;
            let mut ambiguous = false;
            let mut candidates = unused.iter();
            while let Some(&(_, index)) =
                ctx.next_charged(&mut candidates, "f3d spatial profile candidate scan")?
            {
                let [candidate_start, candidate_end] = edges[index].1;
                let match_ = if close(end, candidate_start) {
                    Some((index, false))
                } else if close(end, candidate_end) {
                    Some((index, true))
                } else {
                    None
                };
                if let Some(match_) = match_ {
                    if candidate.replace(match_).is_some() {
                        ambiguous = true;
                        break;
                    }
                }
            }
            if ambiguous {
                break;
            }
            let Some(next) = candidate else {
                break;
            };
            // discarded-value: the edge was selected from the unused set.
            let _ = ctx.remove_btree_set(
                &mut unused,
                &(edges[next.0].0.id(), next.0),
                "f3d spatial profile edge removal",
            )?;
            use_storage
                .with_storage(|| ctx.push_vec(&mut uses, next, "f3d spatial profile use"))?;
            end = if next.1 {
                edges[next.0].1[0]
            } else {
                edges[next.0].1[1]
            };
        }
        if !close(end, start) || uses.len() < 3 {
            continue;
        }
        let mut start_degree = 0;
        let mut incident_edges = edges.iter();
        while let Some((_, [edge_start, edge_end])) =
            ctx.next_charged(&mut incident_edges, "f3d spatial profile start degree")?
        {
            if close(*edge_start, start) || close(*edge_end, start) {
                start_degree += 1;
                if start_degree == 3 {
                    break;
                }
            }
        }
        if start_degree != 2 {
            continue;
        }
        let point_for = |(index, reversed): &(usize, bool)| edges[*index].1[usize::from(*reversed)];
        let origin = point_for(&uses[0]);
        let mut normal = Vector3::new(0.0, 0.0, 0.0);
        for index in ctx.admit_iter(2..uses.len(), "f3d spatial profile normal")? {
            let a = point_for(&uses[index - 1]).vector_from(origin);
            let b = point_for(&uses[index]).vector_from(origin);
            normal = normal + a.cross(b);
        }
        let normal_length = normal.norm();
        let first_end = edges[uses[0].0].1[1];
        let u = first_end.vector_from(origin);
        let u_length = u.norm();
        if normal_length <= tolerance || u_length <= tolerance {
            continue;
        }
        normal = normal.scale(1.0 / normal_length);
        let u_axis = u.scale(1.0 / u_length);
        if ctx.any_by(
            &uses,
            |use_| Ok(point_for(use_).vector_from(origin).dot(normal).abs() > tolerance),
            "f3d spatial profile plane proof",
        )? {
            continue;
        }
        let mut boundary = Vec::new();
        for (index, reversed) in ctx
            .admit_iter(&uses, "f3d spatial profile boundary scan")?
            .copied()
        {
            ctx.push_vec(
                &mut boundary,
                SpatialSketchEntityUse {
                    entity: (edges[index].0.id())
                        .try_clone_for_decode(ctx, "f3d spatial profile boundary id")?,
                    reversed,
                },
                "f3d spatial profile boundary use",
            )?;
        }
        let profile = SpatialSketchProfile::try_new(
            origin,
            normal,
            u_axis,
            boundary,
            ctx,
            "f3d spatial profile boundary uniqueness",
        )?;
        if let Ok(profile) = profile {
            ctx.push_vec(&mut profiles, profile, "f3d spatial profile")?;
        }
    }
    drop(unused);
    drop(edges);
    drop(edge_storage);
    ctx.stable_sort_by(
        &mut profiles[..],
        |value| &value.boundary()[0].entity,
        Ord::cmp,
        "sort f3d design feature_project 17",
    )?;
    Ok(profiles)
}

