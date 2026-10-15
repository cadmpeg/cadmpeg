// SPDX-License-Identifier: Apache-2.0
//! Walk the reachable topology graph, collect wire and shell chains, and
//! classify edge curve senses.

use super::records::{MeshSurfaceSentinel, WireMembers, WireSide, WireTopology};
use crate::ids::{brep_id, IdFormat};
use crate::nurbs;
use crate::sab::{Record, Token};
use cadmpeg_ir::geometry::{
    pcurve::PcurveGeometry, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    CoedgeId, EdgeId, FaceId, LoopId, ProceduralCurveId, ProceduralSurfaceId, RegionId, ShellId,
    SurfaceId, VertexId,
};
use cadmpeg_ir::topology::Sense;
use std::collections::{HashMap, HashSet};

use super::attributes::unknown_record_id;
use super::geometry::{
    analytic_procedural_surface, coedge_pcurve_ref, decode_curve, decode_surface,
    is_analytic_curve, is_analytic_surface, is_coedge_record, is_edge_record, is_vertex_record,
    pcurve_ranges_on_domain, procedural_surface_definition_is_exact_carrier, record_reversed,
    reverse_procedural_curve_definition, sense_at, vertex_point_ref,
};
use super::{count_kind, id, AsmBrep, Carriers, DecodePurpose, Reachable, WireShellTopology};
/// Pass 1: classify carriers and decode analytic geometry. Returns the seeded
/// carrier maps and the set of carriers whose native normal is inward.
pub(super) fn decode_analytic_carriers(
    records: &[Record],
) -> Result<(Carriers, HashSet<i64>), cadmpeg_core::CodecError> {
    let mut surface_geo: HashMap<i64, SurfaceGeometry> = HashMap::new();
    let mut inward_normal_surfaces = HashSet::new();
    let mut curve_geo: HashMap<i64, CurveGeometry> = HashMap::new();
    for r in records {
        if is_analytic_surface(r.head()) {
            if let Some((geometry, inward)) = decode_surface(r) {
                if inward {
                    inward_normal_surfaces.insert(i64::try_from(r.index).map_err(|_| {
                        cadmpeg_core::CodecError::malformed(
                            "ASM record index exceeds the i64 range",
                        )
                    })?);
                }
                surface_geo.insert(
                    i64::try_from(r.index).map_err(|_| {
                        cadmpeg_core::CodecError::malformed(
                            "ASM record index exceeds the i64 range",
                        )
                    })?,
                    SurfaceGeometry::Solved(geometry),
                );
            }
        } else if is_analytic_curve(r.head()) {
            if let Some(g) = decode_curve(r) {
                curve_geo.insert(
                    i64::try_from(r.index).map_err(|_| {
                        cadmpeg_core::CodecError::malformed(
                            "ASM record index exceeds the i64 range",
                        )
                    })?,
                    g,
                );
            }
        }
    }
    let carriers = Carriers {
        surface_geo,
        curve_geo,
        ..Carriers::default()
    };
    Ok((carriers, inward_normal_surfaces))
}

/// Inputs shared by the topology passes of one ASM decode.
#[derive(Clone, Copy)]
pub(super) struct TopologyContext<'a, 'ctx, 'record> {
    pub(super) ctx: &'a cadmpeg_core::decode::DecodeContext<'ctx>,
    pub(super) by_index: &'a HashMap<i64, &'record Record>,
    pub(super) token_table: &'a nurbs::toks::SubtypeTable,
    pub(super) purpose: DecodePurpose,
    pub(super) format: IdFormat,
}

