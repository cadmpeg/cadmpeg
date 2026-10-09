// SPDX-License-Identifier: Apache-2.0
//! Resolve procedural surface supports and swept patches.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_ir::Exactness;
use cadmpeg_ir::annotations::AnnotationBuilder;
use cadmpeg_ir::geometry::ProceduralSurface;
use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::geometry::Surface;
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::ids::ProceduralSurfaceId;
use cadmpeg_ir::ids::SurfaceId;
use crate::brep::LEN_TO_MM;
use crate::brep::index::IndexedCurve;
use crate::brep::index::view::CarrierView;
use crate::brep::offset::OffsetCarrier;
use crate::brep::sweep;
use crate::brep::sweep::SweepKind;
use crate::brep::topology;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use super::Brep;
use super::EPS_SWEEP_EXTENT_ABSOLUTE_MM;
use super::EPS_SWEEP_EXTENT_RELATIVE;
use super::WalkedFace;
use super::admit_brep_entity;
use super::emission::annotate_surface_frame;
use super::id_surf;
use super::inverse::looser_tolerance;

/// Resolve a face whose `refs[4]` carrier is a swept/spun construction to a
/// solved NURBS patch. Returns `(geometry, record offset, annotation tag,
/// derived exactness)`. A spun surface is exact for an exact profile; a swept
/// surface patch is derived because its ruling extent comes from the face's
/// vertex points rather than a stored interval.
pub(super) struct SweepSurfaceResolution {
    pub(super) geometry: SolvedSurfaceGeometry,
    pub(super) offset: usize,
    pub(super) tag: &'static str,
    pub(super) exactness: Option<Exactness>,
}

pub(super) fn resolve_sweep_surface(
    ctx: &DecodeContext<'_>,
    carriers: &CarrierView<'_, '_>,
    tables: &topology::Tables,
    face: &WalkedFace,
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Result<Option<SweepSurfaceResolution>, cadmpeg_core::CodecError> {
    let Some(construction) = carriers.sweep(ctx, face.surface_attr)? else {
        return Ok(None);
    };
    let Some(profile) = carriers.curve(ctx, construction.profile_attr)? else {
        return Ok(None);
    };
    let record = format!(
        "sldprt sweep construction at byte {} for surface attr {}",
        construction.offset, face.surface_attr
    );
    let Some(profile_curve) = sweep::profile_nurbs(ctx, &profile.carrier().geometry, &record, refusal)?
    else {
        return Ok(None);
    };
    let curve = profile_curve.curve();
    let profile_derived = matches!(profile, IndexedCurve::Derived(_));
    match &construction.kind {
        SweepKind::Spun { base, axis } => Ok(sweep::spun_nurbs(
            ctx, curve, *base, *axis, &record, refusal,
        )?
        .map(|surface| SweepSurfaceResolution {
            geometry: SolvedSurfaceGeometry::Nurbs(surface),
            offset: construction.offset,
            tag: "00_44",
            exactness: profile_derived.then_some(Exactness::Derived),
        })),
        SweepKind::Swept { direction } => {
            let unit_direction = *direction;
            let direction = direction.as_raw();
            // Ruling extent: face vertex travel bracketed by the profile poles'
            // own travel along the sweep direction, in millimetres.
            let project = |p: &cadmpeg_ir::math::Point3| {
                p.x * direction.x + p.y * direction.y + p.z * direction.z
            };
            let mut point_lo = f64::INFINITY;
            let mut point_hi = f64::NEG_INFINITY;
            for (_, ring) in
                ctx.admit_iter(&face.loops, "scan SLDPRT resolve_sweep_surface values")?
            {
                for ce_attr in ctx.admit_iter(ring, "scan SLDPRT ring values")? {
                    let Some(vuse) = ctx
                        .get_btree_map(tables.coedges(), ce_attr, "lookup Parasolid coedges")?
                        .map(|ce| ce.refs[4])
                    else {
                        continue;
                    };
                    let Some(vertex) = ctx.get_btree_map(
                        tables.vertex_uses(),
                        &vuse,
                        "lookup Parasolid vertex uses",
                    )?
                    else {
                        continue;
                    };
                    let Some(point) = ctx.get_btree_map(
                        tables.points(),
                        &vertex.refs[4],
                        "lookup Parasolid points",
                    )?
                    else {
                        continue;
                    };
                    let coordinates = point.xyz_m;
                    let travel = coordinates[0] * LEN_TO_MM * direction.x
                        + coordinates[1] * LEN_TO_MM * direction.y
                        + coordinates[2] * LEN_TO_MM * direction.z;
                    point_lo = point_lo.min(travel);
                    point_hi = point_hi.max(travel);
                }
            }
            if point_lo > point_hi {
                return Ok(None);
            }
            let mut pole_lo = f64::INFINITY;
            let mut pole_hi = f64::NEG_INFINITY;
            for index in
                ctx.admit_iter(0..curve.pole_count(), "scan Parasolid sweep profile poles")?
            {
                let point = match curve.pole_rows() {
                    cadmpeg_ir::geometry::nurbs::NurbsPoles3::Polynomial { points } => {
                        points[index].get()
                    }
                    cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational { points } => {
                        points[index].point.get()
                    }
                };
                let travel = project(&point);
                pole_lo = pole_lo.min(travel);
                pole_hi = pole_hi.max(travel);
            }
            let v_start = point_lo - pole_hi;
            let v_end = point_hi - pole_lo;
            // Two independently derived pads on the same swept extent: an
            // absolute one in millimetres and one relative to the travel the
            // poles and points state. The extent must clear both, so the
            // looser is the pad.
            let pad = looser_tolerance(
                EPS_SWEEP_EXTENT_ABSOLUTE_MM,
                (v_end - v_start) * EPS_SWEEP_EXTENT_RELATIVE,
            );
            Ok(sweep::swept_nurbs(
                ctx,
                curve,
                unit_direction,
                v_start - pad,
                v_end + pad,
                &record,
                refusal,
            )?
            .map(|surface| SweepSurfaceResolution {
                geometry: SolvedSurfaceGeometry::Nurbs(surface),
                offset: construction.offset,
                tag: "00_43",
                exactness: Some(Exactness::Derived),
            }))
        }
    }
}

fn id_hidden_support_surface(attr: u16) -> SurfaceId {
    SurfaceId::compose(
        &cadmpeg_ir::identity_namespace!("sldprt", "brep", "hidden-support-surf"),
        attr,
    )
}

fn id_offset_construction(attr: u16) -> ProceduralSurfaceId {
    ProceduralSurfaceId::compose(
        &cadmpeg_ir::identity_namespace!("sldprt", "brep", "offset-support-construction"),
        attr,
    )
}

pub(super) struct BrepSink<'ctx, 'arena, 'out> {
    pub(super) ctx: &'ctx DecodeContext<'arena>,
    pub(super) out: &'out mut Brep,
    /// Carrier attrs whose hidden support surface is already in `out`.
    pub(super) hidden_supports: &'out mut BTreeSet<u16>,
}

