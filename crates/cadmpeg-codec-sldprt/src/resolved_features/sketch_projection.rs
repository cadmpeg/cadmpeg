//! Sketch projection from B-rep geometry.

use super::assembly::contains_ascii_case_insensitive;
use super::names::configuration;
use super::profiles::mint_formatted;
use super::sketch_edges::{
    project_edge, project_endpoint_constraints, project_point, EndpointConstraintSource,
};
use crate::container::ContainerScan;
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::sketches::{
    Sketch, SketchConstraint, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
    SketchGeometryDefinition, SketchId,
};
use cadmpeg_ir::topology::Sense;
use cadmpeg_ir::Exactness;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;

fn index_brep<'a, T, K: Eq + Hash + cadmpeg_core::decode::cost::DecodeCost, V>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    values: &'a [T],
    mut entry: impl FnMut(&'a T) -> (K, V),
) -> Result<HashMap<K, V>, cadmpeg_core::CodecError> {
    let operation = "index SLDPRT sketch B-rep records";
    let mut index = HashMap::new();
    for value in ctx.admit_iter(values, operation)? {
        let (key, record) = entry(value);
        ctx.insert_hash_map(&mut index, key, record, operation)?;
    }
    Ok(index)
}

/// Sketches and their projected entities and constraints.
pub(crate) struct ProjectedSketches {
    pub(crate) sketches: Vec<Sketch>,
    pub(crate) entities: Vec<SketchEntity>,
    pub(crate) constraints: Vec<SketchConstraint>,
}

/// Decode nested feature-input Parasolid streams as placed planar sketches.
pub(crate) fn sketches(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    annotations: &mut Annotations,
) -> Result<ProjectedSketches, cadmpeg_core::CodecError> {
    let mut sketches = Vec::new();
    let mut entities = Vec::new();
    let mut constraints = Vec::new();
    for source in scan.sections(ctx)? {
        let Some(section) = source.name() else {
            continue;
        };
        if !contains_ascii_case_insensitive(section, "resolvedfeatures") {
            continue;
        }
        let source_stream = source.source_stream();
        let (native_ref, _reference_storage) =
            ctx.with_scoped_storage("hold SLDPRT sketch native reference", || {
                ctx.format_retained(
                    format_args!(
                        "sldprt:feature-input:resolved-features#{}",
                        source.ordinal()
                    ),
                    "retain SLDPRT sketch native reference",
                )
            })?;
        for (stream_ordinal, stream) in ctx
            .admit_iter(source.ps_streams(), "project SLDPRT section streams")?
            .enumerate()
        {
            let brep =
                crate::brep::graph::decode(ctx, &stream.payload, &stream.header, source_stream)?;
            let (configuration, _configuration_storage) = ctx
                .with_scoped_storage("hold SLDPRT sketch configuration", || {
                    configuration(ctx, section)
                })?;
            project_brep(
                ctx,
                &brep,
                &BrepSketchSource {
                    block_offset: source.ordinal(),
                    stream_ordinal,
                    stream_offset: stream.offset,
                    source_stream,
                    sketch_name: &stream.header.description,
                    configuration: configuration.as_deref(),
                    native_ref: &native_ref,
                },
                annotations,
                &mut sketches,
                &mut entities,
                &mut constraints,
            )?;
        }
    }
    Ok(ProjectedSketches {
        sketches,
        entities,
        constraints,
    })
}

/// Where one decoded B-rep stream sits in its section and how its sketch is named.
struct BrepSketchSource<'a> {
    block_offset: usize,
    stream_ordinal: usize,
    stream_offset: usize,
    source_stream: &'a cadmpeg_ir::StreamName,
    sketch_name: &'a str,
    configuration: Option<&'a str>,
    native_ref: &'a str,
}

