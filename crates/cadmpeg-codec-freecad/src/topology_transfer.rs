// SPDX-License-Identifier: Apache-2.0
//! Occurrence-aware transfer of exact-shape topology into neutral CADIR.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{FinitePoint3, FiniteVector3};
use cadmpeg_ir::geometry::nurbs::NurbsError;
use cadmpeg_ir::geometry::pcurve::{PcurveMetadata, PcurveNurbsPoles, WeightedPole2};
use cadmpeg_ir::geometry::{
    pcurve::{Pcurve, PcurveGeometry, PcurveNurbs},
    sampled::{PolygonalSurface, PolylineCurve, PolylineSamples, PolylineVertex},
    Curve, CurveGeometry, ProceduralSurface, ProceduralSurfaceDefinition, SolvedCurveGeometry,
    SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::hash::{sha256, LowerHex};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, ProceduralSurfaceId,
    RegionId, ShellId, SurfaceId, VertexId,
};
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::scalar::PositiveReal;
use cadmpeg_ir::scalar::{FiniteReal, NonZeroReal};
use cadmpeg_ir::tessellation::Tessellation;
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::transform::{Transform, Transform2};
use cadmpeg_ir::units::NonzeroPoint2;
use cadmpeg_ir::units::UnitVector3;
use cadmpeg_ir::SourceObjectAssociation;

use crate::brep::{
    location_transform_error, surface_parameter_affine, ShapePayloadRecord, SurfaceParameterAffine,
    Tables, TextCurve2d, TextEdgeRepresentation, TextOrientation, TextShapeKind, TextShapeUse,
    TextSurface, TextTShape, TextTShapeGeometry,
};
use crate::loss::FreecadLossCode;
use crate::native::element_map::ScopedData;
use crate::native::PropertyRecord;
use cadmpeg_ir::report::loss::LossNote;

const EPS_TOPOLOGY_TRANSFER_GEOMETRY: f64 = 1.0e-9;
const EPS_TOPOLOGY_TRANSFER_DEGENERATE: f64 = 1.0e-10;
const EPS_TOPOLOGY_TRANSFER_EXACT_GEOMETRY: f64 = 1.0e-12;

struct IndexedPolygon {
    samples: PolylineSamples<FiniteReal, FinitePoint3>,
    deflection: cadmpeg_ir::scalar::NonNegativeReal,
}

impl IndexedPolygon {
    /// Pair the polygon's two native lanes into sample rows.
    ///
    /// `FreeCAD` states nodes and parameters as two properties, so the codec
    /// pairs them here and refuses a polygon whose lanes disagree. The IR
    /// carries the rows only.
    fn try_new(
        ctx: &DecodeContext<'_>,
        nodes: Vec<FinitePoint3>,
        parameters: Option<Vec<FiniteReal>>,
        deflection: cadmpeg_ir::scalar::NonNegativeReal,
    ) -> Result<Self, CodecError> {
        let samples = match parameters {
            None => PolylineSamples::Unparameterized {
                points: nodes.try_into().map_err(|error| {
                    CodecError::malformed(format_args!("polygon states no node: {error}"))
                })?,
            },
            Some(parameters) => {
                if parameters.len() != nodes.len() {
                    return Err(CodecError::Malformed(
                        "polygon parameters length must equal nodes length".into(),
                    ));
                }
                let mut vertices =
                    ctx.collection_vec(nodes.len(), "FreeCAD indexed polygon vertices")?;
                for (point, parameter) in ctx
                    .admit_iter(nodes, "FreeCAD indexed polygon pairing")?
                    .zip(parameters)
                {
                    vertices.push(PolylineVertex { parameter, point });
                }
                PolylineSamples::Parameterized {
                    vertices: vertices.try_into().map_err(|error| {
                        CodecError::malformed(format_args!("polygon states no node: {error}"))
                    })?,
                }
            }
        };
        Ok(Self {
            samples,
            deflection,
        })
    }
}
type FacePcurve = (PcurveId, Option<[FiniteReal; 2]>);

// Native availability precedes surface placement. Loss indexes remain valid
// because the builder only appends losses before it transfers them.
#[derive(Clone, Copy)]
enum PcurveAvailability {
    Available(Option<[f64; 2]>),
    Unavailable { loss_index: usize },
}

pub(crate) struct TopologyOccurrence {
    pub(crate) property: String,
    pub(crate) indexed_name: &'static str,
    pub(crate) source_index: usize,
    pub(crate) topology_id: String,
}

/// Temporary topology bindings held until the element-map consumer finishes.
pub(crate) struct TopologyOccurrences<'ctx> {
    pub(crate) records: Vec<TopologyOccurrence>,
    _storage: ScopedReservation<'ctx>,
}

struct ScopedVec<'ctx, T> {
    values: Vec<T>,
    _storage: ScopedReservation<'ctx>,
}

/// Transfer text or binary shape-set topology with placements applied once.
pub(crate) fn transfer<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    ir: &mut CadIr,
    payloads: &[ShapePayloadRecord],
    properties: &[PropertyRecord],
    losses: &mut Vec<LossNote>,
    bind_element_maps: bool,
) -> Result<TopologyOccurrences<'ctx>, CodecError> {
    let mut payloads = payloads.iter();
    let first = loop {
        if payloads.len() == 0 {
            break None;
        }
        let Some(payload) = ctx.next_charged(
            &mut payloads,
            "FreeCAD topology payloads",
        )? else {
            break None;
        };
        if let Some(tables) = Tables::from_payload(payload) {
            break Some((payload, tables));
        }
    };
    let mut occurrences = TopologyOccurrences {
        records: Vec::new(),
        _storage: ctx.reserve_scoped(0, "FreeCAD topology occurrences")?,
    };
    let mut pcurves = ScopedVec {
        values: Vec::new(),
        _storage: ctx.reserve_scoped(0, "FreeCAD pcurve candidates")?,
    };
    if let Some(first) = first {
        let (data, storage) = ctx.collect_scoped_btree_map(
            properties
                .iter()
                .rev()
                .map(|property| (property.id.as_str(), property.owner.as_str())),
            "FreeCAD topology property owners",
        )?;
        let owners = ScopedData {
            data,
            _storage: storage,
        };
        let mut geometry = GeometryIndexes::new(ctx)?;
        let mut next_payload = Some(first);
        loop {
            let current = if let Some(first) = next_payload.take() {
                Some(first)
            } else {
                loop {
                    if payloads.len() == 0 {
                        break None;
                    }
                    let Some(payload) =
                        ctx.next_charged(&mut payloads, "FreeCAD topology payloads")?
                    else {
                        break None;
                    };
                    if let Some(tables) = Tables::from_payload(payload) {
                        break Some((payload, tables));
                    }
                }
            };
            let Some((payload, tables)) = current else {
                break;
            };
            let source_object = ctx
                .get_btree_map(
                    &owners.data,
                    payload.property.as_str(),
                    "FreeCAD topology property owner lookup",
                )?
                .copied()
                .unwrap_or(payload.property.as_str());
            let (data, storage) =
                ctx.with_scoped_storage("FreeCAD topology source scratch", || {
                    let source_object =
                        ctx.copy_retained_text(source_object, "FreeCAD topology source object")?;
                    cadmpeg_core::text::NonBlankString::for_decode(
                        ctx,
                        source_object,
                        "validate nonblank text",
                    )?
                    .ok_or_else(|| CodecError::malformed("source object_id must not be empty"))
                })?;
            let source_object = ScopedData {
                data,
                _storage: storage,
            };
            let mut builder = Builder::new(
                ctx,
                payload,
                tables,
                source_object,
                geometry,
                bind_element_maps.then_some(&mut occurrences),
            )?;
            builder.emit_pcurves()?;
            let (values, storage) =
                ctx.with_scoped_storage("FreeCAD topology body roots", || builder.body_roots())?;
            let roots = ScopedVec {
                values,
                _storage: storage,
            };
            let mut root_iter = roots.values.iter().copied();
            while root_iter.len() != 0 {
                let Some(root) = ctx.next_charged(
                    &mut root_iter,
                    "FreeCAD topology body root scan",
                )? else {
                    break;
                };
                builder.append_body(ctx, ir, root)?;
            }
            builder.emit_unowned_triangulations(ir)?;
            ctx.extend_vec(losses, builder.losses, "FreeCAD topology losses")?;
            pcurves._storage.with_storage(|| {
                ctx.extend_vec(&mut pcurves.values, builder.pcurves, "FreeCAD pcurve candidates")
            })?;
            geometry = builder.geometry;
        }
    }
    close_radial_rings(ctx, &mut ir.model.coedges)?;
    let (data, storage) = ctx
        .with_scoped_storage("FreeCAD referenced pcurves", || {
            referenced_pcurve_ids(ctx, &ir.model.coedges)
        })?;
    let referenced_pcurves = ScopedData {
        data,
        _storage: storage,
    };
    ctx.retain_vec(
        &mut ir.model.pcurves,
        |pcurve| {
            ctx.contains_hash_set(
                &referenced_pcurves.data,
                &pcurve.id,
                "FreeCAD referenced pcurve lookup",
            )
        },
        "FreeCAD pcurve retention",
    )?;
    let mut candidates = std::mem::take(&mut pcurves.values).into_iter();
    while candidates.len() != 0 {
        let Some(candidate) = ctx.next_charged(
            &mut candidates,
            "FreeCAD pcurve candidate scan",
        )? else {
            break;
        };
        if ctx.contains_hash_set(
            &referenced_pcurves.data,
            &candidate.0.id,
            "FreeCAD referenced pcurve lookup",
        )? {
            let (pcurve, storage) = candidate;
            storage.commit()?;
            ctx.push_vec(&mut ir.model.pcurves, pcurve, "FreeCAD pcurves records")?;
        }
    }
    Ok(occurrences)
}

fn referenced_pcurve_ids<'a>(
    ctx: &DecodeContext<'_>,
    coedges: &'a [Coedge],
) -> Result<HashSet<&'a PcurveId>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut referenced = HashSet::new();
    let mut coedges = coedges.iter();
    while coedges.len() != 0 {
        let Some(coedge) = ctx.next_charged(
            &mut coedges,
            "FreeCAD referenced coedge scan",
        )? else {
            break;
        };
        let mut pcurves = coedge.pcurves.iter();
        while pcurves.len() != 0 {
            let Some(pcurve) = ctx.next_charged(
                &mut pcurves,
                "FreeCAD referenced pcurve scan",
            )? else {
                break;
            };
            ctx.insert_hash_set(
                &mut referenced,
                &pcurve.pcurve,
                "FreeCAD referenced pcurves",
            )?;
        }
    }
    Ok(referenced)
}

impl Tables<'_> {
    fn location(
        &self,
        index: impl Into<crate::brep::LocationRef>,
    ) -> Result<Transform, CodecError> {
        index.into().resolve(self.locations)
    }
}

#[derive(Clone, Copy)]
struct BodyRoot {
    shape: usize,
    transform: Transform,
    reversed: bool,
    root_ordinal: Option<usize>,
}

#[derive(Clone, Copy)]
struct RegionTraversal {
    shape_index: usize,
    transform: Transform,
    reversed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct OccurrenceKey(String);

impl cadmpeg_core::decode::cost::DecodeCost for OccurrenceKey {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(&self.0, ctx, operation)
    }
}

impl OccurrenceKey {
    fn new(
        ctx: &DecodeContext<'_>,
        shape: usize,
        transform: Transform,
    ) -> Result<Self, CodecError> {
        let label = if is_identity(transform) {
            ctx.format_retained(format_args!("{shape}"), "FreeCAD occurrence key")?
        } else {
            ctx.format_retained(
                format_args!("{}@{}", shape, LowerHex(&transform_digest(transform))),
                "FreeCAD occurrence key",
            )?
        };
        Ok(Self(label))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SourceOccurrenceKey(String);

impl cadmpeg_core::decode::cost::DecodeCost for SourceOccurrenceKey {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(&self.0, ctx, operation)
    }
}

impl SourceOccurrenceKey {
    fn new(
        ctx: &DecodeContext<'_>,
        shape: usize,
        transform: Transform,
    ) -> Result<Self, CodecError> {
        Ok(Self(ctx.format_retained(
            format_args!("{}@{}", shape, LowerHex(&transform_digest(transform))),
            "FreeCAD source occurrence key",
        )?))
    }
}

struct GeometryIndexes<'c> {
    curves: Option<BTreeMap<CurveId, usize>>,
    surfaces: Option<BTreeMap<SurfaceId, usize>>,
    procedural: Option<ProceduralIndexes<'c>>,
    storage: ScopedReservation<'c>,
}

struct ProceduralIndexes<'c> {
    procedural_surfaces: BTreeSet<ProceduralSurfaceId>,
    construction_owners: BTreeMap<ProceduralSurfaceId, Option<usize>>,
    storage: ScopedReservation<'c>,
}

impl<'c> GeometryIndexes<'c> {
    fn new(ctx: &'c DecodeContext<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            curves: None,
            surfaces: None,
            procedural: None,
            storage: ctx.reserve_scoped(0, "FreeCAD geometry indexes")?,
        })
    }

    fn curve_position(
        &mut self,
        ctx: &'c DecodeContext<'_>,
        ir: &CadIr,
        id: &CurveId,
    ) -> Result<Option<usize>, CodecError> {
        if self.curves.is_none() {
            self.curves = Some(BTreeMap::new());
            let mut curves = ir.model.curves.iter().enumerate();
            while curves.len() != 0 {
                let Some((position, curve)) = ctx.next_charged(
                    &mut curves,
                    "FreeCAD curve index scan",
                )? else {
                    break;
                };
                self.index_curve(ctx, &curve.id, position)?;
            }
        }
        let positions = self.curves.as_ref().ok_or_else(|| {
            CodecError::Malformed("FreeCAD curve index was not initialized".into())
        })?;
        ctx.get_btree_map(positions, id, "FreeCAD curve position lookup")
            .map(|position| position.copied())
    }

    fn surface_position(
        &mut self,
        ctx: &'c DecodeContext<'_>,
        ir: &CadIr,
        id: &SurfaceId,
    ) -> Result<Option<usize>, CodecError> {
        if self.surfaces.is_none() {
            self.surfaces = Some(BTreeMap::new());
            let mut surfaces = ir.model.surfaces.iter().enumerate();
            while surfaces.len() != 0 {
                let Some((position, surface)) = ctx.next_charged(
                    &mut surfaces,
                    "FreeCAD surface index scan",
                )? else {
                    break;
                };
                self.index_surface(ctx, &surface.id, position)?;
            }
        }
        let positions = self.surfaces.as_ref().ok_or_else(|| {
            CodecError::Malformed("FreeCAD surface index was not initialized".into())
        })?;
        ctx.get_btree_map(positions, id, "FreeCAD surface position lookup")
            .map(|position| position.copied())
    }

    fn ensure_procedural(
        &mut self,
        ctx: &'c DecodeContext<'_>,
        ir: &CadIr,
    ) -> Result<(), CodecError> {
        if let Some(refusal) = ctx.resource_refusal() {
            return Err(refusal.into());
        }
        if self.procedural.is_some() {
            return Ok(());
        }
        let mut indexes = ProceduralIndexes {
            procedural_surfaces: BTreeSet::new(),
            construction_owners: BTreeMap::new(),
            storage: ctx.reserve_scoped(0, "FreeCAD procedural indexes")?,
        };
        let mut surfaces = ir.model.surfaces.iter().enumerate();
        while surfaces.len() != 0 {
            let Some((position, surface)) = ctx.next_charged(
                &mut surfaces,
                "FreeCAD procedural owner scan",
            )? else {
                break;
            };
            if let Some(construction) = surface.geometry.procedural_construction() {
                if let Some(owner) = ctx.get_mut_btree_map(
                    &mut indexes.construction_owners,
                    construction,
                    "FreeCAD procedural owner lookup",
                )? {
                    *owner = None;
                } else {
                    indexes.storage.with_storage(|| {
                        ctx.insert_btree_map(
                            &mut indexes.construction_owners,
                            construction
                                .try_clone_for_decode(ctx, "FreeCAD procedural owner key")?,
                            Some(position),
                            "FreeCAD procedural owners",
                        )
                    })?;
                }
            }
        }
        let mut surfaces = ir.model.procedural_surfaces.iter();
        while surfaces.len() != 0 {
            let Some(surface) = ctx.next_charged(
                &mut surfaces,
                "FreeCAD procedural surface index scan",
            )? else {
                break;
            };
            indexes.storage.with_storage(|| {
                ctx.insert_btree_set(
                    &mut indexes.procedural_surfaces,
                    surface
                        .id
                        .try_clone_for_decode(ctx, "FreeCAD procedural surface key")?,
                    "FreeCAD procedural surface index",
                )
            })?;
        }
        self.procedural = Some(indexes);
        Ok(())
    }

    fn index_curve(
        &mut self,
        ctx: &DecodeContext<'_>,
        id: &CurveId,
        position: usize,
    ) -> Result<(), CodecError> {
        let Some(curves) = &mut self.curves else {
            return Ok(());
        };
        if !ctx.contains_key_btree_map(curves, id, "FreeCAD curve position lookup")? {
            self.storage.with_storage(|| {
                ctx.insert_btree_map(
                    curves,
                    id.try_clone_for_decode(ctx, "FreeCAD curve position key")?,
                    position,
                    "FreeCAD curve positions",
                )
            })?;
        }
        Ok(())
    }

    fn index_surface(
        &mut self,
        ctx: &DecodeContext<'_>,
        id: &SurfaceId,
        position: usize,
    ) -> Result<(), CodecError> {
        let Some(surfaces) = &mut self.surfaces else {
            return Ok(());
        };
        if !ctx.contains_key_btree_map(surfaces, id, "FreeCAD surface position lookup")? {
            self.storage.with_storage(|| {
                ctx.insert_btree_map(
                    surfaces,
                    id.try_clone_for_decode(ctx, "FreeCAD surface position key")?,
                    position,
                    "FreeCAD surface positions",
                )
            })?;
        }
        Ok(())
    }
}

