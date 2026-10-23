// SPDX-License-Identifier: Apache-2.0
//! The measurement of one face: carrier, boundary, region, area, lengths,
//! box and orientation.

use std::collections::{BTreeSet, HashMap};

use cadmpeg_core::decode::ResourceLimit;
use cadmpeg_ir::source_fidelity::SourceFidelity;
use cadmpeg_ir::topology::Face;

mod readings;

use crate::boundary::{FaceFrame, LoopKind, ResolvedFace, ResolvedLoop};
use crate::canonical::placement::Placement;
use crate::canonical::surface::Chart;
use crate::canonical::{Carrier, Sources};
use crate::context::{Eval, Work};
use crate::edges::EdgeGeometry;
use crate::extent::{angular_sweep, axial_range, parameter_ranges, sphere_extent};
use crate::failure::{settle, Measure, Missing, Outcome};
use crate::flux::{Flux, FluxFrame, MEASURE_COMPONENTS, ORIENTATION_COMPONENTS};
use crate::geom::Aabb;
use crate::orientation::{declared_normal, face_orientation, opposite};
use crate::owners::Owners;
use crate::reason::{CaveatCode, UnavailableReason};
use crate::region::{classify, loop_shape, winding, FiniteContext, LoopShape, Region, RegionInput};
use crate::report::{
    CarrierMeasures, CarrierParameters, Caveat, CoedgeMeasures, FaceMeasures, FaceOrientation,
    FaceQuantities, ListedOrder, LoopMeasures, LoopRole, RegionRule, StatusTally, Witness,
    WitnessKind,
};
use crate::select::SurfaceKind;
use crate::tolerance::{allowance, model_tolerance};
use crate::value::{
    measured, BoundaryBox, BoxKind, BoxValue, Errors, Method, Point, Quantity, Range, Scalar,
    Sweep, Unit,
};
pub(crate) use readings::Verdict;
use readings::{face_area, AreaInput, Readings};

/// Everything measured about one face.
pub(crate) struct FaceResult<'ir> {
    pub(crate) row: FaceMeasures,
    /// The face's box under its reported reading, when measured.
    pub(crate) aabb: Option<Aabb>,
    /// The face's box under the declared and the complement reading.
    aabbs: [Option<Aabb>; 2],
    /// The values that depend on the reading of a declared-orientation
    /// face, under both readings.
    readings: FaceReadings,
    /// The face's chart, when it resolves.
    pub(crate) chart: Option<Chart<'ir>>,
    /// The allowance of the face's boundary.
    pub(crate) alpha: f64,
    /// The model tolerance of the face's boundary.
    pub(crate) tau: f64,
    /// The radius `--radius` matches, when the carrier has one.
    pub(crate) radius: Option<f64>,
    /// The face's fluxes, when the measurement needs them.
    pub(crate) flux: Option<Outcome<FaceFlux>>,
    /// What the readings of a declared-orientation face settle.
    pub(crate) verdict: Verdict,
    /// Whether the region reaches the carrier's natural domain edges.
    pub(crate) natural: bool,
    /// The model tolerance of the face's boundary with its measured gaps.
    pub(crate) tau_b: f64,
    /// The non-seam boundary length.
    pub(crate) boundary_length: f64,
}

