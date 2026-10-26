// SPDX-License-Identifier: Apache-2.0

use crate::records::sketch_placement::DesignSketchPlacement;
use cadmpeg_ir::document::Model;
use cadmpeg_ir::features::DistinctMembers;
use cadmpeg_ir::features::Feature;
use cadmpeg_ir::features::FeatureContent;
use cadmpeg_ir::features::FeatureDefinition;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::ids::RegionId;
use cadmpeg_ir::topology::Body;
use cadmpeg_ir::topology::BodyKind;
use cadmpeg_ir::topology::Region;
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::Native;

use crate::f3z::merge::{
    append_feature_history, compose_transforms, extend_native, occurrence_key,
    reparent_component_roots, OccurrenceScope,
};
use crate::records::xref::XrefReference;
use cadmpeg_ir::features::FeatureOperation;

fn feature(id: &str, ordinal: u64) -> Feature {
    Feature {
        id: FeatureId::mint(id).expect("identity grammar"),
        ordinal,
        name: None,
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind: "test".into(),
                parameters: std::collections::BTreeMap::new(),
            }),
        ),
        native_ref: None,
    }
}

#[test]
fn component_feature_history_follows_the_parent_without_losing_relative_order() {
    let mut parent = Model::default();
    parent.features = vec![
        feature("f3d:test:feature#parent-0", 4),
        feature("f3d:test:feature#parent-1", 8),
    ];
    let mut component = Model::default();
    component.features = vec![
        feature("f3d:test:feature#component-0", 10),
        feature("f3d:test:feature#component-1", 12),
    ];

    append_feature_history(&parent, &mut component).unwrap();

    assert_eq!(
        component
            .features
            .iter()
            .map(|feature| feature.ordinal)
            .collect::<Vec<_>>(),
        vec![9, 11]
    );
}

#[test]
fn component_feature_history_refuses_an_exhausted_ordinal_domain() {
    let mut parent = Model::default();
    parent.features = vec![feature("f3d:test:feature#parent", u64::MAX)];
    let mut component = Model::default();
    component.features = vec![feature("f3d:test:feature#component", 0)];

    let error = append_feature_history(&parent, &mut component).unwrap_err();

    assert!(error
        .to_string()
        .contains("merged F3Z feature ordinal exceeds u64::MAX"));
}