struct Builder<'a, 'c, 'r, 'occ> {
    ctx: &'c DecodeContext<'r>,
    payload: &'a ShapePayloadRecord,
    tables: Tables<'a>,
    geometry: GeometryIndexes<'c>,
    vertices: HashMap<OccurrenceKey, VertexId>,
    edges: HashMap<OccurrenceKey, EdgeId>,
    emitted_curves: HashSet<CurveId>,
    emitted_surfaces: HashSet<SurfaceId>,
    emitted_triangulations: HashSet<usize>,
    body_scope: Transform,
    root_discriminator: Option<usize>,
    current_body: Option<BodyId>,
    source_object: ScopedData<'c, cadmpeg_core::text::NonBlankString>,
    source_indices: Option<ScopedData<'c, HashMap<(TextShapeKind, SourceOccurrenceKey), usize>>>,
    pcurve_availability: BTreeMap<usize, PcurveAvailability>,
    pcurves: Vec<(Pcurve, ScopedReservation<'c>)>,
    occurrences: Option<&'occ mut TopologyOccurrences<'c>>,
    losses: Vec<LossNote>,
    storage: ScopedReservation<'c>,
    cache_storage: ScopedReservation<'c>,
}

impl<'a, 'c, 'r, 'occ> Builder<'a, 'c, 'r, 'occ> {
    fn new(
        ctx: &'c DecodeContext<'r>,
        payload: &'a ShapePayloadRecord,
        tables: Tables<'a>,
        source_object: ScopedData<'c, cadmpeg_core::text::NonBlankString>,
        geometry: GeometryIndexes<'c>,
        occurrences: Option<&'occ mut TopologyOccurrences<'c>>,
    ) -> Result<Self, CodecError> {
        let source_indices = if occurrences.is_some() {
            let (data, storage) = ctx.with_scoped_storage("FreeCAD topology scratch", || {
                source_topology_indices(ctx, tables)
            })?;
            Some(ScopedData {
                data,
                _storage: storage,
            })
        } else {
            None
        };
        Ok(Self {
            ctx,
            payload,
            tables,
            geometry,
            vertices: HashMap::new(),
            edges: HashMap::new(),
            emitted_curves: HashSet::new(),
            emitted_surfaces: HashSet::new(),
            emitted_triangulations: HashSet::new(),
            body_scope: Transform::identity(),
            root_discriminator: None,
            current_body: None,
            source_object,
            source_indices,
            pcurve_availability: BTreeMap::new(),
            pcurves: Vec::new(),
            occurrences,
            losses: Vec::new(),
            storage: ctx.reserve_scoped(0, "FreeCAD topology emitted indexes")?,
            cache_storage: ctx.reserve_scoped(0, "FreeCAD topology occurrence caches")?,
        })
    }

    fn source_association(&self) -> Result<SourceObjectAssociation, CodecError> {
        Ok(SourceObjectAssociation {
            format: cadmpeg_ir::CodecFormat::Fcstd,
            object_id: cadmpeg_core::text::NonBlankString::for_decode(
                self.ctx,
                self.ctx.copy_retained_text(
                    self.source_object.data.as_str(),
                    "FreeCAD topology source association",
                )?,
                "validate nonblank text",
            )?
            .ok_or_else(|| CodecError::malformed("source object_id must not be empty"))?,
            name: None,
            color: None,
            visible: None,
            layer: None,
            instance_path: Vec::new(),
        })
    }

    fn bind_topology(
        &mut self,
        kind: TextShapeKind,
        shape: usize,
        local: Transform,
        topology_id: &str,
    ) -> Result<(), CodecError> {
        let (Some(source_indices), Some(occurrences)) =
            (self.source_indices.as_ref(), self.occurrences.as_deref_mut())
        else {
            return Ok(());
        };
        let (data, storage) =
            self.ctx
                .with_scoped_storage("FreeCAD occurrence lookup scratch", || {
                    SourceOccurrenceKey::new(
                        self.ctx,
                        shape,
                        self.body_scope
                            .compose(local)
                            .map_err(location_transform_error)?,
                    )
                })?;
        let key = ScopedData {
            data,
            _storage: storage,
        };
        let Some(source_index) = self
            .ctx
            .get_hash_map(
                &source_indices.data,
                &(kind, key.data),
                "FreeCAD source topology lookup",
            )?
            .copied()
        else {
            return Ok(());
        };
        let property = self.ctx.copy_scoped_text(
            &self.payload.property,
            &mut occurrences._storage,
            "FreeCAD topology occurrence property",
        )?;
        let topology_id = self.ctx.copy_scoped_text(
            topology_id,
            &mut occurrences._storage,
            "FreeCAD topology occurrence identity",
        )?;
        self.ctx.push_scoped_vec(
            &mut occurrences._storage,
            &mut occurrences.records,
            TopologyOccurrence {
                property,
                indexed_name: indexed_name(kind),
                source_index,
                topology_id,
            },
            "FreeCAD topology occurrences",
        )?;
        Ok(())
    }

    fn emit_pcurves(&mut self) -> Result<(), CodecError> {
        if let Some(refusal) = self.ctx.resource_refusal() {
            return Err(refusal.into());
        }
        let tables = self.tables;
        let mut shapes = tables.tshapes.iter().enumerate();
        while shapes.len() != 0 {
            let Some((position, shape)) = self.ctx.next_charged(
                &mut shapes,
                "FreeCAD pcurve shape scan",
            )? else {
                break;
            };
            let TextTShapeGeometry::Edge {
                representations, ..
            } = &shape.geometry
            else {
                continue;
            };
            let mut representations = representations.iter().enumerate();
            while representations.len() != 0 {
                let Some((representation_index, representation)) = self.ctx.next_charged(
                    &mut representations,
                    "FreeCAD pcurve representation scan",
                )? else {
                    break;
                };
                let (primary, secondary, surface, parameter_range) = match representation {
                    TextEdgeRepresentation::Pcurve {
                        curve,
                        surface,
                        parameter_range,
                        ..
                    } => (*curve, None, *surface, *parameter_range),
                    TextEdgeRepresentation::PcurvePair {
                        curves,
                        surface,
                        parameter_range,
                        ..
                    } => (curves[0], Some(curves[1]), *surface, *parameter_range),
                    _ => continue,
                };
                let affine = self
                    .tables
                    .surfaces
                    .get(surface - 1)
                    .map(surface_parameter_affine);
                if !self.emit_pcurve(
                    position + 1,
                    representation_index,
                    primary,
                    false,
                    parameter_range,
                    affine,
                )? {
                    continue;
                }
                if let Some(secondary) = secondary {
                    self.emit_pcurve(
                        position + 1,
                        representation_index,
                        secondary,
                        true,
                        parameter_range,
                        affine,
                    )?;
                }
            }
        }
        Ok(())
    }

    fn emit_pcurve(
        &mut self,
        shape: usize,
        representation: usize,
        source: usize,
        secondary: bool,
        range: [FiniteReal; 2],
        affine: Option<SurfaceParameterAffine>,
    ) -> Result<bool, CodecError> {
        let read = self
            .ctx
            .with_scoped_storage("FreeCAD pcurve candidate", || {
                Ok::<_, CodecError>(pcurve_geometry(self.ctx, &self.tables.curve2ds[source - 1]))
            })?;
        let mut storage = read.1;
        let read = read.0;
        let read = match read {
            Ok(read) => read,
            Err(PcurveGeometryError::Resource(error)) => return Err(error),
            Err(error) => {
                let loss_index = self.losses.len();
                self.ctx.push_vec(
                    &mut self.losses,
                    pcurve_loss(self.ctx, &self.payload.id, source, Some(&error))?,
                    "FreeCAD pcurve losses",
                )?;
                drop(error);
                drop(storage);
                self.remember_pcurve_availability(
                    source,
                    PcurveAvailability::Unavailable { loss_index },
                )?;
                return Ok(false);
            }
        };
        let Some(geometry) = read else {
            let loss_index = self.losses.len();
            self.ctx.push_vec(
                &mut self.losses,
                pcurve_loss(self.ctx, &self.payload.id, source, None)?,
                "FreeCAD pcurve losses",
            )?;
            drop(storage);
            self.remember_pcurve_availability(
                source,
                PcurveAvailability::Unavailable { loss_index },
            )?;
            return Ok(false);
        };
        let domain = pcurve_parameter_domain(self.ctx, &geometry)?;
        self.remember_pcurve_availability(source, PcurveAvailability::Available(domain))?;
        let geometry = storage
            .with_storage(|| transformed_pcurve_geometry(self.ctx, geometry, affine))?;
        let Some(geometry) = geometry else {
            self.ctx.push_vec(
                &mut self.losses,
                pcurve_loss(self.ctx, &self.payload.id, source, None)?,
                "FreeCAD pcurve losses",
            )?;
            return Ok(false);
        };
        let range = normalize_pcurve_domain_range(domain, Some(range));
        let id = storage.with_storage(|| self.pcurve_id(shape, representation, secondary))?;
        let pcurve = Pcurve {
            id,
            geometry,
            metadata: PcurveMetadata::general(None, range.map(Into::into), None),
        };
        self.storage.with_storage(|| {
            self.ctx.push_vec(
                &mut self.pcurves,
                (pcurve, storage),
                "FreeCAD pcurve candidates",
            )
        })?;
        Ok(true)
    }

    fn remember_pcurve_availability(
        &mut self,
        source: usize,
        availability: PcurveAvailability,
    ) -> Result<(), CodecError> {
        if !self.ctx.contains_key_btree_map(
            &self.pcurve_availability,
            &source,
            "FreeCAD pcurve availability lookup",
        )? {
            self.storage.with_storage(|| {
                self.ctx.insert_btree_map(
                    &mut self.pcurve_availability,
                    source,
                    availability,
                    "FreeCAD pcurve availability index",
                )
            })?;
        }
        Ok(())
    }

    fn emit_unowned_triangulations(&self, ir: &mut CadIr) -> Result<(), CodecError> {
        if let Some(refusal) = self.ctx.resource_refusal() {
            return Err(refusal.into());
        }
        let mut triangulations = self.tables.triangulations.iter().enumerate();
        while triangulations.len() != 0 {
            let Some((offset, triangulation)) = self.ctx.next_charged(
                &mut triangulations,
                "FreeCAD unowned triangulation scan",
            )? else {
                break;
            };
            let index = offset + 1;
            if self.ctx.contains_hash_set(
                &self.emitted_triangulations,
                &index,
                "FreeCAD emitted triangulation lookup",
            )? {
                continue;
            }
            self.ctx.reserve_vec(
                &mut ir.model.tessellations,
                1,
                "FreeCAD tessellations records",
            )?;
            let ordinal_result = self.ctx.format_scoped(
                format_args!("{index}"), "FreeCAD unowned triangulation ordinal",
            )?;
            let ordinal_storage = ordinal_result.1;
            let ordinal = ordinal_result.0;
            let id = cadmpeg_ir::tessellation::TessellationId::mint(
                crate::native::model_id_charged(
                    self.ctx, "tessellation", &self.payload.id, &ordinal,
                )?,
            ).map_err(|error| CodecError::malformed(error.to_string()))?;
            drop((ordinal, ordinal_storage));
            ir.model.tessellations.push(
                Tessellation::from_parts(
                    id,
                    cadmpeg_ir::tessellation::TessellationMesh::from_checked_list_lanes(
                        self.ctx.copy_slice(
                            triangulation.nodes(),
                            "FreeCAD unowned triangulation nodes",
                        )?,
                        self.ctx.copy_slice(
                            triangulation.triangles(),
                            "FreeCAD unowned triangulation triangles",
                        )?,
                        triangulation
                            .normals()
                            .map(|normals| {
                                self.ctx
                                    .copy_slice(normals, "FreeCAD unowned triangulation normals")
                            })
                            .transpose()?,
                    )?,
                    Vec::new(),
                )
                .map_err(|error| {
                    CodecError::malformed(format_args!("invalid triangulation: {error}"))
                })?
                .with_admitted_chordal_deflection(Some(triangulation.deflection))
                .with_source_object(Some(self.source_association()?)),
            );
        }
        Ok(())
    }

    fn pcurve_id(
        &self,
        edge: usize,
        representation: usize,
        secondary: bool,
    ) -> Result<PcurveId, CodecError> {
        let (data, storage) = self.ctx.format_scoped(
            format_args!(
                "{}:{}:{}",
                edge,
                representation + 1,
                usize::from(secondary) + 1
            ),
            "FreeCAD pcurve key",
        )?;
        let key = ScopedData {
            data,
            _storage: storage,
        };
        PcurveId::mint(crate::native::model_id_charged_at(
            self.ctx,
            "pcurve",
            &self.payload.id,
            &key.data,
            "FreeCAD pcurve identity",
        )?)
        .map_err(CodecError::malformed)
    }

    fn shell_component_id(&self, key: &str, component_index: usize) -> Result<ShellId, CodecError> {
        let (data, storage) = self.ctx.format_scoped(
            format_args!("{key}:component:{}", component_index + 1),
            "FreeCAD shell component key",
        )?;
        let child = ScopedData {
            data,
            _storage: storage,
        };
        ShellId::mint(crate::native::model_id_charged_at(
            self.ctx,
            "shell",
            &self.payload.id,
            &child.data,
            "FreeCAD shell component identity",
        )?)
        .map_err(CodecError::malformed)
    }