fn project_brep(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    brep: &crate::brep::graph::Brep,
    source: &BrepSketchSource<'_>,
    annotations: &mut Annotations,
    sketches: &mut Vec<Sketch>,
    entities: &mut Vec<SketchEntity>,
    constraints: &mut Vec<SketchConstraint>,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut temporary_storage =
        ctx.reserve_scoped(0, "SLDPRT sketch_projection temporary storage")?;

    let BrepSketchSource {
        block_offset,
        stream_ordinal,
        stream_offset,
        source_stream,
        sketch_name,
        configuration,
        native_ref,
    } = *source;
    let surfaces = temporary_storage.with_storage(|| {
        index_brep(ctx, &brep.surfaces, |surface| {
            (&surface.id, &surface.geometry)
        })
    })?;
    let loops = temporary_storage
        .with_storage(|| index_brep(ctx, &brep.loops, |loop_| (&loop_.id, loop_)))?;
    let coedges = temporary_storage
        .with_storage(|| index_brep(ctx, &brep.coedges, |coedge| (&coedge.id, coedge)))?;
    let edges =
        temporary_storage.with_storage(|| index_brep(ctx, &brep.edges, |edge| (&edge.id, edge)))?;
    let vertices = temporary_storage
        .with_storage(|| index_brep(ctx, &brep.vertices, |vertex| (&vertex.id, &vertex.point)))?;
    let points = temporary_storage.with_storage(|| {
        index_brep(ctx, &brep.points, |point| {
            (&point.id, point.position().get())
        })
    })?;
    let curves = temporary_storage
        .with_storage(|| index_brep(ctx, &brep.curves, |curve| (&curve.id, &curve.geometry)))?;

    for (face_ordinal, face) in ctx
        .admit_iter(&brep.faces, "project SLDPRT B-rep faces")?
        .enumerate()
    {
        let Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface))) = ctx
            .get_hash_map(
                &surfaces,
                &face.surface,
                "resolve SLDPRT sketch_projection keys",
            )?
            .copied()
        else {
            continue;
        };
        let origin = plane_surface.origin().get();
        let normal = plane_surface.frame().axis().as_raw();
        let u_axis = plane_surface.frame().reference().as_raw();
        let (sketch_id, sketch_id_storage) =
            ctx.with_scoped_storage("build SLDPRT projected sketch identity", || {
                mint_formatted::<SketchId>(
                    ctx,
                    format_args!(
                        "sldprt:model:sketch#{block_offset}:{stream_ordinal}:{face_ordinal}"
                    ),
                    "retain SLDPRT projected sketch ID",
                )
            })?;
        let Some(sketch_id) = sketch_id else {
            continue;
        };
        let Ok(placement) =
            cadmpeg_ir::sketches::SketchPlacement::try_resolved(origin, *normal, *u_axis)
        else {
            continue;
        };
        let v_axis = normal.cross(*u_axis);
        let first_entity = entities.len();
        let mut edge_entities = HashMap::<&cadmpeg_ir::ids::EdgeId, SketchEntityId>::new();
        let mut used_vertices = HashSet::<&cadmpeg_ir::ids::VertexId>::new();
        let mut profiles = Vec::new();
        let (outer, inner): (&[cadmpeg_ir::ids::LoopId], &[cadmpeg_ir::ids::LoopId]) =
            match &face.loops {
                cadmpeg_ir::topology::FaceLoops::Unspecified { loops } => (&[], loops.as_slice()),
                cadmpeg_ir::topology::FaceLoops::Classified { outer, inner } => {
                    (std::slice::from_ref(outer), inner.as_slice())
                }
            };
        for loop_id in ctx
            .admit_iter(outer, "project SLDPRT outer face loops")?
            .chain(ctx.admit_iter(inner, "project SLDPRT inner face loops")?)
        {
            let Some(loop_) =
                ctx.get_hash_map(&loops, loop_id, "resolve SLDPRT sketch_projection keys")?
            else {
                continue;
            };
            let mut profile = Vec::new();
            for coedge_id in ctx.admit_iter(loop_.coedges(), "project SLDPRT loop coedges")? {
                let Some(coedge) =
                    ctx.get_hash_map(&coedges, coedge_id, "resolve SLDPRT sketch_projection keys")?
                else {
                    continue;
                };
                let Some(edge) = ctx.get_hash_map(
                    &edges,
                    &coedge.edge,
                    "resolve SLDPRT sketch_projection keys",
                )?
                else {
                    continue;
                };
                for vertex_id in [&edge.start, &edge.end] {
                    temporary_storage.with_storage(|| {
                        ctx.insert_hash_set(
                            &mut used_vertices,
                            vertex_id,
                            "collect SLDPRT sketch used vertices",
                        )
                    })?;
                }
                let (entity_id, entity_id_storage) = if let Some(id) = ctx.get_hash_map(
                    &edge_entities,
                    &edge.id,
                    "resolve SLDPRT sketch_projection keys",
                )? {
                    ctx.with_scoped_storage("hold SLDPRT profile entity identity", || {
                        id.try_clone_for_decode(ctx, "retain SLDPRT sketch entity ID")
                    })?
                } else {
                    let (id, id_storage) = ctx.with_scoped_storage("hold SLDPRT profile entity identity", || mint_formatted::<SketchEntityId>(
                        ctx,
                        format_args!(
                            "sldprt:model:sketch-entity#{block_offset}:{stream_ordinal}:{face_ordinal}:{}",
                            edge_entities.len()
                        ),
                        "retain SLDPRT sketch entity ID",
                    ))?;
                    let Some(id) = id else {
                        continue;
                    };
                    let mut edge_refusal = crate::lane_refusal::LaneRefusals::new();
                    let projected = project_edge(
                        ctx,
                        edge,
                        &vertices,
                        &points,
                        &curves,
                        super::sketch_edges::SketchPlaneFrame {
                            origin,
                            u_axis: *u_axis,
                            v_axis,
                        },
                        &mut edge_refusal,
                    )?;
                    let edge_refusals = edge_refusal.take_records();
                    if !edge_refusals.is_empty() {
                        let operation = "retain SLDPRT sketch edge refusal";
                        let mut message = String::new();

                        for (index, record) in ctx
                            .admit_iter(&edge_refusals, "format SLDPRT edge refusal text")?
                            .enumerate()
                        {
                            if index != 0 {
                                ctx.append_retained(&mut message, "; ", operation)?;
                            }
                            ctx.append_retained(&mut message, record, operation)?;
                        }
                        return Err(cadmpeg_core::CodecError::Malformed(message));
                    }
                    let Some(geometry) = projected else {
                        continue;
                    };
                    let Some(start_point) = ctx.get_hash_map(
                        &vertices,
                        &edge.start,
                        "resolve SLDPRT sketch_projection keys",
                    )?
                    else {
                        continue;
                    };
                    let Some(end_point) = ctx.get_hash_map(
                        &vertices,
                        &edge.end,
                        "resolve SLDPRT sketch_projection keys",
                    )?
                    else {
                        continue;
                    };
                    crate::annotations::note(
                        ctx,
                        annotations,
                        id.as_str(),
                        source_stream,
                        0,
                        "feature_input_profile_edge",
                        Exactness::Derived,
                    )?;
                    let native_edge_ref = ctx.format_retained(
                        format_args!("{stream_ordinal}:{}", edge.id.as_str()),
                        "retain SLDPRT sketch native edge reference",
                    )?;
                    let geometry_ref = edge
                        .curve()
                        .map(|curve_id| {
                            ctx.format_retained(
                                format_args!("{stream_ordinal}:{}", curve_id.as_str()),
                                "retain SLDPRT sketch curve reference",
                            )
                        })
                        .transpose()?;
                    let mut endpoint_refs = Vec::new();
                    ctx.reserve_vec(
                        &mut endpoint_refs,
                        2,
                        "collect SLDPRT sketch edge endpoints",
                    )?;
                    for point in [start_point, end_point] {
                        endpoint_refs.push(ctx.format_retained(
                            format_args!("{stream_ordinal}:{}", point.as_str()),
                            "retain SLDPRT sketch edge endpoint",
                        )?);
                    }
                    let entity_id =
                        id.try_clone_for_decode(ctx, "retain SLDPRT sketch entity ID")?;
                    let entity_sketch_id =
                        sketch_id.try_clone_for_decode(ctx, "retain SLDPRT entity sketch ID")?;
                    ctx.reserve_vec(entities, 1, "collect SLDPRT sketch edge entities")?;
                    entities.push(
                        SketchEntity::new(entity_id, entity_sketch_id, geometry)
                            .with_native_ref(Some(native_edge_ref))
                            .with_geometry_ref(geometry_ref)
                            .with_endpoint_refs(endpoint_refs),
                    );

                    let index_id = temporary_storage.with_storage(|| {
                        id.try_clone_for_decode(ctx, "retain SLDPRT indexed sketch entity ID")
                    })?;
                    temporary_storage.with_storage(|| {
                        ctx.insert_hash_map(
                            &mut edge_entities,
                            &edge.id,
                            index_id,
                            "resolve SLDPRT sketch_projection keys",
                        )
                    })?;
                    (id, id_storage)
                };
                if edge.curve().is_some()
                    || !ctx.equal(
                        &edge.start,
                        &edge.end,
                        "compare SLDPRT projected edge endpoints",
                    )?
                {
                    ctx.reserve_vec(&mut profile, 1, "collect SLDPRT sketch profile uses")?;
                    profile.push(SketchEntityUse {
                        entity: entity_id,
                        reversed: coedge.sense == Sense::Reversed,
                    });
                    entity_id_storage.commit()?;
                }
            }
            if !profile.is_empty() {
                orient_closed_profile_by_topology(ctx, &mut profile, &entities[first_entity..])?;
                ctx.reserve_vec(&mut profiles, 1, "collect SLDPRT sketch profiles")?;
                profiles.push(profile);
            }
        }
        for vertex in ctx.admit_iter(&brep.vertices, "project SLDPRT sketch vertices")? {
            if ctx.contains_hash_set(
                &used_vertices,
                &vertex.id,
                "resolve SLDPRT sketch_projection keys",
            )? {
                continue;
            }
            let Some(position) = ctx.get_hash_map(
                &points,
                &vertex.point,
                "resolve SLDPRT sketch_projection keys",
            )?
            else {
                continue;
            };
            // Every entity this face appended belongs to its sketch, and no earlier face shares
            // the sketch identity.
            let owned_entity_count = entities.len() - first_entity;
            let entity_ordinal = edge_entities
                .len()
                .checked_add(owned_entity_count)
                .ok_or_else(|| {
                    ctx.refuse_codec_limit("count SLDPRT sketch entities", u64::MAX - 1, u64::MAX)
                })?;
            let Ok(geometry) = SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: project_point(*position, origin, *u_axis, v_axis),
            }) else {
                continue;
            };
            let Some(id) = mint_formatted::<SketchEntityId>(
                ctx,
                format_args!(
                    "sldprt:model:sketch-entity#{block_offset}:{stream_ordinal}:{face_ordinal}:{entity_ordinal}"
                ),
                "retain SLDPRT sketch point entity ID",
            )?
            else {
                continue;
            };
            crate::annotations::note(
                ctx,
                annotations,
                id.as_str(),
                source_stream,
                0,
                "feature_input_profile_point",
                Exactness::Derived,
            )?;
            let entity_sketch_id =
                sketch_id.try_clone_for_decode(ctx, "retain SLDPRT point sketch ID")?;
            let native_vertex_ref = ctx.format_retained(
                format_args!("{stream_ordinal}:{}", vertex.id.as_str()),
                "retain SLDPRT sketch native vertex reference",
            )?;
            let mut endpoint_refs = Vec::new();
            ctx.reserve_vec(
                &mut endpoint_refs,
                1,
                "collect SLDPRT sketch point endpoints",
            )?;
            endpoint_refs.push(ctx.format_retained(
                format_args!("{stream_ordinal}:{}", vertex.point.as_str()),
                "retain SLDPRT sketch point endpoint",
            )?);
            ctx.reserve_vec(entities, 1, "collect SLDPRT sketch point entities")?;
            entities.push(
                SketchEntity::new(id, entity_sketch_id, geometry)
                    .with_native_ref(Some(native_vertex_ref))
                    .with_endpoint_refs(endpoint_refs),
            );
        }
        let Ok(profiles) = cadmpeg_ir::sketches::SketchProfiles::try_from(profiles) else {
            continue;
        };
        if profiles.is_empty() && entities.len() == first_entity {
            continue;
        }
        crate::annotations::note(
            ctx,
            annotations,
            sketch_id.as_str(),
            source_stream,
            cadmpeg_core::decode::u64_from_index(stream_offset),
            "feature_input_profile",
            Exactness::Derived,
        )?;
        project_endpoint_constraints(
            ctx,
            EndpointConstraintSource {
                sketch: &sketch_id,
                entities: &entities[first_entity..],
                block_offset,
                stream_ordinal,
                face_ordinal,
                stream: source_stream,
            },
            annotations,
            constraints,
        )?;
        let name = (!sketch_name.is_empty())
            .then(|| ctx.copy_retained_text(sketch_name, "retain SLDPRT sketch name"))
            .transpose()?;
        let configuration = configuration
            .map(|name| ctx.copy_retained_text(name, "retain SLDPRT sketch configuration"))
            .transpose()?;
        let native_ref =
            ctx.copy_retained_text(native_ref, "retain SLDPRT sketch native reference")?;
        ctx.reserve_vec(sketches, 1, "collect SLDPRT projected sketches")?;
        sketch_id_storage.commit()?;
        sketches.push(Sketch {
            id: sketch_id,
            name,
            configuration,
            visible: None,
            placement,
            profiles,
            native_ref: Some(native_ref),
        });
    }
    Ok(())
}

