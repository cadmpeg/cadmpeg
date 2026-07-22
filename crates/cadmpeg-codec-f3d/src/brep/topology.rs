// SPDX-License-Identifier: Apache-2.0
//! Walk the reachable topology graph, collect wire and shell chains, and
//! classify edge curve senses.

use crate::nurbs;
use crate::records::{MeshSurfaceSentinel, WireSide, WireTopology};
use crate::sab::{Record, Token};
use cadmpeg_ir::geometry::{CurveGeometry, PcurveGeometry, SurfaceGeometry};
use cadmpeg_ir::ids::{
    CoedgeId, EdgeId, FaceId, LoopId, ProceduralSurfaceId, RegionId, ShellId, SurfaceId, UnknownId,
    VertexId,
};
use cadmpeg_ir::topology::Sense;
use std::collections::{HashMap, HashSet};

use super::attributes::{record_slice, unknown_record_id};
use super::geometry::{
    analytic_procedural_surface, decode_curve, decode_surface, is_analytic_curve,
    is_analytic_surface, is_coedge_record, is_edge_record, is_vertex_record,
    pcurve_ranges_on_domain, procedural_surface_definition_is_exact_carrier, record_reversed,
    reverse_nurbs_curve, reverse_procedural_curve_definition, select_face_pcurve, sense_at,
};
use super::{count_kind, id, Brep, Carriers, Reachable, WireShellTopology};
/// Pass 1: classify carriers and decode analytic geometry. Returns the seeded
/// carrier maps and the set of carriers whose native normal is inward.
pub(crate) fn decode_analytic_carriers(records: &[Record]) -> (Carriers, HashSet<i64>) {
    let mut surface_geo: HashMap<i64, (SurfaceGeometry, bool)> = HashMap::new();
    let mut curve_geo: HashMap<i64, CurveGeometry> = HashMap::new();
    for r in records {
        if is_analytic_surface(&r.head) {
            if let Some(g) = decode_surface(r) {
                surface_geo.insert(r.index as i64, g);
            }
        } else if is_analytic_curve(&r.head) {
            if let Some(g) = decode_curve(r) {
                curve_geo.insert(r.index as i64, g);
            }
        }
    }
    // Carriers whose native normal points opposite the IR carrier's normal;
    // the reversal folds into the referencing faces' senses.
    let inward_normal_surfaces: HashSet<i64> = surface_geo
        .iter()
        .filter(|(_, (_, inward))| *inward)
        .map(|(&index, _)| index)
        .collect();
    let carriers = Carriers {
        surface_geo,
        curve_geo,
        ..Carriers::default()
    };
    (carriers, inward_normal_surfaces)
}