#[test]
fn occurrence_transform_composes_outside_existing_body_transform() {
    let outer = Transform::affine([
        [0.0, -1.0, 0.0, 20.0],
        [1.0, 0.0, 0.0, 30.0],
        [0.0, 0.0, 1.0, 40.0],
    ])
    .expect("affine transform");
    let inner = Transform::affine([
        [1.0, 0.0, 0.0, 5.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .expect("affine transform");

    assert_eq!(
        compose_transforms(outer, inner).unwrap().rows(),
        [
            [0.0, -1.0, 0.0, 20.0],
            [1.0, 0.0, 0.0, 35.0],
            [0.0, 0.0, 1.0, 40.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
    );
}

#[test]
fn merged_component_root_occurrences_become_children_of_the_outer_instance() {
    use cadmpeg_ir::ids::OccurrenceId;
    use cadmpeg_ir::products::{Occurrence, OccurrenceParent, PrototypeReference};

    let outer = OccurrenceId::mint("f3d:model:occurrence#xref-0-0").unwrap();
    let root_child = OccurrenceId::mint("f3d:xref/outer/model:occurrence#xref-0-0").unwrap();
    let nested_child = OccurrenceId::mint("f3d:xref/outer/model:occurrence#xref-1-0").unwrap();
    let mut occurrences = vec![
        Occurrence {
            id: root_child.clone(),
            prototype: PrototypeReference::Unresolved {},
            parent: OccurrenceParent::Root {},
            ordinal: 0,
            transform: Transform::identity(),
            linked_prototype: None,
            scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
            name: None,
            visible: None,
            link: None,
            native_ref: None,
        },
        Occurrence {
            id: nested_child.clone(),
            prototype: PrototypeReference::Unresolved {},
            parent: OccurrenceParent::Occurrence {
                occurrence: root_child.clone(),
            },
            ordinal: 0,
            transform: Transform::identity(),
            linked_prototype: None,
            scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
            name: None,
            visible: None,
            link: None,
            native_ref: None,
        },
    ];

    reparent_component_roots(&mut occurrences, &outer).unwrap();

    assert!(matches!(
        occurrences[0].parent,
        OccurrenceParent::Occurrence { ref occurrence } if occurrence == &outer
    ));
    assert!(matches!(
        occurrences[1].parent,
        OccurrenceParent::Occurrence { ref occurrence } if occurrence == &root_child
    ));
}

#[test]
fn repeated_occurrence_merge_remaps_typed_graphs_disjointly() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut merged = Model::default();
    let mut component = Model::default();
    component.bodies = vec![Body {
        id: BodyId::mint("f3d:brep:entity#1").expect("identity grammar"),
        kind: BodyKind::Solid,
        regions: vec![RegionId::mint("f3d:brep:entity#2").expect("identity grammar")],
        transform: None,
        name: None,
        color: None,
        visible: None,
    }];
    component.regions = vec![Region {
        id: RegionId::mint("f3d:brep:entity#2").expect("identity grammar"),
        body: BodyId::mint("f3d:brep:entity#1").expect("identity grammar"),
        shells: Vec::new(),
    }];
    for ordinal in 0..2 {
        let occurrence = format!("role/occurrence-{ordinal}");
        let mut scope = OccurrenceScope {
            ctx: &ctx,
            occurrence: &occurrence,
        };
        merged
            .extend_rewritten(component.clone(), &mut scope)
            .expect("merge component arenas");
    }

    for ordinal in 0..2 {
        let prefix = format!("f3d:xref/role/occurrence-{ordinal}/brep:entity#");
        assert_eq!(merged.bodies[ordinal].id.as_str(), format!("{prefix}1"));
        assert_eq!(
            merged.bodies[ordinal].regions[0].as_str(),
            format!("{prefix}2")
        );
        assert_eq!(merged.regions[ordinal].id.as_str(), format!("{prefix}2"));
        assert_eq!(merged.regions[ordinal].body.as_str(), format!("{prefix}1"));
    }
}

#[test]
fn occurrence_merge_preserves_a_body_name_that_spells_its_identity() {
    use cadmpeg_ir::document::EntityRewrite;
    let ctx = cadmpeg_test_support::service_decode_context();
    let source_id = "f3d:model:body#source";
    let body = Body {
        id: BodyId::mint(source_id).unwrap(),
        kind: BodyKind::Solid,
        regions: vec![RegionId::mint("f3d:model:region#source").unwrap()],
        transform: None,
        name: Some(source_id.into()),
        color: None,
        visible: None,
    };
    let scoped = OccurrenceScope {
        ctx: &ctx,
        occurrence: "component-0",
    }
    .rewrite(body)
    .unwrap();
    assert_eq!(scoped.id.as_str(), "f3d:xref/component-0/model:body#source");
    assert_eq!(
        scoped.regions[0].as_str(),
        "f3d:xref/component-0/model:region#source"
    );
    assert_eq!(scoped.name.as_deref(), Some(source_id));
}

#[test]
fn occurrence_merge_remaps_and_retains_native_records() {
    let placement = DesignSketchPlacement {
        frame: crate::records::sketch_placement::DesignSketchFrame::new(
            42,
            crate::records::sketch_placement::DesignSketchFrameForm::MemberCompact {
                paired_byte_offset: 76,
            },
        )
        .unwrap(),
        id: "f3d:Design/BulkStream.dat:design-sketch-placement#42".into(),
        scope_record_index: None,
        entity_id: crate::records::identity::DesignEntityId::try_from("Sketch_1".to_owned())
            .expect("valid entity ID"),

        visibility: None,

        class_tag: crate::records::references::DesignClassTag::try_from("001".to_owned()).unwrap(),
        record_index: 7,

        paired_class_tag: crate::records::references::DesignClassTag::try_from("002".to_owned())
            .unwrap(),
    };
    let mut component = Native::default();
    component
        .namespace_mut("f3d")
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "design_sketch_placements",
            &[placement],
        )
        .expect("store component native");
    let mut root = Native::default();
    extend_native(
        &cadmpeg_test_support::service_decode_context(),
        &mut root,
        component,
        "role/occurrence-0",
    )
    .unwrap();

    let merged: Vec<DesignSketchPlacement> = root
        .namespace("f3d")
        .expect("merged f3d namespace")
        .arena_as("design_sketch_placements")
        .expect("read merged arena");
    assert_eq!(
        merged[0].id,
        "f3d:xref/role/occurrence-0/Design/BulkStream.dat:design-sketch-placement#42"
    );
}

#[test]
fn occurrence_configuration_survives_document_and_typed_native_admission() {
    use crate::records::configuration::{DesignConfiguration, DesignConfigurationKind};
    let configuration = DesignConfiguration::try_new(
        "Design/table.dsgcfg".into(),
        DesignConfigurationKind::Table,
        Vec::new(),
        serde_json::Map::new(),
    )
    .unwrap();
    let mut component = Native::default();
    component
        .namespace_mut("f3d")
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "design_configurations",
            &[configuration],
        )
        .unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty();
    extend_native(
        &cadmpeg_test_support::service_decode_context(),
        &mut ir.native,
        component,
        "component-0",
    )
    .unwrap();
    let wire = ir.to_canonical_json().unwrap();
    let admitted = cadmpeg_ir::CadIr::from_json(&wire).unwrap();
    let configurations = admitted
        .native
        .namespace("f3d")
        .unwrap()
        .arena_as::<DesignConfiguration>("design_configurations")
        .unwrap();
    assert_eq!(configurations.len(), 1);
    assert_eq!(configurations[0].entry_name(), "Design/table.dsgcfg");
    assert_eq!(
        configurations[0].id(),
        "f3d:xref/component-0/configuration:entry#Design/table.dsgcfg"
    );
    assert_eq!(configurations[0].payload(), serde_json::Map::new());
}

#[test]
fn occurrence_merge_scopes_admitted_native_references_and_preserves_configuration_text() {
    use crate::records::{
        bodies::BodyVisibility,
        configuration::{DesignConfiguration, DesignConfigurationKind},
    };

    let configuration_payload = serde_json::json!({
        "raw_text": "f3d:brep:entity#3",
        "nested": {
            "f3d:brep:entity#4": "f3d:brep:entity#5",
        },
    })
    .as_object()
    .expect("object configuration payload")
    .clone();
    let configuration = DesignConfiguration::try_new(
        "Design/table.dsgcfg".into(),
        DesignConfigurationKind::Table,
        Vec::new(),
        configuration_payload.clone(),
    )
    .expect("admitted configuration payload");
    let visibility = BodyVisibility::try_from(crate::records::bodies::BodyVisibilityWire {
        id: "f3d:Design/BulkStream.dat:body-visibility#3".into(),
        body: BodyId::mint("f3d:brep:entity#1").expect("identity grammar"),
        stream: "Design/BulkStream.dat".into(),
        byte_offset: 10,
        asm_body_key_offset: 20,
        asm_body_key: 3,
        entity_suffix: 1,
        visible: true,
    })
    .unwrap();
    let mut component = Native::default();
    component
        .namespace_mut("f3d")
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "design_configurations",
            &[configuration],
        )
        .expect("store configuration");
    component
        .namespace_mut("f3d")
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "body_visibilities",
            &[visibility],
        )
        .expect("store typed native reference");

    let mut root = Native::default();
    extend_native(
        &cadmpeg_test_support::service_decode_context(),
        &mut root,
        component,
        "role/occurrence-0",
    )
    .unwrap();

    let merged_visibility: Vec<BodyVisibility> = root
        .namespace("f3d")
        .expect("merged f3d namespace")
        .arena_as("body_visibilities")
        .expect("read merged visibility arena");
    assert_eq!(
        merged_visibility[0].id(),
        "f3d:xref/role/occurrence-0/Design/BulkStream.dat:body-visibility#3"
    );
    assert_eq!(
        merged_visibility[0].body.as_str(),
        "f3d:xref/role/occurrence-0/brep:entity#1"
    );
    assert_eq!(merged_visibility[0].stream(), "Design/BulkStream.dat");

    let merged_configurations: Vec<DesignConfiguration> = root
        .namespace("f3d")
        .expect("merged f3d namespace")
        .arena_as("design_configurations")
        .expect("read merged configuration arena");
    assert_eq!(
        merged_configurations[0].id(),
        "f3d:xref/role/occurrence-0/configuration:entry#Design/table.dsgcfg"
    );
    assert_eq!(
        merged_configurations[0].payload(),
        configuration_payload,
        "configuration text and extension map keys are source payload, not identities"
    );
}

