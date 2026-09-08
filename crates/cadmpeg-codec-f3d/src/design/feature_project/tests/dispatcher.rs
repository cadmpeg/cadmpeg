// SPDX-License-Identifier: Apache-2.0
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]
use super::prelude::*;
use crate::records::topology::DesignOperandRole;

#[test]
fn dispatcher_projects_datum_feature_scopes() {
    let mut transform = identity_matrix();
    transform[0][3] = 1.0;
    transform[1][3] = 2.0;
    transform[2][3] = 3.0;

    let mut joint_origin = DesignParameterScope::empty(
        "f3d:native:parameter-scope#1",
        crate::records::feature::DesignFeatureKind::JointOrigin,
        1,
    );
    joint_origin.with_joint_origin_transform(transform);

    let mut work_plane = DesignParameterScope::empty(
        "f3d:native:parameter-scope#2",
        crate::records::feature::DesignFeatureKind::WorkPlane,
        2,
    );
    work_plane.with_work_plane_transform(transform);

    let mut work_point = DesignParameterScope::empty(
        "f3d:native:parameter-scope#3",
        crate::records::feature::DesignFeatureKind::WorkPoint,
        3,
    );
    if let crate::records::feature::DesignScopePayload::WorkPoint(slot) = &mut work_point.payload {
        *slot = Some(crate::records::feature::DesignWorkPointConstruction {
            point_record_index: 4,
            point_record_byte_offset: 0,
            position: [4.0, 5.0, 6.0],
            position_offset: 0,
            rule: crate::records::feature::DesignWorkPointRule::try_from(
                crate::records::feature::DesignWorkPointRuleForm::Native {
                    reference_type: 1,
                    inputs: Vec::new(),
                },
            )
            .expect("compatible WorkPoint rule"),
            reference_type_offset: 0,
        });
    }

    let scopes = vec![joint_origin, work_plane, work_point];
    let (features, _) = project_parameter_design(&[], &[], &scopes, &[], &[], &[], &[], &[]);

    assert!(matches!(
        &features[0].definition,
        FeatureDefinition::DatumCoordinateSystem { origin, .. }
            if *origin == Point3::new(10.0, 20.0, 30.0)
    ));
    assert!(matches!(
        &features[1].definition,
        FeatureDefinition::DatumPlane {
            origin,
            normal,
            u_axis,
        } if *origin == Point3::new(10.0, 20.0, 30.0)
            && *normal == Vector3::new(0.0, 0.0, 1.0)
            && *u_axis == Vector3::new(1.0, 0.0, 0.0)
    ));
    assert!(matches!(
        &features[2].definition,
        FeatureDefinition::DatumPoint { position, construction }
            if *position == Point3::new(40.0, 50.0, 60.0) && construction.is_none()
    ));
}

#[test]
fn dispatcher_projects_scale_point_center_in_neutral_units() {
    let mut scale = DesignParameterScope::empty(
        "f3d:native:parameter-scope#4",
        crate::records::feature::DesignFeatureKind::Scale,
        4,
    );
    if let crate::records::feature::DesignScopePayload::Scale(slot)
    | crate::records::feature::DesignScopePayload::Massstab(slot) = &mut scale.payload
    {
        *slot = Some(DesignScaleOperation {
            body_group_record_index: 5,
            center_record_index: 6,
            center_position: Some(crate::records::Located {
                value: [1.25, -2.5, 3.75],
                offset: 40,
            }),
            uniform_factor: 2.5,
            uniform_factor_offset: 20,
        });
    }

    let (features, _) = project_parameter_design(&[], &[], &[scale], &[], &[], &[], &[], &[]);
    let FeatureDefinition::Scale {
        bodies,
        center: Some(cadmpeg_ir::features::ScaleCenter::Point(center)),
        factors,
    } = &features[0].definition
    else {
        panic!("scale feature with explicit center");
    };
    assert!(matches!(
        bodies,
        cadmpeg_ir::features::BodySelection::Unresolved
    ));
    for (actual, expected) in [center.x, center.y, center.z]
        .into_iter()
        .zip([12.5, -25.0, 37.5])
    {
        assert!((actual - expected).abs() < f64::EPSILON);
    }
    assert!(matches!(
        factors,
        cadmpeg_ir::features::ScaleFactors::Uniform(uniform)
            if (*uniform - 2.5).abs() < f64::EPSILON
    ));
}