/// Pass 2 (faces): keep every face whose surface reference resolves, decoding
/// or classifying its carrier and recording surface reachability.
pub(super) fn keep_faces_and_carriers(
    inputs: TopologyContext<'_, '_, '_>,
    out: &mut AsmBrep,
    records: &[Record],
    carriers: &mut Carriers,
    reach: &mut Reachable,
) -> Result<(), cadmpeg_core::CodecError> {
    let TopologyContext {
        ctx,
        by_index,
        token_table,
        purpose,
        format,
    } = inputs;
    let Carriers {
        surface_geo,
        procedural_surface_defs,
        ..
    } = &mut *carriers;
    let Reachable {
        faces: kept_faces,
        surfaces: kept_surfaces,
        unknown_surface_records,
        cached_unknown_procedural_surfaces,
        undecoded_carriers,
        ..
    } = &mut *reach;
    for r in records {
        if r.head() != "face" {
            continue;
        }
        // A standalone face cannot supply the shell required by the neutral
        // topology. Do not reach its children or carriers through an owner
        // that the emit pass must omit.
        if r.ref_at(5).is_none() {
            continue;
        }
        let Some(surf_ref) = r.ref_at(7) else {
            count_kind(&mut out.stats.missing_face_surface_kinds, "null-reference");
            continue;
        };
        let Some(surf_rec) = by_index.get(&surf_ref) else {
            // Dangling surface reference: a face without a resolvable surface
            // cannot be emitted (the IR requires one), so it is dropped.

            count_kind(
                &mut out.stats.missing_face_surface_kinds,
                "dangling-reference",
            );
            continue;
        };
        kept_faces.insert(i64::try_from(r.index).map_err(|_| {
            cadmpeg_core::CodecError::malformed("ASM record index exceeds the i64 range")
        })?);
        if purpose == DecodePurpose::History {
            let native_kind = (surf_rec.head() == "spline")
                .then(|| nurbs::toks::owned_construction_subtype(&surf_rec.tokens))
                .flatten();
            if native_kind
                .as_deref()
                .is_some_and(|kind| kind.contains("blend"))
            {
                if let Some(procedural) = nurbs::proc_surface::procedural_surface_resolving_refs(
                    ctx,
                    &surf_rec.tokens,
                    token_table,
                ) {
                    procedural_surface_defs.insert(surf_ref, procedural?);
                }
            }
            surface_geo
                .entry(surf_ref)
                .or_insert(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                    record: None,
                }));
            kept_surfaces.insert(surf_ref);
            continue;
        }
        if let Some(procedural) = nurbs::proc_surface::procedural_surface_resolving_refs(
            ctx,
            &surf_rec.tokens,
            token_table,
        ) {
            procedural_surface_defs.insert(surf_ref, procedural?);
        }
        if let Some(procedural) = procedural_surface_defs.get(&surf_ref) {
            if let Some(geometry) = analytic_procedural_surface(procedural.definition()) {
                surface_geo.insert(surf_ref, geometry);
            }
        }
        let exact_cacheless_construction =
            procedural_surface_defs
                .get(&surf_ref)
                .is_some_and(|procedural| {
                    procedural.cache_fit_tolerance().is_none()
                        && procedural_surface_definition_is_exact_carrier(procedural.definition())
                });
        // A non-analytic surface may still carry a decodable B-spline face
        // cache. Exact cacheless constructions own their nested surface blocks
        // as supports, not as evaluated face caches.
        if !exact_cacheless_construction && !surface_geo.contains_key(&surf_ref) {
            if let Some(ns) =
                nurbs::core::surface_cache_resolving_refs(ctx, &surf_rec.tokens, token_table)
            {
                surface_geo.insert(
                    surf_ref,
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(ns?)),
                );
                if surf_rec.head() == "spline" && !procedural_surface_defs.contains_key(&surf_ref) {
                    cached_unknown_procedural_surfaces.insert(surf_ref);
                    count_kind(
                        &mut out.stats.other_record_kinds,
                        "cached-procedural-surface-untyped",
                    );
                }
                out.stats.nurbs_surfaces += 1;
            }
        }
        if !surface_geo.contains_key(&surf_ref) && procedural_surface_defs.contains_key(&surf_ref) {
            let construction_is_exact_carrier =
                procedural_surface_defs
                    .get(&surf_ref)
                    .is_some_and(|procedural| {
                        procedural_surface_definition_is_exact_carrier(procedural.definition())
                    });
            surface_geo.insert(
                surf_ref,
                if construction_is_exact_carrier {
                    SurfaceGeometry::Procedural {
                        construction: brep_id!(
                            format,
                            ProceduralSurfaceId,
                            "procedural_surface",
                            surf_ref
                        ),
                        cache: None,
                    }
                } else {
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                        record: Some(unknown_record_id(surf_rec, format)?),
                    })
                },
            );
            if !construction_is_exact_carrier {
                undecoded_carriers.insert(surf_ref);
            }
        }
        if surface_geo.contains_key(&surf_ref) {
            kept_surfaces.insert(surf_ref);
        } else {
            unknown_surface_records.insert(surf_ref);
            undecoded_carriers.insert(surf_ref);
            if surf_rec.head() == "mesh_surface" && surf_rec.chunks().next().is_none() {
                if !out.mesh_surface_sentinels.iter().any(|sentinel| {
                    usize::try_from(sentinel.record_index).ok() == Some(surf_rec.index)
                }) {
                    out.mesh_surface_sentinels.push(MeshSurfaceSentinel {
                        source_namespace:
                            crate::brep::records::identity::NativeRecordNamespace::new(format),
                        surface: SurfaceId::from(id(format, surf_ref)),
                        record_index: u32::try_from(surf_rec.index).map_err(|_| {
                            cadmpeg_core::CodecError::malformed(
                                "ASM native record index exceeds the u32 range",
                            )
                        })?,
                    });
                }
                out.stats.mesh_surface_faces += 1;
            } else {
                let native_kind = if surf_rec.head() == "spline" {
                    nurbs::toks::owned_construction_subtype(&surf_rec.tokens)
                } else {
                    None
                };
                count_kind(
                    &mut out.stats.unknown_surface_kinds,
                    native_kind.as_deref().unwrap_or_else(|| surf_rec.head()),
                );
            }
        }
    }
    Ok(())
}

