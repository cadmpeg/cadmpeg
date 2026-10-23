// SPDX-License-Identifier: Apache-2.0
//! One measurement pass over a document: selection, per-item work slices,
//! rows and document totals.

mod findings;

use std::collections::{BTreeSet, HashMap};

use cadmpeg_core::decode::{DecodeContext, ResourceDimension, ResourceFailure, ResourceLimit};
use cadmpeg_core::CodecError;
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::source_fidelity::SourceFidelity;
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;

use crate::aggregate::{BoxUnion, ReadingSum};
use crate::body::BodyInput;
use crate::budget::{PassAllowance, WorkProfile};
use crate::canonical::placement::Placement;
use crate::canonical::surface::Chart;
use crate::canonical::Carrier;
use crate::context::{Eval, Work};
use crate::edges::{measure_edge, EdgeGeometry};
use crate::face::{FaceInput, FaceResult, Verdict};
use crate::failure::{settle, Missing, Outcome};
use crate::geom::Aabb;
use crate::owners::Owners;
use crate::reason::CaveatCode;
use crate::report::{
    BodyMeasures, Caveat, DocumentMeasures, DocumentOrientation, EdgeMeasures, FaceMeasures,
    Histogram, ListedOrder, MeasureReport, NormalReading, ReportTolerances, RowPlacement,
    SurfaceMeasures, WorkReport,
};
use crate::select::Selection;
use crate::surface::SurfaceInput;
use crate::value::{Quantity, Scalar, Unavailable, Unit};
use crate::witness::WitnessInput;

/// The document a measurement pass reads.
#[derive(Clone, Copy)]
pub struct MeasureInput<'ir> {
    /// The decoded or loaded document.
    pub ir: &'ir CadIr,
    /// The decode sidecar, when one is available.
    pub fidelity: Option<&'ir SourceFidelity>,
    /// Whether the decode transferred geometry.
    pub geometry_transferred: bool,
}

/// Options of a measurement pass.
#[derive(Debug, Clone, Copy, Default)]
pub struct MeasureOptions {
    /// The work profile that sets the pass allowance.
    pub work: WorkProfile,
    /// The material density of every body; body rows report mass and
    /// inertia only with one.
    pub density: Option<Density>,
}

/// A material density in kilograms per cubic metre: finite and positive.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Density(f64);

impl Density {
    /// The density `kilograms_per_cubic_metre`, absent unless it is finite
    /// and positive.
    #[must_use]
    pub fn new(kilograms_per_cubic_metre: f64) -> Option<Self> {
        (kilograms_per_cubic_metre.is_finite() && kilograms_per_cubic_metre > 0.0)
            .then_some(Self(kilograms_per_cubic_metre))
    }

    /// The density in kilograms per cubic metre.
    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }
}