    fn body_roots(&self) -> Result<Vec<BodyRoot>, CodecError> {
        let has_multiple_roots = self.tables.roots.len() > 1;
        let mut roots = self
            .ctx
            .collection_vec(self.tables.roots.len(), "FreeCAD topology body roots")?;
        let mut source_roots = self.tables.roots.iter().enumerate();
        while source_roots.len() != 0 {
            let Some((index, root)) = self.ctx.next_charged(
                &mut source_roots,
                "FreeCAD body root scan",
            )? else {
                break;
            };
            self.shape(root.shape)?;
            let transform = self.tables.location(root.location)?;
            roots.push(BodyRoot {
                shape: root.shape,
                transform,
                reversed: is_reversed(root.orientation),
                root_ordinal: has_multiple_roots.then_some(index + 1),
            });
        }
        Ok(roots)
    }

    fn append_body(
        &mut self,
        ctx: &DecodeContext<'_>,
        ir: &mut CadIr,
        root: BodyRoot,
    ) -> Result<(), CodecError> {
        self.body_scope = root.transform;
        self.root_discriminator = root.root_ordinal;
        self.vertices = HashMap::new();
        self.edges = HashMap::new();
        self.current_body = None;
        self.cache_storage = self
            .ctx
            .reserve_scoped(0, "FreeCAD topology occurrence caches")?;
        let root_shape = self.shape(root.shape)?;
        let root_kind = root_shape.kind();
        if let TextTShapeGeometry::Edge {
            degenerated,
            representations,
            ..
        } = &root_shape.geometry
        {
            if root_shape.children.is_empty() {
                if !degenerated
                    && !self.ctx.any_by(
                        representations,
                        |representation| {
                            Ok(matches!(
                                representation,
                                TextEdgeRepresentation::Curve3d { .. }
                            ))
                        },
                        "FreeCAD unbounded edge representation search",
                    )?
                {
                    return Err(CodecError::malformed(format_args!(
                        "unbounded edge TShape {} has no exact curve",
                        root.shape
                    )));
                }
                return Ok(());
            }
        }
        let body_key = self.topology_label(root.shape, Transform::identity())?;
        let (data, storage) =
            ctx.with_scoped_storage("FreeCAD body identity scratch", || {
                BodyId::mint(crate::native::model_id_charged_at(
                    self.ctx,
                    "body",
                    &self.payload.id,
                    &body_key.data,
                    "FreeCAD body identity",
                )?)
                .map_err(CodecError::malformed)
            })?;
        let body_id = ScopedData {
            data,
            _storage: storage,
        };
        self.current_body = Some(self.cache_storage.with_storage(|| {
            body_id
                .data
                .try_clone_for_decode(self.ctx, "FreeCAD current body identity")
        })?);
        let kind = match root_kind {
            TextShapeKind::Solid => BodyKind::Solid,
            TextShapeKind::Wire | TextShapeKind::Edge => BodyKind::Wire,
            TextShapeKind::Shell | TextShapeKind::Face => BodyKind::Sheet,
            _ => BodyKind::General,
        };
        let tessellation_start = ir.model.tessellations.len();
        let mut regions = Vec::new();
        self.append_shape_regions(
            ctx,
            ir,
            &body_id.data,
            RegionTraversal {
                shape_index: root.shape,
                transform: Transform::identity(),
                reversed: root.reversed,
            },
            &mut regions,
        )?;
        if regions.is_empty() {
            ctx.truncate_vec(
                &mut ir.model.tessellations,
                tessellation_start,
                "FreeCAD empty body tessellations",
            )?;
            return Ok(());
        }
        self.ctx
            .reserve_vec(&mut ir.model.bodies, 1, "FreeCAD bodies records")?;
        ir.model.bodies.push(Body {
            id: body_id
                .data
                .try_clone_for_decode(self.ctx, "FreeCAD body record identity")?,
            kind,
            regions,
            transform: (!is_identity(root.transform)).then_some(root.transform),
            name: None,
            color: None,
            visible: None,
        });
        if matches!(
            root_kind,
            TextShapeKind::Compound | TextShapeKind::CompSolid
        ) {
            self.bind_topology(
                root_kind,
                root.shape,
                Transform::identity(),
                body_id.data.as_str(),
            )?;
        }
        Ok(())
    }

    fn append_shape_regions(
        &mut self,
        ctx: &DecodeContext<'_>,
        ir: &mut CadIr,
        body: &BodyId,
        traversal: RegionTraversal,
        output: &mut Vec<RegionId>,
    ) -> Result<(), CodecError> {
        let RegionTraversal {
            shape_index,
            transform,
            reversed,
        } = traversal;
        let _depth = ctx.enter_nested("transfer FCStd topology nesting")?;
        let shape = self.shape(shape_index)?;
        if matches!(
            shape.kind(),
            TextShapeKind::Compound | TextShapeKind::CompSolid
        ) {
            let mut children = shape.children.iter();
            while children.len() != 0 {
                let Some(child) = ctx.next_charged(
                    &mut children,
                    "FreeCAD topology child scan",
                )? else {
                    break;
                };
                self.append_shape_regions(
                    ctx,
                    ir,
                    body,
                    RegionTraversal {
                        shape_index: child.shape,
                        transform: transform
                            .compose(self.tables.location(child.location)?)
                            .map_err(location_transform_error)?,
                        reversed: reversed ^ is_reversed(child.orientation),
                    },
                    output,
                )?;
            }
            return Ok(());
        }
        let key = self.topology_label(shape_index, transform)?;
        let (data, storage) =
            ctx.with_scoped_storage("FreeCAD region identity", || {
                RegionId::mint(crate::native::model_id_charged_at(
                    self.ctx,
                    "region",
                    &self.payload.id,
                    &key.data,
                    "FreeCAD region identity",
                )?)
                .map_err(CodecError::malformed)
            })?;
        let region_id = ScopedData {
            data,
            _storage: storage,
        };
        let mut shells = Vec::new();
        if shape.kind() == TextShapeKind::Solid {
            let mut children = shape.children.iter();
            while children.len() != 0 {
                let Some(child) = ctx.next_charged(
                    &mut children,
                    "FreeCAD region shell scan",
                )? else {
                    break;
                };
                if self.tables.tshapes[child.shape - 1].kind() != TextShapeKind::Shell {
                    continue;
                }
                self.append_shell_shape(
                    ctx,
                    ir,
                    &region_id.data,
                    RegionTraversal {
                        shape_index: child.shape,
                        transform: transform
                            .compose(self.tables.location(child.location)?)
                            .map_err(location_transform_error)?,
                        reversed: reversed ^ is_reversed(child.orientation),
                    },
                    &mut shells,
                )?;
            }
        } else {
            self.append_shell_shape(ctx, ir, &region_id.data, traversal, &mut shells)?;
        }
        if !shells.is_empty() {
            region_id._storage.commit()?;
            self.ctx
                .reserve_vec(&mut ir.model.regions, 1, "FreeCAD regions records")?;
            ir.model.regions.push(Region {
                id: region_id
                    .data
                    .try_clone_for_decode(self.ctx, "FreeCAD region record identity")?,
                body: body.try_clone_for_decode(self.ctx, "FreeCAD region body identity")?,
                shells,
            });
            if shape.kind() == TextShapeKind::Solid {
                self.bind_topology(
                    TextShapeKind::Solid,
                    shape_index,
                    transform,
                    region_id.data.as_str(),
                )?;
            }
            ctx.reserve_vec(output, 1, "FreeCAD body regions")?;
            output.push(region_id.data);
        }
        Ok(())
    }

    fn append_shell_shape(
        &mut self,
        ctx: &DecodeContext<'_>,
        ir: &mut CadIr,
        region: &RegionId,
        traversal: RegionTraversal,
        output: &mut Vec<ShellId>,
    ) -> Result<(), CodecError> {
        let RegionTraversal {
            shape_index,
            transform,
            reversed,
        } = traversal;
        let shape = self.shape(shape_index)?;
        let key = self.topology_label(shape_index, transform)?;
        let (data, storage) =
            ctx.with_scoped_storage("FreeCAD shell identity", || {
                ShellId::mint(crate::native::model_id_charged_at(
                    self.ctx,
                    "shell",
                    &self.payload.id,
                    &key.data,
                    "FreeCAD shell identity",
                )?)
                .map_err(CodecError::malformed)
        })?;
        let shell_id = ScopedData {
            data,
            _storage: storage,
        };
        if shape.kind() == TextShapeKind::Shell {
            let (values, storage) =
                ctx.with_scoped_storage("FreeCAD shell face uses", || {
                    let mut face_uses = Vec::new();
                    let mut children = shape.children.iter();
                    while children.len() != 0 {
                        let Some(child) = ctx.next_charged(
                            &mut children,
                            "FreeCAD shell face scan",
                        )? else {
                            break;
                        };
                        if self.tables.tshapes[child.shape - 1].kind() == TextShapeKind::Face {
                            ctx.push_vec(
                                &mut face_uses,
                                child,
                                "FreeCAD shell face uses",
                            )?;
                        }
                    }
                    Ok::<_, CodecError>(face_uses)
                })?;
            let face_uses = ScopedVec {
                values,
                _storage: storage,
            };
            let (values, storage) = ctx
                .with_scoped_storage("FreeCAD face components", || {
                    self.face_components(ctx, &face_uses.values, transform)
                })?;
            let components = ScopedVec {
                values,
                _storage: storage,
            };
            ctx.reserve_vec(output, components.values.len(), "FreeCAD shell components")?;
            let mut components = components.values.iter().enumerate();
            while components.len() != 0 {
                let Some((component_index, component)) = ctx.next_charged(
                    &mut components,
                    "FreeCAD component scan",
                )? else {
                    break;
                };
                let component_id = if component_index == 0 {
                    shell_id
                        .data
                        .try_clone_for_decode(self.ctx, "FreeCAD first shell component identity")?
                } else {
                    self.shell_component_id(&key.data, component_index)?
                };
                let mut faces = self
                    .ctx
                    .collection_vec(component.len(), "FreeCAD component faces")?;
                let mut component_faces = component.iter();
                while component_faces.len() != 0 {
                    let Some(&face_index) = ctx.next_charged(
                        &mut component_faces,
                        "FreeCAD component face scan",
                    )? else {
                        break;
                    };
                    if let Some(face) = self.append_face(
                        ir,
                        &component_id,
                        face_uses.values[face_index],
                        transform,
                        reversed,
                    )? {
                        faces.push(face);
                    }
                }
                self.ctx
                    .reserve_vec(&mut ir.model.shells, 1, "FreeCAD shells records")?;
                ir.model.shells.push(
                    Shell::new(
                        component_id.try_clone_for_decode(
                            self.ctx,
                            "FreeCAD component shell record identity",
                        )?,
                        region.try_clone_for_decode(
                            self.ctx,
                            "FreeCAD component shell region identity",
                        )?,
                        faces,
                        Vec::new(),
                        Vec::new(),
                    )
                    .map_err(|message| cadmpeg_core::CodecError::Malformed(message.to_string()))?,
                );
                self.bind_topology(
                    TextShapeKind::Shell,
                    shape_index,
                    transform,
                    component_id.as_str(),
                )?;
                output.push(component_id);
            }
            return Ok(());
        }
        let mut faces = Vec::new();
        let mut wire_edges = Vec::new();
        match shape.kind() {
            TextShapeKind::Face => {
                let shape_use = TextShapeUse {
                    shape: shape_index,
                    orientation: TextOrientation::Forward,
                    location: 0.into(),
                };
                if let Some(face) =
                    self.append_face(ir, &shell_id.data, &shape_use, transform, reversed)?
                {
                    self.ctx.reserve_vec(&mut faces, 1, "FreeCAD shell faces")?;
                    faces.push(face);
                }
            }
            TextShapeKind::Wire => {
                let mut children = shape.children.iter();
                while children.len() != 0 {
                    let Some(child) = ctx.next_charged(
                        &mut children,
                        "FreeCAD topology child scan",
                    )? else {
                        break;
                    };
                    if self.shape(child.shape)?.kind() == TextShapeKind::Edge {
                        let edge = self.ensure_edge(ir, child, transform)?;
                        self.ctx
                            .reserve_vec(&mut wire_edges, 1, "FreeCAD wire edges")?;
                        wire_edges.push(edge);
                    }
                }
            }
            TextShapeKind::Edge => {
                let edge_use = TextShapeUse {
                    shape: shape_index,
                    orientation: if reversed {
                        TextOrientation::Reversed
                    } else {
                        TextOrientation::Forward
                    },
                    location: 0.into(),
                };
                let edge = self.ensure_edge(ir, &edge_use, transform)?;
                self.ctx
                    .reserve_vec(&mut wire_edges, 1, "FreeCAD wire edges")?;
                wire_edges.push(edge);
            }
            TextShapeKind::Vertex => {
                let vertex_use = TextShapeUse {
                    shape: shape_index,
                    orientation: TextOrientation::Forward,
                    location: 0.into(),
                };
                let vertex = self.ensure_vertex(ir, &vertex_use, transform)?;
                self.ctx
                    .reserve_vec(&mut ir.model.shells, 1, "FreeCAD shells records")?;
                ir.model.shells.push(
                    Shell::new(
                        shell_id.data.try_clone_for_decode(
                            self.ctx,
                            "FreeCAD vertex shell record identity",
                        )?,
                        region.try_clone_for_decode(
                            self.ctx,
                            "FreeCAD vertex shell region identity",
                        )?,
                        faces,
                        wire_edges,
                        {
                            let mut vertices =
                                ctx.collection_vec(1, "FreeCAD free shell vertices")?;
                            vertices.push(vertex);
                            vertices
                        },
                    )
                    .map_err(|message| cadmpeg_core::CodecError::Malformed(message.to_string()))?,
                );
                shell_id._storage.commit()?;
                ctx.push_vec(output, shell_id.data, "FreeCAD shell components")?;
                return Ok(());
            }
            _ => {}
        }
        self.ctx
            .reserve_vec(&mut ir.model.shells, 1, "FreeCAD shells records")?;
        ir.model.shells.push(
            Shell::new(
                shell_id
                    .data
                    .try_clone_for_decode(self.ctx, "FreeCAD shell record identity")?,
                region.try_clone_for_decode(self.ctx, "FreeCAD shell region identity")?,
                faces,
                wire_edges,
                Vec::new(),
            )
            .map_err(|message| cadmpeg_core::CodecError::Malformed(message.to_string()))?,
        );
        if shape.kind() == TextShapeKind::Wire {
            self.bind_topology(shape.kind(), shape_index, transform, shell_id.data.as_str())?;
        }
        shell_id._storage.commit()?;
        ctx.push_vec(output, shell_id.data, "FreeCAD shell components")?;
        Ok(())
    }

