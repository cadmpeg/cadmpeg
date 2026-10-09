// SPDX-License-Identifier: Apache-2.0
//! Emit topology arenas and prune unresolved graph members.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_ir::Exactness;
use cadmpeg_ir::annotations::AnnotationBuilder;
use cadmpeg_ir::annotations::StreamHandle;
use cadmpeg_ir::geometry::BlendCrossSection;
use cadmpeg_ir::geometry::BlendRadiusLaw;
use cadmpeg_ir::geometry::BlendSupport;
use cadmpeg_ir::geometry::Curve;
use cadmpeg_ir::geometry::CurveGeometry;
use cadmpeg_ir::geometry::ProceduralSurface;
use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;
use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::geometry::Surface;
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::geometry::pcurve::Pcurve;
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::ids::CoedgeId;
use cadmpeg_ir::ids::LoopId;
use cadmpeg_ir::ids::PcurveId;
use cadmpeg_ir::ids::ProceduralSurfaceId;
use cadmpeg_ir::ids::RegionId;
use cadmpeg_ir::ids::ShellId;
use cadmpeg_ir::topology::Body;
use cadmpeg_ir::topology::BodyKind;
use cadmpeg_ir::topology::Coedge;
use cadmpeg_ir::topology::Edge;
use cadmpeg_ir::topology::Face;
use cadmpeg_ir::topology::Loop;
use cadmpeg_ir::topology::Point;
use cadmpeg_ir::topology::Region;
use cadmpeg_ir::topology::Sense;
use cadmpeg_ir::topology::Vertex;
use crate::brep::CurveCarrier;
use crate::brep::LEN_TO_MM;
use crate::brep::attrib;
use crate::brep::blend::BlendSupportRef;
use crate::brep::entity;
use crate::brep::graph::shells::ShellFaceIndex;
use crate::brep::index::IndexedCurve;
use crate::brep::index::view::CarrierView;
use crate::brep::topology;
use crate::brep::typed;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashSet;
use super::Brep;
use super::construction::BrepSink;
use super::pcurves::IntersectionPcurveSource;
use super::OwnedFaceColor;
use super::Stats;
use super::construction::SweepSurfaceResolution;
use super::WalkedFace;
use super::admit_brep_entity;
use super::bind_bridges;
use super::body_namespace;
use super::canonical_coedge_attr;
use super::copy_graph_stream_name;
use super::pcurves::derive_pcurves;
use super::edge_end_vuse;
use super::edge_parameter_range;
use super::construction::emit_offset_surface;
use super::emit_shell_components;
use super::construction::ensure_surface_support;
use super::forward_coedges_by_edge;
use super::id_closed_point;
use super::id_closed_vertex;
use super::id_coedge;
use super::id_curve;
use super::id_edge;
use super::id_face;
use super::id_loop;
use super::id_point;
use super::id_surf;
use super::id_vertex;
use super::pcurves::intersection_support_pcurve;
use super::pcurve_namespace;
use super::region_namespace;
use super::construction::resolve_sweep_surface;
use super::shell_namespace;
use super::orientation::solve_face_orientation;
use super::sorted_topology_sequences;
use super::construction::support_is_acyclic;
use super::surface_sense;
use super::seams::synthesize_cylinder_seams;
use super::seams::synthesize_sphere_seams;
use super::typed_body_records;
use super::unique_body_modifiers;
use super::unique_face_colors;
use super::walk_face;