/// Why a measurement pass did not complete.
#[derive(Debug, thiserror::Error)]
pub enum MeasureError {
    /// The decode session refused.
    #[error(transparent)]
    Codec(#[from] CodecError),
    /// A selector names no entity of its arena.
    #[error("{arena} has no entity {id}")]
    UnknownEntity {
        /// The arena searched.
        arena: &'static str,
        /// The missing ID.
        id: String,
    },
    /// A face or edge selector names an entity outside the body scope.
    #[error("{entity} is not in the selected bodies")]
    OutsideScope {
        /// The entity outside the scope.
        entity: String,
    },
    /// A selector names an entity outside the occurrence's prototype.
    #[error("{entity} is not in the prototype of occurrence {occurrence}")]
    OutsideOccurrence {
        /// The entity outside the prototype.
        entity: String,
        /// The occurrence.
        occurrence: String,
    },
}

impl From<ResourceLimit> for MeasureError {
    fn from(limit: ResourceLimit) -> Self {
        Self::Codec(CodecError::ResourceLimit(limit))
    }
}

/// The `check` findings that measurement exposes in `ir`, as warnings:
/// face rings whose coedges are not listed in traversal order
/// ([`Check::RingOrder`](cadmpeg_ir::report::check::Check::RingOrder)),
/// faces whose sense disagrees with their loop winding, and bodies whose
/// shells are closed and whose volume readings conflict
/// ([`Check::Orientation`](cadmpeg_ir::report::check::Check::Orientation)).
///
/// The pass computes boundaries, regions, orientation readings, witnesses
/// and, for bodies whose shells are closed, areas and fluxes, under one
/// pass allowance of `options.work`. An item whose work slice runs out
/// gives no finding. A session refusal of `ctx` is the error.
pub fn findings(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    options: MeasureOptions,
) -> Result<Vec<cadmpeg_ir::report::check::Finding>, CodecError> {
    let input = MeasureInput {
        ir,
        fidelity: None,
        geometry_transferred: true,
    };
    Measurer::new(input, options)
        .orientation_findings(ctx)
        .map_err(CodecError::ResourceLimit)
}

/// Measures the bodies, faces and edges of one document.
pub struct Measurer<'ir> {
    input: MeasureInput<'ir>,
    options: MeasureOptions,
    owners: Owners<'ir>,
    index: ModelIndex<'ir>,
}

impl<'ir> Measurer<'ir> {
    /// A measurer over one document.
    #[must_use]
    pub fn new(input: MeasureInput<'ir>, options: MeasureOptions) -> Self {
        Self {
            input,
            options,
            owners: Owners::new(input.ir),
            index: ModelIndex::new_model_only(input.ir),
        }
    }

    /// Measure the selection. Item work runs in slices of one pass allowance;
    /// a session refusal of `ctx` ends the call. A selection with no face or
    /// edge level measures body rows.
    pub fn measure(
        &self,
        ctx: &DecodeContext<'_>,
        selection: &Selection,
    ) -> Result<MeasureReport, MeasureError> {
        let model = &self.input.ir.model;
        let space = match &selection.occurrence {
            None => None,
            Some(id) => Some(crate::occurrence::space(self.input.ir, id).ok_or_else(|| {
                MeasureError::UnknownEntity {
                    arena: "model.occurrences",
                    id: id.as_str().to_owned(),
                }
            })?),
        };
        let scope = self.scope(selection, space.as_ref())?;
        let face_level = selection.face_level();
        let edge_level = selection.edge_level();
        let body_level = !face_level && !edge_level && !selection.surface_level();

        let in_scope = |body: Option<&str>| match &scope {
            None => true,
            Some(bodies) => body.is_some_and(|body| bodies.contains(body)),
        };
        let mut face_rows: BTreeSet<&'ir str> = BTreeSet::new();
        if face_level {
            let all = selection.all_faces || selection.faces.is_empty();
            for face in &model.faces {
                let selected = all || selection.faces.contains(&face.id);
                if selected && in_scope(self.owners.face_body(face)) {
                    face_rows.insert(face.id.as_str());
                }
            }
        }
        let mut edge_rows: BTreeSet<&'ir str> = BTreeSet::new();
        if edge_level {
            let all = selection.all_edges || selection.edges.is_empty();
            for edge in &model.edges {
                let selected = all || selection.edges.contains(&edge.id);
                if selected && in_scope(self.owners.edge_body(edge.id.as_str())) {
                    edge_rows.insert(edge.id.as_str());
                }
            }
        }
        let body_rows: Vec<&'ir cadmpeg_ir::topology::Body> = if body_level {
            model
                .bodies
                .iter()
                .filter(|body| in_scope(Some(body.id.as_str())))
                .collect()
        } else {
            Vec::new()
        };