    fn face_components(
        &self,
        ctx: &DecodeContext<'_>,
        face_uses: &[&TextShapeUse],
        parent: Transform,
    ) -> Result<Vec<Vec<usize>>, CodecError> {
        if face_uses.is_empty() {
            let mut components = ctx.collection_vec(1, "FreeCAD connected components")?;
            components.push(Vec::new());
            return Ok(components);
        }
        let mut connectivity = ctx.collection_vec(face_uses.len(), "FreeCAD face connectivity")?;
        let mut face_uses = face_uses.iter();
        while face_uses.len() != 0 {
            let Some(&face_use) = ctx.next_charged(
                &mut face_uses,
                "FreeCAD connectivity face scan",
            )? else {
                break;
            };
            let face_transform = parent
                .compose(self.tables.location(face_use.location)?)
                .map_err(location_transform_error)?;
            let face = self.shape(face_use.shape)?;
            let mut keys = BTreeSet::new();
            let mut face_children = face.children.iter();
            while face_children.len() != 0 {
                let Some(wire_use) = ctx.next_charged(
                    &mut face_children,
                    "FreeCAD connectivity wire scan",
                )? else {
                    break;
                };
                if self.tables.tshapes[wire_use.shape - 1].kind() != TextShapeKind::Wire {
                    continue;
                }
                let wire_transform = face_transform
                    .compose(self.tables.location(wire_use.location)?)
                    .map_err(location_transform_error)?;
                let wire = self.shape(wire_use.shape)?;
                let mut wire_children = wire.children.iter();
                while wire_children.len() != 0 {
                    let Some(edge_use) = ctx.next_charged(
                        &mut wire_children,
                        "FreeCAD connectivity edge scan",
                    )? else {
                        break;
                    };
                    if self.tables.tshapes[edge_use.shape - 1].kind() != TextShapeKind::Edge {
                        continue;
                    }
                    let edge_transform = wire_transform
                        .compose(self.tables.location(edge_use.location)?)
                        .map_err(location_transform_error)?;
                    let (data, storage) = self.ctx.with_scoped_storage(
                        "FreeCAD occurrence lookup scratch",
                        || {
                            OccurrenceKey::new(
                                self.ctx,
                                edge_use.shape,
                                self.body_scope
                                    .compose(edge_transform)
                                    .map_err(location_transform_error)?,
                            )
                        },
                    )?;
                    let edge_key = ScopedData {
                        data,
                        _storage: storage,
                    };
                    let (key, key_storage) = ctx.format_scoped(
                        format_args!("edge:{}", edge_key.data.0),
                        "FreeCAD face connectivity edge identity",
                    )?;
                    let key = ScopedData {
                        data: key,
                        _storage: key_storage,
                    };
                    if !ctx.contains_btree_set(
                        &keys,
                        &key.data,
                        "FreeCAD face connectivity edge lookup",
                    )? {
                        key._storage.commit()?;
                        ctx.insert_btree_set(
                            &mut keys,
                            key.data,
                            "FreeCAD face connectivity edge keys",
                        )?;
                    }
                    let edge = self.shape(edge_use.shape)?;
                    let mut edge_children = edge.children.iter();
                    while edge_children.len() != 0 {
                        let Some(vertex_use) = ctx.next_charged(
                            &mut edge_children,
                            "FreeCAD connectivity vertex scan",
                        )? else {
                            break;
                        };
                        if self.tables.tshapes[vertex_use.shape - 1].kind()
                            != TextShapeKind::Vertex
                        {
                            continue;
                        }
                        let vertex_transform = edge_transform
                            .compose(self.tables.location(vertex_use.location)?)
                            .map_err(location_transform_error)?;
                        let (data, storage) = self.ctx.with_scoped_storage(
                            "FreeCAD occurrence lookup scratch",
                            || {
                                OccurrenceKey::new(
                                    self.ctx,
                                    vertex_use.shape,
                                    self.body_scope
                                        .compose(vertex_transform)
                                        .map_err(location_transform_error)?,
                                )
                            },
                        )?;
                        let vertex_key = ScopedData {
                            data,
                            _storage: storage,
                        };
                        let (key, key_storage) = ctx.format_scoped(
                            format_args!("vertex:{}", vertex_key.data.0),
                            "FreeCAD face connectivity vertex identity",
                        )?;
                        let key = ScopedData {
                            data: key,
                            _storage: key_storage,
                        };
                        if !ctx.contains_btree_set(
                            &keys,
                            &key.data,
                            "FreeCAD face connectivity vertex lookup",
                        )? {
                            key._storage.commit()?;
                            ctx.insert_btree_set(
                                &mut keys,
                                key.data,
                                "FreeCAD face connectivity vertex keys",
                            )?;
                        }
                    }
                }
            }
            connectivity.push(keys);
        }

        connected_components(ctx, &connectivity)
    }