/// Pass 2 (faces): keep every face whose surface reference resolves, decoding
/// or classifying its carrier and recording surface reachability.
pub(crate) fn keep_faces_and_carriers(
    out: &mut Brep,
    records: &[Record],
    bytes: &[u8],
    by_index: &HashMap<i64, &Record>,
    subtype_tables: &nurbs::subtypes::SubtypeTables,
    carriers: &mut Carriers,
    reach: &mut Reachable,
) {
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
        if r.head != "face" {
            continue;
        }
        let Some(surf_ref) = r.ref_at(7) else {
            out.stats.missing_face_surfaces += 1;
            count_kind(&mut out.stats.missing_face_surface_kinds, "null-reference");
            continue;
        };
        let Some(surf_rec) = by_index.get(&surf_ref) else {
            // Dangling surface reference: a face without a resolvable surface
            // cannot be emitted (the IR requires one), so it is dropped.
            out.stats.missing_face_surfaces += 1;
            count_kind(
                &mut out.stats.missing_face_surface_kinds,
                "dangling-reference",
            );
            continue;
        };
        kept_faces.insert(r.index as i64);
        if let Some(procedural) = nurbs::proc_surface::decode_procedural_surface_resolving_refs(
            record_slice(surf_rec, bytes),
            bytes,
            subtype_tables,
        ) {
            procedural_surface_defs.insert(surf_ref, procedural);
        }
        if let Some(geometry) = procedural_surface_defs
            .get(&surf_ref)
            .and_then(|procedural| analytic_procedural_surface(&procedural.definition))
        {
            surface_geo.insert(surf_ref, (geometry, false));
        }
        let exact_cacheless_construction =
            procedural_surface_defs
                .get(&surf_ref)
                .is_some_and(|procedural| {
                    procedural.cache_fit_tolerance.is_none()
                        && procedural_surface_definition_is_exact_carrier(&procedural.definition)
                });
        // A non-analytic surface may still carry a decodable B-spline face
        // cache. Exact cacheless constructions own their nested surface blocks
        // as supports, not as evaluated face caches.
        if !exact_cacheless_construction {
            if let std::collections::hash_map::Entry::Vacant(e) = surface_geo.entry(surf_ref) {
                if let Some(ns) = nurbs::core::decode_surface_cache_resolving_refs(
                    record_slice(surf_rec, bytes),
                    bytes,
                    subtype_tables,
                ) {
                    e.insert((SurfaceGeometry::Nurbs(ns), false));
                    if surf_rec.head == "spline" && !procedural_surface_defs.contains_key(&surf_ref)
                    {
                        cached_unknown_procedural_surfaces.insert(surf_ref);
                    }
                    out.stats.nurbs_surfaces += 1;
                }
            }
        }
        if !surface_geo.contains_key(&surf_ref) && procedural_surface_defs.contains_key(&surf_ref) {
            let analytic_geometry = procedural_surface_defs
                .get(&surf_ref)
                .and_then(|procedural| analytic_procedural_surface(&procedural.definition));
            let construction_is_exact_carrier =
                procedural_surface_defs
                    .get(&surf_ref)
                    .is_some_and(|procedural| {
                        procedural_surface_definition_is_exact_carrier(&procedural.definition)
                    });
            surface_geo.insert(
                surf_ref,
                (
                    if let Some(geometry) = analytic_geometry {
                        geometry
                    } else if construction_is_exact_carrier {
                        SurfaceGeometry::Procedural {
                            construction: ProceduralSurfaceId(format!(
                                "f3d:brep:procedural_surface#{surf_ref}"
                            )),
                        }
                    } else {
                        SurfaceGeometry::Unknown {
                            record: Some(UnknownId(unknown_record_id(surf_rec))),
                        }
                    },
                    false,
                ),
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
            if surf_rec.head == "mesh_surface" && surf_rec.tokens.is_empty() {
                if !out
                    .mesh_surface_sentinels
                    .iter()
                    .any(|sentinel| sentinel.record_index == surf_rec.index as u32)
                {
                    out.mesh_surface_sentinels.push(MeshSurfaceSentinel {
                        id: format!("f3d:asm:mesh-surface-sentinel#{}", surf_rec.index),
                        surface: SurfaceId(id(surf_ref)),
                        record_index: surf_rec.index as u32,
                    });
                }
                out.stats.mesh_surface_faces += 1;
            } else {
                out.stats.unknown_surface_faces += 1;
                count_kind(&mut out.stats.unknown_surface_kinds, &surf_rec.head);
            }
        }
    }
}

