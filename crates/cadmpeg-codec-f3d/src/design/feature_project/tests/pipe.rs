// SPDX-License-Identifier: Apache-2.0
#![allow(
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]

use super::prelude::*;

#[test]
fn legacy_pipe_projects_only_the_exact_path_reference_form() {
    use crate::records::{DesignConstructionOperandGroupFrame, DesignParameterKind};
    use cadmpeg_ir::features::{
        FeatureDefinition, GeneratedSweepSection, Length, PathRef, SweepSection,
    };

    let mut scope = DesignParameterScope::empty(
        "f3d:test:pipe-scope#1",
        crate::records::DesignFeatureKind::Pipe,
        1,
    );
    scope.class_tag = "405".into();
    scope.paired_class_tag = "259".into();
    scope.reference_members = vec![10, 11, 12, 13, 20, 21, 22];
    {
        let value = Some(DesignPathFeatureConstruction::Pipe {
            operation: DesignExtrudeOperation::NewBody,
            operation_offset: 26,
            section_shape: crate::records::DesignPipeSectionShape::Circular,
            section_shape_offset: 30,
            filled: true,
            filled_offset: 31,
            values: [1.0, 1.0, 0.6, 0.15],
            record_indexes: [10, 11, 12, 13],
            value_offsets: [40, 151, 262, 373],
        });
        if let crate::records::DesignScopePayload::Loft(slot)
        | crate::records::DesignScopePayload::Sweep(slot)
        | crate::records::DesignScopePayload::Revolve(slot)
        | crate::records::DesignScopePayload::Pipe(slot) = &mut scope.payload
        {
            slot.get_or_insert_with(Default::default)
                .path_feature_construction = value;
        }
    }

    let parameter = |record_index: u32,
                     source_kind: &str,
                     unit: Option<&str>,
                     evaluated_value: f64| DesignParameter {
        id: format!("f3d:test:pipe-parameter#{record_index}"),
        byte_offset: 0,
        class_tag: "277".into(),
        record_index,
        family_discriminator: None,
        family_discriminator_offset: None,
        source_ordinal: record_index,
        owner: crate::records::DesignParameterOwnerKind::from_kind(
            DesignParameterKind::Feature,
            None,
        ),
        expression: String::new(),
        expression_offset: 0,
        source_kind: source_kind.into(),
        source_kind_offset: 0,

        unit: unit.map(str::to_owned),
        unit_offset: None,
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
        class_tag: "312".into(),
        members: vec![21],
        lost_edge_references: Vec::new(),
        member_offsets: vec![0],
        frame: DesignConstructionOperandGroupFrame {
            member_count_offset: 0,
            auxiliary_record_indices: Vec::new(),
            auxiliary_record_offsets: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_record_indices: Vec::new(),
            trailing_record_offsets: Vec::new(),
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 1,
            opaque_index_offset: 0,
            opaque_scalar: 0.0,
            opaque_scalar_offset: 0,
            variant: false,
        },
        role: 0x0000_0005_0000_0000,
        extrude_role: None,
        role_offset: 0,
        paired_class_tag: "258".into(),
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
                outer_radius: Length(3.0),
                wall_thickness: None,
            }),
            path: Some(PathRef::Native(path)),
            ..
        } if path == path_group.id
    ));

    {
        let value = Some(DesignPathFeatureConstruction::Pipe {
            operation: DesignExtrudeOperation::NewBody,
            operation_offset: 26,
            section_shape: crate::records::DesignPipeSectionShape::Circular,
            section_shape_offset: 30,
            filled: false,
            filled_offset: 31,
            values: [1.0, 1.0, 0.6, 0.15],
            record_indexes: [10, 11, 12, 13],
            value_offsets: [40, 151, 262, 373],
        });
        if let crate::records::DesignScopePayload::Loft(slot)
        | crate::records::DesignScopePayload::Sweep(slot)
        | crate::records::DesignScopePayload::Revolve(slot)
        | crate::records::DesignScopePayload::Pipe(slot) = &mut scope.payload
        {
            slot.get_or_insert_with(Default::default)
                .path_feature_construction = value;
        }
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
                outer_radius: Length(3.0),
                wall_thickness: Some(Length(1.5)),
            }),
            path: Some(PathRef::Native(path)),
            ..
        } if path == path_group.id
    ));

    let mut too_thick_parameters = parameters.clone();
    too_thick_parameters[3].evaluated_value = 0.35;
    let too_thick_parameter_refs = too_thick_parameters
        .iter()
        .map(|parameter| (parameter.record_index, parameter))
        .collect::<Vec<_>>();
    {
        let value = Some(DesignPathFeatureConstruction::Pipe {
            operation: DesignExtrudeOperation::NewBody,
            operation_offset: 26,
            section_shape: crate::records::DesignPipeSectionShape::Circular,
            section_shape_offset: 30,
            filled: false,
            filled_offset: 31,
            values: [1.0, 1.0, 0.6, 0.35],
            record_indexes: [10, 11, 12, 13],
            value_offsets: [40, 151, 262, 373],
        });
        if let crate::records::DesignScopePayload::Loft(slot)
        | crate::records::DesignScopePayload::Sweep(slot)
        | crate::records::DesignScopePayload::Revolve(slot)
        | crate::records::DesignScopePayload::Pipe(slot) = &mut scope.payload
        {
            slot.get_or_insert_with(Default::default)
                .path_feature_construction = value;
        }
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
        let value = Some(DesignPathFeatureConstruction::Pipe {
            operation: DesignExtrudeOperation::NewBody,
            operation_offset: 26,
            section_shape: crate::records::DesignPipeSectionShape::Circular,
            section_shape_offset: 30,
            filled: true,
            filled_offset: 31,
            values: [1.0, 1.0, 0.6, 0.15],
            record_indexes: [10, 11, 12, 13],
            value_offsets: [40, 151, 262, 373],
        });
        if let crate::records::DesignScopePayload::Loft(slot)
        | crate::records::DesignScopePayload::Sweep(slot)
        | crate::records::DesignScopePayload::Revolve(slot)
        | crate::records::DesignScopePayload::Pipe(slot) = &mut scope.payload
        {
            slot.get_or_insert_with(Default::default)
                .path_feature_construction = value;
        }
    }

    scope.reference_members.push(23);
    assert!(crate::design::feature_project::project_fixed_pipe(
        &scope,
        &parameter_refs,
        std::slice::from_ref(&path_group),
        &[],
        &[],
    )
    .is_none());

    scope.reference_members.pop();
    scope.class_tag = "475".into();
    scope.paired_class_tag = "260".into();
    assert!(crate::design::feature_project::project_fixed_pipe(
        &scope,
        &parameter_refs,
        std::slice::from_ref(&path_group),
        &[],
        &[],
    )
    .is_some());

    scope.class_tag = "421".into();
    scope.paired_class_tag = "257".into();
    {
        let value = Some(DesignPathFeatureConstruction::Pipe {
            operation: DesignExtrudeOperation::NewBody,
            operation_offset: 25,
            section_shape: crate::records::DesignPipeSectionShape::Circular,
            section_shape_offset: 29,
            filled: true,
            filled_offset: 30,
            values: [1.0, 1.0, 0.6, 0.15],
            record_indexes: [10, 11, 12, 13],
            value_offsets: [40, 151, 262, 373],
        });
        if let crate::records::DesignScopePayload::Loft(slot)
        | crate::records::DesignScopePayload::Sweep(slot)
        | crate::records::DesignScopePayload::Revolve(slot)
        | crate::records::DesignScopePayload::Pipe(slot) = &mut scope.payload
        {
            slot.get_or_insert_with(Default::default)
                .path_feature_construction = value;
        }
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
