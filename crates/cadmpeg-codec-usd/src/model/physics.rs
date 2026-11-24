// SPDX-License-Identifier: Apache-2.0
//! The physical records of a composed layer (spec §16).
//!
//! Masses are scaled by `kilogramsPerUnit`, lengths to millimetres, and
//! angles from degrees to radians. A joint frame is turned so that its z
//! axis is the joint axis.

use std::collections::BTreeSet;

use cadmpeg_ir::features::geometry::{FinitePoint3, FiniteVector3};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::physics::{
    Articulation, Collider, ColliderShape, MassProperties, PhysicalMaterial, PhysicsTarget,
    PrincipalInertia, RigidBody,
};
use cadmpeg_ir::products::{
    AssemblyJoint, DriveAxis, DriveMode, FrameAxis, JointConnector, JointDrive, JointId,
    JointLimits, JointOperand, PairedJointKind,
};
use cadmpeg_ir::scalar::{FiniteReal, NonNegativeReal, NonZeroReal, PositiveReal};
use cadmpeg_ir::transform::Transform;

use cadmpeg_ir::physics::solver::SolverTarget;

use crate::schema::physics::{Drive, JointData, JointType, Limit};
use crate::schema::physx::Holder;
use crate::schema::xform::{compose, quaternion, Affine};
use crate::stage::Prim;

/// The record whose physics a prim's physics schemas describe: the body of
/// its definition, the mesh that is its definition's geometry, or the
/// occurrence of the prim.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PhysicsOwner {
    Body,
    Mesh,
    Occurrence,
}

/// A joint prim collected for the pass after the traversal.
pub(super) struct PendingJoint {
    pub(super) path: String,
    pub(super) data: JointData,
}

/// The rotation that turns a joint frame so that its z axis is `axis`, by
/// a cyclic permutation of the axes.
fn axis_alignment(axis: char) -> Affine {
    match axis {
        'X' => [
            [0.0, 0.0, 1.0, 0.0],
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
        ],
        'Y' => [
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [1.0, 0.0, 0.0, 0.0],
        ],
        _ => crate::schema::xform::IDENTITY,
    }
}

fn frame_axis(letter: char) -> FrameAxis {
    match letter {
        'X' => FrameAxis::X,
        'Y' => FrameAxis::Y,
        _ => FrameAxis::Z,
    }
}

impl super::Builder<'_, '_, '_, '_> {
    /// The joint frame of one side, in millimetres, turned to `alignment`.
    fn joint_frame(&self, data: &JointData, side: usize, alignment: &Affine) -> Option<Transform> {
        let mut rotation = quaternion(data.rotations[side])?;
        rotation = compose(&rotation, alignment);
        for (row, value) in rotation.iter_mut().zip(data.positions[side]) {
            row[3] = value;
        }
        self.transform(&rotation)
    }

    /// Collects the joints and physics scenes of the subtree at `index`,
    /// which has no occurrences; `world` is the composed transform of its
    /// parent.
    pub(super) fn collect_unplaced(
        &mut self,
        index: usize,
        world: &Affine,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let stage = self.stage;
        let Some(prim) = stage.prims.get(index) else {
            return Ok(());
        };
        if super::physx::is_scene_type(prim.type_name()) {
            return self.scene(prim);
        }
        if let Some(kind) = prim.type_name().and_then(JointType::of) {
            let data = crate::schema::physics::joint(prim, kind, &mut self.issues);
            self.pending_joints.push(PendingJoint {
                path: prim.path.clone(),
                data,
            });
            return Ok(());
        }
        let local = crate::schema::xform::local_transform(prim, &mut self.issues)
            .map_or(crate::schema::xform::IDENTITY, |local| local.matrix);
        let world = compose(world, &local);
        for child in prim.children.clone() {
            self.collect_unplaced(child, &world)?;
        }
        Ok(())
    }

