// SPDX-License-Identifier: Apache-2.0
//! `UsdPhysics` records decode in IR units and survive a write.

use cadmpeg_ir::design::construction::FrameAxis as DesignAxis;
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::shared::{
    AxisLimits, Bounds, Coupling, Joint, JointKind, JointMotion, JointOffset,
};
use cadmpeg_ir::physics::PhysicsTarget;
use cadmpeg_ir::products::{DriveAxis, FrameAxis, JointLimits, JointOperands, PairedJointKind};
use cadmpeg_ir::scalar::{Angle, Length};

use crate::test_support::{decode, encode};

/// Relative agreement of a binary32 attribute with its scaled value.
const FLOAT_TOLERANCE: f64 = 1e-6;

/// The design joint of the joint prim at `path`.
fn joint_feature<'ir>(ir: &'ir mut cadmpeg_ir::CadIr, path: &str) -> &'ir mut Joint {
    let id = crate::ids::joint_feature(path);
    let feature = ir
        .design
        .features
        .iter_mut()
        .find(|feature| feature.id == id)
        .unwrap_or_else(|| panic!("no joint feature of {path}"));
    let FeatureOperation::Joint(joint) = &mut feature.definition.operation else {
        panic!("{path} is not a joint feature");
    };
    joint
}

fn close(found: f64, expected: f64) -> bool {
    (found - expected).abs() <= FLOAT_TOLERANCE * expected.abs().max(1.0)
}

const LAYER: &str = "#usda 1.0\n(\n    metersPerUnit = 0.01\n    kilogramsPerUnit = 2\n)\ndef Xform \"A\" (\n    prepend apiSchemas = [\"PhysicsArticulationRootAPI\"]\n)\n{\n    def Xform \"Base\" (\n        prepend apiSchemas = [\"PhysicsRigidBodyAPI\", \"PhysicsMassAPI\"]\n    )\n    {\n        float physics:mass = 3\n        point3f physics:centerOfMass = (0, 0, 1)\n        float3 physics:diagonalInertia = (1, 2, 3)\n        quatf physics:principalAxes = (1, 0, 0, 0)\n    }\n    def Xform \"Arm\" (\n        prepend apiSchemas = [\"PhysicsRigidBodyAPI\"]\n    )\n    {\n        bool physics:kinematicEnabled = 1\n    }\n    def PhysicsRevoluteJoint \"Hinge\" (\n        prepend apiSchemas = [\"PhysicsDriveAPI:angular\"]\n    )\n    {\n        rel physics:body0 = </A/Base>\n        rel physics:body1 = </A/Arm>\n        uniform token physics:axis = \"X\"\n        point3f physics:localPos0 = (1, 2, 3)\n        float physics:lowerLimit = -90\n        float physics:upperLimit = 90\n        float drive:angular:physics:stiffness = 10\n        float drive:angular:physics:targetPosition = 45\n    }\n}\n";

#[test]
fn physics_records_decode_in_ir_units() {
    let result = decode(LAYER);
    let model = &result.ir().model;
    assert_eq!(model.rigid_bodies.len(), 2);
    assert!(model.rigid_bodies.iter().any(|body| body.kinematic));
    assert_eq!(model.articulations.len(), 1);
    let mass = &model.mass_properties[0];
    assert!(close(mass.mass.unwrap().get(), 6.0));
    let center = mass.center_of_mass.unwrap().get();
    assert!(close(center.z, 10.0));
    // kilograms per unit times squared millimetres per unit.
    let moments = mass.inertia.as_ref().unwrap().moments;
    assert!(close(moments[2].get(), 3.0 * 2.0 * 100.0));
    assert!(mass.inertia.as_ref().unwrap().axes.is_none());
    let joint = &model.assembly_joints[0];
    let JointOperands::Pair {
        kind:
            PairedJointKind::Revolute {
                angular_limits: Some(JointLimits::Range(range)),
                ..
            },
        connectors,
        ..
    } = joint.operands()
    else {
        panic!("a revolute joint");
    };
    assert!(close(range.maximum().get(), std::f64::consts::FRAC_PI_2));
    // The frame's z axis is the joint's X axis; its origin is in millimetres.
    let rows = connectors[0].frame.affine_rows();
    assert!(close(rows[0][2], 1.0));
    assert!(close(rows[0][3], 10.0) && close(rows[2][3], 30.0));
    let drive = &joint.drives[0];
    assert_eq!(drive.axis, DriveAxis::Rotation { axis: FrameAxis::Z });
    assert!(close(
        drive.target_position.unwrap().get(),
        std::f64::consts::FRAC_PI_4
    ));
    // Torque per degree in kilograms and centimetres, to kilogram square
    // millimetres per second squared per radian.
    let expected = 10.0 * 2.0 * 10.0 * 10.0 * 180.0 / std::f64::consts::PI;
    assert!(close(drive.stiffness.unwrap().get(), expected));
}

#[test]
fn physics_records_survive_a_write() {
    let first = decode(LAYER);
    let (text, report) = encode(first.ir());
    assert!(report.losses.is_empty(), "{:#?}", report.losses);
    let second = decode(&text);
    let (before, after) = (&first.ir().model, &second.ir().model);
    assert_eq!(before.rigid_bodies.len(), after.rigid_bodies.len());
    assert_eq!(before.articulations.len(), after.articulations.len());
    assert_eq!(before.assembly_joints.len(), after.assembly_joints.len());
    let masses = |model: &cadmpeg_ir::document::Model| {
        model
            .mass_properties
            .iter()
            .map(|mass| mass.mass.map(cadmpeg_ir::scalar::PositiveReal::get))
            .collect::<Vec<_>>()
    };
    assert_eq!(masses(before), masses(after));
    let (one, two) = (&before.assembly_joints[0], &after.assembly_joints[0]);
    let stiffness =
        |joint: &cadmpeg_ir::products::AssemblyJoint| joint.drives[0].stiffness.unwrap().get();
    assert!(close(stiffness(one), stiffness(two)));
    for (left, right) in one.connectors().zip(two.connectors()) {
        for (row_left, row_right) in left
            .frame
            .affine_rows()
            .iter()
            .zip(right.frame.affine_rows())
        {
            for (a, b) in row_left.iter().zip(row_right) {
                assert!(close(*a, b), "{a} != {b}");
            }
        }
    }
    assert!(after
        .rigid_bodies
        .iter()
        .all(|body| matches!(body.target.entity, PhysicsTarget::Occurrence { .. })));
}

