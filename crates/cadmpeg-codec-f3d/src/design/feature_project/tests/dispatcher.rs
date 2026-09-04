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

#[test]
fn dispatcher_projects_datum_feature_scopes() {
    let mut transform = identity_matrix();
    transform[0][3] = 1.0;
    transform[1][3] = 2.0;
    transform[2][3] = 3.0;

    let mut joint_origin =
        DesignParameterScope::empty("f3d:native:parameter-scope#1", "JointOrigin", 1);
    joint_origin.joint_origin_transform = Some(transform);

    let mut work_plane =
        DesignParameterScope::empty("f3d:native:parameter-scope#2", "WorkPlane", 2);
    work_plane.work_plane_transform = Some(transform);

    let mut work_point =
        DesignParameterScope::empty("f3d:native:parameter-scope#3", "WorkPoint", 3);
    work_point.work_point_construction = Some(crate::records::DesignWorkPointConstruction {
        point_record_index: 4,
        point_record_byte_offset: 0,
        position: [4.0, 5.0, 6.0],
        position_offset: 0,
        rule: crate::records::DesignWorkPointRule::Native {
            reference_type: 1,
            inputs: Vec::new(),
        },
        reference_type_offset: 0,
    });

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
    let mut scale = DesignParameterScope::empty("f3d:native:parameter-scope#4", "Scale", 4);
    scale.scale_operation = Some(DesignScaleOperation {
        body_group_record_index: 5,
        center_record_index: 6,
        center_position: Some([1.25, -2.5, 3.75]),
        center_position_offset: Some(40),
        uniform_factor: 2.5,
        uniform_factor_offset: 20,
    });

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
    let mut referenced =
        DesignParameterScope::empty("f3d:native:parameter-scope#10", "WorkPlane", 10);
    referenced.work_plane_transform = Some(identity_matrix());
    referenced.work_plane_reference = Some(11);

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
    use crate::records::{DesignVertexRecipe, DesignWorkPlaneConstruction};
    use cadmpeg_ir::features::VertexSelection;

    let recipe = |record_index, vertex| DesignVertexRecipe {
        record_index,
        byte_offset: u64::from(record_index),
        class_tag: "306".into(),
        paired_byte_offset: 1,
        paired_class_tag: "261".into(),
        recipe_record_index: record_index + 3,
        recipe_record_byte_offset: 2,
        recipe_id: format!("f3d:native:construction-recipe#{record_index}"),
        recipe_prefix_offset: 3,
        recipe_prefix_bytes: Vec::new(),
        recipe_references: Vec::new(),
        recipe_program_offset: 4,
        recipe_program: vec![0],
        recipe_state_id: Some(4),
        resolved_vertex_slot: Some(vertex),
        next_record_index: record_index + 5,
        next_byte_offset: 5,
    };
    let mut plane = DesignParameterScope::empty("f3d:native:parameter-scope#20", "WorkPlane", 20);
    plane.work_plane_transform = Some(identity_matrix());
    plane.work_plane_construction = Some(DesignWorkPlaneConstruction::ThreePoint {
        placement_record_index: 21,
        inputs: Box::new([recipe(22, 43), recipe(27, 64), recipe(32, 84)]),
    });

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
        ] if first.0.ends_with(":43") && second.0.ends_with(":64") && third.0.ends_with(":84")
    ));
}

