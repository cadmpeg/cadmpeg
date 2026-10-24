// SPDX-License-Identifier: Apache-2.0
//! Neutral B-rep of one exact stream.
//!
//! One neutral face is written per face region (§8.19). Carriers come from
//! [`frames`]; loops, sides, and vertices from [`topology`]. Neutral loops
//! run with the stored coedge order of a face, or against it when the face
//! is turned (see [`topology::resolve`]); coedge senses and pcurve ranges
//! follow that traversal.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::{AnnotationBuilder, StreamHandle};
use cadmpeg_ir::document::Model;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::pcurve::{Pcurve, PcurveGeometry, PcurveMetadata};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry as IrCurveGeometry, DirectedParameterRange, SolvedCurveGeometry,
    SolvedSurfaceGeometry, Surface, SurfaceGeometry as IrSurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, IdentityKey, LoopId, PcurveId, PointId, RegionId,
    ShellId, SurfaceId, VertexId,
};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{FiniteReal, PositiveReal};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, EdgeCarrier, Face, FaceLoops, Loop, LoopBoundary, LoopRing,
    ParameterInterval, PcurveUse, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::Exactness;

use super::eval::{cross, curve_interval, dot, scale};
use super::frames::{self, CarrierRefusal, CurveChart, SurfaceChart, UvMap, MILLIMETRES_PER_METRE};
use super::model::{ExactModel, SurfaceGeometry, Vec2, Vec3};
use super::resolve::{domain_point, PcurveSlot, ENDPOINT_TOLERANCE};
use super::topology::{self, Topology};
use crate::loss::PvzLossCode;

/// Factor between a measured endpoint residual and the face tolerance that
/// admits it.
const RESIDUAL_MARGIN: f64 = 1.5;
/// Factor between the spread of the ends joined into a vertex and the
/// vertex tolerance.
const SPREAD_MARGIN: f64 = 1.5;

/// Destination of the neutral records of one exact stream.
pub(crate) struct Sink<'s> {
    /// Neutral model the records are appended to.
    pub(crate) model: &'s mut Model,
    /// Exactness and provenance of the records.
    pub(crate) annotations: &'s mut AnnotationBuilder,
    /// Losses of the stream.
    pub(crate) losses: &'s mut Vec<LossNote>,
    /// Source stream that provenance offsets refer to.
    pub(crate) stream: &'s StreamHandle,
}

/// Identities of the records of one exact stream, keyed under `scope`.
struct Ids<'k> {
    scope: &'k IdentityKey,
}

macro_rules! minter {
    ($name:ident, $ty:ty, $kind:literal) => {
        fn $name(&self, key: impl Into<IdentityKey>) -> $ty {
            <$ty>::compose(
                &cadmpeg_ir::identity_namespace!("pvz", "model", $kind),
                self.scope.clone().colon(key),
            )
        }
    };
}

impl Ids<'_> {
    minter!(surface, SurfaceId, "surface");
    minter!(face, FaceId, "face");
    minter!(loop_, LoopId, "loop");
    minter!(coedge, CoedgeId, "coedge");
    minter!(edge, EdgeId, "edge");
    minter!(curve, CurveId, "curve");
    minter!(pcurve, PcurveId, "pcurve");
    minter!(vertex, VertexId, "vertex");
    minter!(point, PointId, "point");
    minter!(shell, ShellId, "shell");
    minter!(region, RegionId, "region");

    fn body(&self) -> BodyId {
        BodyId::compose(
            &cadmpeg_ir::identity_namespace!("pvz", "model", "body"),
            self.scope.clone(),
        )
    }

    fn pair(first: usize, second: usize) -> IdentityKey {
        IdentityKey::from(first).colon(second)
    }
}

/// Arena lengths before the stream's records were appended.
#[derive(Debug, Clone, Copy)]
struct Arenas {
    loops: usize,
    coedges: usize,
    faces: usize,
}

/// How the coedges of one face carry pcurves into its neutral surface.
#[derive(Debug, Clone, Copy, PartialEq)]
enum PcurveTransfer {
    /// The pcurves of the face slot, through this map.
    Mapped(UvMap),
    /// No pcurve is written.
    Omitted,
}