    fn append_face(
        &mut self,
        ir: &mut CadIr,
        shell: &ShellId,
        face_use: &TextShapeUse,
        parent: Transform,
        reversed: bool,
    ) -> Result<Option<FaceId>, CodecError> {
        let face_transform = parent
            .compose(self.tables.location(face_use.location)?)
            .map_err(location_transform_error)?;
        let face_reversed = reversed ^ is_reversed(face_use.orientation);
        let shape = self.shape(face_use.shape)?;
        let TextTShapeGeometry::Face {
            tolerance,
            surface,
            location,
            triangulation,
            ..
        } = shape.geometry
        else {
            return Ok(None);
        };
        let surface_transform = face_transform
            .compose(self.tables.location(location)?)
            .map_err(location_transform_error)?;
        let face_key = self.topology_label(face_use.shape, face_transform)?;
        let face_id = FaceId::mint(crate::native::model_id_charged_at(
            self.ctx,
            "face",
            &self.payload.id,
            &face_key.data,
            "FreeCAD face identity",
        )?)
        .map_err(CodecError::malformed)?;
        // OCCT triangulation nodes are already expressed in the face's surface-location frame.
        // Only the owning topological face placement remains to be applied here.
        let located_triangulation = triangulation
            .map(|index| {
                let triangulation = index.resolve(self.tables.triangulations)?;
                let index = index.index();
                let mut vertices = self.ctx.collection_vec(triangulation.nodes().len(), "FreeCAD placed triangulation nodes")?;
                let mut nodes = triangulation.nodes().iter();
                while nodes.len() != 0 {
                    let Some(point) = self.ctx.next_charged(
                        &mut nodes,
                        "FreeCAD placed triangulation node scan",
                    )? else {
                        break;
                    };
                    vertices.push(face_transform.apply_point(point.get()).ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "placed triangulation node for face {} contains a non-finite coordinate",
                            face_use.shape
                        ))
                    })?);
                }
                let triangles = self.ctx.copy_slice(triangulation.triangles(), "FreeCAD placed triangulation triangles")?;
                let scale = uniform_scale(face_transform)?;
                Ok::<_, CodecError>((index, triangulation, vertices, triangles, scale))
            })
            .transpose()?;
        let surface_id =
            if let Some(surface) = surface {
                surface.resolve(self.tables.surfaces)?;
                self.located_surface(ir, surface.index(), surface_transform)?
            } else if let Some((index, triangulation, vertices, triangles, deflection_scale)) =
                &located_triangulation
            {
                let (key, _key_storage) = self.ctx.format_scoped(
                    format_args!("triangulation:{index}@{}", face_key.data),
                    "FreeCAD triangulation surface key",
                )?;
                let key = ScopedData {
                    data: key,
                    _storage: _key_storage,
                };
                let id = SurfaceId::mint(crate::native::model_id_charged_at(
                    self.ctx,
                    "surface",
                    &self.payload.id,
                    &key.data,
                    "FreeCAD triangulation surface identity",
                )?)
                .map_err(CodecError::malformed)?;
                let new_surface = !self.ctx.contains_hash_set(
                    &self.emitted_surfaces,
                    &id,
                    "FreeCAD emitted surface lookup",
                )?;
                if new_surface {
                    self.storage.with_storage(|| {
                        self.ctx.insert_hash_set(
                            &mut self.emitted_surfaces,
                            id.try_clone_for_decode(self.ctx, "FreeCAD emitted surface identity")?,
                            "FreeCAD emitted surfaces",
                        )
                    })?;
                    self.ctx
                        .reserve_vec(&mut ir.model.surfaces, 1, "FreeCAD surfaces records")?;
                    ir.model.surfaces.push(Surface {
                        id: id.try_clone_for_decode(self.ctx, "FreeCAD polygonal surface identity")?,
                        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Polygonal(
                            PolygonalSurface::from_admitted_scaled_deflection(
                                self.ctx
                                    .copy_slice(vertices, "FreeCAD polygonal surface vertices")?,
                                self.ctx
                                    .copy_slice(triangles, "FreeCAD polygonal surface triangles")?,
                                triangulation.deflection,
                                *deflection_scale,
                                self.ctx,
                            )?
                            .map_err(|error| CodecError::Malformed(error.to_string()))?,
                        )),
                        source_object: Some(self.source_association()?),
                    });
                    self.geometry.index_surface(
                        self.ctx,
                        &ir.model.surfaces[ir.model.surfaces.len() - 1].id,
                        ir.model.surfaces.len() - 1,
                    )?;
                }
                id
            } else {
                return Ok(None);
            };
        if let Some((index, triangulation, vertices, triangles, deflection_scale)) =
            located_triangulation
        {
            self.storage.with_storage(|| {
                self.ctx.insert_hash_set(
                    &mut self.emitted_triangulations,
                    index,
                    "FreeCAD emitted triangulations",
                )
            })?;
            let (tessellation_key, _tessellation_key_storage) = self.ctx.format_scoped(
                format_args!("{index}@{}", face_key.data), "FreeCAD tessellation key",
            )?;
            let tessellation_key = ScopedData {
                data: tessellation_key,
                _storage: _tessellation_key_storage,
            };
            let mut faces = self.ctx.collection_vec(1, "FreeCAD tessellation faces")?;
            faces.push(
                face_id.try_clone_for_decode(self.ctx, "FreeCAD tessellation face identity")?,
            );
            // An unshaded mesh is stated by absence, not by an empty lane.
            let normals = if let Some(native_normals) = triangulation.normals() {
                let mut normals = self
                    .ctx
                    .collection_vec(native_normals.len(), "FreeCAD placed triangulation normals")?;
                let mut native_normals = native_normals.iter();
                while native_normals.len() != 0 {
                    let Some(normal) = self.ctx.next_charged(
                        &mut native_normals,
                        "FreeCAD placed triangulation normal scan",
                    )? else {
                        break;
                    };
                    normals.push(transform_normalized_vector(face_transform, normal.get()).ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "placed triangulation normal for face {} contains a non-finite component",
                            face_key.data
                        ))
                    })?);
                }
                Some(normals)
            } else {
                None
            };
            self.ctx.reserve_vec(
                &mut ir.model.tessellations,
                1,
                "FreeCAD tessellations records",
            )?;
            ir.model.tessellations.push(
                Tessellation::from_parts(
                    cadmpeg_ir::tessellation::TessellationId::mint(crate::native::model_id_charged_at(self.ctx, "tessellation",
                        &self.payload.id, &tessellation_key.data, "FreeCAD tessellation identity")?)
                        .map_err(|error| CodecError::malformed(error.to_string()))?,
                    cadmpeg_ir::tessellation::TessellationMesh::from_checked_list_lanes(
                        vertices, triangles, normals,
                    )?,
                    Vec::new(),
                )
                .map_err(|error| {
                    CodecError::malformed(format_args!("invalid triangulation: {error}"))
                })?
                .with_body(self.current_body.as_ref().map(|body| {
                    body.try_clone_for_decode(self.ctx, "FreeCAD tessellation body identity")
                }).transpose()?)
                .with_faces(faces)
                .with_admitted_chordal_deflection(Some(
                    triangulation
                        .deflection
                        .scaled(deflection_scale)
                        .ok_or_else(|| {
                            CodecError::malformed(format_args!(
                                "invalid triangulation deflection: chordal_deflection must be finite and non-negative"
                            ))
                        })?,
                ))
                .with_source_object(Some(self.source_association()?)),
            );
        }
        let mut loops = Vec::new();
        let mut loop_index = 0;
        let mut face_children = shape.children.iter();
        while face_children.len() != 0 {
            let Some(wire_use) = self.ctx.next_charged(
                &mut face_children,
                "FreeCAD face wire scan",
            )? else {
                break;
            };
            if self.tables.tshapes[wire_use.shape - 1].kind() != TextShapeKind::Wire {
                continue;
            }
            let current_loop_index = loop_index;
            loop_index += 1;
            let wire_transform = face_transform
                .compose(self.tables.location(wire_use.location)?)
                .map_err(location_transform_error)?;
            let wire = self.shape(wire_use.shape)?;
            let (values, storage) =
                self.ctx.with_scoped_storage("FreeCAD wire edge uses", || {
                    let mut edge_uses = Vec::new();
                    let mut wire_children = wire.children.iter();
                    while wire_children.len() != 0 {
                        let Some(child) = self.ctx.next_charged(
                            &mut wire_children,
                            "FreeCAD wire edge scan",
                        )? else {
                            break;
                        };
                        if self.tables.tshapes[child.shape - 1].kind() == TextShapeKind::Edge {
                            self.ctx.push_vec(
                                &mut edge_uses,
                                child,
                                "FreeCAD wire edge uses",
                            )?;
                        }
                    }
                    Ok::<_, CodecError>(edge_uses)
                })?;
            let mut edge_uses = ScopedVec {
                values,
                _storage: storage,
            };
            let wire_reversed = face_reversed ^ is_reversed(wire_use.orientation);
            if wire_reversed {
                self.ctx
                    .reverse(&mut edge_uses.values, "FreeCAD wire edge reversal")?;
            }
            if edge_uses.values.is_empty() {
                continue;
            }
            let (data, storage) = self.ctx.format_scoped(
                format_args!("{}:{}", face_key.data, current_loop_index + 1),
                "FreeCAD face loop key",
            )?;
            let loop_key = ScopedData {
                data,
                _storage: storage,
            };
            let loop_id = LoopId::mint(crate::native::model_id_charged_at(
                self.ctx,
                "loop",
                &self.payload.id,
                &loop_key.data,
                "FreeCAD face loop identity",
            )?)
            .map_err(CodecError::malformed)?;
            let mut coedge_ids = self
                .ctx
                .collection_vec(edge_uses.values.len(), "FreeCAD loop coedge IDs")?;
            let mut coedge_indices = (0..edge_uses.values.len()).enumerate();
            while coedge_indices.len() != 0 {
                let Some((index, _)) = self.ctx.next_charged(
                    &mut coedge_indices,
                    "FreeCAD coedge identity scan",
                )? else {
                    break;
                };
                let (data, storage) = self.ctx.format_scoped(
                    format_args!("{}:{}:{}", face_key.data, current_loop_index + 1, index + 1),
                    "FreeCAD face coedge key",
                )?;
                let key = ScopedData {
                    data,
                    _storage: storage,
                };
                coedge_ids.push(
                    CoedgeId::mint(crate::native::model_id_charged_at(
                        self.ctx,
                        "coedge",
                        &self.payload.id,
                        &key.data,
                        "FreeCAD face coedge identity",
                    )?)
                    .map_err(CodecError::malformed)?,
                );
            }
            let mut edge_use_iter = edge_uses.values.iter().enumerate();
            while edge_use_iter.len() != 0 {
                let Some((index, edge_use)) = self.ctx.next_charged(
                    &mut edge_use_iter,
                    "FreeCAD edge use scan",
                )? else {
                    break;
                };
                let edge_transform = wire_transform
                    .compose(self.tables.location(edge_use.location)?)
                    .map_err(location_transform_error)?;
                let edge = self.ensure_edge(ir, edge_use, wire_transform)?;
                let pcurve =
                    self.face_pcurve(edge_use, edge_transform, surface, surface_transform)?;
                let id: CoedgeId = coedge_ids[index]
                    .try_clone_for_decode(self.ctx, "FreeCAD coedge radial identity")?;
                self.ctx
                    .reserve_vec(&mut ir.model.coedges, 1, "FreeCAD coedges records")?;
                ir.model.coedges.push(Coedge {
                    id: id.try_clone_for_decode(self.ctx, "FreeCAD coedge record identity")?,
                    owner_loop: loop_id.try_clone_for_decode(self.ctx, "FreeCAD coedge loop identity")?,
                    edge,
                    radial_next: id,
                    sense: sense(is_reversed(edge_use.orientation) ^ wire_reversed),
                    use_curve: None,
                    pcurves: {
                        let mut uses = Vec::new();
                        if let Some((pcurve, parameter_range)) = pcurve {
                            self.ctx.push_vec(&mut uses, cadmpeg_ir::topology::PcurveUse {
                                pcurve, isoparametric: None,
                                parameter_range: parameter_range.map(cadmpeg_ir::geometry::DirectedParameterRange::from_finite_endpoints).transpose().map_err(CodecError::malformed)?,
                            }, "FreeCAD coedge pcurve uses")?;
                        }
                        uses
                    },
                });
            }
            self.ctx
                .reserve_vec(&mut ir.model.loops, 1, "FreeCAD loops records")?;
            ir.model.loops.push(Loop {
                id: loop_id.try_clone_for_decode(self.ctx, "FreeCAD loop record identity")?,
                face: face_id.try_clone_for_decode(self.ctx, "FreeCAD loop face identity")?,
                boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
                    cadmpeg_ir::topology::LoopRing::new(self.ctx, coedge_ids, Vec::new())
                        .map_err(cadmpeg_core::CodecError::from)?
                        .map_err(|error| {
                            crate::resource::malformed_charged(
                                self.ctx,
                                format_args!(
                                    "FCStd face {} loop {} has invalid ring: {error}",
                                    face_id,
                                    current_loop_index + 1,
                                ),
                                "FreeCAD face ring diagnostic",
                            )
                        })?,
                ),
            });
            self.bind_topology(
                TextShapeKind::Wire,
                wire_use.shape,
                wire_transform,
                loop_id.as_str(),
            )?;
            self.ctx.reserve_vec(&mut loops, 1, "FreeCAD face loops")?;
            loops.push(loop_id);
        }
        self.ctx
            .reserve_vec(&mut ir.model.faces, 1, "FreeCAD faces records")?;
        ir.model.faces.push(Face {
            id: face_id.try_clone_for_decode(self.ctx, "FreeCAD face record identity")?,
            shell: shell.try_clone_for_decode(self.ctx, "FreeCAD face shell identity")?,
            surface: surface_id,
            sense: sense(face_reversed),
            loops: cadmpeg_ir::topology::FaceLoops::unspecified(loops),
            name: None,
            color: None,
            tolerance: PositiveReal::from_finite(tolerance),
        });
        self.bind_topology(
            TextShapeKind::Face,
            face_use.shape,
            face_transform,
            face_id.as_str(),
        )?;
        Ok(Some(face_id))
    }

    fn ensure_edge(
        &mut self,
        ir: &mut CadIr,
        edge_use: &TextShapeUse,
        parent: Transform,
    ) -> Result<EdgeId, CodecError> {
        let transform = parent
            .compose(self.tables.location(edge_use.location)?)
            .map_err(location_transform_error)?;
        let (data, storage) =
            self.ctx
                .with_scoped_storage("FreeCAD occurrence lookup scratch", || {
                    OccurrenceKey::new(
                        self.ctx,
                        edge_use.shape,
                        self.body_scope
                            .compose(transform)
                            .map_err(location_transform_error)?,
                    )
                })?;
        let key = ScopedData {
            data,
            _storage: storage,
        };
        if let Some(id) = self
            .ctx
            .get_hash_map(&self.edges, &key.data, "FreeCAD cached edge lookup")?
        {
            let id: EdgeId =
                id.try_clone_for_decode(self.ctx, "FreeCAD cached edge lookup identity")?;
            self.bind_topology(TextShapeKind::Edge, edge_use.shape, transform, id.as_str())?;
            return Ok(id);
        }
        let shape = self.shape(edge_use.shape)?;
        let TextTShapeGeometry::Edge {
            tolerance,
            degenerated,
            representations,
            ..
        } = &shape.geometry
        else {
            return Err(CodecError::malformed(format_args!(
                "TShape {} is not an edge",
                edge_use.shape
            )));
        };
        let (start_use, end_use) = edge_endpoint_uses(self.ctx, edge_use.shape, &shape.children)?;
        let start = self.ensure_vertex(ir, start_use, transform)?;
        let end = self.ensure_vertex(ir, end_use, transform)?;
        let label = self.topology_label(edge_use.shape, transform)?;
        let id = EdgeId::mint(crate::native::model_id_charged_at(
            self.ctx,
            "edge",
            &self.payload.id,
            &label.data,
            "FreeCAD edge identity",
        )?)
        .map_err(CodecError::malformed)?;
        let curve_representation = select_exact_curve_representation(
            self.ctx,
            edge_use.shape,
            representations,
            &self.tables,
        )?;
        let polygon_representation = if curve_representation.is_none() {
            unique_fallback_polygon_representation(self.ctx, edge_use.shape, representations)?
        } else {
            None
        };
        let curve = if *degenerated {
            None
        } else if let Some((
            _,
            TextEdgeRepresentation::Curve3d {
                curve, location, ..
            },
        )) = curve_representation
        {
            let carrier_transform = transform
                .compose(self.tables.location(*location)?)
                .map_err(location_transform_error)?;
            Some(self.located_curve(ir, *curve, carrier_transform)?)
        } else if let Some((ordinal, representation)) = polygon_representation {
            Some(self.polygon_curve(ir, &id, ordinal, representation, transform)?)
        } else {
            None
        };
        let param_range = curve_representation
            .and_then(|(_, representation)| representation.parameter_range())
            .or_else(|| {
                polygon_representation.and_then(|(_, representation)| {
                    self.polygon_parameters(representation)
                        .and_then(|parameters| Some([*parameters.first()?, *parameters.last()?]))
                })
            });
        let curve_position = curve
            .as_ref()
            .map(|id| self.geometry.curve_position(self.ctx, ir, id))
            .transpose()?
            .flatten();
        let param_range = curve_position.map_or(param_range, |position| {
            normalize_occt_curve_range(ir.model.curves[position].geometry.solved()?, param_range)
        });
        self.ctx
            .reserve_vec(&mut ir.model.edges, 1, "FreeCAD edges records")?;
        ir.model.edges.push(Edge {
            id: id.try_clone_for_decode(self.ctx, "FreeCAD edge record identity")?,
            carrier: cadmpeg_ir::topology::EdgeCarrier::from_finite_parts(curve, param_range)
                .map_err(CodecError::malformed)?,
            start,
            end,
            tolerance: PositiveReal::from_finite(*tolerance),
        });
        self.bind_topology(TextShapeKind::Edge, edge_use.shape, transform, id.as_str())?;
        self.cache_storage.with_storage(|| {
            let cached_id = id.try_clone_for_decode(self.ctx, "FreeCAD cached edge identity")?;
            key._storage.commit()?;
            self.ctx
                .insert_hash_map(&mut self.edges, key.data, cached_id, "FreeCAD cached edges")?;
            Ok::<(), CodecError>(())
        })?;
        Ok(id)
    }

    fn polygon_curve(
        &mut self,
        ir: &mut CadIr,
        edge: &EdgeId,
        ordinal: usize,
        representation: &TextEdgeRepresentation,
        transform: Transform,
    ) -> Result<CurveId, CodecError> {
        let carrier_transform = transform
            .compose(self.tables.location(representation.location())?)
            .map_err(location_transform_error)?;
        let scale = uniform_scale(carrier_transform)?;
        let mut sample_storage = self
            .ctx
            .reserve_scoped(0, "FreeCAD polygon sample inputs")?;
        let IndexedPolygon {
            mut samples,
            deflection,
        } = match representation {
            TextEdgeRepresentation::Polygon3d { polygon, .. } => {
                let polygon = &self.tables.polygons3d[polygon - 1];
                IndexedPolygon::try_new(
                    self.ctx,
                    if polygon.parameters.is_some() {
                        sample_storage.with_storage(|| {
                            self.ctx
                                .copy_slice(&polygon.nodes, "FreeCAD standalone polygon nodes")
                        })?
                    } else {
                        self.ctx
                            .copy_slice(&polygon.nodes, "FreeCAD standalone polygon nodes")?
                    },
                    polygon
                        .parameters
                        .as_ref()
                        .map(|parameters| {
                            sample_storage.with_storage(|| {
                                self.ctx
                                    .copy_slice(parameters, "FreeCAD standalone polygon parameters")
                            })
                        })
                        .transpose()?,
                    polygon.deflection,
                )?
            }
            TextEdgeRepresentation::PolygonOnTriangulation {
                polygon,
                triangulation,
                ..
            } => self.indexed_polygon(*polygon, *triangulation)?,
            TextEdgeRepresentation::PolygonPair {
                polygons,
                triangulation,
                ..
            } => self.indexed_polygon(polygons[0], *triangulation)?,
            _ => {
                return Err(CodecError::Malformed(
                    "non-polygon edge representation reached polygon transfer".into(),
                ))
            }
        };
        drop(sample_storage);
        let id = self.polygon_curve_id(edge, ordinal, false)?;
        self.ctx
            .reserve_vec(&mut ir.model.curves, 1, "FreeCAD curves records")?;
        ir.model.curves.push(Curve {
            id: id.try_clone_for_decode(self.ctx, "FreeCAD polygon curve record identity")?,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Polyline({
                place_polyline_samples(&mut samples, carrier_transform, self.ctx)?;
                PolylineCurve::from_scaled_deflection(samples, deflection, scale, self.ctx)?
                    .map_err(|error| CodecError::Malformed(error.to_string()))?
            })),
            source_object: Some(self.source_association()?),
        });
        self.geometry.index_curve(
            self.ctx,
            &ir.model.curves[ir.model.curves.len() - 1].id,
            ir.model.curves.len() - 1,
        )?;
        if let TextEdgeRepresentation::PolygonPair {
            polygons,
            triangulation,
            ..
        } = representation
        {
            let IndexedPolygon {
                mut samples,
                deflection,
            } = self.indexed_polygon(polygons[1], *triangulation)?;
            self.ctx
                .reserve_vec(&mut ir.model.curves, 1, "FreeCAD curves records")?;
            ir.model.curves.push(Curve {
                id: self.polygon_curve_id(edge, ordinal, true)?,
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Polyline({
                    place_polyline_samples(&mut samples, carrier_transform, self.ctx)?;
                    PolylineCurve::from_scaled_deflection(samples, deflection, scale, self.ctx)?
                        .map_err(|error| CodecError::Malformed(error.to_string()))?
                })),
                source_object: Some(self.source_association()?),
            });
            self.geometry.index_curve(
                self.ctx,
                &ir.model.curves[ir.model.curves.len() - 1].id,
                ir.model.curves.len() - 1,
            )?;
        }
        Ok(id)
    }

    fn polygon_curve_id(
        &self,
        edge: &EdgeId,
        ordinal: usize,
        secondary: bool,
    ) -> Result<CurveId, CodecError> {
        let id = if secondary {
            self.ctx.format_retained(
                format_args!("{}:polygon:{}:secondary", edge.as_str(), ordinal + 1),
                "FreeCAD secondary polygon curve identity",
            )?
        } else {
            self.ctx.format_retained(
                format_args!("{}:polygon:{}", edge.as_str(), ordinal + 1),
                "FreeCAD polygon curve identity",
            )?
        };
        CurveId::mint(id).map_err(CodecError::malformed)
    }

    fn indexed_polygon(
        &self,
        index: usize,
        triangulation_index: usize,
    ) -> Result<IndexedPolygon, CodecError> {
        let polygon = &self.tables.polygons_on_triangulations[index - 1];
        let triangulation = &self.tables.triangulations[triangulation_index - 1];
        let mut input_storage = self
            .ctx
            .reserve_scoped(0, "FreeCAD indexed polygon inputs")?;
        let mut points = if polygon.parameters.is_some() {
            input_storage.with_storage(|| {
                self.ctx
                    .collection_vec(polygon.nodes.len(), "FreeCAD indexed polygon points")
            })?
        } else {
            self.ctx
                .collection_vec(polygon.nodes.len(), "FreeCAD indexed polygon points")?
        };
        let mut nodes = polygon.nodes.iter();
        while nodes.len() != 0 {
            let Some(node) = self.ctx.next_charged(&mut nodes, "FreeCAD indexed polygon node scan")? else {
                break;
            };
            let point = usize::try_from(*node)
                .ok()
                .and_then(|node| node.checked_sub(1))
                .and_then(|node| triangulation.nodes().get(node).copied())
                .ok_or_else(|| {
                    CodecError::Malformed("polygon-on-triangulation node is out of bounds".into())
                })?;
            points.push(point);
        }
        let parameters = polygon
            .parameters
            .as_ref()
            .map(|parameters| {
                input_storage.with_storage(|| {
                    self.ctx
                        .copy_slice(parameters, "FreeCAD indexed polygon parameters")
                })
            })
            .transpose()?;
        IndexedPolygon::try_new(self.ctx, points, parameters, polygon.deflection)
    }

    fn polygon_parameters(&self, representation: &TextEdgeRepresentation) -> Option<&[FiniteReal]> {
        match representation {
            TextEdgeRepresentation::Polygon3d { polygon, .. } => {
                self.tables.polygons3d[polygon - 1].parameters.as_deref()
            }
            TextEdgeRepresentation::PolygonOnTriangulation { polygon, .. } => {
                self.tables.polygons_on_triangulations[polygon - 1]
                    .parameters
                    .as_deref()
            }
            TextEdgeRepresentation::PolygonPair { polygons, .. } => {
                self.tables.polygons_on_triangulations[polygons[0] - 1]
                    .parameters
                    .as_deref()
            }
            _ => None,
        }
    }

    fn ensure_vertex(
        &mut self,
        ir: &mut CadIr,
        vertex_use: &TextShapeUse,
        parent: Transform,
    ) -> Result<VertexId, CodecError> {
        let transform = parent
            .compose(self.tables.location(vertex_use.location)?)
            .map_err(location_transform_error)?;
        let (data, storage) =
            self.ctx
                .with_scoped_storage("FreeCAD occurrence lookup scratch", || {
                    OccurrenceKey::new(
                        self.ctx,
                        vertex_use.shape,
                        self.body_scope
                            .compose(transform)
                            .map_err(location_transform_error)?,
                    )
                })?;
        let key = ScopedData {
            data,
            _storage: storage,
        };
        if let Some(id) =
            self.ctx
                .get_hash_map(&self.vertices, &key.data, "FreeCAD cached vertex lookup")?
        {
            let id: VertexId =
                id.try_clone_for_decode(self.ctx, "FreeCAD cached vertex lookup identity")?;
            self.bind_topology(
                TextShapeKind::Vertex,
                vertex_use.shape,
                transform,
                id.as_str(),
            )?;
            return Ok(id);
        }
        let shape = self.shape(vertex_use.shape)?;
        let TextTShapeGeometry::Vertex {
            tolerance, point, ..
        } = shape.geometry
        else {
            return Err(CodecError::malformed(format_args!(
                "TShape {} is not a vertex",
                vertex_use.shape
            )));
        };
        let label = self.topology_label(vertex_use.shape, transform)?;
        let point_id = PointId::mint(crate::native::model_id_charged_at(
            self.ctx,
            "point",
            &self.payload.id,
            &label.data,
            "FreeCAD point identity",
        )?)
        .map_err(CodecError::malformed)?;
        let vertex_id = VertexId::mint(crate::native::model_id_charged_at(
            self.ctx,
            "vertex",
            &self.payload.id,
            &label.data,
            "FreeCAD vertex identity",
        )?)
        .map_err(CodecError::malformed)?;
        // A finite point and a finite location still multiply and add to a
        // non-finite coordinate, which states no position.
        let Some(position) = transform.apply_point(point.get()) else {
            return Err(CodecError::malformed(format_args!(
                "placed vertex {} position contains a non-finite coordinate",
                vertex_use.shape
            )));
        };
        self.ctx
            .reserve_vec(&mut ir.model.points, 1, "FreeCAD points records")?;
        ir.model.points.push(Point::new(
            point_id.try_clone_for_decode(self.ctx, "FreeCAD point record identity")?,
            position,
            Some(self.source_association()?),
        ));
        self.ctx
            .reserve_vec(&mut ir.model.vertices, 1, "FreeCAD vertices records")?;
        ir.model.vertices.push(Vertex {
            id: vertex_id.try_clone_for_decode(self.ctx, "FreeCAD vertex record identity")?,
            point: point_id,
            tolerance: positive_tolerance(tolerance.get() * uniform_scale(transform)?.get()),
        });
        self.bind_topology(
            TextShapeKind::Vertex,
            vertex_use.shape,
            transform,
            vertex_id.as_str(),
        )?;
        self.cache_storage.with_storage(|| {
            let cached_id = vertex_id.try_clone_for_decode(self.ctx, "FreeCAD cached vertex identity")?;
            key._storage.commit()?;
            self.ctx.insert_hash_map(
                &mut self.vertices,
                key.data,
                cached_id,
                "FreeCAD cached vertices",
            )?;
            Ok::<(), CodecError>(())
        })?;
        Ok(vertex_id)
    }

    fn located_curve(
        &mut self,
        ir: &mut CadIr,
        source: usize,
        transform: Transform,
    ) -> Result<CurveId, CodecError> {
        let build_base = || {
            let (data, storage) = self
                .ctx
                .format_scoped(format_args!("{source}"), "FreeCAD base curve key")?;
            let base_key = ScopedData {
                data,
                _storage: storage,
            };
            CurveId::mint(crate::native::model_id_charged_at(
                self.ctx,
                "curve",
                &self.payload.id,
                &base_key.data,
                "FreeCAD base curve identity",
            )?)
            .map_err(CodecError::malformed)
        };
        if is_identity(transform) {
            return build_base();
        }
        let (data, storage) = self.ctx.format_scoped(
            format_args!("{}@{}", source, LowerHex(&transform_digest(transform))),
            "FreeCAD located curve key",
        )?;
        let key = ScopedData {
            data,
            _storage: storage,
        };
        let id = CurveId::mint(crate::native::model_id_charged_at(
            self.ctx,
            "curve",
            &self.payload.id,
            &key.data,
            "FreeCAD located curve identity",
        )?)
        .map_err(CodecError::malformed)?;
        if !self
            .ctx
            .contains_hash_set(&self.emitted_curves, &id, "FreeCAD emitted curve lookup")?
        {
            let (data, storage) = self
                .ctx
                .with_scoped_storage("FreeCAD base geometry lookup", build_base)?;
            let base_id = ScopedData {
                data,
                _storage: storage,
            };
            self.storage.with_storage(|| {
                let cached_id = id.try_clone_for_decode(self.ctx, "FreeCAD emitted curve identity")?;
                self.ctx.insert_hash_set(
                    &mut self.emitted_curves,
                    cached_id,
                    "FreeCAD emitted curves",
                )?;
                Ok::<(), CodecError>(())
            })?;
            let position = self
                .geometry
                .curve_position(self.ctx, ir, &base_id.data)?
                .ok_or_else(|| {
                    CodecError::malformed(format_args!("missing curve table entry {source}"))
                })?;
            let base = &ir.model.curves[position];
            let geometry = transform_curve(self.ctx, &base.geometry, transform)?;
            let source_object = base
                .source_object
                .as_ref()
                .map(|source| {
                    (source).try_clone_for_decode(self.ctx, "FreeCAD geometry source association")
                })
                .transpose()?;
            self.ctx
                .reserve_vec(&mut ir.model.curves, 1, "FreeCAD curves records")?;
            ir.model.curves.push(Curve {
                id: id.try_clone_for_decode(self.ctx, "FreeCAD located curve record identity")?,
                geometry,
                source_object,
            });
            self.geometry.index_curve(
                self.ctx,
                &ir.model.curves[ir.model.curves.len() - 1].id,
                ir.model.curves.len() - 1,
            )?;
        }
        Ok(id)
    }

    fn located_surface(
        &mut self,
        ir: &mut CadIr,
        source: usize,
        transform: Transform,
    ) -> Result<SurfaceId, CodecError> {
        let build_base = || {
            let (data, storage) = self
                .ctx
                .format_scoped(format_args!("{source}"), "FreeCAD base surface key")?;
            let base_key = ScopedData {
                data,
                _storage: storage,
            };
            SurfaceId::mint(crate::native::model_id_charged_at(
                self.ctx,
                "surface",
                &self.payload.id,
                &base_key.data,
                "FreeCAD base surface identity",
            )?)
            .map_err(CodecError::malformed)
        };
        if is_identity(transform) {
            return build_base();
        }
        let (data, storage) = self.ctx.format_scoped(
            format_args!("{}@{}", source, LowerHex(&transform_digest(transform))),
            "FreeCAD located surface key",
        )?;
        let key = ScopedData {
            data,
            _storage: storage,
        };
        let id = SurfaceId::mint(crate::native::model_id_charged_at(
            self.ctx,
            "surface",
            &self.payload.id,
            &key.data,
            "FreeCAD located surface identity",
        )?)
        .map_err(CodecError::malformed)?;
        if !self.ctx.contains_hash_set(
            &self.emitted_surfaces,
            &id,
            "FreeCAD emitted surface lookup",
        )? {
            let (data, storage) = self
                .ctx
                .with_scoped_storage("FreeCAD base geometry lookup", build_base)?;
            let base_id = ScopedData {
                data,
                _storage: storage,
            };
            self.storage.with_storage(|| {
                let cached_id = id.try_clone_for_decode(self.ctx, "FreeCAD emitted surface identity")?;
                self.ctx.insert_hash_set(
                    &mut self.emitted_surfaces,
                    cached_id,
                    "FreeCAD emitted surfaces",
                )?;
                Ok::<(), CodecError>(())
            })?;
            let position = self
                .geometry
                .surface_position(self.ctx, ir, &base_id.data)?
                .ok_or_else(|| {
                    CodecError::malformed(format_args!("missing surface table entry {source}"))
                })?;
            let base = &ir.model.surfaces[position];
            let geometry = transform_surface(self.ctx, &base.geometry, transform)?;
            let source_object = base
                .source_object
                .as_ref()
                .map(|source| {
                    (source).try_clone_for_decode(self.ctx, "FreeCAD geometry source association")
                })
                .transpose()?;
            let has_procedural_construction =
                if let Some(construction) = base.geometry.procedural_construction() {
                    self.geometry.ensure_procedural(self.ctx, ir)?;
                    if let Some(indexes) = &self.geometry.procedural {
                        self.ctx
                            .get_btree_map(
                                &indexes.construction_owners,
                                construction,
                                "FreeCAD procedural owner lookup",
                            )?
                            .copied()
                            .flatten()
                            == Some(position)
                            && self.ctx.contains_btree_set(
                                &indexes.procedural_surfaces,
                                construction,
                                "FreeCAD procedural surface lookup",
                            )?
                    } else {
                        false
                    }
                } else {
                    false
                };
            self.ctx
                .reserve_vec(&mut ir.model.surfaces, 1, "FreeCAD surfaces records")?;
            ir.model.surfaces.push(Surface {
                id: id.try_clone_for_decode(self.ctx, "FreeCAD located surface record identity")?,
                geometry,
                source_object,
            });
            self.geometry.index_surface(
                self.ctx,
                &ir.model.surfaces[ir.model.surfaces.len() - 1].id,
                ir.model.surfaces.len() - 1,
            )?;
            if has_procedural_construction {
                ir.model
                    .add_procedural_surface(
                        self.ctx,
                        &id,
                        ProceduralSurface::new(
                            ProceduralSurfaceId::mint(self.ctx.format_retained(
                                format_args!("{id}:construction"),
                                "FreeCAD procedural construction identity",
                            )?)
                            .map_err(CodecError::malformed)?,
                            ProceduralSurfaceDefinition::Replica {
                                source: base_id.data.try_clone_for_decode(
                                    self.ctx,
                                    "FreeCAD procedural surface source identity",
                                )?,
                                transform,
                            },
                            None,
                        ),
                    )?
                    .map_err(|error| CodecError::malformed(error.to_string()))?;
                let procedural_position = ir.model.procedural_surfaces.len() - 1;
                let construction = &ir.model.procedural_surfaces[procedural_position].id;
                if let Some(indexes) = &mut self.geometry.procedural {
                    indexes.storage.with_storage(|| {
                        self.ctx.insert_btree_set(
                            &mut indexes.procedural_surfaces,
                            construction
                                .try_clone_for_decode(self.ctx, "FreeCAD procedural surface key")?,
                            "FreeCAD procedural surface index",
                        )?;
                        self.ctx.insert_btree_map(
                            &mut indexes.construction_owners,
                            construction
                                .try_clone_for_decode(self.ctx, "FreeCAD procedural owner key")?,
                            Some(ir.model.surfaces.len() - 1),
                            "FreeCAD procedural owners",
                        )?;
                        Ok::<(), CodecError>(())
                    })?;
                }
            }
        }
        Ok(id)
    }

    fn face_pcurve(
        &mut self,
        edge_use: &TextShapeUse,
        edge_transform: Transform,
        surface: Option<crate::brep::TableRef<TextSurface>>,
        surface_transform: Transform,
    ) -> Result<Option<FacePcurve>, CodecError> {
        let Some(surface) = surface else {
            return Ok(None);
        };
        let TextTShapeGeometry::Edge {
            degenerated,
            representations,
            ..
        } = &self.tables.tshapes[edge_use.shape - 1].geometry
        else {
            return Ok(None);
        };
        let Some((index, representation)) = select_pcurve_representation(
            self.ctx,
            representations,
            &self.tables,
            edge_transform,
            surface.index(),
            surface_transform,
        )?
        else {
            return Ok(None);
        };
        let reversed = is_reversed(edge_use.orientation);
        let (curve_index, parameter_range, secondary) = match representation {
            TextEdgeRepresentation::Pcurve {
                curve,
                parameter_range,
                ..
            } => (*curve, *parameter_range, false),
            TextEdgeRepresentation::PcurvePair {
                curves,
                parameter_range,
                ..
            } => {
                let secondary = reversed;
                (
                    if secondary { curves[1] } else { curves[0] },
                    *parameter_range,
                    secondary,
                )
            }
            _ => return Ok(None),
        };
        let domain = match self.ctx.get_btree_map(
            &self.pcurve_availability,
            &curve_index,
            "FreeCAD pcurve availability lookup",
        )?.copied() {
            Some(PcurveAvailability::Available(domain)) => domain,
            Some(PcurveAvailability::Unavailable { loss_index }) => {
                let message = self.ctx.copy_retained_text(
                    &self.losses[loss_index].message,
                    "FreeCAD pcurve loss",
                )?;
                self.ctx.push_vec(
                    &mut self.losses,
                    FreecadLossCode::PcurveNotTransferred.note(message),
                    "FreeCAD pcurve losses",
                )?;
                return Ok(None);
            }
            None => {
                let (read, read_storage) =
                    self.ctx
                        .with_scoped_storage("FreeCAD face pcurve domain", || {
                            Ok::<_, CodecError>(pcurve_geometry(
                                self.ctx,
                                &self.tables.curve2ds[curve_index - 1],
                            ))
                        })?;
                let read = match read {
                    Ok(geometry) => geometry,
                    Err(PcurveGeometryError::Resource(error)) => return Err(error),
                    Err(error) => {
                        let loss_index = self.losses.len();
                        self.ctx
                            .reserve_vec(&mut self.losses, 1, "FreeCAD pcurve losses")?;
                        self.losses.push(pcurve_loss(
                            self.ctx,
                            &self.payload.id,
                            curve_index,
                            Some(&error),
                        )?);
                        drop(error);
                        drop(read_storage);
                        self.remember_pcurve_availability(
                            curve_index,
                            PcurveAvailability::Unavailable { loss_index },
                        )?;
                        return Ok(None);
                    }
                };
                let Some(geometry) = read else {
                    let loss_index = self.losses.len();
                    self.ctx
                        .reserve_vec(&mut self.losses, 1, "FreeCAD pcurve losses")?;
                    self.losses
                        .push(pcurve_loss(self.ctx, &self.payload.id, curve_index, None)?);
                    drop(read_storage);
                    self.remember_pcurve_availability(
                        curve_index,
                        PcurveAvailability::Unavailable { loss_index },
                    )?;
                    return Ok(None);
                };
                let domain = pcurve_parameter_domain(self.ctx, &geometry)?;
                drop(geometry);
                drop(read_storage);
                self.remember_pcurve_availability(curve_index, PcurveAvailability::Available(domain))?;
                domain
            }
        };
        let parameter_range = normalize_pcurve_domain_range(domain, Some(parameter_range));
        Ok(Some((
            self.pcurve_id(edge_use.shape, index, secondary)?,
            bounded_pcurve_range(*degenerated, parameter_range),
        )))
    }

    fn shape(&self, index: usize) -> Result<&'a TextTShape, CodecError> {
        self.tables.tshapes.resolve(index)
    }

    fn topology_label(
        &self,
        shape: usize,
        local: Transform,
    ) -> Result<ScopedData<'c, String>, CodecError> {
        let transform = self
            .body_scope
            .compose(local)
            .map_err(location_transform_error)?;
        let (data, storage) = if is_identity(transform) {
            self.ctx
                .format_scoped(format_args!("{shape}"), "FreeCAD topology label")?
        } else {
            self.ctx.format_scoped(
                format_args!("{}@{}", shape, LowerHex(&transform_digest(transform))),
                "FreeCAD topology label",
            )?
        };
        let label = ScopedData {
            data,
            _storage: storage,
        };
        match self.root_discriminator {
            Some(ordinal) => {
                let (data, storage) = self.ctx.format_scoped(
                    format_args!("{}~root{ordinal}", label.data),
                    "FreeCAD topology root label",
                )?;
                Ok(ScopedData {
                    data,
                    _storage: storage,
                })
            }
            None => Ok(label),
        }
    }
}

