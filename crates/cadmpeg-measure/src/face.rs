// SPDX-License-Identifier: Apache-2.0
//! The measurement of one face: carrier, boundary, region, area, lengths,
//! box and orientation.

use std::collections::{BTreeSet, HashMap};

use cadmpeg_core::decode::ResourceLimit;
use cadmpeg_ir::source_fidelity::SourceFidelity;
use cadmpeg_ir::topology::Face;

use crate::area::nesting_area;
use crate::boundary::{FaceFrame, LoopKind, ResolvedFace, ResolvedLoop};
use crate::canonical::placement::Placement;
use crate::canonical::surface::Chart;
use crate::canonical::{Carrier, Sources};
use crate::context::Eval;
use crate::edges::EdgeGeometry;
use crate::failure::{settle, Measure, Missing, Outcome};
use crate::geom::Aabb;
use crate::orientation::face_orientation;
use crate::owners::Owners;
use crate::reason::{CaveatCode, UnavailableReason};
use crate::region::{classify, loop_shape, winding, LoopShape, Region};
use crate::report::{
    CarrierMeasures, CarrierParameters, Caveat, CoedgeMeasures, FaceMeasures, FaceOrientation,
    FaceQuantities, ListedOrder, LoopMeasures, LoopRole, StatusTally,
};
use crate::select::SurfaceKind;
use crate::tolerance::{allowance, model_tolerance};
use crate::value::{
    measured, BoundaryBox, BoxKind, BoxValue, Errors, Method, Point, Quantity, Scalar, Unit,
};

/// Everything measured about one face.
pub(crate) struct FaceResult {
    pub(crate) row: FaceMeasures,
    /// The face's box, when measured.
    pub(crate) aabb: Option<Aabb>,
    /// The model tolerance of the face's boundary.
    pub(crate) tau: f64,
    /// The radius `--radius` matches, when the carrier has one.
    pub(crate) radius: Option<f64>,
}

/// What one face measurement reads.
pub(crate) struct FaceInput<'a, 'ir> {
    pub(crate) owners: &'a Owners<'ir>,
    pub(crate) edges: &'a HashMap<&'ir str, EdgeGeometry<'ir>>,
    pub(crate) fidelity: Option<&'a SourceFidelity>,
    pub(crate) body: Option<&'ir str>,
    pub(crate) placement: Outcome<Placement>,
}

/// The facts of a face boundary, from its resolution.
struct Boundary<'ir> {
    resolved: ResolvedFace<'ir>,
    shapes: Vec<Option<LoopShape>>,
}

fn unavailable<T>(reason: UnavailableReason, entity: &str, message: &str) -> Quantity<T> {
    Quantity::missing(reason, Some(entity), message.to_owned())
}