/// The neutral carrier of one face.
struct FaceCarrier {
    geometry: SolvedSurfaceGeometry,
    transfer: PcurveTransfer,
    /// Sign of the domain normal against the neutral surface normal.
    side: f64,
}

/// Counts that become one loss note per code.
#[derive(Default)]
struct LossTally {
    counts: BTreeMap<PvzLossCode, usize>,
}

impl LossTally {
    fn add(&mut self, code: PvzLossCode, count: usize) {
        if count > 0 {
            *self.counts.entry(code).or_default() += count;
        }
    }

    fn notes(self, losses: &mut Vec<LossNote>) {
        for (code, count) in self.counts {
            losses.push(code.note(code.tally_message(count)));
        }
    }
}

fn finite_point(value: Vec3) -> Result<FinitePoint3, CodecError> {
    let [x, y, z] = scale(value, MILLIMETRES_PER_METRE);
    FinitePoint3::new(Point3::new(x, y, z))
        .ok_or_else(|| CodecError::malformed("exact-stream vertex is not finite"))
}

fn annotation_error(error: impl std::fmt::Display) -> CodecError {
    CodecError::malformed(format_args!("pvz exact annotation: {error}"))
}

/// Writes the neutral B-rep of `exact` and returns its body, or `None` when
/// the stream has no face. The body is a solid when every shell is closed,
/// a sheet when none is, and general otherwise.
pub(crate) fn emit(
    ctx: &DecodeContext<'_>,
    exact: &ExactModel,
    scope: &IdentityKey,
    sink: &mut Sink<'_>,
) -> Result<Option<BodyId>, CodecError> {
    emit_with_work(ctx, exact, scope, sink, topology::work_limit(exact))
}

/// [`emit`] with the derived-search work limit `work`.
fn emit_with_work(
    ctx: &DecodeContext<'_>,
    exact: &ExactModel,
    scope: &IdentityKey,
    sink: &mut Sink<'_>,
    work: usize,
) -> Result<Option<BodyId>, CodecError> {
    let mut tally = LossTally::default();
    tally.add(PvzLossCode::ExactRecordUninterpreted, exact.opaque.len());
    if exact.faces.is_empty() {
        tally.notes(sink.losses);
        return Ok(None);
    }
    let topology = topology::resolve(exact, work)?;
    let ids = Ids { scope };
    let mut writer = Writer {
        ctx,
        exact,
        topology: &topology,
        ids,
        sink,
        tally,
    };
    let body = writer.write()?;
    let Writer { tally, sink, .. } = writer;
    tally.notes(sink.losses);
    Ok(Some(body))
}

struct Writer<'w, 's, 'c, 'k> {
    ctx: &'w DecodeContext<'c>,
    exact: &'w ExactModel,
    topology: &'w Topology,
    ids: Ids<'k>,
    sink: &'w mut Sink<'s>,
    tally: LossTally,
}

impl Writer<'_, '_, '_, '_> {
    fn derived(&mut self, id: &impl std::fmt::Display) {
        self.sink
            .annotations
            .exactness(id.to_string(), Exactness::Derived);
    }

    fn located(&mut self, id: &impl std::fmt::Display, offset: usize, tag: &str) {
        self.sink.annotations.note(
            id.to_string(),
            self.sink.stream,
            cadmpeg_core::decode::u64_from_index(offset),
            Some(tag),
        );
        self.derived(id);
    }

    fn write(&mut self) -> Result<BodyId, CodecError> {
        let first = Arenas {
            loops: self.sink.model.loops.len(),
            coedges: self.sink.model.coedges.len(),
            faces: self.sink.model.faces.len(),
        };
        let carriers = self.write_surfaces()?;
        let used_edges = self.used_edges();
        let charts = self.write_curves(&used_edges)?;
        let used_vertices = self.write_edges(&used_edges, &charts)?;
        self.write_vertices(&used_vertices)?;
        let faces = self.write_faces(&carriers)?;
        self.write_shells(&faces, first)
    }