#[test]
fn dispatcher_projects_referenced_work_plane_frame() {
    let mut referenced = DesignParameterScope::empty(
        "f3d:native:parameter-scope#10",
        crate::records::feature::DesignFeatureKind::WorkPlane,
        10,
    );
    referenced.with_work_plane_transform(identity_matrix());
    referenced.with_work_plane_reference(11);

    let (features, _) = project_parameter_design(&[], &[], &[referenced], &[], &[], &[], &[], &[]);
    assert!(matches!(
        &features[0].definition,
        FeatureDefinition::DatumPlane {
            origin,
            normal,
            u_axis,
        } if *origin == Point3::new(0.0, 0.0, 0.0)
            && *normal == Vector3::new(0.0, 0.0, 1.0)
            && *u_axis == Vector3::new(1.0, 0.0, 0.0)
    ));
}

#[test]
fn dispatcher_projects_three_point_work_plane_vertices() {
    use crate::records::feature::{DesignVertexRecipe, DesignWorkPlaneConstruction};
    use cadmpeg_ir::features::VertexSelection;

    let recipe = |record_index, vertex| DesignVertexRecipe {
        record_index,
        byte_offset: u64::from(record_index),
        class_tag: crate::records::DesignClassTag::try_from("306".to_owned()).unwrap(),
        paired_byte_offset: 1,
        paired_class_tag: crate::records::DesignClassTag::try_from("261".to_owned()).unwrap(),
        recipe_record_index: record_index + 3,
        recipe_record_byte_offset: 2,
        recipe_id: format!("f3d:native:construction-recipe#{record_index}"),
        recipe_prefix_offset: 3,
        recipe_prefix_bytes: Vec::new(),
        recipe_references: Vec::new(),
        recipe_program_offset: 4,
        recipe_program: vec![0],
        resolution: Some(
            crate::records::feature::DesignVertexResolution::new(4, vertex)
                .expect("valid vertex slot"),
        ),
        next_record_index: record_index + 5,
        next_byte_offset: 5,
    };
    let mut plane = DesignParameterScope::empty(
        "f3d:native:parameter-scope#20",
        crate::records::feature::DesignFeatureKind::WorkPlane,
        20,
    );
    plane.with_work_plane_transform(identity_matrix());
    if let Some(frame) = plane.work_plane_frame_mut() {
        frame.work_plane_construction = Some(DesignWorkPlaneConstruction {
            placement_record_index: 21,
            inputs: Box::new([recipe(22, 43), recipe(27, 64), recipe(32, 84)]),
        });
    }

    let (features, _) = project_parameter_design(&[], &[], &[plane], &[], &[], &[], &[], &[]);
    let FeatureDefinition::DatumThreePointPlane { points, .. } = &features[0].definition else {
        panic!("three-point datum plane")
    };
    assert!(matches!(
        points.as_ref(),
        [
            VertexSelection::Historical { vertex: first, .. },
            VertexSelection::Historical { vertex: second, .. },
            VertexSelection::Historical { vertex: third, .. },
        ] if first.as_str().ends_with(":43") && second.as_str().ends_with(":64") && third.as_str().ends_with(":84")
    ));
}