fn bounded_pcurve_range(
    degenerated: bool,
    range: Option<[FiniteReal; 2]>,
) -> Option<[FiniteReal; 2]> {
    (!degenerated)
        .then_some(range)
        .flatten()
        .filter(|range| range[0] < range[1])
}

fn pcurve_parameter_domain(
    ctx: &DecodeContext<'_>,
    geometry: &PcurveGeometry,
) -> Result<Option<[f64; 2]>, CodecError> {
    let mut bases = std::iter::successors(Some(geometry), |geometry| match geometry {
        PcurveGeometry::Offset(offset) => Some(offset.basis()),
        PcurveGeometry::Transformed(placed) => Some(placed.basis()),
        _ => None,
    });
    while let Some(geometry) = ctx.next_charged(&mut bases, "FreeCAD pcurve domain scan")? {
        match geometry {
            PcurveGeometry::Nurbs { nurbs } => {
                let Ok(degree) = usize::try_from(nurbs.degree()) else {
                    return Ok(None);
                };
                let Some(end) = nurbs.knots().len().checked_sub(degree + 1) else {
                    return Ok(None);
                };
                let Some((&start, &end)) = nurbs.knots().get(degree).zip(nurbs.knots().get(end)) else {
                    return Ok(None);
                };
                return Ok(Some([start, end]));
            }
            PcurveGeometry::Trimmed(trimmed_pcurve) => {
                let parameter_range = trimmed_pcurve.parameter_range();
                return Ok(Some(parameter_range.endpoints()));
            }
            PcurveGeometry::Offset(_) | PcurveGeometry::Transformed(_) => {}
            _ => return Ok(None),
        }
    }
    Ok(None)
}

#[cfg(test)]
fn normalize_pcurve_parameter_range(
    geometry: &PcurveGeometry,
    range: Option<[FiniteReal; 2]>,
) -> Option<[FiniteReal; 2]> {
    let domain = pcurve_parameter_domain(
        &cadmpeg_test_support::service_decode_context(),
        geometry,
    ).expect("test pcurve domain admission");
    normalize_pcurve_domain_range(domain, range)
}