    /// Reads the physics API schemas of the prim of occurrence `path`, which
    /// defines `key` whose geometry is `owner`.
    pub(super) fn physics_apis(
        &mut self,
        prim: &Prim<'_>,
        key: &str,
        owner: PhysicsOwner,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let schemas = prim.api_schemas();
        let occurrence = PhysicsTarget::Occurrence {
            occurrence: crate::ids::occurrence(&prim.path),
        };
        for name in crate::schema::physics::unread_body_properties(prim) {
            self.tallies
                .physics_properties
                .push(format!("{}.{name}", prim.path));
        }
        if schemas.contains("PhysicsRigidBodyAPI") {
            let data = crate::schema::physics::rigid_body(prim, &mut self.issues);
            self.ir.model.rigid_bodies.push(RigidBody {
                id: crate::ids::rigid_body(&prim.path),
                target: occurrence.clone().into(),
                kinematic: data.kinematic,
                enabled: data.enabled,
                linear_velocity: data.velocity.and_then(|[x, y, z]| {
                    FiniteVector3::new(Vector3::new(x * self.scale, y * self.scale, z * self.scale))
                }),
                angular_velocity: data.angular_velocity.and_then(|[x, y, z]| {
                    FiniteVector3::new(Vector3::new(x.to_radians(), y.to_radians(), z.to_radians()))
                }),
                starts_asleep: data.starts_asleep,
                native_ref: Some(prim.path.clone()),
            });
            self.admit("usd_rigid_body")?;
        }
        if schemas.contains("PhysicsArticulationRootAPI") {
            self.ir.model.articulations.push(Articulation {
                id: crate::ids::articulation(&prim.path),
                root: occurrence.clone().into(),
                native_ref: Some(prim.path.clone()),
            });
            self.admit("usd_articulation")?;
        }
        if schemas.contains("PhysicsMassAPI") {
            let data = crate::schema::physics::mass(prim, &mut self.issues);
            let kilograms = self.kilograms_per_unit;
            let density = |value: f64| value * kilograms / self.scale.powi(3);
            let inertia = data.diagonal_inertia.and_then(|moments| {
                let factor = kilograms * self.scale * self.scale;
                let moments = [0, 1, 2]
                    .map(|axis| NonNegativeReal::new(moments[axis] * factor))
                    .into_iter()
                    .collect::<Option<Vec<_>>>()?;
                let axes = data
                    .principal_axes
                    .filter(|axes| *axes != [1.0, 0.0, 0.0, 0.0])
                    .and_then(quaternion)
                    .and_then(Transform::affine);
                Some(PrincipalInertia {
                    moments: [moments[0], moments[1], moments[2]],
                    axes,
                })
            });
            self.ir.model.mass_properties.push(MassProperties {
                id: crate::ids::mass_properties(&prim.path),
                target: occurrence.clone().into(),
                mass: data
                    .mass
                    .and_then(|value| PositiveReal::new(value * kilograms)),
                density: data
                    .density
                    .and_then(|value| PositiveReal::new(density(value))),
                center_of_mass: data.center_of_mass.and_then(|[x, y, z]| {
                    FinitePoint3::new(Point3::new(x * self.scale, y * self.scale, z * self.scale))
                }),
                inertia,
                material: None,
                native_ref: Some(prim.path.clone()),
            });
            self.admit("usd_mass")?;
        }
        if schemas.contains("PhysicsCollisionAPI") {
            self.collider(prim, key, owner)?;
        } else {
            let target = match owner {
                PhysicsOwner::Body => SolverTarget::Body {
                    body: crate::ids::body(key),
                },
                PhysicsOwner::Mesh => SolverTarget::Definition {
                    definition: crate::ids::definition(key),
                },
                PhysicsOwner::Occurrence => SolverTarget::Occurrence {
                    occurrence: crate::ids::occurrence(&prim.path),
                },
            };
            self.physx(prim, &[Holder::Collision], &target, None);
        }
        let articulation = if schemas.contains("PhysicsArticulationRootAPI") {
            SolverTarget::Articulation {
                articulation: crate::ids::articulation(&prim.path),
            }
        } else {
            SolverTarget::Occurrence {
                occurrence: crate::ids::occurrence(&prim.path),
            }
        };
        self.physx(prim, &[Holder::Articulation], &articulation, None);
        self.physx(
            prim,
            &[Holder::Body],
            &SolverTarget::Occurrence {
                occurrence: crate::ids::occurrence(&prim.path),
            },
            None,
        );
        Ok(())
    }