#[test]
fn dispatcher_projects_work_point_plane_construction_and_dependencies() {
    use crate::records::feature::{
        DesignWorkPointConstruction, DesignWorkPointInput, DesignWorkPointInputCarrier,
        DesignWorkPointPlaneSelection,
    };
    use cadmpeg_ir::features::{DatumPlaneReference, DatumPointConstruction};

    let planes = [10, 20, 30].map(|record_index| {
        let id = format!("f3d:native:parameter-scope#{record_index}");
        let mut scope = DesignParameterScope::empty(
            &id,
            crate::records::feature::DesignFeatureKind::WorkPlane,
            record_index,
        );
        scope.with_work_plane_transform(identity_matrix());
        scope
    });
    let input = |record_index, work_plane_scope_record_index| DesignWorkPointInput {
        record_index,
        reference_offset: u64::from(record_index),
        carrier: Some(Box::new(DesignWorkPointInputCarrier::WorkPlane {
            selection: DesignWorkPointPlaneSelection {
                class_tag: crate::records::DesignClassTag::try_from("267".to_owned()).unwrap(),
                asset_id: crate::records::DesignRelaxedGuidText::try_from(
                    "00000000-0000-0000-0000-000000000001".to_owned(),
                )
                .unwrap(),
                asset_id_offset: 1,
                context_id: crate::records::DesignRelaxedGuidText::try_from(
                    "00000000-0000-0000-0000-000000000002".to_owned(),
                )
                .unwrap(),
                context_id_offset: 2,
                identity_record_index: record_index + 3,
                identity_record_offset: 3,
                primary_identity: u64::from(work_plane_scope_record_index - 1),
                primary_identity_offset: 24,
                work_plane_scope_record_index,
                next_record_index: record_index + 4,
                next_byte_offset: 32,
            },
        })),
    };
    let mut point = DesignParameterScope::empty(
        "f3d:native:parameter-scope#40",
        crate::records::feature::DesignFeatureKind::WorkPoint,
        40,
    );
    if let crate::records::feature::DesignScopePayload::WorkPoint(slot) = &mut point.payload {
        *slot = Some(DesignWorkPointConstruction {
            point_record_index: 41,
            point_record_byte_offset: 0,
            position: [1.0, 2.0, 3.0],
            position_offset: 0,
            rule: crate::records::feature::DesignWorkPointRule::try_from(
                crate::records::feature::DesignWorkPointRuleForm::ThreePlaneIntersection {
                    inputs: [input(42, 10), input(46, 20), input(50, 30)],
                },
            )
            .expect("compatible WorkPoint rule"),
            reference_type_offset: 0,
        });
    }
    let mut scopes = planes.to_vec();
    scopes.push(point);

    let (features, _) = project_parameter_design(&[], &[], &scopes, &[], &[], &[], &[], &[]);
    let point = features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some("f3d:native:parameter-scope#40"))
        .expect("projected work point");
    let FeatureDefinition::DatumPoint {
        construction: Some(construction),
        ..
    } = &point.definition
    else {
        panic!("typed datum-point construction");
    };
    let DatumPointConstruction::ThreePlaneIntersection { planes } = construction.as_ref() else {
        panic!("three-plane construction");
    };
    let plane_features = planes
        .iter()
        .map(|plane| match plane {
            DatumPlaneReference::Feature(feature) => feature.clone(),
            DatumPlaneReference::Face(_) | DatumPlaneReference::ResolvedPlane { .. } => {
                panic!("feature-backed plane")
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(point.dependencies, plane_features);
}

#[test]
fn dispatcher_projects_work_point_historical_vertex_and_dependency() {
    use crate::records::feature::{
        DesignVertexRecipe, DesignWorkPointConstruction, DesignWorkPointInput,
        DesignWorkPointInputCarrier,
    };
    use cadmpeg_ir::features::{DatumPointConstruction, VertexSelection};

    let mut predecessor = DesignParameterScope::empty(
        "f3d:native:parameter-scope#10",
        crate::records::feature::DesignFeatureKind::Extrude,
        10,
    );
    predecessor.history_state_id = Some(4);
    let recipe_id = "f3d:native:construction-recipe#vertex".to_string();
    let recipe = DesignVertexRecipe {
        record_index: 12,
        byte_offset: 0,
        class_tag: crate::records::DesignClassTag::try_from("369".to_owned()).unwrap(),
        paired_byte_offset: 1,
        paired_class_tag: crate::records::DesignClassTag::try_from("261".to_owned()).unwrap(),
        recipe_record_index: 23,
        recipe_record_byte_offset: 2,
        recipe_id: recipe_id.clone(),
        recipe_prefix_offset: 3,
        recipe_prefix_bytes: Vec::new(),
        recipe_references: Vec::new(),
        recipe_program_offset: 4,
        recipe_program: vec![0],
        resolution: Some(
            crate::records::feature::DesignVertexResolution::new(4, 43).expect("valid vertex slot"),
        ),
        next_record_index: 25,
        next_byte_offset: 5,
    };
    let mut point = DesignParameterScope::empty(
        "f3d:native:parameter-scope#20",
        crate::records::feature::DesignFeatureKind::WorkPoint,
        20,
    );
    if let crate::records::feature::DesignScopePayload::WorkPoint(slot) = &mut point.payload {
        *slot = Some(DesignWorkPointConstruction {
            point_record_index: 21,
            point_record_byte_offset: 0,
            position: [4.0, 3.0, 0.0],
            position_offset: 0,
            rule: crate::records::feature::DesignWorkPointRule::try_from(
                crate::records::feature::DesignWorkPointRuleForm::Vertex {
                    input: DesignWorkPointInput {
                        record_index: 22,
                        reference_offset: 0,
                        carrier: Some(Box::new(DesignWorkPointInputCarrier::VertexRecipe {
                            recipe,
                        })),
                    },
                },
            )
            .expect("compatible WorkPoint rule"),
            reference_type_offset: 0,
        });
    }
    let timeline = DesignFeatureTimeline {
        frame: crate::records::DesignTimelineFrame::test_items(
            0,
            vec![
                crate::records::Located {
                    value: 10,
                    offset: 0,
                },
                crate::records::Located {
                    value: 20,
                    offset: 0,
                },
            ],
        ),
        id: crate::ids::native_design_feature_timeline_id_in_stream("f3d:native", 0),
        class_tag: crate::records::DesignClassTag::try_from("256".to_owned()).unwrap(),
        record_index: std::num::NonZeroU64::new(1).unwrap(),
        source_ordinal: 0,
        context_record_index: std::num::NonZeroU64::new(1).unwrap(),
    };
    let scopes = vec![predecessor, point];
    let (features, _) = project_parameter_design_with_edge_identities(
        &crate::design::feature_project::ProjectInputs {
            native: &[],
            owners: &[],
            scopes: &scopes,
            timelines: std::slice::from_ref(&timeline),
            construction_groups: &[],
            fillet_radius_groups: &[],
            edge_operands: &[],
            edge_identity_operands: &[],
            edge_treatment_vertex_operands: &[],
            entity_selection_operands: &[],
            curve_identities: &[],
            face_operands: &[],
            body_recipe_operands: &[],
            legacy_loft_body_carriers: &[],
            placements: &[],
            body_bindings: &[],
            component_naming_spaces: &[],
            histories: &[],
        },
    )
    .expect("authored WorkPoint timeline");
    let predecessor = features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some(&scopes[0].id))
        .expect("projected predecessor");
    let point = features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some(&scopes[1].id))
        .expect("projected WorkPoint");
    let FeatureDefinition::DatumPoint {
        construction: Some(construction),
        ..
    } = &point.definition
    else {
        panic!("typed datum-point construction")
    };
    let DatumPointConstruction::Vertex {
        vertex:
            VertexSelection::Historical {
                state,
                vertex,
                native,
            },
    } = construction.as_ref()
    else {
        panic!("historical vertex construction")
    };
    let feature_key = point
        .id
        .as_str()
        .split_once('#')
        .map_or(point.id.as_str(), |(_, key)| key);
    let prefix = crate::ids::history_input_prefix(feature_key, 4);
    assert_eq!(
        state,
        &crate::design::edge_resolve::feature_input_topology_id(&point.id, 4)
    );
    assert_eq!(vertex, &crate::ids::history_input_vertex_id(&prefix, 43));
    assert_eq!(native, &recipe_id);
    assert_eq!(point.dependencies, [predecessor.id.clone()]);
}

