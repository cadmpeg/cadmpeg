// SPDX-License-Identifier: Apache-2.0
#![allow(
    unused_imports,
    clippy::default_trait_access,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]

use super::prelude::*;

const EPS_SURFACE_DISTANCE_MM: f64 = 1.0e-12;

#[test]
fn dispatcher_projects_perpendicular_surface_extend() {
    let mut scope = DesignParameterScope::empty(
        "f3d:native:parameter-scope#surface-extend",
        "SurfaceExtend",
        12,
    );
    scope.set_surface_extend_operation(Some(DesignSurfaceExtendOperation {
        distance: 0.04,
        distance_offset: 40,
        distance_record_index: 400,
        method: DesignSurfaceExtendMethod::Perpendicular,
        method_offset: 102,
        boundary_record_index: 500,
        boundary_reference_record_index: 900,
        boundary_reference_offset: 106,
        edge_record_indices: vec![503, 507],
        tolerance: f64::EPSILON,
        tolerance_offset: 139,
    }));
    let (features, _) = project_parameter_design(&[], &[], &[scope], &[], &[], &[], &[], &[]);

    let [Feature {
        definition:
            FeatureDefinition::ExtendSurface {
                faces: FaceSelection::Native(native),
                distance: Some(Length(distance)),
                method: cadmpeg_ir::features::SurfaceExtension::Perpendicular,
            },
        ..
    }] = features.as_slice()
    else {
        panic!("perpendicular SurfaceExtend did not project as a typed feature");
    };
    assert!(native.ends_with(":design-record#500"));
    assert!((*distance - 0.4).abs() < EPS_SURFACE_DISTANCE_MM);
}