/// Pass 2 (topology): walk each kept face's loops and coedge rings, pulling in
/// the supporting edge/vertex/point graph and decoding curve and pcurve carriers.
pub(super) fn walk_reachable_topology(
    inputs: TopologyContext<'_, '_, '_>,
    out: &mut AsmBrep,
    carriers: &mut Carriers,
    reach: &mut Reachable,
) -> Result<(), cadmpeg_core::CodecError> {
    let TopologyContext {
        ctx,
        by_index,
        token_table,
        purpose,
        format,
    } = inputs;
    let Carriers {
        curve_geo,
        procedural_curve_defs,
        pcurve_geo,
        pcurve_parameter_ranges,
        ..
    } = &mut *carriers;
    let Reachable {
        faces: kept_faces,
        loops: kept_loops,
        coedges: kept_coedges,
        edges: kept_edges,
        vertices: kept_vertices,
        points: kept_points,
        curves: kept_curves,
        pcurves: kept_pcurves,
        undecoded_carriers,
        ..
    } = &mut *reach;
    // Walk each kept face's loops and coedge rings, collecting supporting graph.
    let face_indices = kept_faces.iter().copied().collect::<Vec<_>>();
    for face_idx in face_indices {
        let Some(face) = by_index.get(&face_idx) else {
            continue;
        };
        let mut loop_ref = face.ref_at(4);
        let mut loop_guard = HashSet::new();
        while let Some(li) = loop_ref {
            if !loop_guard.insert(li) {
                break;
            }
            let Some(lp) = by_index.get(&li) else { break };
            if lp.head() != "loop" {
                break;
            }
            kept_loops.insert(li);
            // Ring-walk coedges via chunk[3] = next.
            if let Some(first_ce) = lp.ref_at(4) {
                let mut ce_ref = Some(first_ce);
                let mut ce_guard = HashSet::new();
                while let Some(ci) = ce_ref {
                    if !ce_guard.insert(ci) {
                        break;
                    }
                    let Some(ce) = by_index.get(&ci) else { break };
                    if !is_coedge_record(ce) {
                        break;
                    }
                    kept_coedges.insert(ci);
                    if let Some(pc) = coedge_pcurve_ref(ce) {
                        if let Some(prec) = by_index.get(&pc) {
                            if purpose == DecodePurpose::History {
                                pcurve_geo.entry(super::PcurveRecordIndex(pc)).or_insert(
                                    PcurveGeometry::Line(
                                        cadmpeg_ir::geometry::pcurve::LinePcurve::U_AXIS,
                                    ),
                                );
                                kept_pcurves.insert(pc);
                            } else {
                                // An inline `exp_par_cur` owns its first BS2 field.
                                // A wrapped subtype ref resolves that same typed
                                // field. A nonzero-discriminator ref form names one
                                // of the target intcurve's two pcurve slots; its
                                // sign composes with the intcurve sense bit.
                                let decoded = match (prec.chunk(3), prec.chunk(4)) {
                                    (Some(Token::Long(0)), Some(Token::True | Token::False)) => {
                                        if let Some(span) = nurbs::toks::payload_subtype_toks(
                                            prec,
                                            5,
                                            "exp_par_cur",
                                        ) {
                                            nurbs::pcurve::explicit_pcurve_cache(span)
                                                .map(|pcurve| Ok((pcurve, true)))
                                        } else if let Some(span) =
                                            nurbs::toks::payload_subtype_toks(prec, 5, "ref")
                                        {
                                            // The interior opens with the `ref`
                                            // identifier the lookup matched; the
                                            // index is the field after it.
                                            match span.interior() {
                                                [_name, Token::Long(index), ..] =>
                                                    nurbs::pcurve::explicit_pcurve_cache_from_subtype_ref(
                                                        *index,
                                                        token_table,
                                                    )
                                                    .map(|pcurve| Ok((pcurve, true))),
                                                _ => None,
                                            }
                                        } else {
                                            None
                                        }
                                    }
                                    (Some(Token::Long(selector)), Some(Token::Ref(reference)))
                                        if matches!(*selector, 1 | 2 | -1 | -2) =>
                                    {
                                        by_index
                                            .get(reference)
                                            .filter(|record| record.head() == "intcurve")
                                            .and_then(|intcurve| {
                                                nurbs::proc_curve::pcurve_for_selector_with_chart(
                                                    ctx,
                                                    &intcurve.tokens,
                                                    *selector,
                                                    token_table,
                                                )
                                                .map(|result| result.map(|(mut curve, native_chart)| {
                                                    if (*selector < 0) ^ record_reversed(intcurve) {
                                                        curve.reverse_parameterization();
                                                    }
                                                    (curve, native_chart)
                                                }))
                                            })
                                    }
                                    _ => None,
                                }.transpose()?;
                                let edge =
                                    ce.ref_at(6).and_then(|edge| by_index.get(&edge)).copied();
                                let decoded = decoded.map(|(mut decoded, native_chart)| -> Result<_, cadmpeg_core::CodecError> {
                                    if native_chart {
                                        if let Some(surface) = face
                                            .ref_at(7)
                                            .and_then(|surface| by_index.get(&surface))
                                        {
                                            let Some(()) = nurbs::support_chart::SupportChart::of_surface_record(surface.head(), &surface.tokens).and_then(|chart| chart.normalize(&mut decoded)) else { return Ok(None); };
                                        }
                                    }
                                    Ok(pcurve_ranges_on_domain(&decoded, edge)
                                        .and_then(|ranges| ranges.into_iter().next())
                                        .map(|range| (decoded, range)))
                                }).transpose()?.flatten();
                                if let Some((decoded, parameter_range)) = decoded {
                                    pcurve_geo.insert(
                                        super::PcurveRecordIndex(pc),
                                        PcurveGeometry::Nurbs { nurbs: decoded },
                                    );
                                    pcurve_parameter_ranges
                                        .insert(super::CoedgeRecordIndex(ci), parameter_range);
                                    kept_pcurves.insert(pc);
                                } else {
                                    count_kind(&mut out.stats.undecoded_pcurve_kinds, prec.head());
                                }
                            }
                        } else {
                            count_kind(&mut out.stats.undecoded_pcurve_kinds, "dangling-reference");
                        }
                    }
                    if let Some(ei) = ce.ref_at(6) {
                        if let Some(edge) = by_index.get(&ei) {
                            // An edge is shared by two coedges; process (and
                            // count its curve loss) only the first time it is
                            // reached so shared edges are not double-counted.
                            if is_edge_record(edge) && kept_edges.insert(ei) {
                                for slot in [3usize, 5] {
                                    if let Some(vi) = edge.ref_at(slot) {
                                        if let Some(v) = by_index.get(&vi) {
                                            if is_vertex_record(v) {
                                                kept_vertices.insert(vi);
                                                if let Some(pi) = vertex_point_ref(v) {
                                                    kept_points.insert(pi);
                                                }
                                            }
                                        }
                                    }
                                }
                                match edge.ref_at(8) {
                                    Some(cv) if curve_geo.contains_key(&cv) => {
                                        kept_curves.insert(cv);
                                    }
                                    Some(cv) => {
                                        if let Some(crec) = by_index.get(&cv) {
                                            if purpose == DecodePurpose::History {
                                                curve_geo.insert(cv, CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }));
                                                kept_curves.insert(cv);
                                            // A procedural curve carries an inline
                                            // 3D B-spline cache in most subtypes.
                                            } else if let Some(decoded) =
                                                nurbs::proc_curve::procedural_curve_resolving_refs(
                                                    ctx,
                                                    &crec.tokens,
                                                    token_table,
                                                ).transpose()?
                                            {
                                                let parsed_domain = nurbs::proc_curve::nurbs_curve_parameter_domain(&decoded.curve);
                                                let mut curve = decoded.curve;
                                                // A reversed intcurve parameterizes
                                                // as the negation of its cache; the
                                                // edge's stored range is on the
                                                // reversed parameterization.
                                                if record_reversed(crec) {
                                                    curve.reverse_parameterization();
                                                }
                                                curve_geo.insert(cv, CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)));
                                                procedural_curve_defs.insert(cv, super::ProceduralCurveSource::Cached {
                                                        construction: Box::new(decoded.construction),
                                                        cache_fit_tolerance: decoded.cache_fit_tolerance,
                                                        parsed_domain,
                                                    });
                                                out.stats.nurbs_curves += 1;
                                                kept_curves.insert(cv);
                                            } else if let Some(definition) =
                                                nurbs::proc_curve::cacheless_procedural_curve_resolving_refs(
                                                    ctx,
                                                    &crec.tokens,
                                                    token_table,
                                                ).transpose()?.and_then(|definition| definition.into_definition().ok())
                                                .and_then(|mut definition| {
                                                    if record_reversed(crec) {
                                                        reverse_procedural_curve_definition(&mut definition).ok()?;
                                                    }
                                                    Some(definition)
                                                })
                                            {
                                                curve_geo.insert(cv, CurveGeometry::Procedural {
                                                    construction: brep_id!(
                                                        format,
                                                        ProceduralCurveId,
                                                        "procedural_curve",
                                                        cv
                                                    ),
                                                        cache: None,
                                                    });
                                                procedural_curve_defs.insert(cv, super::ProceduralCurveSource::Cacheless(Box::new(definition)));
                                                kept_curves.insert(cv);
                                            } else {
                                                undecoded_carriers.insert(cv);

                                                count_kind(&mut out.stats.procedural_curve_kinds, crec.head());
                                            }
                                        } else {
                                            count_kind(
                                                &mut out.stats.procedural_curve_kinds,
                                                "dangling-reference",
                                            );
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    ce_ref = ce.ref_at(3);
                    if ce_ref == Some(first_ce) {
                        break;
                    }
                }
            }
            loop_ref = lp.ref_at(3);
        }
    }
    Ok(())
}

/// Pass 2 (wires): collect shell wire edges and free vertices, decoding wire
/// curve carriers and emitting wire topologies.
pub(super) fn collect_wire_topology(
    inputs: TopologyContext<'_, '_, '_>,
    out: &mut AsmBrep,
    records: &[Record],
    saved_entity_limit: Option<i64>,
    carriers: &mut Carriers,
    reach: &mut Reachable,
) -> Result<WireShellTopology, cadmpeg_core::CodecError> {
    let TopologyContext {
        by_index, format, ..
    } = inputs;
    let mut wire_edges_by_shell = HashMap::<i64, Vec<i64>>::new();
    let mut free_vertices_by_shell = HashMap::<i64, Vec<i64>>::new();
    let mut saved_free_edges = Vec::new();
    if let Some(limit) = saved_entity_limit {
        for edge in records {
            let edge_index = i64::try_from(edge.index).map_err(|_| {
                cadmpeg_core::CodecError::malformed("ASM record index exceeds the i64 range")
            })?;
            if !(1..limit).contains(&edge_index) || !is_edge_record(edge) {
                continue;
            }
            let already_owned = reach.edges.contains(&edge_index);
            keep_wire_edge(inputs, out, edge_index, carriers, reach)?;
            if !already_owned && reach.edges.contains(&edge_index) {
                saved_free_edges.push(edge_index);
            }
        }
    }
    for shell in records.iter().filter(|record| record.head() == "shell") {
        let shell_index = i64::try_from(shell.index).map_err(|_| {
            cadmpeg_core::CodecError::malformed("ASM record index exceeds the i64 range")
        })?;
        let mut wire_guard = HashSet::new();
        for root in shell_wire_roots(shell, by_index) {
            let mut wire_ref = Some(root);
            while let Some(wire_index) = wire_ref {
                if !wire_guard.insert(wire_index) {
                    break;
                }
                let Some(wire) = by_index
                    .get(&wire_index)
                    .filter(|record| record.head() == "wire")
                else {
                    break;
                };
                let side = match wire.chunk(7) {
                    Some(Token::True) => Some(WireSide::In),
                    Some(Token::False) => Some(WireSide::Out),
                    _ => None,
                };
                let mut wire_edges = Vec::new();
                if let Some(first_coedge) = wire.ref_at(4) {
                    let mut coedge_ref = Some(first_coedge);
                    let mut coedge_guard = HashSet::new();
                    while let Some(coedge_index) = coedge_ref {
                        if !coedge_guard.insert(coedge_index) {
                            break;
                        }
                        let Some(coedge) = by_index
                            .get(&coedge_index)
                            .filter(|record| is_coedge_record(record))
                        else {
                            break;
                        };
                        if let Some(edge_index) = coedge.ref_at(6) {
                            if !wire_edges.contains(&edge_index) {
                                wire_edges.push(edge_index);
                            }

                            let edges = wire_edges_by_shell.entry(shell_index).or_default();
                            if !edges.contains(&edge_index) {
                                edges.push(edge_index);
                            }
                            keep_wire_edge(inputs, out, edge_index, carriers, reach)?;
                        }
                        coedge_ref = coedge.ref_at(3);
                        if coedge_ref == Some(first_coedge) {
                            break;
                        }
                    }
                }
                let free_vertex = if wire.ref_at(4).is_none() {
                    wire.ref_at(6).filter(|vertex| {
                        by_index
                            .get(vertex)
                            .is_some_and(|record| is_vertex_record(record))
                    })
                } else {
                    None
                };
                if let Some(vertex) = free_vertex {
                    reach.vertices.insert(vertex);

                    let vertices = free_vertices_by_shell.entry(shell_index).or_default();
                    if !vertices.contains(&vertex) {
                        vertices.push(vertex);
                    }
                    if let Some(point) = by_index
                        .get(&vertex)
                        .and_then(|record| vertex_point_ref(record))
                    {
                        reach.points.insert(point);
                    }
                }
                if let Some(side) = side {
                    out.wire_topologies.push(WireTopology {
                        source_namespace:
                            crate::brep::records::identity::NativeRecordNamespace::new(format),
                        shell: ShellId::from(id(format, shell_index)),
                        record_index: u32::try_from(wire.index).map_err(|_| {
                            cadmpeg_core::CodecError::malformed(
                                "ASM native record index exceeds the u32 range",
                            )
                        })?,
                        members: match free_vertex {
                            Some(vertex) => WireMembers::Vertex(VertexId::from(id(format, vertex))),
                            None => WireMembers::Edges(
                                wire_edges
                                    .into_iter()
                                    .map(|edge| EdgeId::from(id(format, edge)))
                                    .collect::<Vec<_>>(),
                            ),
                        },
                        side,
                    });
                }
                wire_ref = wire.ref_at(3);
            }
        }
    }
    Ok(WireShellTopology {
        wire_edges_by_shell,
        free_vertices_by_shell,
        saved_free_edges,
    })
}

fn keep_wire_edge(
    inputs: TopologyContext<'_, '_, '_>,
    out: &mut AsmBrep,
    edge_index: i64,
    carriers: &mut Carriers,
    reach: &mut Reachable,
) -> Result<(), cadmpeg_core::CodecError> {
    let TopologyContext {
        ctx,
        by_index,
        token_table,
        purpose,
        format,
    } = inputs;
    let Carriers {
        curve_geo,
        procedural_curve_defs,
        ..
    } = carriers;
    let Reachable {
        edges: kept_edges,
        vertices: kept_vertices,
        points: kept_points,
        curves: kept_curves,
        undecoded_carriers,
        ..
    } = reach;
    let Some(edge) = by_index
        .get(&edge_index)
        .filter(|edge| is_edge_record(edge))
    else {
        return Ok(());
    };
    if !kept_edges.insert(edge_index) {
        return Ok(());
    }
    for slot in [3usize, 5] {
        if let Some(vertex_index) = edge.ref_at(slot) {
            if let Some(vertex) = by_index
                .get(&vertex_index)
                .filter(|vertex| is_vertex_record(vertex))
            {
                kept_vertices.insert(vertex_index);
                if let Some(point_index) = vertex_point_ref(vertex) {
                    kept_points.insert(point_index);
                }
            }
        }
    }
    let Some(curve_index) = edge.ref_at(8) else {
        return Ok(());
    };
    if curve_geo.contains_key(&curve_index) {
        kept_curves.insert(curve_index);
    } else {
        let Some(curve_record) = by_index.get(&curve_index) else {
            count_kind(&mut out.stats.procedural_curve_kinds, "dangling-reference");
            return Ok(());
        };
        if purpose == DecodePurpose::History {
            curve_geo.insert(
                curve_index,
                CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
            );
            kept_curves.insert(curve_index);
            return Ok(());
        }
        if let Some(decoded) = nurbs::proc_curve::procedural_curve_resolving_refs(
            ctx,
            &curve_record.tokens,
            token_table,
        )
        .transpose()?
        {
            let parsed_domain = nurbs::proc_curve::nurbs_curve_parameter_domain(&decoded.curve);
            let mut curve = decoded.curve;
            if record_reversed(curve_record) {
                curve.reverse_parameterization();
            }
            curve_geo.insert(
                curve_index,
                CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
            );
            procedural_curve_defs.insert(
                curve_index,
                super::ProceduralCurveSource::Cached {
                    construction: Box::new(decoded.construction),
                    cache_fit_tolerance: decoded.cache_fit_tolerance,
                    parsed_domain,
                },
            );
            kept_curves.insert(curve_index);
            out.stats.nurbs_curves += 1;
        } else if let Some(definition) =
            nurbs::proc_curve::cacheless_procedural_curve_resolving_refs(
                ctx,
                &curve_record.tokens,
                token_table,
            )
            .transpose()?
            .and_then(|definition| definition.into_definition().ok())
            .and_then(|mut definition| {
                if record_reversed(curve_record) {
                    reverse_procedural_curve_definition(&mut definition).ok()?;
                }
                Some(definition)
            })
        {
            curve_geo.insert(
                curve_index,
                CurveGeometry::Procedural {
                    construction: brep_id!(
                        format,
                        ProceduralCurveId,
                        "procedural_curve",
                        curve_index
                    ),
                    cache: None,
                },
            );
            procedural_curve_defs.insert(
                curve_index,
                super::ProceduralCurveSource::Cacheless(Box::new(definition)),
            );
            kept_curves.insert(curve_index);
        } else {
            undecoded_carriers.insert(curve_index);

            count_kind(&mut out.stats.procedural_curve_kinds, curve_record.head());
        }
    }
    Ok(())
}

/// Partition kept edges' curve references by sense so a carrier shared across
/// both senses can emit a `:reversed` clone beside its forward orientation.
pub(super) fn classify_edge_curve_senses(
    records: &[Record],
    reach: &Reachable,
) -> Result<(HashSet<i64>, HashSet<i64>), cadmpeg_core::CodecError> {
    let Reachable {
        edges: kept_edges,
        curves: kept_curves,
        ..
    } = reach;
    let mut reversed_curve_refs: HashSet<i64> = HashSet::new();
    let mut forward_curve_refs: HashSet<i64> = HashSet::new();
    for r in records {
        if !is_edge_record(r)
            || !kept_edges.contains(
                &(i64::try_from(r.index).map_err(|_| {
                    cadmpeg_core::CodecError::malformed("ASM record index exceeds the i64 range")
                })?),
            )
        {
            continue;
        }
        let Some(curve) = r.ref_at(8).filter(|c| kept_curves.contains(c)) else {
            continue;
        };
        match sense_at(r, 9) {
            Sense::Reversed => reversed_curve_refs.insert(curve),
            Sense::Forward => forward_curve_refs.insert(curve),
        };
    }
    Ok((reversed_curve_refs, forward_curve_refs))
}

pub(super) fn ring_coedges(
    loop_rec: &Record,
    by_index: &HashMap<i64, &Record>,
    kept: &HashSet<i64>,
    format: IdFormat,
) -> Vec<CoedgeId> {
    let id = |i: i64| CoedgeId::from(super::id(format, i));
    let mut out = Vec::new();
    let Some(first) = loop_rec.ref_at(4) else {
        return out;
    };
    let mut cur = Some(first);
    let mut guard = HashSet::new();
    while let Some(ci) = cur {
        if !guard.insert(ci) || !kept.contains(&ci) {
            break;
        }
        out.push(id(ci));
        let Some(ce) = by_index.get(&ci) else { break };
        cur = ce.ref_at(3);
        if cur == Some(first) {
            break;
        }
    }
    out
}

pub(super) fn loop_chain(
    face_rec: &Record,
    by_index: &HashMap<i64, &Record>,
    kept: &HashSet<i64>,
    format: IdFormat,
) -> Vec<LoopId> {
    let id = |i: i64| LoopId::from(super::id(format, i));
    let mut out = Vec::new();
    let mut cur = face_rec.ref_at(4);
    let mut guard = HashSet::new();
    while let Some(li) = cur {
        if !guard.insert(li) {
            break;
        }
        if kept.contains(&li) {
            out.push(id(li));
        }
        let Some(lp) = by_index.get(&li) else { break };
        cur = lp.ref_at(3);
    }
    out
}

fn face_chain(
    shell_rec: &Record,
    by_index: &HashMap<i64, &Record>,
    kept: &HashSet<i64>,
    format: IdFormat,
) -> Vec<FaceId> {
    let id = |i: i64| FaceId::from(super::id(format, i));
    let mut out = Vec::new();
    let mut cur = shell_rec.ref_at(5);
    let mut guard = HashSet::new();
    while let Some(fi) = cur {
        if !guard.insert(fi) {
            break;
        }
        if kept.contains(&fi) {
            out.push(id(fi));
        }
        let Some(f) = by_index.get(&fi) else { break };
        cur = f.ref_at(3);
    }
    out
}

pub(super) fn subshell_ancestor_shells(
    records: &[Record],
    by_index: &HashMap<i64, &Record>,
) -> Result<HashMap<i64, i64>, cadmpeg_core::CodecError> {
    let mut out = HashMap::new();
    for record in records.iter().filter(|record| record.head() == "subshell") {
        let mut owner = record.ref_at(3);
        let mut guard = HashSet::new();
        while let Some(index) = owner {
            if !guard.insert(index) {
                break;
            }
            let Some(parent) = by_index.get(&index) else {
                break;
            };
            if parent.head() == "shell" {
                out.insert(
                    i64::try_from(record.index).map_err(|_| {
                        cadmpeg_core::CodecError::malformed(
                            "ASM record index exceeds the i64 range",
                        )
                    })?,
                    index,
                );
                break;
            }
            if parent.head() != "subshell" {
                break;
            }
            owner = parent.ref_at(3);
        }
    }
    Ok(out)
}

pub(super) fn shell_faces(
    shell: &Record,
    by_index: &HashMap<i64, &Record>,
    kept: &HashSet<i64>,
    format: IdFormat,
) -> Vec<FaceId> {
    let mut out = face_chain(shell, by_index, kept, format);
    let mut pending = shell.ref_at(4).into_iter().collect::<Vec<_>>();
    let mut guard = HashSet::new();
    while let Some(index) = pending.pop() {
        if !guard.insert(index) {
            break;
        }
        let Some(record) = by_index
            .get(&index)
            .filter(|record| record.head() == "subshell")
        else {
            break;
        };
        for face in face_chain_from(record.ref_at(6), by_index, kept, format) {
            out.push(face);
        }
        if let Some(next) = record.ref_at(4) {
            pending.push(next);
        }
        if let Some(child) = record.ref_at(5) {
            pending.push(child);
        }
    }
    out
}

pub(super) fn shell_wire_roots(shell: &Record, by_index: &HashMap<i64, &Record>) -> Vec<i64> {
    let mut out = shell.ref_at(6).into_iter().collect::<Vec<_>>();
    let mut pending = shell.ref_at(4).into_iter().collect::<Vec<_>>();
    let mut guard = HashSet::new();
    while let Some(index) = pending.pop() {
        if !guard.insert(index) {
            break;
        }
        let Some(record) = by_index
            .get(&index)
            .filter(|record| record.head() == "subshell")
        else {
            break;
        };
        if let Some(wire) = record.ref_at(7) {
            out.push(wire);
        }
        if let Some(next) = record.ref_at(4) {
            pending.push(next);
        }
        if let Some(child) = record.ref_at(5) {
            pending.push(child);
        }
    }
    out
}

fn face_chain_from(
    mut current: Option<i64>,
    by_index: &HashMap<i64, &Record>,
    kept: &HashSet<i64>,
    format: IdFormat,
) -> Vec<FaceId> {
    let mut out = Vec::new();
    let mut guard = HashSet::new();
    while let Some(index) = current {
        if !guard.insert(index) {
            break;
        }
        if kept.contains(&index) {
            out.push(FaceId::from(id(format, index)));
        }
        let Some(face) = by_index.get(&index) else {
            break;
        };
        current = face.ref_at(3);
    }
    out
}

/// Whether `index` names a record with one of `heads` whose owner slot refers
/// back to `parent`. Emission writes exactly these records, so a parent lists
/// only children that exist in the output.
fn owned_child<'a>(
    by_index: &HashMap<i64, &'a Record>,
    index: i64,
    heads: &[&str],
    owner_slot: usize,
    parent: &Record,
) -> Option<&'a Record> {
    let parent = i64::try_from(parent.index).ok()?;
    by_index.get(&index).copied().filter(|record| {
        heads.contains(&record.head()) && record.ref_at(owner_slot) == Some(parent)
    })
}

