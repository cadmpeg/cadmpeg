// SPDX-License-Identifier: Apache-2.0
//! Writes the physical records as `UsdPhysics` schemas (spec §16).
//!
//! The layer's mass unit is the kilogram and its length unit the millimetre.
//! Rigid bodies, articulation roots, and mass properties of occurrences go
//! on the occurrences' prims; colliders and mass properties of bodies go on
//! the bodies' geometry prims. Physical materials and joints are prims of a
//! `Physics` scope. A joint's bodies are rigid bodies: an occurrence that a
//! joint connects and that no rigid body names applies `PhysicsRigidBodyAPI`.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::ids::{BodyId, OccurrenceId, PhysicalMaterialId, PhysicsSceneId};
use cadmpeg_ir::physics::solver::SolverTarget;
use cadmpeg_ir::physics::{ColliderShape, MassProperties, PhysicsTarget};
use cadmpeg_ir::products::{
    AssemblyJoint, DriveAxis, DriveMode, FrameAxis, JointConnector, JointId, JointLimits,
    JointOperands, OperandContainer, PairedJointKind,
};
use cadmpeg_ir::scalar::PositiveReal;
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;

use super::text::{quoted, real32, tuple32, Names};
use super::Writer;

/// Name of the scope that holds physical materials and joints.
pub(super) const PHYSICS: &str = "Physics";
/// Scene-unit density per kilogram per cubic millimetre, for `length`
/// millimetres per scene unit.
fn density_scale(length: f64) -> f64 {
    length.powi(3)
}

/// The schemas and properties of one prim.
pub(super) type PrimPhysics = (Vec<String>, Vec<String>);

/// The initial velocities of a rigid body: linear in scene units per
/// second and angular in degrees per second, in the coordinates of the
/// model.
#[derive(Debug, Clone, Copy, Default)]
struct Velocity {
    linear: Option<[f64; 3]>,
    angular: Option<[f64; 3]>,
}

impl Velocity {
    /// The velocity attributes of a prim whose transform turns its parent
    /// space by `turn` from the model's.
    fn lines(self, turn: Option<f64>) -> Vec<String> {
        let mut lines = Vec::new();
        if let Some(linear) = self.linear {
            lines.push(format!(
                "vector3f physics:velocity = {}",
                floats(&super::turned(turn, linear))
            ));
        }
        if let Some(angular) = self.angular {
            lines.push(format!(
                "vector3f physics:angularVelocity = {}",
                floats(&super::turned(turn, angular))
            ));
        }
        lines
    }
}

/// The physics schemas the writer applies to occurrence and body prims.
pub(super) struct Plan {
    occurrences: BTreeMap<OccurrenceId, PrimPhysics>,
    bodies: BTreeMap<BodyId, PrimPhysics>,
    /// The initial velocities of the rigid bodies on occurrence and body
    /// prims.
    occurrence_velocities: BTreeMap<OccurrenceId, Velocity>,
    body_velocities: BTreeMap<BodyId, Velocity>,
    /// The collision schemas of each body.
    colliders: BTreeMap<BodyId, PrimPhysics>,
    /// Material names in the `Physics` scope.
    materials: BTreeMap<PhysicalMaterialId, String>,
    /// The solver settings of each joint, physical material, and scene.
    joint_settings: BTreeMap<JointId, PrimPhysics>,
    material_settings: BTreeMap<PhysicalMaterialId, PrimPhysics>,
    scene_settings: BTreeMap<PhysicsSceneId, PrimPhysics>,
    /// Solver properties with no prim or attribute to write.
    pub(super) unwritten_settings: usize,
    /// The path of the `Physics` scope.
    scope: String,
}