impl FaceResult<'_> {
    /// The face's area under its declared reading, or under its complement
    /// reading when `complement` is set. A face whose region does not rest
    /// on its orientation has one area.
    pub(crate) fn area_reading(&self, complement: bool) -> &Quantity<Scalar> {
        match (&self.readings.area.complement, complement) {
            (Some(area), true) => area,
            _ => &self.readings.area.declared,
        }
    }

    /// Report the face's values and orientation under `verdict`, decided by
    /// `witness`.
    pub(crate) fn settle_reading(&mut self, verdict: Verdict, witness: Option<Witness>) {
        let id = self.row.face.clone();
        self.verdict = verdict;
        let readings = &self.readings;
        let measures = &mut self.row.measures;
        measures.area = readings.area.reported(verdict, &id);
        measures.bbox = readings.bbox.reported(verdict, &id);
        if let Some(latitude) = &readings.latitude {
            measures.latitude_range = Some(latitude.reported(verdict, &id));
        }
        if let Some(sweep) = &readings.sweep {
            measures.angular_sweep = Some(sweep.reported(verdict, &id));
        }
        self.aabb = match &measures.bbox {
            Quantity::Unavailable(_) => None,
            Quantity::Measured(_)
                if verdict == Verdict::Conflict && readings.bbox.complement.is_some() =>
            {
                self.aabbs[1]
            }
            Quantity::Measured(_) => self.aabbs[0],
        };
        let orientation = &mut self.row.orientation;
        let declared = orientation.declared_normal;
        match verdict {
            Verdict::Settled => {}
            Verdict::Confirmed => {
                orientation.witness = witness;
                orientation.consistent = Some(true);
                orientation.loop_normal = declared;
            }
            Verdict::Conflict | Verdict::Contradicted => {
                orientation.witness = witness;
                orientation.consistent = Some(false);
                orientation.loop_normal = opposite(declared);
            }
            Verdict::Unconfirmed => {
                orientation.witness = None;
                orientation.consistent = None;
            }
        }
        self.row
            .caveats
            .retain(|caveat| caveat.code != CaveatCode::OrientationUnconfirmed);
        if verdict == Verdict::Unconfirmed {
            self.row.caveats.push(Caveat::new(
                CaveatCode::OrientationUnconfirmed,
                Some(&id),
                "the face's region rests on its declared orientation and no witness confirms it",
            ));
            self.row.caveats.sort();
        }
        let mut tally = StatusTally::default();
        self.row.carrier.parameters.visit(&mut tally);
        self.row.measures.visit(&mut tally);
        self.row.status = tally.status();
    }
}

/// The values of a face that depend on its reading.
struct FaceReadings {
    area: Readings<Scalar>,
    bbox: Readings<BoxValue>,
    latitude: Option<Readings<Range>>,
    sweep: Option<Readings<Sweep>>,
}

/// A face's fluxes under its readings.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FaceFlux {
    /// The flux of the region the face's declared orientation states, or
    /// of its region when the rule does not rest on the orientation.
    pub(crate) declared: Flux,
    /// The flux of the complement region of a declared-orientation face.
    pub(crate) complement: Option<Flux>,
}

/// What one face measurement reads.
pub(crate) struct FaceInput<'a, 'ir> {
    pub(crate) work: Work,
    pub(crate) owners: &'a Owners<'ir>,
    pub(crate) edges: &'a HashMap<&'ir str, EdgeGeometry<'ir>>,
    pub(crate) fidelity: Option<&'a SourceFidelity>,
    pub(crate) body: Option<&'ir str>,
    pub(crate) placement: Outcome<Placement>,
    /// The reference point of the body's fluxes, when the measurement needs
    /// them.
    pub(crate) flux_origin: Option<cadmpeg_ir::math::Point3>,
}

/// The facts of a face boundary, from its resolution.
struct Boundary<'ir> {
    resolved: ResolvedFace<'ir>,
    shapes: Vec<Option<LoopShape>>,
}

fn unavailable<T>(reason: UnavailableReason, entity: &str, message: &str) -> Quantity<T> {
    Quantity::missing(reason, Some(entity), message.to_owned())
}

/// A quantity an orientation pass grants no work.
fn not_computed<T>(entity: &str) -> Quantity<T> {
    unavailable(
        UnavailableReason::EvaluationWorkExhausted,
        entity,
        "an orientation pass does not compute this quantity",
    )
}

/// The carrier row of a face whose carrier is not reported.
fn unreported_carrier(face: &Face) -> CarrierMeasures {
    CarrierMeasures {
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
    }
}