#[test]
fn dispatcher_projects_remaining_operand_feature_scopes() {
    use crate::records::feature::{
        DesignBaseFeatureConstruction, DesignBaseFlangeOperation, DesignCopyPasteBodiesOperation,
        DesignCopyPasteComponentOperation,
    };
    use crate::records::topology::DesignConstructionOperandGroupFrame;
    use cadmpeg_ir::features::{BodyRetentionMode, BodySelection, SheetMetalThicknessSide};

    let stream = "f3d:native";
    let group = |scope_record_index: u32,
                 scope_reference_ordinal: u32,
                 record_index: u32,
                 members: &[u32],
                 role: DesignOperandRole| {
        DesignConstructionOperandGroup {
            id: format!("{stream}:construction-group#{record_index}"),
            scope_record_index,
            scope_reference_ordinal,
            record_index,
            byte_offset: 0,
            class_tag: crate::records::DesignClassTag::try_from("264".to_owned()).unwrap(),
            members: members
                .iter()
                .copied()
                .map(|value| crate::records::Located { value, offset: 0 })
                .collect(),
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
            operand_role: crate::records::topology::DesignConstructionOperandRole::Other(role),
            role_offset: 0,
            paired_class_tag: crate::records::DesignClassTag::try_from("264".to_owned()).unwrap(),
            paired_byte_offset: 0,
        }
    };

    let mut base_flange = DesignParameterScope::empty(
        &format!("{stream}:scope#base-flange"),
        crate::records::feature::DesignFeatureKind::BaseFlange,
        10,
    );
    {
        let value = Some(DesignBaseFlangeOperation {
            thickness: 0.2,
            thickness_offset: 0,
            profile_group_record_index: 100,
            profile_record_index: 101,
            thickness_record_index: 102,
            settings_record_index: 103,
        });
        if let crate::records::feature::DesignScopePayload::BaseFlange(slot) =
            &mut base_flange.payload
        {
            slot.get_or_insert_with(Default::default)
                .base_flange_operation = value;
        }
    }
    {
        let value = Some(DesignSketchProfileOperand {
            scope_reference_ordinal: 1,
            record_index: 101,
            byte_offset: 0,
            class_tag: crate::records::DesignClassTag::try_from("377".to_owned()).unwrap(),
            asset_id: crate::records::DesignRelaxedGuidText::try_from(
                "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
            )
            .unwrap(),
            asset_id_offset: 0,
            entity_id: crate::records::DesignEntityId::try_from("Sketch_7".to_owned())
                .expect("valid entity identity"),
            entity_reference_offset: 0,
            region_selection: None,
            paired_class_tag: crate::records::DesignClassTag::try_from("264".to_owned()).unwrap(),
            paired_byte_offset: 0,
        });
        if let crate::records::feature::DesignScopePayload::BaseFlange(slot) =
            &mut base_flange.payload
        {
            slot.get_or_insert_with(Default::default)
                .base_flange_profile = value;
        }
    }

    let mut remove_body = DesignParameterScope::empty(
        &format!("{stream}:scope#remove-body"),
        crate::records::feature::DesignFeatureKind::RemoveBody,
        20,
    );
    remove_body.reference_members = crate::records::ReferenceRun::unlocated(vec![200]);

    let mut surface_stitch = DesignParameterScope::empty(
        &format!("{stream}:scope#surface-stitch"),
        crate::records::feature::DesignFeatureKind::SurfaceStitch,
        30,
    );
    surface_stitch.reference_members =
        crate::records::ReferenceRun::unlocated(vec![300, 301, 302, 303]);
    if let crate::records::feature::DesignScopePayload::SurfaceStitch(slot) =
        &mut surface_stitch.payload
    {
        *slot = Some(DesignSurfaceStitchOperation {
            gap_tolerance: 0.01,
            gap_tolerance_offset: 0,
            tolerance_record_index: 302,
            settings_record_index: 303,
        });
    }

    let mut copy_paste = DesignParameterScope::empty(
        &format!("{stream}:scope#copy-paste"),
        crate::records::feature::DesignFeatureKind::CopyPaste,
        40,
    );
    if let crate::records::feature::DesignScopePayload::CopyPaste(slot) = &mut copy_paste.payload {
        *slot = Some(DesignCopyPasteComponentOperation {
            relation_record_index: 401,
            source_occurrence_record_index: 402,
            copied_occurrence_record_index: 403,
            component_guid: "11111111-1111-4111-8111-111111111111"
                .to_owned()
                .try_into()
                .expect("GUID"),
            source_occurrence_guid: "22222222-2222-4222-8222-222222222222"
                .to_owned()
                .try_into()
                .expect("GUID"),
            copied_occurrence_guid: "33333333-3333-4333-8333-333333333333"
                .to_owned()
                .try_into()
                .expect("GUID"),
            source_transform: identity_matrix(),
            source_transform_offset: 0,
            copied_transform: identity_matrix(),
            copied_transform_offset: 0,
        });
    }

    let mut copy_paste_bodies = DesignParameterScope::empty(
        &format!("{stream}:scope#copy-paste-bodies"),
        crate::records::feature::DesignFeatureKind::CopyPasteBodies,
        50,
    );
    if let crate::records::feature::DesignScopePayload::CopyPasteBodies(slot) =
        &mut copy_paste_bodies.payload
    {
        *slot = Some(DesignCopyPasteBodiesOperation {
            bodies: vec![crate::records::feature::DesignCopiedBody {
                operand: crate::records::Located {
                    value: 502,
                    offset: 0,
                },
                source: crate::records::Located {
                    value: 11,
                    offset: 0,
                },
                copied: crate::records::Located {
                    value: 12,
                    offset: 0,
                },
            }],
            body_group_record_index: 501,
            body_group_class_tag: crate::records::DesignClassTag::try_from("264".to_owned())
                .unwrap(),
            body_group_byte_offset: 0,
            relation_record_index: 503,
            relation_class_tag: crate::records::DesignClassTag::try_from("264".to_owned()).unwrap(),
            relation_byte_offset: 0,
        });
    }

    let mut base_feature = DesignParameterScope::empty(
        &format!("{stream}:scope#base-feature"),
        crate::records::feature::DesignFeatureKind::BaseFeature,
        60,
    );
    if let crate::records::feature::DesignScopePayload::BaseFeature(slot) =
        &mut base_feature.payload
    {
        *slot = Some(DesignBaseFeatureConstruction::ResultBodies {
            bodies: crate::records::feature::DesignBaseFeatureResults::WithoutRepeatedFields(vec![
                crate::records::feature::DesignBaseFeatureResultBody {
                    entity: crate::records::feature::DesignBaseFeatureEntry {
                        value: 21,
                        offset: 0,
                        field: [0; 6],
                    },
                    reference: crate::records::feature::DesignBaseFeatureEntry {
                        value: 601,
                        offset: 0,
                        field: [0; 6],
                    },
                    result: crate::records::feature::DesignBaseFeatureEntry {
                        value: 603,
                        offset: 0,
                        field: [0; 6],
                    },
                },
            ]),
            metadata_record: 602,
            metadata_record_offset: 0,
            metadata_field: vec![0, 0],
        });
    }

    let mut thread = DesignParameterScope::empty(
        &format!("{stream}:scope#thread"),
        crate::records::feature::DesignFeatureKind::Thread,
        70,
    );
    if let crate::records::feature::DesignScopePayload::Thread(slot) = &mut thread.payload {
        *slot = Some(DesignThreadConstruction {
            form: DesignThreadForm::Compact(None),
            designation_offset: 0,
            designation: "M3.5x0.6".into(),
            nominal_size: crate::records::feature::DesignThreadNominalSize::try_from(
                "3.5".to_owned(),
            )
            .expect("nominal size"),
            profile: "GB Metric profile".into(),
            major_diameter: 0.35995,
            minor_diameter: 0.293,
            pitch: 0.06,
            pitch_diameter: 0.3166,
            face_group_record_indices: vec![701],
        });
    }
    thread.reference_members = crate::records::ReferenceRun::unlocated(vec![701, 702]);

    let scopes = vec![
        base_flange,
        remove_body,
        surface_stitch,
        copy_paste,
        copy_paste_bodies,
        base_feature,
        thread,
    ];
    let groups = vec![
        group(10, 0, 100, &[101], DesignOperandRole::PROFILE),
        group(20, 0, 200, &[201], DesignOperandRole::BODIES_A),
        group(30, 0, 300, &[301], DesignOperandRole::ROLE_0X5),
        group(70, 0, 701, &[702], DesignOperandRole::ROLE_0X10),
    ];
    let placement = DesignSketchPlacement {
        frame: crate::records::DesignSketchFrame::new(
            0,
            crate::records::DesignSketchFrameForm::ScopeCompact,
        )
        .unwrap(),
        id: format!("{stream}:placement#7"),
        scope_record_index: None,
        entity_id: crate::records::DesignEntityId::try_from("Sketch_7".to_owned())
            .expect("valid entity ID"),

        visibility: None,

        class_tag: crate::records::DesignClassTag::try_from("264".to_owned()).unwrap(),
        record_index: 700,

        paired_class_tag: crate::records::DesignClassTag::try_from("264".to_owned()).unwrap(),
    };
    let (features, _) = project_parameter_design(
        &[],
        &[],
        &scopes,
        &groups,
        &[],
        &[],
        &[],
        std::slice::from_ref(&placement),
    );
    let definition = |kind: &str| {
        features
            .iter()
            .find(|feature| feature.source_tag.as_deref() == Some(kind))
            .map_or_else(
                || panic!("missing dispatched {kind} feature"),
                |feature| feature.definition.clone(),
            )
    };

    assert_eq!(
        definition("BaseFlange"),
        FeatureDefinition::SheetMetalBaseFlange {
            profile: ProfileRef::Sketch(neutral_sketch_id(&placement).unwrap()),
            thickness: Length(2.0),
            side: SheetMetalThicknessSide::Forward,
        }
    );
    assert_eq!(
        definition("RemoveBody"),
        FeatureDefinition::DeleteBody {
            bodies: BodySelection::Native(groups[1].id.clone()),
            mode: BodyRetentionMode::DeleteSelected,
        }
    );
    assert_eq!(
        definition("SurfaceStitch"),
        FeatureDefinition::KnitSurface {
            faces: FaceSelection::Native(scopes[2].id.clone()),
            merge_entities: Some(true),
            create_solid: Some(true),
            gap_tolerance: Some(Length(0.1)),
        }
    );
    assert_eq!(
        definition("CopyPaste"),
        FeatureDefinition::InsertComponent {
            occurrence: crate::ids::neutral_component_occurrence_id(
                "33333333-3333-4333-8333-333333333333"
            ),
        }
    );
    assert_eq!(
        definition("CopyPasteBodies"),
        FeatureDefinition::InsertBodies {
            bodies: BodySelection::Native(scopes[4].id.clone()),
        }
    );
    assert_eq!(
        definition("Base Feature"),
        FeatureDefinition::BaseFeature {
            bodies: BodySelection::Native(scopes[5].id.clone()),
        }
    );
    assert_eq!(
        definition("Thread"),
        FeatureDefinition::CosmeticThread {
            face: FaceSelection::Native(groups[3].id.clone()),
            diameter: Some(Length(3.5)),
            extent: Some(cadmpeg_ir::features::CosmeticThreadExtent::Through),
        }
    );
}