#[test]
fn occurrence_key_preserves_authored_roles() {
    assert!(crate::records::xref::RequiredXrefText::try_from(String::new()).is_err());
    let reference = |role: &str, ordinal: u32| XrefReference {
        id: "f3d:xref:reference#1".into(),
        ordinal,
        occurrence_ordinal: 0,
        placement_source: None,
        from: "root.f3d".to_owned().try_into().unwrap(),
        relative_path: "part.f3d".to_owned().try_into().unwrap(),
        neutron_role: role.to_owned().try_into().unwrap(),
        neutron_data: String::new(),
        transform: None,
    };

    assert_eq!(
        occurrence_key(&reference("ordinal-7", 7)),
        "role-ordinal-7/reference-7/occurrence-0"
    );
    assert_eq!(
        occurrence_key(&reference("role /#: value", 7)),
        "role-role%20%2F%23%3A%20value/reference-7/occurrence-0"
    );
    let key = occurrence_key(&reference("role /#: value", 7));
    cadmpeg_ir::ids::Identity::new(format!("f3d:xref/{key}/native:record#1"))
        .expect("encoded occurrence key remains an admitted identity scope");
}

#[test]
fn occurrence_key_separates_same_role_references_with_reset_ordinals() {
    let reference = |ordinal| XrefReference {
        id: format!("f3d:xref:reference#{ordinal}"),
        ordinal,
        occurrence_ordinal: 0,
        placement_source: None,
        from: "root.f3d".to_owned().try_into().unwrap(),
        relative_path: format!("part-{ordinal}.f3d").try_into().unwrap(),
        neutron_role: "same-role".to_owned().try_into().unwrap(),
        neutron_data: String::new(),
        transform: None,
    };

    assert_ne!(
        occurrence_key(&reference(0)),
        occurrence_key(&reference(1)),
        "reference ordinal is part of the occurrence owner when role ordinals reset"
    );
}