pub(super) fn emit_offset_surface(
    sink: &mut BrepSink<'_, '_, '_>,
    annotations: &mut AnnotationBuilder,
    source_stream: &cadmpeg_ir::annotations::StreamHandle,
    surface: SurfaceId,
    construction: ProceduralSurfaceId,
    support: SurfaceId,
    offset: &OffsetCarrier,
) -> Result<(), cadmpeg_core::CodecError> {
    annotations.note(
        sink.ctx,
        surface.as_str(),
        source_stream,
        u64_from_index(offset.offset),
        Some("00_3c"),
    )?;
    let payload = cadmpeg_ir::geometry::surface_payloads::OffsetSurfaceConstruction::legacy(
        support,
        offset.distance,
        None,
        None,
        false,
        cadmpeg_ir::geometry::LegacyExtensionFlags::Absent {},
        None,
    );
    let procedural = ProceduralSurface::new(
        construction
            .try_clone_for_decode(sink.ctx, "copy Parasolid offset construction identity")?,
        ProceduralSurfaceDefinition::Offset(payload),
        None,
    );
    admit_brep_entity(sink.ctx)?;
    sink.ctx.reserve_vec(
        &mut sink.out.procedural_surfaces,
        1,
        "collect Parasolid offset constructions",
    )?;
    sink.out.procedural_surfaces.push(procedural);
    let geometry = SurfaceGeometry::Procedural {
        construction,
        cache: None,
    };
    admit_brep_entity(sink.ctx)?;
    sink.ctx.reserve_vec(
        &mut sink.out.surfaces,
        1,
        "collect Parasolid offset surfaces",
    )?;
    sink.out.surfaces.push(Surface {
        id: surface,
        source_object: None,
        geometry,
    });
    Ok(())
}

/// Whether one referenced support can be emitted without a procedural cycle.
///
/// An untyped reference is still a valid opaque support surface. Offset
/// carriers recurse because a cycle cannot define a surface.
pub(super) fn support_is_acyclic(
    ctx: &DecodeContext<'_>,
    attr: u16,
    carriers: &CarrierView<'_, '_>,
    resolving: &mut BTreeSet<u16>,
) -> Result<bool, cadmpeg_core::CodecError> {
    let _depth = ctx.enter_nested("check Parasolid surface support")?;
    ctx.charge_work(1, "check Parasolid surface support")?;
    if !ctx.insert_btree_set(resolving, attr, "track Parasolid surface support")? {
        return Ok(false);
    }
    let acyclic = if carriers.surface(ctx, attr)?.is_some() {
        Ok(true)
    } else if let Some(offset) = carriers.offset(ctx, attr)? {
        support_is_acyclic(ctx, offset.support, carriers, resolving)
    } else {
        Ok(true)
    };
    ctx.remove_btree_set(resolving, &attr, "finish Parasolid surface support")?;
    acyclic
}