pub(super) fn decode_graph(
    ctx: &DecodeContext<'_>,
    carriers: &CarrierView<'_, '_>,
    t: &topology::Tables,
    entity_facts: entity::FactsSource<'_>,
    typed_facts: &typed::Facts,
    stream: &cadmpeg_ir::StreamName,
) -> Result<Brep, cadmpeg_core::CodecError> {
    let (entity_facts, _entity_fact_storage) = entity_facts.into_parts();
    let (typed_records, _body_record_storage) = ctx
        .with_scoped_storage("hold Parasolid body records", || {
            typed_body_records(ctx, typed_facts, t)
        })?;
    let body_records = typed_records.unwrap_or_default();
    let body_modifiers = unique_body_modifiers(ctx, entity_facts.body_modifiers)?;
    let (face_colors, conflicting_face_colors) = unique_face_colors(
        ctx,
        entity_facts.face_colors,
        entity_facts.face_color_versions,
    )?;
    let face_bridge_sequences = sorted_topology_sequences(
        ctx,
        t.bridges()
            .values()
            .map(|bridge| (bridge.sequence, bridge.attr)),
        "collect Parasolid face bridge sequences",
    )?;
    let edge_use_sequences = sorted_topology_sequences(
        ctx,
        t.edge_uses()
            .values()
            .map(|edge_use| (edge_use.sequence, edge_use.attr)),
        "collect Parasolid edge use sequences",
    )?;
    let vertex_use_sequences = sorted_topology_sequences(
        ctx,
        t.vertex_uses()
            .values()
            .map(|vertex_use| (vertex_use.sequence, vertex_use.attr)),
        "collect Parasolid vertex use sequences",
    )?;

    let owned_face_colors = ctx.try_collect_vec(
        face_colors.into_iter().map(|value| {
            Ok::<_, cadmpeg_core::CodecError>(OwnedFaceColor {
                value,
                source_stream: copy_graph_stream_name(ctx, stream)?,
                site_key: None,
            })
        }),
        "collect Parasolid face colors",
    )?;
    let mut out = Brep {
        face_colors: owned_face_colors,
        face_bridge_sequences,
        edge_use_sequences,
        vertex_use_sequences,
        body_modifiers,
        losses: carriers.clone_lane_refusals(ctx)?,
        stats: Stats {
            source_entity_records: entity_facts.entity_count,
            unresolved_face_colors: entity_facts.unresolved_face_colors + conflicting_face_colors,
            ..Stats::default()
        },
        ..Brep::default()
    };
    let mut annotations = AnnotationBuilder::new();
    let source_stream = StreamHandle::new(
        ctx,
        copy_graph_stream_name(ctx, stream)?,
        "allocate annotation stream handle",
    )?;
    if t.bridges().is_empty() {
        return Ok(out);
    }
    // Indexes and membership sets that only steer this walk are held here,
    // apart from the arenas the walk emits.
    let mut scratch = ctx.reserve_scoped(0, "SLDPRT graph walk scratch")?;

    // Walk every face-use bridge to collect its ordered loop/coedge structure.
    // A bridge owner identifies the canonical face entity, not an additional
    // face identity. Equivalent bridge payloads are duplicate uses; distinct
    // payloads have no source selector and must remain unresolved together.
    let mut faces = Vec::new();
    let mut owned_faces = BTreeMap::<u16, Vec<(&topology::Bridge, WalkedFace)>>::new();
    for (_, bridge) in ctx.admit_iter(t.bridges(), "walk Parasolid face bridges")? {
        let face = scratch.with_storage(|| walk_face(ctx, bridge, t))?;
        if let Some(owner) = bridge.owner {
            scratch.with_storage(|| {
                ctx.push_btree_group(
                    &mut owned_faces,
                    owner,
                    (bridge, face),
                    "index Parasolid face owners",
                    "collect Parasolid face uses",
                )
            })?;
        } else {
            scratch.with_storage(|| ctx.push_vec(&mut faces, face, "collect Parasolid faces"))?;
        }
    }
    let mut ambiguous_face_owners = 0;
    for (_, mut uses) in ctx.admit_iter(owned_faces, "resolve Parasolid face owners")? {
        ctx.stable_sort_by_key(
            &mut uses,
            |value| (value.0.offset, value.0.attr),
            Ord::cmp,
            "sort Parasolid face owners",
        )?;
        let Some((first_bridge, first_face)) = uses.first() else {
            continue;
        };
        let equivalent = ctx.all_by(
            &uses[1..],
            |(bridge, face)| {
                Ok(bridge.refs == first_bridge.refs
                    && bridge.sense == first_bridge.sense
                    && face.surface_attr == first_face.surface_attr
                    && face.sense == first_face.sense
                    && face.loops.len() == first_face.loops.len()
                    && ctx.equal(
                        &face.loops,
                        &first_face.loops,
                        "compare Parasolid face uses",
                    )?)
            },
            "compare Parasolid face uses",
        )?;
        if equivalent {
            if let Some((_, first_face)) = uses.into_iter().next() {
                scratch.with_storage(|| {
                    ctx.push_vec(&mut faces, first_face, "collect Parasolid faces")
                })?;
            }
        } else {
            ambiguous_face_owners += 1;
        }
    }
    ctx.stable_sort_by(
        &mut faces,
        |value| &value.bridge_attr,
        Ord::cmp,
        "sort Parasolid topology",
    )?;
    out.stats.ambiguous_face_owners += ambiguous_face_owners;

    // Edge attr -> [(coedge attr, start vuse, next coedge's start vuse)] from
    // the ring walk. The ring order supplies a boundary edge's second endpoint;
    // a reciprocal twin supplies it for a two-sided edge.
    let mut edge_incidence: BTreeMap<u16, Vec<(u16, u16, u16)>> = BTreeMap::new();

    for f in ctx.admit_iter(&faces, "index Parasolid face edge incidences")? {
        for (_loop_attr, ring) in
            ctx.admit_iter(&f.loops, "index Parasolid loop edge incidences")?
        {
            let k = ring.len();
            for (i, &ce_attr) in ctx
                .admit_iter(ring, "index Parasolid edge incidences")?
                .enumerate()
            {
                let Some(ce) =
                    ctx.get_btree_map(t.coedges(), &ce_attr, "lookup Parasolid coedges")?
                else {
                    continue;
                };
                let next_attr = ring[(i + 1) % k];
                let start_vuse = ce.refs[4];
                let next_vuse = ctx
                    .get_btree_map(t.coedges(), &next_attr, "lookup Parasolid coedges")?
                    .map_or(0, |next| next.refs[4]);
                let edge_attr = ce.refs[6];
                if edge_attr != 0 {
                    scratch.with_storage(|| {
                        ctx.push_btree_group(
                            &mut edge_incidence,
                            edge_attr,
                            (ce_attr, start_vuse, next_vuse),
                            "index Parasolid edge incidences",
                            "collect Parasolid edge incidences",
                        )
                    })?;
                }
            }
        }
    }
    let forward_coedges = scratch.with_storage(|| forward_coedges_by_edge(ctx, t.coedges()))?;

    // Kept-entity sets, so only chain-reachable records are emitted.
    let mut kept_vertices: BTreeSet<u16> = BTreeSet::new();
    let mut kept_points: BTreeSet<u16> = BTreeSet::new();
    // Edge attr -> (canonical start vuse, canonical end vuse, curve carrier attr).
    let mut edge_ends: BTreeMap<u16, (u16, u16, u16)> = BTreeMap::new();

    for (edge_attr, incidences) in
        ctx.admit_iter(edge_incidence, "resolve Parasolid edge incidences")?
    {
        let canonical = canonical_coedge_attr(
            ctx,
            edge_attr,
            ctx.get_btree_map(t.edge_uses(), &edge_attr, "lookup Parasolid edge_uses")?,
            t.coedges(),
            &forward_coedges,
        )?;
        let Some(canonical) = canonical else {
            continue;
        };
        let Some(&(_, start_vuse, ring_end_vuse)) = ctx.find_by(
            &incidences,
            |(coedge_attr, _, _)| Ok(*coedge_attr == canonical),
            "find Parasolid canonical edge incidence",
        )?
        else {
            continue;
        };
        let end_vuse = edge_end_vuse(ctx, canonical, ring_end_vuse, t.coedges())?;
        let curve_attr = ctx
            .get_btree_map(t.edge_uses(), &edge_attr, "lookup Parasolid edge_uses")?
            .map_or(0, |edge_use| edge_use.references.curve());
        scratch.with_storage(|| {
            ctx.insert_btree_map(
                &mut edge_ends,
                edge_attr,
                (start_vuse, end_vuse, curve_attr),
                "index Parasolid edge endpoints",
            )
        })?;
        for vuse in [start_vuse, end_vuse] {
            if vuse == 0 {
                continue;
            }
            if let Some(vu) =
                ctx.get_btree_map(t.vertex_uses(), &vuse, "lookup Parasolid vertex_uses")?
            {
                let point_attr = vu.refs[4];
                if ctx.contains_key_btree_map(t.points(), &point_attr, "lookup Parasolid points")? {
                    scratch.with_storage(|| {
                        ctx.insert_btree_set(&mut kept_vertices, vuse, "track Parasolid vertices")
                    })?;
                    scratch.with_storage(|| {
                        ctx.insert_btree_set(&mut kept_points, point_attr, "track Parasolid points")
                    })?;
                }
            }
        }
    }

    // Points, in attr order.
    for &a in ctx.admit_iter(&kept_points, "emit Parasolid points")? {
        let Some(rec) = ctx.get_btree_map(t.points(), &a, "lookup Parasolid points")? else {
            continue;
        };
        annotations.note(
            ctx,
            id_point(a).as_str(),
            &source_stream,
            u64_from_index(rec.offset),
            Some("00_1d"),
        )?;
        let [x, y, z] = rec.xyz_m;
        let finite_position = cadmpeg_ir::features::FinitePoint3::new(
            cadmpeg_ir::math::Point3::new(x * LEN_TO_MM, y * LEN_TO_MM, z * LEN_TO_MM),
        )
        .ok_or(Point::NON_FINITE_POSITION)
        .map_err(cadmpeg_core::CodecError::malformed)?;
        admit_brep_entity(ctx)?;
        ctx.reserve_vec(&mut out.points, 1, "collect Parasolid points")?;
        out.points
            .push(Point::new(id_point(a), finite_position, None));
    }

    // Vertices, in attr order.
    for &a in ctx.admit_iter(&kept_vertices, "emit Parasolid vertices")? {
        let Some(rec) = ctx.get_btree_map(t.vertex_uses(), &a, "lookup Parasolid vertex_uses")?
        else {
            continue;
        };
        let point_attr = rec.refs[4];
        annotations.note(
            ctx,
            id_vertex(a).as_str(),
            &source_stream,
            u64_from_index(rec.offset),
            Some("00_12"),
        )?;
        admit_brep_entity(ctx)?;
        ctx.reserve_vec(&mut out.vertices, 1, "collect Parasolid vertices")?;
        out.vertices.push(Vertex {
            id: id_vertex(a),
            point: id_point(point_attr),
            tolerance: None,
        });
    }

    // Curves and edges. An edge keeps a curve only when its carrier decodes to a
    // curve kind; a nonzero-but-untyped carrier is counted as loss.
    let mut emitted_curves: BTreeSet<u16> = BTreeSet::new();
    let mut edge_set: BTreeSet<u16> = BTreeSet::new();
    let mut edge_endpoint_positions = BTreeMap::<u16, [cadmpeg_ir::math::Point3; 2]>::new();
    let mut reversed_edge_orientation = BTreeSet::<u16>::new();
    for (&e, &(start_v, end_v, curve_attr)) in ctx.admit_iter(&edge_ends, "emit Parasolid edges")? {
        let resolved_endpoints =
            ctx.contains_btree_set(&kept_vertices, &start_v, "test Parasolid kept_vertices")?
                && ctx.contains_btree_set(
                    &kept_vertices,
                    &end_v,
                    "test Parasolid kept_vertices",
                )?;
        let closed_carrier = if !resolved_endpoints && start_v <= 1 && end_v <= 1 {
            carriers.curve(ctx, curve_attr)?
        } else {
            None
        };
        let closed_circle_point =
            closed_carrier.and_then(|carrier| match &carrier.carrier().geometry {
                CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) => {
                    let center = circle_curve.center().get();
                    let ref_direction = circle_curve.frame().reference().as_raw();
                    let radius = circle_curve.radius().get();
                    Some(cadmpeg_ir::math::Point3::new(
                        center.x + ref_direction.x * radius,
                        center.y + ref_direction.y * radius,
                        center.z + ref_direction.z * radius,
                    ))
                }
                _ => None,
            });
        if !resolved_endpoints && closed_circle_point.is_none() {
            continue;
        }
        let (mut start_id, mut end_id) = if let Some(position) = closed_circle_point {
            let point_id = id_closed_point(e);
            let vertex_id = id_closed_vertex(e);
            annotations.note(
                ctx,
                point_id.as_str(),
                &source_stream,
                0,
                Some("derived_closed_circle_seam"),
            )?;
            annotations.exactness(ctx, point_id.as_str(), Exactness::Derived)?;
            annotations.note(
                ctx,
                vertex_id.as_str(),
                &source_stream,
                0,
                Some("derived_closed_circle_seam"),
            )?;
            annotations.exactness(ctx, vertex_id.as_str(), Exactness::Derived)?;
            let finite_position = cadmpeg_ir::features::FinitePoint3::new(position)
                .ok_or(Point::NON_FINITE_POSITION)
                .map_err(cadmpeg_core::CodecError::malformed)?;
            admit_brep_entity(ctx)?;
            ctx.reserve_vec(&mut out.points, 1, "collect Parasolid closed-circle points")?;
            out.points.push(Point::new(
                point_id.try_clone_for_decode(ctx, "SLDPRT decoded identity copy")?,
                finite_position,
                None,
            ));
            admit_brep_entity(ctx)?;
            ctx.reserve_vec(
                &mut out.vertices,
                1,
                "collect Parasolid closed-circle vertices",
            )?;
            out.vertices.push(Vertex {
                id: vertex_id.try_clone_for_decode(ctx, "SLDPRT decoded identity copy")?,
                point: point_id,
                tolerance: None,
            });
            (
                vertex_id.try_clone_for_decode(ctx, "SLDPRT decoded identity copy")?,
                vertex_id,
            )
        } else {
            (id_vertex(start_v), id_vertex(end_v))
        };
        if resolved_endpoints {
            let position = |vertex_use: u16| -> Result<
                Option<cadmpeg_ir::math::Point3>,
                cadmpeg_core::CodecError,
            > {
                let Some(vertex) = ctx.get_btree_map(
                    t.vertex_uses(),
                    &vertex_use,
                    "lookup Parasolid vertex uses",
                )?
                else {
                    return Ok(None);
                };
                let Some(point) =
                    ctx.get_btree_map(t.points(), &vertex.refs[4], "lookup Parasolid points")?
                else {
                    return Ok(None);
                };
                let [x, y, z] = point.xyz_m;
                Ok(Some(cadmpeg_ir::math::Point3::new(
                    x * LEN_TO_MM,
                    y * LEN_TO_MM,
                    z * LEN_TO_MM,
                )))
            };
            if let (Some(start), Some(end)) = (position(start_v)?, position(end_v)?) {
                scratch.with_storage(|| {
                    ctx.insert_btree_map(
                        &mut edge_endpoint_positions,
                        e,
                        [start, end],
                        "index Parasolid edge endpoint positions",
                    )
                })?;
            }
        }
        let parameter_range = carriers
            .curve(ctx, curve_attr)?
            .map(|carrier| {
                edge_parameter_range(
                    ctx,
                    carrier.carrier(),
                    ctx.get_btree_map(
                        &edge_endpoint_positions,
                        &e,
                        "lookup Parasolid edge_endpoint_positions",
                    )?
                    .copied(),
                )
            })
            .transpose()?
            .flatten();
        if parameter_range.is_some_and(|(_, reversed)| reversed) {
            std::mem::swap(&mut start_id, &mut end_id);
            if let Some(endpoints) = ctx.get_mut_btree_map(
                &mut edge_endpoint_positions,
                &e,
                "lookup Parasolid edge_endpoint_positions",
            )? {
                endpoints.swap(0, 1);
            }
            scratch.with_storage(|| {
                ctx.insert_btree_set(
                    &mut reversed_edge_orientation,
                    e,
                    "track reversed Parasolid edges",
                )
            })?;
        }
        let eu = ctx.get_btree_map(t.edge_uses(), &e, "lookup Parasolid edge_uses")?;
        let mut curve = None;
        if curve_attr != 0 {
            match carriers.curve(ctx, curve_attr)? {
                Some(indexed) => {
                    let carrier = indexed.carrier();
                    if scratch.with_storage(|| {
                        ctx.insert_btree_set(
                            &mut emitted_curves,
                            curve_attr,
                            "track emitted Parasolid curves",
                        )
                    })? {
                        emit_curve(ctx, &mut out, carrier)?;
                        if matches!(indexed, IndexedCurve::Derived(_)) {
                            let offset = carrier.offset;
                            annotations.note(
                                ctx,
                                id_curve(curve_attr).as_str(),
                                &source_stream,
                                u64_from_index(offset),
                                Some("surface_intersection"),
                            )?;
                            annotations.exactness(
                                ctx,
                                id_curve(curve_attr).as_str(),
                                Exactness::Derived,
                            )?;
                        }
                    }
                    curve = Some(id_curve(curve_attr));
                }
                _ => {
                    if scratch.with_storage(|| {
                        ctx.insert_btree_set(
                            &mut emitted_curves,
                            curve_attr,
                            "track emitted Parasolid curves",
                        )
                    })? {
                        let offset = eu.map_or(0, |record| record.offset);
                        annotations.note(
                            ctx,
                            id_curve(curve_attr).as_str(),
                            &source_stream,
                            u64_from_index(offset),
                            Some("unknown_curve"),
                        )?;
                        annotations.exactness(
                            ctx,
                            id_curve(curve_attr).as_str(),
                            Exactness::Unknown,
                        )?;
                        admit_brep_entity(ctx)?;
                        ctx.reserve_vec(&mut out.curves, 1, "collect unknown Parasolid curves")?;
                        out.curves.push(Curve {
                            id: id_curve(curve_attr),
                            source_object: None,
                            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
                                record: None,
                            }),
                        });
                    }
                    curve = Some(id_curve(curve_attr));
                    out.stats.unknown_curve_edges += 1;
                }
            }
        }
        let off = eu.map_or(0, |r| r.offset);
        annotations.note(
            ctx,
            id_edge(e).as_str(),
            &source_stream,
            u64_from_index(off),
            Some("00_10"),
        )?;
        admit_brep_entity(ctx)?;
        ctx.reserve_vec(&mut out.edges, 1, "collect Parasolid edges")?;
        out.edges.push(Edge {
            id: id_edge(e),
            carrier: cadmpeg_ir::topology::EdgeCarrier::new(
                curve,
                parameter_range.map(|(range, _)| range),
            )
            .map_err(cadmpeg_core::CodecError::malformed)?,
            start: start_id,
            end: end_id,
            tolerance: None,
        });
        scratch.with_storage(|| {
            ctx.insert_btree_set(&mut edge_set, e, "track emitted Parasolid edges")
        })?;
    }

    // A loop is kept only when its whole ring resolves: every coedge exists and
    // its edge was emitted. A partial ring is dropped whole, so an emitted
    // coedge's `next`/`prev` never dangle and every emitted loop closes.
    let mut kept_loops: BTreeSet<u16> = BTreeSet::new();
    let mut emitted_coedges = BTreeSet::new();
    for f in ctx.admit_iter(&faces, "check Parasolid face loop rings")? {
        for (loop_attr, ring) in ctx.admit_iter(&f.loops, "check Parasolid loop rings")? {
            let ok = !ring.is_empty()
                && ctx.all_by(
                    ring,
                    |c| match ctx.get_btree_map(t.coedges(), c, "lookup Parasolid coedges")? {
                        Some(ce) => ctx.contains_btree_set(
                            &edge_set,
                            &ce.refs[6],
                            "test emitted Parasolid edges",
                        ),
                        None => Ok(false),
                    },
                    "check Parasolid loop ring",
                )?;
            if !ok {
                continue;
            }
            scratch.with_storage(|| {
                ctx.insert_btree_set(&mut kept_loops, *loop_attr, "track kept Parasolid loops")
            })?;
            for &coedge in ctx.admit_iter(ring, "index emitted Parasolid coedges")? {
                scratch.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut emitted_coedges,
                        coedge,
                        "track emitted Parasolid coedges",
                    )
                })?;
            }
        }
    }

    // Coedges of kept loops: `next`/`prev` from the ring order, partner from a
    // mutual twin that is itself emitted.
    for f in ctx.admit_iter(&faces, "emit Parasolid face coedges")? {
        for (loop_attr, ring) in ctx.admit_iter(&f.loops, "emit Parasolid loop coedges")? {
            if !ctx.contains_btree_set(&kept_loops, loop_attr, "test Parasolid kept_loops")? {
                continue;
            }
            for &ce_attr in ctx.admit_iter(ring, "emit Parasolid coedges")? {
                let Some(ce) =
                    ctx.get_btree_map(t.coedges(), &ce_attr, "lookup Parasolid coedges")?
                else {
                    continue;
                };
                let edge_attr = ce.refs[6];
                let twin = ce.refs[5];
                let partner =
                    match ctx.get_btree_map(t.coedges(), &twin, "lookup Parasolid coedges")? {
                        Some(tw)
                            if tw.refs[5] == ce_attr
                                && ctx.contains_btree_set(
                                    &emitted_coedges,
                                    &twin,
                                    "test emitted Parasolid coedges",
                                )? =>
                        {
                            Some(id_coedge(twin))
                        }
                        _ => None,
                    };
                annotations.note(
                    ctx,
                    id_coedge(ce_attr).as_str(),
                    &source_stream,
                    u64_from_index(ce.offset),
                    Some("00_11"),
                )?;
                let mut pcurve_refusal = crate::lane_refusal::LaneRefusals::new();
                let pcurve_refusal = &mut pcurve_refusal;
                let pcurves = if let Some((_, _, curve_attr)) =
                    ctx.get_btree_map(&edge_ends, &edge_attr, "lookup Parasolid edge_ends")?
                {
                    (|| -> Result<_, cadmpeg_core::CodecError> {
                        let Some(IndexedCurve::Derived(intersection)) =
                            carriers.curve(ctx, *curve_attr)?
                        else {
                            return Ok(None);
                        };
                        let support_data = &intersection.support_data;
                        let curve_carrier = &intersection.carrier;
                        let Some(SolvedCurveGeometry::Nurbs(curve)) =
                            curve_carrier.geometry.solved()
                        else {
                            return Ok(None);
                        };
                        let Some(surface) = carriers.surface(ctx, f.surface_attr)? else {
                            return Ok(None);
                        };
                        let surface = &surface.geometry;
                        let Some(endpoint_positions) = ctx.get_btree_map(
                            &edge_endpoint_positions,
                            &edge_attr,
                            "lookup Parasolid edge_endpoint_positions",
                        )?
                        else {
                            return Ok(None);
                        };
                        let Some((geometry, parameter_range, source)) =
                            intersection_support_pcurve(
                                ctx,
                                support_data,
                                curve,
                                f.surface_attr,
                                surface,
                                *endpoint_positions,
                                pcurve_refusal,
                            )?
                        else {
                            return Ok(None);
                        };
                        let Some(finite_range) =
                            cadmpeg_ir::units::FiniteVector::new(parameter_range)
                        else {
                            return Ok(None);
                        };
                        let Ok(fit_tolerance) = cadmpeg_ir::geometry::FitTolerance::try_new(
                            support_data.fit_tolerance_mm,
                        ) else {
                            return Ok(None);
                        };
                        let id = PcurveId::compose(
                            &pcurve_namespace(),
                            cadmpeg_ir::identity_key!("intersection:").then(ce_attr),
                        );
                        let offset = curve_carrier.offset;
                        annotations.note(
                            ctx,
                            id.as_str(),
                            &source_stream,
                            u64_from_index(offset),
                            Some(match source {
                                IntersectionPcurveSource::StoredCache => "surface_intersection_uv",
                                IntersectionPcurveSource::AnalyticInverse => {
                                    "derived_intersection_analytic_uv"
                                }
                                IntersectionPcurveSource::NurbsInverse => {
                                    "derived_intersection_nurbs_uv"
                                }
                            }),
                        )?;
                        annotations.exactness(ctx, id.as_str(), Exactness::Derived)?;
                        admit_brep_entity(ctx)?;
                        ctx.reserve_vec(
                            &mut out.pcurves,
                            1,
                            "collect intersection Parasolid pcurves",
                        )?;
                        out.pcurves.push(Pcurve {
                            id: id.try_clone_for_decode(ctx, "SLDPRT decoded identity copy")?,
                            geometry,
                            metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::general(
                                None,
                                Some(finite_range),
                                Some(fit_tolerance),
                            ),
                        });
                        Ok(Some(
                            cadmpeg_ir::geometry::DirectedParameterRange::new(parameter_range).map(
                                |range| {
                                    vec![cadmpeg_ir::topology::PcurveUse {
                                        pcurve: id,
                                        isoparametric: None,
                                        parameter_range: Some(range),
                                    }]
                                },
                            ),
                        ))
                    })()?
                } else {
                    None
                };
                // The sink is drained before the `?` below: an error on that
                // route must not drop a refusal the walk above already pushed.
                for record in ctx.admit_iter(
                    pcurve_refusal.take_records(),
                    "collect intersection pcurve losses",
                )? {
                    let note = crate::loss::spline_lane_refusal(
                        ctx,
                        format_args!("intersection pcurve for coedge {ce_attr}: {record}"),
                    )?;
                    ctx.reserve_vec(&mut out.losses, 1, "collect intersection pcurve losses")?;
                    out.losses.push(note);
                }
                let pcurves = pcurves
                    .transpose()
                    .map_err(cadmpeg_core::CodecError::malformed)?
                    .unwrap_or_default();
                let mut sense = ce.sense;
                if ctx.contains_btree_set(
                    &reversed_edge_orientation,
                    &edge_attr,
                    "test Parasolid reversed_edge_orientation",
                )? {
                    sense = match sense {
                        Sense::Forward => Sense::Reversed,
                        Sense::Reversed => Sense::Forward,
                    };
                }
                admit_brep_entity(ctx)?;
                ctx.reserve_vec(&mut out.coedges, 1, "collect Parasolid coedges")?;
                out.coedges.push(Coedge {
                    id: id_coedge(ce_attr),
                    owner_loop: id_loop(*loop_attr),
                    edge: id_edge(edge_attr),
                    radial_next: partner.unwrap_or_else(|| id_coedge(ce_attr)),
                    sense,
                    use_curve: None,
                    pcurves,
                });
            }
        }
    }

    // Loops.
    for f in ctx.admit_iter(&faces, "emit Parasolid face loops")? {
        for (loop_attr, ring) in ctx.admit_iter(&f.loops, "emit Parasolid loops")? {
            if !ctx.contains_btree_set(&kept_loops, loop_attr, "test Parasolid kept_loops")? {
                continue;
            }
            let coedges = ctx.collect_vec(
                ring.iter().map(|a| id_coedge(*a)),
                "collect Parasolid loop coedges",
            )?;
            let off = ctx
                .get_btree_map(t.loops(), loop_attr, "lookup Parasolid loops")?
                .map_or(0, |r| r.offset);
            annotations.note(
                ctx,
                id_loop(*loop_attr).as_str(),
                &source_stream,
                u64_from_index(off),
                Some("00_0f"),
            )?;
            let Ok(ring) = cadmpeg_ir::topology::LoopRing::new(ctx, coedges, Vec::new())
                .map_err(cadmpeg_core::CodecError::from)?
            else {
                continue;
            };
            admit_brep_entity(ctx)?;
            ctx.reserve_vec(&mut out.loops, 1, "collect Parasolid loops")?;
            out.loops.push(Loop {
                id: id_loop(*loop_attr),
                face: id_face(f.bridge_attr),
                boundary: cadmpeg_ir::topology::LoopBoundary::Ring(ring),
            });
        }
    }
    let loop_set = kept_loops;

    // Surfaces + faces.
    let (bridge_group, bridge_shell) =
        scratch.with_storage(|| bind_bridges(ctx, t, &body_records, &faces))?;
    if !body_records.is_empty() {
        let walked = faces.len();
        ctx.retain_vec(
            &mut faces,
            |face| {
                ctx.contains_key_btree_map(
                    &bridge_group,
                    &face.bridge_attr,
                    "lookup Parasolid bridge_group",
                )
            },
            "select claimed Parasolid faces",
        )?;
        out.stats.unclaimed_faces += walked - faces.len();
    }
    let face_edges_by_surface_carrier = scratch.with_storage(|| {
        let mut face_edges_by_surface_carrier = BTreeMap::<u16, Vec<BTreeSet<u16>>>::new();
        for face in ctx.admit_iter(&faces, "index Parasolid surface face edges")? {
            let mut edges = BTreeSet::new();
            for (_, ring) in ctx.admit_iter(&face.loops, "index Parasolid face loop edges")? {
                for coedge in ctx.admit_iter(ring, "index Parasolid face edges")? {
                    if let Some(edge) = ctx
                        .get_btree_map(t.coedges(), coedge, "lookup Parasolid coedges")?
                        .map(|coedge| coedge.refs[6])
                    {
                        if edge != 0 {
                            ctx.insert_btree_set(&mut edges, edge, "track Parasolid face edges")?;
                        }
                    }
                }
            }
            ctx.push_btree_group(
                &mut face_edges_by_surface_carrier,
                face.surface_attr,
                edges,
                "index Parasolid surface face edges",
                "collect Parasolid surface face edges",
            )?;
        }
        Ok::<_, cadmpeg_core::CodecError>(face_edges_by_surface_carrier)
    })?;
    let emitted_face_surface_by_carrier = scratch.with_storage(|| {
        let mut emitted_face_surface_by_carrier = BTreeMap::<u16, u16>::new();
        for face in ctx.admit_iter(&faces, "select Parasolid face surface carriers")? {
            if ctx.any_by(
                &face.loops,
                |(loop_attr, _)| {
                    ctx.contains_btree_set(&loop_set, loop_attr, "test Parasolid loop_set")
                },
                "select Parasolid face surface carriers",
            )? {
                match ctx.entry_btree_map(
                    &mut emitted_face_surface_by_carrier,
                    face.surface_attr,
                    "index Parasolid face surface carriers",
                )? {
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert(face.bridge_attr);
                    }
                    std::collections::btree_map::Entry::Occupied(mut entry) => {
                        let bridge = entry.get_mut();
                        *bridge = (*bridge).min(face.bridge_attr);
                    }
                }
            }
        }
        Ok::<_, cadmpeg_core::CodecError>(emitted_face_surface_by_carrier)
    })?;
    // The first resolved color of each face owner, read once per emitted face.
    let owner_colors = scratch.with_storage(|| {
        let mut owner_colors = BTreeMap::new();
        for entry in ctx.admit_iter(&out.face_colors, "index Parasolid face owner colors")? {
            if let std::collections::btree_map::Entry::Vacant(slot) = ctx.entry_btree_map(
                &mut owner_colors,
                entry.value.face_attr,
                "index Parasolid face owner colors",
            )? {
                slot.insert(entry.value.color);
            }
        }
        Ok::<_, cadmpeg_core::CodecError>(owner_colors)
    })?;
    let mut hidden_supports = BTreeSet::new();
    let mut emitted_bridges = BTreeSet::new();
    for f in ctx.admit_iter(&faces, "emit Parasolid faces")? {
        let mut loops = Vec::new();
        for (loop_attr, _) in ctx.admit_iter(&f.loops, "select Parasolid face loops")? {
            if ctx.contains_btree_set(&loop_set, loop_attr, "test Parasolid loop_set")? {
                ctx.push_vec(
                    &mut loops,
                    id_loop(*loop_attr),
                    "collect Parasolid face loops",
                )?;
            }
        }
        if loops.is_empty() {
            continue;
        }
        // Support surface: a decoded surface carrier, else an opaque carrier.
        let surf_off = ctx
            .get_btree_map(t.bridges(), &f.bridge_attr, "lookup Parasolid bridges")?
            .map_or(0, |r| r.offset);
        let mut surface_orientation_reversed = false;
        match carriers.surface(ctx, f.surface_attr)? {
            Some(c) => {
                surface_orientation_reversed = c.orientation_reversed;
                annotations.note(
                    ctx,
                    id_surf(f.bridge_attr).as_str(),
                    &source_stream,
                    u64_from_index(c.offset),
                    Some("compact_surface"),
                )?;
                let geometry = c
                    .geometry
                    .try_clone_for_decode(ctx, "copy Parasolid surface geometry")?;
                if let SurfaceGeometry::Solved(solved) = &geometry {
                    annotate_surface_frame(
                        ctx,
                        &mut annotations,
                        id_surf(f.bridge_attr).as_str(),
                        solved,
                    )?;
                }
                admit_brep_entity(ctx)?;
                ctx.push_vec(
                    &mut (out.surfaces),
                    Surface {
                        id: id_surf(f.bridge_attr),
                        source_object: None,
                        geometry,
                    },
                    "collect SLDPRT decoded vector items",
                )?;
            }
            _ => {
                let resolved_offset = if let Some(offset) = carriers.offset(ctx, f.surface_attr)? {
                    if support_is_acyclic(ctx, offset.support, carriers, &mut BTreeSet::new())? {
                        ensure_surface_support(
                            &mut BrepSink {
                                ctx,
                                out: &mut out,
                                hidden_supports: &mut hidden_supports,
                            },
                            offset.support,
                            carriers,
                            &emitted_face_surface_by_carrier,
                            &mut annotations,
                            &source_stream,
                            &mut BTreeSet::new(),
                        )?
                        .map(|support| (offset, support))
                    } else {
                        None
                    }
                } else {
                    None
                };
                let blend_attrs = (|| -> Result<_, cadmpeg_core::CodecError> {
                    let Some(blend) = carriers.blend(ctx, f.surface_attr)? else {
                        return Ok(None);
                    };
                    let mut face_edges = BTreeSet::new();
                    for (_, ring) in
                        ctx.admit_iter(&f.loops, "select Parasolid blend face loop edges")?
                    {
                        for coedge in ctx.admit_iter(ring, "select Parasolid blend face edges")? {
                            if let Some(edge) = ctx
                                .get_btree_map(t.coedges(), coedge, "lookup Parasolid coedges")?
                                .map(|coedge| coedge.refs[6])
                            {
                                if edge != 0 {
                                    ctx.insert_btree_set(
                                        &mut face_edges,
                                        edge,
                                        "collect Parasolid blend face edges",
                                    )?;
                                }
                            }
                        }
                    }
                    let resolve_support =
                        |support| -> Result<Option<u16>, cadmpeg_core::CodecError> {
                            match support {
                                BlendSupportRef::Surface(attr) => Ok(Some(attr)),
                                BlendSupportRef::Pair(attr) => {
                                    let Some(pair) = carriers.blend_support_pair(ctx, attr)? else {
                                        return Ok(None);
                                    };
                                    if carriers.curve(ctx, pair.intersection)?.is_none() {
                                        return Ok(None);
                                    }
                                    let mut selected = None;
                                    for candidate in &pair.supports {
                                        let Some(edges) = ctx.get_btree_map(
                                            &face_edges_by_surface_carrier,
                                            candidate,
                                            "lookup Parasolid face_edges_by_surface_carrier",
                                        )?
                                        else {
                                            continue;
                                        };
                                        if ctx.any_by(
                                            edges,
                                            |edges| {
                                                Ok(!ctx.is_disjoint_btree_set(
                                                    &face_edges,
                                                    edges,
                                                    "compare Parasolid blend support face edges",
                                                )?)
                                            },
                                            "scan Parasolid blend support face edges",
                                        )? {
                                            if selected.is_some() {
                                                return Ok(None);
                                            }
                                            selected = Some(*candidate);
                                        }
                                    }
                                    Ok(selected)
                                }
                            }
                        };
                    let [Some(first_attr), Some(second_attr)] = [
                        resolve_support(blend.supports[0])?,
                        resolve_support(blend.supports[1])?,
                    ] else {
                        return Ok(None);
                    };
                    for attr in [first_attr, second_attr] {
                        if !support_is_acyclic(ctx, attr, carriers, &mut BTreeSet::new())? {
                            return Ok(None);
                        }
                    }
                    Ok(Some((blend, first_attr, second_attr)))
                })()?;
                let resolved_blend = if let Some((blend, first_attr, second_attr)) = blend_attrs {
                    if let Some(first) = ensure_surface_support(
                        &mut BrepSink {
                            ctx,
                            out: &mut out,
                            hidden_supports: &mut hidden_supports,
                        },
                        first_attr,
                        carriers,
                        &emitted_face_surface_by_carrier,
                        &mut annotations,
                        &source_stream,
                        &mut BTreeSet::new(),
                    )? {
                        ensure_surface_support(
                            &mut BrepSink {
                                ctx,
                                out: &mut out,
                                hidden_supports: &mut hidden_supports,
                            },
                            second_attr,
                            carriers,
                            &emitted_face_surface_by_carrier,
                            &mut annotations,
                            &source_stream,
                            &mut BTreeSet::new(),
                        )?
                        .map(|second| (blend, first, second))
                    } else {
                        None
                    }
                } else {
                    None
                };
                if let Some((offset, support)) = resolved_offset {
                    let construction = ProceduralSurfaceId::compose(
                        &cadmpeg_ir::identity_namespace!("sldprt", "brep", "offset-construction"),
                        f.bridge_attr,
                    );
                    emit_offset_surface(
                        &mut BrepSink {
                            ctx,
                            out: &mut out,
                            hidden_supports: &mut hidden_supports,
                        },
                        &mut annotations,
                        &source_stream,
                        id_surf(f.bridge_attr),
                        construction,
                        support,
                        offset,
                    )?;
                } else if let Some((blend, first, second)) = resolved_blend {
                    let spine = if let Some(indexed) = carriers.curve(ctx, blend.spine)? {
                        let carrier = indexed.carrier();
                        if scratch.with_storage(|| {
                            ctx.insert_btree_set(
                                &mut emitted_curves,
                                blend.spine,
                                "track Parasolid blend spines",
                            )
                        })? {
                            emit_curve(ctx, &mut out, carrier)?;
                            annotations.note(
                                ctx,
                                id_curve(blend.spine).as_str(),
                                &source_stream,
                                u64_from_index(carrier.offset),
                                Some("blend_spine"),
                            )?;
                        }
                        Some(id_curve(blend.spine))
                    } else {
                        None
                    };
                    let procedural_id = ProceduralSurfaceId::compose(
                        &cadmpeg_ir::identity_namespace!("sldprt", "brep", "blend-construction"),
                        f.bridge_attr,
                    );
                    let admitted_payload = BlendRadiusLaw::constant(blend.signed_radius)
                        .and_then(|radius| {
                            cadmpeg_ir::geometry::surface_payloads::BlendSurfacePayload::try_new(
                                [
                                    Some(BlendSupport {
                                        surface: first,
                                        reversed: blend.reversed[0],
                                    }),
                                    Some(BlendSupport {
                                        surface: second,
                                        reversed: blend.reversed[1],
                                    }),
                                ],
                                spine,
                                radius,
                                BlendCrossSection::Circular,
                                cadmpeg_ir::geometry::CacheContract::from_form(None),
                            )
                        })
                        .map_err(cadmpeg_core::CodecError::malformed)?;
                    admit_brep_entity(ctx)?;
                    ctx.reserve_vec(
                        &mut out.procedural_surfaces,
                        1,
                        "collect Parasolid blend constructions",
                    )?;
                    out.procedural_surfaces.push(ProceduralSurface::new(
                        procedural_id.try_clone_for_decode(ctx, "SLDPRT decoded identity copy")?,
                        ProceduralSurfaceDefinition::Blend(admitted_payload),
                        None,
                    ));
                    let geometry = SurfaceGeometry::Procedural {
                        construction: procedural_id,
                        cache: None,
                    };
                    annotations.note(
                        ctx,
                        id_surf(f.bridge_attr).as_str(),
                        &source_stream,
                        u64_from_index(blend.offset),
                        Some("00_38"),
                    )?;
                    admit_brep_entity(ctx)?;
                    ctx.reserve_vec(&mut out.surfaces, 1, "collect Parasolid blend surfaces")?;
                    out.surfaces.push(Surface {
                        id: id_surf(f.bridge_attr),
                        source_object: None,
                        geometry,
                    });
                } else if let Some(SweepSurfaceResolution {
                    geometry,
                    offset,
                    tag,
                    exactness,
                }) = {
                    let mut sweep_refusal = crate::lane_refusal::LaneRefusals::new();
                    let resolved = resolve_sweep_surface(ctx, carriers, t, f, &mut sweep_refusal)?;
                    for record in ctx
                        .admit_iter(sweep_refusal.take_records(), "collect swept surface losses")?
                    {
                        let note = crate::loss::spline_lane_refusal(
                            ctx,
                            format_args!(
                                "swept surface for face attr {}: {record}",
                                f.surface_attr
                            ),
                        )?;
                        ctx.reserve_vec(&mut out.losses, 1, "collect swept surface losses")?;
                        out.losses.push(note);
                    }
                    resolved
                } {
                    annotations.note(
                        ctx,
                        id_surf(f.bridge_attr).as_str(),
                        &source_stream,
                        u64_from_index(offset),
                        Some(tag),
                    )?;
                    if let Some(exactness) = exactness {
                        annotations.exactness(ctx, id_surf(f.bridge_attr).as_str(), exactness)?;
                    }
                    admit_brep_entity(ctx)?;
                    ctx.reserve_vec(&mut out.surfaces, 1, "collect Parasolid swept surfaces")?;
                    out.surfaces.push(Surface {
                        id: id_surf(f.bridge_attr),
                        source_object: None,
                        geometry: SurfaceGeometry::Solved(geometry),
                    });
                } else {
                    out.stats.unknown_surface_faces += 1;
                    annotations.note(
                        ctx,
                        id_surf(f.bridge_attr).as_str(),
                        &source_stream,
                        u64_from_index(surf_off),
                        Some("unknown_surface"),
                    )?;
                    annotations.exactness(
                        ctx,
                        id_surf(f.bridge_attr).as_str(),
                        Exactness::Unknown,
                    )?;
                    admit_brep_entity(ctx)?;
                    ctx.reserve_vec(&mut out.surfaces, 1, "collect Parasolid opaque surfaces")?;
                    out.surfaces.push(Surface {
                        id: id_surf(f.bridge_attr),
                        source_object: None,
                        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                            record: None,
                        }),
                    });
                }
            }
        }
        annotations.note(
            ctx,
            id_face(f.bridge_attr).as_str(),
            &source_stream,
            u64_from_index(surf_off),
            Some("00_0e"),
        )?;
        admit_brep_entity(ctx)?;
        ctx.reserve_vec(&mut out.faces, 1, "collect Parasolid faces")?;
        let shell_attr = match ctx
            .get_btree_map(
                &bridge_shell,
                &f.bridge_attr,
                "lookup Parasolid bridge_shell",
            )?
            .copied()
        {
            Some(shell_attr) => shell_attr,
            None => match ctx
                .get_btree_map(
                    &bridge_group,
                    &f.bridge_attr,
                    "lookup Parasolid bridge_group",
                )?
                .copied()
            {
                Some(group) => u16::try_from(group).map_err(|_| {
                    ctx.refuse_codec_limit("index Parasolid bridge groups", u64::MAX - 1, u64::MAX)
                })?,
                None => 0,
            },
        };
        out.faces.push(Face {
            id: id_face(f.bridge_attr),
            shell: ShellId::compose(&shell_namespace(), shell_attr),
            surface: id_surf(f.bridge_attr),
            sense: surface_sense(f.sense, surface_orientation_reversed),
            loops: cadmpeg_ir::topology::FaceLoops::unspecified(loops),
            name: None,
            color: match ctx
                .get_btree_map(t.bridges(), &f.bridge_attr, "lookup Parasolid bridges")?
                .and_then(|bridge| bridge.owner)
            {
                Some(owner) => ctx
                    .get_btree_map(&owner_colors, &owner, "lookup Parasolid owner colors")?
                    .copied(),
                None => None,
            },
            tolerance: None,
        });
        scratch.with_storage(|| {
            ctx.insert_btree_set(
                &mut emitted_bridges,
                f.bridge_attr,
                "track emitted Parasolid faces",
            )
        })?;
    }
    // A face color targets the first walked face of its owner, when that face
    // was emitted.
    let first_face_by_owner = scratch.with_storage(|| {
        let mut first_face_by_owner = BTreeMap::new();
        for face in ctx.admit_iter(&faces, "index Parasolid face owners")? {
            let Some(owner) = ctx
                .get_btree_map(t.bridges(), &face.bridge_attr, "lookup Parasolid bridges")?
                .and_then(|bridge| bridge.owner)
            else {
                continue;
            };
            if let std::collections::btree_map::Entry::Vacant(slot) = ctx.entry_btree_map(
                &mut first_face_by_owner,
                owner,
                "index Parasolid face owners",
            )? {
                slot.insert(face.bridge_attr);
            }
        }
        Ok::<_, cadmpeg_core::CodecError>(first_face_by_owner)
    })?;
    for appearance in ctx.admit_iter(&mut out.face_colors, "bind Parasolid face colors")? {
        appearance.value.target = match ctx.get_btree_map(
            &first_face_by_owner,
            &appearance.value.face_attr,
            "lookup Parasolid first face by owner",
        )? {
            Some(bridge)
                if ctx.contains_btree_set(
                    &emitted_bridges,
                    bridge,
                    "test emitted Parasolid faces",
                )? =>
            {
                Some(id_face(*bridge).into_string())
            }
            _ => None,
        };
    }
    let mut bound_faces = BTreeSet::new();
    for atom in ctx.admit_iter(entity_facts.face_atoms, "bind Parasolid face atoms")? {
        let Some(identity) = atom.identity.as_ref() else {
            continue;
        };
        if !ctx.contains_btree_set(
            &emitted_bridges,
            &atom.face_attr,
            "test Parasolid emitted_bridges",
        )? {
            continue;
        }
        if scratch.with_storage(|| {
            ctx.insert_btree_set(
                &mut bound_faces,
                atom.face_attr,
                "track bound Parasolid faces",
            )
        })? {
            ctx.push_vec(
                &mut out.face_atoms,
                attrib::FaceAtom {
                    face: id_face(atom.face_attr),
                    identity: identity.copy_to_retained(ctx)?,
                },
                "collect Parasolid face atoms",
            )?;
        }
    }
    solve_face_orientation(ctx, &mut out)?;
    synthesize_cylinder_seams(ctx, &mut out, &mut annotations, &source_stream)?;
    synthesize_sphere_seams(ctx, &mut out, &mut annotations, &source_stream)?;
    derive_pcurves(ctx, &mut out, &mut annotations, &source_stream)?;
    prune_rejected_topology(ctx, &mut out)?;

    if out.faces.is_empty() {
        return Ok(Brep {
            losses: out.losses,
            stats: out.stats,
            ..Brep::default()
        });
    }
    let synthetic_grouping = body_records.is_empty();
    out.stats.synthetic_body_grouping = synthetic_grouping;
    let mut shell_faces = ShellFaceIndex::new(ctx, &out.faces, &out.loops, &out.coedges)?;

    for (group, body_record) in ctx
        .admit_iter(&body_records, "scan Parasolid body groups")?
        .map(Some)
        .chain(std::iter::once(None).take(usize::from(synthetic_grouping)))
        .enumerate()
    {
        let body_id = BodyId::compose(
            &body_namespace(),
            body_record.map_or(0_u16, |record| record.attr),
        );
        let mut annotate_group =
            |id: &str, source: Option<(usize, &str)>| -> Result<(), cadmpeg_core::CodecError> {
                let (offset, tag, exactness) = source.map_or(
                    (0, "synthetic_grouping", Exactness::Derived),
                    |(offset, tag)| (offset, tag, Exactness::ByteExact),
                );
                annotations.note(ctx, id, &source_stream, u64_from_index(offset), Some(tag))?;
                annotations.exactness(ctx, id, exactness)?;
                Ok(())
            };
        annotate_group(
            body_id.as_str(),
            body_record.map(|record| (record.offset, "00_51_body")),
        )?;
        let native_regions = body_record.map_or(&[][..], |record| record.regions.as_slice());
        let mut body_regions = Vec::new();
        if native_regions.is_empty() {
            let region_id = RegionId::compose(&region_namespace(), group);
            let native_shell_id = ShellId::compose(&shell_namespace(), group);
            annotate_group(region_id.as_str(), None)?;
            let region_shells = emit_shell_components(
                ctx,
                &mut shell_faces,
                &native_shell_id,
                &region_id,
                None,
                &mut annotate_group,
                &mut out.shells,
            )?;
            admit_brep_entity(ctx)?;
            ctx.reserve_vec(&mut out.regions, 1, "collect synthetic Parasolid regions")?;
            out.regions.push(Region {
                id: region_id.try_clone_for_decode(ctx, "SLDPRT decoded identity copy")?,
                body: body_id.try_clone_for_decode(ctx, "SLDPRT decoded identity copy")?,
                shells: region_shells,
            });
            ctx.reserve_vec(&mut body_regions, 1, "collect synthetic body regions")?;
            body_regions.push(region_id);
        } else {
            for region in ctx.admit_iter(native_regions, "scan SLDPRT native_regions values")? {
                let region_id = RegionId::compose(&region_namespace(), region.attr);
                annotate_group(region_id.as_str(), Some((region.offset, "00_51_region")))?;
                let mut region_shells = Vec::new();
                for shell in ctx.admit_iter(&region.shells, "scan SLDPRT decode_graph values")? {
                    let native_shell_id = ShellId::compose(&shell_namespace(), shell.attr);
                    let mut components = emit_shell_components(
                        ctx,
                        &mut shell_faces,
                        &native_shell_id,
                        &region_id,
                        Some((shell.offset, "00_51_shell")),
                        &mut annotate_group,
                        &mut out.shells,
                    )?;
                    ctx.append_vec(
                        &mut region_shells,
                        &mut components,
                        "collect native region shells",
                    )?;
                }
                admit_brep_entity(ctx)?;
                ctx.reserve_vec(&mut out.regions, 1, "collect native Parasolid regions")?;
                out.regions.push(Region {
                    id: region_id.try_clone_for_decode(ctx, "SLDPRT decoded identity copy")?,
                    body: body_id.try_clone_for_decode(ctx, "SLDPRT decoded identity copy")?,
                    shells: region_shells,
                });
                ctx.reserve_vec(&mut body_regions, 1, "collect native body regions")?;
                body_regions.push(region_id);
            }
        }
        admit_brep_entity(ctx)?;
        ctx.reserve_vec(&mut out.bodies, 1, "collect Parasolid bodies")?;
        out.bodies.push(Body {
            id: body_id,
            kind: body_record.map_or(BodyKind::Solid, |record| record.kind),
            regions: body_regions,
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
    }
    shell_faces.into_assignments().apply(&mut out.faces)?;

    // Body and curve identities here are composed from a constant namespace
    // and one u16 attr, so reading the attr back is fixed-size work.
    let mut body_ids_by_attr = BTreeMap::<u16, Option<&str>>::new();
    for body in ctx.admit_iter(&out.bodies, "index Parasolid body attributes")? {
        let Some(attr) = body
            .id
            .as_str()
            .strip_prefix("sldprt:brep:body#")
            .and_then(|value| value.parse::<u16>().ok())
        else {
            continue;
        };
        match scratch.with_storage(|| {
            ctx.entry_btree_map(
                &mut body_ids_by_attr,
                attr,
                "index Parasolid body attributes",
            )
        })? {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(Some(body.id.as_str()));
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                *entry.get_mut() = None;
            }
        }
    }
    for modifier in ctx.admit_iter(&mut out.body_modifiers, "bind Parasolid body modifiers")? {
        modifier.target = match ctx.get_btree_map(
            &body_ids_by_attr,
            &modifier.body_attr,
            "lookup Parasolid body_ids_by_attr",
        )? {
            Some(Some(id)) => Some(ctx.copy_retained_text(id, "copy Parasolid modifier body ID")?),
            _ => None,
        };
    }

    for curve in ctx.admit_iter(&out.curves, "annotate Parasolid curves")? {
        let Some(attr) = curve
            .id
            .as_str()
            .strip_prefix("sldprt:brep:curve#")
            .and_then(|value| value.parse::<u16>().ok())
        else {
            continue;
        };
        if let Some(indexed) = carriers.curve(ctx, attr)? {
            let carrier = indexed.carrier();
            annotations.note(
                ctx,
                curve.id.as_str(),
                &source_stream,
                u64_from_index(carrier.offset),
                Some("compact_curve"),
            )?;
            if matches!(
                curve.geometry,
                CurveGeometry::Solved(SolvedCurveGeometry::Unknown { .. })
            ) {
                annotations.exactness(ctx, curve.id.as_str(), Exactness::Unknown)?;
            }
        }
    }
    ctx.stable_sort_by(
        &mut out.bodies,
        |value| &value.id,
        Ord::cmp,
        "sort Parasolid graph ids",
    )?;
    ctx.stable_sort_by(
        &mut out.regions,
        |value| &value.id,
        Ord::cmp,
        "sort Parasolid graph ids",
    )?;
    ctx.stable_sort_by(
        &mut out.shells,
        |value| &value.id,
        Ord::cmp,
        "sort Parasolid graph ids",
    )?;
    ctx.stable_sort_by(
        &mut out.faces,
        |value| &value.id,
        Ord::cmp,
        "sort Parasolid graph ids",
    )?;
    ctx.stable_sort_by(
        &mut out.loops,
        |value| &value.id,
        Ord::cmp,
        "sort Parasolid graph ids",
    )?;
    ctx.stable_sort_by(
        &mut out.coedges,
        |value| &value.id,
        Ord::cmp,
        "sort Parasolid graph ids",
    )?;
    ctx.stable_sort_by(
        &mut out.edges,
        |value| &value.id,
        Ord::cmp,
        "sort Parasolid graph ids",
    )?;
    ctx.stable_sort_by(
        &mut out.vertices,
        |value| &value.id,
        Ord::cmp,
        "sort Parasolid graph ids",
    )?;
    ctx.stable_sort_by(
        &mut out.points,
        |value| &value.id,
        Ord::cmp,
        "sort Parasolid graph ids",
    )?;
    ctx.stable_sort_by(
        &mut out.surfaces,
        |value| &value.id,
        Ord::cmp,
        "sort Parasolid graph ids",
    )?;
    ctx.stable_sort_by(
        &mut out.procedural_surfaces,
        |value| &value.id,
        Ord::cmp,
        "sort Parasolid graph ids",
    )?;
    ctx.stable_sort_by(
        &mut out.curves,
        |value| &value.id,
        Ord::cmp,
        "sort Parasolid graph ids",
    )?;
    ctx.stable_sort_by(
        &mut out.pcurves,
        |value| &value.id,
        Ord::cmp,
        "sort Parasolid graph ids",
    )?;
    out.annotations = annotations.build();
    let retained_ids = scratch.with_storage(|| {
        ctx.collect_hash_set(
            out.bodies
                .iter()
                .map(|entity| entity.id.as_str())
                .chain(out.regions.iter().map(|entity| entity.id.as_str()))
                .chain(out.shells.iter().map(|entity| entity.id.as_str()))
                .chain(out.faces.iter().map(|entity| entity.id.as_str()))
                .chain(out.loops.iter().map(|entity| entity.id.as_str()))
                .chain(out.coedges.iter().map(|entity| entity.id.as_str()))
                .chain(out.edges.iter().map(|entity| entity.id.as_str()))
                .chain(out.vertices.iter().map(|entity| entity.id.as_str()))
                .chain(out.points.iter().map(|entity| entity.id.as_str()))
                .chain(out.surfaces.iter().map(|entity| entity.id.as_str()))
                .chain(
                    out.procedural_surfaces
                        .iter()
                        .map(|entity| entity.id.as_str()),
                )
                .chain(out.curves.iter().map(|entity| entity.id.as_str()))
                .chain(out.pcurves.iter().map(|entity| entity.id.as_str())),
            "index retained Parasolid entities",
        )
    })?;
    let mut keep =
        |id: &str| ctx.contains_hash_set(&(retained_ids), id, "test SLDPRT hashed identity");
    out.annotations.retain_provenance(ctx, &mut keep)?;
    let mut annotations = AnnotationBuilder::resume(std::mem::take(&mut out.annotations));
    annotations.retain_exactness(ctx, keep)?;
    out.annotations = annotations.build();
    Ok(out)
}

