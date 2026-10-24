// SPDX-License-Identifier: Apache-2.0
//! Check findings from one orientation pass.
//!
//! The pass reads the defect classes that measurement exposes and reports
//! each as a warning:
//!
//! - [`Check::RingOrder`] on a loop: the ring's coedges form one closed
//!   traversal by vertex identity in another order, several closed
//!   traversals, or none.
//! - [`Check::PcurveOrder`] on a coedge: its pcurve uses form one chain
//!   from its end to its start, the `boundary.pcurve-order` caveat of its
//!   face.
//! - [`Check::Orientation`] on a face: its sense disagrees with the winding
//!   of its loops, or its loops disagree with each other.
//! - [`Check::Orientation`] on a body whose shells are closed: its volume
//!   readings conflict (`volume.orientation-conflict`), or the flux of the
//!   unit field exceeds its tolerance (`volume.closure-flux`).
//!
//! The pass resolves boundaries, regions, orientation readings and
//! witnesses for every face, and areas and fluxes for the faces of bodies
//! whose shells use every edge twice with opposite senses. It computes no
//! carrier parameter, box or extent. Every item draws a slice of one
//! findings allowance; an item whose slice runs out gives no finding, and
//! one [`Check::MeasurementIncomplete`] info finding states how many faces
//! and bodies the pass did not check.

use std::collections::{BTreeSet, HashMap};

use cadmpeg_core::decode::{DecodeContext, ResourceDimension, ResourceFailure, ResourceLimit};
use cadmpeg_ir::report::check::{Check, Finding};
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::topology::LoopBoundary;

use super::Measurer;
use crate::body::{closure, BodyInput};
use crate::budget::PassAllowance;
use crate::canonical::placement::Placement;
use crate::context::{Eval, Work};
use crate::edges::{measure_edge, EdgeGeometry};
use crate::face::{FaceInput, FaceResult, Verdict};
use crate::failure::{settle, Outcome};
use crate::reason::CaveatCode;
use crate::report::{ListedOrder, NormalReading};
use crate::traversal::{traverse_counted, TraversalError};
use crate::witness::WitnessInput;

/// The serialized name of a normal reading.
const fn reading_name(reading: NormalReading) -> &'static str {
    match reading {
        NormalReading::Surface => "surface",
        NormalReading::Reversed => "reversed",
        NormalReading::Mixed => "mixed",
        NormalReading::NotDeterminable => "not_determinable",
    }
}

fn warning(check: Check, entity: &str, related: Vec<String>, message: String) -> Finding {
    Finding {
        check,
        severity: Severity::Warning,
        message,
        entity: Some(entity.to_owned()),
        related,
    }
}

