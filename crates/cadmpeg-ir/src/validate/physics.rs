// SPDX-License-Identifier: Apache-2.0
//! References of the physical records.

use crate::document::CadIr;
use crate::index::identities::BorrowedIdentities;
use crate::physics::solver::SolverTarget;
use crate::physics::PhysicsTarget;
use crate::report::check::{Check, Finding};
use crate::report::Severity;

use super::record_finding;

/// The identities of the arenas that physical records name.
struct Arenas<'ir> {
    bodies: BorrowedIdentities<'ir, ()>,
    occurrences: BorrowedIdentities<'ir, ()>,
    definitions: BorrowedIdentities<'ir, ()>,
    materials: BorrowedIdentities<'ir, ()>,
    colliders: BorrowedIdentities<'ir, ()>,
    articulations: BorrowedIdentities<'ir, ()>,
    scenes: BorrowedIdentities<'ir, ()>,
    joints: BorrowedIdentities<'ir, ()>,
}

impl<'ir> Arenas<'ir> {
    fn new(ir: &'ir CadIr) -> Self {
        let model = &ir.model;
        Self {
            bodies: BorrowedIdentities::build(|add| {
                for body in &model.bodies {
                    add(body.id.as_str(), ());
                }
            }),
            occurrences: BorrowedIdentities::build(|add| {
                for occurrence in &model.occurrences {
                    add(occurrence.id.as_str(), ());
                }
            }),
            definitions: BorrowedIdentities::build(|add| {
                for definition in &model.product_definitions {
                    add(definition.id.as_str(), ());
                }
            }),
            materials: BorrowedIdentities::build(|add| {
                for material in &model.physical_materials {
                    add(material.id.as_str(), ());
                }
            }),
            colliders: BorrowedIdentities::build(|add| {
                for collider in &model.colliders {
                    add(collider.id.as_str(), ());
                }
            }),
            articulations: BorrowedIdentities::build(|add| {
                for articulation in &model.articulations {
                    add(articulation.id.as_str(), ());
                }
            }),
            scenes: BorrowedIdentities::build(|add| {
                for scene in &model.physics_scenes {
                    add(scene.id.as_str(), ());
                }
            }),
            joints: BorrowedIdentities::build(|add| {
                for joint in &model.assembly_joints {
                    add(joint.id.as_str(), ());
                }
            }),
        }
    }

    /// The kind and identity of `target` when it names no record.
    fn missing(&self, target: &PhysicsTarget) -> Option<(&'static str, String)> {
        let (kind, id, present) = match target {
            PhysicsTarget::Body { body } => ("body", body.as_str(), &self.bodies),
            PhysicsTarget::Occurrence { occurrence } => {
                ("occurrence", occurrence.as_str(), &self.occurrences)
            }
            PhysicsTarget::Definition { definition } => {
                ("product definition", definition.as_str(), &self.definitions)
            }
        };
        (!present.contains(id)).then(|| (kind, id.to_owned()))
    }

    fn missing_solver_target(&self, target: &SolverTarget) -> Option<(&'static str, String)> {
        let (kind, id, present) = match target {
            SolverTarget::Scene { scene } => ("physics scene", scene.as_str(), &self.scenes),
            SolverTarget::Occurrence { occurrence } => {
                ("occurrence", occurrence.as_str(), &self.occurrences)
            }
            SolverTarget::Definition { definition } => {
                ("product definition", definition.as_str(), &self.definitions)
            }
            SolverTarget::Body { body } => ("body", body.as_str(), &self.bodies),
            SolverTarget::Collider { collider } => ("collider", collider.as_str(), &self.colliders),
            SolverTarget::Joint { joint } => ("assembly joint", joint.as_str(), &self.joints),
            SolverTarget::PhysicalMaterial { material } => {
                ("physical material", material.as_str(), &self.materials)
            }
            SolverTarget::Articulation { articulation } => {
                ("articulation", articulation.as_str(), &self.articulations)
            }
        };
        (!present.contains(id)).then(|| (kind, id.to_owned()))
    }
}

fn missing(findings: &mut Vec<Finding>, owner: &str, (kind, target): (&'static str, String)) {
    record_finding(
        findings,
        Check::ReferentialIntegrity,
        Severity::Error,
        Some(owner),
        format_args!("references missing {kind} `{target}`"),
    );
}

/// Each physical record names records of the model.
pub(super) fn check_physics(ir: &CadIr, findings: &mut Vec<Finding>) {
    let model = &ir.model;
    let arenas = Arenas::new(ir);
    let material = |id: Option<&crate::ids::PhysicalMaterialId>| {
        id.filter(|id| !arenas.materials.contains(id.as_str()))
            .map(|id| ("physical material", id.as_str().to_owned()))
    };
    for record in &model.mass_properties {
        let owner = record.id.as_str();
        for gap in [
            arenas.missing(&record.target),
            material(record.material.as_ref()),
        ]
        .into_iter()
        .flatten()
        {
            missing(findings, owner, gap);
        }
    }
    for record in &model.rigid_bodies {
        if let Some(gap) = arenas.missing(&record.target) {
            missing(findings, record.id.as_str(), gap);
        }
    }
    for record in &model.colliders {
        let owner = record.id.as_str();
        for gap in [
            arenas.missing(&record.target),
            material(record.material.as_ref()),
        ]
        .into_iter()
        .flatten()
        {
            missing(findings, owner, gap);
        }
    }
    for record in &model.articulations {
        if let Some(gap) = arenas.missing(&record.root) {
            missing(findings, record.id.as_str(), gap);
        }
    }
    for record in &model.solver_settings {
        let owner = record.id.as_str();
        let mut targets = vec![&record.target];
        for value in record.values.values() {
            if let crate::physics::solver::SolverValue::Targets { targets: named } = value {
                targets.extend(named);
            }
        }
        for target in targets {
            if let Some(gap) = arenas.missing_solver_target(target) {
                missing(findings, owner, gap);
            }
        }
    }
}

#[cfg(test)]
mod tests;