fn schemas(names: &[&str]) -> String {
    format!(
        "prepend apiSchemas = [{}]",
        names
            .iter()
            .map(|name| quoted(name))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// The `(real, i, j, k)` rotation of a transform's linear part.
fn quaternion(transform: &Transform) -> [f64; 4] {
    let [r0, r1, r2] = transform.affine_rows();
    let rotation = [
        [r0[0], r0[1], r0[2]],
        [r1[0], r1[1], r1[2]],
        [r2[0], r2[1], r2[2]],
    ];
    let element = |row: usize, column: usize| rotation[row][column];
    let trace = element(0, 0) + element(1, 1) + element(2, 2);
    // Each branch divides by the largest of the four squared components.
    let components = if trace > 0.0 {
        let root = (trace + 1.0).sqrt() * 2.0;
        [
            0.25 * root,
            (element(2, 1) - element(1, 2)) / root,
            (element(0, 2) - element(2, 0)) / root,
            (element(1, 0) - element(0, 1)) / root,
        ]
    } else if element(0, 0) > element(1, 1) && element(0, 0) > element(2, 2) {
        let root = (1.0 + element(0, 0) - element(1, 1) - element(2, 2)).sqrt() * 2.0;
        [
            (element(2, 1) - element(1, 2)) / root,
            0.25 * root,
            (element(0, 1) + element(1, 0)) / root,
            (element(0, 2) + element(2, 0)) / root,
        ]
    } else if element(1, 1) > element(2, 2) {
        let root = (1.0 + element(1, 1) - element(0, 0) - element(2, 2)).sqrt() * 2.0;
        [
            (element(0, 2) - element(2, 0)) / root,
            (element(0, 1) + element(1, 0)) / root,
            0.25 * root,
            (element(1, 2) + element(2, 1)) / root,
        ]
    } else {
        let root = (1.0 + element(2, 2) - element(0, 0) - element(1, 1)).sqrt() * 2.0;
        [
            (element(1, 0) - element(0, 1)) / root,
            (element(0, 2) + element(2, 0)) / root,
            (element(1, 2) + element(2, 1)) / root,
            0.25 * root,
        ]
    };
    let norm = components
        .iter()
        .map(|value| value * value)
        .sum::<f64>()
        .sqrt();
    components.map(|value| value / norm)
}

/// A `float` attribute value: `value` rounded to binary32.
fn float(value: f64) -> String {
    real32(crate::text::value::round_f32(value))
}

/// A `float` tuple value, each component rounded to binary32.
fn floats(values: &[f64]) -> String {
    tuple32(
        &values
            .iter()
            .map(|value| crate::text::value::round_f32(*value))
            .collect::<Vec<_>>(),
    )
}

fn mass_lines(mass: &MassProperties, length: f64) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(value) = mass.mass {
        lines.push(format!("float physics:mass = {}", float(value.get())));
    }
    if let Some(value) = mass.density {
        lines.push(format!(
            "float physics:density = {}",
            float(value.get() * density_scale(length))
        ));
    }
    if let Some(point) = mass.center_of_mass {
        let point = point.get();
        lines.push(format!(
            "point3f physics:centerOfMass = {}",
            floats(&[point.x, point.y, point.z].map(|value| value / length))
        ));
    }
    if let Some(inertia) = &mass.inertia {
        lines.push(format!(
            "float3 physics:diagonalInertia = {}",
            floats(
                &inertia
                    .moments
                    .map(|moment| moment.get() / (length * length))
            )
        ));
        let axes = inertia
            .axes
            .as_ref()
            .map_or([1.0, 0.0, 0.0, 0.0], quaternion);
        lines.push(format!("quatf physics:principalAxes = {}", floats(&axes)));
    }
    lines
}

impl Plan {
    /// Whether the layer has a `Physics` scope or physics schemas.
    pub(super) fn is_empty(&self) -> bool {
        self.occurrences.is_empty()
            && self.bodies.is_empty()
            && self.colliders.is_empty()
            && self.materials.is_empty()
    }

    /// The plan of `ir`, in scene units of `length` millimetres.
    pub(super) fn new(ir: &CadIr, length: f64) -> Self {
        let model = &ir.model;
        let mut occurrences: BTreeMap<OccurrenceId, PrimPhysics> = BTreeMap::new();
        let mut bodies: BTreeMap<BodyId, PrimPhysics> = BTreeMap::new();
        let mut colliders: BTreeMap<BodyId, PrimPhysics> = BTreeMap::new();
        let mut occurrence_velocities: BTreeMap<OccurrenceId, Velocity> = BTreeMap::new();
        let mut body_velocities: BTreeMap<BodyId, Velocity> = BTreeMap::new();
        let mut names = Names::default();
        let materials: BTreeMap<PhysicalMaterialId, String> = model
            .physical_materials
            .iter()
            .map(|material| {
                let name = names.unique(material.name.as_deref().unwrap_or("Material"));
                (material.id.clone(), name)
            })
            .collect();
        let mut add = |target: &PhysicsTarget, schema: &str, lines: Vec<String>| {
            let collision = matches!(
                schema,
                "PhysicsCollisionAPI" | "PhysicsMeshCollisionAPI" | "MaterialBindingAPI"
            ) || crate::schema::physx::holder(
                crate::schema::physx::split(schema).0,
            ) == Some(crate::schema::physx::Holder::Collision);
            let entry = match target {
                PhysicsTarget::Occurrence { occurrence } => {
                    occurrences.entry(occurrence.clone()).or_default()
                }
                PhysicsTarget::Body { body } if collision => {
                    colliders.entry(body.clone()).or_default()
                }
                PhysicsTarget::Body { body } => bodies.entry(body.clone()).or_default(),
                PhysicsTarget::Definition { definition } => {
                    let placements: Vec<OccurrenceId> = model
                        .occurrences
                        .iter()
                        .filter(|occurrence| &occurrence.prototype == definition)
                        .map(|occurrence| occurrence.id.clone())
                        .collect();
                    for occurrence in placements {
                        let entry = occurrences.entry(occurrence).or_default();
                        entry.0.push(schemas(&[schema]));
                        entry.1.extend(lines.iter().cloned());
                    }
                    return;
                }
            };
            entry.0.push(schemas(&[schema]));
            entry.1.extend(lines);
        };
        let mut rigid: BTreeSet<OccurrenceId> = BTreeSet::new();
        for body in &model.rigid_bodies {
            if let PhysicsTarget::Occurrence { occurrence } = &body.target.entity {
                rigid.insert(occurrence.clone());
            }
            let mut lines = Vec::new();
            if body.kinematic {
                lines.push("bool physics:kinematicEnabled = 1".to_owned());
            }
            if !body.enabled {
                lines.push("bool physics:rigidBodyEnabled = 0".to_owned());
            }
            let velocity = Velocity {
                linear: body.linear_velocity.map(|velocity| {
                    let velocity = velocity.get();
                    [velocity.x, velocity.y, velocity.z].map(|value| value / length)
                }),
                angular: body.angular_velocity.map(|velocity| {
                    let velocity = velocity.get();
                    [velocity.x, velocity.y, velocity.z].map(f64::to_degrees)
                }),
            };
            if velocity.linear.is_some() || velocity.angular.is_some() {
                match &body.target.entity {
                    PhysicsTarget::Occurrence { occurrence } => {
                        occurrence_velocities.insert(occurrence.clone(), velocity);
                    }
                    PhysicsTarget::Body { body } => {
                        body_velocities.insert(body.clone(), velocity);
                    }
                    PhysicsTarget::Definition { definition } => {
                        for occurrence in model
                            .occurrences
                            .iter()
                            .filter(|occurrence| &occurrence.prototype == definition)
                        {
                            occurrence_velocities.insert(occurrence.id.clone(), velocity);
                        }
                    }
                }
            }
            if body.starts_asleep {
                lines.push("bool physics:startsAsleep = 1".to_owned());
            }
            add(&body.target.entity, "PhysicsRigidBodyAPI", lines);
        }
        for joint in &model.assembly_joints {
            for connector in joint.connectors() {
                if let OperandContainer::Occurrence { occurrence, .. } =
                    &connector.operand.container
                {
                    if rigid.insert(occurrence.clone()) {
                        add(
                            &PhysicsTarget::Occurrence {
                                occurrence: occurrence.clone(),
                            },
                            "PhysicsRigidBodyAPI",
                            Vec::new(),
                        );
                    }
                }
            }
        }
        for articulation in &model.articulations {
            add(
                &articulation.root.entity,
                "PhysicsArticulationRootAPI",
                Vec::new(),
            );
        }
        for mass in &model.mass_properties {
            add(
                &mass.target.entity,
                "PhysicsMassAPI",
                mass_lines(mass, length),
            );
        }
        let scope_path = String::new();
        for collider in &model.colliders {
            let approximation = match &collider.shape {
                ColliderShape::Exact => None,
                ColliderShape::ConvexHull => Some("convexHull"),
                ColliderShape::ConvexDecomposition => Some("convexDecomposition"),
                ColliderShape::BoundingBox => Some("boundingCube"),
                ColliderShape::BoundingSphere => Some("boundingSphere"),
                ColliderShape::Simplified => Some("meshSimplification"),
                ColliderShape::Native { name } => Some(name.as_str()),
            };
            let mut lines = Vec::new();
            if !collider.enabled {
                lines.push("bool physics:collisionEnabled = 0".to_owned());
            }
            add(&collider.target.entity, "PhysicsCollisionAPI", lines);
            if let Some(approximation) = approximation {
                add(
                    &collider.target.entity,
                    "PhysicsMeshCollisionAPI",
                    vec![format!(
                        "uniform token physics:approximation = {}",
                        quoted(approximation)
                    )],
                );
            }
            if let Some(material) = collider.material.as_ref().and_then(|id| materials.get(id)) {
                add(
                    &collider.target.entity,
                    "MaterialBindingAPI",
                    vec![format!(
                        "rel material:binding:physics = <{scope_path}/{PHYSICS}/{material}>"
                    )],
                );
            }
        }
        let mut joint_settings: BTreeMap<JointId, PrimPhysics> = BTreeMap::new();
        let mut material_settings: BTreeMap<PhysicalMaterialId, PrimPhysics> = BTreeMap::new();
        let mut scene_settings: BTreeMap<PhysicsSceneId, PrimPhysics> = BTreeMap::new();
        let mut unwritten_settings = 0;
        for settings in &model.solver_settings {
            if super::physx::settings_holder(settings).is_none() {
                unwritten_settings += settings.values.len().max(1);
                continue;
            }
            let (schema, lines, unwritten) = super::physx::lines(settings, length);
            unwritten_settings += unwritten;
            let onto = |entry: &mut PrimPhysics| {
                entry.0.push(schema.clone());
                entry.1.extend(lines.iter().cloned());
            };
            let physics_target = match &settings.target.entity {
                SolverTarget::Occurrence { occurrence } => Some(PhysicsTarget::Occurrence {
                    occurrence: occurrence.clone(),
                }),
                SolverTarget::Definition { definition } => Some(PhysicsTarget::Definition {
                    definition: definition.clone(),
                }),
                SolverTarget::Body { body } => Some(PhysicsTarget::Body { body: body.clone() }),
                SolverTarget::Collider { collider } => model
                    .colliders
                    .iter()
                    .find(|record| &record.id == collider)
                    .map(|record| record.target.entity.clone()),
                SolverTarget::Articulation { articulation } => model
                    .articulations
                    .iter()
                    .find(|record| &record.id == articulation)
                    .map(|record| record.root.entity.clone()),
                SolverTarget::Joint { joint } => {
                    onto(joint_settings.entry(joint.clone()).or_default());
                    None
                }
                SolverTarget::PhysicalMaterial { material } => {
                    onto(material_settings.entry(material.clone()).or_default());
                    None
                }
                SolverTarget::Scene { scene } => {
                    onto(scene_settings.entry(scene.clone()).or_default());
                    None
                }
            };
            if let Some(target) = physics_target {
                add(&target, &settings.schema, lines);
            }
        }
        Self {
            occurrences,
            bodies,
            occurrence_velocities,
            body_velocities,
            colliders,
            materials,
            joint_settings,
            material_settings,
            scene_settings,
            unwritten_settings,
            scope: String::new(),
        }
    }

    /// Sets the path of the prim that holds the `Physics` scope: empty for
    /// the layer root.
    pub(super) fn set_parent(&mut self, parent: &str) {
        let old = format!("<{}/{PHYSICS}/", self.scope);
        let new = format!("<{parent}/{PHYSICS}/");
        for (_, lines) in self
            .occurrences
            .values_mut()
            .chain(self.bodies.values_mut())
            .chain(self.colliders.values_mut())
            .chain(self.joint_settings.values_mut())
            .chain(self.material_settings.values_mut())
            .chain(self.scene_settings.values_mut())
        {
            for line in lines.iter_mut() {
                if line.contains(&old) {
                    *line = line.replace(&old, &new);
                }
            }
        }
        parent.clone_into(&mut self.scope);
    }

    /// The physics of the prim of occurrence `id`, whose transform turns
    /// its parent space by `turn`.
    pub(super) fn occurrence(&self, id: &OccurrenceId, turn: Option<f64>) -> PrimPhysics {
        let mut physics = self.occurrences.get(id).cloned().unwrap_or_default();
        if let Some(velocity) = self.occurrence_velocities.get(id) {
            physics.1.extend(velocity.lines(turn));
        }
        physics
    }

    /// The physics of the prim of body `id`, whose transform turns its
    /// parent space by `turn`.
    pub(super) fn body(&self, id: &BodyId, turn: Option<f64>) -> PrimPhysics {
        let mut physics = self.bodies.get(id).cloned().unwrap_or_default();
        if let Some(velocity) = self.body_velocities.get(id) {
            physics.1.extend(velocity.lines(turn));
        }
        physics
    }

    /// The collision schemas of body `id`, for the prim of its collision
    /// geometry.
    pub(super) fn collider(&self, id: &BodyId) -> PrimPhysics {
        self.colliders.get(id).cloned().unwrap_or_default()
    }
}

/// The USD joint type of a joint, or `None` when `UsdPhysics` has no joint
/// for its family.
fn joint_type(joint: &AssemblyJoint) -> Option<&'static str> {
    Some(match joint.operands() {
        JointOperands::Grounded { .. } => "PhysicsFixedJoint",
        JointOperands::Pair { kind, .. } => match kind {
            PairedJointKind::Fixed { .. } => "PhysicsFixedJoint",
            PairedJointKind::Revolute { .. } => "PhysicsRevoluteJoint",
            PairedJointKind::Slider { .. } => "PhysicsPrismaticJoint",
            PairedJointKind::Ball {} => "PhysicsSphericalJoint",
            PairedJointKind::Distance { .. } => "PhysicsDistanceJoint",
            PairedJointKind::Cylindrical { .. } | PairedJointKind::PlaneCoincidence { .. } => {
                "PhysicsJoint"
            }
            PairedJointKind::Native { name, .. } if name == "PhysicsJoint" => "PhysicsJoint",
            PairedJointKind::Gears { coupled_joints, .. } if coupled_joints.len() == 2 => {
                "PhysxPhysicsGearJoint"
            }
            PairedJointKind::RackPinion { coupled_joints, .. } if coupled_joints.len() == 2 => {
                "PhysxPhysicsRackAndPinionJoint"
            }
            _ => return None,
        },
    })
}