#[test]
fn merged_documents_keep_parameter_names_and_document_local_expression_dependencies() {
    use crate::f3z::merge::scope_document_parameters;

    let ctx = cadmpeg_test_support::service_decode_context();
    let (mut parent, mut native) = parameter_document(&ctx, 1.0);
    let (mut component, component_native) = parameter_document(&ctx, 2.0);
    let mut owned = component.parameters[1].clone();
    let construction = feature("f3d:model:feature#consumer", 0);
    owned.id = cadmpeg_ir::features::ParameterId::mint("f3d:model:parameter#input").unwrap();
    owned.owner = Some(construction.id.clone());
    owned.ordinal = 0;
    component.parameters.push(owned);
    component.features.push(construction);

    scope_document_parameters(&ctx, &mut component).unwrap();
    append_feature_history(&parent, &mut component).unwrap();
    parent
        .extend_rewritten(
            component,
            &mut OccurrenceScope {
                ctx: &ctx,
                occurrence: "component-0",
            },
        )
        .unwrap();
    extend_native(&ctx, &mut native, component_native, "component-0").unwrap();

    assert_eq!(parent.parameters.len(), 5);
    assert_eq!(parent.parameters[0].name, parent.parameters[2].name);
    assert_eq!(parent.parameters[1].name, parent.parameters[3].name);
    assert_ne!(parent.parameters[0].value, parent.parameters[2].value);
    assert_eq!(
        parent
            .parameters
            .iter()
            .map(|parameter| parameter.ordinal)
            .collect::<Vec<_>>(),
        [0, 1, 0, 1, 0]
    );
    assert_eq!(parent.parameters[1].expression, "Span / 2");
    assert_eq!(parent.parameters[3].expression, "Span / 2");
    assert_eq!(parent.parameters[0].owner, None);
    let scope = parent.parameters[2].owner.as_ref().unwrap();
    assert_eq!(parent.parameters[3].owner.as_ref(), Some(scope));
    assert_ne!(parent.parameters[4].owner.as_ref(), Some(scope));
    assert_eq!(
        parent.parameters[1].dependencies.as_slice(),
        std::slice::from_ref(&parent.parameters[0].id)
    );
    assert_eq!(
        parent.parameters[3].dependencies.as_slice(),
        std::slice::from_ref(&parent.parameters[2].id)
    );
    assert_eq!(
        parent.parameters[4].dependencies.as_slice(),
        std::slice::from_ref(&parent.parameters[2].id)
    );
    let container = parent
        .features
        .iter()
        .find(|feature| &feature.id == scope)
        .unwrap();
    assert!(matches!(
        container.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::TreeNode {
            role: cadmpeg_ir::features::FeatureTreeNodeRole::Equations,
            ..
        })
    ));
    assert!(parent
        .features
        .iter()
        .filter(|feature| &feature.id != scope)
        .all(|feature| container.ordinal < feature.ordinal));
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model = parent;
    ir.native = native;
    ir.finalize();
    let report = cadmpeg_ir::validate::validate_neutral(&ir, Vec::new()).unwrap();
    assert!(report.is_ok(), "{:?}", report.findings);
}

