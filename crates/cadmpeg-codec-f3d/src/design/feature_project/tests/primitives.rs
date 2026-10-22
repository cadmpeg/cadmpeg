// SPDX-License-Identifier: Apache-2.0

use crate::design::feature_project::project_solid_primitive;
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::primitives::DesignCylinderPrimitive;
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
};
use crate::records::identity::Located;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

fn cylinder(transform: Option<[[f64; 4]; 4]>) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#40",
        DesignFeatureKind::CylinderPrimitive,
        40,
    );
    if let DesignScopePayloadMut::CylinderPrimitive(slot) = scope.payload_mut() {
        *slot = Some(DesignCylinderPrimitive {
            height: cadmpeg_ir::scalar::PositiveReal::new(2.0).unwrap(),
            height_record_index: 41,
            height_offset: 0,
            diameter: cadmpeg_ir::scalar::PositiveReal::new(1.0).unwrap(),
            diameter_record_index: 42,
            diameter_offset: 0,
            transform: transform.map(|rows| Located {
                value: rows.try_into().unwrap(),
                offset: 72,
            }),
            operation: DesignExtrudeOperation::NewBody,
            operation_offset: 22,
        });
    }
    scope
}

/// The shifted cylinder's frame is the primitive's placement, with its
/// translation in millimetres; a cylinder without a frame has none.
#[test]
fn shifted_cylinder_frame_places_the_primitive() {
    // Axis along model -y (frame z = -y), base centre at (1, 2, 3) cm.
    let frame = [
        [1.0, 0.0, 0.0, 1.0],
        [0.0, 0.0, -1.0, 2.0],
        [0.0, 1.0, 0.0, 3.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let Some(FeatureDefinition::Operation(FeatureOperation::Primitive { placement, .. })) =
        project_solid_primitive(&cylinder(Some(frame)), &[])
    else {
        unreachable!("cylinder primitive");
    };
    let rows = placement.expect("shifted cylinder placement").rows();
    assert_eq!(rows[0], [1.0, 0.0, 0.0, 10.0]);
    assert_eq!(rows[1], [0.0, 0.0, -1.0, 20.0]);
    assert_eq!(rows[2], [0.0, 1.0, 0.0, 30.0]);

    assert!(matches!(
        project_solid_primitive(&cylinder(None), &[]),
        Some(FeatureDefinition::Operation(FeatureOperation::Primitive {
            placement: None,
            ..
        }))
    ));
}