    /// The neutral carrier of every face, written to the surface arena.
    fn write_surfaces(&mut self) -> Result<Vec<FaceCarrier>, CodecError> {
        let mut carriers = Vec::new();
        for (index, face) in self.exact.faces.iter().enumerate() {
            let carrier = self.face_carrier(index)?;
            let id = self.ids.surface(index);
            let geometry = match &carrier {
                Some(carrier) => carrier.geometry.clone(),
                None => SolvedSurfaceGeometry::Unknown { record: None },
            };
            self.ctx.charge_entities(1, "pvz exact surface")?;
            self.sink.model.surfaces.push(Surface {
                id: id.clone(),
                geometry: IrSurfaceGeometry::Solved(geometry),
                source_object: None,
            });
            self.sink.annotations.note(
                id.to_string(),
                self.sink.stream,
                cadmpeg_core::decode::u64_from_index(face.offset),
                Some("0x13"),
            );
            let exactness = if carrier.is_some() {
                Exactness::Derived
            } else {
                Exactness::Unknown
            };
            self.sink.annotations.exactness(id.to_string(), exactness);
            carriers.push(carrier.unwrap_or(FaceCarrier {
                geometry: SolvedSurfaceGeometry::Unknown { record: None },
                transfer: PcurveTransfer::Omitted,
                side: 1.0,
            }));
        }
        Ok(carriers)
    }

    /// Chooses the neutral carrier of a face and how its pcurves transfer.
    fn face_carrier(&mut self, index: usize) -> Result<Option<FaceCarrier>, CodecError> {
        let face = &self.exact.faces[index];
        let slot = self.topology.faces[index].slot;
        let embedded = || face.surface.embedded.as_ref().map(frames::bspline_chart);
        let edgeless = face.loops.iter().any(|loop_index| {
            self.exact.loops[*loop_index]
                .coedges
                .iter()
                .any(|coedge| coedge.edge.is_none())
        });
        let pcurve_count: usize = face
            .loops
            .iter()
            .map(|loop_index| self.exact.loops[*loop_index].coedges.len())
            .sum();
        let identity = |chart: SurfaceChart| FaceCarrier {
            geometry: chart.geometry,
            transfer: PcurveTransfer::Mapped(UvMap::IDENTITY),
            side: 1.0,
        };
        Ok(match (&face.surface.geometry, slot) {
            (SurfaceGeometry::Undefined { .. }, _) => {
                self.tally.add(PvzLossCode::ExactSurfaceUntyped, 1);
                None
            }
            (SurfaceGeometry::Bspline(surface), _) => match frames::bspline_chart(surface) {
                Ok(chart) => {
                    if slot.is_none() {
                        self.tally
                            .add(PvzLossCode::ExactPcurveUntransferred, pcurve_count);
                        Some(FaceCarrier {
                            transfer: PcurveTransfer::Omitted,
                            ..identity(chart)
                        })
                    } else {
                        Some(identity(chart))
                    }
                }
                Err(_) => {
                    self.tally.add(PvzLossCode::ExactSurfaceUntyped, 1);
                    None
                }
            },
            (geometry, Some(PcurveSlot::Native)) => match frames::analytic_chart(geometry) {
                Ok(chart) => {
                    let [[a, b], [c, d]] = chart.map.linear;
                    let side = if a * d - b * c < 0.0 { -1.0 } else { 1.0 };
                    Some(FaceCarrier {
                        geometry: chart.geometry,
                        transfer: PcurveTransfer::Mapped(chart.map),
                        side,
                    })
                }
                Err(refusal) => self.embedded_fallback(index, refusal, embedded()),
            },
            (geometry, Some(PcurveSlot::Secondary)) => {
                if edgeless {
                    return Ok(self.embedded_fallback(index, CarrierRefusal::Untyped, embedded()));
                }
                match frames::analytic_chart(geometry) {
                    Ok(chart) => {
                        self.tally
                            .add(PvzLossCode::ExactSecondaryPcurveOmitted, pcurve_count);
                        let side = self.secondary_side(index, &chart)?;
                        Some(FaceCarrier {
                            geometry: chart.geometry,
                            transfer: PcurveTransfer::Omitted,
                            side,
                        })
                    }
                    Err(refusal) => self.embedded_fallback(index, refusal, embedded()),
                }
            }
            (geometry, None) => {
                self.tally
                    .add(PvzLossCode::ExactPcurveUntransferred, pcurve_count);
                match frames::analytic_chart(geometry) {
                    Ok(chart) => Some(FaceCarrier {
                        geometry: chart.geometry,
                        transfer: PcurveTransfer::Omitted,
                        side: 1.0,
                    }),
                    Err(_) => {
                        self.tally.add(PvzLossCode::ExactSurfaceUntyped, 1);
                        None
                    }
                }
            }
        })
    }