#[test]
fn a_collider_of_a_brep_body_is_written_on_its_mesh_and_read_back() {
    let mut ir = decode("#usda 1.0\ndef Cylinder \"C\"\n{\n}\n").ir().clone();
    let body = ir.geometry.bodies[0].id.clone();
    ir.model.colliders.push(cadmpeg_ir::physics::Collider {
        id: crate::ids::collider("collider"),
        target: PhysicsTarget::Body { body }.into(),
        shape: cadmpeg_ir::physics::ColliderShape::ConvexHull,
        enabled: true,
        material: None,
        native_ref: None,
    });
    let (written, _) = encode(&ir);
    let mesh = written.split("def Mesh \"Mesh\"").nth(1).expect("a mesh");
    let mesh_head: String = mesh.lines().take_while(|line| line.trim() != "{").collect();
    assert!(mesh_head.contains("PhysicsCollisionAPI"), "{written}");
    assert!(mesh_head.contains("PhysicsMeshCollisionAPI"), "{written}");
    let again = decode(&written);
    assert!(
        !again
            .report()
            .losses
            .iter()
            .any(|loss| loss.message.contains("PhysicsCollisionAPI")),
        "{:#?}",
        again.report().losses
    );
    let model = &again.ir().model;
    assert_eq!(model.colliders.len(), 1);
    assert!(matches!(
        model.colliders[0].target.entity,
        PhysicsTarget::Body { .. }
    ));
    assert!(matches!(
        model.colliders[0].shape,
        cadmpeg_ir::physics::ColliderShape::ConvexHull
    ));
}