        let mut needed_faces: BTreeSet<&'ir str> = face_rows.clone();
        let mut needed_edges: BTreeSet<&'ir str> = edge_rows.clone();
        for body in &body_rows {
            needed_faces.extend(self.owners.body_faces(body.id.as_str()));
            needed_edges.extend(self.owners.body_edges(body.id.as_str()));
        }
        for face in &face_rows {
            needed_faces.extend(self.witness_shell(face));
        }
        for face in &needed_faces {
            if let Some(face) = self.owners.face(face) {
                for loop_id in &face.loops {
                    if let Some(face_loop) = self.owners.loop_(loop_id.as_str()) {
                        for coedge in face_loop.coedges() {
                            if let Some(coedge) = self.owners.coedge(coedge.as_str()) {
                                needed_edges.insert(coedge.edge.as_str());
                            }
                        }
                    }
                }
            }
        }

        let items = model.faces.len() + model.edges.len();
        let Some(allowance) = self.options.work.pass_allowance(items) else {
            return Err(ResourceLimit {
                dimension: ResourceDimension::Entities,
                reason: ResourceFailure::BudgetExceeded,
                limit: 0,
                used: cadmpeg_core::decode::u64_from_index(items),
                additional: 0,
                operation: "measurement pass allowance",
            }
            .into());
        };
        let pass = PassAllowance::new(allowance);

        let world = |body: &str| space.as_ref().and_then(|space| space.placements.get(body));
        let mut composed: HashMap<&'ir str, Option<Transform>> = HashMap::new();
        let placements: HashMap<&'ir str, Outcome<Placement>> = model
            .bodies
            .iter()
            .map(|body| {
                let placed = match (world(body.id.as_str()), body.transform) {
                    (None, transform) => Ok(transform),
                    (Some(world), transform) => world
                        .compose(transform.unwrap_or_else(Transform::identity))
                        .map(Some),
                };
                if let Ok(transform) = placed {
                    composed.insert(
                        body.id.as_str(),
                        transform.filter(|transform| *transform != Transform::identity()),
                    );
                }
                let placement = match placed {
                    Ok(Some(transform)) => settle(Placement::of(transform)),
                    Ok(None) => Ok(Ok(Placement::identity())),
                    Err(_) => Ok(Err(Missing {
                        reason: crate::reason::UnavailableReason::PlacementNonFinite,
                        entity: Some(body.id.as_str().to_owned()),
                        message: "the occurrence placement and the body transform do not \
                                  compose to a finite placement"
                            .to_owned(),
                    })),
                };
                placement.map(|placement| (body.id.as_str(), placement))
            })
            .collect::<Result<_, ResourceLimit>>()?;
        let placement_of = |body: Option<&str>| -> Outcome<Placement> {
            body.and_then(|body| placements.get(body))
                .cloned()
                .unwrap_or(Ok(Placement::identity()))
        };

        let surface_ids: BTreeSet<&str> = selection
            .surfaces
            .iter()
            .map(cadmpeg_ir::ids::SurfaceId::as_str)
            .collect();
        let surfaces = SurfaceInput {
            owners: &self.owners,
            fidelity: self.input.fidelity,
            faces: &model.faces,
            scope: space.as_ref().map(|space| &space.bodies),
            transform: &|body: &str| composed.get(body).copied().flatten(),
            placement: &placement_of,
        };
        let mut surface_out: Vec<SurfaceMeasures> = Vec::new();
        let placed = space.as_ref().is_none_or(crate::occurrence::Space::placed);
        for id in surface_ids.into_iter().filter(|_| placed) {
            if let Some(surface) = self.owners.surface(id) {
                surface_out.push(surfaces.row(surface)?);
            }
        }

        let mut edges: HashMap<&'ir str, EdgeGeometry<'ir>> = HashMap::new();
        for edge_id in &needed_edges {
            let Some(edge) = self.owners.edge(edge_id) else {
                continue;
            };
            let body = self.owners.edge_body(edge_id);
            let slice = pass.edge_slice();
            let eval = Eval {
                ctx,
                index: &self.index,
                work: &slice,
            };
            let geometry = measure_edge(&eval, &self.owners, edge, &placement_of(body), Work::Row)?;
            edges.insert(edge_id, geometry);
        }