#[test]
fn loft_path_preserves_complete_historical_edge_selection() {
    use cadmpeg_ir::features::{EdgeSelection, PathRef};
    use cadmpeg_ir::ids::{FeatureInputTopologyId, HistoricalEdgeId};

    let state =
        FeatureInputTopologyId::mint("f3d:history-input:state#feature").expect("identity grammar");
    let edge =
        HistoricalEdgeId::mint("f3d:history-input:edge#7:feature:41:17").expect("identity grammar");
    assert_eq!(
        crate::design::feature_project::loft_path_from_edge_selection(
            "group",
            EdgeSelection::Historical {
                state: state.clone(),
                edges: vec![edge.clone()],
                native: "selection".into(),
            },
        ),
        PathRef::HistoricalEdges {
            state: state.clone(),
            edges: vec![edge.clone()],
            native: "selection".into(),
        }
    );
    assert_eq!(
        crate::design::feature_project::loft_path_from_edge_selection(
            "group",
            EdgeSelection::HistoricalPartial {
                state,
                edges: vec![edge],
                unresolved: vec!["operand".into()],
                native: "selection".into(),
            },
        ),
        PathRef::Native("group".into())
    );
}

#[test]
fn form_dispatcher_binds_the_legacy_single_cage_gate() {
    use std::io::{Cursor, Write};
    use zip::CompressionMethod;

    let stream = "FusionAssetName[Active]/FusionDesignSegmentType1/BulkStream.dat";
    let mut bulk = Vec::new();
    let mut cage_list = vec![0; 100];
    cage_list[..4].copy_from_slice(&3u32.to_le_bytes());
    cage_list[4..7].copy_from_slice(b"355");
    cage_list[7..11].copy_from_slice(&205u32.to_le_bytes());
    cage_list[21] = 1;
    cage_list[22..30].copy_from_slice(&201u64.to_le_bytes());
    cage_list[32..36].copy_from_slice(&1u32.to_le_bytes());
    cage_list[36] = 1;
    cage_list[37..45].copy_from_slice(&971u64.to_le_bytes());
    cage_list[47..49].copy_from_slice(&[0xfc, 0]);
    bulk.extend_from_slice(&cage_list);

    let mut paired = vec![0; 15];
    paired[..4].copy_from_slice(&3u32.to_le_bytes());
    paired[4..7].copy_from_slice(b"262");
    paired[7..11].copy_from_slice(&205u32.to_le_bytes());
    bulk.extend_from_slice(&paired);

    let mut object = vec![0; 15];
    object[..4].copy_from_slice(&3u32.to_le_bytes());
    object[4..7].copy_from_slice(b"325");
    object[7..11].copy_from_slice(&971u32.to_le_bytes());
    bulk.extend_from_slice(&object);

    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    crate::write_synthetic_manifests(&mut archive, stored);
    archive.start_file(stream, stored).unwrap();
    archive.write_all(&bulk).unwrap();
    let archive = archive.finish().unwrap().into_inner();

    let mut scope = crate::records::feature::DesignParameterScope::empty(
        &format!("f3d:{stream}:scope#201"),
        crate::records::feature::DesignFeatureKind::Form,
        201,
    );
    scope.reference_members = crate::records::ReferenceRun::unlocated(vec![205]);
    let feature_id = crate::ids::neutral_feature_id(&scope);
    let mut features = vec![cadmpeg_ir::features::Feature {
        id: feature_id,
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: Some("Form".into()),
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: cadmpeg_ir::features::FeatureDefinition::Native {
            kind: "Form".into(),
            parameters: Default::default(),
        },
        native_ref: Some(scope.id.clone()),
    }];
    let cages = [cadmpeg_ir::SubdSurface {
        id: cadmpeg_ir::ids::SubdId::mint("f3d:model:subd#1").expect("identity grammar"),
        scheme: cadmpeg_ir::subd::SubdScheme::CatmullClark,
        vertices: Vec::new(),
        edges: Vec::new(),
        faces: Vec::new(),
        symmetries: Vec::new(),
        source_object: None,
    }];

    crate::with_scan(&archive, |scan| {
        crate::design::feature_project::bind_form_cages(
            scan,
            std::slice::from_ref(&scope),
            &mut features,
            &cages,
        )
    })
    .expect("legacy Form cage binding");
    assert_eq!(
        features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::Form {
            cages: vec![cages[0].id.clone()],
        }
    );
}