    /// The embedded B-spline as the face carrier, with the secondary pcurves.
    fn embedded_fallback(
        &mut self,
        index: usize,
        refusal: CarrierRefusal,
        embedded: Option<Result<SurfaceChart, CarrierRefusal>>,
    ) -> Option<FaceCarrier> {
        let secondary = self.topology.faces[index].slot == Some(PcurveSlot::Secondary);
        match embedded {
            Some(Ok(chart)) if secondary => {
                if refusal != CarrierRefusal::Untyped {
                    self.tally.add(PvzLossCode::ExactSurfaceAsBspline, 1);
                }
                Some(FaceCarrier {
                    geometry: chart.geometry,
                    transfer: PcurveTransfer::Mapped(UvMap::IDENTITY),
                    side: 1.0,
                })
            }
            Some(Ok(chart)) => {
                self.tally.add(PvzLossCode::ExactSurfaceAsBspline, 1);
                let pcurves: usize = self.exact.faces[index]
                    .loops
                    .iter()
                    .map(|loop_index| self.exact.loops[*loop_index].coedges.len())
                    .sum();
                self.tally
                    .add(PvzLossCode::ExactPcurveUntransferred, pcurves);
                Some(FaceCarrier {
                    geometry: chart.geometry,
                    transfer: PcurveTransfer::Omitted,
                    side: 1.0,
                })
            }
            _ => {
                self.tally.add(PvzLossCode::ExactSurfaceUntyped, 1);
                None
            }
        }
    }

    /// Sign of the embedded B-spline normal against the neutral analytic
    /// normal, at a loop sample of the face.
    fn secondary_side(&self, index: usize, chart: &SurfaceChart) -> Result<f64, CodecError> {
        let face = &self.exact.faces[index];
        let sample = self.topology.faces[index]
            .loops
            .iter()
            .find_map(|loop_| loop_.polygon.as_ref()?.points.first().copied());
        let Some(uv) = sample else {
            return Ok(1.0);
        };
        let (Some(point), Some(normal)) = (
            domain_point(face, PcurveSlot::Secondary, uv),
            topology::domain_normal(self.exact, index, PcurveSlot::Secondary, uv),
        ) else {
            return Ok(1.0);
        };
        let geometry = IrSurfaceGeometry::Solved(chart.geometry.clone());
        let [x, y, z] = scale(point, MILLIMETRES_PER_METRE);
        let Some(parameters) =
            cadmpeg_ir::eval::analytic_surface_parameters(&geometry, Point3::new(x, y, z))
        else {
            return Ok(1.0);
        };
        let parameters = parameters.get();
        let partials = match cadmpeg_ir::eval::surface_partials(
            self.ctx,
            &geometry,
            parameters.u,
            parameters.v,
        ) {
            Ok(partials) => partials.into_raw(),
            Err(failure) => {
                failure.non_finite().map_err(CodecError::ResourceLimit)?;
                return Ok(1.0);
            }
        };
        let raw = |vector: cadmpeg_ir::math::Vector3| [vector.x, vector.y, vector.z];
        let neutral = cross(raw(partials.du), raw(partials.dv));
        Ok(if dot(normal, neutral) < 0.0 {
            -1.0
        } else {
            1.0
        })
    }

    /// Edges referenced by coedges, in index order.
    fn used_edges(&self) -> BTreeSet<usize> {
        self.exact
            .loops
            .iter()
            .flat_map(|record| record.coedges.iter().filter_map(|coedge| coedge.edge))
            .collect()
    }

