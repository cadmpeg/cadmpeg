// SPDX-License-Identifier: Apache-2.0
//! Resolve procedural surface supports and swept patches.

use super::admit_brep_entity;
use super::emission::annotate_surface_frame;
use super::id_surf;
use super::inverse::looser_tolerance;
use super::Brep;
use super::WalkedFace;
use super::EPS_SWEEP_EXTENT_ABSOLUTE_MM;
use super::EPS_SWEEP_EXTENT_RELATIVE;
use crate::brep::index::view::CarrierView;
use crate::brep::index::IndexedCurve;
use crate::brep::offset::OffsetCarrier;
use crate::brep::sweep;
use crate::brep::sweep::SweepKind;
use crate::brep::topology;
use crate::brep::LEN_TO_MM;
use crate::lane_refusal::scoped::RefusalSink;
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::annotations::AnnotationBuilder;
use cadmpeg_ir::geometry::ProceduralSurface;
use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::geometry::Surface;
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::ids::ProceduralSurfaceId;
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::Exactness;
use std::collections::BTreeMap;
use std::collections::BTreeSet;

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

pub(super) fn resolve_sweep_surface<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    carriers: CarrierView<'_, '_>,
    tables: &topology::Tables,
    face: &WalkedFace,
    refusal: &mut RefusalSink<'_, 'ctx>,
) -> Result<Option<SweepSurfaceResolution>, cadmpeg_core::CodecError> {
    let Some(construction) = carriers.sweep(ctx, face.surface_attr)? else {
        return Ok(None);
    };
    let Some(profile) = carriers.curve(ctx, construction.profile_attr)? else {
        return Ok(None);
    };
    let Some(profile_curve) = sweep::profile_nurbs(
        ctx,
        &profile.carrier().geometry,
        &format_args!(
            "sldprt sweep construction at byte {} for surface attr {}",
            construction.offset, face.surface_attr
        ),
        refusal,
    )?
    else {
        return Ok(None);
    };
    let curve = profile_curve.curve();
    let profile_derived = matches!(profile, IndexedCurve::Derived(_));
    match &construction.kind {
        SweepKind::Spun { base, axis } => Ok(sweep::spun_nurbs(
            ctx,
            curve,
            *base,
            *axis,
            &format_args!(
                "sldprt sweep construction at byte {} for surface attr {}",
                construction.offset, face.surface_attr
            ),
            refusal,
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
                &format_args!(
                    "sldprt sweep construction at byte {} for surface attr {}",
                    construction.offset, face.surface_attr
                ),
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

/// Edge sets in source face order; carrier groups reference those slots.
pub(super) struct FaceEdges {
    pub(super) faces: Vec<BTreeSet<u16>>,
    pub(super) by_carrier: BTreeMap<u16, Vec<usize>>,
}

/// Procedural surface resolution reads these indexes lazily. Analytic faces
/// do not traverse the source faces for procedural support lookup.
pub(super) struct ProceduralSupportIndexes<'source, 'ctx> {
    faces: &'source [WalkedFace],
    loop_set: &'source BTreeSet<u16>,
    tables: &'source topology::Tables,
    emitted: Option<BTreeMap<u16, u16>>,
    face_edges: Option<FaceEdges>,
    storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'source, 'ctx> ProceduralSupportIndexes<'source, 'ctx> {
    pub(super) fn new(
        ctx: &'ctx DecodeContext<'_>,
        faces: &'source [WalkedFace],
        loop_set: &'source BTreeSet<u16>,
        tables: &'source topology::Tables,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        Ok(Self {
            faces,
            loop_set,
            tables,
            emitted: None,
            face_edges: None,
            storage: ctx.reserve_scoped(0, "hold Parasolid procedural support indexes")?,
        })
    }

    fn emitted(
        &mut self,
        ctx: &DecodeContext<'_>,
    ) -> Result<&BTreeMap<u16, u16>, cadmpeg_core::CodecError> {
        let faces = self.faces;
        let loop_set = self.loop_set;
        match self.emitted {
            Some(ref index) => Ok(index),
            None => {
                let index = self.storage.with_storage(|| {
                    let mut emitted_face_surface_by_carrier = BTreeMap::<u16, u16>::new();
                    for face in ctx.admit_iter(faces, "select Parasolid face surface carriers")? {
                        if ctx.any_by(
                            &face.loops,
                            |(loop_attr, _)| {
                                ctx.contains_btree_set(
                                    loop_set,
                                    loop_attr,
                                    "test Parasolid loop_set",
                                )
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
                Ok(self.emitted.insert(index))
            }
        }
    }

    pub(super) fn face_edges(
        &mut self,
        ctx: &DecodeContext<'_>,
    ) -> Result<&FaceEdges, cadmpeg_core::CodecError> {
        let faces = self.faces;
        let t = self.tables;
        match self.face_edges {
            Some(ref index) => Ok(index),
            None => {
                let index = self.storage.with_storage(|| {
                    let mut index = FaceEdges {
                        faces: Vec::new(),
                        by_carrier: BTreeMap::new(),
                    };
                    for face in ctx.admit_iter(faces, "index Parasolid surface face edges")? {
                        let mut edges = BTreeSet::new();
                        for (_, ring) in
                            ctx.admit_iter(&face.loops, "index Parasolid face loop edges")?
                        {
                            for coedge in ctx.admit_iter(ring, "index Parasolid face edges")? {
                                if let Some(edge) = ctx
                                    .get_btree_map(t.coedges(), coedge, "lookup Parasolid coedges")?
                                    .map(|coedge| coedge.refs[6])
                                {
                                    if edge != 0 {
                                        ctx.insert_btree_set(
                                            &mut edges,
                                            edge,
                                            "track Parasolid face edges",
                                        )?;
                                    }
                                }
                            }
                        }
                        let position = index.faces.len();
                        ctx.push_vec(&mut index.faces, edges, "collect Parasolid face edge sets")?;
                        ctx.push_btree_group(
                            &mut index.by_carrier,
                            face.surface_attr,
                            position,
                            "index Parasolid surface face edges",
                            "collect Parasolid surface face edges",
                        )?;
                    }
                    Ok::<_, cadmpeg_core::CodecError>(index)
                })?;
                Ok(self.face_edges.insert(index))
            }
        }
    }
    /// Hidden supports have already published their complete offset chains.
    /// Analytic supports can also reuse the surface of an emitted face.
    pub(super) fn support_is_published(
        &mut self,
        ctx: &DecodeContext<'_>,
        attr: u16,
        carriers: CarrierView<'_, '_>,
        hidden: &BTreeSet<u16>,
    ) -> Result<bool, cadmpeg_core::CodecError> {
        if ctx.contains_btree_set(hidden, &attr, "test published Parasolid blend supports")? {
            return Ok(true);
        }
        if carriers.surface(ctx, attr)?.is_none() {
            return Ok(false);
        }
        ctx.contains_key_btree_map(
            self.emitted(ctx)?,
            &attr,
            "lookup Parasolid emitted_face_surface_by_carrier",
        )
    }

    /// Inspect the selected supports with the acyclicity traversal. No separate
    /// offset-chain scan is needed to decide whether a losing blend publishes.
    pub(super) fn blend_supports_need_emission(
        &mut self,
        ctx: &DecodeContext<'_>,
        supports: [u16; 2],
        carriers: CarrierView<'_, '_>,
        hidden: &BTreeSet<u16>,
    ) -> Result<Option<bool>, cadmpeg_core::CodecError> {
        let mut needs_emission = false;
        for attr in supports {
            if self.support_is_published(ctx, attr, carriers, hidden)? {
                continue;
            }
        let mut visit = |attr| -> Result<(), cadmpeg_core::CodecError> {
            if needs_emission {
                return Ok(());
            }
            if !ctx.contains_key_btree_map(
                self.emitted(ctx)?,
                &attr,
                "lookup Parasolid emitted_face_surface_by_carrier",
            )? && !ctx.contains_btree_set(hidden, &attr, "test emitted Parasolid support surfaces")? {
                needs_emission = true;
            }
            Ok(())
        };
            let (acyclic, storage) = ctx.with_scoped_storage("hold Parasolid surface support walk", || {
                support_is_acyclic_with(ctx, attr, carriers, &mut BTreeSet::new(), &mut visit)
            })?;
            drop(storage);
            if !acyclic {
                return Ok(None);
            }
        }
        Ok(Some(needs_emission))
    }

}

pub(super) struct BrepSink<'ctx, 'arena, 'out> {
    pub(super) ctx: &'ctx DecodeContext<'arena>,
    pub(super) out: &'out mut Brep,
    /// Carrier attrs whose hidden support surface is already in `out`.
    pub(super) hidden_supports: &'out mut BTreeSet<u16>,
    pub(super) hidden_storage: &'out mut cadmpeg_core::decode::ScopedReservation<'ctx>,
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
    carriers: CarrierView<'_, '_>,
) -> Result<bool, cadmpeg_core::CodecError> {
    let (acyclic, storage) = ctx
        .with_scoped_storage("hold Parasolid surface support walk", || {
            support_is_acyclic_with(ctx, attr, carriers, &mut BTreeSet::new(), &mut |_| Ok(()))
        })?;
    drop(storage);
    Ok(acyclic)
}

fn support_is_acyclic_with(
    ctx: &DecodeContext<'_>,
    attr: u16,
    carriers: CarrierView<'_, '_>,
    resolving: &mut BTreeSet<u16>,
    visit: &mut impl FnMut(u16) -> Result<(), cadmpeg_core::CodecError>,
) -> Result<bool, cadmpeg_core::CodecError> {
    let _depth = ctx.enter_nested("check Parasolid surface support")?;
    ctx.charge_work(1, "check Parasolid surface support")?;
    if !ctx.insert_btree_set(resolving, attr, "track Parasolid surface support")? {
        return Ok(false);
    }
    visit(attr)?;
    let acyclic = if carriers.surface(ctx, attr)?.is_some() {
        Ok(true)
    } else if let Some(offset) = carriers.offset(ctx, attr)? {
        support_is_acyclic_with(ctx, offset.support, carriers, resolving, visit)
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
    carriers: CarrierView<'_, '_>,
    indexes: &mut ProceduralSupportIndexes<'_, '_>,
    annotations: &mut AnnotationBuilder,
    source_stream: &cadmpeg_ir::annotations::StreamHandle,
) -> Result<Option<SurfaceId>, cadmpeg_core::CodecError> {
    let mut walk = SurfaceSupportWalk {
        resolving: BTreeSet::new(),
        storage: sink
            .ctx
            .reserve_scoped(0, "hold resolved Parasolid support walk")?,
    };
    ensure_surface_support_with(
        sink,
        attr,
        carriers,
        indexes,
        annotations,
        source_stream,
        &mut walk,
    )
}

struct SurfaceSupportWalk<'ctx> {
    resolving: BTreeSet<u16>,
    storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

fn ensure_surface_support_with(
    sink: &mut BrepSink<'_, '_, '_>,
    attr: u16,
    carriers: CarrierView<'_, '_>,
    indexes: &mut ProceduralSupportIndexes<'_, '_>,
    annotations: &mut AnnotationBuilder,
    source_stream: &cadmpeg_ir::annotations::StreamHandle,
    walk: &mut SurfaceSupportWalk<'_>,
) -> Result<Option<SurfaceId>, cadmpeg_core::CodecError> {
    let _depth = sink.ctx.enter_nested("resolve Parasolid surface support")?;
    sink.ctx
        .charge_work(1, "resolve Parasolid surface support")?;
    if !walk.storage.with_storage(|| {
        sink.ctx.insert_btree_set(
            &mut walk.resolving,
            attr,
            "track resolved Parasolid support",
        )
    })? {
        return Ok(None);
    }
    // A carrier owned by an emitted face names that face's surface. Any other
    // carrier names its hidden support surface, which is emitted once.
    let emitted_bridge = sink
        .ctx
        .get_btree_map(
            indexes.emitted(sink.ctx)?,
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
                sink.hidden_storage.with_storage(|| {
                    sink.ctx.insert_btree_set(
                        sink.hidden_supports,
                        attr,
                        "track emitted Parasolid support surfaces",
                    )
                })?;
            }
            Ok(Some(id))
        } else if let Some(offset) = carriers.offset(sink.ctx, attr)? {
            let Some(support) = ensure_surface_support_with(
                sink,
                offset.support,
                carriers,
                indexes,
                annotations,
                source_stream,
                walk,
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
                sink.hidden_storage.with_storage(|| {
                    sink.ctx.insert_btree_set(
                        sink.hidden_supports,
                        attr,
                        "track emitted Parasolid support surfaces",
                    )
                })?;
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
                sink.hidden_storage.with_storage(|| {
                    sink.ctx.insert_btree_set(
                        sink.hidden_supports,
                        attr,
                        "track emitted Parasolid support surfaces",
                    )
                })?;
            }
            Ok(Some(surface))
        }
    })();
    sink.ctx.remove_btree_set(
        &mut walk.resolving,
        &attr,
        "finish Parasolid surface support",
    )?;
    result
}

#[cfg(test)]
mod tests;
