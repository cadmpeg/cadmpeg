// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use cadmpeg_ir::ids::{OccurrenceId, SolverSettingsId};
use cadmpeg_ir::physics::solver::{Dimension, SolverSettings, SolverTarget, SolverValue};
use cadmpeg_ir::products::JointId;
use cadmpeg_ir::scalar::FiniteReal;

use super::{lines, placeholder, resolve};

fn joint_settings(values: Vec<(&str, SolverValue)>) -> SolverSettings {
    SolverSettings {
        id: SolverSettingsId::mint("test:model:solver#0").unwrap(),
        solver: "physx".to_owned(),
        schema: "PhysxJointAPI".to_owned(),
        target: SolverTarget::Joint {
            joint: JointId::mint("test:model:joint#0").unwrap(),
        },
        values: values
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect::<BTreeMap<_, _>>(),
        native_ref: None,
    }
}

#[test]
fn values_return_to_scene_units_by_their_dimension() {
    // Kilogram square millimetres to kilogram square metres; radians per
    // second to degrees per second.
    let settings = joint_settings(vec![
        (
            "physxJoint:armature",
            SolverValue::Real {
                value: FiniteReal::new(2.0e6).unwrap(),
                dimension: Dimension::new(2, 1, 0),
            },
        ),
        (
            "physxJoint:maxJointVelocity",
            SolverValue::Real {
                value: FiniteReal::new(std::f64::consts::PI).unwrap(),
                dimension: Dimension::new(0, 0, 1),
            },
        ),
    ]);
    let (schema, written, unwritten) = lines(&settings);
    assert_eq!(schema, "prepend apiSchemas = [\"PhysxJointAPI\"]");
    assert_eq!(unwritten, 0);
    assert!(
        written.contains(&"float physxJoint:armature = 2".to_owned()),
        "{written:?}"
    );
    assert!(
        written.contains(&"float physxJoint:maxJointVelocity = 180".to_owned()),
        "{written:?}"
    );
}

#[test]
fn a_value_with_no_attribute_or_unit_is_not_written() {
    let settings = joint_settings(vec![
        ("physxJoint:unknown", SolverValue::Bool { value: true }),
        (
            "physxJoint:armature",
            SolverValue::Real {
                value: FiniteReal::new(1.0).unwrap(),
                dimension: Dimension::new(3, 0, 0),
            },
        ),
    ]);
    let (_, written, unwritten) = lines(&settings);
    assert!(written.is_empty());
    assert_eq!(unwritten, 2);
}

#[test]
fn placeholders_resolve_to_paths_and_unresolved_lines_are_dropped() {
    let occurrence = SolverTarget::Occurrence {
        occurrence: OccurrenceId::mint("test:model:occurrence#0").unwrap(),
    };
    let joint = SolverTarget::Joint {
        joint: JointId::mint("test:model:joint#0").unwrap(),
    };
    let text = format!(
        "rel a = [<{}>]\nrel b = [<{}>]\n",
        placeholder(&occurrence).unwrap(),
        placeholder(&joint).unwrap()
    );
    let paths = BTreeMap::from([(placeholder(&occurrence).unwrap(), "/World/Arm".to_owned())]);
    let (resolved, unresolved) = resolve(&text, &paths);
    assert_eq!(resolved, "rel a = [</World/Arm>]\n");
    assert_eq!(unresolved, 1);
}