fn axis_letter(axis: FrameAxis) -> &'static str {
    match axis {
        FrameAxis::X => "X",
        FrameAxis::Y => "Y",
        FrameAxis::Z => "Z",
    }
}

impl Writer<'_> {
    /// The prim path of a connector's body, or `None` for the world. `Err`
    /// when the body is not a written prim.
    fn connector_body(&self, connector: &JointConnector) -> Result<Option<String>, ()> {
        match &connector.operand.container {
            OperandContainer::Root {} => Ok(None),
            OperandContainer::Occurrence {
                occurrence,
                occurrence_path,
            } if occurrence_path.is_empty() => {
                self.paths.get(occurrence).cloned().map(Some).ok_or(())
            }
            _ => Err(()),
        }
    }

    /// The model-space placement of an occurrence.
    fn world(&self, occurrence: &OccurrenceId) -> Option<Transform> {
        let model = &self.ir.model;
        let mut current = model
            .occurrences
            .iter()
            .find(|candidate| &candidate.id == occurrence)?;
        let mut transform = current.transform?;
        for _ in 0..model.occurrences.len() {
            let Some(owner) = cadmpeg_ir::products::owner(&model.product_definitions, &current.id)
            else {
                return Some(transform);
            };
            if owner.is_root() {
                return Some(transform);
            }
            current = cadmpeg_ir::products::sole_placement(&model.occurrences, &owner.id)?;
            transform = current.transform?.compose(transform).ok()?;
        }
        None
    }

    fn frame_lines(&mut self, side: usize, frame: &Transform) {
        let [r0, r1, r2] = frame.affine_rows();
        self.out.line(&format!(
            "point3f physics:localPos{side} = {}",
            floats(&[r0[3], r1[3], r2[3]].map(|value| value / self.millimetres_per_unit))
        ));
        self.out.line(&format!(
            "quatf physics:localRot{side} = {}",
            floats(&quaternion(frame))
        ));
    }

    /// Writes one joint prim, or returns `false` when it has no `UsdPhysics`
    /// form.
    pub(super) fn joint(
        &mut self,
        joint: &AssemblyJoint,
        names: &mut Names,
        name: Option<&str>,
    ) -> bool {
        let Some(joint_type) = joint_type(joint) else {
            return false;
        };
        let connectors: Vec<&JointConnector> = joint.connectors().collect();
        let mut bodies = Vec::new();
        for connector in &connectors {
            match self.connector_body(connector) {
                Ok(body) => bodies.push(body),
                Err(()) => return false,
            }
        }
        // A grounded joint fixes its one connector to the world at the
        // connector's model-space frame, or at its offset frame.
        let sides: Vec<(Option<String>, Transform)> = match joint.operands() {
            JointOperands::Grounded {
                connector,
                offset_frame,
            } => {
                let world = offset_frame.or_else(|| match &connector.operand.container {
                    OperandContainer::Occurrence { occurrence, .. } => self
                        .world(occurrence)
                        .and_then(|placement| placement.compose(connector.frame).ok()),
                    _ => Some(connector.frame),
                });
                let Some(world) = world else {
                    return false;
                };
                vec![(None, world), (bodies[0].clone(), connector.frame)]
            }
            JointOperands::Pair { .. } => {
                // Opposed connector frames meet with the second frame turned
                // by a half turn about its x axis.
                let opposed = joint.frame_sense().or_else(|| joint.kind_sense())
                    == Some(cadmpeg_ir::products::PlaneSense::Opposed);
                let half_turn = Transform::affine([
                    [1.0, 0.0, 0.0, 0.0],
                    [0.0, -1.0, 0.0, 0.0],
                    [0.0, 0.0, -1.0, 0.0],
                ]);
                let mut sides = Vec::new();
                for (side, (connector, body)) in connectors.iter().zip(bodies).enumerate() {
                    let frame = match (side, opposed, half_turn) {
                        (1, true, Some(turn)) => match connector.frame.compose(turn) {
                            Ok(frame) => frame,
                            Err(_) => return false,
                        },
                        _ => connector.frame,
                    };
                    sides.push((body, frame));
                }
                sides
            }
        };
        let name = names.unique(name.unwrap_or_else(|| {
            joint
                .native_ref
                .as_deref()
                .and_then(|path| path.rsplit('/').next())
                .unwrap_or("Joint")
        }));
        let mut applied = Vec::new();
        let mut drives = Vec::new();
        for drive in &joint.drives {
            let instance = match (joint_type, drive.axis) {
                ("PhysicsRevoluteJoint", DriveAxis::Rotation { axis: FrameAxis::Z }) => {
                    "angular".to_owned()
                }
                ("PhysicsPrismaticJoint", DriveAxis::Translation { axis: FrameAxis::Z }) => {
                    "linear".to_owned()
                }
                ("PhysicsJoint", DriveAxis::Rotation { axis }) => {
                    format!("rot{}", axis_letter(axis))
                }
                ("PhysicsJoint", DriveAxis::Translation { axis }) => {
                    format!("trans{}", axis_letter(axis))
                }
                _ => {
                    self.unwritten.joint_drives += 1;
                    continue;
                }
            };
            applied.push(format!("PhysicsDriveAPI:{instance}"));
            drives.push((instance, drive.clone()));
        }
        let limit_axes: &[&str] = match joint.operands() {
            JointOperands::Pair {
                kind: PairedJointKind::Cylindrical { .. },
                ..
            } => &["transX", "transY", "rotX", "rotY"],
            JointOperands::Pair {
                kind: PairedJointKind::PlaneCoincidence { .. },
                ..
            } => &["transZ", "rotX", "rotY"],
            _ => &[],
        };
        for axis in limit_axes {
            applied.push(format!("PhysicsLimitAPI:{axis}"));
        }
        let (settings_schemas, settings_lines) = self
            .physics
            .joint_settings
            .get(&joint.id)
            .cloned()
            .unwrap_or_default();
        let mut metadata = if applied.is_empty() {
            Vec::new()
        } else {
            vec![format!(
                "prepend apiSchemas = [{}]",
                applied
                    .iter()
                    .map(|name| quoted(name))
                    .collect::<Vec<_>>()
                    .join(", ")
            )]
        };
        metadata.extend(settings_schemas);
        self.prim_head(&format!("def {joint_type}"), &name, &metadata);
        self.out.open();
        if let Some(key) = super::physx::placeholder(&SolverTarget::Joint {
            joint: joint.id.clone(),
        }) {
            self.target_paths.insert(key, self.out.path());
        }
        for line in &settings_lines {
            self.out.line(line);
        }
        for (side, (body, frame)) in sides.iter().enumerate() {
            if let Some(body) = body {
                self.out.line(&format!("rel physics:body{side} = <{body}>"));
            }
            self.frame_lines(side, frame);
        }
        if joint.suppressed {
            self.out.line("bool physics:jointEnabled = 0");
        }
        self.coupling_lines(joint);
        let limit =
            |limits: &Option<JointLimits>, convert: &dyn Fn(f64) -> f64| -> [Option<f64>; 2] {
                match limits {
                    None => [None, None],
                    Some(JointLimits::Minimum { minimum }) => [Some(convert(minimum.get())), None],
                    Some(JointLimits::Maximum { maximum }) => [None, Some(convert(maximum.get()))],
                    Some(JointLimits::Range(range)) => [
                        Some(convert(range.minimum().get())),
                        Some(convert(range.maximum().get())),
                    ],
                }
            };
        let (axis_limits, distance) = match joint.operands() {
            JointOperands::Pair { kind, .. } => match kind {
                PairedJointKind::Revolute { angular_limits, .. } => (
                    limit(angular_limits, &|value| {
                        value * (180.0 / std::f64::consts::PI)
                    }),
                    None,
                ),
                PairedJointKind::Slider { linear_limits, .. } => (
                    limit(linear_limits, &|value| value / self.millimetres_per_unit),
                    None,
                ),
                PairedJointKind::Distance { distance } => ([None, None], *distance),
                _ => ([None, None], None),
            },
            JointOperands::Grounded { .. } => ([None, None], None),
        };
        if matches!(joint_type, "PhysicsRevoluteJoint" | "PhysicsPrismaticJoint") {
            self.out.line("uniform token physics:axis = \"Z\"");
            if let Some(lower) = axis_limits[0] {
                self.out
                    .line(&format!("float physics:lowerLimit = {}", float(lower)));
            }
            if let Some(upper) = axis_limits[1] {
                self.out
                    .line(&format!("float physics:upperLimit = {}", float(upper)));
            }
        }
        if let Some(distance) = distance {
            let value = float(distance.get() / self.millimetres_per_unit);
            self.out
                .line(&format!("float physics:minDistance = {value}"));
            self.out
                .line(&format!("float physics:maxDistance = {value}"));
        }
        for axis in limit_axes {
            // A low limit above the high limit locks the degree of freedom.
            self.out
                .line(&format!("float limit:{axis}:physics:low = 1"));
            self.out
                .line(&format!("float limit:{axis}:physics:high = -1"));
        }
        for (instance, drive) in drives {
            let rotation = matches!(drive.axis, DriveAxis::Rotation { .. });
            let degrees = 180.0 / std::f64::consts::PI;
            // See the reader for the inverse factors.
            let millimetres = self.millimetres_per_unit;
            let (position, response, force) = if rotation {
                (
                    degrees,
                    1.0 / (millimetres * millimetres * degrees),
                    1.0 / (millimetres * millimetres),
                )
            } else {
                (1.0, 1.0, 1.0 / millimetres)
            };
            let name = |field: &str| format!("drive:{instance}:physics:{field}");
            if drive.mode == DriveMode::Acceleration {
                self.out.line(&format!(
                    "uniform token {} = \"acceleration\"",
                    name("type")
                ));
            }
            for (field, value, factor) in [
                (
                    "targetPosition",
                    drive
                        .target_position
                        .map(cadmpeg_ir::scalar::FiniteReal::get),
                    position,
                ),
                (
                    "targetVelocity",
                    drive
                        .target_velocity
                        .map(cadmpeg_ir::scalar::FiniteReal::get),
                    position,
                ),
                (
                    "stiffness",
                    drive
                        .stiffness
                        .map(cadmpeg_ir::scalar::NonNegativeReal::get),
                    response,
                ),
                (
                    "damping",
                    drive.damping.map(cadmpeg_ir::scalar::NonNegativeReal::get),
                    response,
                ),
                (
                    "maxForce",
                    drive.max_force.map(cadmpeg_ir::scalar::PositiveReal::get),
                    force,
                ),
            ] {
                if let Some(value) = value {
                    self.out.line(&format!(
                        "float {} = {}",
                        name(field),
                        float(value * factor)
                    ));
                }
            }
        }
        self.out.close();
        true
    }

    /// Writes the ratio and the coupled joints of a gear or rack-and-pinion
    /// joint. A gear stated by its pitch radii has the ratio of the radii.
    fn coupling_lines(&mut self, joint: &AssemblyJoint) {
        let JointOperands::Pair { kind, .. } = joint.operands() else {
            return;
        };
        let (ratio, coupled, names) = match kind {
            PairedJointKind::Gears {
                distance,
                distance2,
                ratio,
                coupled_joints,
            } => (
                ratio.map(PositiveReal::get).or_else(|| {
                    let (first, second) = ((*distance)?.get(), (*distance2)?.get());
                    (second != 0.0).then(|| (first / second).abs())
                }),
                coupled_joints,
                ["physics:gearRatio", "physics:hinge0", "physics:hinge1"],
            ),
            PairedJointKind::RackPinion {
                ratio,
                coupled_joints,
                ..
            } => (
                ratio.map(|ratio| (ratio.get() * self.millimetres_per_unit).to_degrees()),
                coupled_joints,
                ["physics:ratio", "physics:hinge", "physics:prismatic"],
            ),
            _ => return,
        };
        // A coupling between joints of an articulation runs outside it.
        self.out.line("bool physics:excludeFromArticulation = 1");
        match ratio {
            Some(ratio) => self
                .out
                .line(&format!("float {} = {}", names[0], float(ratio))),
            None => self.unwritten.joint_properties += 1,
        }
        for (name, coupled) in names[1..].iter().zip(coupled) {
            if let Some(key) = super::physx::placeholder(&SolverTarget::Joint {
                joint: coupled.clone(),
            }) {
                self.out.line(&format!("rel {name} = <{key}>"));
            }
        }
    }

    /// Writes the `Physics` scope with the physical materials and joints.
    pub(super) fn joints(&mut self, names: &mut Names) {
        let model = &self.ir.model;
        if model.physical_materials.is_empty()
            && model.assembly_joints.is_empty()
            && model.physics_scenes.is_empty()
        {
            return;
        }
        names.reserve(PHYSICS);
        self.prim_head("def Scope", PHYSICS, &[]);
        self.out.open();
        let mut children = Names::default();
        for scene in &model.physics_scenes {
            let name = children.unique(scene.name.as_deref().unwrap_or("PhysicsScene"));
            let (settings_schemas, settings_lines) = self
                .physics
                .scene_settings
                .get(&scene.id)
                .cloned()
                .unwrap_or_default();
            self.prim_head("def PhysicsScene", &name, &settings_schemas);
            self.out.open();
            if let Some(direction) = scene.gravity_direction {
                let direction = cadmpeg_ir::math::Vector3::from(direction);
                self.out.line(&format!(
                    "vector3f physics:gravityDirection = {}",
                    floats(&super::turned(
                        self.turn,
                        [direction.x, direction.y, direction.z]
                    ))
                ));
            }
            if let Some(magnitude) = scene.gravity_magnitude {
                self.out.line(&format!(
                    "float physics:gravityMagnitude = {}",
                    float(magnitude.get() / self.millimetres_per_unit)
                ));
            }
            for line in &settings_lines {
                self.out.line(line);
            }
            self.out.close();
        }
        for material in &model.physical_materials {
            let name = self
                .physics
                .materials
                .get(&material.id)
                .cloned()
                .unwrap_or_else(|| "Material".to_owned());
            children.reserve(&name);
            let (settings_schemas, settings_lines) = self
                .physics
                .material_settings
                .get(&material.id)
                .cloned()
                .unwrap_or_default();
            let mut metadata = vec!["prepend apiSchemas = [\"PhysicsMaterialAPI\"]".to_owned()];
            metadata.extend(settings_schemas);
            self.prim_head("def Material", &name, &metadata);
            self.out.open();
            for line in &settings_lines {
                self.out.line(line);
            }
            for (field, value) in [
                (
                    "density",
                    material
                        .density
                        .map(|value| value.get() * density_scale(self.millimetres_per_unit)),
                ),
                (
                    "staticFriction",
                    material
                        .static_friction
                        .map(cadmpeg_ir::scalar::NonNegativeReal::get),
                ),
                (
                    "dynamicFriction",
                    material
                        .dynamic_friction
                        .map(cadmpeg_ir::scalar::NonNegativeReal::get),
                ),
                (
                    "restitution",
                    material
                        .restitution
                        .map(cadmpeg_ir::scalar::NonNegativeReal::get),
                ),
            ] {
                if let Some(value) = value {
                    self.out
                        .line(&format!("float physics:{field} = {}", float(value)));
                }
            }
            self.out.close();
        }
        for joint in &model.assembly_joints {
            if !self.joint(joint, &mut children, None) {
                self.unwritten.joints += 1;
            }
        }
        self.out.close();
    }
}

#[cfg(test)]
mod tests;