fn physx_fixture() -> String {
    std::fs::read_to_string(format!(
        "{}/tests/golden/fixtures/physx_arm.usda",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("fixture")
}

/// The value of property `name` of the settings of schema `schema`.
fn solver_value(
    ir: &cadmpeg_ir::CadIr,
    schema: &str,
    name: &str,
) -> Option<cadmpeg_ir::physics::solver::SolverValue> {
    ir.model
        .solver_settings
        .iter()
        .filter(|settings| settings.schema == schema)
        .find_map(|settings| settings.values.get(name).cloned())
}

fn real(value: Option<cadmpeg_ir::physics::solver::SolverValue>) -> f64 {
    match value {
        Some(cadmpeg_ir::physics::solver::SolverValue::Real { value, .. }) => value.get(),
        other => panic!("not a real value: {other:?}"),
    }
}

#[test]
fn physx_settings_decode_in_ir_units() {
    let result = decode(&physx_fixture());
    let ir = result.ir();
    // Square centimetres per square second to square millimetres.
    assert!(close(
        real(solver_value(
            ir,
            "PhysxRigidBodyAPI",
            "physxRigidBody:sleepThreshold"
        )),
        50.0
    ));
    // Degrees per second to radians per second.
    assert!(close(
        real(solver_value(
            ir,
            "PhysxRigidBodyAPI",
            "physxRigidBody:maxAngularVelocity"
        )),
        2.0 * std::f64::consts::PI
    ));
    assert!(matches!(
        solver_value(ir, "PhysxRigidBodyAPI", "physxRigidBody:maxLinearVelocity"),
        Some(cadmpeg_ir::physics::solver::SolverValue::Unbounded { negative: false })
    ));
    // A revolute armature is a moment of inertia; a prismatic one a mass.
    let armatures: Vec<f64> = ir
        .model
        .solver_settings
        .iter()
        .filter(|settings| settings.schema == "PhysxJointAPI")
        .filter_map(|settings| settings.values.get("physxJoint:armature"))
        .map(|value| real(Some(value.clone())))
        .collect();
    assert!(
        armatures.iter().any(|value| close(*value, 1.0)),
        "{armatures:?}"
    );
    assert!(
        armatures.iter().any(|value| close(*value, 0.5)),
        "{armatures:?}"
    );
    let shoulder = crate::ids::joint_feature("/Arm/Shoulder");
    assert!(matches!(
        solver_value(ir, "PhysxMimicJointAPI:transX", "physxMimicJoint:transX:referenceJoint"),
        Some(cadmpeg_ir::physics::solver::SolverValue::Targets { targets })
            if matches!(targets.as_slice(), [target] if matches!(
                &target.entity,
                cadmpeg_ir::physics::solver::SolverTarget::Joint { joint } if *joint == shoulder
            ))
    ));
    assert!(
        result.report().losses.iter().all(|loss| !loss
            .message
            .contains("PhysX properties were not transferred")),
        "{:#?}",
        result.report().losses
    );
    let scene = &ir.model.physics_scenes[0];
    assert!(close(scene.gravity_magnitude.unwrap().get(), 9810.0));
}

#[test]
fn physx_settings_survive_a_write() {
    let first = decode(&physx_fixture());
    let (text, report) = encode(first.ir());
    assert!(
        report
            .losses
            .iter()
            .all(|loss| !loss.message.contains("solver properties")),
        "{:#?}",
        report.losses
    );
    let second = decode(&text);
    let (before, after) = (&first.ir().model, &second.ir().model);
    assert_eq!(before.solver_settings.len(), after.solver_settings.len());
    for settings in &before.solver_settings {
        let again = after
            .solver_settings
            .iter()
            .find(|candidate| {
                candidate.schema == settings.schema
                    && candidate.values.len() == settings.values.len()
            })
            .unwrap_or_else(|| panic!("{} was not written\n{text}", settings.schema));
        for (name, value) in &settings.values {
            match (value, again.values.get(name)) {
                (
                    cadmpeg_ir::physics::solver::SolverValue::Real {
                        value: left,
                        dimension,
                    },
                    Some(cadmpeg_ir::physics::solver::SolverValue::Real {
                        value: right,
                        dimension: other,
                    }),
                ) => {
                    assert_eq!(dimension, other, "{name}");
                    assert!(close(right.get(), left.get()), "{name}: {left:?} {right:?}");
                }
                (
                    cadmpeg_ir::physics::solver::SolverValue::Targets { .. },
                    Some(cadmpeg_ir::physics::solver::SolverValue::Targets { .. }),
                ) => {}
                (left, right) => assert_eq!(Some(left), right, "{name}"),
            }
        }
    }
    assert_eq!(before.physics_scenes.len(), after.physics_scenes.len());
}

/// Each physics joint is a design joint feature in the stage's scope: the
/// feature of a joint under the arm prim belongs to the arm's definition, and
/// its motion names the joint's axis.
#[test]
fn physics_joints_are_design_joint_features() {
    let result = decode(&physx_fixture());
    let ir = result.ir();
    let arm = ir
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.id == crate::ids::occurrence("/Arm"))
        .expect("the arm occurrence");
    let motion = |path: &str| {
        let id = crate::ids::joint_feature(path);
        let feature = ir
            .design
            .features
            .iter()
            .find(|feature| feature.id == id)
            .unwrap_or_else(|| panic!("no joint feature of {path}"));
        assert_eq!(feature.component.as_ref(), Some(&arm.prototype), "{path}");
        assert_eq!(feature.name.as_deref(), path.rsplit('/').next(), "{path}");
        let FeatureOperation::Joint(joint) = &feature.definition.operation else {
            panic!("{path} is not a joint feature");
        };
        joint.kind.clone()
    };
    assert!(matches!(
        motion("/Arm/Shoulder"),
        JointKind::Motion {
            motion: JointMotion::Revolute { axis: DesignAxis::X },
            limits,
            ..
        } if matches!(limits.as_slice(), [AxisLimits::Rotation { axis: DesignAxis::X, .. }])
    ));
    assert!(matches!(
        motion("/Arm/Grip"),
        JointKind::Motion {
            motion: JointMotion::Slider { axis: DesignAxis::X },
            limits,
            ..
        } if matches!(limits.as_slice(), [AxisLimits::Translation { axis: DesignAxis::X, .. }])
    ));
    let scope = ir
        .design
        .scopes
        .iter()
        .find(|scope| scope.definition == crate::ids::stage())
        .expect("the stage's scope");
    assert_eq!(scope.roots, [crate::ids::joint_group()]);
    let report = cadmpeg_ir::validate::validate_neutral(ir, Vec::new()).expect("validate");
    assert_eq!(report.error_count(), 0, "{:#?}", report.findings);
}

/// Decodes `text` at `depth`.
fn decode_at(
    text: &str,
    depth: cadmpeg_core::decode::DecodeDepth,
) -> cadmpeg_ir::codec::DecodeResult {
    use cadmpeg_ir::codec::Codec;
    let options = cadmpeg_ir::codec::DecodeOptions {
        depth,
        ..Default::default()
    };
    crate::UsdCodec
        .decode(&mut std::io::Cursor::new(text.as_bytes()), &options)
        .expect("decode")
}

/// The `decode/depth.not-delivered` losses of `result`.
fn depth_losses(result: &cadmpeg_ir::codec::DecodeResult) -> Vec<&str> {
    result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.to_string() == "decode/depth.not-delivered")
        .map(|loss| loss.message.as_str())
        .collect()
}

/// A decode at geometry depth leaves out the joints, which are design
/// features, and the `PhysX` settings that target them, and charges
/// `decode/depth.not-delivered` for `design` in their place.
#[test]
fn a_geometry_decode_of_joints_charges_the_undelivered_design() {
    use cadmpeg_core::decode::DecodeDepth;
    let result = decode_at(&physx_fixture(), DecodeDepth::Geometry);
    let ir = result.ir();
    assert!(!ir.scope().design);
    assert!(ir.design.features.is_empty());
    assert_eq!(ir.model.assembly_joints.len(), 2);
    assert!(solver_value(ir, "PhysxJointAPI", "physxJoint:armature").is_none());
    assert!(ir.model.solver_settings.iter().all(|settings| !matches!(
        settings.target.entity,
        cadmpeg_ir::physics::solver::SolverTarget::Joint { .. }
    )));
    let losses = depth_losses(&result);
    assert_eq!(losses.len(), 1, "{losses:?}");
    assert!(losses[0].contains("design records"), "{}", losses[0]);
    for loss in &result.report().losses {
        for property in [
            "/Arm/Shoulder.physxJoint:armature",
            "/Arm/Grip.physxMimicJoint:transX:referenceJoint",
        ] {
            assert!(!loss.message.contains(property), "{}", loss.message);
        }
    }
}

/// A decode that delivers the design states the joints and charges no
/// depth loss.
#[test]
fn a_design_decode_of_joints_charges_no_depth_loss() {
    let result = decode_at(&physx_fixture(), cadmpeg_core::decode::DecodeDepth::Design);
    assert!(!result.ir().design.features.is_empty());
    assert!(
        depth_losses(&result).is_empty(),
        "{:#?}",
        result.report().losses
    );
}

/// A stage without joints holds no design, so neither depth charges one.
#[test]
fn a_stage_without_joints_charges_no_depth_loss() {
    use cadmpeg_core::decode::DecodeDepth;
    let layer = "#usda 1.0\ndef Xform \"Base\" (\n    prepend apiSchemas = [\"PhysicsRigidBodyAPI\"]\n)\n{\n}\n";
    for depth in [DecodeDepth::Geometry, DecodeDepth::Design] {
        let result = decode_at(layer, depth);
        assert!(result.ir().model.assembly_joints.is_empty());
        assert!(
            depth_losses(&result).is_empty(),
            "{depth}: {:#?}",
            result.report().losses
        );
    }
}

