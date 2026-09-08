// SPDX-License-Identifier: Apache-2.0
#![allow(
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]

use super::prelude::*;
use crate::records::feature::DesignPathFeatureConstruction;
use crate::records::topology::DesignOperandRole;

#[test]
fn legacy_pipe_projects_only_the_exact_path_reference_form() {
    use crate::records::topology::DesignConstructionOperandGroupFrame;

    use cadmpeg_ir::features::{FeatureDefinition, GeneratedSweepSection, PathRef, SweepSection};

    let mut scope = DesignParameterScope::empty(
        "f3d:test:pipe-scope#1",
        crate::records::feature::DesignFeatureKind::Pipe,
        1,
    );
    scope.class_tag = crate::records::DesignClassTag::try_from("405".to_owned()).unwrap();
    scope.paired_class_tag = crate::records::DesignClassTag::try_from("259".to_owned()).unwrap();
    scope.reference_members =
        crate::records::ReferenceRun::unlocated(vec![10, 11, 12, 13, 20, 21, 22]);
    {
        let value = Some(DesignPathFeatureConstruction::Pipe(
            crate::records::feature::DesignPipeConstruction {
                operation: DesignExtrudeOperation::NewBody,
                operation_offset: 26,
                section_shape: crate::records::feature::DesignPipeSectionShape::Circular,
                section_shape_offset: 30,
                filled: true,
                filled_offset: 31,
                values: [1.0, 1.0, 0.6, 0.15],
                record_indexes: [10, 11, 12, 13],
                value_offsets: [40, 151, 262, 373],
            },
        ));
        scope.payload = value.map_or_else(|| scope.kind().into(), Into::into);
    }

    let parameter = |record_index: u32,
                     source_kind: &str,
                     unit: Option<&str>,
                     evaluated_value: f64| DesignParameter {
        id: format!("f3d:test:pipe-parameter#{record_index}"),
        byte_offset: 0,
        class_tag: crate::records::DesignClassTag::try_from("277".to_owned()).unwrap(),
        record_index,
        source_ordinal: record_index,
        source: crate::records::DesignParameterSource::new(source_kind.into(), Some(0), None)
            .unwrap(),
        expression: String::new(),
        expression_offset: 0,
        source_kind_offset: 0,

        unit: unit.map(|value| crate::records::RecordedValue {
            value: value.to_owned(),
            offset: None,
        }),
        name: source_kind.into(),
        name_offset: 0,
        evaluated_value,
        evaluated_value_offset: 0,
    };
    let parameters = [
        parameter(10, "AlongDistance", None, 1.0),
        parameter(11, "AgainstDistance", None, 1.0),
        parameter(12, "SectionSize", Some("cm"), 0.6),
        parameter(13, "SectionThickness", Some("cm"), 0.15),
    ];
    let parameter_refs = parameters
        .iter()
        .map(|parameter| (parameter.record_index, parameter))
        .collect::<Vec<_>>();
    let path_group = DesignConstructionOperandGroup {
        id: "f3d:test:pipe-group#20".into(),
        scope_record_index: 1,
        scope_reference_ordinal: 4,
        record_index: 20,
        byte_offset: 0,
        class_tag: crate::records::DesignClassTag::try_from("312".to_owned()).unwrap(),
        members: vec![crate::records::Located {
            value: 21,
            offset: 0,
        }],
        lost_edge_references: Vec::new(),
        frame: DesignConstructionOperandGroupFrame {
            member_count_offset: 0,
            auxiliary_records: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_records: Vec::new(),
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 1,
            opaque_index_offset: 0,
            opaque_scalar: 0.0,
            opaque_scalar_offset: 0,
            variant: false,
        },
        operand_role: crate::records::topology::DesignConstructionOperandRole::Other(
            DesignOperandRole::ROLE_0X5,
        ),
        role_offset: 0,
        paired_class_tag: crate::records::DesignClassTag::try_from("258".to_owned()).unwrap(),
        paired_byte_offset: 0,
    };

    let definition = crate::design::feature_project::project_fixed_pipe(
        &scope,
        &parameter_refs,
        std::slice::from_ref(&path_group),
        &[],
        &[],
    )
    .expect("exact legacy Pipe reference form");
    assert!(matches!(
        definition,
        FeatureDefinition::Sweep {
            section: SweepSection::Generated(GeneratedSweepSection::CircularRegion {
                outer_radius: actual_outer_radius,
                wall_thickness: None,
            }),
            path: Some(PathRef::Native(path)),
            ..
        } if (path == path_group.id) && actual_outer_radius.get() == 3.0
    ));

    {
        let value = Some(DesignPathFeatureConstruction::Pipe(
            crate::records::feature::DesignPipeConstruction {
                operation: DesignExtrudeOperation::NewBody,
                operation_offset: 26,
                section_shape: crate::records::feature::DesignPipeSectionShape::Circular,
                section_shape_offset: 30,
                filled: false,
                filled_offset: 31,
                values: [1.0, 1.0, 0.6, 0.15],
                record_indexes: [10, 11, 12, 13],
                value_offsets: [40, 151, 262, 373],
            },
        ));
        scope.payload = value.map_or_else(|| scope.kind().into(), Into::into);
    }
    let hollow_definition = crate::design::feature_project::project_fixed_pipe(
        &scope,
        &parameter_refs,
        std::slice::from_ref(&path_group),
        &[],
        &[],
    )
    .expect("exact hollow circular Pipe reference form");
    assert!(matches!(
        hollow_definition,
        FeatureDefinition::Sweep {
            section: SweepSection::Generated(GeneratedSweepSection::CircularRegion {
                outer_radius: actual_outer_radius,
                wall_thickness: Some(actual_wall_thickness),
            }),
            path: Some(PathRef::Native(path)),
            ..
        } if (path == path_group.id) && actual_outer_radius.get() == 3.0 && actual_wall_thickness.get() == 1.5
    ));

    let mut too_thick_parameters = parameters.clone();
    too_thick_parameters[3].evaluated_value = 0.35;
    let too_thick_parameter_refs = too_thick_parameters
        .iter()
        .map(|parameter| (parameter.record_index, parameter))
        .collect::<Vec<_>>();
    {
        let value = Some(DesignPathFeatureConstruction::Pipe(
            crate::records::feature::DesignPipeConstruction {
                operation: DesignExtrudeOperation::NewBody,
                operation_offset: 26,
                section_shape: crate::records::feature::DesignPipeSectionShape::Circular,
                section_shape_offset: 30,
                filled: false,
                filled_offset: 31,
                values: [1.0, 1.0, 0.6, 0.35],
                record_indexes: [10, 11, 12, 13],
                value_offsets: [40, 151, 262, 373],
            },
        ));
        scope.payload = value.map_or_else(|| scope.kind().into(), Into::into);
    }
    assert!(crate::design::feature_project::project_fixed_pipe(
        &scope,
        &too_thick_parameter_refs,
        std::slice::from_ref(&path_group),
        &[],
        &[],
    )
    .is_none());

    {
        let value = Some(DesignPathFeatureConstruction::Pipe(
            crate::records::feature::DesignPipeConstruction {
                operation: DesignExtrudeOperation::NewBody,
                operation_offset: 26,
                section_shape: crate::records::feature::DesignPipeSectionShape::Circular,
                section_shape_offset: 30,
                filled: true,
                filled_offset: 31,
                values: [1.0, 1.0, 0.6, 0.15],
                record_indexes: [10, 11, 12, 13],
                value_offsets: [40, 151, 262, 373],
            },
        ));
        scope.payload = value.map_or_else(|| scope.kind().into(), Into::into);
    }

    scope.reference_members = {
        let mut values: Vec<u32> = scope.reference_members.values().copied().collect();
        values.push(23);
        crate::records::ReferenceRun::unlocated(values)
    };
    assert!(crate::design::feature_project::project_fixed_pipe(
        &scope,
        &parameter_refs,
        std::slice::from_ref(&path_group),
        &[],
        &[],
    )
    .is_none());

    scope.reference_members = {
        let mut values: Vec<u32> = scope.reference_members.values().copied().collect();
        values.pop();
        crate::records::ReferenceRun::unlocated(values)
    };
    scope.class_tag = crate::records::DesignClassTag::try_from("475".to_owned()).unwrap();
    scope.paired_class_tag = crate::records::DesignClassTag::try_from("260".to_owned()).unwrap();
    assert!(crate::design::feature_project::project_fixed_pipe(
        &scope,
        &parameter_refs,
        std::slice::from_ref(&path_group),
        &[],
        &[],
    )
    .is_some());

    scope.class_tag = crate::records::DesignClassTag::try_from("421".to_owned()).unwrap();
    scope.paired_class_tag = crate::records::DesignClassTag::try_from("257".to_owned()).unwrap();
    {
        let value = Some(DesignPathFeatureConstruction::Pipe(
            crate::records::feature::DesignPipeConstruction {
                operation: DesignExtrudeOperation::NewBody,
                operation_offset: 25,
                section_shape: crate::records::feature::DesignPipeSectionShape::Circular,
                section_shape_offset: 29,
                filled: true,
                filled_offset: 30,
                values: [1.0, 1.0, 0.6, 0.15],
                record_indexes: [10, 11, 12, 13],
                value_offsets: [40, 151, 262, 373],
            },
        ));
        scope.payload = value.map_or_else(|| scope.kind().into(), Into::into);
    }
    assert!(crate::design::feature_project::project_fixed_pipe(
        &scope,
        &parameter_refs,
        std::slice::from_ref(&path_group),
        &[],
        &[],
    )
    .is_some());
}