/// Pass 2 (topology): walk each kept face's loops and coedge rings, pulling in
/// the supporting edge/vertex/point graph and decoding curve and pcurve carriers.
pub(crate) fn walk_reachable_topology(
    out: &mut Brep,
    by_index: &HashMap<i64, &Record>,
    bytes: &[u8],
    ref_width: usize,
    subtype_tables: &nurbs::subtypes::SubtypeTables,
    carriers: &mut Carriers,
    reach: &mut Reachable,
) {
    let Carriers {
        surface_geo,
        procedural_surface_defs,
        curve_geo,
        procedural_curve_defs,
        cacheless_procedural_curve_defs,
        pcurve_geo,
        pcurve_parameter_ranges,
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
    for &face_idx in &kept_faces.iter().copied().collect::<Vec<_>>() {
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
            if lp.head != "loop" {
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
                    if let Some(pc) = ce.ref_at(10) {
                        if let Some(prec) = by_index.get(&pc) {
                            // A wrapped pcurve either owns an inline 2D block
                            // or delegates through a subtype-table reference;
                            // a nonzero-discriminator ref form delegates to an
                            // intcurve entity. Decode every referenced candidate
                            // and keep the one whose endpoints land on the edge's
                            // vertices through the face surface. An inline scope
                            // owns exactly one BS2 carrier and needs no
                            // disambiguation.
                            let inline = matches!(
                                (prec.chunk(3), prec.chunk(4)),
                                (Some(Token::Long(0)), Some(Token::True | Token::False))
                            );
                            let candidates = match (prec.chunk(3), prec.chunk(4)) {
                                (Some(Token::Long(0)), Some(Token::True | Token::False)) => {
                                    if let Some(span) = crate::sab::payload_subtype_span(
                                        bytes,
                                        prec,
                                        5,
                                        ref_width,
                                        "exp_par_cur",
                                    ) {
                                        nurbs::pcurve::decode_pcurve_cache_candidates_resolving_refs(
                                            span,
                                            bytes,
                                            subtype_tables,
                                        )
                                    } else if crate::sab::payload_subtype_span(
                                        bytes, prec, 5, ref_width, "ref",
                                    )
                                    .is_some()
                                    {
                                        // The resolver needs the `ref N` opener and name,
                                        // not only the scope interior returned above.
                                        nurbs::pcurve::decode_pcurve_cache_candidates_resolving_refs(
                                            record_slice(prec, bytes),
                                            bytes,
                                            subtype_tables,
                                        )
                                    } else {
                                        Vec::new()
                                    }
                                }
                                (
                                    Some(Token::Long(1 | 2 | -1 | -2)),
                                    Some(Token::Ref(reference)),
                                ) => by_index
                                    .get(reference)
                                    .filter(|record| record.head == "intcurve")
                                    .map(|intcurve| {
                                        nurbs::pcurve::decode_pcurve_cache_candidates_resolving_refs(
                                            record_slice(intcurve, bytes),
                                            bytes,
                                            subtype_tables,
                                        )
                                    })
                                    .unwrap_or_default(),
                                _ => Vec::new(),
                            };
                            let edge = ce.ref_at(6).and_then(|edge| by_index.get(&edge)).copied();
                            let decoded = if inline && candidates.len() == 1 {
                                let candidate =
                                    candidates.into_iter().next().expect("one candidate");
                                let range = pcurve_ranges_on_domain(&candidate.curve, edge)
                                    .and_then(|ranges| ranges.into_iter().next());
                                range.map(|range| (candidate.curve, range))
                            } else if candidates
                                .iter()
                                .filter(|candidate| candidate.unambiguous_2d)
                                .count()
                                == 1
                            {
                                let candidate = candidates
                                    .into_iter()
                                    .find(|candidate| candidate.unambiguous_2d)
                                    .expect("one unambiguous candidate");
                                let range = pcurve_ranges_on_domain(&candidate.curve, edge)
                                    .and_then(|ranges| ranges.into_iter().next());
                                range.map(|range| (candidate.curve, range))
                            } else {
                                select_face_pcurve(
                                    candidates
                                        .into_iter()
                                        .map(|candidate| candidate.curve)
                                        .collect(),
                                    face.ref_at(7)
                                        .and_then(|surface| surface_geo.get(&surface))
                                        .map(|(geometry, _)| geometry),
                                    face.ref_at(7).is_some_and(|surface| {
                                        procedural_surface_defs.contains_key(&surface)
                                    }),
                                    edge,
                                    by_index,
                                )
                            };
                            if let Some((decoded, parameter_range)) = decoded {
                                pcurve_geo.insert(
                                    pc,
                                    PcurveGeometry::Nurbs {
                                        degree: decoded.degree,
                                        knots: decoded.knots,
                                        control_points: decoded.control_points,
                                        weights: decoded.weights,
                                        periodic: decoded.periodic,
                                    },
                                );
                                pcurve_parameter_ranges.insert(ci, parameter_range);
                                kept_pcurves.insert(pc);
                            } else {
                                out.stats.undecoded_pcurve_refs += 1;
                                count_kind(&mut out.stats.undecoded_pcurve_kinds, &prec.head);
                            }
                        } else {
                            out.stats.undecoded_pcurve_refs += 1;
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
                                                if let Some(pi) = v.ref_at(5) {
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
                                            // A procedural curve carries an inline
                                            // 3D B-spline cache in most subtypes.
                                            if let Some(decoded) =
                                                nurbs::proc_curve::decode_procedural_curve_resolving_refs(
                                                    record_slice(crec, bytes),
                                                    bytes,
                                                    subtype_tables,
                                                )
                                            {
                                                let mut curve = decoded.curve;
                                                // A reversed intcurve parameterizes
                                                // as the negation of its cache; the
                                                // edge's stored range is on the
                                                // reversed parameterization.
                                                if record_reversed(crec) {
                                                    reverse_nurbs_curve(&mut curve);
                                                }
                                                curve_geo.insert(cv, CurveGeometry::Nurbs(curve));
                                                procedural_curve_defs.insert(
                                                    cv,
                                                    (
                                                        decoded.native_kind,
                                                        decoded.definition,
                                                        decoded.vector_offset,
                                                        decoded.subset,
                                                        decoded.compound,
                                                        decoded.embedded_two_sided_offset,
                                                        decoded.embedded_intersection,
                                                        decoded.embedded_three_surface_intersection,
                                                        decoded.embedded_surface_curve,
                                                        decoded.embedded_silhouette,
                                                        decoded.embedded_surface_offset,
                                                        decoded.embedded_spring,
                                                        decoded.embedded_deformable,
                                                        decoded.embedded_projection,
                                                        decoded.embedded_law,
                                                        decoded.cache_fit_tolerance,
                                                    ),
                                                );
                                                out.stats.nurbs_curves += 1;
                                                kept_curves.insert(cv);
                                            } else if let Some((native_kind, mut definition)) =
                                                nurbs::proc_curve::decode_cacheless_procedural_curve_resolving_refs(
                                                    record_slice(crec, bytes),
                                                    bytes,
                                                    subtype_tables,
                                                )
                                            {
                                                if record_reversed(crec) {
                                                    reverse_procedural_curve_definition(
                                                        &mut definition,
                                                    );
                                                }
                                                curve_geo.insert(
                                                    cv,
                                                    CurveGeometry::Procedural {
                                                        construction: format!(
                                                            "f3d:brep:procedural_curve#{cv}"
                                                        )
                                                        .into(),
                                                    },
                                                );
                                                cacheless_procedural_curve_defs
                                                    .insert(cv, (native_kind, definition));
                                                kept_curves.insert(cv);
                                            } else {
                                                undecoded_carriers.insert(cv);
                                                out.stats.procedural_curve_edges += 1;
                                                count_kind(
                                                    &mut out.stats.procedural_curve_kinds,
                                                    &crec.head,
                                                );
                                            }
                                        } else {
                                            out.stats.procedural_curve_edges += 1;
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
}

/// Pass 2 (wires): collect shell wire edges and free vertices, decoding wire
/// curve carriers and emitting wire topologies.
pub(crate) fn collect_wire_topology(
    out: &mut Brep,
    records: &[Record],
    by_index: &HashMap<i64, &Record>,
    bytes: &[u8],
    subtype_tables: &nurbs::subtypes::SubtypeTables,
    carriers: &mut Carriers,
    reach: &mut Reachable,
) -> WireShellTopology {
    let Carriers {
        curve_geo,
        procedural_curve_defs,
        cacheless_procedural_curve_defs,
        ..
    } = &mut *carriers;
    let Reachable {
        edges: kept_edges,
        vertices: kept_vertices,
        points: kept_points,
        curves: kept_curves,
        undecoded_carriers,
        ..
    } = &mut *reach;
    let mut wire_edges_by_shell = HashMap::<i64, Vec<i64>>::new();
    let mut free_vertices_by_shell = HashMap::<i64, Vec<i64>>::new();
    for shell in records.iter().filter(|record| record.head == "shell") {
        let shell_index = shell.index as i64;
        let mut wire_guard = HashSet::new();
        for root in shell_wire_roots(shell, by_index) {
            let mut wire_ref = Some(root);
            while let Some(wire_index) = wire_ref.filter(|index| wire_guard.insert(*index)) {
                let Some(wire) = by_index
                    .get(&wire_index)
                    .filter(|record| record.head == "wire")
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
                    while let Some(coedge_index) =
                        coedge_ref.filter(|index| coedge_guard.insert(*index))
                    {
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
                            if let Some(edge) = by_index.get(&edge_index) {
                                if is_edge_record(edge) && kept_edges.insert(edge_index) {
                                    for slot in [3usize, 5] {
                                        if let Some(vertex_index) = edge.ref_at(slot) {
                                            if let Some(vertex) = by_index.get(&vertex_index) {
                                                if is_vertex_record(vertex) {
                                                    kept_vertices.insert(vertex_index);
                                                    if let Some(point_index) = vertex.ref_at(5) {
                                                        kept_points.insert(point_index);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    if let Some(curve_index) = edge.ref_at(8) {
                                        match curve_geo.entry(curve_index) {
                                            std::collections::hash_map::Entry::Occupied(_) => {
                                                kept_curves.insert(curve_index);
                                            }
                                            std::collections::hash_map::Entry::Vacant(entry) => {
                                                if let Some(curve_record) =
                                                    by_index.get(&curve_index)
                                                {
                                                    if let Some(decoded) =
                                                        nurbs::proc_curve::decode_procedural_curve_resolving_refs(
                                                            record_slice(curve_record, bytes),
                                                            bytes,
                                                            subtype_tables,
                                                        )
                                                {
                                                    let mut curve = decoded.curve;
                                                    if record_reversed(curve_record) {
                                                        reverse_nurbs_curve(&mut curve);
                                                    }
                                                    entry.insert(CurveGeometry::Nurbs(curve));
                                                    procedural_curve_defs.insert(
                                                        curve_index,
                                                        (
                                                            decoded.native_kind,
                                                            decoded.definition,
                                                            decoded.vector_offset,
                                                            decoded.subset,
                                                            decoded.compound,
                                                            decoded.embedded_two_sided_offset,
                                                            decoded.embedded_intersection,
                                                            decoded.embedded_three_surface_intersection,
                                                            decoded.embedded_surface_curve,
                                                            decoded.embedded_silhouette,
                                                            decoded.embedded_surface_offset,
                                                            decoded.embedded_spring,
                                                            decoded.embedded_deformable,
                                                            decoded.embedded_projection,
                                                            decoded.embedded_law,
                                                            decoded.cache_fit_tolerance,
                                                        ),
                                                    );
                                                    kept_curves.insert(curve_index);
                                                    out.stats.nurbs_curves += 1;
                                                } else if let Some((native_kind, mut definition)) =
                                                    nurbs::proc_curve::decode_cacheless_procedural_curve_resolving_refs(
                                                        record_slice(curve_record, bytes),
                                                        bytes,
                                                        subtype_tables,
                                                    )
                                                {
                                                    if record_reversed(curve_record) {
                                                        reverse_procedural_curve_definition(
                                                            &mut definition,
                                                        );
                                                    }
                                                    entry.insert(CurveGeometry::Procedural {
                                                        construction: format!(
                                                            "f3d:brep:procedural_curve#{curve_index}"
                                                        )
                                                        .into(),
                                                    });
                                                    cacheless_procedural_curve_defs.insert(
                                                        curve_index,
                                                        (native_kind, definition),
                                                    );
                                                    kept_curves.insert(curve_index);
                                                } else {
                                                    undecoded_carriers.insert(curve_index);
                                                    out.stats.procedural_curve_edges += 1;
                                                    count_kind(
                                                        &mut out.stats.procedural_curve_kinds,
                                                        &curve_record.head,
                                                    );
                                                }
                                                } else {
                                                    out.stats.procedural_curve_edges += 1;
                                                    count_kind(
                                                        &mut out.stats.procedural_curve_kinds,
                                                        "dangling-reference",
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                            }
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
                    kept_vertices.insert(vertex);
                    let vertices = free_vertices_by_shell.entry(shell_index).or_default();
                    if !vertices.contains(&vertex) {
                        vertices.push(vertex);
                    }
                    if let Some(point) = by_index.get(&vertex).and_then(|record| record.ref_at(5)) {
                        kept_points.insert(point);
                    }
                }
                if let Some(side) = side {
                    out.wire_topologies.push(WireTopology {
                        id: format!("f3d:asm:wire-topology#{wire_index}"),
                        shell: ShellId(id(shell_index)),
                        record_index: wire.index as u32,
                        edges: wire_edges
                            .into_iter()
                            .map(|edge| EdgeId(id(edge)))
                            .collect(),
                        free_vertex: free_vertex.map(|vertex| VertexId(id(vertex))),
                        side,
                    });
                }
                wire_ref = wire.ref_at(3);
            }
        }
    }
    WireShellTopology {
        wire_edges_by_shell,
        free_vertices_by_shell,
    }
}

/// Partition kept edges' curve references by sense so a carrier shared across
/// both senses can emit a `:reversed` clone beside its forward orientation.
pub(crate) fn classify_edge_curve_senses(
    records: &[Record],
    reach: &Reachable,
) -> (HashSet<i64>, HashSet<i64>) {
    let Reachable {
        edges: kept_edges,
        curves: kept_curves,
        ..
    } = reach;
    let mut reversed_curve_refs: HashSet<i64> = HashSet::new();
    let mut forward_curve_refs: HashSet<i64> = HashSet::new();
    for r in records {
        if !is_edge_record(r) || !kept_edges.contains(&(r.index as i64)) {
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
    (reversed_curve_refs, forward_curve_refs)
}

pub(crate) fn ring_coedges(
    loop_rec: &Record,
    by_index: &HashMap<i64, &Record>,
    kept: &HashSet<i64>,
) -> Vec<CoedgeId> {
    let id = |i: i64| CoedgeId(format!("f3d:brep:entity#{i}"));
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

pub(crate) fn loop_chain(
    face_rec: &Record,
    by_index: &HashMap<i64, &Record>,
    kept: &HashSet<i64>,
) -> Vec<LoopId> {
    let id = |i: i64| LoopId(format!("f3d:brep:entity#{i}"));
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
) -> Vec<FaceId> {
    let id = |i: i64| FaceId(format!("f3d:brep:entity#{i}"));
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

pub(crate) fn subshell_ancestor_shells(
    records: &[Record],
    by_index: &HashMap<i64, &Record>,
) -> HashMap<i64, i64> {
    let mut out = HashMap::new();
    for record in records.iter().filter(|record| record.head == "subshell") {
        let mut owner = record.ref_at(3);
        let mut guard = HashSet::new();
        while let Some(index) = owner.filter(|index| guard.insert(*index)) {
            let Some(parent) = by_index.get(&index) else {
                break;
            };
            if parent.head == "shell" {
                out.insert(record.index as i64, index);
                break;
            }
            if parent.head != "subshell" {
                break;
            }
            owner = parent.ref_at(3);
        }
    }
    out
}

pub(crate) fn shell_faces(
    shell: &Record,
    by_index: &HashMap<i64, &Record>,
    kept: &HashSet<i64>,
) -> Vec<FaceId> {
    let mut out = face_chain(shell, by_index, kept);
    let mut pending = shell.ref_at(4).into_iter().collect::<Vec<_>>();
    let mut guard = HashSet::new();
    while let Some(index) = pending.pop().filter(|index| guard.insert(*index)) {
        let Some(record) = by_index
            .get(&index)
            .filter(|record| record.head == "subshell")
        else {
            break;
        };
        out.extend(face_chain_from(record.ref_at(6), by_index, kept));
        if let Some(next) = record.ref_at(4) {
            pending.push(next);
        }
        if let Some(child) = record.ref_at(5) {
            pending.push(child);
        }
    }
    out
}

pub(crate) fn shell_wire_roots(shell: &Record, by_index: &HashMap<i64, &Record>) -> Vec<i64> {
    let mut out = shell.ref_at(6).into_iter().collect::<Vec<_>>();
    let mut pending = shell.ref_at(4).into_iter().collect::<Vec<_>>();
    let mut guard = HashSet::new();
    while let Some(index) = pending.pop().filter(|index| guard.insert(*index)) {
        let Some(record) = by_index
            .get(&index)
            .filter(|record| record.head == "subshell")
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
) -> Vec<FaceId> {
    let mut out = Vec::new();
    let mut guard = HashSet::new();
    while let Some(index) = current.filter(|index| guard.insert(*index)) {
        if kept.contains(&index) {
            out.push(FaceId(format!("f3d:brep:entity#{index}")));
        }
        let Some(face) = by_index.get(&index) else {
            break;
        };
        current = face.ref_at(3);
    }
    out
}

pub(crate) fn shell_chain(region_rec: &Record, by_index: &HashMap<i64, &Record>) -> Vec<ShellId> {
    let id = |i: i64| ShellId(format!("f3d:brep:entity#{i}"));
    let mut out = Vec::new();
    let mut cur = region_rec.ref_at(4);
    let mut guard = HashSet::new();
    while let Some(si) = cur {
        if !guard.insert(si) {
            break;
        }
        out.push(id(si));
        let Some(s) = by_index.get(&si) else { break };
        cur = s.ref_at(3);
    }
    out
}

pub(crate) fn region_chain(body_rec: &Record, by_index: &HashMap<i64, &Record>) -> Vec<RegionId> {
    let id = |i: i64| RegionId(format!("f3d:brep:entity#{i}"));
    let mut out = Vec::new();
    let mut cur = body_rec.ref_at(3);
    let mut guard = HashSet::new();
    while let Some(li) = cur {
        if !guard.insert(li) {
            break;
        }
        out.push(id(li));
        let Some(l) = by_index.get(&li) else { break };
        cur = l.ref_at(3);
    }
    out
}