/// A saved document holds the joints only as design joint features. A write
/// from it keeps the joints, their `PhysX` settings, and the mimic coupling
/// that names the shoulder joint.
#[test]
fn joints_and_their_physx_settings_survive_a_saved_document() {
    let first = decode(&physx_fixture());
    let saved = cadmpeg_ir::CadIr::from_json(
        &first
            .ir()
            .to_canonical_json()
            .expect("serialize the document"),
    )
    .expect("read the document");
    assert!(saved.model.assembly_joints.is_empty());
    let (text, report) = encode(&saved);
    assert!(
        report.losses.iter().all(|loss| {
            !loss.message.contains("solver properties") && !loss.message.contains("joints")
        }),
        "{:#?}",
        report.losses
    );
    let second = decode(&text);
    let ir = second.ir();
    assert_eq!(
        ir.model.assembly_joints.len(),
        first.ir().model.assembly_joints.len(),
        "{text}"
    );
    assert_eq!(
        ir.model.solver_settings.len(),
        first.ir().model.solver_settings.len(),
        "{text}"
    );
    let Some(cadmpeg_ir::physics::solver::SolverValue::Targets { targets }) = solver_value(
        ir,
        "PhysxMimicJointAPI:transX",
        "physxMimicJoint:transX:referenceJoint",
    ) else {
        panic!("no mimic reference joint:\n{text}");
    };
    let [target] = targets.as_slice() else {
        panic!("{targets:?}");
    };
    let cadmpeg_ir::physics::solver::SolverTarget::Joint { joint } = &target.entity else {
        panic!("{target:?}");
    };
    let referenced = ir
        .design
        .features
        .iter()
        .find(|feature| &feature.id == joint)
        .expect("the referenced joint feature");
    assert_eq!(referenced.name.as_deref(), Some("Shoulder"));
}

const COUPLINGS: &str = "#usda 1.0\n(\n    metersPerUnit = 0.01\n)\ndef Xform \"Rig\"\n{\n    def Xform \"Base\" (\n        prepend apiSchemas = [\"PhysicsRigidBodyAPI\"]\n    )\n    {\n    }\n    def Xform \"A\" (\n        prepend apiSchemas = [\"PhysicsRigidBodyAPI\"]\n    )\n    {\n    }\n    def Xform \"B\" (\n        prepend apiSchemas = [\"PhysicsRigidBodyAPI\"]\n    )\n    {\n    }\n    def Xform \"Rack\" (\n        prepend apiSchemas = [\"PhysicsRigidBodyAPI\"]\n    )\n    {\n    }\n    def PhysicsRevoluteJoint \"HingeA\"\n    {\n        rel physics:body0 = </Rig/Base>\n        rel physics:body1 = </Rig/A>\n    }\n    def PhysicsRevoluteJoint \"HingeB\"\n    {\n        rel physics:body0 = </Rig/Base>\n        rel physics:body1 = </Rig/B>\n    }\n    def PhysicsPrismaticJoint \"Track\"\n    {\n        rel physics:body0 = </Rig/Base>\n        rel physics:body1 = </Rig/Rack>\n    }\n    def PhysxPhysicsGearJoint \"Gear\"\n    {\n        rel physics:body0 = </Rig/A>\n        rel physics:body1 = </Rig/B>\n        float physics:gearRatio = 2\n        rel physics:hinge0 = </Rig/HingeA>\n        rel physics:hinge1 = </Rig/HingeB>\n    }\n    def PhysxPhysicsRackAndPinionJoint \"Pinion\"\n    {\n        rel physics:body0 = </Rig/A>\n        rel physics:body1 = </Rig/Rack>\n        float physics:ratio = 36\n        rel physics:hinge = </Rig/HingeA>\n        rel physics:prismatic = </Rig/Track>\n    }\n}\n";

fn coupling(model: &cadmpeg_ir::document::Model, gear: bool) -> (f64, Vec<String>) {
    model
        .assembly_joints
        .iter()
        .find_map(|joint| match joint.operands() {
            JointOperands::Pair {
                kind:
                    PairedJointKind::Gears {
                        ratio,
                        coupled_joints,
                        ..
                    },
                ..
            } if gear => Some((ratio.unwrap().get(), coupled_joints.clone())),
            JointOperands::Pair {
                kind:
                    PairedJointKind::RackPinion {
                        ratio,
                        coupled_joints,
                        ..
                    },
                ..
            } if !gear => Some((ratio.unwrap().get(), coupled_joints.clone())),
            _ => None,
        })
        .map(|(ratio, coupled)| (ratio, coupled.iter().map(ToString::to_string).collect()))
        .expect("a coupling joint")
}

#[test]
fn gear_and_rack_joints_couple_their_joints_and_survive_a_write() {
    let first = decode(COUPLINGS);
    let model = &first.ir().model;
    // PhysX turns B the other way at twice A's angle: a ratio of -2.
    let (gear_ratio, gear_joints) = coupling(model, true);
    assert!(close(gear_ratio, -2.0));
    assert_eq!(
        gear_joints,
        [
            crate::ids::joint("/Rig/HingeA"),
            crate::ids::joint("/Rig/HingeB")
        ]
        .map(|id| id.to_string())
    );
    // 36 degrees per centimetre is π / 50 radians per millimetre.
    let (rack_ratio, rack_joints) = coupling(model, false);
    assert!(close(rack_ratio, std::f64::consts::PI / 50.0));
    assert_eq!(
        rack_joints,
        [
            crate::ids::joint("/Rig/HingeA"),
            crate::ids::joint("/Rig/Track")
        ]
        .map(|id| id.to_string())
    );
    let (text, _) = encode(first.ir());
    assert!(text.contains("def PhysxPhysicsGearJoint"), "{text}");
    assert!(text.contains("float physics:gearRatio = 2"), "{text}");
    assert!(text.contains("float physics:ratio = 3600"), "{text}");
    let second = decode(&text);
    let again = &second.ir().model;
    assert!(close(coupling(again, true).0, -2.0));
    assert!(close(coupling(again, false).0, rack_ratio));
    assert_eq!(coupling(again, true).1.len(), 2);
    assert_eq!(coupling(again, false).1.len(), 2);
    assert!(
        second
            .report()
            .losses
            .iter()
            .all(|loss| !loss.message.contains("coupled joints")),
        "{:#?}",
        second.report().losses
    );
}

