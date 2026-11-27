// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::design::construction::{kinds, Construction, FrameValue};
use crate::design::container::Container;
use crate::document::CadIr;
use crate::features::geometry::FinitePoint3;
use crate::math::{Point3, Vector3};
use crate::units::UnitVector3;
use crate::validate::design::fixture::{design_findings, document};

const IDENTITY: &str = "design.placement-no-inputs-no-bound-step";

/// The fixture with its timeline placed at `z` on the z axis by a
/// placement with no inputs and no steps.
fn placed_at(z: f64) -> CadIr {
    let mut ir = document();
    let Container::Ordered { placement, .. } = &mut ir.design.containers[0] else {
        panic!("{:?}", ir.design.containers);
    };
    let axis = |x: f64, y: f64, z: f64| UnitVector3::new(Vector3::new(x, y, z)).unwrap();
    *placement = Some(Box::new(Construction::<kinds::Frame> {
        inputs: Vec::new(),
        after: Vec::new(),
        solution: None,
        mode: None,
        resolved: Some(FrameValue {
            origin: FinitePoint3::new(Point3::new(0.0, 0.0, z)).unwrap(),
            x_axis: axis(1.0, 0.0, 0.0),
            y_axis: axis(0.0, 1.0, 0.0),
            z_axis: axis(0.0, 0.0, 1.0),
        }),
        resolved_in: BTreeMap::new(),
    }));
    ir
}

fn identity_findings(ir: &CadIr) -> Vec<String> {
    design_findings(ir)
        .into_iter()
        .filter(|finding| finding.rule.as_str() == IDENTITY)
        .filter_map(|finding| finding.entity)
        .collect()
}

#[test]
fn an_ordered_container_placement_at_the_identity_is_absent() {
    assert_eq!(
        identity_findings(&placed_at(0.0)),
        ["test:design:container#timeline"]
    );
    assert!(identity_findings(&placed_at(5.0)).is_empty());
}