        let flux_bodies: BTreeSet<&'ir str> = needed_faces
            .iter()
            .filter_map(|face| self.owners.face(face))
            .filter_map(|face| self.owners.face_body(face))
            .collect();
        let flux_origins: HashMap<&'ir str, cadmpeg_ir::math::Point3> = flux_bodies
            .into_iter()
            .map(|id| (id, self.flux_origin(id, &edges, &placement_of(Some(id)))))
            .collect();
        let mut faces: HashMap<&'ir str, FaceResult<'ir>> = HashMap::new();
        for face_id in &needed_faces {
            let Some(face) = self.owners.face(face_id) else {
                continue;
            };
            let body = self.owners.face_body(face);
            let slice = pass.face_slice();
            let eval = Eval {
                ctx,
                index: &self.index,
                work: &slice,
            };
            let input = FaceInput {
                work: Work::Row,
                owners: &self.owners,
                edges: &edges,
                fidelity: self.input.fidelity,
                body,
                placement: placement_of(body),
                flux_origin: body.and_then(|body| flux_origins.get(body).copied()),
            };
            faces.insert(face_id, input.measure(&eval, face)?);
        }
        let witness_slice = pass.face_slice();
        WitnessInput {
            owners: &self.owners,
            edges: &edges,
            pass: &pass,
            eval: Eval {
                ctx,
                index: &self.index,
                work: &witness_slice,
            },
            placements: &placements,
        }
        .run(&mut faces)?;
        if let Some(limit) = ctx.resource_refusal() {
            return Err(limit.into());
        }

        let mut face_out: Vec<FaceMeasures> = face_rows
            .iter()
            .filter_map(|id| faces.get(id))
            .filter(|result| filter_matches(selection, result))
            .map(|result| result.row.clone())
            .collect();
        let mut edge_out: Vec<EdgeMeasures> = edge_rows
            .iter()
            .filter_map(|id| edges.get(id))
            .map(|geometry| geometry.row(&self.owners))
            .collect();
        let bodies = BodyInput {
            owners: &self.owners,
            faces: &faces,
            edges: &edges,
            origins: &flux_origins,
            placements: &placements,
            density: self.options.density,
        };
        let mut body_out: Vec<BodyMeasures> =
            body_rows.iter().map(|body| bodies.row(body)).collect();
        if let Some(space) = space.as_ref().filter(|space| space.placed()) {
            let placement = RowPlacement::Occurrence {
                occurrence: space.report.occurrence.clone(),
            };
            for row in &mut body_out {
                row.placement = placement.clone();
            }
            for row in &mut face_out {
                row.placement = placement.clone();
            }
            for row in &mut edge_out {
                row.placement = placement.clone();
            }
            for row in &mut surface_out {
                row.placement = placement.clone();
            }
        }