/// A coupling joint needs the joints it couples. Where one of them is given
/// up, the couplings that name it leave the document with it, named in its
/// `record.unreadable` and charged nothing of their own.
#[test]
fn couplings_of_a_given_up_hinge_are_dropped_with_it() {
    let text = COUPLINGS.replacen(
        "rel physics:body1 = </Rig/A>\n    }\n    def PhysicsRevoluteJoint \"HingeB\"",
        "rel physics:body1 = </Rig/Missing>\n    }\n    def PhysicsRevoluteJoint \"HingeB\"",
        1,
    );
    assert_ne!(text, COUPLINGS);
    let result = decode(&text);
    let gone = ["/Rig/HingeA", "/Rig/Gear", "/Rig/Pinion"];
    let model = &result.ir().model;
    for path in gone {
        let joint = crate::ids::joint(path);
        assert!(
            model
                .assembly_joints
                .iter()
                .all(|record| record.id != joint),
            "{path}"
        );
        let feature = crate::ids::joint_feature(path);
        assert!(
            result
                .ir()
                .design
                .features
                .iter()
                .all(|record| record.id != feature),
            "{path}"
        );
    }
    for path in ["/Rig/HingeB", "/Rig/Track"] {
        let joint = crate::ids::joint(path);
        assert!(
            model
                .assembly_joints
                .iter()
                .any(|record| record.id == joint),
            "{path}"
        );
    }
    let mut dropped = [
        crate::ids::joint("/Rig/Gear").to_string(),
        crate::ids::joint("/Rig/Pinion").to_string(),
    ];
    dropped.sort();
    let charges: Vec<&str> = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == cadmpeg_ir::unknown::part::record_unreadable_kind())
        .map(|loss| loss.message.as_str())
        .collect();
    assert_eq!(
        charges,
        [format!(
            "record {} given up: usd/physics.joint-rejected: the joint names a body that is not a \
             transferred prim; dropped=[{}]",
            crate::ids::joint("/Rig/HingeA"),
            dropped.join(", ")
        )]
    );
}

/// A coupling relationship that targets a prim that is no transferred joint
/// gives the coupling up: an empty coupled list would state that it acts on
/// its connectors directly.
#[test]
fn a_coupling_whose_relationship_names_no_joint_is_given_up() {
    let text = COUPLINGS.replacen(
        "rel physics:hinge1 = </Rig/HingeB>",
        "rel physics:hinge1 = </Rig/Base>",
        1,
    );
    assert_ne!(text, COUPLINGS);
    let result = decode(&text);
    let model = &result.ir().model;
    let gear = crate::ids::joint("/Rig/Gear");
    assert!(model.assembly_joints.iter().all(|joint| joint.id != gear));
    assert_eq!(coupling(model, false).1.len(), 2);
    let charges: Vec<&str> = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == cadmpeg_ir::unknown::part::record_unreadable_kind())
        .map(|loss| loss.message.as_str())
        .collect();
    assert_eq!(
        charges,
        [format!(
            "record {gear} given up: usd/physics.joint-rejected: the joint couples a joint that \
             is not a transferred joint; dropped=[]"
        )]
    );
}

/// A negative `physics:gearRatio` turns both gears the same way, as an
/// internal gear does: a positive ratio, written back as it was read. A
/// negative rack ratio moves the rack against the pinion's turn.
#[test]
fn couplings_that_turn_the_same_way_keep_their_sign() {
    let source = COUPLINGS
        .replace("physics:gearRatio = 2", "physics:gearRatio = -0.5")
        .replace("physics:ratio = 36", "physics:ratio = -36");
    let first = decode(&source);
    let model = &first.ir().model;
    assert!(close(coupling(model, true).0, 0.5));
    assert!(close(
        coupling(model, false).0,
        -std::f64::consts::PI / 50.0
    ));
    let (text, _) = encode(first.ir());
    assert!(text.contains("float physics:gearRatio = -0.5"), "{text}");
    assert!(text.contains("float physics:ratio = -3600"), "{text}");
}

#[test]
fn a_gear_stated_by_its_radii_writes_the_ratio_of_the_radii() {
    let mut ir = decode(COUPLINGS).ir().clone();
    let JointKind::Coupling { coupling, .. } = &mut joint_feature(&mut ir, "/Rig/Gear").kind else {
        panic!("the gear is a coupling");
    };
    *coupling = Coupling::Gear {
        ratio: None,
        radii: [30.0, 12.0].map(|radius| Length::new(radius).map(Dimension::Literal)),
    };
    let (text, _) = encode(&ir);
    assert!(text.contains("float physics:gearRatio = 2.5"), "{text}");
}

