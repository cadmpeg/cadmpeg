// SPDX-License-Identifier: Apache-2.0
//! Physics scenes and the settings of the carried `PhysX` schemas (spec
//! §16.7, §16.8).
//!
//! Settings are collected during the traversal and resolved after it, when
//! the joints and occurrences that their relationships name exist. Values
//! are scaled to millimetres, kilograms, and radians.

use std::collections::BTreeMap;

use cadmpeg_ir::physics::solver::{SolverSettings, SolverTarget, SolverValue};
use cadmpeg_ir::physics::PhysicsScene;
use cadmpeg_ir::scalar::{FiniteReal, NonNegativeReal};
use cadmpeg_ir::units::UnitVector3;

use crate::schema::physx::{Dim, Freedom, Holder, Read};
use crate::stage::Prim;

/// The solver that the `PhysX` schemas configure.
const SOLVER: &str = "physx";

/// Settings read during the traversal.
#[derive(Debug)]
pub(super) struct PendingSettings {
    path: String,
    schema: String,
    target: SolverTarget,
    values: Vec<(String, Read)>,
}

/// Whether a prim type is the physics scene type.
pub(super) fn is_scene_type(type_name: Option<&str>) -> bool {
    type_name == Some("PhysicsScene")
}

/// The joint freedom of a `UsdPhysics` joint type, for `PhysX` attributes
/// whose unit depends on it.
pub(super) fn joint_freedom(kind: crate::schema::physics::JointType) -> Freedom {
    use crate::schema::physics::JointType;
    match kind {
        JointType::Prismatic | JointType::Distance => Freedom::Linear,
        JointType::Revolute
        | JointType::Spherical
        | JointType::Fixed
        | JointType::Generic
        | JointType::Gear
        | JointType::RackPinion => Freedom::Angular,
    }
}

impl super::Builder<'_, '_, '_, '_> {
    /// Reads the carried `PhysX` schemas of `prim` for holders `holders` as
    /// settings of `target`.
    pub(super) fn physx(
        &mut self,
        prim: &Prim<'_>,
        holders: &[Holder],
        target: &SolverTarget,
        freedom: Option<Freedom>,
    ) {
        for schema in prim.api_schemas() {
            let (base, _) = crate::schema::physx::split(schema);
            if !crate::schema::physx::holder(base).is_some_and(|holder| holders.contains(&holder)) {
                continue;
            }
            let values = crate::schema::physx::read(prim, schema, freedom, &mut self.issues);
            for name in crate::schema::physx::uncarried(prim, schema) {
                self.tallies
                    .solver_properties
                    .push(format!("{}.{name}", prim.path));
            }
            self.pending_physx.push(PendingSettings {
                path: prim.path.clone(),
                schema: schema.to_owned(),
                target: target.clone(),
                values,
            });
        }
    }

    /// Adds the physics scene of `prim` and its `PhysX` settings.
    pub(super) fn scene(&mut self, prim: &Prim<'_>) -> Result<(), cadmpeg_core::CodecError> {
        let data = crate::schema::physics::scene(prim, &mut self.issues);
        let id = crate::ids::physics_scene(&prim.path);
        let gravity_direction = data
            .gravity_direction
            .and_then(|[x, y, z]| UnitVector3::normalized(cadmpeg_ir::math::Vector3::new(x, y, z)));
        let scale = self.scale;
        let gravity_magnitude = data
            .gravity_magnitude
            .filter(|value| value.is_finite() && *value >= 0.0)
            .and_then(|value| NonNegativeReal::new(value * scale));
        self.ir.model.physics_scenes.push(PhysicsScene {
            id: id.clone(),
            name: Some(prim.name.clone()),
            gravity_direction,
            gravity_magnitude,
            native_ref: Some(prim.path.clone()),
        });
        self.admit("usd_physics_scene")?;
        self.physx(
            prim,
            &[Holder::Scene],
            &SolverTarget::Scene { scene: id },
            None,
        );
        Ok(())
    }

    /// The factor from scene units to IR units of a unit.
    fn physx_factor(&self, dim: Dim) -> f64 {
        let radians = if dim.degrees {
            std::f64::consts::PI / 180.0
        } else {
            1.0
        };
        dim.dimension()
            .factor(self.scale, self.kilograms_per_unit, radians)
    }

    /// The record that a relationship target names.
    fn physx_target(&self, path: &str) -> Option<SolverTarget> {
        let prim = self.stage.find(path)?;
        let model = &self.ir.model;
        if prim
            .type_name()
            .and_then(crate::schema::physics::JointType::of)
            .is_some()
        {
            let joint = self.joint_id(path);
            return model
                .assembly_joints
                .iter()
                .any(|record| record.id == joint)
                .then_some(SolverTarget::Joint { joint });
        }
        let occurrence = self.occurrence_id(path);
        model
            .occurrences
            .iter()
            .any(|record| record.id == occurrence)
            .then_some(SolverTarget::Occurrence { occurrence })
    }

    /// Adds the settings read during the traversal.
    pub(super) fn finish_physx(&mut self) -> Result<(), cadmpeg_core::CodecError> {
        for pending in std::mem::take(&mut self.pending_physx) {
            let mut values = BTreeMap::new();
            for (name, read) in pending.values {
                let value = match read {
                    Read::Bool(value) => Some(SolverValue::Bool { value }),
                    Read::Integer(value) => Some(SolverValue::Integer { value }),
                    Read::Token(value) => Some(SolverValue::Token { value }),
                    Read::Tokens(values) => Some(SolverValue::Tokens { values }),
                    Read::Real(value, _) if value.is_infinite() => Some(SolverValue::Unbounded {
                        negative: value < 0.0,
                    }),
                    Read::Real(value, dim) => {
                        FiniteReal::new(value * self.physx_factor(dim)).map(|value| {
                            SolverValue::Real {
                                value,
                                dimension: dim.dimension(),
                            }
                        })
                    }
                    Read::Reals(values, dim) => {
                        let factor = self.physx_factor(dim);
                        values
                            .iter()
                            .map(|value| FiniteReal::new(value * factor))
                            .collect::<Option<Vec<_>>>()
                            .map(|values| SolverValue::Reals {
                                values,
                                dimension: dim.dimension(),
                            })
                    }
                    Read::Targets(paths) => paths
                        .iter()
                        .map(|path| self.physx_target(path))
                        .collect::<Option<Vec<_>>>()
                        .map(|targets| SolverValue::Targets { targets }),
                };
                match value {
                    Some(value) => {
                        values.insert(name, value);
                    }
                    None => self
                        .tallies
                        .solver_properties
                        .push(format!("{}.{name}", pending.path)),
                }
            }
            self.ir.model.solver_settings.push(SolverSettings {
                id: crate::ids::solver_settings(&pending.path, &pending.schema),
                solver: SOLVER.to_owned(),
                schema: pending.schema,
                target: pending.target,
                values,
                native_ref: Some(pending.path),
            });
            self.admit("usd_solver_settings")?;
        }
        Ok(())
    }
}