#[test]
fn document_parameter_scope_is_charged_against_the_entity_budget() {
    use crate::f3z::merge::scope_document_parameters;

    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_entities = 0;
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let ctx = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .unwrap()
        .0;
    let (mut model, _) = parameter_document(&cadmpeg_test_support::service_decode_context(), 1.0);
    let features = model.features.len();

    let error = scope_document_parameters(&ctx, &mut model).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)),
        "{error:?}"
    );
    assert_eq!(model.features.len(), features);
    assert!(model
        .parameters
        .iter()
        .all(|parameter| parameter.owner.is_none()));
}

fn parameter_document(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scalar: f64,
) -> (Model, Native) {
    use crate::design::feature_project::{
        project_parameter_design_with_edge_identities, ProjectInputs,
    };
    use crate::design::test_support::synthetic_feature_timelines;
    use crate::records::parameters::{
        DesignParameter, DesignParameterDraft, DesignParameterSource,
    };

    let native = [("Span", "10 mm"), ("HalfSpan", "Span / 2")]
        .into_iter()
        .enumerate()
        .map(|(ordinal, (name, expression))| {
            DesignParameter::try_from(DesignParameterDraft {
                id: format!("f3d:native:parameter#{ordinal}"),
                byte_offset: 0,
                class_tag: "305".to_owned().try_into().unwrap(),
                record_index: u32::try_from(ordinal).unwrap(),
                source_ordinal: u32::try_from(ordinal).unwrap(),
                source: DesignParameterSource::User {
                    family_discriminator: None,
                },
                expression: expression.into(),
                expression_offset: 1,
                source_kind_offset: 2,
                unit: Some(crate::records::identity::RecordedValue {
                    value: "mm".into(),
                    offset: 3,
                }),
                name: name.into(),
                name_offset: 4,
                evaluated_value: scalar,
                evaluated_value_offset: 5,
            })
            .unwrap()
        })
        .collect::<Vec<_>>();
    let timelines = synthetic_feature_timelines(&[]);
    let projected = project_parameter_design_with_edge_identities(
        ctx,
        &ProjectInputs {
            native: &native,
            timelines: &timelines,
            ..ProjectInputs::default()
        },
    )
    .unwrap();
    let mut model = Model::default();
    model.parameters = projected.parameters;
    model.features = projected.features;
    let mut records = Native::default();
    records
        .namespace_mut("f3d")
        .set_arena(ctx, "design_parameters", &native)
        .unwrap();
    (model, records)
}