pub(super) fn prune_rejected_topology(
    ctx: &DecodeContext<'_>,
    out: &mut Brep,
) -> Result<(), cadmpeg_core::CodecError> {
    const TEST: &str = "test SLDPRT hashed identity";
    let mut scratch = ctx.reserve_scoped(0, "SLDPRT retained topology identities")?;

    let kept_loops = scratch.with_storage(|| {
        let mut kept = HashSet::new();
        for face in ctx.admit_iter(&out.faces, "track retained Parasolid loops")? {
            ctx.extend_hash_set(
                &mut kept,
                face.loops.iter().map(LoopId::as_str),
                "track retained Parasolid loops",
            )?;
        }
        Ok::<_, cadmpeg_core::CodecError>(kept)
    })?;
    ctx.retain_vec(
        &mut out.loops,
        |loop_| ctx.contains_hash_set(&kept_loops, loop_.id.as_str(), TEST),
        "prune Parasolid loops",
    )?;

    let kept_coedges = scratch.with_storage(|| {
        let mut kept = HashSet::new();
        for loop_ in ctx.admit_iter(&out.loops, "track retained Parasolid coedges")? {
            ctx.extend_hash_set(
                &mut kept,
                loop_.coedges().iter().map(CoedgeId::as_str),
                "track retained Parasolid coedges",
            )?;
        }
        Ok::<_, cadmpeg_core::CodecError>(kept)
    })?;
    ctx.retain_vec(
        &mut out.coedges,
        |coedge| ctx.contains_hash_set(&kept_coedges, coedge.id.as_str(), TEST),
        "prune Parasolid coedges",
    )?;
    for coedge in ctx.admit_iter(&mut out.coedges, "repair Parasolid radial coedges")? {
        if !ctx.contains_hash_set(&kept_coedges, coedge.radial_next.as_str(), TEST)? {
            coedge.radial_next = coedge
                .id
                .try_clone_for_decode(ctx, "SLDPRT decoded identity copy")?;
        }
    }

    let kept_pcurves = scratch.with_storage(|| {
        let mut kept = HashSet::new();
        for coedge in ctx.admit_iter(&out.coedges, "track retained Parasolid pcurves")? {
            ctx.extend_hash_set(
                &mut kept,
                coedge.pcurves.iter().map(|use_| use_.pcurve.as_str()),
                "track retained Parasolid pcurves",
            )?;
        }
        Ok::<_, cadmpeg_core::CodecError>(kept)
    })?;
    ctx.retain_vec(
        &mut out.pcurves,
        |pcurve| ctx.contains_hash_set(&kept_pcurves, pcurve.id.as_str(), TEST),
        "prune Parasolid pcurves",
    )?;

    let kept_edges = scratch.with_storage(|| {
        ctx.collect_hash_set(
            out.coedges.iter().map(|coedge| coedge.edge.as_str()),
            "track retained Parasolid edges",
        )
    })?;
    ctx.retain_vec(
        &mut out.edges,
        |edge| ctx.contains_hash_set(&kept_edges, edge.id.as_str(), TEST),
        "prune Parasolid edges",
    )?;

    let kept_vertices = scratch.with_storage(|| {
        ctx.collect_hash_set(
            out.edges
                .iter()
                .flat_map(|edge| [edge.start.as_str(), edge.end.as_str()]),
            "track retained Parasolid vertices",
        )
    })?;
    ctx.retain_vec(
        &mut out.vertices,
        |vertex| ctx.contains_hash_set(&kept_vertices, vertex.id.as_str(), TEST),
        "prune Parasolid vertices",
    )?;

    let kept_points = scratch.with_storage(|| {
        ctx.collect_hash_set(
            out.vertices.iter().map(|vertex| vertex.point.as_str()),
            "track retained Parasolid points",
        )
    })?;
    ctx.retain_vec(
        &mut out.points,
        |point| ctx.contains_hash_set(&kept_points, point.id.as_str(), TEST),
        "prune Parasolid points",
    )?;

    let kept_curves = scratch.with_storage(|| {
        let mut kept = HashSet::new();
        for edge in ctx.admit_iter(&out.edges, "track retained Parasolid curves")? {
            if let Some(curve) = edge.curve() {
                ctx.insert_hash_set(&mut kept, curve.as_str(), "track retained Parasolid curves")?;
            }
        }
        for surface in
            ctx.admit_iter(&out.procedural_surfaces, "track retained Parasolid curves")?
        {
            if let ProceduralSurfaceDefinition::Blend(definition_payload) = surface.definition() {
                if let Some(spine) = definition_payload.spine() {
                    ctx.insert_hash_set(
                        &mut kept,
                        spine.as_str(),
                        "track retained Parasolid curves",
                    )?;
                }
            }
        }
        Ok::<_, cadmpeg_core::CodecError>(kept)
    })?;
    ctx.retain_vec(
        &mut out.curves,
        |curve| ctx.contains_hash_set(&kept_curves, curve.id.as_str(), TEST),
        "prune Parasolid curves",
    )?;

    let unknown_curves = scratch.with_storage(|| {
        let mut unknown = HashSet::new();
        for curve in ctx.admit_iter(&out.curves, "track unknown Parasolid curves")? {
            if matches!(
                curve.geometry,
                CurveGeometry::Solved(SolvedCurveGeometry::Unknown { .. })
            ) {
                ctx.insert_hash_set(
                    &mut unknown,
                    curve.id.as_str(),
                    "track unknown Parasolid curves",
                )?;
            }
        }
        Ok::<_, cadmpeg_core::CodecError>(unknown)
    })?;
    let mut unknown_curve_edges = 0_usize;
    for edge in ctx.admit_iter(&out.edges, "count unknown Parasolid curve edges")? {
        if let Some(curve) = edge.curve() {
            if ctx.contains_hash_set(&unknown_curves, curve.as_str(), TEST)? {
                unknown_curve_edges += 1;
            }
        }
    }
    out.stats.unknown_curve_edges = unknown_curve_edges;
    Ok(())
}