pub(super) fn shell_chain(
    region_rec: &Record,
    by_index: &HashMap<i64, &Record>,
    emitted_shells: &HashSet<i64>,
    format: IdFormat,
) -> Vec<ShellId> {
    let id = |i: i64| ShellId::from(super::id(format, i));
    let mut out = Vec::new();
    let mut cur = region_rec.ref_at(4);
    let mut guard = HashSet::new();
    while let Some(si) = cur {
        if !guard.insert(si) {
            break;
        }
        let Some(s) = owned_child(by_index, si, &["shell"], 7, region_rec) else {
            break;
        };
        if emitted_shells.contains(&si) {
            out.push(id(si));
        }
        cur = s.ref_at(3);
    }
    out
}

pub(super) fn region_chain(
    body_rec: &Record,
    by_index: &HashMap<i64, &Record>,
    format: IdFormat,
) -> Vec<RegionId> {
    let id = |i: i64| RegionId::from(super::id(format, i));
    let mut out = Vec::new();
    let mut cur = body_rec.ref_at(3);
    let mut guard = HashSet::new();
    while let Some(li) = cur {
        if !guard.insert(li) {
            break;
        }
        let Some(l) = owned_child(by_index, li, &["region", "lump"], 5, body_rec) else {
            break;
        };
        out.push(id(li));
        cur = l.ref_at(3);
    }
    out
}

#[cfg(test)]
mod tests;