#[test]
fn external_placement_source_keeps_its_local_owner_and_scopes_its_stream() {
    let mut fields = serde_json::json!({
        "placement_source": {
            "stream": "f3d:Design/BulkStream.dat",
            "carrier_record_index": 71,
            "owner": { "target": 19, "type_guid": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee" }
        }
    })
    .as_object()
    .unwrap()
    .clone();
    crate::test_support::with_decode_context(|ctx| {
        super::super::rescope_native_reference_fields(
            ctx,
            "xref_references",
            &mut fields,
            "part/occurrence-2",
        )
        .unwrap();
    });
    assert_eq!(
        fields["placement_source"]["stream"],
        "f3d:xref/part/occurrence-2/Design/BulkStream.dat"
    );
    assert_eq!(fields["placement_source"]["carrier_record_index"], 71);
    assert_eq!(fields["placement_source"]["owner"]["target"], 19);
    assert_eq!(
        fields["placement_source"]["owner"]["type_guid"],
        "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"
    );
}

#[test]
fn repeated_local_owners_merge_each_external_child_tree_in_its_composed_frame() {
    use crate::test_support::assembly_test::{
        f3d_without_brep_with_xref_placement, f3z_archive, XREF_ROLE,
    };
    use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
    use crate::test_support::zip_test::f3d_with_smbh;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_ir::ids::OccurrenceId;
    use cadmpeg_ir::products::{
        ExternalDocument, Occurrence, OccurrenceParent, PrototypeReference,
    };

    let mut child_matrix = Transform::identity().rows();
    child_matrix[1][3] = 0.5;
    let member = f3d_without_brep_with_xref_placement(
        "assembly-design",
        "member.f3d",
        "leaf.f3d",
        XREF_ROLE,
        child_matrix,
    );
    let leaf = f3d_with_smbh(&synthetic_geometry_smbh());
    let archive = f3z_archive(
        "member.f3d",
        &[("member.f3d", &member), ("leaf.f3d", &leaf)],
    );
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, bytes) = DecodeContext::from_root_bytes(&archive, &arena, &policy).unwrap();
    let scan = crate::container::scan(&ctx, bytes).unwrap();
    let archive = crate::f3z::archive::classify_members(&ctx, &scan).unwrap();
    let reference = XrefReference {
        id: "f3d:xref:reference#0".to_owned(),
        ordinal: 0,
        occurrence_ordinal: 0,
        placement_source: None,
        from: "root.f3d".to_owned().try_into().unwrap(),
        relative_path: "member.f3d".to_owned().try_into().unwrap(),
        neutron_role: "outer-role".to_owned().try_into().unwrap(),
        neutron_data: String::new(),
        transform: None,
    };
    let table = crate::xref::XrefTable {
        references: vec![reference],
        ..Default::default()
    };
    let mut ir = cadmpeg_ir::CadIr::empty();
    let mut children = Vec::new();
    for index in 0..2 {
        let parent = OccurrenceId::mint(format!("f3d:model:occurrence#group-{index}")).unwrap();
        let transform = if index == 0 {
            Transform::affine([
                [0.0, -1.0, 0.0, 30.0],
                [1.0, 0.0, 0.0, 40.0],
                [0.0, 0.0, 1.0, 0.0],
            ])
            .unwrap()
        } else {
            Transform::affine([
                [1.0, 0.0, 0.0, 80.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ])
            .unwrap()
        };
        let group = Occurrence {
            id: parent.clone(),
            prototype: PrototypeReference::Unresolved {},
            parent: OccurrenceParent::Root {},
            ordinal: index,
            transform,
            linked_prototype: None,
            scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
            name: None,
            visible: None,
            link: None,
            native_ref: None,
        };
        let mut child = group.clone();
        child.id = crate::ids::nested_xref_occurrence_id(&parent, 0, 0);
        children.push(child.id.clone());
        child.parent = OccurrenceParent::Occurrence { occurrence: parent };
        child.prototype = PrototypeReference::External {
            document: ExternalDocument::path("member.f3d"),
            object: None,
        };
        child.transform = Transform::affine([
            [1.0, 0.0, 0.0, 20.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .unwrap();
        child.native_ref = Some(table.references[0].id.clone());
        ir.model.occurrences.extend([group, child]);
    }
    let mut report =
        cadmpeg_ir::codec::DecodeBody::new(cadmpeg_ir::report::decode::DecodeTransfer::at(
            cadmpeg_core::decode::DecodeDepth::Full,
            false,
        ));
    let mut fidelity = cadmpeg_ir::SourceFidelity::default();
    let merged = crate::f3z::merge::MergeSession {
        ctx: &ctx,
        scan: &scan,
        archive: &archive,
        stack: vec!["root.f3d".to_owned()],
    }
    .merge(&mut ir, &mut report, &mut fidelity, &table)
    .unwrap();
    assert_eq!(merged, 4);
    assert_eq!(ir.model.bodies.len(), 2);
    let translations = ir
        .model
        .bodies
        .iter()
        .map(|body| {
            let rows = body.transform.unwrap().rows();
            [rows[0][3], rows[1][3]]
        })
        .collect::<Vec<_>>();
    assert_eq!(translations, [[25.0, 60.0], [100.0, 5.0]]);
    assert_ne!(ir.model.bodies[0].id, ir.model.bodies[1].id);
    for child in children {
        let imported = ir
            .model
            .occurrences
            .iter()
            .find(|occurrence| {
                occurrence.parent
                    == OccurrenceParent::Occurrence {
                        occurrence: child.clone(),
                    }
            })
            .unwrap();
        assert_eq!(imported.transform.rows()[1][3], 5.0);
    }
    assert_eq!(ir.model.occurrences[1].transform.rows()[0][3], 20.0);
    assert_eq!(ir.model.occurrences[3].transform.rows()[0][3], 20.0);
}

#[test]
fn imported_definition_bodies_keep_their_definition_frame() {
    let mut model = cadmpeg_ir::examples::unit_cube().unwrap().model;
    let body = model.bodies[0].id.clone();
    model
        .product_definitions
        .push(cadmpeg_ir::products::ProductDefinition {
            id: cadmpeg_ir::ids::ProductDefinitionId::mint("f3d:model:component#owned").unwrap(),
            kind: cadmpeg_ir::products::ProductDefinitionKind::Part,
            bodies: vec![body],
            source_name: None,
            label: None,
            description: None,
            part_number: None,
            bom_properties: std::collections::BTreeMap::new(),
            sketches: Vec::new(),
            native_ref: None,
        });
    let original = model.bodies[0].transform;
    let transform = Transform::affine([
        [1.0, 0.0, 0.0, 20.0],
        [0.0, 1.0, 0.0, 30.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .unwrap();
    crate::f3z::merge::apply_occurrence_transform(&mut model, transform).unwrap();
    assert_eq!(model.bodies[0].transform, original);
}