fn normalize_pcurve_domain_range(
    domain: Option<[f64; 2]>,
    range: Option<[FiniteReal; 2]>,
) -> Option<[FiniteReal; 2]> {
    let mut range = range?;
    let Some(domain) = domain else {
        return Some(range);
    };
    let scale = range
        .into_iter()
        .map(FiniteReal::get)
        .chain(domain)
        .fold(1.0_f64, |scale, value| scale.max(value.abs()));
    // Snap neighborhoods must not overlap, even on a small or translated domain.
    let tolerance =
        (scale * EPS_TOPOLOGY_TRANSFER_GEOMETRY).min((0.25 * domain[1] - 0.25 * domain[0]).abs());
    for value in &mut range {
        if (domain[0]..=domain[1]).contains(&value.get()) {
            continue;
        }
        let lower_distance = (value.get() - domain[0]).abs();
        let upper_distance = (value.get() - domain[1]).abs();
        if lower_distance < upper_distance && lower_distance <= tolerance {
            *value = FiniteReal::new(domain[0])?;
        } else if upper_distance < lower_distance && upper_distance <= tolerance {
            *value = FiniteReal::new(domain[1])?;
        }
    }
    Some(range)
}

fn connected_components(
    ctx: &DecodeContext<'_>,
    connectivity: &[BTreeSet<String>],
) -> Result<Vec<Vec<usize>>, CodecError> {
    let (assigned, assignment_storage) =
        ctx.with_scoped_storage("FreeCAD connected-component assignment", || {
            ctx.alloc_filled(
                connectivity.len(),
                false,
                "freecad connected-component assignment",
            )
        })?;
    let mut assigned = ScopedVec {
        values: assigned,
        _storage: assignment_storage,
    };
    let mut key_storage = ctx.reserve_scoped(0, "FreeCAD connected-component key index")?;
    let mut by_key: BTreeMap<&String, ScopedVec<'_, usize>> = BTreeMap::new();
    let mut faces = connectivity.iter().enumerate();
    while faces.len() != 0 {
        let Some((face, keys)) = ctx.next_charged(
            &mut faces,
            "FreeCAD connected-component face keys",
        )? else {
            break;
        };
        let mut keys = keys.iter();
        while keys.len() != 0 {
            let Some(key) = ctx.next_charged(
                &mut keys,
                "FreeCAD connected-component key scan",
            )? else {
                break;
            };
            if let Some(candidates) = ctx.get_mut_btree_map(
                &mut by_key,
                &key,
                "FreeCAD connected-component key index",
            )? {
                candidates._storage.with_storage(|| {
                    ctx.push_vec(
                        &mut candidates.values,
                        face,
                        "FreeCAD connected-component key index",
                    )
                })?;
            } else {
                let (mut values, storage) =
                    ctx.temporary_vec(1, "FreeCAD connected-component key index")?;
                values.push(face);
                let candidates = ScopedVec {
                    values,
                    _storage: storage,
                };
                key_storage.with_storage(|| {
                    ctx.insert_btree_map(
                        &mut by_key,
                        key,
                        candidates,
                        "FreeCAD connected-component key index",
                    )
                })?;
            }
        }
    }
    let mut components = Vec::new();
    let mut seeds = 0..connectivity.len();
    while seeds.len() != 0 {
        let Some(seed) = ctx.next_charged(
            &mut seeds,
            "FreeCAD connected-component seeds",
        )? else {
            break;
        };
        if assigned.values[seed] {
            continue;
        }
        assigned.values[seed] = true;
        let mut component = Vec::new();
        let (mut values, storage) =
            ctx.temporary_vec(1, "FreeCAD connected-component stack")?;
        values.push(seed);
        let mut stack = ScopedVec {
            values,
            _storage: storage,
        };
        while !stack.values.is_empty() {
            ctx.charge_work(1, "FreeCAD connected-component members")?;
            let Some(current) = stack.values.pop() else {
                break;
            };
            ctx.reserve_vec(&mut component, 1, "FreeCAD connected-component members")?;
            component.push(current);
            let mut member_keys = connectivity[current].iter();
            while member_keys.len() != 0 {
                let Some(key) = ctx.next_charged(
                    &mut member_keys,
                    "FreeCAD connected-component member keys",
                )? else {
                    break;
                };
                let Some(mut candidates) = ctx.remove_btree_map(
                    &mut by_key,
                    &key,
                    "FreeCAD connected-component comparison",
                )?
                else {
                    continue;
                };
                let mut candidate_iter = std::mem::take(&mut candidates.values).into_iter();
                while candidate_iter.len() != 0 {
                    let Some(candidate) = ctx.next_charged(
                        &mut candidate_iter,
                        "FreeCAD connected-component candidates",
                    )? else {
                        break;
                    };
                    if assigned.values[candidate] {
                        continue;
                    }
                    assigned.values[candidate] = true;
                    stack._storage.with_storage(|| {
                        ctx.reserve_vec(
                            &mut stack.values,
                            1,
                            "FreeCAD connected-component stack",
                        )
                    })?;
                    stack.values.push(candidate);
                }
                drop(candidate_iter);
                drop(candidates);
            }
        }
        ctx.stable_sort_by(
            &mut component,
            |value| value,
            Ord::cmp,
            "FreeCAD connected-component members sort",
        )?;
        ctx.reserve_vec(&mut components, 1, "FreeCAD connected components")?;
        components.push(component);
    }
    Ok(components)
}

/// The pcurve placed onto its support's parameter frame.
///
/// The affine coefficients come off the document, so a scale or offset that
/// drives one non-finite is a source the transform carrier refuses; `None`
/// then states that the placement is not representable, rather than panicking.
fn transformed_pcurve_geometry(
    ctx: &DecodeContext<'_>,
    geometry: PcurveGeometry,
    affine: Option<SurfaceParameterAffine>,
) -> Result<Option<PcurveGeometry>, CodecError> {
    let Some(affine) = affine else {
        return Ok(Some(geometry));
    };
    if affine
        == (SurfaceParameterAffine {
            u_scale: 1.0,
            u_offset: 0.0,
            v_scale: 1.0,
            v_offset: 0.0,
        })
    {
        return Ok(Some(geometry));
    }
    let Some(transform) = Transform2::affine([
        [affine.u_scale, 0.0, affine.u_offset],
        [0.0, affine.v_scale, affine.v_offset],
    ]) else {
        return Ok(None);
    };
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(std::mem::size_of::<PcurveGeometry>()),
        "FreeCAD pcurve placement basis",
    )?;
    Ok(
        cadmpeg_ir::geometry::pcurve::PlacedPcurve::try_new(Box::new(geometry), transform)
            .ok()
            .map(PcurveGeometry::Transformed),
    )
}

fn positive_tolerance(value: f64) -> Option<cadmpeg_ir::scalar::PositiveReal> {
    cadmpeg_ir::scalar::PositiveReal::new(value)
}

#[derive(Debug)]
pub(crate) enum PcurveGeometryError {
    Nurbs(NurbsError),
    Resource(CodecError),
}

fn pcurve_loss(
    ctx: &DecodeContext<'_>,
    payload_id: &str,
    curve_index: usize,
    error: Option<&PcurveGeometryError>,
) -> Result<LossNote, CodecError> {
    let message = match error {
        Some(error) => ctx.format_retained(format_args!(
            "payload {payload_id} curve2ds index {curve_index} could not enter neutral geometry: {error}"
        ), "FreeCAD pcurve loss")?,
        None => ctx.format_retained(format_args!(
            "payload {payload_id} curve2ds index {curve_index} could not enter neutral geometry"
        ), "FreeCAD pcurve loss")?,
    };
    Ok(FreecadLossCode::PcurveNotTransferred.note(message))
}

impl std::fmt::Display for PcurveGeometryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Nurbs(error) => error.fmt(formatter),
            Self::Resource(error) => error.fmt(formatter),
        }
    }
}

impl From<NurbsError> for PcurveGeometryError {
    fn from(error: NurbsError) -> Self {
        Self::Nurbs(error)
    }
}

impl From<CodecError> for PcurveGeometryError {
    fn from(error: CodecError) -> Self {
        Self::Resource(error)
    }
}

impl From<cadmpeg_core::decode::ResourceLimit> for PcurveGeometryError {
    fn from(error: cadmpeg_core::decode::ResourceLimit) -> Self {
        Self::Resource(error.into())
    }
}

impl From<PcurveGeometryError> for CodecError {
    fn from(error: PcurveGeometryError) -> Self {
        match error {
            PcurveGeometryError::Nurbs(error) => error.into(),
            PcurveGeometryError::Resource(error) => error,
        }
    }
}

/// Read a 2D curve record into neutral geometry. `Ok(None)` states a record
/// the neutral model cannot carry. `Nurbs` errors state refused B-spline lanes.
/// `Resource` errors can occur for any curve kind.
pub(crate) fn pcurve_geometry(
    ctx: &DecodeContext<'_>,
    curve: &TextCurve2d,
) -> Result<Option<PcurveGeometry>, PcurveGeometryError> {
    let _depth = ctx.enter_nested("FreeCAD pcurve geometry nesting")?;
    Ok(match curve {
        TextCurve2d::Line { origin, direction } => NonzeroPoint2::new(direction.get())
            .map(|direction| cadmpeg_ir::geometry::pcurve::LinePcurve::new(*origin, direction))
            .map(PcurveGeometry::Line),
        TextCurve2d::Circle {
            center,
            x_axis,
            y_axis,
            radius,
        } => PositiveReal::from_finite(*radius)
            .and_then(|radius| {
                cadmpeg_ir::geometry::pcurve::CirclePcurve::from_parts(
                    *center, *x_axis, *y_axis, radius,
                )
            })
            .map(PcurveGeometry::Circle),
        TextCurve2d::Ellipse {
            center,
            x_axis,
            y_axis,
            major_radius,
            minor_radius,
        } => PositiveReal::from_finite(*major_radius)
            .zip(PositiveReal::from_finite(*minor_radius))
            .and_then(|(major_radius, minor_radius)| {
                cadmpeg_ir::geometry::pcurve::EllipsePcurve::from_parts(
                    *center,
                    *x_axis,
                    *y_axis,
                    major_radius,
                    minor_radius,
                )
            })
            .map(PcurveGeometry::Ellipse),
        TextCurve2d::Parabola {
            vertex,
            x_axis,
            y_axis,
            focal_distance,
        } => PositiveReal::from_finite(*focal_distance)
            .and_then(|focal_distance| {
                cadmpeg_ir::geometry::pcurve::ParabolaPcurve::from_parts(
                    *vertex,
                    *x_axis,
                    *y_axis,
                    focal_distance,
                )
            })
            .map(PcurveGeometry::Parabola),
        TextCurve2d::Hyperbola {
            center,
            x_axis,
            y_axis,
            major_radius,
            minor_radius,
        } => PositiveReal::from_finite(*major_radius)
            .zip(PositiveReal::from_finite(*minor_radius))
            .and_then(|(major_radius, minor_radius)| {
                cadmpeg_ir::geometry::pcurve::HyperbolaPcurve::from_parts(
                    *center,
                    *x_axis,
                    *y_axis,
                    major_radius,
                    minor_radius,
                )
            })
            .map(PcurveGeometry::Hyperbola),
        TextCurve2d::Nurbs(nurbs) => {
            let poles = if let Some(weights) = &nurbs.weights {
                if weights.len() != nurbs.control_points.len() {
                    return Err(NurbsError::WeightLaneLength {
                        field: "pcurve poles".into(),
                        poles: nurbs.control_points.len(),
                        weights: weights.len(),
                    }
                    .into());
                }
                let mut rows = ctx
                    .collection_vec(nurbs.control_points.len(), "FreeCAD pcurve rational poles")?;
                let mut poles = nurbs.control_points.iter().zip(weights).enumerate();
                while poles.len() != 0 {
                    let Some((index, (point, weight))) = ctx.next_charged(
                        &mut poles,
                        "FreeCAD pcurve rational pole scan",
                    )? else {
                        break;
                    };
                    let Some(weight) = NonZeroReal::from_finite(*weight) else {
                        return Err(NurbsError::UnusableWeight {
                            field: "pcurve poles".into(),
                            index,
                            weight: weight.get(),
                        }
                        .into());
                    };
                    rows.push(WeightedPole2 {
                        point: *point,
                        weight,
                    });
                }
                PcurveNurbsPoles::Rational { points: rows }
            } else {
                PcurveNurbsPoles::Polynomial {
                    points: ctx
                        .copy_slice(&nurbs.control_points, "FreeCAD pcurve polynomial poles")?,
                }
            };
            let mut knots = ctx.collection_vec(nurbs.knots.len(), "FreeCAD pcurve knots")?;
            knots.extend(
                ctx.admit_iter(&nurbs.knots, "FreeCAD pcurve knot scan")?
                    .map(|knot| knot.get()),
            );
            Some(PcurveGeometry::Nurbs {
                nurbs: PcurveNurbs::new(
                    ctx,
                    nurbs.degree,
                    cadmpeg_ir::geometry::nurbs::KnotVector::new(ctx, knots)??,
                    poles,
                    nurbs.periodic,
                )??,
            })
        }
        TextCurve2d::Trimmed {
            parameter_range,
            basis,
        } => {
            let Some(basis) = pcurve_geometry(ctx, basis.curve())? else {
                return Ok(None);
            };
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(std::mem::size_of::<PcurveGeometry>()),
                "FreeCAD trimmed pcurve basis",
            )?;
            cadmpeg_ir::geometry::pcurve::TrimmedPcurve::from_finite_parts(
                *parameter_range,
                true,
                Box::new(basis),
            )
            .ok()
            .map(PcurveGeometry::Trimmed)
        }
        TextCurve2d::Offset { distance, basis } => {
            let Some(basis) = pcurve_geometry(ctx, basis.curve())? else {
                return Ok(None);
            };
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(std::mem::size_of::<PcurveGeometry>()),
                "FreeCAD offset pcurve basis",
            )?;
            cadmpeg_ir::geometry::pcurve::OffsetPcurve::from_finite_parts(
                *distance,
                Box::new(basis),
            )
            .ok()
            .map(PcurveGeometry::Offset)
        }
    })
}

fn ensure_similarity(transform: Transform) -> Result<(), CodecError> {
    uniform_scale(transform).map(|_| ())
}

fn uniform_scale(transform: Transform) -> Result<cadmpeg_ir::scalar::PositiveReal, CodecError> {
    let columns = transform.linear_columns();
    let scale = cadmpeg_ir::scalar::PositiveReal::new(columns[0].norm()).ok_or_else(|| {
        CodecError::Malformed("B-rep location is not a finite similarity transform".into())
    })?;
    let lengths = columns.map(|column| column.norm());
    let units = columns.map(cadmpeg_ir::features::FiniteVector3::unit_nonzero);
    if lengths.iter().any(|length| {
        !length.is_finite()
            || (*length / scale.get() - 1.0).abs() > EPS_TOPOLOGY_TRANSFER_DEGENERATE
    }) || units.iter().any(Option::is_none)
        || units[0]
            .zip(units[1])
            .is_some_and(|(a, b)| a.dot(b).abs() > EPS_TOPOLOGY_TRANSFER_DEGENERATE)
        || units[0]
            .zip(units[2])
            .is_some_and(|(a, b)| a.dot(b).abs() > EPS_TOPOLOGY_TRANSFER_DEGENERATE)
        || units[1]
            .zip(units[2])
            .is_some_and(|(a, b)| a.dot(b).abs() > EPS_TOPOLOGY_TRANSFER_DEGENERATE)
    {
        return Err(CodecError::Malformed(
            "B-rep location is not a finite similarity transform".into(),
        ));
    }
    Ok(scale)
}