fn orient_closed_profile_by_topology(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    profile: &mut [SketchEntityUse],
    entities: &[SketchEntity],
) -> Result<(), cadmpeg_core::CodecError> {
    let mut temporary_storage =
        ctx.reserve_scoped(0, "SLDPRT sketch_projection temporary storage")?;

    if profile.len() < 2 {
        return Ok(());
    }
    let entities = temporary_storage
        .with_storage(|| index_brep(ctx, entities, |entity| (entity.id(), entity)))?;
    let mut orientations = Vec::new();
    for (index, use_) in ctx
        .admit_iter(&*profile, "orient SLDPRT sketch profile")?
        .enumerate()
    {
        let Some(current) = ctx.get_hash_map(
            &entities,
            &use_.entity,
            "resolve SLDPRT sketch_projection keys",
        )?
        else {
            return Ok(());
        };
        let Some(next) = ctx.get_hash_map(
            &entities,
            &profile[(index + 1) % profile.len()].entity,
            "resolve SLDPRT sketch_projection keys",
        )?
        else {
            return Ok(());
        };
        let [start, end] = current.endpoint_refs.as_slice() else {
            return Ok(());
        };
        let operation = "compare SLDPRT profile endpoint incidence";
        let mut shared = None;
        for endpoint in ctx.admit_iter(&current.endpoint_refs, operation)? {
            if ctx.contains(&next.endpoint_refs, endpoint, operation)? {
                if shared.is_some() {
                    return Ok(());
                }
                shared = Some(endpoint);
            }
        }
        let Some(first) = shared else {
            return Ok(());
        };
        let reversed = if ctx.equal(first, end, operation)? {
            false
        } else if ctx.equal(first, start, operation)? {
            true
        } else {
            return Ok(());
        };
        temporary_storage.with_storage(|| {
            ctx.push_vec(
                &mut orientations,
                reversed,
                "collect SLDPRT sketch profile orientations",
            )
        })?;
    }
    for (index, reversed) in ctx
        .admit_iter(
            &(0..profile.len()),
            "apply SLDPRT sketch profile orientations",
        )?
        .zip(ctx.admit_iter(&orientations, "apply SLDPRT sketch profile orientations")?)
    {
        profile[index].reversed = *reversed;
    }
    Ok(())
}

