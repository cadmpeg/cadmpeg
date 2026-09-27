// SPDX-License-Identifier: Apache-2.0
//! Walk the reachable topology graph, collect wire and shell chains, and
//! classify edge curve senses.

use super::records::{MeshSurfaceSentinel, WireMembers, WireSide, WireTopology};
use crate::decode_alloc::CountedIteratorExt;
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    records: &[Record],
) -> Result<(Carriers, HashSet<i64>), cadmpeg_core::CodecError> {
    let mut surface_geo: HashMap<i64, SurfaceGeometry> = HashMap::new();
    let mut inward_normal_surfaces = HashSet::new();
    let mut curve_geo: HashMap<i64, CurveGeometry> = HashMap::new();
    for r in records {
        if is_analytic_surface(r.head()) {
            if let Some((geometry, inward)) = decode_surface(ctx, r).transpose()? {
                if inward {
                    crate::decode_alloc::insert_hash_set(
                        ctx,
                        &mut inward_normal_surfaces,
                        r.index as i64,
                        "ASM topology inward_normal_surfaces",
                    )?;
                }
                crate::decode_alloc::insert_hash_map(
                    ctx,
                    &mut surface_geo,
                    r.index as i64,
                    SurfaceGeometry::Solved(geometry),
                    "ASM topology surface_geo",
                )?;
            }
        } else if is_analytic_curve(r.head()) {
            if let Some(g) = decode_curve(ctx, r).transpose()? {
                crate::decode_alloc::insert_hash_map(
                    ctx,
                    &mut curve_geo,
                    r.index as i64,
                    g,
                    "ASM topology curve_geo",
                )?;
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

/// Pass 2 (faces): keep every face whose surface reference resolves, decoding
/// or classifying its carrier and recording surface reachability.
#[allow(clippy::too_many_arguments)]
pub(super) fn keep_faces_and_carriers(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    out: &mut AsmBrep,
    records: &[Record],
    by_index: &HashMap<i64, &Record>,
    token_table: &nurbs::toks::SubtypeTable,
    carriers: &mut Carriers,
    reach: &mut Reachable,
    purpose: DecodePurpose,
    format: IdFormat,
) -> Result<(), cadmpeg_core::CodecError> {
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
        let Some(surf_ref) = r.ref_at(7) else {
            count_kind(
                ctx,
                &mut out.stats.missing_face_surface_kinds,
                "null-reference",
            )?;
            continue;
        };
        let Some(surf_rec) = by_index.get(&surf_ref) else {
            // Dangling surface reference: a face without a resolvable surface
            // cannot be emitted (the IR requires one), so it is dropped.

            count_kind(
                ctx,
                &mut out.stats.missing_face_surface_kinds,
                "dangling-reference",
            )?;
            continue;
        };
        crate::decode_alloc::insert_hash_set(
            ctx,
            kept_faces,
            r.index as i64,
            "ASM topology kept_faces",
        )?;
        if purpose == DecodePurpose::History {
            let native_kind = (surf_rec.head() == "spline")
                .then(|| nurbs::toks::owned_construction_subtype(ctx, &surf_rec.tokens))
                .flatten()
                .transpose()?;
            if native_kind
                .as_deref()
                .is_some_and(|kind| kind.contains("blend"))
            {
                if let Some(procedural) = nurbs::proc_surface::procedural_surface_resolving_refs(
                    ctx,
                    &surf_rec.tokens,
                    token_table,
                ) {
                    crate::decode_alloc::insert_hash_map(
                        ctx,
                        procedural_surface_defs,
                        surf_ref,
                        procedural?,
                        "ASM topology procedural_surface_defs",
                    )?;
                }
            }
            if !surface_geo.contains_key(&surf_ref) {
                crate::decode_alloc::insert_hash_map(
                    ctx,
                    surface_geo,
                    surf_ref,
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
                    "ASM topology surface_geo",
                )?;
            }
            crate::decode_alloc::insert_hash_set(
                ctx,
                kept_surfaces,
                surf_ref,
                "ASM topology kept_surfaces",
            )?;
            continue;
        }
        if let Some(procedural) = nurbs::proc_surface::procedural_surface_resolving_refs(
            ctx,
            &surf_rec.tokens,
            token_table,
        ) {
            crate::decode_alloc::insert_hash_map(
                ctx,
                procedural_surface_defs,
                surf_ref,
                procedural?,
                "ASM topology procedural_surface_defs",
            )?;
        }
        if let Some(procedural) = procedural_surface_defs.get(&surf_ref) {
            if let Some(geometry) = analytic_procedural_surface(ctx, procedural.definition()) {
                crate::decode_alloc::insert_hash_map(
                    ctx,
                    surface_geo,
                    surf_ref,
                    geometry?,
                    "ASM topology surface_geo",
                )?;
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
                crate::decode_alloc::insert_hash_map(
                    ctx,
                    surface_geo,
                    surf_ref,
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(ns?)),
                    "ASM topology surface_geo",
                )?;
                if surf_rec.head() == "spline" && !procedural_surface_defs.contains_key(&surf_ref) {
                    crate::decode_alloc::insert_hash_set(
                        ctx,
                        cached_unknown_procedural_surfaces,
                        surf_ref,
                        "ASM topology cached_unknown_procedural_surfaces",
                    )?;
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
            crate::decode_alloc::insert_hash_map(
                ctx,
                surface_geo,
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
                        record: Some(unknown_record_id(ctx, surf_rec, format)?),
                    })
                },
                "ASM topology surface_geo",
            )?;
            if !construction_is_exact_carrier {
                crate::decode_alloc::insert_hash_set(
                    ctx,
                    undecoded_carriers,
                    surf_ref,
                    "ASM topology undecoded_carriers",
                )?;
            }
        }
        if surface_geo.contains_key(&surf_ref) {
            crate::decode_alloc::insert_hash_set(
                ctx,
                kept_surfaces,
                surf_ref,
                "ASM topology kept_surfaces",
            )?;
        } else {
            crate::decode_alloc::insert_hash_set(
                ctx,
                unknown_surface_records,
                surf_ref,
                "ASM topology unknown_surface_records",
            )?;
            crate::decode_alloc::insert_hash_set(
                ctx,
                undecoded_carriers,
                surf_ref,
                "ASM topology undecoded_carriers",
            )?;
            if surf_rec.head() == "mesh_surface" && surf_rec.chunks().next().is_none() {
                if !out
                    .mesh_surface_sentinels
                    .iter()
                    .any(|sentinel| sentinel.record_index == surf_rec.index as u32)
                {
                    crate::decode_alloc::reserve_vec_slot(
                        ctx,
                        &mut out.mesh_surface_sentinels,
                        "ASM mesh surface sentinels",
                    )?;
                    out.mesh_surface_sentinels.push(MeshSurfaceSentinel {
                        source_namespace:
                            crate::brep::records::identity::NativeRecordNamespace::new(format),
                        surface: SurfaceId::from(id(format, surf_ref)),
                        record_index: surf_rec.index as u32,
                    });
                }
                out.stats.mesh_surface_faces += 1;
            } else {
                let native_kind = if surf_rec.head() == "spline" {
                    nurbs::toks::owned_construction_subtype(ctx, &surf_rec.tokens).transpose()?
                } else {
                    None
                };
                count_kind(
                    ctx,
                    &mut out.stats.unknown_surface_kinds,
                    native_kind.as_deref().unwrap_or_else(|| surf_rec.head()),
                )?;
            }
        }
    }
    Ok(())
}

/// Pass 2 (topology): walk each kept face's loops and coedge rings, pulling in
/// the supporting edge/vertex/point graph and decoding curve and pcurve carriers.
#[allow(clippy::too_many_arguments)]
pub(super) fn walk_reachable_topology(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    out: &mut AsmBrep,
    by_index: &HashMap<i64, &Record>,
    token_table: &nurbs::toks::SubtypeTable,
    carriers: &mut Carriers,
    reach: &mut Reachable,
    purpose: DecodePurpose,
    format: IdFormat,
) -> Result<(), cadmpeg_core::CodecError> {
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
    let face_indices = kept_faces
        .iter()
        .copied()
        .collect_counted_vec(ctx, "ASM reachable face walk")?;
    for face_idx in face_indices {
        let Some(face) = by_index.get(&face_idx) else {
            continue;
        };
        let mut loop_ref = face.ref_at(4);
        let mut loop_guard = HashSet::new();
        while let Some(li) = loop_ref {
            if !crate::decode_alloc::insert_hash_set(
                ctx,
                &mut loop_guard,
                li,
                "ASM topology loop_guard",
            )? {
                break;
            }
            let Some(lp) = by_index.get(&li) else { break };
            if lp.head() != "loop" {
                break;
            }
            crate::decode_alloc::insert_hash_set(ctx, kept_loops, li, "ASM topology kept_loops")?;
            // Ring-walk coedges via chunk[3] = next.
            if let Some(first_ce) = lp.ref_at(4) {
                let mut ce_ref = Some(first_ce);
                let mut ce_guard = HashSet::new();
                while let Some(ci) = ce_ref {
                    if !crate::decode_alloc::insert_hash_set(
                        ctx,
                        &mut ce_guard,
                        ci,
                        "ASM topology ce_guard",
                    )? {
                        break;
                    }
                    let Some(ce) = by_index.get(&ci) else { break };
                    if !is_coedge_record(ce) {
                        break;
                    }
                    crate::decode_alloc::insert_hash_set(
                        ctx,
                        kept_coedges,
                        ci,
                        "ASM topology kept_coedges",
                    )?;
                    if let Some(pc) = coedge_pcurve_ref(ce) {
                        if let Some(prec) = by_index.get(&pc) {
                            if purpose == DecodePurpose::History {
                                if !pcurve_geo.contains_key(&super::PcurveRecordIndex(pc)) {
                                    crate::decode_alloc::insert_hash_map(
                                        ctx,
                                        pcurve_geo,
                                        super::PcurveRecordIndex(pc),
                                        PcurveGeometry::Line(
                                            cadmpeg_ir::geometry::pcurve::LinePcurve::U_AXIS,
                                        ),
                                        "ASM topology pcurve_geo",
                                    )?;
                                }
                                crate::decode_alloc::insert_hash_set(
                                    ctx,
                                    kept_pcurves,
                                    pc,
                                    "ASM topology kept_pcurves",
                                )?;
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
                                            nurbs::pcurve::explicit_pcurve_cache(ctx, span)
                                                .map(|pcurve| pcurve.map(|pcurve| (pcurve, true)))
                                        } else if let Some(span) =
                                            nurbs::toks::payload_subtype_toks(prec, 5, "ref")
                                        {
                                            // The interior opens with the `ref`
                                            // identifier the lookup matched; the
                                            // index is the field after it.
                                            match span.interior() {
                                                [_name, Token::Long(index), ..] =>
                                                    nurbs::pcurve::explicit_pcurve_cache_from_subtype_ref(ctx,
                                                        *index,
                                                        token_table,
                                                    )
                                                    .map(|pcurve| pcurve.map(|pcurve| (pcurve, true))),
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
                                let decoded = decoded.and_then(|(mut decoded, native_chart)| {
                                    if native_chart {
                                        if let Some(surface) = face
                                            .ref_at(7)
                                            .and_then(|surface| by_index.get(&surface))
                                        {
                                            nurbs::proc_curve::normalize_pcurve_for_surface_record(
                                                surface.head(),
                                                &surface.tokens,
                                                &mut decoded,
                                            )?;
                                        }
                                    }
                                    pcurve_ranges_on_domain(&decoded, edge)
                                        .and_then(|ranges| ranges.into_iter().next())
                                        .map(|range| (decoded, range))
                                });
                                if let Some((decoded, parameter_range)) = decoded {
                                    crate::decode_alloc::insert_hash_map(
                                        ctx,
                                        pcurve_geo,
                                        super::PcurveRecordIndex(pc),
                                        PcurveGeometry::Nurbs { nurbs: decoded },
                                        "ASM topology pcurve_geo",
                                    )?;
                                    crate::decode_alloc::insert_hash_map(
                                        ctx,
                                        pcurve_parameter_ranges,
                                        super::CoedgeRecordIndex(ci),
                                        parameter_range,
                                        "ASM topology pcurve_parameter_ranges",
                                    )?;
                                    crate::decode_alloc::insert_hash_set(
                                        ctx,
                                        kept_pcurves,
                                        pc,
                                        "ASM topology kept_pcurves",
                                    )?;
                                } else {
                                    count_kind(
                                        ctx,
                                        &mut out.stats.undecoded_pcurve_kinds,
                                        prec.head(),
                                    )?;
                                }
                            }
                        } else {
                            count_kind(
                                ctx,
                                &mut out.stats.undecoded_pcurve_kinds,
                                "dangling-reference",
                            )?;
                        }
                    }
                    if let Some(ei) = ce.ref_at(6) {
                        if let Some(edge) = by_index.get(&ei) {
                            // An edge is shared by two coedges; process (and
                            // count its curve loss) only the first time it is
                            // reached so shared edges are not double-counted.
                            if is_edge_record(edge)
                                && crate::decode_alloc::insert_hash_set(
                                    ctx,
                                    kept_edges,
                                    ei,
                                    "ASM topology kept_edges",
                                )?
                            {
                                for slot in [3usize, 5] {
                                    if let Some(vi) = edge.ref_at(slot) {
                                        if let Some(v) = by_index.get(&vi) {
                                            if is_vertex_record(v) {
                                                crate::decode_alloc::insert_hash_set(
                                                    ctx,
                                                    kept_vertices,
                                                    vi,
                                                    "ASM topology kept_vertices",
                                                )?;
                                                if let Some(pi) = vertex_point_ref(v) {
                                                    crate::decode_alloc::insert_hash_set(
                                                        ctx,
                                                        kept_points,
                                                        pi,
                                                        "ASM topology kept_points",
                                                    )?;
                                                }
                                            }
                                        }
                                    }
                                }
                                match edge.ref_at(8) {
                                    Some(cv) if curve_geo.contains_key(&cv) => {
                                        crate::decode_alloc::insert_hash_set(
                                            ctx,
                                            kept_curves,
                                            cv,
                                            "ASM topology kept_curves",
                                        )?;
                                    }
                                    Some(cv) => {
                                        if let Some(crec) = by_index.get(&cv) {
                                            if purpose == DecodePurpose::History {
                                                crate::decode_alloc::insert_hash_map(ctx, curve_geo,
                                                    cv,
                                                    CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }), "ASM topology curve_geo")?;
                                                crate::decode_alloc::insert_hash_set(ctx, kept_curves, cv, "ASM topology kept_curves")?;
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
                                                crate::decode_alloc::insert_hash_map(ctx, curve_geo, cv, CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)), "ASM topology curve_geo")?;
                                                crate::decode_alloc::insert_hash_map(ctx, procedural_curve_defs,
                                                    cv,
                                                    super::ProceduralCurveSource::Cached {
                                                        construction: Box::new(decoded.construction),
                                                        cache_fit_tolerance: decoded.cache_fit_tolerance,
                                                        parsed_domain,
                                                    }, "ASM topology procedural_curve_defs")?;
                                                out.stats.nurbs_curves += 1;
                                                crate::decode_alloc::insert_hash_set(ctx, kept_curves, cv, "ASM topology kept_curves")?;
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
                                                crate::decode_alloc::insert_hash_map(ctx, curve_geo,
                                                    cv,
                                                    CurveGeometry::Procedural {
                                                    construction: brep_id!(
                                                        format,
                                                        ProceduralCurveId,
                                                        "procedural_curve",
                                                        cv
                                                    ),
                                                        cache: None,
                                                    }, "ASM topology curve_geo")?;
                                                crate::decode_alloc::insert_hash_map(ctx, procedural_curve_defs, cv, super::ProceduralCurveSource::Cacheless(Box::new(definition)), "ASM topology procedural_curve_defs")?;
                                                crate::decode_alloc::insert_hash_set(ctx, kept_curves, cv, "ASM topology kept_curves")?;
                                            } else {
                                                crate::decode_alloc::insert_hash_set(ctx, undecoded_carriers, cv, "ASM topology undecoded_carriers")?;

                                                count_kind(
                                                    ctx,
                                                    &mut out.stats.procedural_curve_kinds,
                                                    crec.head(),
                                                )?;
                                            }
                                        } else {
                                            count_kind(
                                                ctx,
                                                &mut out.stats.procedural_curve_kinds,
                                                "dangling-reference",
                                            )?;
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
#[allow(clippy::too_many_arguments)]
pub(super) fn collect_wire_topology(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    out: &mut AsmBrep,
    records: &[Record],
    by_index: &HashMap<i64, &Record>,
    saved_entity_limit: Option<i64>,
    token_table: &nurbs::toks::SubtypeTable,
    carriers: &mut Carriers,
    reach: &mut Reachable,
    purpose: DecodePurpose,
    format: IdFormat,
) -> Result<WireShellTopology, cadmpeg_core::CodecError> {
    let mut wire_edges_by_shell = HashMap::<i64, Vec<i64>>::new();
    let mut free_vertices_by_shell = HashMap::<i64, Vec<i64>>::new();
    let mut saved_free_edges = Vec::new();
    if let Some(limit) = saved_entity_limit {
        for edge in records.iter().filter(|record| {
            let index = record.index as i64;
            (1..limit).contains(&index) && is_edge_record(record)
        }) {
            let edge_index = edge.index as i64;
            let already_owned = reach.edges.contains(&edge_index);
            keep_wire_edge(
                ctx,
                out,
                edge_index,
                by_index,
                token_table,
                carriers,
                reach,
                purpose,
                format,
            )?;
            if !already_owned && reach.edges.contains(&edge_index) {
                crate::decode_alloc::push_vec(
                    ctx,
                    &mut saved_free_edges,
                    edge_index,
                    "ASM saved free edges",
                )?;
            }
        }
    }
    for shell in records.iter().filter(|record| record.head() == "shell") {
        let shell_index = shell.index as i64;
        let mut wire_guard = HashSet::new();
        for root in shell_wire_roots(ctx, shell, by_index)? {
            let mut wire_ref = Some(root);
            while let Some(wire_index) = wire_ref {
                if !crate::decode_alloc::insert_hash_set(
                    ctx,
                    &mut wire_guard,
                    wire_index,
                    "ASM topology wire_guard",
                )? {
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
                        if !crate::decode_alloc::insert_hash_set(
                            ctx,
                            &mut coedge_guard,
                            coedge_index,
                            "ASM topology coedge_guard",
                        )? {
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
                                crate::decode_alloc::push_vec(
                                    ctx,
                                    &mut wire_edges,
                                    edge_index,
                                    "ASM wire edges",
                                )?;
                            }
                            crate::decode_alloc::reserve_hash_map_entry(
                                ctx,
                                &mut wire_edges_by_shell,
                                &shell_index,
                                "ASM wire edges by shell",
                            )?;
                            let edges = wire_edges_by_shell.entry(shell_index).or_default();
                            if !edges.contains(&edge_index) {
                                crate::decode_alloc::push_vec(
                                    ctx,
                                    edges,
                                    edge_index,
                                    "ASM shell wire edges",
                                )?;
                            }
                            keep_wire_edge(
                                ctx,
                                out,
                                edge_index,
                                by_index,
                                token_table,
                                carriers,
                                reach,
                                purpose,
                                format,
                            )?;
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
                    crate::decode_alloc::insert_hash_set(
                        ctx,
                        &mut reach.vertices,
                        vertex,
                        "ASM topology reach.vertices",
                    )?;
                    crate::decode_alloc::reserve_hash_map_entry(
                        ctx,
                        &mut free_vertices_by_shell,
                        &shell_index,
                        "ASM free vertices by shell",
                    )?;
                    let vertices = free_vertices_by_shell.entry(shell_index).or_default();
                    if !vertices.contains(&vertex) {
                        crate::decode_alloc::push_vec(
                            ctx,
                            vertices,
                            vertex,
                            "ASM shell free vertices",
                        )?;
                    }
                    if let Some(point) = by_index
                        .get(&vertex)
                        .and_then(|record| vertex_point_ref(record))
                    {
                        crate::decode_alloc::insert_hash_set(
                            ctx,
                            &mut reach.points,
                            point,
                            "ASM topology reach.points",
                        )?;
                    }
                }
                if let Some(side) = side {
                    crate::decode_alloc::reserve_vec_slot(
                        ctx,
                        &mut out.wire_topologies,
                        "ASM wire topologies",
                    )?;
                    out.wire_topologies.push(WireTopology {
                        source_namespace:
                            crate::brep::records::identity::NativeRecordNamespace::new(format),
                        shell: ShellId::from(id(format, shell_index)),
                        record_index: wire.index as u32,
                        members: match free_vertex {
                            Some(vertex) => WireMembers::Vertex(VertexId::from(id(format, vertex))),
                            None => WireMembers::Edges(
                                wire_edges
                                    .into_iter()
                                    .map(|edge| EdgeId::from(id(format, edge)))
                                    .collect_counted_vec(ctx, "ASM wire member edges")?,
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

#[allow(clippy::too_many_arguments)]
fn keep_wire_edge(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    out: &mut AsmBrep,
    edge_index: i64,
    by_index: &HashMap<i64, &Record>,
    token_table: &nurbs::toks::SubtypeTable,
    carriers: &mut Carriers,
    reach: &mut Reachable,
    purpose: DecodePurpose,
    format: IdFormat,
) -> Result<(), cadmpeg_core::CodecError> {
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
    if !crate::decode_alloc::insert_hash_set(
        ctx,
        kept_edges,
        edge_index,
        "ASM topology kept_edges",
    )? {
        return Ok(());
    }
    for slot in [3usize, 5] {
        if let Some(vertex_index) = edge.ref_at(slot) {
            if let Some(vertex) = by_index
                .get(&vertex_index)
                .filter(|vertex| is_vertex_record(vertex))
            {
                crate::decode_alloc::insert_hash_set(
                    ctx,
                    kept_vertices,
                    vertex_index,
                    "ASM topology kept_vertices",
                )?;
                if let Some(point_index) = vertex_point_ref(vertex) {
                    crate::decode_alloc::insert_hash_set(
                        ctx,
                        kept_points,
                        point_index,
                        "ASM topology kept_points",
                    )?;
                }
            }
        }
    }
    let Some(curve_index) = edge.ref_at(8) else {
        return Ok(());
    };
    if curve_geo.contains_key(&curve_index) {
        crate::decode_alloc::insert_hash_set(
            ctx,
            kept_curves,
            curve_index,
            "ASM topology kept_curves",
        )?;
    } else {
        let Some(curve_record) = by_index.get(&curve_index) else {
            count_kind(
                ctx,
                &mut out.stats.procedural_curve_kinds,
                "dangling-reference",
            )?;
            return Ok(());
        };
        if purpose == DecodePurpose::History {
            crate::decode_alloc::insert_hash_map(
                ctx,
                curve_geo,
                curve_index,
                CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
                "ASM topology curve_geo",
            )?;
            crate::decode_alloc::insert_hash_set(
                ctx,
                kept_curves,
                curve_index,
                "ASM topology kept_curves",
            )?;
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
            crate::decode_alloc::insert_hash_map(
                ctx,
                curve_geo,
                curve_index,
                CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
                "ASM topology curve_geo",
            )?;
            crate::decode_alloc::insert_hash_map(
                ctx,
                procedural_curve_defs,
                curve_index,
                super::ProceduralCurveSource::Cached {
                    construction: Box::new(decoded.construction),
                    cache_fit_tolerance: decoded.cache_fit_tolerance,
                    parsed_domain,
                },
                "ASM topology procedural_curve_defs",
            )?;
            crate::decode_alloc::insert_hash_set(
                ctx,
                kept_curves,
                curve_index,
                "ASM topology kept_curves",
            )?;
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
            crate::decode_alloc::insert_hash_map(
                ctx,
                curve_geo,
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
                "ASM topology curve_geo",
            )?;
            crate::decode_alloc::insert_hash_map(
                ctx,
                procedural_curve_defs,
                curve_index,
                super::ProceduralCurveSource::Cacheless(Box::new(definition)),
                "ASM topology procedural_curve_defs",
            )?;
            crate::decode_alloc::insert_hash_set(
                ctx,
                kept_curves,
                curve_index,
                "ASM topology kept_curves",
            )?;
        } else {
            crate::decode_alloc::insert_hash_set(
                ctx,
                undecoded_carriers,
                curve_index,
                "ASM topology undecoded_carriers",
            )?;

            count_kind(
                ctx,
                &mut out.stats.procedural_curve_kinds,
                curve_record.head(),
            )?;
        }
    }
    Ok(())
}

/// Partition kept edges' curve references by sense so a carrier shared across
/// both senses can emit a `:reversed` clone beside its forward orientation.
pub(super) fn classify_edge_curve_senses(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
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
        if !is_edge_record(r) || !kept_edges.contains(&(r.index as i64)) {
            continue;
        }
        let Some(curve) = r.ref_at(8).filter(|c| kept_curves.contains(c)) else {
            continue;
        };
        match sense_at(r, 9) {
            Sense::Reversed => crate::decode_alloc::insert_hash_set(
                ctx,
                &mut reversed_curve_refs,
                curve,
                "ASM topology reversed_curve_refs",
            )?,
            Sense::Forward => crate::decode_alloc::insert_hash_set(
                ctx,
                &mut forward_curve_refs,
                curve,
                "ASM topology forward_curve_refs",
            )?,
        };
    }
    Ok((reversed_curve_refs, forward_curve_refs))
}

pub(super) fn ring_coedges(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    loop_rec: &Record,
    by_index: &HashMap<i64, &Record>,
    kept: &HashSet<i64>,
    format: IdFormat,
) -> Result<Vec<CoedgeId>, cadmpeg_core::CodecError> {
    let id = |i: i64| CoedgeId::from(super::id(format, i));
    let mut out = Vec::new();
    let Some(first) = loop_rec.ref_at(4) else {
        return Ok(out);
    };
    let mut cur = Some(first);
    let mut guard = HashSet::new();
    while let Some(ci) = cur {
        if !crate::decode_alloc::insert_hash_set(ctx, &mut guard, ci, "ASM topology guard")?
            || !kept.contains(&ci)
        {
            break;
        }
        crate::decode_alloc::push_vec(ctx, &mut out, id(ci), "ASM ring coedges")?;
        let Some(ce) = by_index.get(&ci) else { break };
        cur = ce.ref_at(3);
        if cur == Some(first) {
            break;
        }
    }
    Ok(out)
}

pub(super) fn loop_chain(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    face_rec: &Record,
    by_index: &HashMap<i64, &Record>,
    kept: &HashSet<i64>,
    format: IdFormat,
) -> Result<Vec<LoopId>, cadmpeg_core::CodecError> {
    let id = |i: i64| LoopId::from(super::id(format, i));
    let mut out = Vec::new();
    let mut cur = face_rec.ref_at(4);
    let mut guard = HashSet::new();
    while let Some(li) = cur {
        if !crate::decode_alloc::insert_hash_set(ctx, &mut guard, li, "ASM topology guard")? {
            break;
        }
        if kept.contains(&li) {
            crate::decode_alloc::push_vec(ctx, &mut out, id(li), "ASM face loops")?;
        }
        let Some(lp) = by_index.get(&li) else { break };
        cur = lp.ref_at(3);
    }
    Ok(out)
}

fn face_chain(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    shell_rec: &Record,
    by_index: &HashMap<i64, &Record>,
    kept: &HashSet<i64>,
    format: IdFormat,
) -> Result<Vec<FaceId>, cadmpeg_core::CodecError> {
    let id = |i: i64| FaceId::from(super::id(format, i));
    let mut out = Vec::new();
    let mut cur = shell_rec.ref_at(5);
    let mut guard = HashSet::new();
    while let Some(fi) = cur {
        if !crate::decode_alloc::insert_hash_set(ctx, &mut guard, fi, "ASM topology guard")? {
            break;
        }
        if kept.contains(&fi) {
            crate::decode_alloc::push_vec(ctx, &mut out, id(fi), "ASM shell faces")?;
        }
        let Some(f) = by_index.get(&fi) else { break };
        cur = f.ref_at(3);
    }
    Ok(out)
}

pub(super) fn subshell_ancestor_shells(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    records: &[Record],
    by_index: &HashMap<i64, &Record>,
) -> Result<HashMap<i64, i64>, cadmpeg_core::CodecError> {
    let mut out = HashMap::new();
    for record in records.iter().filter(|record| record.head() == "subshell") {
        let mut owner = record.ref_at(3);
        let mut guard = HashSet::new();
        while let Some(index) = owner {
            if !crate::decode_alloc::insert_hash_set(ctx, &mut guard, index, "ASM topology guard")?
            {
                break;
            }
            let Some(parent) = by_index.get(&index) else {
                break;
            };
            if parent.head() == "shell" {
                crate::decode_alloc::insert_hash_map(
                    ctx,
                    &mut out,
                    record.index as i64,
                    index,
                    "ASM topology out",
                )?;
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    shell: &Record,
    by_index: &HashMap<i64, &Record>,
    kept: &HashSet<i64>,
    format: IdFormat,
) -> Result<Vec<FaceId>, cadmpeg_core::CodecError> {
    let mut out = face_chain(ctx, shell, by_index, kept, format)?;
    let mut pending = shell
        .ref_at(4)
        .into_iter()
        .collect_counted_vec(ctx, "ASM pending subshells")?;
    let mut guard = HashSet::new();
    while let Some(index) = pending.pop() {
        if !crate::decode_alloc::insert_hash_set(ctx, &mut guard, index, "ASM topology guard")? {
            break;
        }
        let Some(record) = by_index
            .get(&index)
            .filter(|record| record.head() == "subshell")
        else {
            break;
        };
        for face in face_chain_from(ctx, record.ref_at(6), by_index, kept, format)? {
            crate::decode_alloc::push_vec(ctx, &mut out, face, "ASM shell faces")?;
        }
        if let Some(next) = record.ref_at(4) {
            crate::decode_alloc::push_vec(ctx, &mut pending, next, "ASM pending subshells")?;
        }
        if let Some(child) = record.ref_at(5) {
            crate::decode_alloc::push_vec(ctx, &mut pending, child, "ASM pending subshells")?;
        }
    }
    Ok(out)
}

pub(super) fn shell_wire_roots(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    shell: &Record,
    by_index: &HashMap<i64, &Record>,
) -> Result<Vec<i64>, cadmpeg_core::CodecError> {
    let mut out = shell
        .ref_at(6)
        .into_iter()
        .collect_counted_vec(ctx, "ASM shell wire roots")?;
    let mut pending = shell
        .ref_at(4)
        .into_iter()
        .collect_counted_vec(ctx, "ASM pending subshells")?;
    let mut guard = HashSet::new();
    while let Some(index) = pending.pop() {
        if !crate::decode_alloc::insert_hash_set(ctx, &mut guard, index, "ASM topology guard")? {
            break;
        }
        let Some(record) = by_index
            .get(&index)
            .filter(|record| record.head() == "subshell")
        else {
            break;
        };
        if let Some(wire) = record.ref_at(7) {
            crate::decode_alloc::push_vec(ctx, &mut out, wire, "ASM shell wire roots")?;
        }
        if let Some(next) = record.ref_at(4) {
            crate::decode_alloc::push_vec(ctx, &mut pending, next, "ASM pending subshells")?;
        }
        if let Some(child) = record.ref_at(5) {
            crate::decode_alloc::push_vec(ctx, &mut pending, child, "ASM pending subshells")?;
        }
    }
    Ok(out)
}

fn face_chain_from(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    mut current: Option<i64>,
    by_index: &HashMap<i64, &Record>,
    kept: &HashSet<i64>,
    format: IdFormat,
) -> Result<Vec<FaceId>, cadmpeg_core::CodecError> {
    let mut out = Vec::new();
    let mut guard = HashSet::new();
    while let Some(index) = current {
        if !crate::decode_alloc::insert_hash_set(ctx, &mut guard, index, "ASM topology guard")? {
            break;
        }
        if kept.contains(&index) {
            crate::decode_alloc::push_vec(
                ctx,
                &mut out,
                FaceId::from(id(format, index)),
                "ASM shell faces",
            )?;
        }
        let Some(face) = by_index.get(&index) else {
            break;
        };
        current = face.ref_at(3);
    }
    Ok(out)
}

pub(super) fn shell_chain(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    region_rec: &Record,
    by_index: &HashMap<i64, &Record>,
    format: IdFormat,
) -> Result<Vec<ShellId>, cadmpeg_core::CodecError> {
    let id = |i: i64| ShellId::from(super::id(format, i));
    let mut out = Vec::new();
    let mut cur = region_rec.ref_at(4);
    let mut guard = HashSet::new();
    while let Some(si) = cur {
        if !crate::decode_alloc::insert_hash_set(ctx, &mut guard, si, "ASM topology guard")? {
            break;
        }
        crate::decode_alloc::push_vec(ctx, &mut out, id(si), "ASM region shells")?;
        let Some(s) = by_index.get(&si) else { break };
        cur = s.ref_at(3);
    }
    Ok(out)
}

pub(super) fn region_chain(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    body_rec: &Record,
    by_index: &HashMap<i64, &Record>,
    format: IdFormat,
) -> Result<Vec<RegionId>, cadmpeg_core::CodecError> {
    let id = |i: i64| RegionId::from(super::id(format, i));
    let mut out = Vec::new();
    let mut cur = body_rec.ref_at(3);
    let mut guard = HashSet::new();
    while let Some(li) = cur {
        if !crate::decode_alloc::insert_hash_set(ctx, &mut guard, li, "ASM topology guard")? {
            break;
        }
        crate::decode_alloc::push_vec(ctx, &mut out, id(li), "ASM body regions")?;
        let Some(l) = by_index.get(&li) else { break };
        cur = l.ref_at(3);
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