    /// Reads the collision schemas of `prim`: a collider of the body or the
    /// mesh of definition `key` where `owner` names one, and of the occurrence
    /// of `prim` otherwise.
    pub(super) fn collider(
        &mut self,
        prim: &Prim<'_>,
        key: &str,
        owner: PhysicsOwner,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let occurrence = PhysicsTarget::Occurrence {
            occurrence: crate::ids::occurrence(&prim.path),
        };
        let data = crate::schema::physics::collision(prim, &mut self.issues);
        let shape = match data.approximation.as_deref() {
            None | Some("none") => ColliderShape::Exact,
            Some("convexHull") => ColliderShape::ConvexHull,
            Some("convexDecomposition") => ColliderShape::ConvexDecomposition,
            Some("boundingCube") => ColliderShape::BoundingBox,
            Some("boundingSphere") => ColliderShape::BoundingSphere,
            Some("meshSimplification") => ColliderShape::Simplified,
            Some(other) => ColliderShape::Native {
                name: other.to_owned(),
            },
        };
        let material = self.physics_material(prim)?;
        let (id, target) = match owner {
            PhysicsOwner::Body => (
                crate::ids::collider(key),
                PhysicsTarget::Body {
                    body: crate::ids::body(key),
                },
            ),
            PhysicsOwner::Mesh => (
                crate::ids::collider(key),
                PhysicsTarget::Definition {
                    definition: crate::ids::definition(key),
                },
            ),
            PhysicsOwner::Occurrence => (crate::ids::collider(&prim.path), occurrence),
        };
        if !self
            .ir
            .model
            .colliders
            .iter()
            .any(|collider| collider.id == id)
        {
            self.ir.model.colliders.push(Collider {
                id: id.clone(),
                target: target.into(),
                shape,
                enabled: data.enabled,
                material,
                native_ref: Some(prim.path.clone()),
            });
            self.admit("usd_collider")?;
            self.physx(
                prim,
                &[Holder::Collision],
                &SolverTarget::Collider { collider: id },
                None,
            );
        }
        Ok(())
    }

    /// The joints that the coupling joint at `path` names, when each names
    /// one joint that was added; otherwise none, reported.
    fn coupled_joints(&mut self, path: &str, coupled: &[Option<String>]) -> Vec<JointId> {
        let ids: Option<Vec<JointId>> = coupled
            .iter()
            .map(|target| {
                let id = crate::ids::joint(target.as_deref()?);
                self.ir
                    .model
                    .assembly_joints
                    .iter()
                    .any(|joint| joint.id == id)
                    .then_some(id)
            })
            .collect();
        ids.unwrap_or_else(|| {
            self.tallies
                .joint_properties
                .push(format!("{path}: the coupled joints do not resolve"));
            Vec::new()
        })
    }

    /// The physics material that `prim` binds: its physics-purpose binding,
    /// or its all-purpose binding, when that material applies
    /// `PhysicsMaterialAPI`.
    fn physics_material(
        &mut self,
        prim: &Prim<'_>,
    ) -> Result<Option<cadmpeg_ir::ids::PhysicalMaterialId>, cadmpeg_core::CodecError> {
        let targets = {
            let physics = prim.relationship_targets("material:binding:physics");
            if physics.is_empty() {
                prim.relationship_targets("material:binding")
            } else {
                physics
            }
        };
        let stage = self.stage;
        let Some(material) = targets
            .first()
            .and_then(|path| stage.find(path))
            .filter(|material| material.api_schemas().contains("PhysicsMaterialAPI"))
        else {
            return Ok(None);
        };
        let id = crate::ids::physical_material(&material.path);
        if self.physical_materials.insert(material.path.clone()) {
            let data = crate::schema::physics::material(material, &mut self.issues);
            let density = data
                .density
                .map(|value| value * self.kilograms_per_unit / self.scale.powi(3));
            self.ir.model.physical_materials.push(PhysicalMaterial {
                id: id.clone(),
                name: Some(material.name.clone()),
                density: density.and_then(PositiveReal::new),
                static_friction: data.static_friction.and_then(NonNegativeReal::new),
                dynamic_friction: data.dynamic_friction.and_then(NonNegativeReal::new),
                restitution: data.restitution.and_then(NonNegativeReal::new),
                native_ref: Some(material.path.clone()),
            });
            self.admit("usd_physical_material")?;
            self.physx(
                material,
                &[Holder::Material],
                &SolverTarget::PhysicalMaterial {
                    material: id.clone(),
                },
                None,
            );
        }
        Ok(Some(id))
    }