#[test]
fn initial_velocities_survive_a_write_and_unread_body_properties_are_reported() {
    let text = "#usda 1.0\n(\n    metersPerUnit = 0.01\n)\ndef Xform \"A\" (\n    prepend apiSchemas = [\"PhysicsRigidBodyAPI\"]\n)\n{\n    vector3f physics:velocity = (10, 0, 0)\n    vector3f physics:angularVelocity = (0, 0, 90)\n    bool physics:startsAsleep = 1\n    rel physics:simulationOwner = </Scene>\n}\n";
    let first = decode(text);
    let body = &first.ir().model.rigid_bodies[0];
    let velocity = body.linear_velocity.unwrap().get();
    assert!(close(velocity.x, 100.0));
    assert!(close(
        body.angular_velocity.unwrap().get().z,
        std::f64::consts::FRAC_PI_2
    ));
    assert!(body.starts_asleep);
    assert!(
        first
            .report()
            .losses
            .iter()
            .any(|loss| loss.message.contains("/A.physics:simulationOwner")),
        "{:#?}",
        first.report().losses
    );
    let (written, _) = encode(first.ir());
    assert!(
        written.contains("vector3f physics:velocity = (0.1, 0, 0)"),
        "{written}"
    );
    assert!(
        written.contains("vector3f physics:angularVelocity = (0, 0, 90)"),
        "{written}"
    );
    let second = decode(&written);
    let again = &second.ir().model.rigid_bodies[0];
    assert!(close(again.linear_velocity.unwrap().get().x, 100.0));
    assert!(again.starts_asleep);
}

/// A cylindrical joint is a generic joint that locks the other four degrees of
/// freedom and limits the two it leaves free: the turn about z in degrees and
/// the travel along z in metres.
#[test]
fn a_cylindrical_joint_limits_its_free_axes() {
    fn bounds<Q>(min: Q, max: Q) -> Bounds<Q> {
        Bounds {
            min: Some(Dimension::Literal(min)),
            max: Some(Dimension::Literal(max)),
        }
    }
    let mut ir = decode(LAYER).ir().clone();
    let angle = |value: f64| Angle::new(value).unwrap();
    let length = |value: f64| Length::new(value).unwrap();
    joint_feature(&mut ir, "/A/Hinge").kind = JointKind::Motion {
        motion: JointMotion::Cylindrical {
            axis: DesignAxis::X,
        },
        axes: None,
        offset: JointOffset {
            x: None,
            y: None,
            z: None,
            angle: None,
        },
        limits: vec![
            AxisLimits::Rotation {
                axis: DesignAxis::X,
                limits: Some(bounds(
                    angle(-std::f64::consts::FRAC_PI_6),
                    angle(std::f64::consts::FRAC_PI_3),
                )),
                rest: None,
            },
            AxisLimits::Translation {
                axis: DesignAxis::X,
                limits: Some(bounds(length(0.0), length(20.0))),
                rest: None,
            },
        ],
        drives: Vec::new(),
    };
    let (text, _) = encode(&ir);
    let prim = text
        .split("def PhysicsJoint")
        .nth(1)
        .unwrap_or_else(|| panic!("no generic joint:\n{text}"));
    for axis in ["transX", "transY", "rotX", "rotY", "rotZ", "transZ"] {
        assert!(
            prim.contains(&format!("\"PhysicsLimitAPI:{axis}\"")),
            "{text}"
        );
    }
    let value = |name: &str| -> f64 {
        let at = prim
            .find(&format!("float {name} = "))
            .unwrap_or_else(|| panic!("no {name}:\n{text}"));
        prim[at + name.len() + 9..]
            .lines()
            .next()
            .unwrap_or_default()
            .trim()
            .parse()
            .unwrap()
    };
    assert!(close(value("limit:rotZ:physics:low"), -30.0), "{text}");
    assert!(close(value("limit:rotZ:physics:high"), 60.0), "{text}");
    assert!(close(value("limit:transZ:physics:low"), 0.0), "{text}");
    assert!(close(value("limit:transZ:physics:high"), 0.02), "{text}");
}

/// The `BrepArray` box with a mass of 2 kg centred at (5, 10, 15) mm, set on
/// its one body.
fn box_with_mass() -> cadmpeg_ir::CadIr {
    let text = std::fs::read_to_string(format!(
        "{}/tests/golden/fixtures/brep_array_box.usda",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("fixture");
    let mut ir = decode(&text).ir().clone();
    let body = ir.geometry.bodies[0].id.clone();
    ir.model
        .mass_properties
        .push(cadmpeg_ir::physics::MassProperties {
            id: cadmpeg_ir::ids::MassPropertiesId::try_from("test:usd:mass#box".to_owned())
                .expect("an id"),
            target: PhysicsTarget::Body { body }.into(),
            mass: cadmpeg_ir::scalar::PositiveReal::new(2.0),
            density: None,
            center_of_mass: cadmpeg_ir::features::geometry::FinitePoint3::new(
                cadmpeg_ir::math::Point3::new(5.0, 10.0, 15.0),
            ),
            inertia: None,
            material: None,
            native_ref: None,
        });
    ir
}

/// A body written as a group of prims (its B-rep and its mesh) carries its
/// mass on the group prim, in kilograms and metres.
#[test]
fn a_group_body_carries_its_mass() {
    let (text, report) = encode(&box_with_mass());
    let group = text
        .split("def Xform")
        .find(|prim| prim.contains("def BrepArray"))
        .expect("the body's group prim");
    let head = group.split('{').next().unwrap_or_default();
    assert!(head.contains("PhysicsMassAPI"), "{text}");
    assert!(group.contains("float physics:mass = 2\n"), "{text}");
    assert!(
        group.contains("point3f physics:centerOfMass = (0.005, 0.01, 0.015)"),
        "{text}"
    );
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::UsdLossCode::ExportPhysicsNotWritten.kind()));
}