impl<'ir> FaceInput<'_, 'ir> {
    /// Measure one face.
    pub(crate) fn measure(
        &self,
        eval: &Eval<'_, '_>,
        face: &'ir Face,
    ) -> Result<FaceResult<'ir>, ResourceLimit> {
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
            (Ok(placement), Some(surface)) => settle(
                Carrier::resolve_in(owners, surface, *placement).map_err(|failure| failure.at(id)),
            )?,
            (Ok(_), None) => Err(Missing {
                reason: UnavailableReason::CarrierUnresolved,
                entity: Some(face.surface.as_str().to_owned()),
                message: "the face's surface does not resolve".to_owned(),
            }),
        };
        let row_work = self.work == Work::Row;
        let carrier_row = match &carrier {
            Ok(carrier) if row_work => carrier.measures(Sources {
                fidelity: self.fidelity,
                surface: carrier.id(),
                depth: carrier.depth,
                body: self.body.filter(|body| {
                    owners
                        .body(body)
                        .is_some_and(|body| body.transform.is_some())
                }),
            }),
            _ => unreported_carrier(face),
        };

        let chart: Outcome<Chart<'ir>> = match &carrier {
            Ok(carrier) => settle(carrier.chart().copied())?,
            Err(missing) => Err(missing.clone()),
        };
        let boundary: Outcome<Boundary<'ir>> = match (&chart, &self.placement) {
            (Ok(chart), Ok(placement)) => {
                settle(self.boundary(eval, face, chart, *placement, alpha))?
            }
            (Err(missing), _) | (_, Err(missing)) => Err(missing.clone()),
        };
        let declared = declared_normal(face, chart.as_ref().ok());
        let region: Outcome<Region> = match (&chart, &boundary) {
            (Ok(chart), Ok(boundary)) => {
                let v_tolerance = chart.axis().map_or(alpha, |axis| alpha / axis.h);
                let seams: Vec<Vec<bool>> = boundary
                    .resolved
                    .loops
                    .iter()
                    .map(|resolution| match &resolution.outcome {
                        Ok(resolved) => resolved
                            .pieces
                            .iter()
                            .map(|piece| {
                                piece
                                    .coedge
                                    .is_some_and(|coedge| FaceFrame::is_seam(owners, face, coedge))
                            })
                            .collect(),
                        Err(_) => Vec::new(),
                    })
                    .collect();
                let input = RegionInput {
                    eval: *eval,
                    context: FiniteContext {
                        implicit_outer: carrier
                            .as_ref()
                            .is_ok_and(|carrier| carrier.implicit_outer),
                        declared,
                    },
                    seams: &seams,
                };
                settle(
                    classify(
                        face,
                        chart,
                        &boundary.resolved,
                        &boundary.shapes,
                        v_tolerance,
                        &input,
                    )
                    .map_err(|failure| failure.at(id)),
                )?
            }
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
        let fit = carrier.as_ref().map_or(0.0, |carrier| carrier.fit);
        let tau_b = tau.max(fit).max(max_gap).max(off_surface);
        let hull = match &chart {
            Ok(Chart::Nurbs(nurbs)) => Some(nurbs.control_hull()),
            _ => None,
        };
        let carrier_size = chart
            .as_ref()
            .ok()
            .and_then(Chart::critical_box)
            .map_or(0.0, |aabb| aabb.diagonal());
        let scale = match (&hull, &boundary) {
            (Some(Ok(hull)), _) => hull.diagonal(),
            (_, Ok(boundary)) => boundary
                .shapes
                .iter()
                .flatten()
                .map(|shape| shape.diagonal)
                .fold(carrier_size, f64::max),
            _ => carrier_size,
        };
        let scale = if scale > 0.0 { scale } else { alpha };

        let lengths = self.lengths(face, &coedges);
        let placement_term = |dimension: u8, value: f64| match &self.placement {
            Ok(placement) => placement.model_term(dimension, value),
            Err(_) => 0.0,
        };

        let (area, verdict) = match (&chart, &boundary, &region) {
            (Ok(_), Ok(_), Ok(region)) if !row_work && self.flux_origin.is_none() => (
                Readings::single(not_computed(id)),
                if region.rule == RegionRule::DeclaredOrientation {
                    Verdict::Unconfirmed
                } else {
                    Verdict::Settled
                },
            ),
            (Ok(chart), Ok(boundary), Ok(region)) => {
                let resolved: Vec<Option<&ResolvedLoop<'_>>> = boundary
                    .resolved
                    .loops
                    .iter()
                    .map(|resolution| resolution.outcome.as_ref().ok())
                    .collect();
                let area_placement = |value: f64| placement_term(2, value);
                let input = AreaInput {
                    id,
                    chart,
                    region,
                    shapes: &boundary.shapes,
                    resolved: &resolved,
                    scale,
                    boundary_model: lengths.boundary * tau_b,
                    placement: &area_placement,
                };
                match settle(face_area(eval, &input).map_err(|failure| failure.at(id)))? {
                    Ok(face_area) => (face_area.area, face_area.verdict),
                    Err(missing) => (Readings::single(missing.quantity()), Verdict::Settled),
                }
            }
            (Err(missing), _, _) | (_, Err(missing), _) | (_, _, Err(missing)) => {
                (Readings::single(missing.quantity()), Verdict::Settled)
            }
        };

        let (bbox, aabbs) = match (&chart, &region) {
            _ if !row_work => (Readings::single(not_computed(id)), [None, None]),
            (Ok(chart @ (Chart::Sphere(_) | Chart::Torus(_))), Ok(region)) => self.closed_box(
                face,
                &coedges,
                &boundary,
                ClosedBox {
                    chart,
                    region,
                    tau_b,
                    alpha,
                },
            ),
            (chart, region) => {
                let (bbox, aabb) = self.plain_box(
                    face,
                    &coedges,
                    &boundary,
                    PlainBox {
                        chart,
                        region,
                        tau_b,
                        hull,
                    },
                );
                (Readings::single(bbox), [aabb, None])
            }
        };

        let directed: Vec<bool> = match &boundary {
            Ok(boundary) => boundary
                .resolved
                .loops
                .iter()
                .map(|resolution| {
                    resolution
                        .outcome
                        .as_ref()
                        .map_or(true, |resolved| resolved.directed)
                })
                .collect(),
            Err(_) => Vec::new(),
        };
        let orientation: FaceOrientation =
            face_orientation(declared, region.as_ref().ok(), &directed);
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

        let extents = if row_work {
            extent_quantities(
                eval,
                &ExtentInput {
                    id,
                    chart: &chart,
                    boundary: &boundary,
                    region: &region,
                    tau_b,
                    alpha,
                    angular: owners.ir.tolerances.angular.get(),
                    placement: self.placement.as_ref().ok(),
                },
            )?
        } else {
            Extents::default()
        };
        let readings = FaceReadings {
            area,
            bbox,
            latitude: extents.latitude_range,
            sweep: extents.angular_sweep,
        };
        let measures = FaceQuantities {
            area: readings.area.declared.clone(),
            boundary_length,
            seam_length,
            bbox: readings.bbox.declared.clone(),
            axial_range: extents.axial_range,
            axial_extent: extents.axial_extent,
            angular_sweep: readings.sweep.as_ref().map(|sweep| sweep.declared.clone()),
            latitude_range: readings
                .latitude
                .as_ref()
                .map(|latitude| latitude.declared.clone()),
            radius_range: extents.radius_range,
            u_range: extents.u_range,
            v_range: extents.v_range,
            region: region.as_ref().ok().map(|region| region.rule),
            loops,
        };
        let radius = radius_of(&carrier_row);
        let row = FaceMeasures {
            face: id.to_owned(),
            placement: crate::report::RowPlacement::Model {},
            name: face.name.clone(),
            body: self.body.map(str::to_owned),
            shell: face.shell.as_str().to_owned(),
            sense: face.sense,
            carrier: carrier_row,
            measures,
            orientation,
            caveats,
            status: crate::report::RowStatus::Measured,
        };
        let flux = match (self.flux_origin, &chart, &boundary, &region) {
            (None, ..) => None,
            (Some(origin), Ok(chart), Ok(boundary), Ok(region)) => {
                let resolved: Vec<Option<&ResolvedLoop<'_>>> = boundary
                    .resolved
                    .loops
                    .iter()
                    .map(|resolution| resolution.outcome.as_ref().ok())
                    .collect();
                let frame = FluxFrame {
                    chart,
                    origin,
                    scale,
                    active: match self.work {
                        Work::Row => MEASURE_COMPONENTS,
                        Work::Orientation => ORIENTATION_COMPONENTS,
                    },
                };
                Some(settle(
                    face_flux(eval, &frame, region, &resolved).map_err(|failure| failure.at(id)),
                )?)
            }
            (Some(_), Err(missing), _, _)
            | (Some(_), _, Err(missing), _)
            | (Some(_), _, _, Err(missing)) => Some(Err(missing.clone())),
        };
        let natural = region.as_ref().is_ok_and(|region| region.exterior != 0.0);
        let mut result = FaceResult {
            row,
            aabb: aabbs[0],
            aabbs,
            readings,
            chart: chart.as_ref().ok().copied(),
            alpha,
            tau,
            radius,
            flux,
            verdict,
            natural,
            tau_b,
            boundary_length: lengths.boundary,
        };
        let witness = matches!(verdict, Verdict::Confirmed | Verdict::Conflict).then(|| Witness {
            kind: WitnessKind::EmptyReading,
            entity: id.to_owned(),
            agrees: verdict == Verdict::Confirmed,
        });
        result.settle_reading(verdict, witness);
        Ok(result)
    }

    fn boundary(
        &self,
        eval: &Eval<'_, '_>,
        face: &'ir Face,
        chart: &Chart<'ir>,
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

    /// The box of a face on a plane, cylinder, cone or NURBS carrier.
    fn plain_box(
        &self,
        face: &Face,
        coedges: &[&'ir cadmpeg_ir::topology::Coedge],
        boundary: &Outcome<Boundary<'ir>>,
        basis: PlainBox<'_, '_>,
    ) -> (Quantity<BoxValue>, Option<Aabb>) {
        let id = face.id.as_str();
        let PlainBox {
            chart,
            region,
            tau_b,
            hull,
        } = basis;
        match (chart, region) {
            (Ok(Chart::Plane(_) | Chart::Cylinder(_)), Ok(_)) => self.boundary_box(
                face,
                coedges,
                boundary,
                BoxBasis {
                    tau_b,
                    member: None,
                    method: Method::BoundaryExtremum,
                },
            ),
            (Ok(Chart::Cone(_)), Ok(region)) => self.boundary_box(
                face,
                coedges,
                boundary,
                BoxBasis {
                    tau_b,
                    member: region.apex,
                    method: Method::CriticalPoints,
                },
            ),
            (Ok(Chart::Nurbs(_)), Ok(_)) => match hull {
                Some(Ok(hull)) => {
                    let (boundary_box, _) = self.boundary_box(
                        face,
                        coedges,
                        boundary,
                        BoxBasis {
                            tau_b,
                            member: None,
                            method: Method::BoundaryExtremum,
                        },
                    );
                    let boundary_aabb = boundary_box.measured().map(|measured| {
                        let value = measured.value();
                        let mut aabb = Aabb::of_point(value.min.get());
                        aabb.include(value.max.get());
                        aabb
                    });
                    (
                        measured(
                            box_value(hull, BoxKind::Conservative, boundary_aabb),
                            Unit::Millimeter,
                            Method::ControlHull,
                            Errors::new(0.0, tau_b),
                            id,
                        ),
                        Some(hull),
                    )
                }
                Some(Err(reason)) => (
                    unavailable(reason, id, "the control net bounds no box"),
                    None,
                ),
                None => (
                    unavailable(
                        UnavailableReason::BboxInteriorUnbounded,
                        id,
                        "the carrier has no control net",
                    ),
                    None,
                ),
            },
            (Ok(_), Ok(_)) => (
                unavailable(
                    UnavailableReason::RegionDomainUnsupported,
                    id,
                    "boxes on this carrier are not computed",
                ),
                None,
            ),
            (Err(missing), _) | (_, Err(missing)) => (missing.quantity(), None),
        }
    }

    /// The edge boxes and vertex-loop points of a face, and the largest
    /// numeric error of the edge boxes; `None` without boundary pieces.
    fn boundary_aabb(
        &self,
        coedges: &[&'ir cadmpeg_ir::topology::Coedge],
        boundary: &Outcome<Boundary<'ir>>,
    ) -> Result<(Option<Aabb>, f64), Missing> {
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
                Err(missing) => return Err(missing.clone()),
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
        Ok((aabb, numeric_error))
    }

    /// The box of a sphere or torus face under both readings: the boundary
    /// box joined with the critical points that are members of each
    /// reading's region.
    fn closed_box(
        &self,
        face: &Face,
        coedges: &[&'ir cadmpeg_ir::topology::Coedge],
        boundary: &Outcome<Boundary<'ir>>,
        basis: ClosedBox<'_, '_>,
    ) -> (Readings<BoxValue>, [Option<Aabb>; 2]) {
        let id = face.id.as_str();
        let ClosedBox {
            chart,
            region,
            tau_b,
            alpha,
        } = basis;
        let (edges, numeric_error) = match self.boundary_aabb(coedges, boundary) {
            Ok(found) => found,
            Err(missing) => return (Readings::single(missing.quantity()), [None, None]),
        };
        let Some(closed) = &region.closed else {
            return (
                Readings::single(unavailable(
                    UnavailableReason::RegionDomainUnsupported,
                    id,
                    "the region has no closed-domain membership",
                )),
                [None, None],
            );
        };
        let poles = chart.poles();
        let mut boxes = [edges, edges];
        for (uv, point) in chart.critical_points() {
            let pole = poles.and_then(|poles| {
                poles
                    .iter()
                    .position(|(_, pole)| pole.distance(point) <= alpha)
            });
            let member = match pole {
                Some(0) => closed.membership.end(-1.0),
                Some(_) => closed.membership.end(1.0),
                None => closed.membership.member(uv),
            };
            let slot = &mut boxes[usize::from(!member)];
            *slot = Some(slot.map_or(Aabb::of_point(point), |aabb| {
                aabb.union(Aabb::of_point(point))
            }));
        }
        let quantity = |aabb: Option<Aabb>| match aabb {
            Some(aabb) => measured(
                box_value(aabb, BoxKind::Tight, edges),
                Unit::Millimeter,
                Method::CriticalPoints,
                Errors::new(numeric_error, tau_b),
                id,
            ),
            None => unavailable(
                UnavailableReason::BoundaryUnboundedFace,
                id,
                "the region has no point",
            ),
        };
        let readings = Readings {
            declared: quantity(boxes[0]),
            complement: (region.rule == RegionRule::DeclaredOrientation)
                .then(|| quantity(boxes[1])),
        };
        (readings, boxes)
    }

    /// The box of the face's boundary pieces (its edges and vertex loops),
    /// joined with a member interior point.
    fn boundary_box(
        &self,
        face: &Face,
        coedges: &[&'ir cadmpeg_ir::topology::Coedge],
        boundary: &Outcome<Boundary<'ir>>,
        basis: BoxBasis,
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
        let mut face_box = aabb;
        if let Some(member) = basis.member {
            face_box.include(member);
        }
        let value = box_value(face_box, BoxKind::Tight, Some(aabb));
        (
            measured(
                value,
                Unit::Millimeter,
                basis.method,
                Errors::new(numeric_error, basis.tau_b),
                id,
            ),
            Some(face_box),
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

/// A face's fluxes under its readings.
fn face_flux(
    eval: &Eval<'_, '_>,
    frame: &FluxFrame<'_, '_>,
    region: &Region,
    resolved: &[Option<&ResolvedLoop<'_>>],
) -> Measure<FaceFlux> {
    let declared = frame.region_flux(eval, region, resolved)?;
    let complement = if region.rule == RegionRule::DeclaredOrientation {
        Some(frame.domain_flux(eval, region)?.add(declared, -1.0))
    } else {
        None
    };
    Ok(FaceFlux {
        declared,
        complement,
    })
}

/// What the box of a face on a plane, cylinder, cone or NURBS carrier
/// reads.
#[derive(Clone, Copy)]
struct PlainBox<'a, 'ir> {
    chart: &'a Outcome<Chart<'ir>>,
    region: &'a Outcome<Region>,
    tau_b: f64,
    hull: Option<Result<Aabb, UnavailableReason>>,
}

/// What the box of a face on a sphere or torus reads.
#[derive(Clone, Copy)]
struct ClosedBox<'a, 'ir> {
    chart: &'a Chart<'ir>,
    region: &'a Region,
    tau_b: f64,
    alpha: f64,
}

/// What a face box joins to its boundary box, and how.
#[derive(Clone, Copy)]
struct BoxBasis {
    tau_b: f64,
    /// An interior critical point of the carrier that lies in the region.
    member: Option<cadmpeg_ir::math::Point3>,
    method: Method,
}

/// What the extents of a face read.
struct ExtentInput<'a, 'ir> {
    id: &'a str,
    chart: &'a Outcome<Chart<'ir>>,
    boundary: &'a Outcome<Boundary<'ir>>,
    region: &'a Outcome<Region>,
    tau_b: f64,
    alpha: f64,
    angular: f64,
    placement: Option<&'a Placement>,
}

impl ExtentInput<'_, '_> {
    /// The placement term of a value of `dimension`.
    fn placement_term(&self, dimension: u8, value: f64) -> f64 {
        self.placement
            .map_or(0.0, |placement| placement.model_term(dimension, value))
    }
}

/// The kind-specific extents of a face.
#[derive(Default)]
struct Extents {
    axial_range: Option<Quantity<Range>>,
    axial_extent: Option<Quantity<Scalar>>,
    angular_sweep: Option<Readings<Sweep>>,
    latitude_range: Option<Readings<Range>>,
    radius_range: Option<Quantity<Range>>,
    u_range: Option<Quantity<Range>>,
    v_range: Option<Quantity<Range>>,
}

/// The extents that apply to the face's carrier kind: measured, or
/// unavailable with the first failure upstream of them.
fn extent_quantities(
    eval: &Eval<'_, '_>,
    input: &ExtentInput<'_, '_>,
) -> Result<Extents, ResourceLimit> {
    let id = input.id;
    let Ok(chart) = input.chart else {
        return Ok(Extents::default());
    };
    let upstream = match (input.boundary, input.region) {
        (Ok(boundary), Ok(region)) => Ok((boundary, region)),
        (Err(missing), _) | (_, Err(missing)) => Err(missing.clone()),
    };
    let mut extents = Extents::default();
    match chart {
        Chart::Plane(_) => {}
        Chart::Nurbs(_) => match upstream {
            Ok((boundary, region)) => {
                match settle(
                    parameter_ranges(eval, chart, &boundary.resolved, region)
                        .map_err(|failure| failure.at(id)),
                )? {
                    Ok(ranges) => {
                        let [u, v] = ranges.map(|range| {
                            measured(
                                Range::new(range.min, range.max),
                                Unit::One,
                                Method::BoundaryExtremum,
                                Errors::new(range.numeric_error, 0.0),
                                id,
                            )
                        });
                        extents.u_range = Some(u);
                        extents.v_range = Some(v);
                    }
                    Err(missing) => {
                        extents.u_range = Some(missing.quantity());
                        extents.v_range = Some(missing.quantity());
                    }
                }
            }
            Err(missing) => {
                extents.u_range = Some(missing.quantity());
                extents.v_range = Some(missing.quantity());
            }
        },
        Chart::Sphere(sphere) => {
            let (latitude, sweep) = sphere_readings(eval, input, chart, sphere.radius.abs())?;
            extents.latitude_range = Some(latitude);
            extents.angular_sweep = Some(sweep);
        }
        Chart::Torus(_) => {
            extents.angular_sweep = Some(Readings::single(unavailable(
                UnavailableReason::SweepUnsupportedCarrier,
                id,
                "the angular sweep of a torus face is not computed",
            )));
        }
        Chart::Cylinder(_) | Chart::Cone(_) => {
            let Some(axis) = chart.axis() else {
                return Ok(extents);
            };
            let (boundary, region) = match upstream {
                Ok(pair) => pair,
                Err(missing) => {
                    extents.axial_range = Some(missing.quantity());
                    extents.axial_extent = Some(missing.quantity());
                    extents.angular_sweep = Some(Readings::single(missing.quantity()));
                    if matches!(chart, Chart::Cone(_)) {
                        extents.radius_range = Some(missing.quantity());
                    }
                    return Ok(extents);
                }
            };
            let range = settle(
                axial_range(eval, &axis, &boundary.resolved, region)
                    .map_err(|failure| failure.at(id)),
            )?;
            match &range {
                Ok(range) => {
                    extents.axial_range = Some(measured(
                        Range::new(range.min, range.max),
                        Unit::Millimeter,
                        Method::BoundaryExtremum,
                        Errors::new(range.numeric_error, input.tau_b),
                        id,
                    ));
                    let extent = range.max - range.min;
                    extents.axial_extent = Some(measured(
                        Scalar::new(extent),
                        Unit::Millimeter,
                        Method::BoundaryExtremum,
                        Errors::new(
                            2.0 * range.numeric_error,
                            2.0 * input.tau_b + input.placement_term(1, extent),
                        ),
                        id,
                    ));
                }
                Err(missing) => {
                    extents.axial_range = Some(missing.quantity());
                    extents.axial_extent = Some(missing.quantity());
                }
            }
            let radius = match (&range, chart) {
                (Ok(range), Chart::Cone(_)) => {
                    let ends =
                        [range.min / axis.h, range.max / axis.h].map(|v| chart.section_radius(v));
                    match ends {
                        [Some(low), Some(high)] => {
                            let minimum = if region.apex.is_some() {
                                0.0
                            } else {
                                low.min(high)
                            };
                            Some((minimum, low.max(high), range.numeric_error))
                        }
                        _ => None,
                    }
                }
                (Ok(_), _) => chart
                    .section_radius(0.0)
                    .map(|radius| (radius, radius, 0.0)),
                (Err(_), _) => None,
            };
            if let Chart::Cone(cone) = chart {
                extents.radius_range = Some(match (&range, radius) {
                    (Err(missing), _) => missing.quantity(),
                    (Ok(_), None) => unavailable(
                        UnavailableReason::CarrierEllipticCone,
                        id,
                        "the cross-section of an elliptic cone has no radius",
                    ),
                    (Ok(_), Some((minimum, maximum, error))) => measured(
                        Range::new(minimum, maximum),
                        Unit::Millimeter,
                        Method::BoundaryExtremum,
                        Errors::new(
                            (cone.tau / cone.h).abs() * error,
                            input.tau_b + input.placement_term(1, maximum),
                        ),
                        id,
                    ),
                });
            }
            extents.angular_sweep = Some(Readings::single(if chart.elliptic() {
                unavailable(
                    UnavailableReason::SweepUnsupportedCarrier,
                    id,
                    "the angular sweep of an elliptic cone face is not computed",
                )
            } else {
                let largest = radius.map_or(0.0, |(_, maximum, _)| maximum);
                let tolerance = if largest > 0.0 {
                    input.angular.max(input.alpha / largest)
                } else {
                    input.angular
                };
                match settle(
                    angular_sweep(eval, chart, &boundary.resolved, tolerance)
                        .map_err(|failure| failure.at(id)),
                )? {
                    Ok(sweep) => measured(
                        Sweep::new(sweep.value, sweep.full, &sweep.intervals),
                        Unit::Radian,
                        Method::BoundaryExtremum,
                        Errors::new(
                            sweep.numeric_error,
                            if largest > 0.0 {
                                input.tau_b / largest
                            } else {
                                0.0
                            },
                        ),
                        id,
                    ),
                    Err(missing) => missing.quantity(),
                }
            }));
        }
    }
    Ok(extents)
}

/// The latitude range and angular sweep of a sphere face under both
/// readings.
fn sphere_readings(
    eval: &Eval<'_, '_>,
    input: &ExtentInput<'_, '_>,
    chart: &Chart<'_>,
    radius: f64,
) -> Result<(Readings<Range>, Readings<Sweep>), ResourceLimit> {
    let id = input.id;
    let failed = |missing: Missing| {
        (
            Readings::single(missing.quantity()),
            Readings::single(missing.quantity()),
        )
    };
    let (boundary, region) = match (input.boundary, input.region) {
        (Ok(boundary), Ok(region)) => (boundary, region),
        (Err(missing), _) | (_, Err(missing)) => return Ok(failed(missing.clone())),
    };
    let Some(closed) = &region.closed else {
        return Ok(failed(Missing {
            reason: UnavailableReason::RegionDomainUnsupported,
            entity: Some(id.to_owned()),
            message: "the region has no closed-domain membership".to_owned(),
        }));
    };
    let tolerance = input.angular.max(input.alpha / radius);
    let model = input.tau_b / radius;
    let mirrored = matches!(chart, Chart::Sphere(sphere) if sphere.radius < 0.0);
    let reading = |complement: bool| -> Result<(Quantity<Range>, Quantity<Sweep>), ResourceLimit> {
        let found = settle(
            sphere_extent(
                eval,
                chart,
                &boundary.resolved,
                &closed.membership,
                complement,
                tolerance,
            )
            .map_err(|failure| failure.at(id)),
        )?;
        let found = found.map(|extent| if mirrored { extent.mirrored() } else { extent });
        Ok(match found {
            Ok(extent) => (
                measured(
                    Range::new(extent.latitude.min, extent.latitude.max),
                    Unit::Radian,
                    Method::BoundaryExtremum,
                    Errors::new(extent.latitude.numeric_error, model),
                    id,
                ),
                measured(
                    Sweep::new(
                        extent.sweep.value,
                        extent.sweep.full,
                        &extent.sweep.intervals,
                    ),
                    Unit::Radian,
                    Method::BoundaryExtremum,
                    Errors::new(extent.sweep.numeric_error, model),
                    id,
                ),
            ),
            Err(missing) => (missing.quantity(), missing.quantity()),
        })
    };
    let (latitude, sweep) = reading(false)?;
    let (latitude_complement, sweep_complement) = if region.rule == RegionRule::DeclaredOrientation
    {
        let (latitude, sweep) = reading(true)?;
        (Some(latitude), Some(sweep))
    } else {
        (None, None)
    };
    Ok((
        Readings {
            declared: latitude,
            complement: latitude_complement,
        },
        Readings {
            declared: sweep,
            complement: sweep_complement,
        },
    ))
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