#[test]
fn dispatcher_projects_work_point_plane_construction_and_dependencies() {
    use crate::records::{
        DesignWorkPointConstruction, DesignWorkPointInput, DesignWorkPointInputCarrier,
        DesignWorkPointPlaneSelection, DesignWorkPointRule,
    };
    use cadmpeg_ir::features::{DatumPlaneReference, DatumPointConstruction};

    let planes = [10, 20, 30].map(|record_index| {
        let id = format!("f3d:native:parameter-scope#{record_index}");
        let mut scope = DesignParameterScope::empty(&id, "WorkPlane", record_index);
        scope.work_plane_transform = Some(identity_matrix());
        scope
    });
    let input = |record_index, work_plane_scope_record_index| DesignWorkPointInput {
        record_index,
        reference_offset: u64::from(record_index),
        carrier: Some(Box::new(DesignWorkPointInputCarrier::WorkPlane {
            selection: DesignWorkPointPlaneSelection {
                class_tag: "267".into(),
                asset_id: "00000000-0000-0000-0000-000000000001".into(),
                asset_id_offset: 1,
                context_id: "00000000-0000-0000-0000-000000000002".into(),
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
    let mut point = DesignParameterScope::empty("f3d:native:parameter-scope#40", "WorkPoint", 40);
    point.work_point_construction = Some(DesignWorkPointConstruction {
        point_record_index: 41,
        point_record_byte_offset: 0,
        position: [1.0, 2.0, 3.0],
        position_offset: 0,
        rule: DesignWorkPointRule::ThreePlaneIntersection {
            inputs: [input(42, 10), input(46, 20), input(50, 30)],
        },
        reference_type_offset: 0,
    });
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
    use crate::records::{
        DesignVertexRecipe, DesignWorkPointConstruction, DesignWorkPointInput,
        DesignWorkPointInputCarrier, DesignWorkPointRule,
    };
    use cadmpeg_ir::features::{DatumPointConstruction, VertexSelection};

    let mut predecessor =
        DesignParameterScope::empty("f3d:native:parameter-scope#10", "Extrude", 10);
    predecessor.history_state_id = Some(4);
    let recipe_id = "f3d:native:construction-recipe#vertex".to_string();
    let recipe = DesignVertexRecipe {
        record_index: 12,
        byte_offset: 0,
        class_tag: "369".into(),
        paired_byte_offset: 1,
        paired_class_tag: "261".into(),
        recipe_record_index: 23,
        recipe_record_byte_offset: 2,
        recipe_id: recipe_id.clone(),
        recipe_prefix_offset: 3,
        recipe_prefix_bytes: Vec::new(),
        recipe_references: Vec::new(),
        recipe_program_offset: 4,
        recipe_program: vec![0],
        recipe_state_id: Some(4),
        resolved_vertex_slot: Some(43),
        next_record_index: 25,
        next_byte_offset: 5,
    };
    let mut point = DesignParameterScope::empty("f3d:native:parameter-scope#20", "WorkPoint", 20);
    point.work_point_construction = Some(DesignWorkPointConstruction {
        point_record_index: 21,
        point_record_byte_offset: 0,
        position: [4.0, 3.0, 0.0],
        position_offset: 0,
        rule: DesignWorkPointRule::Vertex {
            input: DesignWorkPointInput {
                record_index: 22,
                reference_offset: 0,
                carrier: Some(Box::new(DesignWorkPointInputCarrier::VertexRecipe {
                    recipe,
                })),
            },
        },
        reference_type_offset: 0,
    });
    let timeline = DesignFeatureTimeline {
        id: crate::ids::native_design_feature_timeline_id_in_stream("f3d:native", 0),
        byte_offset: 0,
        class_tag: "256".into(),
        record_index: 1,
        source_ordinal: 0,
        frame_length: 0,
        context_record_index: 1,
        context_record_index_offset: 0,
        item_count_offset: 0,
        item_record_indices: vec![10, 20],
        item_record_index_offsets: vec![0, 0],
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
        .0
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
    use crate::records::{
        DesignBaseFeatureConstruction, DesignBaseFlangeOperation,
        DesignConstructionOperandGroupFrame, DesignCopyPasteBodiesOperation,
        DesignCopyPasteComponentOperation,
    };
    use cadmpeg_ir::features::{BodyRetentionMode, BodySelection, SheetMetalThicknessSide};

    let stream = "f3d:native";
    let group = |scope_record_index: u32,
                 scope_reference_ordinal: u32,
                 record_index: u32,
                 members: &[u32],
                 role: u64| {
        DesignConstructionOperandGroup {
            id: format!("{stream}:construction-group#{record_index}"),
            scope_record_index,
            scope_reference_ordinal,
            record_index,
            byte_offset: 0,
            class_tag: "264".into(),
            members: members.to_vec(),
            lost_edge_references: Vec::new(),
            member_offsets: vec![0; members.len()],
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
            role,
            extrude_role: None,
            extrude_face_role: None,
            role_offset: 0,
            paired_class_tag: "264".into(),
            paired_byte_offset: 0,
        }
    };

    let mut base_flange =
        DesignParameterScope::empty(&format!("{stream}:scope#base-flange"), "BaseFlange", 10);
    base_flange.base_flange_operation = Some(DesignBaseFlangeOperation {
        thickness: 0.2,
        thickness_offset: 0,
        profile_group_record_index: 100,
        profile_record_index: 101,
        thickness_record_index: 102,
        settings_record_index: 103,
    });
    base_flange.base_flange_profile = Some(DesignSketchProfileOperand {
        scope_reference_ordinal: 1,
        record_index: 101,
        byte_offset: 0,
        class_tag: "377".into(),
        asset_id: "asset".into(),
        asset_id_offset: 0,
        entity_id: format!("{stream}:sketch#7"),
        entity_suffix: 7,
        entity_reference_offset: 0,
        region_selection: None,
        paired_class_tag: "264".into(),
        paired_byte_offset: 0,
    });

    let mut remove_body =
        DesignParameterScope::empty(&format!("{stream}:scope#remove-body"), "RemoveBody", 20);
    remove_body.reference_members = vec![200];

    let mut surface_stitch = DesignParameterScope::empty(
        &format!("{stream}:scope#surface-stitch"),
        "SurfaceStitch",
        30,
    );
    surface_stitch.reference_members = vec![300, 301, 302, 303];
    surface_stitch.surface_stitch_operation = Some(DesignSurfaceStitchOperation {
        gap_tolerance: 0.01,
        gap_tolerance_offset: 0,
        tolerance_record_index: 302,
        settings_record_index: 303,
    });

    let mut copy_paste =
        DesignParameterScope::empty(&format!("{stream}:scope#copy-paste"), "CopyPaste", 40);
    copy_paste.copy_paste_component_operation = Some(DesignCopyPasteComponentOperation {
        relation_record_index: 401,
        source_occurrence_record_index: 402,
        copied_occurrence_record_index: 403,
        component_guid: "component".into(),
        source_occurrence_guid: "source-occurrence".into(),
        copied_occurrence_guid: "copied-occurrence".into(),
        source_transform: identity_matrix(),
        source_transform_offset: 0,
        copied_transform: identity_matrix(),
        copied_transform_offset: 0,
    });

    let mut copy_paste_bodies = DesignParameterScope::empty(
        &format!("{stream}:scope#copy-paste-bodies"),
        "CopyPasteBodies",
        50,
    );
    copy_paste_bodies.copy_paste_bodies_operation = Some(DesignCopyPasteBodiesOperation {
        body_group_record_index: 501,
        body_group_class_tag: "264".into(),
        body_group_byte_offset: 0,
        body_operand_record_indices: vec![502],
        body_operand_record_offsets: vec![0],
        relation_record_index: 503,
        relation_class_tag: "264".into(),
        relation_byte_offset: 0,
        source_body_entity_suffixes: vec![11],
        source_body_entity_suffix_offsets: vec![0],
        copied_body_entity_suffixes: vec![12],
        copied_body_entity_suffix_offsets: vec![0],
    });

    let mut base_feature =
        DesignParameterScope::empty(&format!("{stream}:scope#base-feature"), "Base Feature", 60);
    base_feature.base_feature_construction = Some(DesignBaseFeatureConstruction::ResultBodies {
        body_entity_suffixes: vec![21],
        body_entity_suffix_offsets: vec![0],
        body_entity_fields: vec![[0; 6]],
        body_reference_records: vec![601],
        body_reference_record_offsets: vec![0],
        body_reference_fields: vec![[0; 6]],
        repeated_reference_fields: Vec::new(),
        metadata_record: 602,
        metadata_record_offset: 0,
        metadata_field: vec![0, 0],
        result_records: vec![603],
        result_record_offsets: vec![0],
        result_fields: vec![[0; 6]],
    });

    let mut thread = DesignParameterScope::empty(&format!("{stream}:scope#thread"), "Thread", 70);
    thread.thread_construction = Some(DesignThreadConstruction {
        form: DesignThreadForm::Compact,
        designation_offset: 0,
        designation: "M3.5x0.6".into(),
        nominal_size_text: "3.5".into(),
        nominal_size: 3.5,
        profile: "GB Metric profile".into(),
        major_diameter: 0.35995,
        minor_diameter: 0.293,
        pitch: 0.06,
        pitch_diameter: 0.3166,
        trailing_reference_record_index: None,
        trailing_reference_offset: None,
        face_group_record_indices: vec![701],
    });
    thread.reference_members = vec![701, 702];

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
        group(10, 0, 100, &[101], 0x0000_0041_0000_0000),
        group(20, 0, 200, &[201], 0x0000_0004_0000_0000),
        group(30, 0, 300, &[301], 0x0000_0005_0000_0000),
        group(70, 0, 701, &[702], 0x0000_0010_0000_0000),
    ];
    let placement = DesignSketchPlacement {
        id: format!("{stream}:placement#7"),
        scope_record_index: None,
        entity_id: format!("{stream}:sketch#7"),
        entity_suffix: 7,
        visibility: None,
        byte_offset: 0,
        class_tag: "264".into(),
        record_index: 700,
        frame_length: 0,
        transform: identity_matrix(),
        transform_offset: None,
        paired_class_tag: "264".into(),
        paired_byte_offset: 0,
        member_run_head: false,
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
            profile: ProfileRef::Sketch(neutral_sketch_id(&placement)),
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
            occurrence: crate::ids::neutral_component_occurrence_id("copied-occurrence"),
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

    let mut scope = crate::records::DesignParameterScope::empty(
        &format!("f3d:{stream}:scope#201"),
        "Form",
        201,
    );
    scope.reference_members = vec![205];
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

    let mut scope = crate::records::DesignParameterScope::empty(
        &format!("f3d:{stream}:scope#201"),
        "Form",
        201,
    );
    scope.reference_members = vec![205];
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