impl<'ir> Measurer<'ir> {
    /// The ring-order and orientation findings of the document, under one
    /// pass allowance. A session refusal of `ctx` ends the call.
    pub(super) fn orientation_findings(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Vec<Finding>, ResourceLimit> {
        let model = &self.input.ir.model;
        let items = model.faces.len() + model.edges.len();
        let Some(allowance) = self.options.work.findings_allowance(items) else {
            return Err(ResourceLimit {
                dimension: ResourceDimension::Entities,
                reason: ResourceFailure::BudgetExceeded,
                limit: 0,
                used: cadmpeg_core::decode::u64_from_index(items),
                additional: 0,
                operation: "measurement findings allowance",
            });
        };
        self.findings_under(ctx, &PassAllowance::findings(allowance))
    }

    /// The findings under `pass`. A face or edge whose slice refused a
    /// charge is starved: no finding names it or the body that owns it, and
    /// the info finding counts the faces and bodies left unchecked.
    fn findings_under(
        &self,
        ctx: &DecodeContext<'_>,
        pass: &PassAllowance,
    ) -> Result<Vec<Finding>, ResourceLimit> {
        let model = &self.input.ir.model;
        let (mut findings, mut unchecked) = self.ring_findings(pass);
        let mut starved: BTreeSet<&'ir str> = BTreeSet::new();

        let placements: HashMap<&'ir str, Outcome<Placement>> = model
            .bodies
            .iter()
            .map(|body| {
                let placement = match body.transform {
                    Some(transform) => settle(Placement::of(transform)),
                    None => Ok(Ok(Placement::identity())),
                };
                placement.map(|placement| (body.id.as_str(), placement))
            })
            .collect::<Result<_, ResourceLimit>>()?;
        let placement_of = |body: Option<&str>| -> Outcome<Placement> {
            body.and_then(|body| placements.get(body))
                .cloned()
                .unwrap_or(Ok(Placement::identity()))
        };

        let mut needed_edges: BTreeSet<&'ir str> = BTreeSet::new();
        for face in &model.faces {
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
        let mut edges: HashMap<&'ir str, EdgeGeometry<'ir>> = HashMap::new();
        for edge_id in needed_edges {
            let Some(edge) = self.owners.edge(edge_id) else {
                continue;
            };
            let slice = pass.edge_slice();
            let eval = Eval {
                ctx,
                index: &self.index,
                work: &slice,
            };
            let placement = placement_of(self.owners.edge_body(edge_id));
            let geometry = measure_edge(&eval, &self.owners, edge, &placement, Work::Orientation)?;
            if slice.exhausted() {
                starved.insert(edge_id);
            }
            edges.insert(edge_id, geometry);
        }

        let no_faces: HashMap<&'ir str, FaceResult<'ir>> = HashMap::new();
        let flux_bodies: BTreeSet<&'ir str> = model
            .bodies
            .iter()
            .filter(|body| {
                let shells: Vec<_> = body
                    .regions
                    .iter()
                    .filter_map(|region| self.owners.region(region.as_str()))
                    .flat_map(|region| &region.shells)
                    .filter_map(|shell| self.owners.shell(shell.as_str()))
                    .collect();
                !shells.is_empty()
                    && shells.iter().all(|shell| {
                        !shell.faces().is_empty()
                            && closure(&self.owners, &no_faces, &edges, shell).closed()
                    })
            })
            .map(|body| body.id.as_str())
            .collect();
        let flux_origins: HashMap<&'ir str, cadmpeg_ir::math::Point3> = flux_bodies
            .iter()
            .map(|id| (*id, self.flux_origin(id, &edges, &placement_of(Some(id)))))
            .collect();

        let mut faces: HashMap<&'ir str, FaceResult<'ir>> = HashMap::new();
        for face in &model.faces {
            let body = self.owners.face_body(face);
            let slice = pass.face_slice();
            let eval = Eval {
                ctx,
                index: &self.index,
                work: &slice,
            };
            let input = FaceInput {
                work: Work::Orientation,
                owners: &self.owners,
                edges: &edges,
                fidelity: None,
                body,
                placement: placement_of(body),
                flux_origin: body.and_then(|body| flux_origins.get(body).copied()),
            };
            let result = input.measure(&eval, face)?;
            let uses_starved = face
                .loops
                .iter()
                .filter_map(|loop_id| self.owners.loop_(loop_id.as_str()))
                .flat_map(cadmpeg_ir::topology::Loop::coedges)
                .filter_map(|coedge| self.owners.coedge(coedge.as_str()))
                .any(|coedge| starved.contains(coedge.edge.as_str()));
            if slice.exhausted() || uses_starved {
                starved.insert(face.id.as_str());
            }
            faces.insert(face.id.as_str(), result);
        }
        let witness_slice = pass.face_slice();
        WitnessInput {
            owners: &self.owners,
            edges: &edges,
            pass,
            eval: Eval {
                ctx,
                index: &self.index,
                work: &witness_slice,
            },
            placements: &placements,
        }
        .run(&mut faces)?;
        if let Some(limit) = ctx.resource_refusal() {
            return Err(limit);
        }

        for face in &model.faces {
            if starved.contains(face.id.as_str()) {
                unchecked.insert(face.id.as_str());
                continue;
            }
            let Some(result) = faces.get(face.id.as_str()) else {
                continue;
            };
            findings.extend(pcurve_order_findings(result));
            if let Some(finding) = face_finding(result) {
                findings.push(finding);
            }
        }
        let bodies = BodyInput {
            owners: &self.owners,
            faces: &faces,
            edges: &edges,
            origins: &flux_origins,
            placements: &placements,
            density: None,
        };
        let mut unchecked_bodies = 0_usize;
        for body in &model.bodies {
            let id = body.id.as_str();
            let starved_member = witness_slice.exhausted()
                || self
                    .owners
                    .body_faces(id)
                    .any(|face| starved.contains(face))
                || self
                    .owners
                    .body_edges(id)
                    .any(|edge| starved.contains(edge));
            if !flux_bodies.contains(id) {
                continue;
            }
            if starved_member {
                unchecked_bodies += 1;
                continue;
            }
            if let Some(conflict) = bodies.volume_conflict(body) {
                findings.push(warning(
                    Check::Orientation,
                    body.id.as_str(),
                    conflict.related,
                    format!(
                        "the body's volume readings conflict ({}): {}",
                        conflict.reason.code(),
                        conflict.message
                    ),
                ));
            }
        }
        if !unchecked.is_empty() || unchecked_bodies > 0 {
            findings.push(Finding {
                check: Check::MeasurementIncomplete,
                severity: Severity::Info,
                message: format!(
                    "measurement checks were skipped: the findings allowance ran out before {} \
                     and {} were checked",
                    counted(unchecked.len(), "face", "faces"),
                    counted(unchecked_bodies, "body", "bodies")
                ),
                entity: None,
                related: Vec::new(),
            });
        }
        Ok(findings)
    }

    /// One finding per face ring whose listed coedges are not a closed
    /// traversal by vertex identity, and the faces with a ring left
    /// unchecked when the face's slice ran out.
    fn ring_findings(&self, pass: &PassAllowance) -> (Vec<Finding>, BTreeSet<&'ir str>) {
        let mut findings = Vec::new();
        let mut unchecked = BTreeSet::new();
        for face in &self.input.ir.model.faces {
            let slice = pass.face_slice();
            for loop_id in &face.loops {
                let Some(face_loop) = self.owners.loop_(loop_id.as_str()) else {
                    continue;
                };
                let LoopBoundary::Ring(ring) = &face_loop.boundary else {
                    continue;
                };
                let Some(coedges) = ring
                    .coedges()
                    .iter()
                    .map(|coedge| self.owners.coedge(coedge.as_str()))
                    .collect::<Option<Vec<_>>>()
                else {
                    continue;
                };
                let ends: Vec<(&str, &str)> = coedges
                    .iter()
                    .map(|coedge| self.owners.oriented_ends(coedge))
                    .collect();
                let Some(steps) = slice.remaining().checked_sub(ends.len()) else {
                    unchecked.insert(face.id.as_str());
                    break;
                };
                let (result, work) = traverse_counted(&ends, steps);
                if slice.charge(work).is_err() {
                    unchecked.insert(face.id.as_str());
                    break;
                }
                let message = match result {
                    Ok(traversal) => match traversal.listed {
                        ListedOrder::Traversal => continue,
                        ListedOrder::Reversed => {
                            "the ring lists its coedges in reversed traversal order"
                        }
                        ListedOrder::Other => "the ring lists its coedges out of traversal order",
                    },
                    Err(TraversalError::Failed) => {
                        "the ring's coedges form no closed traversal by vertex identity"
                    }
                    Err(TraversalError::Ambiguous) => {
                        "the ring's coedges are not listed in traversal order and form several \
                         closed traversals"
                    }
                    Err(TraversalError::Exhausted) => {
                        unchecked.insert(face.id.as_str());
                        continue;
                    }
                };
                findings.push(warning(
                    Check::RingOrder,
                    face_loop.id.as_str(),
                    vec![face.id.as_str().to_owned()],
                    message.to_owned(),
                ));
            }
        }
        (findings, unchecked)
    }
}

/// `count` followed by the singular or plural noun.
fn counted(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

/// One finding per coedge of the face whose pcurve uses are listed from its
/// end to its start.
fn pcurve_order_findings<'a>(result: &'a FaceResult<'_>) -> impl Iterator<Item = Finding> + 'a {
    result
        .row
        .caveats
        .iter()
        .filter(|caveat| caveat.code() == CaveatCode::BoundaryPcurveOrder)
        .filter_map(|caveat| caveat.entity.as_deref())
        .map(|coedge| {
            warning(
                Check::PcurveOrder,
                coedge,
                vec![result.row.face.clone()],
                "the coedge lists its pcurve uses from its end to its start".to_owned(),
            )
        })
}

/// The finding of a face whose sense disagrees with its loop winding, or
/// whose loops disagree with each other. A face whose witness decides its
/// reading is reported through its body's volume.
fn face_finding(result: &FaceResult<'_>) -> Option<Finding> {
    if matches!(result.verdict, Verdict::Conflict | Verdict::Contradicted) {
        return None;
    }
    let orientation = &result.row.orientation;
    if orientation.consistent != Some(false) {
        return None;
    }
    let message = match orientation.loop_normal {
        NormalReading::Mixed => {
            "the face's loops disagree with each other on its oriented normal".to_owned()
        }
        loops => format!(
            "the face's sense gives the {} normal and its loop winding the {} normal",
            reading_name(orientation.declared_normal),
            reading_name(loops)
        ),
    };
    Some(warning(
        Check::Orientation,
        &result.row.face,
        Vec::new(),
        message,
    ))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use cadmpeg_ir::report::check::{Check, Finding};
    use cadmpeg_ir::report::Severity;
    use cadmpeg_ir::topology::Sense;

    use crate::budget::PassAllowance;
    use crate::canonical::placement::Placement;
    use crate::context::{Eval, Work};
    use crate::edges::measure_edge;
    use crate::face::FaceInput;
    use crate::flux::Flux;
    use crate::session::{MeasureInput, MeasureOptions, Measurer};
    use crate::test_support::builders::{
        rotated_cuboid, rotation, service_context, TEST_ABSOLUTE_FLOOR,
    };

    /// The cube with its first face flipped and its first ring listed in
    /// reversed order: one ring-order and two orientation findings.
    fn defective_cube() -> cadmpeg_ir::CadIr {
        let mut cube = cadmpeg_ir::examples::unit_cube().expect("fixture");
        cube.model.faces[0].sense = match cube.model.faces[0].sense {
            Sense::Forward => Sense::Reversed,
            Sense::Reversed => Sense::Forward,
        };
        let face_loop = &mut cube.model.loops[0];
        let mut coedges = face_loop.coedges().to_vec();
        coedges.reverse();
        face_loop.replace_ring(coedges, Vec::new()).expect("ring");
        cube
    }

    fn findings_under(allowance: usize) -> Vec<Finding> {
        let cube = defective_cube();
        let ctx = service_context();
        let input = MeasureInput {
            ir: &cube,
            fidelity: None,
            geometry_transferred: true,
        };
        Measurer::new(input, MeasureOptions::default())
            .findings_under(&ctx, &PassAllowance::findings(allowance))
            .expect("findings")
    }

    fn checks(found: &[Finding]) -> Vec<Check> {
        found.iter().map(|finding| finding.check).collect()
    }

    /// The one info finding of a pass that ran out, with the faces and
    /// bodies it did not check.
    fn incomplete(found: &[Finding]) -> &Finding {
        let [finding] = found
            .iter()
            .filter(|finding| finding.check == Check::MeasurementIncomplete)
            .collect::<Vec<_>>()[..]
        else {
            panic!("one measurement_incomplete finding: {found:#?}");
        };
        assert_eq!(finding.severity, Severity::Info);
        assert!(finding.entity.is_none());
        finding
    }

    #[test]
    fn an_exhausted_pass_states_what_it_did_not_check() {
        let full = findings_under(1 << 26);
        assert_eq!(
            checks(&full),
            [Check::RingOrder, Check::Orientation, Check::Orientation]
        );
        let empty = findings_under(0);
        assert_eq!(checks(&empty), [Check::MeasurementIncomplete]);
        assert!(
            incomplete(&empty)
                .message
                .ends_with("before 6 faces and 1 body were checked"),
            "{}",
            incomplete(&empty).message
        );
    }

    #[test]
    fn a_pass_that_pays_only_the_rings_gives_only_the_ring_finding() {
        let cube = defective_cube();
        let owners = crate::owners::Owners::new(&cube);
        let rings: usize = cube
            .model
            .loops
            .iter()
            .map(|face_loop| {
                let ends: Vec<(&str, &str)> = face_loop
                    .coedges()
                    .iter()
                    .map(|coedge| owners.oriented_ends(owners.coedge(coedge.as_str()).unwrap()))
                    .collect();
                crate::traversal::traverse_counted(&ends, usize::MAX).1
            })
            .sum();
        let found = findings_under(rings);
        assert_eq!(
            checks(&found),
            [Check::RingOrder, Check::MeasurementIncomplete]
        );
        assert!(incomplete(&found)
            .message
            .ends_with("before 6 faces and 1 body were checked"));
    }

    /// The volume and closure fluxes of every planar face of `ir`, from the
    /// findings pass and from a measurement pass, about one flux origin.
    fn planar_fluxes(ir: &cadmpeg_ir::CadIr) -> Vec<(Flux, Flux)> {
        let ctx = service_context();
        let input = MeasureInput {
            ir,
            fidelity: None,
            geometry_transferred: true,
        };
        let measurer = Measurer::new(input, MeasureOptions::default());
        let pass = PassAllowance::new(1 << 30);
        let placement = Ok(Placement::identity());
        let mut edges = HashMap::new();
        for edge in &ir.model.edges {
            let slice = pass.edge_slice();
            let eval = Eval {
                ctx: &ctx,
                index: &measurer.index,
                work: &slice,
            };
            let geometry = measure_edge(&eval, &measurer.owners, edge, &placement, Work::Row)
                .expect("session stays unrefused");
            edges.insert(edge.id.as_str(), geometry);
        }
        let body = ir.model.bodies[0].id.as_str();
        let origin = measurer.flux_origin(body, &edges, &placement);
        let mut fluxes = Vec::new();
        for face in &ir.model.faces {
            let [row, orientation] = [Work::Row, Work::Orientation].map(|work| {
                let slice = pass.face_slice();
                let eval = Eval {
                    ctx: &ctx,
                    index: &measurer.index,
                    work: &slice,
                };
                let result = FaceInput {
                    work,
                    owners: &measurer.owners,
                    edges: &edges,
                    fidelity: None,
                    body: Some(body),
                    placement: placement.clone(),
                    flux_origin: Some(origin),
                }
                .measure(&eval, face)
                .expect("session stays unrefused");
                result
                    .flux
                    .expect("a flux")
                    .expect("a measured flux")
                    .declared
            });
            fluxes.push((row, orientation));
        }
        ctx.finish_session().expect("session stays unrefused");
        fluxes
    }

    /// On a plane `d·N` is constant over the face, so the findings pass
    /// takes the volume and closure fluxes from the area: they agree with
    /// the quadrature of a measurement pass within both error estimates.
    #[test]
    fn planar_findings_fluxes_agree_with_the_measured_fluxes() {
        let half = [500.0, 0.5, 2.0];
        for ir in [
            rotated_cuboid(
                &rotation([1.0, 2.0, 3.0], 0.7),
                [5.0, 7.0, 9.0],
                half.map(|h| -h),
                half,
            ),
            crate::test_support::builders::plate_with_hole(true),
        ] {
            let fluxes = planar_fluxes(&ir);
            assert!(!fluxes.is_empty());
            for (row, orientation) in fluxes {
                let pairs = std::iter::once((
                    row.volume(),
                    orientation.volume(),
                    row.volume_error() + orientation.volume_error(),
                ))
                .chain((0..3).map(|axis| {
                    (
                        row.closure()[axis],
                        orientation.closure()[axis],
                        row.closure_error() + orientation.closure_error(),
                    )
                }));
                for (measured, closed, error) in pairs {
                    let floor = TEST_ABSOLUTE_FLOOR * measured.abs().max(1.0);
                    assert!(
                        (measured - closed).abs() <= error + floor,
                        "{measured} against {closed} within {error}"
                    );
                }
            }
        }
    }

    /// A 100 000 × 1 × 1 bar turned about (1, 2, 3). Each end face lies far
    /// from the body's flux origin relative to its own size; on a plane the
    /// volume and closure fluxes follow from the area, so the whole bar is
    /// checked within the findings allowance and has no finding.
    #[test]
    fn a_needle_bar_is_checked_within_the_findings_allowance() {
        let length = 100_000.0;
        let half = [length / 2.0, 0.5, 0.5];
        for angle in [0.7, 1.3, 2.9] {
            let ir = rotated_cuboid(
                &rotation([1.0, 2.0, 3.0], angle),
                [5.0, 7.0, 9.0],
                half.map(|h| -h),
                half,
            );
            let ctx = service_context();
            let found =
                crate::session::findings(&ctx, &ir, MeasureOptions::default()).expect("findings");
            ctx.finish_session().expect("session stays unrefused");
            assert_eq!(checks(&found), [], "{angle}: {found:#?}");
        }
    }
}