#[test]
fn form_dispatcher_binds_a_unique_long_cage_list() {
    use std::io::{Cursor, Write};
    use zip::CompressionMethod;

    let stream = "FusionAssetName[Active]/FusionDesignSegmentType1/BulkStream.dat";
    let mut cage_list = vec![0; 99];
    cage_list[..4].copy_from_slice(&3u32.to_le_bytes());
    cage_list[4..7].copy_from_slice(b"415");
    cage_list[7..11].copy_from_slice(&205u32.to_le_bytes());
    cage_list[21] = 1;
    cage_list[22..30].copy_from_slice(&201u64.to_le_bytes());
    cage_list[32..36].copy_from_slice(&1u32.to_le_bytes());
    cage_list[36] = 1;
    cage_list[37..45].copy_from_slice(&971u64.to_le_bytes());
    let mut paired = vec![0; 15];
    paired[..4].copy_from_slice(&3u32.to_le_bytes());
    paired[4..7].copy_from_slice(b"258");
    paired[7..11].copy_from_slice(&205u32.to_le_bytes());
    let mut bulk = cage_list;
    bulk.extend_from_slice(&paired);

    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    crate::write_synthetic_manifests(&mut archive, stored);
    archive.start_file(stream, stored).unwrap();
    archive.write_all(&bulk).unwrap();
    let archive = archive.finish().unwrap().into_inner();

    let mut scope = crate::records::feature::DesignParameterScope::empty(
        &format!("f3d:{stream}:scope#201"),
        crate::records::feature::DesignFeatureKind::Form,
        201,
    );
    scope.reference_members = crate::records::ReferenceRun::unlocated(vec![205]);
    let feature_id = crate::ids::neutral_feature_id(&scope);
    let mut features = vec![cadmpeg_ir::features::Feature {
        id: feature_id,
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: Some("Form".into()),
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: cadmpeg_ir::features::FeatureDefinition::Native {
            kind: "Form".into(),
            parameters: Default::default(),
        },
        native_ref: Some(scope.id.clone()),
    }];
    let cages = [cadmpeg_ir::SubdSurface {
        id: cadmpeg_ir::ids::SubdId::mint("f3d:model:subd#1").expect("identity grammar"),
        scheme: cadmpeg_ir::subd::SubdScheme::CatmullClark,
        vertices: Vec::new(),
        edges: Vec::new(),
        faces: Vec::new(),
        symmetries: Vec::new(),
        source_object: None,
    }];

    crate::with_scan(&archive, |scan| {
        crate::design::feature_project::bind_form_cages(
            scan,
            std::slice::from_ref(&scope),
            &mut features,
            &cages,
        )
    })
    .expect("long Form cage binding");
    assert_eq!(
        features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::Form {
            cages: vec![cages[0].id.clone()],
        }
    );
}
