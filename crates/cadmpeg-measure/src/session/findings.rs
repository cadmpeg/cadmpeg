// SPDX-License-Identifier: Apache-2.0
//! Check findings from one orientation pass.
//!
//! The pass reads the defect classes that measurement exposes and reports
//! each as a warning:
//!
//! - [`Check::RingOrder`] on a loop: the ring's coedges form one closed
//!   traversal by vertex identity in another order, several closed
//!   traversals, or none.
//! - [`Check::Orientation`] on a face: its sense disagrees with the winding
//!   of its loops, or its loops disagree with each other.
//! - [`Check::Orientation`] on a body whose shells are closed: its volume
//!   readings conflict (`volume.orientation-conflict`), or the flux of the
//!   unit field exceeds its tolerance (`volume.closure-flux`).
//!
//! The pass resolves boundaries, regions, orientation readings and
//! witnesses for every face, and areas and fluxes for the faces of bodies
//! whose shells use every edge twice with opposite senses. It computes no
//! carrier parameter, box or extent. Every item draws a slice of one pass
//! allowance; an item whose slice runs out gives no finding.

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
        let Some(allowance) = self.options.work.pass_allowance(items) else {
            return Err(ResourceLimit {
                dimension: ResourceDimension::Entities,
                reason: ResourceFailure::BudgetExceeded,
                limit: 0,
                used: cadmpeg_core::decode::u64_from_index(items),
                additional: 0,
                operation: "measurement pass allowance",
            });
        };
        self.findings_under(ctx, &PassAllowance::new(allowance))
    }

    /// The findings under `pass`. A face or edge whose slice refused a
    /// charge is starved: no finding names it or the body that owns it.
    fn findings_under(
        &self,
        ctx: &DecodeContext<'_>,
        pass: &PassAllowance,
    ) -> Result<Vec<Finding>, ResourceLimit> {
        let model = &self.input.ir.model;
        let mut findings = self.ring_findings(pass);
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
        }
        .run(&mut faces)?;
        if let Some(limit) = ctx.resource_refusal() {
            return Err(limit);
        }

        for face in &model.faces {
            if starved.contains(face.id.as_str()) {
                continue;
            }
            let Some(result) = faces.get(face.id.as_str()) else {
                continue;
            };
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
            if !flux_bodies.contains(id) || starved_member {
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
        Ok(findings)
    }

    /// One finding per face ring whose listed coedges are not a closed
    /// traversal by vertex identity.
    fn ring_findings(&self, pass: &PassAllowance) -> Vec<Finding> {
        let mut findings = Vec::new();
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
                    break;
                };
                let (result, work) = traverse_counted(&ends, steps);
                if slice.charge(work).is_err() {
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
                    Err(TraversalError::Exhausted) => continue,
                };
                findings.push(warning(
                    Check::RingOrder,
                    face_loop.id.as_str(),
                    vec![face.id.as_str().to_owned()],
                    message.to_owned(),
                ));
            }
        }
        findings
    }
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
    use cadmpeg_ir::topology::Sense;

    use crate::budget::PassAllowance;
    use crate::session::{MeasureInput, MeasureOptions, Measurer};
    use crate::test_support::service_context;

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

    fn findings_under(allowance: usize) -> usize {
        let cube = defective_cube();
        let ctx = service_context();
        let input = MeasureInput {
            ir: &cube,
            fidelity: None,
            geometry_transferred: true,
        };
        let found = Measurer::new(input, MeasureOptions::default())
            .findings_under(&ctx, &PassAllowance::new(allowance))
            .expect("findings");
        found.len()
    }

    #[test]
    fn an_exhausted_pass_gives_no_finding() {
        assert_eq!(findings_under(1 << 26), 3);
        assert_eq!(findings_under(0), 0);
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
        assert_eq!(findings_under(rings), 1);
    }
}