fn transform_curve(
    ctx: &DecodeContext<'_>,
    geometry: &CurveGeometry,
    transform: Transform,
) -> Result<CurveGeometry, CodecError> {
    ensure_similarity(transform)?;
    let solved = geometry.solved().ok_or_else(|| {
        cadmpeg_core::CodecError::NotImplemented("carrier has no solved geometry".into())
    })?;
    let basis = solved.try_clone_for_decode(ctx, "FreeCAD NURBS curve copy")?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(std::mem::size_of::<SolvedCurveGeometry>()),
        "FreeCAD curve placement basis",
    )?;
    Ok(CurveGeometry::Solved(SolvedCurveGeometry::Transformed(
        cadmpeg_ir::geometry::PlacedCurve::try_new(Box::new(basis), transform)
            .map_err(cadmpeg_core::CodecError::malformed)?,
    )))
}

fn transform_surface(
    ctx: &DecodeContext<'_>,
    geometry: &SurfaceGeometry,
    transform: Transform,
) -> Result<SurfaceGeometry, CodecError> {
    ensure_similarity(transform)?;
    let solved = geometry.solved().ok_or_else(|| {
        cadmpeg_core::CodecError::NotImplemented("carrier has no solved geometry".into())
    })?;
    let basis = solved.try_clone_for_decode(ctx, "FreeCAD NURBS surface copy")?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(std::mem::size_of::<SolvedSurfaceGeometry>()),
        "FreeCAD surface placement basis",
    )?;
    Ok(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Transformed(
        cadmpeg_ir::geometry::PlacedSurface::try_new(Box::new(basis), transform)
            .map_err(cadmpeg_core::CodecError::malformed)?,
    )))
}

/// Places every polyline sample, refusing a sample the transform sends out of
/// the finite range.
fn place_polyline_samples(
    samples: &mut PolylineSamples<FiniteReal, FinitePoint3>,
    transform: Transform,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    samples.edit_admitted_points(
        |point| match transform.apply_point(point.get()) {
            Some(point) => Ok(point),
            None => Err(CodecError::Malformed(ctx.copy_retained_text(
                "placed polyline sample contains a non-finite coordinate",
                "FreeCAD polyline placement refusal",
            )?)),
        },
        ctx,
    )??;
    Ok(())
}

fn transform_normalized_vector(transform: Transform, vector: Vector3) -> Option<FiniteVector3> {
    UnitVector3::normalized_nonzero(transform.apply_vector(vector)?).map(FiniteVector3::from)
}

#[cfg(test)]
fn occurrence_label(shape: usize, transform: Transform) -> String {
    if is_identity(transform) {
        shape.to_string()
    } else {
        format!("{}@{}", shape, LowerHex(&transform_digest(transform)))
    }
}

fn source_topology_indices(
    ctx: &DecodeContext<'_>,
    tables: Tables<'_>,
) -> Result<HashMap<(TextShapeKind, SourceOccurrenceKey), usize>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut indices = HashMap::new();
    for target in [
        TextShapeKind::Vertex,
        TextShapeKind::Edge,
        TextShapeKind::Wire,
        TextShapeKind::Face,
        TextShapeKind::Shell,
        TextShapeKind::Solid,
        TextShapeKind::CompSolid,
        TextShapeKind::Compound,
    ] {
        let mut next_index = 1;
        let mut roots = tables.roots.iter();
        while roots.len() != 0 {
            let Some(root) = ctx.next_charged(
                &mut roots,
                "FreeCAD source topology roots",
            )? else {
                break;
            };
            let (values, storage) =
                ctx.temporary_vec(1, "FreeCAD source topology stack")?;
            let mut stack = ScopedVec {
                values,
                _storage: storage,
            };
            stack
                .values
                .push((root.clone(), Transform::identity()));
            while !stack.values.is_empty() {
                ctx.charge_work(1, "FreeCAD source topology scan")?;
                let Some((shape_use, parent)) = stack.values.pop() else {
                    break;
                };
                let transform = parent
                    .compose(tables.location(shape_use.location)?)
                    .map_err(location_transform_error)?;
                let shape = &tables.tshapes[shape_use.shape - 1];
                if shape.kind() == target {
                    let (data, storage) =
                        ctx.with_scoped_storage("FreeCAD source occurrence scratch", || {
                            Ok::<_, CodecError>((
                                target,
                                SourceOccurrenceKey::new(ctx, shape_use.shape, transform)?,
                            ))
                        })?;
                    let key = ScopedData {
                        data,
                        _storage: storage,
                    };
                    if !ctx.contains_key_hash_map(
                        &indices,
                        &key.data,
                        "FreeCAD source topology lookup",
                    )? {
                        key._storage.commit()?;
                        ctx.insert_hash_map(
                            &mut indices,
                            key.data,
                            next_index,
                            "FreeCAD source topology index",
                        )?;
                        next_index += 1;
                    }
                    continue;
                }
                if topology_rank(shape.kind()) < topology_rank(target) {
                    stack._storage.with_storage(|| {
                        ctx.reserve_vec(
                            &mut stack.values,
                            shape.children.len(),
                            "FreeCAD source topology stack",
                        )
                    })?;
                    stack.values.extend(
                        ctx.admit_iter(&shape.children, "FreeCAD source topology children")?
                            .rev()
                            .cloned()
                            .map(|child| (child, transform)),
                    );
                }
            }
        }
    }
    Ok(indices)
}

fn topology_rank(kind: TextShapeKind) -> u8 {
    match kind {
        TextShapeKind::Compound => 0,
        TextShapeKind::CompSolid => 1,
        TextShapeKind::Solid => 2,
        TextShapeKind::Shell => 3,
        TextShapeKind::Face => 4,
        TextShapeKind::Wire => 5,
        TextShapeKind::Edge => 6,
        TextShapeKind::Vertex => 7,
    }
}

fn indexed_name(kind: TextShapeKind) -> &'static str {
    match kind {
        TextShapeKind::Vertex => "Vertex",
        TextShapeKind::Edge => "Edge",
        TextShapeKind::Wire => "Wire",
        TextShapeKind::Face => "Face",
        TextShapeKind::Shell => "Shell",
        TextShapeKind::Solid => "Solid",
        TextShapeKind::CompSolid => "CompSolid",
        TextShapeKind::Compound => "Compound",
    }
}

fn transform_digest(transform: Transform) -> [u8; 8] {
    // TopLoc_Location equality is exact; neutral identities must not merge
    // source-distinct placements at a decoder tolerance boundary.
    let mut bytes = [0; std::mem::size_of::<[[f64; 4]; 4]>()];
    for (slot, value) in bytes
        .chunks_exact_mut(8)
        .zip(transform.rows().into_iter().flatten())
    {
        let canonical = if value == 0.0 { 0.0 } else { value };
        slot.copy_from_slice(&canonical.to_bits().to_le_bytes());
    }
    let mut prefix = [0; 8];
    prefix.copy_from_slice(&sha256(&bytes)[..8]);
    prefix
}

fn is_identity(transform: Transform) -> bool {
    exact_transforms_equal(transform, Transform::identity())
}

fn exact_transforms_equal(left: Transform, right: Transform) -> bool {
    left.rows()
        .into_iter()
        .flatten()
        .zip(right.rows().into_iter().flatten())
        .all(|(left, right)| left == right)
}

fn is_reversed(orientation: TextOrientation) -> bool {
    orientation == TextOrientation::Reversed
}

fn sense(reversed: bool) -> Sense {
    if reversed {
        Sense::Reversed
    } else {
        Sense::Forward
    }
}

fn close_radial_rings(ctx: &DecodeContext<'_>, coedges: &mut [Coedge]) -> Result<(), CodecError> {
    let (data, storage) = ctx.collect_scoped_btree_groups(
        coedges
            .iter()
            .enumerate()
            .map(|(index, coedge)| (coedge.edge.as_str(), index)),
        "FreeCAD radial edge index",
    )?;
    let by_edge = ScopedData {
        data,
        _storage: storage,
    };
    let (values, storage) = ctx.temporary_vec(0, "FreeCAD radial pairs")?;
    let mut pairs = ScopedVec {
        values,
        _storage: storage,
    };
    let mut groups = by_edge.data.iter();
    while groups.len() != 0 {
        let Some((_, indices)) = ctx.next_charged(
            &mut groups,
            "FreeCAD radial groups",
        )? else {
            break;
        };
        if let [first, second] = indices.as_slice() {
            pairs._storage.with_storage(|| {
                ctx.push_vec(&mut pairs.values, (*first, *second), "FreeCAD radial pairs")
            })?;
        }
    }
    drop(by_edge);
    let mut pair_iter = pairs.values.iter().copied();
    while pair_iter.len() != 0 {
        let Some((first, second)) = ctx.next_charged(
            &mut pair_iter,
            "FreeCAD radial pairs",
        )? else {
            break;
        };
        coedges[first].radial_next = coedges[second]
            .id
            .try_clone_for_decode(ctx, "FreeCAD radial coedge identity")?;
        coedges[second].radial_next = coedges[first]
            .id
            .try_clone_for_decode(ctx, "FreeCAD radial coedge identity")?;
    }
    Ok(())
}

fn edge_endpoint_uses<'a>(
    ctx: &DecodeContext<'_>,
    edge: usize,
    children: &'a [TextShapeUse],
) -> Result<(&'a TextShapeUse, &'a TextShapeUse), CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut start = None;
    let mut end = None;
    let mut children = children.iter();
    while children.len() != 0 {
        let Some(child) = ctx.next_charged(
            &mut children,
            "FreeCAD edge endpoint search",
        )? else {
            break;
        };
        match child.orientation {
            TextOrientation::Forward => {
                if start.replace(child).is_some() {
                    return Err(CodecError::malformed(format_args!(
                        "edge TShape {edge} has multiple forward endpoint uses"
                    )));
                }
            }
            TextOrientation::Reversed => {
                if end.replace(child).is_some() {
                    return Err(CodecError::malformed(format_args!(
                        "edge TShape {edge} has multiple reversed endpoint uses"
                    )));
                }
            }
            TextOrientation::Internal | TextOrientation::External => {}
        }
    }
    start.zip(end).ok_or_else(|| {
        CodecError::malformed(format_args!(
            "edge TShape {edge} does not have both forward and reversed endpoint uses"
        ))
    })
}

fn select_pcurve_representation<'a>(
    ctx: &DecodeContext<'_>,
    representations: &'a [TextEdgeRepresentation],
    tables: &Tables<'_>,
    edge_transform: Transform,
    surface: usize,
    surface_transform: Transform,
) -> Result<Option<(usize, &'a TextEdgeRepresentation)>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut matched = None;
    let mut representations = representations.iter().enumerate();
    while representations.len() != 0 {
        let Some((index, representation)) = ctx.next_charged(
            &mut representations,
            "FreeCAD pcurve representation search",
        )? else {
            break;
        };
        match representation {
            TextEdgeRepresentation::Pcurve {
                surface: candidate_surface,
                location,
                ..
            }
            | TextEdgeRepresentation::PcurvePair {
                surface: candidate_surface,
                location,
                ..
            } if *candidate_surface == surface
                && exact_transforms_equal(
                    edge_transform
                        .compose(tables.location(*location)?)
                        .map_err(location_transform_error)?,
                    surface_transform,
                ) =>
            {
                matched = Some((index, representation));
                break;
            }
            _ => {}
        }
    }
    Ok(matched)
}

fn select_exact_curve_representation<'a>(
    ctx: &DecodeContext<'_>,
    edge: usize,
    representations: &'a [TextEdgeRepresentation],
    tables: &Tables<'_>,
) -> Result<Option<(usize, &'a TextEdgeRepresentation)>, CodecError> {
    let Some(position) = ctx.position_by(
        representations,
        |representation| {
            Ok(matches!(
                representation,
                TextEdgeRepresentation::Curve3d { .. }
            ))
        },
        "FreeCAD exact curve representation search",
    )?
    else {
        return Ok(None);
    };
    let first = (position, &representations[position]);
    if !ctx.all_by(
        &representations[position + 1..],
        |representation| {
            Ok(
                !matches!(representation, TextEdgeRepresentation::Curve3d { .. })
                    || equivalent_exact_curve_representation(first.1, representation, tables),
            )
        },
        "FreeCAD exact curve representation comparison",
    )? {
        return Err(CodecError::malformed(format_args!(
            "edge TShape {edge} has non-equivalent 3D curve representations"
        )));
    }
    Ok(Some(first))
}

fn equivalent_exact_curve_representation(
    left: &TextEdgeRepresentation,
    right: &TextEdgeRepresentation,
    tables: &Tables<'_>,
) -> bool {
    let (
        TextEdgeRepresentation::Curve3d {
            curve: left_curve,
            location: left_location,
            parameter_range: left_range,
        },
        TextEdgeRepresentation::Curve3d {
            curve: right_curve,
            location: right_location,
            parameter_range: right_range,
        },
    ) = (left, right)
    else {
        return false;
    };
    let Some(left_curve) = left_curve.checked_sub(1) else {
        return false;
    };
    let Some(right_curve) = right_curve.checked_sub(1) else {
        return false;
    };
    tables.curves.get(left_curve) == tables.curves.get(right_curve)
        && tables
            .location(*left_location)
            .ok()
            .zip(tables.location(*right_location).ok())
            .is_some_and(|(left, right)| left == right)
        && left_range == right_range
}

fn unique_fallback_polygon_representation<'a>(
    ctx: &DecodeContext<'_>,
    edge: usize,
    representations: &'a [TextEdgeRepresentation],
) -> Result<Option<(usize, &'a TextEdgeRepresentation)>, CodecError> {
    let predicate = |representation: &TextEdgeRepresentation| {
        Ok(matches!(
            representation,
            TextEdgeRepresentation::Polygon3d { .. }
                | TextEdgeRepresentation::PolygonOnTriangulation { .. }
                | TextEdgeRepresentation::PolygonPair { .. }
        ))
    };
    let Some(position) = ctx.position_by(
        representations,
        predicate,
        "FreeCAD fallback polygon representation search",
    )?
    else {
        return Ok(None);
    };
    let first = (position, &representations[position]);
    if ctx.any_by(
        &representations[position + 1..],
        predicate,
        "FreeCAD fallback polygon representation search",
    )? {
        return Err(CodecError::malformed(format_args!(
            "edge TShape {edge} has multiple fallback polygon representations"
        )));
    }
    Ok(Some(first))
}

pub(crate) fn normalize_occt_curve_range(
    geometry: &SolvedCurveGeometry,
    range: Option<[FiniteReal; 2]>,
) -> Option<[FiniteReal; 2]> {
    match geometry {
        SolvedCurveGeometry::Circle(_) | SolvedCurveGeometry::Ellipse(_) => {
            let [start, end] = range?;
            let sweep = end.get() - start.get();
            let tau = std::f64::consts::TAU;
            if !sweep.is_finite() || (sweep - tau).abs() <= EPS_TOPOLOGY_TRANSFER_GEOMETRY {
                return Some([start, end]);
            }
            let canonical_start = start.get().rem_euclid(tau);
            let canonical_start =
                if (tau - canonical_start).abs() <= EPS_TOPOLOGY_TRANSFER_EXACT_GEOMETRY {
                    0.0
                } else {
                    canonical_start
                };
            Some([
                FiniteReal::new(canonical_start)?,
                FiniteReal::new(canonical_start + sweep)?,
            ])
        }
        SolvedCurveGeometry::Parabola(parabola_curve) => {
            let focal_distance = parabola_curve.focal_distance().magnitude();
            let [start, end] = range?;
            Some([
                cadmpeg_ir::math::multiply_divide(start, FiniteReal::HALF, focal_distance)?,
                cadmpeg_ir::math::multiply_divide(end, FiniteReal::HALF, focal_distance)?,
            ])
        }
        SolvedCurveGeometry::Transformed(placed) => {
            normalize_occt_curve_range(placed.basis(), range)
        }
        _ => range,
    }
}

#[cfg(test)]
pub(crate) mod tests;

#[cfg(test)]
mod admission_tests;

#[cfg(test)]
mod numerical_range_tests;

#[cfg(test)]
mod repair_tests;