impl<'ir> FaceInput<'_, 'ir> {
    /// Measure one face.
    pub(crate) fn measure(
        &self,
        eval: &Eval<'_, '_>,
        face: &'ir Face,
    ) -> Result<FaceResult, ResourceLimit> {
        let id = face.id.as_str();
        let owners = self.owners;
        let coedges: Vec<_> = face
            .loops
            .iter()
            .filter_map(|loop_id| owners.loop_(loop_id.as_str()))
            .flat_map(cadmpeg_ir::topology::Loop::coedges)
            .filter_map(|coedge| owners.coedge(coedge.as_str()))
            .collect();
        let linear = owners.ir.tolerances.linear.get();
        let mut local: Vec<f64> = face
            .tolerance
            .map(cadmpeg_ir::scalar::PositiveReal::get)
            .into_iter()
            .collect();
        for coedge in &coedges {
            if let Some(edge) = self.edges.get(coedge.edge.as_str()) {
                local.push(edge.tau);
            }
        }
        let alpha = allowance(linear, local.iter().copied());
        let tau = model_tolerance(linear, local.iter().copied());

        let carrier = match (&self.placement, owners.surface(face.surface.as_str())) {
            (Err(missing), _) => Err(missing.clone()),
            (Ok(placement), Some(surface)) => {
                settle(Carrier::resolve(surface, *placement).map_err(|failure| failure.at(id)))?
            }
            (Ok(_), None) => Err(Missing {
                reason: UnavailableReason::CarrierUnresolved,
                entity: Some(face.surface.as_str().to_owned()),
                message: "the face's surface does not resolve".to_owned(),
            }),
        };
        let carrier_row = match &carrier {
            Ok(carrier) => carrier.measures(Sources {
                fidelity: self.fidelity,
                surface: carrier.id(),
                depth: carrier.depth,
                body: self.body.filter(|body| {
                    owners
                        .body(body)
                        .is_some_and(|body| body.transform.is_some())
                }),
            }),
            Err(_) => CarrierMeasures {
                surface: face.surface.as_str().to_owned(),
                kind: SurfaceKind::Unknown,
                chain: Vec::new(),
                analytic: false,
                placement: crate::report::PlacementReport {
                    class: crate::report::PlacementClass::Identity,
                    scale: None,
                    reflection: None,
                },
                parameters: CarrierParameters::None {},
            },
        };

        let chart: Outcome<Chart> = match &carrier {
            Ok(carrier) => settle(carrier.chart().copied())?,
            Err(missing) => Err(missing.clone()),
        };
        let boundary: Outcome<Boundary<'ir>> = match (&chart, &self.placement) {
            (Ok(chart), Ok(placement)) => {
                settle(self.boundary(eval, face, chart, *placement, alpha))?
            }
            (Err(missing), _) | (_, Err(missing)) => Err(missing.clone()),
        };
        let region: Outcome<Region> = match (&chart, &boundary) {
            (Ok(chart), Ok(boundary)) => settle(
                classify(face, chart, &boundary.resolved, &boundary.shapes)
                    .map_err(|failure| failure.at(id)),
            )?,
            (Err(missing), _) | (_, Err(missing)) => Err(missing.clone()),
        };

        let mut max_gap: f64 = 0.0;
        let mut off_surface: f64 = 0.0;
        if let Ok(boundary) = &boundary {
            for resolution in &boundary.resolved.loops {
                if let Ok(resolved) = &resolution.outcome {
                    max_gap = max_gap.max(resolved.max_gap);
                    off_surface = off_surface.max(resolved.off_surface);
                }
            }
        }
        let tau_b = tau.max(max_gap).max(off_surface);

        let lengths = self.lengths(face, &coedges);
        let placement_term = |dimension: u8, value: f64| match &self.placement {
            Ok(placement) => placement.model_term(dimension, value),
            Err(_) => 0.0,
        };

        let area = match (&chart, &boundary, &region) {
            (Ok(chart), Ok(boundary), Ok(region)) => {
                let resolved: Vec<Option<&ResolvedLoop<'_>>> = boundary
                    .resolved
                    .loops
                    .iter()
                    .map(|resolution| resolution.outcome.as_ref().ok())
                    .collect();
                match settle(
                    nesting_area(chart, region, &boundary.shapes, &resolved)
                        .map_err(|failure| failure.at(id)),
                )? {
                    Ok(area) => measured(
                        Scalar::new(area.value),
                        Unit::SquareMillimeter,
                        Method::BoundaryQuadrature,
                        Errors::new(
                            area.numeric_error,
                            lengths.boundary * tau_b + placement_term(2, area.value),
                        ),
                        id,
                    ),
                    Err(missing) => missing.quantity(),
                }
            }
            (Err(missing), _, _) | (_, Err(missing), _) | (_, _, Err(missing)) => {
                missing.quantity()
            }
        };

        let (bbox, aabb) = match (&chart, &region) {
            (Ok(Chart::Plane(_)), Ok(_)) => self.boundary_box(face, &coedges, &boundary, tau_b),
            (Ok(_), Ok(_)) => (
                unavailable(
                    UnavailableReason::RegionDomainUnsupported,
                    id,
                    "boxes on this carrier are not computed in this build",
                ),
                None,
            ),
            (Err(missing), _) | (_, Err(missing)) => (missing.quantity(), None),
        };

        let orientation: FaceOrientation = face_orientation(face, region.as_ref().ok());
        let mut caveats = Vec::new();
        let loops = self.loops(face, &boundary, region.as_ref().ok(), &mut caveats);
        caveats.sort();

        let boundary_length = match &lengths.failure {
            None => measured(
                Scalar::new(lengths.boundary),
                Unit::Millimeter,
                lengths.method,
                Errors::new(
                    lengths.boundary_error,
                    lengths.boundary_model + placement_term(1, lengths.boundary),
                ),
                id,
            ),
            Some(missing) => missing.quantity(),
        };
        let seam_length = match &lengths.failure {
            None => measured(
                Scalar::new(lengths.seam),
                Unit::Millimeter,
                lengths.seam_method,
                Errors::new(
                    lengths.seam_error,
                    lengths.seam_model + placement_term(1, lengths.seam),
                ),
                id,
            ),
            Some(missing) => missing.quantity(),
        };

        let measures = FaceQuantities {
            area,
            boundary_length,
            seam_length,
            bbox,
            axial_range: None,
            axial_extent: None,
            angular_sweep: None,
            latitude_range: None,
            radius_range: None,
            u_range: None,
            v_range: None,
            region: region.as_ref().ok().map(|region| region.rule),
            loops,
        };
        let radius = radius_of(&carrier_row);
        let mut tally = StatusTally::default();
        carrier_row.parameters.visit(&mut tally);
        measures.visit(&mut tally);
        let row = FaceMeasures {
            face: id.to_owned(),
            name: face.name.clone(),
            body: self.body.map(str::to_owned),
            shell: face.shell.as_str().to_owned(),
            sense: face.sense,
            carrier: carrier_row,
            measures,
            orientation,
            caveats,
            status: tally.status(),
        };
        Ok(FaceResult {
            row,
            aabb,
            tau,
            radius,
        })
    }

    fn boundary(
        &self,
        eval: &Eval<'_, '_>,
        face: &'ir Face,
        chart: &Chart,
        placement: Placement,
        alpha: f64,
    ) -> Measure<Boundary<'ir>> {
        let frame = FaceFrame {
            owners: self.owners,
            edges: self.edges,
            face,
            chart,
            body: placement,
            alpha,
        };
        let resolved = frame.resolve(eval)?;
        let mut shapes = Vec::with_capacity(resolved.loops.len());
        for resolution in &resolved.loops {
            let shape = match &resolution.outcome {
                Ok(resolved_loop) if matches!(resolved_loop.kind, LoopKind::Ring) => {
                    Some(loop_shape(eval, chart, resolved_loop)?)
                }
                _ => None,
            };
            shapes.push(shape);
        }
        Ok(Boundary { resolved, shapes })
    }

    /// Boundary and seam lengths from the face's edges. A seam edge counts
    /// once; its two coedges lie in this face.
    fn lengths(&self, face: &Face, coedges: &[&'ir cadmpeg_ir::topology::Coedge]) -> Lengths {
        let mut lengths = Lengths::default();
        let mut methods = BTreeSet::new();
        let mut seam_methods = BTreeSet::new();
        let mut seams = BTreeSet::new();
        for coedge in coedges {
            let Some(edge) = self.edges.get(coedge.edge.as_str()) else {
                lengths.failure.get_or_insert(Missing {
                    reason: UnavailableReason::BoundaryCoedgeUnresolved,
                    entity: Some(coedge.id.as_str().to_owned()),
                    message: "the coedge's edge does not resolve".to_owned(),
                });
                continue;
            };
            let length = match &edge.length {
                Ok(length) => *length,
                Err(missing) => {
                    lengths.failure.get_or_insert_with(|| missing.clone());
                    continue;
                }
            };
            if FaceFrame::is_seam(self.owners, face, coedge) {
                if seams.insert(coedge.edge.as_str()) {
                    lengths.seam += length.value;
                    lengths.seam_error += length.numeric_error;
                    lengths.seam_model += 2.0 * edge.tau;
                    seam_methods.insert(method_rank(length.method));
                }
            } else {
                lengths.boundary += length.value;
                lengths.boundary_error += length.numeric_error;
                lengths.boundary_model += 2.0 * edge.tau;
                methods.insert(method_rank(length.method));
            }
        }
        lengths.method = combined(&methods);
        lengths.seam_method = combined(&seam_methods);
        lengths
    }

    /// The box of the face's boundary pieces: its edges and vertex loops.
    fn boundary_box(
        &self,
        face: &Face,
        coedges: &[&'ir cadmpeg_ir::topology::Coedge],
        boundary: &Outcome<Boundary<'ir>>,
        tau_b: f64,
    ) -> (Quantity<BoxValue>, Option<Aabb>) {
        let id = face.id.as_str();
        let mut aabb: Option<Aabb> = None;
        let mut numeric_error: f64 = 0.0;
        let mut include = |other: Aabb| {
            aabb = Some(aabb.map_or(other, |aabb| aabb.union(other)));
        };
        for coedge in coedges {
            let Some(edge) = self.edges.get(coedge.edge.as_str()) else {
                continue;
            };
            match &edge.bbox {
                Ok(curve_box) => {
                    include(curve_box.aabb);
                    numeric_error = numeric_error.max(curve_box.numeric_error);
                }
                Err(missing) => return (missing.quantity(), None),
            }
        }
        if let Ok(boundary) = boundary {
            for resolution in &boundary.resolved.loops {
                if let Ok(ResolvedLoop {
                    kind: LoopKind::Vertex(point),
                    ..
                }) = &resolution.outcome
                {
                    include(Aabb::of_point(*point));
                }
            }
        }
        let Some(aabb) = aabb else {
            return (
                unavailable(
                    UnavailableReason::BoundaryUnboundedFace,
                    id,
                    "the face has no boundary piece",
                ),
                None,
            );
        };
        let value = box_value(aabb, BoxKind::Tight, Some(aabb));
        (
            measured(
                value,
                Unit::Millimeter,
                Method::BoundaryExtremum,
                Errors::new(numeric_error, tau_b),
                id,
            ),
            Some(aabb),
        )
    }

    /// The loop entries of a face, in face order, and their ring-order
    /// caveats.
    fn loops(
        &self,
        face: &Face,
        boundary: &Outcome<Boundary<'ir>>,
        region: Option<&Region>,
        caveats: &mut Vec<Caveat>,
    ) -> Vec<LoopMeasures> {
        let Ok(boundary) = boundary else {
            return face
                .loops
                .iter()
                .map(|loop_id| LoopMeasures {
                    loop_id: loop_id.as_str().to_owned(),
                    declared_role: declared(face, loop_id),
                    role: None,
                    winding: None,
                    listed_order: None,
                    net_shift: None,
                    max_gap: None,
                    length: None,
                    seam_length: None,
                    coedges: Vec::new(),
                })
                .collect();
        };
        let mut entries = Vec::new();
        for (index, resolution) in boundary.resolved.loops.iter().enumerate() {
            let loop_id = resolution.face_loop.id.as_str();
            if matches!(
                resolution.listed,
                Some(ListedOrder::Reversed | ListedOrder::Other)
            ) {
                caveats.push(Caveat::new(
                    CaveatCode::BoundaryRingOrder,
                    Some(loop_id),
                    "the ring's coedges are listed out of traversal order",
                ));
            }
            let resolved = resolution.outcome.as_ref().ok();
            let shape = boundary.shapes.get(index).and_then(Option::as_ref);
            let role = match resolved.map(|resolved| resolved.kind) {
                Some(LoopKind::Vertex(_)) => Some(LoopRole::Vertex),
                _ => region
                    .and_then(|region| region.loops.get(index))
                    .map(|member| member.role),
            };
            let mut length = Some(0.0);
            let mut seam_length = Some(0.0);
            let mut seams = BTreeSet::new();
            let mut coedges = Vec::new();
            for coedge in &resolution.coedges {
                let edge = self.edges.get(coedge.edge.as_str());
                let edge_length = edge
                    .and_then(|edge| edge.length.as_ref().ok())
                    .map(|l| l.value);
                let seam = FaceFrame::is_seam(self.owners, face, coedge);
                if seam {
                    if seams.insert(coedge.edge.as_str()) {
                        seam_length = seam_length.zip(edge_length).map(|(sum, value)| sum + value);
                    }
                } else {
                    length = length.zip(edge_length).map(|(sum, value)| sum + value);
                }
                let uv_source = resolved.and_then(|resolved| {
                    resolved
                        .pieces
                        .iter()
                        .find(|piece| piece.coedge.is_some_and(|piece| piece.id == coedge.id))
                        .map(|piece| piece.uv_source)
                });
                coedges.push(CoedgeMeasures {
                    coedge: coedge.id.as_str().to_owned(),
                    edge: coedge.edge.as_str().to_owned(),
                    seam,
                    uv_source,
                    length: edge_length,
                });
            }
            entries.push(LoopMeasures {
                loop_id: loop_id.to_owned(),
                declared_role: resolution.declared_role,
                role,
                winding: resolved.map(|resolved| winding(resolved, shape)),
                listed_order: resolution.listed,
                net_shift: resolved.map(|resolved| resolved.net_shift),
                max_gap: resolved.map(|resolved| resolved.max_gap),
                length,
                seam_length,
                coedges,
            });
        }
        entries
    }
}

/// Boundary and seam length sums of one face.
struct Lengths {
    boundary: f64,
    boundary_error: f64,
    boundary_model: f64,
    method: Method,
    seam: f64,
    seam_error: f64,
    seam_model: f64,
    seam_method: Method,
    failure: Option<Missing>,
}

impl Default for Lengths {
    fn default() -> Self {
        Self {
            boundary: 0.0,
            boundary_error: 0.0,
            boundary_model: 0.0,
            method: Method::ClosedForm,
            seam: 0.0,
            seam_error: 0.0,
            seam_model: 0.0,
            seam_method: Method::ClosedForm,
            failure: None,
        }
    }
}

/// A stable rank of the methods an edge length can have.
fn method_rank(method: Method) -> u8 {
    match method {
        Method::ClosedForm => 0,
        Method::BoundaryQuadrature => 1,
        _ => 2,
    }
}

/// The method of a sum: the members' one method, else `sum`.
fn combined(ranks: &BTreeSet<u8>) -> Method {
    match ranks.iter().collect::<Vec<_>>().as_slice() {
        [] | [0] => Method::ClosedForm,
        [1] => Method::BoundaryQuadrature,
        _ => Method::Sum,
    }
}

/// The stored role of a loop of `face`.
fn declared(face: &Face, loop_id: &cadmpeg_ir::ids::LoopId) -> crate::report::DeclaredRole {
    match face.loop_role(loop_id) {
        cadmpeg_ir::topology::LoopBoundaryRole::Outer => crate::report::DeclaredRole::Outer,
        cadmpeg_ir::topology::LoopBoundaryRole::Inner => crate::report::DeclaredRole::Inner,
        cadmpeg_ir::topology::LoopBoundaryRole::Unspecified => {
            crate::report::DeclaredRole::Unspecified
        }
    }
}

/// A box value from an accumulated box.
pub(crate) fn box_value(aabb: Aabb, kind: BoxKind, boundary: Option<Aabb>) -> Option<BoxValue> {
    let min = Point::new(aabb.min_point())?.value;
    let max = Point::new(aabb.max_point())?.value;
    let boundary = match boundary {
        Some(boundary) => Some(BoundaryBox {
            min: Point::new(boundary.min_point())?.value,
            max: Point::new(boundary.max_point())?.value,
        }),
        None => None,
    };
    Some(BoxValue {
        min,
        max,
        kind,
        boundary,
    })
}

/// The radius `--radius` matches: cylinder `r`, sphere `|R|`, torus minor
/// `|r|`.
fn radius_of(carrier: &CarrierMeasures) -> Option<f64> {
    let radius = match &carrier.parameters {
        CarrierParameters::Cylinder { radius, .. } | CarrierParameters::Sphere { radius, .. } => {
            radius
        }
        CarrierParameters::Torus { minor_radius, .. } => minor_radius,
        _ => return None,
    };
    radius.measured().map(|radius| radius.value().value.get())
}