pub(super) fn annotate_surface_frame(
    ctx: &DecodeContext<'_>,
    annotations: &mut AnnotationBuilder,
    id: &str,
    mut geometry: &SolvedSurfaceGeometry,
) -> Result<(), cadmpeg_core::CodecError> {
    loop {
        ctx.charge_work(1, "scan Parasolid surface frame annotations")?;
        match geometry {
            SolvedSurfaceGeometry::Plane(_) => {
                annotations.field_exactness(ctx, id, "geometry.u_axis", Exactness::Derived)?;
                break;
            }
            SolvedSurfaceGeometry::Cylinder(_)
            | SolvedSurfaceGeometry::Cone(_)
            | SolvedSurfaceGeometry::Torus(_) => {
                annotations.field_exactness(
                    ctx,
                    id,
                    "geometry.ref_direction",
                    Exactness::Derived,
                )?;
                break;
            }
            SolvedSurfaceGeometry::Sphere(_) => {
                annotations.field_exactness(ctx, id, "geometry.axis", Exactness::Derived)?;
                annotations.field_exactness(
                    ctx,
                    id,
                    "geometry.ref_direction",
                    Exactness::Derived,
                )?;
                break;
            }
            SolvedSurfaceGeometry::Transformed(placed) => geometry = placed.basis(),
            SolvedSurfaceGeometry::Nurbs(_)
            | SolvedSurfaceGeometry::Polygonal(_)
            | SolvedSurfaceGeometry::Unknown { .. } => break,
        }
    }
    Ok(())
}

fn emit_curve(
    ctx: &DecodeContext<'_>,
    out: &mut Brep,
    carrier: &CurveCarrier,
) -> Result<(), cadmpeg_core::CodecError> {
    admit_brep_entity(ctx)?;
    ctx.reserve_vec(&mut out.curves, 1, "collect Parasolid curves")?;
    out.curves.push(Curve {
        id: id_curve(carrier.attr),
        source_object: None,
        geometry: carrier
            .geometry
            .try_clone_for_decode(ctx, "copy Parasolid curve geometry")?,
    });
    Ok(())
}

#[cfg(test)]
mod tests;