        let rows = Rows {
            bodies: &body_out,
            faces: &face_out,
            edges: &edge_out,
            surfaces: &surface_out,
            body_level,
            unplaced: space
                .as_ref()
                .and_then(crate::occurrence::Space::unavailable),
        };
        let document = Self::document(
            &rows,
            &faces,
            &pass,
            [
                if body_level {
                    body_rows
                        .iter()
                        .map(|body| self.owners.body_faces(body.id.as_str()).count())
                        .sum()
                } else {
                    face_out.len()
                },
                if body_level {
                    body_rows
                        .iter()
                        .map(|body| self.owners.body_edges(body.id.as_str()).count())
                        .sum()
                } else {
                    edge_out.len()
                },
            ],
        );
        let mut caveats = Vec::new();
        if self.input.fidelity.is_none() {
            caveats.push(Caveat::new(
                CaveatCode::MeasureProvenanceNotRecorded,
                None,
                "the document carries no decode sidecar; source exactness is not recorded",
            ));
        }
        if !self.input.geometry_transferred {
            caveats.push(Caveat::new(
                CaveatCode::MeasureGeometryNotTransferred,
                None,
                "the decode transferred no geometry",
            ));
        }
        caveats.sort();
        Ok(MeasureReport {
            occurrence: space.map(|space| space.report),
            tolerances: ReportTolerances {
                linear: self.input.ir.tolerances.linear.get(),
                angular: self.input.ir.tolerances.angular.get(),
            },
            caveats,
            bodies: body_out,
            faces: face_out,
            edges: edge_out,
            surfaces: surface_out,
            document,
        })
    }

    /// The faces of the shell of a face whose region may rest on its
    /// declared orientation: their witnesses read the whole shell. A face
    /// on a plane, cylinder or cone has none.
    fn witness_shell(&self, face: &str) -> Vec<&'ir str> {
        let Some(face) = self.owners.face(face) else {
            return Vec::new();
        };
        let settled = self
            .owners
            .surface(face.surface.as_str())
            .is_some_and(|surface| {
                matches!(
                    &surface.geometry,
                    cadmpeg_ir::geometry::SurfaceGeometry::Solved(
                        cadmpeg_ir::geometry::SolvedSurfaceGeometry::Plane(_)
                            | cadmpeg_ir::geometry::SolvedSurfaceGeometry::Cylinder(_)
                            | cadmpeg_ir::geometry::SolvedSurfaceGeometry::Cone(_)
                    )
                )
            });
        if settled {
            return Vec::new();
        }
        self.owners
            .shell(face.shell.as_str())
            .map(|shell| {
                shell
                    .faces()
                    .iter()
                    .map(cadmpeg_ir::ids::FaceId::as_str)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The reference point of a body's fluxes: the centre of the box of its
    /// edges and vertices, else of its faces' whole carriers, else the
    /// origin. No orientation reading enters it, so every reading of the
    /// body uses the same point.
    fn flux_origin(
        &self,
        body: &str,
        edges: &HashMap<&'ir str, EdgeGeometry<'ir>>,
        placement: &Outcome<Placement>,
    ) -> cadmpeg_ir::math::Point3 {
        let union =
            |aabb: Option<Aabb>, other: Aabb| Some(aabb.map_or(other, |aabb| aabb.union(other)));
        let mut aabb: Option<Aabb> = None;
        for edge in self.owners.body_edges(body) {
            if let Some(Ok(curve_box)) = edges.get(edge).map(|geometry| &geometry.bbox) {
                aabb = union(aabb, curve_box.aabb);
            }
        }
        if let Ok(placement) = placement {
            for point in self.owners.body_vertex_points(body) {
                aabb = union(aabb, Aabb::of_point(placement.point(point)));
            }
            if aabb.is_none() {
                for face in self.owners.body_faces(body) {
                    if let Some(carrier_box) = self.carrier_box(face, *placement) {
                        aabb = union(aabb, carrier_box);
                    }
                }
            }
        }
        aabb.map_or(cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0), |aabb| {
            let [low, high] = [aabb.min_point(), aabb.max_point()];
            cadmpeg_ir::math::Point3::new(
                low.x.midpoint(high.x),
                low.y.midpoint(high.y),
                low.z.midpoint(high.z),
            )
        })
    }

    /// The box of a face's whole carrier: the critical points of a sphere
    /// or torus, the control net of a NURBS carrier.
    fn carrier_box(&self, face: &str, placement: Placement) -> Option<Aabb> {
        let face = self.owners.face(face)?;
        let surface = self.owners.surface(face.surface.as_str())?;
        let carrier = Carrier::resolve_in(&self.owners, surface, placement).ok()?;
        let chart = carrier.chart().ok()?;
        match chart {
            Chart::Nurbs(nurbs) => nurbs.control_hull().ok(),
            other => other.critical_box(),
        }
    }

    /// The body scope, or `None` for every body. With an occurrence, the
    /// scope is the bodies of its subtree, and empty when its space has no
    /// rows.
    fn scope(
        &self,
        selection: &Selection,
        space: Option<&crate::occurrence::Space<'ir>>,
    ) -> Result<Option<BTreeSet<&'ir str>>, MeasureError> {
        let outside = |entity: &str| match space {
            Some(space) => MeasureError::OutsideOccurrence {
                entity: entity.to_owned(),
                occurrence: space.report.occurrence.clone(),
            },
            None => MeasureError::OutsideScope {
                entity: entity.to_owned(),
            },
        };
        let mut scope = BTreeSet::new();
        for body in &selection.bodies {
            let Some(body) = self.owners.body(body.as_str()) else {
                return Err(MeasureError::UnknownEntity {
                    arena: "model.bodies",
                    id: body.as_str().to_owned(),
                });
            };
            if space.is_some_and(|space| !space.bodies.contains(body.id.as_str())) {
                return Err(outside(body.id.as_str()));
            }
            scope.insert(body.id.as_str());
        }
        let scope = match space {
            Some(space) if scope.is_empty() => Some(space.bodies.clone()),
            Some(_) => Some(scope),
            None => (!scope.is_empty()).then_some(scope),
        };
        for face in &selection.faces {
            let Some(record) = self.owners.face(face.as_str()) else {
                return Err(MeasureError::UnknownEntity {
                    arena: "model.faces",
                    id: face.as_str().to_owned(),
                });
            };
            if let Some(scope) = &scope {
                if !self
                    .owners
                    .face_body(record)
                    .is_some_and(|body| scope.contains(body))
                {
                    return Err(outside(face.as_str()));
                }
            }
        }
        for surface in &selection.surfaces {
            if self.owners.surface(surface.as_str()).is_none() {
                return Err(MeasureError::UnknownEntity {
                    arena: "model.surfaces",
                    id: surface.as_str().to_owned(),
                });
            }
            if let Some(scope) = &scope {
                let used = self.input.ir.model.faces.iter().any(|face| {
                    face.surface == *surface
                        && self
                            .owners
                            .face_body(face)
                            .is_some_and(|body| scope.contains(body))
                });
                if !used {
                    return Err(outside(surface.as_str()));
                }
            }
        }
        for edge in &selection.edges {
            if self.owners.edge(edge.as_str()).is_none() {
                return Err(MeasureError::UnknownEntity {
                    arena: "model.edges",
                    id: edge.as_str().to_owned(),
                });
            }
            if let Some(scope) = &scope {
                if !self
                    .owners
                    .edge_body(edge.as_str())
                    .is_some_and(|body| scope.contains(body))
                {
                    return Err(outside(edge.as_str()));
                }
            }
        }
        if space.is_some_and(|space| !space.placed()) {
            return Ok(Some(BTreeSet::new()));
        }
        Ok(scope)
    }

    fn document(
        rows: &Rows<'_>,
        face_results: &HashMap<&'ir str, FaceResult<'ir>>,
        pass: &PassAllowance,
        counts: [usize; 2],
    ) -> DocumentMeasures {
        let Rows {
            bodies,
            faces,
            edges,
            surfaces,
            body_level,
            unplaced,
        } = *rows;
        let mut histogram = Histogram::default();
        if let Some(unplaced) = unplaced {
            histogram.count(unplaced);
        }
        for body in bodies {
            body.visit(&mut histogram);
        }
        for face in faces {
            face.visit(&mut histogram);
        }
        for edge in edges {
            edge.visit(&mut histogram);
        }
        for surface in surfaces {
            surface.visit(&mut histogram);
        }
        let (area, bbox, closed_volume) = match unplaced {
            Some(unplaced) if body_level => (
                Some(Quantity::Unavailable(unplaced.recast())),
                Some(Quantity::Unavailable(unplaced.recast())),
                Some(Quantity::Unavailable(unplaced.recast())),
            ),
            _ if body_level && !bodies.is_empty() => (
                Some(total_area(bodies)),
                Some(union_box(bodies)),
                bodies
                    .iter()
                    .any(|body| body.volume.is_some())
                    .then(|| total_volume(bodies)),
            ),
            _ => (None, None, None),
        };
        let mut orientation = DocumentOrientation::default();
        let mut face_ids: Vec<&&str> = face_results.keys().collect();
        face_ids.sort();
        for id in face_ids {
            let Some(result) = face_results.get(*id) else {
                continue;
            };
            let face = &result.row;
            match (
                face.orientation.consistent,
                face.orientation.loop_normal,
                result.verdict,
            ) {
                (_, _, Verdict::Conflict | Verdict::Contradicted) => {}
                (Some(false), NormalReading::Mixed, _) => {
                    orientation.mixed.push(face.face.clone());
                }
                (Some(false), _, _) => orientation.inconsistent.push(face.face.clone()),
                _ => {}
            }
            match result.verdict {
                Verdict::Unconfirmed => orientation.unconfirmed.push(face.face.clone()),
                Verdict::Conflict | Verdict::Contradicted => {
                    orientation.conflict.push(face.face.clone());
                }
                Verdict::Settled | Verdict::Confirmed => {}
            }
            if face.measures.loops.iter().any(|entry| {
                matches!(
                    entry.listed_order,
                    Some(ListedOrder::Reversed | ListedOrder::Other)
                )
            }) {
                orientation.ring_order.push(face.face.clone());
            }
        }
        DocumentMeasures {
            bodies: bodies.len(),
            faces: counts[0],
            edges: counts[1],
            surfaces: surfaces.len(),
            area,
            closed_volume,
            bbox,
            orientation,
            unavailable: histogram.0,
            work: WorkReport {
                allowance: pass.allowance(),
                used: pass.used(),
                exhausted: pass.exhausted(),
            },
        }
    }
}