    fn write_curves(
        &mut self,
        used: &BTreeSet<usize>,
    ) -> Result<BTreeMap<usize, CurveChart>, CodecError> {
        let mut charts = BTreeMap::new();
        for edge in used {
            let record = &self.exact.edges[*edge];
            let chart = if self.topology.singular_edges.contains(edge) {
                let vertex = self.topology.slot_vertex[2 * edge];
                let point = finite_point(self.topology.vertices[vertex])?;
                Ok(CurveChart {
                    geometry: SolvedCurveGeometry::Degenerate(
                        cadmpeg_ir::geometry::analytic::DegenerateCurve::new(point),
                    ),
                    parameter: [1.0, 0.0],
                })
            } else {
                frames::edge_curve(&record.geometry)
            };
            let Ok(chart) = chart else {
                self.tally.add(PvzLossCode::ExactCurveUntyped, 1);
                continue;
            };
            let id = self.ids.curve(*edge);
            self.ctx.charge_entities(1, "pvz exact curve")?;
            self.sink.model.curves.push(Curve {
                id: id.clone(),
                geometry: IrCurveGeometry::Solved(chart.geometry.clone()),
                parameter_range: None,
                source_object: None,
            });
            self.located(&id, record.offset, "0x14");
            charts.insert(*edge, chart);
        }
        Ok(charts)
    }

    /// Writes edges and returns the vertices they bound.
    fn write_edges(
        &mut self,
        used: &BTreeSet<usize>,
        charts: &BTreeMap<usize, CurveChart>,
    ) -> Result<BTreeSet<usize>, CodecError> {
        let mut vertices = BTreeSet::new();
        for edge in used {
            let record = &self.exact.edges[*edge];
            let start = self.topology.slot_vertex[2 * edge];
            let end = self.topology.slot_vertex[2 * edge + 1];
            let carrier = match charts.get(edge) {
                Some(_) if self.topology.singular_edges.contains(edge) => {
                    EdgeCarrier::Curve(self.ids.curve(*edge))
                }
                Some(chart) => {
                    match curve_interval(&record.geometry).map(|range| chart.interval(range)) {
                        Some(range) if range[0] <= range[1] => {
                            let interval =
                                ParameterInterval::new(range).map_err(CodecError::malformed)?;
                            EdgeCarrier::Bounded(self.ids.curve(*edge), interval)
                        }
                        _ => EdgeCarrier::Curve(self.ids.curve(*edge)),
                    }
                }
                None => EdgeCarrier::Free,
            };
            let id = self.ids.edge(*edge);
            self.ctx.charge_entities(1, "pvz exact edge")?;
            self.sink.model.edges.push(Edge {
                id: id.clone(),
                carrier,
                start: self.ids.vertex(start),
                end: self.ids.vertex(end),
                tolerance: None,
            });
            self.located(&id, record.offset, "0x14");
            vertices.extend([start, end]);
        }
        for ((loop_index, position), base) in &self.topology.free_slots {
            let start = self.topology.slot_vertex[*base];
            let end = self.topology.slot_vertex[base + 1];
            let id = self
                .ids
                .edge(cadmpeg_ir::identity_key!("free").colon(Ids::pair(*loop_index, *position)));
            self.ctx.charge_entities(1, "pvz exact edge")?;
            self.sink.model.edges.push(Edge {
                id: id.clone(),
                carrier: EdgeCarrier::Free,
                start: self.ids.vertex(start),
                end: self.ids.vertex(end),
                tolerance: None,
            });
            self.derived(&id);
            vertices.extend([start, end]);
        }
        Ok(vertices)
    }

    fn write_vertices(&mut self, used: &BTreeSet<usize>) -> Result<(), CodecError> {
        for vertex in used {
            let point_id = self.ids.point(*vertex);
            let position = finite_point(self.topology.vertices[*vertex])?;
            let tolerance = (self.topology.vertex_spread[*vertex] * SPREAD_MARGIN)
                .max(ENDPOINT_TOLERANCE)
                * MILLIMETRES_PER_METRE;
            self.ctx.charge_entities(2, "pvz exact vertex")?;
            self.sink
                .model
                .points
                .push(Point::new(point_id.clone(), position, None));
            self.derived(&point_id);
            let id = self.ids.vertex(*vertex);
            self.sink.model.vertices.push(Vertex {
                id: id.clone(),
                point: point_id,
                tolerance: PositiveReal::new(tolerance),
            });
            self.derived(&id);
            self.sink
                .annotations
                .field_exactness(id.to_string(), "tolerance", Exactness::Inferred)
                .map_err(annotation_error)?;
        }
        Ok(())
    }