#[cfg(test)]
mod projected_profile_orientation_tests {
    use super::{index_brep, orient_closed_profile_by_topology};
    use cadmpeg_ir::{
        math::Point2,
        sketches::{
            SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
            SketchGeometryDefinition, SketchId,
        },
    };

    fn orient_with_service(profile: &mut [SketchEntityUse], entities: &[SketchEntity]) {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .expect("empty root fits service policy");
        orient_closed_profile_by_topology(&ctx, profile, entities)
            .expect("service policy admits profile orientation");
    }

    #[test]
    fn sketch_projection_index_refuses_collection_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 1;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root fits policy");
        let error = index_brep(&ctx, &[1, 2], |value| (*value, *value))
            .expect_err("two B-rep records exceed the collection limit");
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
        ));
    }

    fn line(id: &str, start_ref: &str, end_ref: &str) -> SketchEntity {
        SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            SketchId::mint("synthetic:test:id#sketch").unwrap(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(1.0, 0.0),
            })
            .unwrap(),
        )
        .with_endpoint_refs(vec![start_ref.into(), end_ref.into()])
    }

    #[test]
    fn orients_each_closed_profile_edge_toward_its_topological_successor() {
        let entities = [
            line("synthetic:test:id#a", "p0", "p1"),
            line("synthetic:test:id#b", "p1", "p2"),
            line("synthetic:test:id#c", "p0", "p2"),
        ];
        let mut profile = entities
            .iter()
            .map(|entity| SketchEntityUse {
                entity: entity.id().clone(),
                reversed: true,
            })
            .collect::<Vec<_>>();

        orient_with_service(&mut profile, &entities);

        assert_eq!(
            profile.iter().map(|use_| use_.reversed).collect::<Vec<_>>(),
            [false, false, true]
        );
    }

    #[test]
    fn preserves_all_orientations_when_endpoint_incidence_is_ambiguous() {
        let entities = [
            line("synthetic:test:id#a", "p0", "p1"),
            line("synthetic:test:id#b", "p1", "p2"),
            line("synthetic:test:id#c", "p3", "p4"),
        ];
        let mut profile = entities
            .iter()
            .map(|entity| SketchEntityUse {
                entity: entity.id().clone(),
                reversed: true,
            })
            .collect::<Vec<_>>();

        orient_with_service(&mut profile, &entities);

        assert!(profile.iter().all(|use_| use_.reversed));
    }
}