/// The rows one pass emits.
#[derive(Clone, Copy)]
struct Rows<'a> {
    bodies: &'a [BodyMeasures],
    faces: &'a [FaceMeasures],
    edges: &'a [EdgeMeasures],
    surfaces: &'a [SurfaceMeasures],
    body_level: bool,
    /// Why the occurrence space has no rows.
    unplaced: Option<&'a Unavailable<()>>,
}

/// Whether a face row passes the kind and radius filters.
fn filter_matches(selection: &Selection, result: &FaceResult<'_>) -> bool {
    let filter = &selection.face_filter;
    if !filter.kinds.is_empty() && !filter.kinds.contains(&result.row.carrier.kind) {
        return false;
    }
    match &filter.radius {
        None => true,
        Some(range) => result
            .radius
            .is_some_and(|radius| range.contains(radius, result.tau)),
    }
}

fn total_area(bodies: &[BodyMeasures]) -> Quantity<Scalar> {
    total(
        bodies,
        |body| body.area.as_ref(),
        Unit::SquareMillimeter,
        "area",
    )
}

fn total_volume(bodies: &[BodyMeasures]) -> Quantity<Scalar> {
    total(
        bodies,
        |body| body.volume.as_ref(),
        Unit::CubicMillimeter,
        "volume",
    )
}

/// The sum of one member quantity over the bodies that have it.
fn total(
    bodies: &[BodyMeasures],
    member: impl Fn(&BodyMeasures) -> Option<&Quantity<Scalar>>,
    unit: Unit,
    name: &str,
) -> Quantity<Scalar> {
    let mut sum = ReadingSum::new();
    for body in bodies {
        if let Some(quantity) = member(body) {
            sum.add(&body.body, name, quantity);
        }
    }
    sum.finish(unit, "document")
}

fn union_box(bodies: &[BodyMeasures]) -> Quantity<crate::value::BoxValue> {
    let mut union = BoxUnion::new();
    for body in bodies {
        union.add(&body.body, &body.bbox);
    }
    union.finish(None, "the document has no bounded body")
}
