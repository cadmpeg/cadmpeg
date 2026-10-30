// SPDX-License-Identifier: Apache-2.0

use super::{read, Dim, Freedom, Read};
use crate::schema::tests::with_prim;

#[test]
fn a_joint_attribute_takes_the_unit_of_the_joint_freedom() {
    with_prim(
        "float physxJoint:armature = 2\n    float physxJoint:maxJointVelocity = inf",
        |prim, issues| {
            let angular = read(prim, "PhysxJointAPI", Some(Freedom::Angular), issues);
            assert!(angular.contains(&(
                "physxJoint:armature".to_owned(),
                Read::Real(2.0, Dim::new(2, 1, 0, false))
            )));
            assert!(angular.contains(&(
                "physxJoint:maxJointVelocity".to_owned(),
                Read::Real(f64::INFINITY, Dim::new(0, 0, 1, true))
            )));
            let linear = read(prim, "PhysxJointAPI", Some(Freedom::Linear), issues);
            assert!(linear.contains(&(
                "physxJoint:armature".to_owned(),
                Read::Real(2.0, Dim::new(0, 1, 0, false))
            )));
        },
    );
}

#[test]
fn an_instance_name_states_the_freedom_and_names_the_attributes() {
    with_prim(
        "float physxLimit:transX:stiffness = 5\n    float physxLimit:rotY:stiffness = 7",
        |prim, issues| {
            assert_eq!(
                read(prim, "PhysxLimitAPI:transX", None, issues),
                [(
                    "physxLimit:transX:stiffness".to_owned(),
                    Read::Real(5.0, Dim::new(0, 1, 0, false))
                )]
            );
            assert_eq!(
                read(prim, "PhysxLimitAPI:rotY", None, issues),
                [(
                    "physxLimit:rotY:stiffness".to_owned(),
                    Read::Real(7.0, Dim::new(2, 1, -1, true))
                )]
            );
        },
    );
}

#[test]
fn a_mimic_gearing_is_the_follower_unit_over_the_reference_unit() {
    with_prim(
        "float physxMimicJoint:transX:gearing = 0.5\n    uniform token physxMimicJoint:transX:referenceJointAxis = \"rotZ\"",
        |prim, issues| {
            let values = read(prim, "PhysxMimicJointAPI:transX", None, issues);
            assert!(values.contains(&(
                "physxMimicJoint:transX:gearing".to_owned(),
                Read::Real(0.5, Dim::new(1, 0, -1, true))
            )));
        },
    );
}

#[test]
fn a_force_vector_takes_the_unit_of_the_force_mode() {
    with_prim(
        "vector3f physxForce:force = (1, 2, 3)\n    uniform token physxForce:mode = \"force\"",
        |prim, issues| {
            let values = read(prim, "PhysxForceAPI", None, issues);
            assert!(values.contains(&(
                "physxForce:force".to_owned(),
                Read::Reals(vec![1.0, 2.0, 3.0], Dim::new(1, 1, 0, false))
            )));
        },
    );
}