#[cfg(test)]
mod projected_brep_output_tests {
    use super::{project_brep, BrepSketchSource};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_ir::geometry::{
        analytic::PlaneSurface, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{FaceId, PointId, ShellId, SurfaceId, VertexId};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::topology::{Face, FaceLoops, Point, Sense, Vertex};

    fn point_brep() -> crate::brep::graph::Brep {
        let surface_id = SurfaceId::mint("test:model:surface#plane").expect("surface ID");
        let point_id = PointId::mint("test:model:point#free").expect("point ID");
        crate::brep::graph::Brep {
            surfaces: vec![Surface {
                id: surface_id.clone(),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    PlaneSurface::try_new(
                        Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(0.0, 0.0, 1.0),
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .expect("plane frame"),
                )),
                source_object: None,
            }],
            faces: vec![Face {
                id: FaceId::mint("test:model:face#plane").expect("face ID"),
                shell: ShellId::mint("test:model:shell#one").expect("shell ID"),
                surface: surface_id,
                sense: Sense::Forward,
                loops: FaceLoops::unspecified(Vec::new()),
                name: None,
                color: None,
                tolerance: None,
            }],
            vertices: vec![Vertex {
                id: VertexId::mint("test:model:vertex#free").expect("vertex ID"),
                point: point_id.clone(),
                tolerance: None,
            }],
            points: vec![Point::new(
                point_id,
                cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 0.0, 0.0))
                    .expect("finite position"),
                None,
            )],
            ..Default::default()
        }
    }

    #[test]
    fn sketch_projection_output_refuses_retained_limit() {
        let brep = point_brep();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root fits policy");
        let stream = cadmpeg_ir::stream_name!("test:sketch-projection");
        let mut annotations = cadmpeg_ir::annotations::Annotations::default();
        let mut sketches = Vec::new();
        let mut entities = Vec::new();
        let mut constraints = Vec::new();
        let error = project_brep(
            &ctx,
            &brep,
            &BrepSketchSource {
                block_offset: 0,
                stream_ordinal: 0,
                stream_offset: 0,
                source_stream: &stream,
                sketch_name: "point sketch",
                configuration: None,
                native_ref: "native:point",
            },
            &mut annotations,
            &mut sketches,
            &mut entities,
            &mut constraints,
        )
        .expect_err("projected point identity exceeds retained limit");
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::RetainedBytes
        ));
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("empty root fits service policy");
        let mut annotations = cadmpeg_ir::annotations::Annotations::default();
        let mut sketches = Vec::new();
        let mut entities = Vec::new();
        let mut constraints = Vec::new();
        project_brep(
            &ctx,
            &brep,
            &BrepSketchSource {
                block_offset: 0,
                stream_ordinal: 0,
                stream_offset: 0,
                source_stream: &stream,
                sketch_name: "point sketch",
                configuration: None,
                native_ref: "native:point",
            },
            &mut annotations,
            &mut sketches,
            &mut entities,
            &mut constraints,
        )
        .expect("service policy admits the point sketch");
        assert_eq!(sketches.len(), 1);
        assert_eq!(entities.len(), 1);
        assert_eq!(sketches[0].name.as_deref(), Some("point sketch"));
        assert_eq!(entities[0].endpoint_refs, ["0:test:model:point#free"]);
    }
}

#[cfg(test)]
mod sketch_projection_tests;