/// A body with a transform of its own is a group prim whose space is not the
/// body's: its mass is not written, and a loss names the body.
#[test]
fn a_transformed_group_body_reports_its_mass() {
    let mut ir = box_with_mass();
    ir.geometry.bodies[0].transform = Some(
        cadmpeg_ir::transform::Transform::affine([
            [1.0, 0.0, 0.0, 10.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .expect("a transform"),
    );
    let (text, report) = encode(&ir);
    assert!(!text.contains("physics:mass"), "{text}");
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::UsdLossCode::ExportPhysicsNotWritten.kind()));
}

/// `LAYER` with its hinge replaced by a generic joint that applies
/// `PhysicsLimitAPI` with `limits`: `(instance, low, high)` in degrees and
/// scene units (centimetres), low above high locking the axis.
fn generic_joint(limits: &[(&str, f64, f64)]) -> String {
    use std::fmt::Write as _;
    let start = LAYER.find("    def PhysicsRevoluteJoint").unwrap();
    let end = start + LAYER[start..].find("    }\n}\n").unwrap() + "    }\n".len();
    let schemas: Vec<String> = limits
        .iter()
        .map(|(instance, _, _)| format!("\"PhysicsLimitAPI:{instance}\""))
        .collect();
    let mut values = String::new();
    for (instance, low, high) in limits {
        writeln!(values, "        float limit:{instance}:physics:low = {low}").unwrap();
        writeln!(
            values,
            "        float limit:{instance}:physics:high = {high}"
        )
        .unwrap();
    }
    let joint = format!(
        "    def PhysicsJoint \"Joint\" (\n        prepend apiSchemas = [{}]\n    )\n    {{\n        rel physics:body0 = </A/Base>\n        rel physics:body1 = </A/Arm>\n{values}    }}\n",
        schemas.join(", ")
    );
    format!("{}{joint}{}", &LAYER[..start], &LAYER[end..])
}

const LOCKED: [(&str, f64, f64); 4] = [
    ("transX", 1.0, -1.0),
    ("transY", 1.0, -1.0),
    ("rotX", 1.0, -1.0),
    ("rotY", 1.0, -1.0),
];

fn joint_kind(source: &str) -> (PairedJointKind, Vec<String>) {
    let result = decode(source);
    let JointOperands::Pair { kind, .. } = result.ir().model.assembly_joints[0].operands() else {
        panic!("a paired joint");
    };
    let losses = result
        .report()
        .losses
        .iter()
        .map(|loss| loss.message.clone())
        .collect();
    (kind.clone(), losses)
}

fn range(limits: Option<&JointLimits>) -> (f64, f64) {
    let Some(JointLimits::Range(range)) = limits else {
        panic!("a range: {limits:?}");
    };
    (range.minimum().get(), range.maximum().get())
}

/// A generic joint that locks every axis but `rotZ` and `transZ` is a
/// cylindrical joint along its Z axis: `rotZ` from -30 to 60 degrees is -π/6
/// to π/3, and `transZ` from 0 to 2 cm is 0 to 20 mm. Its limits are read,
/// and no degree of freedom is reported lost.
#[test]
fn a_generic_joint_free_along_and_about_z_is_a_cylindrical_joint() {
    let mut limits = LOCKED.to_vec();
    limits.extend([("rotZ", -30.0, 60.0), ("transZ", 0.0, 2.0)]);
    let (kind, losses) = joint_kind(&generic_joint(&limits));
    let PairedJointKind::Cylindrical {
        angular_limits,
        linear_limits,
        ..
    } = kind
    else {
        panic!("a cylindrical joint: {kind:?}");
    };
    let (low, high) = range(angular_limits.as_ref());
    assert!(close(low, -std::f64::consts::PI / 6.0) && close(high, std::f64::consts::PI / 3.0));
    let (low, high) = range(linear_limits.as_ref());
    assert!(close(low, 0.0) && close(high, 20.0));
    assert!(
        losses
            .iter()
            .all(|loss| !loss.contains("degrees of freedom") && !loss.contains("limit:")),
        "{losses:#?}"
    );
}

/// Locking `transZ` as well leaves a revolute joint about Z, and locking
/// `rotZ` a slider along Z; a free axis with no limit has none.
#[test]
fn a_generic_joint_free_about_or_along_z_is_a_revolute_or_slider_joint() {
    let mut limits = LOCKED.to_vec();
    limits.extend([("rotZ", -90.0, 90.0), ("transZ", 1.0, -1.0)]);
    let (kind, _) = joint_kind(&generic_joint(&limits));
    let PairedJointKind::Revolute { angular_limits, .. } = kind else {
        panic!("a revolute joint: {kind:?}");
    };
    let (low, high) = range(angular_limits.as_ref());
    assert!(close(low, -std::f64::consts::FRAC_PI_2) && close(high, std::f64::consts::FRAC_PI_2));
    let mut limits = LOCKED.to_vec();
    limits.push(("rotZ", 1.0, -1.0));
    let (kind, _) = joint_kind(&generic_joint(&limits));
    assert!(
        matches!(
            kind,
            PairedJointKind::Slider {
                linear_limits: None,
                ..
            }
        ),
        "{kind:?}"
    );
}

/// A generic joint that leaves another axis free stays a native joint: its
/// degrees of freedom and the limit it keeps no record of are reported lost.
#[test]
fn another_generic_joint_stays_native_and_reports_its_freedoms() {
    let limits = [
        ("transX", 1.0, -1.0),
        ("transY", 0.0, 5.0),
        ("rotX", 1.0, -1.0),
        ("rotY", 1.0, -1.0),
    ];
    let (kind, losses) = joint_kind(&generic_joint(&limits));
    assert!(matches!(kind, PairedJointKind::Native { .. }), "{kind:?}");
    for lost in [
        "degrees of freedom",
        "limit:transY:physics:low",
        "limit:transY:physics:high",
    ] {
        assert!(
            losses.iter().any(|loss| loss.contains(lost)),
            "{lost}: {losses:#?}"
        );
    }
}

/// A distance limit has no place on a cylindrical joint: the joint stays
/// native and the distance limit is reported lost.
#[test]
fn a_generic_joint_with_a_distance_limit_stays_native() {
    let mut limits = LOCKED.to_vec();
    limits.push(("distance", 0.0, 5.0));
    let (kind, losses) = joint_kind(&generic_joint(&limits));
    assert!(matches!(kind, PairedJointKind::Native { .. }), "{kind:?}");
    for lost in ["limit:distance:physics:low", "limit:distance:physics:high"] {
        assert!(
            losses.iter().any(|loss| loss.contains(lost)),
            "{lost}: {losses:#?}"
        );
    }
}

/// A cylindrical joint written as a generic joint reads back as a cylindrical
/// joint with its limits: -π/6 to π/3 and 0 to 20 mm.
#[test]
fn a_cylindrical_joint_survives_a_write() {
    let mut limits = LOCKED.to_vec();
    limits.extend([("rotZ", -30.0, 60.0), ("transZ", 0.0, 2.0)]);
    let first = decode(&generic_joint(&limits));
    let (text, _) = encode(first.ir());
    assert!(text.contains("def PhysicsJoint"), "{text}");
    let (kind, _) = joint_kind(&text);
    let PairedJointKind::Cylindrical {
        angular_limits,
        linear_limits,
        ..
    } = kind
    else {
        panic!("a cylindrical joint: {kind:?}");
    };
    let (low, high) = range(angular_limits.as_ref());
    assert!(close(low, -std::f64::consts::PI / 6.0) && close(high, std::f64::consts::PI / 3.0));
    let (low, high) = range(linear_limits.as_ref());
    assert!(close(low, 0.0) && close(high, 20.0));
}

/// An infinite bound still participates in the lock test (`low > high`):
/// `transZ` at `(0, -inf)` locks translation along Z, leaving a revolute
/// joint about Z with no `limit:transZ` loss.
#[test]
fn an_infinite_lock_bound_locks_the_axis_as_a_revolute_joint() {
    let mut limits = LOCKED.to_vec();
    limits.push(("transZ", 0.0, f64::NEG_INFINITY));
    let (kind, losses) = joint_kind(&generic_joint(&limits));
    assert!(
        matches!(
            kind,
            PairedJointKind::Revolute {
                angular_limits: None,
                ..
            }
        ),
        "{kind:?}"
    );
    assert!(
        losses.iter().all(|loss| !loss.contains("limit:transZ")),
        "{losses:#?}"
    );
}

const MESH_RIG: &str = "#usda 1.0\ndef Xform \"Rig\"\n{\n    def Mesh \"Base\" (\n        prepend apiSchemas = [\"PhysicsRigidBodyAPI\"]\n    )\n    {\n        int[] faceVertexCounts = [3]\n        int[] faceVertexIndices = [0, 1, 2]\n        point3f[] points = [(0, 0, 0), (1, 0, 0), (0, 1, 0)]\n    }\n    def Mesh \"Arm\" (\n        prepend apiSchemas = [\"PhysicsRigidBodyAPI\"]\n    )\n    {\n        int[] faceVertexCounts = [3]\n        int[] faceVertexIndices = [0, 1, 2]\n        point3f[] points = [(0, 0, 2), (1, 0, 2), (0, 1, 2)]\n    }\n    def PhysicsRevoluteJoint \"Hinge\"\n    {\n        rel physics:body0 = </Rig/Base>\n        rel physics:body1 = </Rig/Arm>\n    }\n}\n";

/// The `export:value.unwritable` notes of `report`.
fn unwritable(report: &cadmpeg_ir::report::export::ExportReport) -> Vec<&str> {
    report
        .losses
        .iter()
        .filter(|loss| loss.code.to_string() == "export/value.unwritable")
        .map(|loss| loss.message.as_str())
        .collect()
}

/// A joint whose body prim is left out for a point past the binary32 range
/// is left out with it: no written relationship names a prim the layer
/// does not hold.
#[test]
fn a_joint_to_a_prim_left_out_is_left_out_with_it() {
    let (mut ir, _, _) = decode(MESH_RIG).into_parts();
    let (written, _) = encode(&ir);
    let target = written
        .lines()
        .find_map(|line| line.trim().strip_prefix("rel physics:body1 = <"))
        .and_then(|rest| rest.strip_suffix('>'))
        .expect("the joint names its second body")
        .to_owned();
    let arm = ir
        .model
        .tessellations
        .iter()
        .position(|tessellation| tessellation.id.as_str().contains("Arm"))
        .expect("the arm mesh");
    ir.model.tessellations[arm]
        .edit_vertices(|point| {
            point.x = 1e300;
            Ok(())
        })
        .expect("a finite point");
    let (text, report) = encode(&ir);
    assert!(!text.contains("def Mesh \"Arm\""), "{text}");
    assert!(!text.contains(&format!("<{target}")), "{text}");
    assert!(!text.contains("PhysicsRevoluteJoint"), "{text}");
    let notes = unwritable(&report);
    assert!(
        notes.iter().any(|note| note.contains(&target))
            && notes.iter().any(|note| note.contains("Hinge")),
        "{:#?}",
        report.losses
    );
    let read = decode(&text);
    assert!(read.ir().model.assembly_joints.is_empty());
    assert_eq!(read.ir().model.rigid_bodies.len(), 1);
}

/// A mass past the binary32 range is left out; its body stays and takes
/// the fallback mass.
#[test]
fn a_mass_past_the_binary32_range_is_left_out_and_its_body_stays() {
    let (mut ir, _, _) = decode(LAYER).into_parts();
    ir.model.mass_properties[0].mass = cadmpeg_ir::scalar::PositiveReal::new(1e39);
    let (text, report) = encode(&ir);
    assert!(!text.contains("physics:mass"), "{text}");
    let notes = unwritable(&report);
    assert!(
        notes.len() == 1 && notes[0].contains("/A/Base.physics:mass"),
        "{:#?}",
        report.losses
    );
    let read = decode(&text);
    assert_eq!(read.ir().model.rigid_bodies.len(), 2);
    assert_eq!(read.ir().model.assembly_joints.len(), 1);
    assert!(read
        .ir()
        .model
        .mass_properties
        .iter()
        .all(|mass| mass.mass.is_none()));
}