/// Resolve one carrier as a support of a procedural construction. A carrier
/// owned by an emitted face reuses that face's surface identity. Otherwise the
/// decoder emits a hidden analytic, NURBS, recursive offset, or opaque support
/// surface. Offset cycles invalidate the complete construction.
pub(super) fn ensure_surface_support(
    sink: &mut BrepSink<'_, '_, '_>,
    attr: u16,
    carriers: &CarrierView<'_, '_>,
    emitted_face_surface_by_carrier: &BTreeMap<u16, u16>,
    annotations: &mut AnnotationBuilder,
    source_stream: &cadmpeg_ir::annotations::StreamHandle,
    resolving: &mut BTreeSet<u16>,
) -> Result<Option<SurfaceId>, cadmpeg_core::CodecError> {
    let _depth = sink.ctx.enter_nested("resolve Parasolid surface support")?;
    sink.ctx
        .charge_work(1, "resolve Parasolid surface support")?;
    if !sink
        .ctx
        .insert_btree_set(resolving, attr, "track resolved Parasolid support")?
    {
        return Ok(None);
    }
    // A carrier owned by an emitted face names that face's surface. Any other
    // carrier names its hidden support surface, which is emitted once.
    let emitted_bridge = sink
        .ctx
        .get_btree_map(
            emitted_face_surface_by_carrier,
            &attr,
            "lookup Parasolid emitted_face_surface_by_carrier",
        )?
        .copied();
    let surface_id = || emitted_bridge.map_or_else(|| id_hidden_support_surface(attr), id_surf);
    let result = (|| -> Result<Option<SurfaceId>, cadmpeg_core::CodecError> {
        let emit_hidden = emitted_bridge.is_none()
            && !sink.ctx.contains_btree_set(
                sink.hidden_supports,
                &attr,
                "test emitted Parasolid support surfaces",
            )?;
        if let Some(carrier) = carriers.surface(sink.ctx, attr)? {
            let id = surface_id();
            if emit_hidden {
                let geometry = carrier
                    .geometry
                    .try_clone_for_decode(sink.ctx, "copy Parasolid surface geometry")?;
                if let SurfaceGeometry::Solved(solved) = &geometry {
                    match annotate_surface_frame(sink.ctx, annotations, id.as_str(), solved) {
                        Ok(()) => {}
                        Err(error @ cadmpeg_core::CodecError::ResourceLimit(_)) => {
                            return Err(error)
                        }
                        Err(_) => return Ok(None),
                    }
                }
                annotations.note(
                    sink.ctx,
                    id.as_str(),
                    source_stream,
                    u64_from_index(carrier.offset),
                    Some("procedural_support"),
                )?;
                admit_brep_entity(sink.ctx)?;
                sink.ctx.reserve_vec(
                    &mut sink.out.surfaces,
                    1,
                    "collect Parasolid support surfaces",
                )?;
                sink.out.surfaces.push(Surface {
                    id: id.try_clone_for_decode(sink.ctx, "SLDPRT decoded identity copy")?,
                    source_object: None,
                    geometry,
                });
                sink.ctx.insert_btree_set(
                    sink.hidden_supports,
                    attr,
                    "track emitted Parasolid support surfaces",
                )?;
            }
            Ok(Some(id))
        } else if let Some(offset) = carriers.offset(sink.ctx, attr)? {
            let Some(support) = ensure_surface_support(
                sink,
                offset.support,
                carriers,
                emitted_face_surface_by_carrier,
                annotations,
                source_stream,
                resolving,
            )?
            else {
                return Ok(None);
            };
            let surface = surface_id();
            if emit_hidden {
                let construction = id_offset_construction(attr);
                emit_offset_surface(
                    sink,
                    annotations,
                    source_stream,
                    surface.try_clone_for_decode(
                        sink.ctx,
                        "copy Parasolid support surface identity",
                    )?,
                    construction,
                    support,
                    offset,
                )?;
                sink.ctx.insert_btree_set(
                    sink.hidden_supports,
                    attr,
                    "track emitted Parasolid support surfaces",
                )?;
            }
            Ok(Some(surface))
        } else {
            let surface = surface_id();
            if emit_hidden {
                annotations.exactness(sink.ctx, surface.as_str(), Exactness::Unknown)?;
                sink.out.stats.unknown_procedural_supports += 1;
                admit_brep_entity(sink.ctx)?;
                sink.ctx.reserve_vec(
                    &mut sink.out.surfaces,
                    1,
                    "collect Parasolid opaque supports",
                )?;
                sink.out.surfaces.push(Surface {
                    id: surface
                        .try_clone_for_decode(sink.ctx, "copy Parasolid opaque surface identity")?,
                    source_object: None,
                    geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                        record: None,
                    }),
                });
                sink.ctx.insert_btree_set(
                    sink.hidden_supports,
                    attr,
                    "track emitted Parasolid support surfaces",
                )?;
            }
            Ok(Some(surface))
        }
    })();
    sink.ctx
        .remove_btree_set(resolving, &attr, "finish Parasolid surface support")?;
    result
}