    /// Writes loops, coedges, pcurves, and faces. Returns, per face record,
    /// the neutral faces of its regions.
    fn write_faces(&mut self, carriers: &[FaceCarrier]) -> Result<Vec<Vec<FaceId>>, CodecError> {
        let mut faces = Vec::new();
        let mut radial: BTreeMap<EdgeId, Vec<CoedgeId>> = BTreeMap::new();
        let mut written: BTreeMap<CoedgeId, usize> = BTreeMap::new();
        let mut open_loops = 0;
        let mut mismatched = 0;
        let mut gauged = 0;
        for (index, carrier) in carriers.iter().enumerate() {
            let face_topology = &self.topology.faces[index];
            let record = &self.exact.faces[index];
            let reversed = face_topology.reversed;
            let traversal = if reversed { -1.0 } else { 1.0 };
            let sense = if traversal * face_topology.winding * carrier.side > 0.0 {
                Sense::Forward
            } else {
                Sense::Reversed
            };
            if !face_topology.side_derived {
                gauged += 1;
            }
            let regions: Vec<Vec<usize>> = if face_topology.regions.is_empty() {
                vec![Vec::new()]
            } else {
                face_topology
                    .regions
                    .iter()
                    .map(|region| region.loops.clone())
                    .collect()
            };
            let mut ids = Vec::new();
            for (region_index, members) in regions.iter().enumerate() {
                let face_id = self.ids.face(Ids::pair(index, region_index));
                let mut loop_ids = Vec::new();
                let mut residual: f64 = 0.0;
                for position in members {
                    let loop_topology = &face_topology.loops[*position];
                    if loop_topology.gap.is_none_or(|gap| gap > ENDPOINT_TOLERANCE) {
                        open_loops += 1;
                    }
                    let loop_id = self.ids.loop_(loop_topology.index);
                    let count = loop_topology.coedges.len();
                    let order: Vec<usize> = if reversed {
                        (0..count).rev().collect()
                    } else {
                        (0..count).collect()
                    };
                    let mut ring = Vec::new();
                    for coedge_position in order {
                        let geometry = &loop_topology.coedges[coedge_position];
                        let source =
                            &self.exact.loops[loop_topology.index].coedges[coedge_position];
                        let key = Ids::pair(loop_topology.index, coedge_position);
                        let coedge_id = self.ids.coedge(key.clone());
                        let edge_id = match source.edge {
                            Some(edge) => self.ids.edge(edge),
                            None => self
                                .ids
                                .edge(cadmpeg_ir::identity_key!("free").colon(key.clone())),
                        };
                        let along = loop_topology.along[coedge_position] != reversed;
                        let mut pcurves = Vec::new();
                        if let (PcurveTransfer::Mapped(map), Some((pcurve, _))) =
                            (carrier.transfer, geometry.pcurve)
                        {
                            let native = &self.exact.pcurves[pcurve];
                            match frames::pcurve(&native.geometry, &map) {
                                Ok(pcurve_geometry) => {
                                    let runs =
                                        loop_topology.directions[coedge_position] != reversed;
                                    pcurves.push(self.write_pcurve(
                                        key.clone(),
                                        pcurve_geometry,
                                        native.offset,
                                        geometry.interval,
                                        runs,
                                    )?);
                                    if let Some(value) = geometry.residual {
                                        residual = residual.max(value);
                                    }
                                }
                                Err(_) => self.tally.add(PvzLossCode::ExactPcurveUntransferred, 1),
                            }
                        }
                        self.ctx.charge_entities(1, "pvz exact coedge")?;
                        written.insert(coedge_id.clone(), self.sink.model.coedges.len());
                        self.sink.model.coedges.push(Coedge {
                            id: coedge_id.clone(),
                            owner_loop: loop_id.clone(),
                            edge: edge_id.clone(),
                            radial_next: coedge_id.clone(),
                            sense: if along {
                                Sense::Forward
                            } else {
                                Sense::Reversed
                            },
                            pcurves,
                            use_curve: None,
                        });
                        self.located(&coedge_id, source.offset, "0x11");
                        radial.entry(edge_id).or_default().push(coedge_id.clone());
                        ring.push(coedge_id);
                    }
                    let boundary = LoopRing::new(ring, Vec::new()).map_err(|error| {
                        CodecError::malformed(format_args!("pvz exact loop: {error}"))
                    })?;
                    self.ctx.charge_entities(1, "pvz exact loop")?;
                    self.sink.model.loops.push(Loop {
                        id: loop_id.clone(),
                        face: face_id.clone(),
                        boundary: LoopBoundary::Ring(boundary),
                    });
                    let offset = self.exact.loops[loop_topology.index].offset;
                    self.located(&loop_id, offset, "0x01000010");
                    loop_ids.push(loop_id);
                }
                let classified = face_topology
                    .regions
                    .get(region_index)
                    .is_some_and(|region| region.classified);
                let loops = match loop_ids.split_first() {
                    Some((outer, inner)) if classified => {
                        FaceLoops::classified(outer.clone(), inner.to_vec())
                    }
                    _ => FaceLoops::unspecified(loop_ids),
                };
                let tolerance = if residual > ENDPOINT_TOLERANCE {
                    mismatched += 1;
                    PositiveReal::new(residual * RESIDUAL_MARGIN * MILLIMETRES_PER_METRE)
                } else {
                    None
                };
                self.ctx.charge_entities(1, "pvz exact face")?;
                self.sink.model.faces.push(Face {
                    id: face_id.clone(),
                    shell: self.ids.shell(0),
                    surface: self.ids.surface(index),
                    sense,
                    loops,
                    name: None,
                    color: None,
                    tolerance,
                });
                self.located(&face_id, record.offset, "0x13");
                if !face_topology.side_derived {
                    self.sink
                        .annotations
                        .field_exactness(face_id.to_string(), "sense", Exactness::Inferred)
                        .map_err(annotation_error)?;
                }
                if tolerance.is_some() {
                    self.sink
                        .annotations
                        .field_exactness(face_id.to_string(), "tolerance", Exactness::Derived)
                        .map_err(annotation_error)?;
                }
                ids.push(face_id);
            }
            faces.push(ids);
        }
        for ring in radial.values() {
            for (position, id) in ring.iter().enumerate() {
                let next = ring[(position + 1) % ring.len()].clone();
                if let Some(coedge) = written
                    .get(id)
                    .and_then(|index| self.sink.model.coedges.get_mut(*index))
                {
                    coedge.radial_next = next;
                }
            }
        }
        self.tally.add(
            PvzLossCode::ExactLoopOpen,
            open_loops + self.topology.split_junctions,
        );
        self.tally
            .add(PvzLossCode::ExactEndpointMismatch, mismatched);
        self.tally.add(PvzLossCode::ExactFaceSideGauge, gauged);
        self.tally.add(
            PvzLossCode::ExactFaceSideGauge,
            self.topology.side_conflicts,
        );
        self.tally.add(
            PvzLossCode::ExactTopologyWorkExhausted,
            self.topology.work_exhausted,
        );
        Ok(faces)
    }