    /// One drive in IR units, or `None` when the joint type has no degree of
    /// freedom for its instance.
    fn drive(&self, kind: JointType, drive: &Drive) -> Option<JointDrive> {
        let axis = match (kind, drive.instance.as_str()) {
            (JointType::Revolute, "angular") => DriveAxis::Rotation { axis: FrameAxis::Z },
            (JointType::Prismatic, "linear") => DriveAxis::Translation { axis: FrameAxis::Z },
            (JointType::Generic, instance) => {
                let letter = instance.chars().last()?;
                match instance.get(..instance.len() - 1)? {
                    "trans" => DriveAxis::Translation {
                        axis: frame_axis(letter),
                    },
                    "rot" => DriveAxis::Rotation {
                        axis: frame_axis(letter),
                    },
                    _ => return None,
                }
            }
            _ => return None,
        };
        let millimetres = self.scale;
        let kilograms = self.kilograms_per_unit;
        let degrees = 180.0 / std::f64::consts::PI;
        // Angular drives state torques per degree; linear drives forces per
        // scene length. Coefficients are in kilograms, millimetres, seconds,
        // and radians.
        let (position, response, limit) = match axis {
            DriveAxis::Rotation { .. } => (
                1.0 / degrees,
                kilograms * millimetres * millimetres * degrees,
                kilograms * millimetres * millimetres,
            ),
            DriveAxis::Translation { .. } => (self.scale, kilograms, kilograms * millimetres),
        };
        Some(JointDrive {
            axis,
            mode: if drive.acceleration {
                DriveMode::Acceleration
            } else {
                DriveMode::Force
            },
            target_position: drive
                .target_position
                .and_then(|value| FiniteReal::new(value * position)),
            target_velocity: drive
                .target_velocity
                .and_then(|value| FiniteReal::new(value * position)),
            stiffness: drive
                .stiffness
                .and_then(|value| NonNegativeReal::new(value * response)),
            damping: drive
                .damping
                .and_then(|value| NonNegativeReal::new(value * response)),
            max_force: drive
                .max_force
                .and_then(|value| PositiveReal::new(value * limit)),
        })
    }

    /// Adds the joints collected during the traversal.
    pub(super) fn joints(
        &mut self,
        pending: Vec<PendingJoint>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let occurrences: BTreeSet<String> = self
            .ir
            .model
            .occurrences
            .iter()
            .map(|occurrence| occurrence.id.to_string())
            .collect();
        // A coupling joint names the joints it couples, so it is added after
        // them.
        let mut pending = pending;
        pending.sort_by_key(|joint| {
            matches!(joint.data.kind, JointType::Gear | JointType::RackPinion)
        });
        for PendingJoint { path, data } in pending {
            let alignment = match data.kind {
                JointType::Revolute | JointType::Prismatic => axis_alignment(data.axis),
                _ => crate::schema::xform::IDENTITY,
            };
            let mut connectors = Vec::new();
            let mut unresolved = false;
            for side in 0..2 {
                let Some(frame) = self.joint_frame(&data, side, &alignment) else {
                    unresolved = true;
                    break;
                };
                let operand = match &data.bodies[side] {
                    Some(body) => {
                        let occurrence = crate::ids::occurrence(body);
                        if !occurrences.contains(occurrence.as_str()) {
                            unresolved = true;
                            break;
                        }
                        Some(JointOperand::occurrence(
                            occurrence,
                            body.clone(),
                            Vec::new(),
                        ))
                    }
                    None => None,
                };
                connectors.push((operand, frame));
            }
            if unresolved || connectors.iter().all(|(operand, _)| operand.is_none()) {
                self.tallies.rejected_joints.push(path);
                continue;
            }
            let connector = |(operand, frame): (Option<JointOperand>, Transform)| JointConnector {
                operand: operand.unwrap_or_else(|| JointOperand::root("/", Vec::new())),
                frame,
                detached: false,
            };
            let degrees = |value: f64| value.to_radians();
            let limits = |lower: Option<f64>, upper: Option<f64>, factor: &dyn Fn(f64) -> f64| {
                JointLimits::new(lower.map(factor), upper.map(factor))
            };
            let scale = self.scale;
            let generic = (data.kind == JointType::Generic)
                .then(|| generic_kind(&data.limits, scale))
                .flatten();
            let id = crate::ids::joint(&path);
            let mut joint = match data.kind {
                JointType::Fixed if connectors.iter().any(|(operand, _)| operand.is_none()) => {
                    let (grounded, world): (Vec<_>, Vec<_>) = connectors
                        .into_iter()
                        .partition(|(operand, _)| operand.is_some());
                    let world = world.into_iter().next().map(|(_, frame)| frame);
                    let Some(grounded) = grounded.into_iter().next() else {
                        continue;
                    };
                    AssemblyJoint::grounded(id, connector(grounded), world)
                }
                kind => {
                    let paired = match kind {
                        JointType::Fixed => PairedJointKind::Fixed {
                            angle: None,
                            translation_offset: None,
                            angular_limits: None,
                            linear_limits: None,
                        },
                        JointType::Revolute => PairedJointKind::Revolute {
                            angle: None,
                            angular_limits: limits(data.lower, data.upper, &degrees),
                        },
                        JointType::Prismatic => PairedJointKind::Slider {
                            distance: None,
                            translation_offset: None,
                            linear_limits: limits(data.lower, data.upper, &|value| value * scale),
                        },
                        JointType::Spherical => PairedJointKind::Ball {},
                        JointType::Distance => PairedJointKind::Distance {
                            distance: data
                                .max_distance
                                .and_then(|value| FiniteReal::new(value * scale)),
                        },
                        JointType::Gear | JointType::RackPinion => {
                            let coupled_joints = self.coupled_joints(&path, &data.coupled);
                            if data.kind == JointType::Gear {
                                PairedJointKind::Gears {
                                    distance: None,
                                    distance2: None,
                                    ratio: data.ratio.and_then(|ratio| NonZeroReal::new(-ratio)),
                                    coupled_joints,
                                }
                            } else {
                                PairedJointKind::RackPinion {
                                    distance: None,
                                    distance2: None,
                                    ratio: data.ratio.and_then(|ratio| {
                                        NonZeroReal::new(ratio.to_radians() / scale)
                                    }),
                                    coupled_joints,
                                }
                            }
                        }
                        JointType::Generic => {
                            generic.clone().unwrap_or_else(|| PairedJointKind::Native {
                                name: "PhysicsJoint".to_owned(),
                                angle: None,
                                translation_offset: None,
                                distance: None,
                                distance2: None,
                                angular_limits: None,
                                linear_limits: None,
                            })
                        }
                    };
                    let mut iter = connectors.into_iter();
                    let (Some(first), Some(second)) = (iter.next(), iter.next()) else {
                        continue;
                    };
                    AssemblyJoint::paired(id, paired, [connector(first), connector(second)], None)
                }
            };
            joint.suppressed = !data.enabled;
            joint.native_ref = Some(path.clone());
            for drive in &data.drives {
                match self.drive(data.kind, drive) {
                    Some(drive) => joint.drives.push(drive),
                    None => self
                        .tallies
                        .joint_properties
                        .push(format!("{path}: drive {}", drive.instance)),
                }
            }
            let mut dropped = data.dropped.clone();
            if data.kind == JointType::Distance && data.min_distance.is_some() {
                dropped.push("physics:minDistance".to_owned());
            }
            if generic.is_some() {
                // The kind states the locked axes, and its limits the free ones.
                dropped.retain(|name| {
                    !["transX", "transY", "transZ", "rotX", "rotY", "rotZ"]
                        .into_iter()
                        .any(|dof| {
                            name == &format!("limit:{dof}:physics:low")
                                || name == &format!("limit:{dof}:physics:high")
                        })
                });
            } else if data.kind == JointType::Generic {
                dropped.push("the degrees of freedom of a generic joint".to_owned());
            }
            for property in dropped {
                self.tallies
                    .joint_properties
                    .push(format!("{path}: {property}"));
            }
            let id = joint.id.clone();
            self.ir.model.assembly_joints.push(joint);
            self.admit("usd_joint")?;
            if let Some(prim) = self.stage.find(&path) {
                self.physx(
                    prim,
                    &[Holder::Joint],
                    &SolverTarget::Joint { joint: id },
                    Some(super::physx::joint_freedom(data.kind)),
                );
            }
        }
        Ok(())
    }
}

