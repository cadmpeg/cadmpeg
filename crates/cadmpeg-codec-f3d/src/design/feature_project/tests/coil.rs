// SPDX-License-Identifier: Apache-2.0
#![allow(
    unused_imports,
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]
use super::prelude::*;

use super::project_coil;
use crate::records::{
    DesignCoilExtent, DesignCoilSection, DesignCoilSectionPlacement, DesignCoilTransform,
    DesignExtrudeOperation, DesignParameter, DesignParameterKind, DesignParameterScope,
};
use cadmpeg_ir::features::{CoilPlacement, FeatureDefinition};

fn parameter(
    record_index: u32,
    source_kind: &str,
    unit: Option<&str>,
    value: f64,
) -> DesignParameter {
    DesignParameter {
        id: format!("f3d:Design/BulkStream.dat:parameter#{record_index}"),
        byte_offset: 0,
        class_tag: "000".into(),
        record_index,
        family_discriminator: None,
        family_discriminator_offset: None,
        source_ordinal: 0,
        owner: crate::records::DesignParameterOwnerKind::from_kind(
            DesignParameterKind::Feature,
            None,
        ),
        expression: value.to_string(),
        expression_offset: 0,
        source_kind: source_kind.into(),
        source_kind_offset: 0,

        unit: unit.map(str::to_owned),
        unit_offset: None,
        name: source_kind.into(),
        name_offset: 0,
        evaluated_value: value,
        evaluated_value_offset: 0,
    }
}

#[test]
fn long_coil_matrix_projects_as_explicit_placement() {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#40",
        crate::records::DesignFeatureKind::CoilPrimitive,
        40,
    );
    scope.ensure_coil().coil_operation = Some(DesignExtrudeOperation::NewBody);
    scope.ensure_coil().coil_extent = Some(DesignCoilExtent::RevolutionsHeight);
    scope.ensure_coil().coil_section = Some(DesignCoilSection::Circular);
    scope.ensure_coil().coil_section_placement = Some(DesignCoilSectionPlacement::Inside);
    scope.ensure_coil().coil_clockwise = Some(false);
    scope.ensure_coil().coil_transform = Some(DesignCoilTransform {
        transform: [
            [1.0, 0.0, 0.0, 1.25],
            [0.0, 1.0, 0.0, -2.5],
            [0.0, 0.0, 1.0, 3.75],
            [0.0, 0.0, 0.0, 1.0],
        ],
        transform_offset: 77,
    });
    let parameters = [
        parameter(1, "Diameter", Some("cm"), 2.0),
        parameter(2, "SectionSize", Some("cm"), 0.2),
        parameter(3, "TaperAngle", Some("rad"), 0.0),
        parameter(4, "Revolutions", None, 3.0),
        parameter(5, "Height", Some("cm"), 1.5),
    ];
    let owned = parameters
        .iter()
        .enumerate()
        .map(|(ordinal, parameter)| (ordinal as u32, parameter))
        .collect::<Vec<_>>();

    let FeatureDefinition::Coil { construction, .. } =
        project_coil(&scope, &owned, &[]).expect("typed long Coil")
    else {
        panic!("expected Coil definition")
    };
    assert_eq!(
        construction.placement,
        CoilPlacement::Explicit {
            origin: cadmpeg_ir::math::Point3::new(12.5, -25.0, 37.5),
            axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            radial: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        }
    );
}