    fn write_pcurve(
        &mut self,
        key: IdentityKey,
        geometry: PcurveGeometry,
        offset: usize,
        interval: Option<Vec2>,
        runs: bool,
    ) -> Result<PcurveUse, CodecError> {
        let id = self.ids.pcurve(key);
        self.ctx.charge_entities(1, "pvz exact pcurve")?;
        self.sink.model.pcurves.push(Pcurve {
            id: id.clone(),
            geometry,
            metadata: PcurveMetadata::default(),
        });
        self.located(&id, offset, "0x15");
        let parameter_range = interval.and_then(|[start, end]| {
            let ends = if runs { [start, end] } else { [end, start] };
            let ends = [FiniteReal::new(ends[0])?, FiniteReal::new(ends[1])?];
            DirectedParameterRange::from_finite_endpoints(ends).ok()
        });
        Ok(PcurveUse {
            pcurve: id,
            isoparametric: None,
            parameter_range,
        })
    }

    /// Groups neutral faces into shells by shared edges and writes shells,
    /// regions, and the body.
    fn write_shells(&mut self, faces: &[Vec<FaceId>], first: Arenas) -> Result<BodyId, CodecError> {
        let flat: Vec<FaceId> = faces.iter().flatten().cloned().collect();
        let position: BTreeMap<&FaceId, usize> = flat
            .iter()
            .enumerate()
            .map(|(index, id)| (id, index))
            .collect();
        let mut parent: Vec<usize> = (0..flat.len()).collect();
        let loop_face: BTreeMap<LoopId, usize> = self.sink.model.loops[first.loops..]
            .iter()
            .filter_map(|loop_| Some((loop_.id.clone(), *position.get(&loop_.face)?)))
            .collect();
        let mut edge_faces: BTreeMap<EdgeId, Vec<usize>> = BTreeMap::new();
        for coedge in &self.sink.model.coedges[first.coedges..] {
            if let Some(face) = loop_face.get(&coedge.owner_loop) {
                edge_faces
                    .entry(coedge.edge.clone())
                    .or_default()
                    .push(*face);
            }
        }
        for list in edge_faces.values() {
            for pair in list.windows(2) {
                join(&mut parent, pair[0], pair[1]);
            }
        }
        let mut shell_of_root: BTreeMap<usize, usize> = BTreeMap::new();
        let mut members: Vec<Vec<usize>> = Vec::new();
        for face in 0..flat.len() {
            let root = root(&mut parent, face);
            let shell = *shell_of_root.entry(root).or_insert_with(|| {
                members.push(Vec::new());
                members.len() - 1
            });
            members[shell].push(face);
        }
        let singular: BTreeSet<EdgeId> = self
            .topology
            .singular_edges
            .iter()
            .map(|edge| self.ids.edge(*edge))
            .collect();
        let mut closed_shells = 0;
        let body = self.ids.body();
        let mut regions = Vec::new();
        let mut shell_of_face = vec![0; flat.len()];
        for (shell, list) in members.iter().enumerate() {
            let set: BTreeSet<usize> = list.iter().copied().collect();
            let closed = edge_faces.iter().all(|(edge, uses)| {
                let inside = uses.iter().filter(|face| set.contains(face)).count();
                inside == 0 || singular.contains(edge) || (inside == 2 && uses.len() == 2)
            });
            if closed {
                closed_shells += 1;
            }
            let shell_id = self.ids.shell(shell);
            let region_id = self.ids.region(shell);
            let face_ids: Vec<FaceId> = list.iter().map(|face| flat[*face].clone()).collect();
            for face in list {
                shell_of_face[*face] = shell;
            }
            self.ctx.charge_entities(2, "pvz exact shell")?;
            self.sink.model.shells.push(
                Shell::with_faces(shell_id.clone(), region_id.clone(), face_ids).map_err(
                    |error| CodecError::malformed(format_args!("pvz exact shell: {error}")),
                )?,
            );
            self.derived(&shell_id);
            self.sink.model.regions.push(Region {
                id: region_id.clone(),
                body: body.clone(),
                shells: vec![shell_id],
            });
            self.derived(&region_id);
            regions.push(region_id);
        }
        let face_shell: BTreeMap<FaceId, ShellId> = flat
            .iter()
            .zip(&shell_of_face)
            .map(|(face, shell)| (face.clone(), self.ids.shell(*shell)))
            .collect();
        for face in &mut self.sink.model.faces[first.faces..] {
            if let Some(shell) = face_shell.get(&face.id) {
                face.shell = shell.clone();
            }
        }
        let shells = members.len();
        let kind = if closed_shells == shells {
            BodyKind::Solid
        } else if closed_shells == 0 {
            BodyKind::Sheet
        } else {
            BodyKind::General
        };
        self.ctx.charge_entities(1, "pvz exact body")?;
        self.sink.model.bodies.push(Body {
            id: body.clone(),
            kind,
            regions,
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        self.derived(&body);
        Ok(body)
    }
}

fn root(parent: &mut [usize], index: usize) -> usize {
    let mut current = index;
    while parent[current] != current {
        parent[current] = parent[parent[current]];
        current = parent[current];
    }
    current
}

fn join(parent: &mut [usize], a: usize, b: usize) {
    let (a, b) = (root(parent, a), root(parent, b));
    if a != b {
        parent[a.max(b)] = a.min(b);
    }
}

#[cfg(test)]
mod tests;