/// The neutral kind of a generic joint whose `PhysicsLimitAPI` instances lock
/// `transX`, `transY`, `rotX` and `rotY`: a cylindrical joint along its Z
/// axis, or a revolute or slider joint when `transZ` or `rotZ` is locked too,
/// with the limits of the free degrees of freedom. `None` for another
/// generic joint.
fn generic_kind(limits: &[Limit], scale: f64) -> Option<PairedJointKind> {
    let limit = |instance: &str| limits.iter().find(|limit| limit.instance == instance);
    let locked = |instance: &str| limit(instance).is_some_and(Limit::locks);
    if !["transX", "transY", "rotX", "rotY"].into_iter().all(locked) || limit("distance").is_some()
    {
        return None;
    }
    let range = |instance: &str, factor: f64| {
        limit(instance).and_then(|limit| {
            JointLimits::new(
                limit
                    .low
                    .filter(|value| value.is_finite())
                    .map(|low| low * factor),
                limit
                    .high
                    .filter(|value| value.is_finite())
                    .map(|high| high * factor),
            )
        })
    };
    let angular = range("rotZ", std::f64::consts::PI / 180.0);
    let linear = range("transZ", scale);
    Some(match (locked("rotZ"), locked("transZ")) {
        (false, false) => PairedJointKind::Cylindrical {
            angle: None,
            distance: None,
            angular_limits: angular,
            linear_limits: linear,
        },
        (false, true) => PairedJointKind::Revolute {
            angle: None,
            angular_limits: angular,
        },
        (true, false) => PairedJointKind::Slider {
            distance: None,
            translation_offset: None,
            linear_limits: linear,
        },
        (true, true) => return None,
    })
}
