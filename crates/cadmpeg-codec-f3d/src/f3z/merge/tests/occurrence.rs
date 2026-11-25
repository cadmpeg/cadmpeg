// SPDX-License-Identifier: Apache-2.0

mod partial_operands;
mod repeated_owners;

use crate::records::sketch_placement::DesignSketchPlacement;
use cadmpeg_ir::document::{Model, Topology};
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::Feature;
use cadmpeg_ir::features::FeatureContent;
use cadmpeg_ir::features::FeatureDefinition;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::ids::RegionId;
use cadmpeg_ir::schema::rewrite::typed::TextReplacements;
use cadmpeg_ir::topology::Body;
use cadmpeg_ir::topology::BodyKind;
use cadmpeg_ir::topology::Region;
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::Native;

use super::member_source;
use crate::f3z::merge::{append_feature_history, extend_native, occurrence_key, OccurrenceScope};
use crate::records::xref::XrefReference;
use cadmpeg_ir::features::FeatureOperation;

fn feature(id: &str, ordinal: u64) -> Feature {
    Feature {
        product_definition: None,
        id: FeatureId::mint(id).expect("identity grammar"),
        ordinal,
        name: None,
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),

        evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
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
fn member_definition_preserves_the_body_transform_inside_each_placement() {
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

    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    ir.source = Some(member_source());
    ir.geometry.bodies[0].transform = Some(inner);
    crate::f3z::merge::define_member(&ctx, &mut ir, "member", "member.f3d").unwrap();
    assert_eq!(
        ir.model.product_definitions[0].bodies,
        [ir.geometry.bodies[0].id.clone()]
    );
    assert_eq!(
        outer
            .compose(ir.geometry.bodies[0].transform.unwrap())
            .unwrap()
            .rows(),
        [
            [0.0, -1.0, 0.0, 20.0],
            [1.0, 0.0, 0.0, 35.0],
            [0.0, 0.0, 1.0, 40.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
    );
}

fn merged_member(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    member: cadmpeg_ir::CadIr,
) -> cadmpeg_ir::CadIr {
    merged_member_in(ctx, member, "member")
}

fn merged_member_in(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    member: cadmpeg_ir::CadIr,
    occurrence: &str,
) -> cadmpeg_ir::CadIr {
    let mut ir = cadmpeg_ir::CadIr::empty();
    let mut scope = OccurrenceScope {
        native_text: &TextReplacements::default(),
        ctx,
        occurrence,
    };
    ir.geometry
        .extend_rewritten(member.geometry, &mut scope)
        .unwrap();
    ir.model.extend_rewritten(member.model, &mut scope).unwrap();
    ir
}

#[test]
fn saved_edge_loss_counts_final_archive_ownership_once() {
    use crate::loss::F3dLossCode;
    use crate::test_support::assembly_test::{f3d_without_brep, f3z_archive};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodeDepth, DecodePolicy};
    use cadmpeg_ir::codec::DecodeBody;
    use cadmpeg_ir::report::decode::DecodeTransfer;

    let root = f3d_without_brep("assembly-design", "root.f3d", &[]);
    let bytes = f3z_archive("root.f3d", &[("root.f3d", &root)]);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, bytes) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let scan = crate::container::scan(&ctx, bytes).unwrap();
    let session = crate::F3zSession::open_scanned(&ctx, scan.fork()).unwrap();
    let archive = &session.archive;
    for (has_definitions, owned_after_composition) in [(true, true), (true, false), (false, false)]
    {
        let mut member = cadmpeg_ir::CadIr::empty();
        member.source = Some(member_source());
        if has_definitions {
            crate::f3z::merge::define_member(&ctx, &mut member, "existing", "existing.f3d")
                .unwrap();
        }
        member.geometry.bodies = (0..2)
            .map(|index| Body {
                id: BodyId::mint(format!("f3d:brep:saved-edge-body#{index}")).unwrap(),
                kind: BodyKind::Wire,
                transform: None,
                name: None,
                color: None,
                visible: None,
                native_ref: None,
                unstated: None,
            })
            .collect();
        let member_loss = crate::decode::saved_free_edge_bodies_loss(&member).unwrap();
        let mut report = DecodeBody::new(DecodeTransfer::at(DecodeDepth::Full, true));
        report.losses = vec![member_loss.clone(), member_loss];
        let other_loss = F3dLossCode::XrefMemberMissing.note("missing member".to_owned());
        report.losses.push(other_loss.clone());
        if owned_after_composition {
            crate::f3z::merge::define_member(&ctx, &mut member, "member", "member.f3d").unwrap();
        }
        let mut ir = merged_member(&ctx, member);
        crate::f3z::merge::MergeSession {
            ctx: &ctx,
            scan: &scan,
            archive,
            session: &session,
            decoded: &mut std::collections::BTreeMap::new(),
            stack: vec!["root.f3d".to_owned()],
            members: std::collections::BTreeMap::new(),
            failed: std::collections::BTreeMap::new(),
            scene_sources: &mut crate::scene::Sources::new(),
            operation_poses: std::collections::BTreeMap::new(),
            operation_protected: std::collections::BTreeSet::new(),
            component_path_tables: Vec::new(),
            instance_scopes: &mut crate::f3z::merge::InstanceScopes::new(),
        }
        .merge(
            &mut ir,
            &mut report,
            &mut cadmpeg_ir::SourceFidelity::new(Sha256Digest::digest(b"")),
            &crate::xref::XrefTable::default(),
        )
        .unwrap();
        let saved_losses = report
            .losses
            .iter()
            .filter(|loss| loss.code == F3dLossCode::SavedFreeEdgeBodyUnowned.kind())
            .collect::<Vec<_>>();
        if owned_after_composition {
            assert!(saved_losses.is_empty());
        } else {
            assert_eq!(saved_losses.len(), 1);
            assert_eq!(saved_losses[0].message, "2 saved free-edge bodies have no product-definition owner; their component placement is not assigned.");
        }
        assert!(report.losses.contains(&other_loss));
    }
}

#[test]
fn a_member_definition_owns_the_top_level_of_its_file() {
    use cadmpeg_ir::ids::{OccurrenceId, ProductDefinitionId};
    use cadmpeg_ir::products::{Occurrence, Storage};

    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let mut member = cadmpeg_ir::examples::unit_cube().unwrap();
    member.model.product_definitions.clear();
    let top = OccurrenceId::mint("f3d:model:occurrence#xref-0-0").unwrap();
    let external = ProductDefinitionId::mint("f3d:xref:document#part.f3d").unwrap();
    member.model.occurrences = vec![Occurrence::new(
        top.clone(),
        external.clone(),
        Some(Transform::identity()),
    )];
    member
        .model
        .product_definitions
        .push(cadmpeg_ir::products::ProductDefinition::new(
            external,
            Storage::External(cadmpeg_ir::products::ExternalDocument::path("part.f3d")),
        ));
    crate::tree::place_top_level(&mut member.model, top.clone());
    member.source = Some(member_source());
    let definition =
        crate::f3z::merge::define_member(&ctx, &mut member, "Member", "member.f3d").unwrap();
    // §8.1: the member file's definition is stored in its own file of the
    // archive and holds the member's top level; no other definition is a
    // root.
    let row = member
        .model
        .product_definitions
        .iter()
        .find(|row| row.id == definition)
        .unwrap();
    assert_eq!(row.occurrences, [top]);
    assert!(matches!(
        &row.storage,
        Storage::File { document, .. }
            if document.path.as_ref().map(cadmpeg_core::text::NonBlankString::as_str) == Some("member.f3d")
    ));
    assert!(member
        .model
        .product_definitions
        .iter()
        .all(|row| !row.is_root()));
}

#[test]
fn repeated_occurrence_merge_remaps_typed_graphs_disjointly() {
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let mut merged = Topology::default();
    let component = Topology {
        bodies: vec![Body {
            id: BodyId::mint("f3d:brep:entity#1").expect("identity grammar"),
            kind: BodyKind::Solid,
            transform: None,
            name: None,
            color: None,
            visible: None,
            native_ref: None,
            unstated: None,
        }],
        regions: vec![Region::new(
            RegionId::mint("f3d:brep:entity#2").expect("identity grammar"),
            BodyId::mint("f3d:brep:entity#1").expect("identity grammar"),
            Vec::new(),
        )],
        ..Topology::default()
    };
    for ordinal in 0..2 {
        let occurrence = format!("role/occurrence-{ordinal}");
        let mut scope = OccurrenceScope {
            native_text: &TextReplacements::default(),
            ctx: &ctx,
            occurrence: &occurrence,
        };
        merged
            .extend_rewritten(component.clone(), &mut scope)
            .expect("merge component arenas");
    }

    for ordinal in 0..2 {
        let prefix = format!("f3d:role/occurrence-{ordinal}@brep:entity#");
        assert_eq!(merged.bodies[ordinal].id.as_str(), format!("{prefix}1"));
        assert_eq!(merged.regions[ordinal].id.as_str(), format!("{prefix}2"));
        assert_eq!(merged.regions[ordinal].body.as_str(), format!("{prefix}1"));
    }
}

#[test]
fn occurrence_merge_rewrites_only_member_native_record_text() {
    use cadmpeg_ir::document::EntityRewrite;
    use cadmpeg_ir::features::profiles::{PlanarProfileRef, ProfileRef};
    use cadmpeg_ir::NativeRecord;

    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let id = "f3d:Design1/BulkStream.dat:design-construction-operand-group#7";
    let mut native = Native::default();
    native
        .namespace_mut(cadmpeg_ir::codec_id!("f3d"))
        .arenas_mut()
        .insert(
            cadmpeg_core::nonblank_literal!("operands"),
            vec![NativeRecord::new(id.try_into().unwrap(), serde_json::Map::new()).unwrap()],
        );
    let targets = super::super::native_record_targets(&native, "member");
    let native_text = TextReplacements::build(targets.iter()).unwrap();
    let mut scope = OccurrenceScope {
        ctx: &ctx,
        occurrence: "member",
        native_text: &native_text,
    };
    for (source, expected) in [
        (
            id,
            "f3d:member@Design1/BulkStream.dat:design-construction-operand-group#7",
        ),
        (
            "f3d:Design1/BulkStream.dat:design-construction-operand-group#8",
            "f3d:Design1/BulkStream.dat:design-construction-operand-group#8",
        ),
    ] {
        let profile = ProfileRef::Planar(PlanarProfileRef::Native(source.into()));
        assert_eq!(
            scope.rewrite(profile).unwrap(),
            ProfileRef::Planar(PlanarProfileRef::Native(expected.into()))
        );
    }
}

#[test]
fn occurrence_merge_scopes_unmaterialized_partial_pattern_references() {
    use cadmpeg_ir::document::EntityRewrite;
    use cadmpeg_ir::features::patterns::{
        curve_driven::CurveDrivenPatternParameters, PatternKind, PatternSeed, PatternTransform,
    };
    use cadmpeg_ir::features::profiles::PathRef;

    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let source = "f3d:Design1/BulkStream.dat:design-record#91";
    let mut row = feature("f3d:model:feature#pattern", 0);
    row.name = Some(source.into());
    row.evaluation = cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
        FeatureDefinition::Operation(FeatureOperation::Pattern {
            seeds: vec![PatternSeed::Native(source.into())],
            pattern: PatternKind::new(PatternTransform::CurveDrivenPartial {
                parameters: CurveDrivenPatternParameters {
                    path: Some(PathRef::Native(source.into())),
                    ..Default::default()
                },
            })
            .unwrap(),
        }),
    );
    let mut model = Model::default();
    model.features.push(row.clone());
    for occurrence in ["left", "right"] {
        let targets = super::super::native_record_targets(&Native::default(), occurrence);
        let mut scoped_model = model.clone();
        super::super::rescope_partial_pattern_sources(&mut scoped_model, &|text| {
            super::super::rescope(text, occurrence)
        });
        let native_text = TextReplacements::build(targets.iter()).unwrap();
        let rewritten = OccurrenceScope {
            ctx: &ctx,
            occurrence,
            native_text: &native_text,
        }
        .rewrite(scoped_model.features.remove(0))
        .unwrap();
        assert_eq!(rewritten.name.as_deref(), Some(source));
        let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }) =
            rewritten.evaluation.definition()
        else {
            panic!("pattern")
        };
        let PatternTransform::CurveDrivenPartial { parameters } = pattern.definition() else {
            panic!("partial")
        };
        let expected = format!("f3d:{occurrence}@Design1/BulkStream.dat:design-record#91");
        assert_eq!(parameters.path, Some(PathRef::Native(expected.clone())));
        assert_eq!(seeds, &[PatternSeed::Native(expected)]);
    }
}

#[test]
fn occurrence_merge_preserves_a_body_name_that_spells_its_identity() {
    use cadmpeg_ir::document::EntityRewrite;
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let source_id = "f3d:model:body#source";
    let body = Body {
        id: BodyId::mint(source_id).unwrap(),
        kind: BodyKind::Solid,
        transform: None,
        name: Some(source_id.into()),
        color: None,
        visible: None,
        native_ref: None,
        unstated: None,
    };
    let scoped = OccurrenceScope {
        native_text: &TextReplacements::default(),
        ctx: &ctx,
        occurrence: "component-0",
    }
    .rewrite(body)
    .unwrap();
    assert_eq!(scoped.id.as_str(), "f3d:component-0@model:body#source");
    assert_eq!(scoped.name.as_deref(), Some(source_id));
}

#[test]
fn occurrence_merge_remaps_and_retains_native_records() {
    let placement = DesignSketchPlacement {
        display_name: None,
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
        .namespace_mut(cadmpeg_ir::codec_id!("f3d"))
        .set_arena(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            "design_sketch_placements",
            &[placement],
        )
        .expect("store component native");
    let mut root = Native::default();
    extend_native(
        &cadmpeg_core::decode::test_support::service_decode_context(),
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
        "f3d:role/occurrence-0@Design/BulkStream.dat:design-sketch-placement#42"
    );
}

#[test]
fn occurrence_configuration_survives_document_and_typed_native_admission() {
    use crate::records::configuration::{DesignConfiguration, DesignConfigurationKind};
    let configuration = DesignConfiguration::try_new(
        "Design/table.dsgcfg".into(),
        DesignConfigurationKind::Table,
        serde_json::Map::new(),
    )
    .unwrap();
    let mut component = Native::default();
    component
        .namespace_mut(cadmpeg_ir::codec_id!("f3d"))
        .set_arena(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            "design_configurations",
            &[configuration],
        )
        .unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty();
    extend_native(
        &cadmpeg_core::decode::test_support::service_decode_context(),
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
        "f3d:component-0@configuration:entry#Design/table.dsgcfg"
    );
    assert_eq!(configurations[0].payload(), &serde_json::Map::new());
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
        .namespace_mut(cadmpeg_ir::codec_id!("f3d"))
        .set_arena(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            "design_configurations",
            &[configuration],
        )
        .expect("store configuration");
    component
        .namespace_mut(cadmpeg_ir::codec_id!("f3d"))
        .set_arena(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            "body_visibilities",
            &[visibility],
        )
        .expect("store typed native reference");

    let mut root = Native::default();
    extend_native(
        &cadmpeg_core::decode::test_support::service_decode_context(),
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
        "f3d:role/occurrence-0@Design/BulkStream.dat:body-visibility#3"
    );
    assert_eq!(
        merged_visibility[0].body.as_str(),
        "f3d:role/occurrence-0@brep:entity#1"
    );
    assert_eq!(merged_visibility[0].stream(), "Design/BulkStream.dat");

    let merged_configurations: Vec<DesignConfiguration> = root
        .namespace("f3d")
        .expect("merged f3d namespace")
        .arena_as("design_configurations")
        .expect("read merged configuration arena");
    assert_eq!(
        merged_configurations[0].id(),
        "f3d:role/occurrence-0@configuration:entry#Design/table.dsgcfg"
    );
    assert_eq!(
        merged_configurations[0].payload(),
        &configuration_payload,
        "configuration text and extension map keys are source payload, not identities"
    );
}

#[test]
fn occurrence_key_preserves_authored_roles() {
    assert!(crate::records::xref::RequiredXrefText::try_from(String::new()).is_err());
    let reference = |role: &str, ordinal: u32| XrefReference {
        current_pose: None,
        operation_pose: None,
        contained_poses: Vec::new(),
        owned_poses: Vec::new(),
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
    assert_eq!(
        occurrence_key(&reference("pin@2", 7)),
        "role-pin%402/reference-7/occurrence-0"
    );
    let key = occurrence_key(&reference("role /#: value", 7));
    cadmpeg_ir::ids::Identity::new(format!("f3d:{key}@native:record#1"))
        .expect("encoded occurrence key remains an admitted identity scope");
}

#[test]
fn occurrence_key_separates_same_role_references_with_reset_ordinals() {
    let reference = |ordinal| XrefReference {
        current_pose: None,
        operation_pose: None,
        contained_poses: Vec::new(),
        owned_poses: Vec::new(),
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

    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
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
                native_text: &TextReplacements::default(),
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
            role: cadmpeg_ir::features::tree::FeatureTreeNodeRole::Equations,
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
    ir.model.product_definitions.extend([
        cadmpeg_ir::products::ProductDefinition::root(
            crate::tree::file_root_id(),
            cadmpeg_ir::products::ExternalDocument::default(),
        ),
        cadmpeg_ir::products::ProductDefinition::new(
            crate::ids::member_definition_id(None, "component-0").unwrap(),
            cadmpeg_ir::products::Storage::File {
                document: cadmpeg_ir::products::ExternalDocument::path("component.f3d"),
                source: Some(member_source()),
            },
        ),
    ]);
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
    let (mut model, _) = parameter_document(
        &cadmpeg_core::decode::test_support::service_decode_context(),
        1.0,
    );
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
                    source_kind: cadmpeg_core::nonblank_literal!("User Parameter"),
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
        .namespace_mut(cadmpeg_ir::codec_id!("f3d"))
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
            &|text| super::super::rescope(text, "part/occurrence-2"),
        )
        .unwrap();
    });
    assert_eq!(
        fields["placement_source"]["stream"],
        "f3d:part/occurrence-2@Design/BulkStream.dat"
    );
    assert_eq!(fields["placement_source"]["carrier_record_index"], 71);
    assert_eq!(fields["placement_source"]["owner"]["target"], 19);
    assert_eq!(
        fields["placement_source"]["owner"]["type_guid"],
        "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"
    );
}

#[test]
fn imported_definition_bodies_keep_their_definition_frame() {
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    ir.source = Some(member_source());
    ir.model.product_definitions.clear();
    let body = ir.geometry.bodies[0].id.clone();
    ir.model
        .product_definitions
        .push(cadmpeg_ir::products::ProductDefinition {
            id: cadmpeg_ir::ids::ProductDefinitionId::mint("f3d:model:component#owned").unwrap(),
            kind: Some(cadmpeg_ir::products::ProductDefinitionKind::Part),
            bodies: vec![body],
            source_name: None,
            label: None,
            description: None,
            part_number: None,
            bom_properties: std::collections::BTreeMap::new(),
            sketches: Vec::new(),
            native_ref: None,
            generated_from: None,
            storage: cadmpeg_ir::products::Storage::Embedded,
            occurrences: Vec::new(),
            instance_bodies: Vec::new(),
            representations: Vec::new(),
            units: None,
            general_tolerance: None,
            active: None,
        });
    let original = ir.geometry.bodies[0].transform;
    let mut free_body = ir.geometry.bodies[0].clone();
    free_body.id = BodyId::mint("f3d:model:body#free").unwrap();
    free_body.transform = Some(
        Transform::affine([
            [1.0, 0.0, 0.0, 20.0],
            [0.0, 1.0, 0.0, 30.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .unwrap(),
    );
    ir.geometry.bodies.push(free_body.clone());
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let definition =
        crate::f3z::merge::define_member(&ctx, &mut ir, "member", "member.f3d").unwrap();
    assert_eq!(ir.geometry.bodies[0].transform, original);
    assert_eq!(ir.geometry.bodies[1], free_body);
    assert_eq!(ir.model.product_definitions[1].id, definition);
    assert_eq!(ir.model.product_definitions[1].bodies, [free_body.id]);
}

#[test]
fn shared_members_check_cycles_for_each_instance_path() {
    use crate::test_support::assembly_test::{f3d_without_brep, f3z_archive};
    use cadmpeg_ir::codec::DecodeOptions;
    use std::io::Cursor;

    let root = f3d_without_brep(
        "assembly-design",
        "root.f3d",
        &[("first.f3d", "first-role"), ("second.f3d", "second-role")],
    );
    let first = f3d_without_brep(
        "assembly-design",
        "first.f3d",
        &[("second.f3d", "child-role")],
    );
    let second = f3d_without_brep(
        "assembly-design",
        "second.f3d",
        &[("first.f3d", "child-role")],
    );
    let archive = f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", &root),
            ("first.f3d", &first),
            ("second.f3d", &second),
        ],
    );
    let decoded = crate::test_support::archive_session::decode(
        &mut Cursor::new(archive),
        &DecodeOptions::default(),
    )
    .unwrap();
    // §8.1: each member is one definition, and its occurrence of the other
    // member is stated once, so the reference that closes the cycle stays
    // an External definition.
    let model = &decoded.ir().model;
    let members = model
        .product_definitions
        .iter()
        .filter(|definition| {
            matches!(
                definition.storage,
                cadmpeg_ir::products::Storage::File {
                    source: Some(_),
                    ..
                }
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(members.len(), 2);
    for member in &members {
        assert_eq!(member.occurrences.len(), 1, "{}", member.id);
    }
    assert_eq!(model.occurrences.len(), 4);
    let external = model
        .occurrences
        .iter()
        .filter(|occurrence| {
            model
                .product_definitions
                .iter()
                .find(|definition| definition.id == occurrence.prototype)
                .is_some_and(|definition| {
                    matches!(
                        definition.storage,
                        cadmpeg_ir::products::Storage::External(_)
                    )
                })
        })
        .count();
    assert_eq!(external, 1);
    assert_eq!(
        decoded
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == crate::F3dLossCode::XrefCycle.kind())
            .count(),
        2
    );
}

#[test]
fn a_repeated_member_states_its_joints_once() {
    use cadmpeg_ir::ids::{OccurrenceId, ProductDefinitionId};
    use cadmpeg_ir::products::JointId;
    use cadmpeg_ir::products::{
        AssemblyJoint, JointConnector, JointOperand, Occurrence, OperandContainer, PairedJointKind,
    };
    let definition = ProductDefinitionId::mint("f3d:model:product-definition#shared").unwrap();
    let child = OccurrenceId::mint("f3d:first@model:occurrence#child").unwrap();
    let native_ref = "f3d:first@Design/BulkStream.dat:design-parameter-scope#10";
    let mut joint = AssemblyJoint::paired(
        JointId::mint("f3d:first@model:joint#group").unwrap(),
        PairedJointKind::Fixed {
            angle: None,
            translation_offset: None,
            angular_limits: None,
            linear_limits: None,
        },
        [
            JointConnector {
                operand: JointOperand::root("root", Vec::new()),
                frame: Transform::identity(),
                detached: false,
            },
            JointConnector {
                operand: JointOperand::occurrence(child.clone(), "child", Vec::new()),
                frame: Transform::identity(),
                detached: false,
            },
        ],
        None,
    );
    joint.native_ref = Some(native_ref.to_owned());
    let member = crate::f3z::merge::MemberDefinition {
        definition: definition.clone(),
        scope: "first".into(),
        occurrences: vec![Occurrence {
            id: child,
            prototype: definition.clone(),
            transform: Some(Transform::identity()),
            suppressed: None,
            visible: None,
            representation: None,
            overrides: Vec::new(),
            name: None,
            native_ref: None,
        }],
        joints: vec![joint],
        table: crate::xref::XrefTable::default(),
    };
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let first_parent = OccurrenceId::mint("f3d:model:occurrence#first").unwrap();
    let second_parent = OccurrenceId::mint("f3d:model:occurrence#second").unwrap();
    let first = member.instantiate(&ctx, "first", &first_parent).unwrap();
    let second = member
        .instantiate(&ctx, "second/instance-child", &second_parent)
        .unwrap();
    assert_eq!(
        second.occurrences[0].id.as_str(),
        "f3d:second/instance-child@model:occurrence#child"
    );
    assert_eq!(
        first.assembly_joints[0].id.as_str(),
        "f3d:first@model:joint#group"
    );
    assert!(second.assembly_joints.is_empty());
    for instance in [&first, &second] {
        assert!(instance.product_definitions.is_empty());
        assert_eq!(instance.occurrences[0].prototype, definition);
    }
    assert_eq!(
        first.assembly_joints[0].native_ref.as_deref(),
        Some(native_ref)
    );
    let connectors = first.assembly_joints[0].connectors().collect::<Vec<_>>();
    assert_eq!(
        connectors[0].operand.container,
        OperandContainer::Occurrence {
            occurrence: first_parent,
            occurrence_path: Vec::new()
        }
    );
    assert_eq!(
        connectors[1].operand.container,
        OperandContainer::Occurrence {
            occurrence: first.occurrences[0].id.clone(),
            occurrence_path: Vec::new()
        }
    );
    assert!(connectors
        .iter()
        .all(|connector| connector.frame == Transform::identity()));
}

#[test]
fn a_joint_one_placement_of_a_repeated_member_binds_names_the_others() {
    use crate::loss::F3dLossCode;
    use cadmpeg_ir::ids::OccurrenceId;
    use cadmpeg_ir::products::{
        AssemblyJoint, JointConnector, JointId, JointOperand, Occurrence, PairedJointKind,
        ProductDefinition, Storage,
    };

    let definition = crate::ids::member_definition_id(None, "first").unwrap();
    let part =
        cadmpeg_ir::ids::ProductDefinitionId::mint("f3d:first@model:component#part").unwrap();
    let occurrence = |id: &str, prototype: &cadmpeg_ir::ids::ProductDefinitionId| {
        Occurrence::new(OccurrenceId::mint(id).unwrap(), prototype.clone(), None)
    };
    let first = occurrence("f3d:model:occurrence#first", &definition);
    let second = occurrence("f3d:model:occurrence#second", &definition);
    let child = occurrence("f3d:first@model:occurrence#child", &part);
    let other = occurrence("f3d:first@model:occurrence#other", &part);
    let joint = |name: &str, operands: [&Occurrence; 2]| {
        AssemblyJoint::paired(
            JointId::mint(format!("f3d:first@model:joint#{name}")).unwrap(),
            PairedJointKind::Fixed {
                angle: None,
                translation_offset: None,
                angular_limits: None,
                linear_limits: None,
            },
            operands.map(|operand| JointConnector {
                operand: JointOperand::occurrence(operand.id.clone(), "object", Vec::new()),
                frame: Transform::identity(),
                detached: false,
            }),
            None,
        )
    };
    let mut model = Model::default();
    model.product_definitions = vec![
        ProductDefinition::new(
            definition.clone(),
            Storage::File {
                document: cadmpeg_ir::products::ExternalDocument::path("first.f3d"),
                source: Some(member_source()),
            },
        ),
        ProductDefinition::new(part.clone(), Storage::Embedded),
    ];
    model.occurrences = vec![first.clone(), second.clone(), child.clone(), other.clone()];
    model.assembly_joints = vec![
        joint("bound", [&first, &child]),
        joint("inside", [&child, &other]),
    ];

    let losses = crate::f3z::merge::charge_placement_bound_joints(&model);
    assert_eq!(losses.len(), 1, "{losses:#?}");
    assert_eq!(
        losses[0].code,
        F3dLossCode::AssemblyMemberJointPlacementBound.kind()
    );
    assert_eq!(
        losses[0].message,
        "joint f3d:first@model:joint#bound names an operand of the root of member \
         f3d:xref:product-definition#first through placement f3d:model:occurrence#first; \
         placement(s) f3d:model:occurrence#second of that member do not state it"
    );

    model.occurrences.retain(|row| row.id != second.id);
    assert!(crate::f3z::merge::charge_placement_bound_joints(&model).is_empty());
}

#[test]
fn merged_joint_descent_selects_unique_member_children_and_retains_ambiguous_paths() {
    use crate::f3z::merge::{define_member, MemberDefinition, MergeSession};
    use crate::loss::F3dLossCode;
    use crate::records::feature::assembly_features::{
        DesignComponentOccurrence, DesignComponentOccurrenceDraft,
        DesignComponentOccurrencePlacement,
    };
    use crate::test_support::assembly_test::{f3d_without_brep, f3z_archive};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodeDepth, DecodePolicy};
    use cadmpeg_ir::ids::OccurrenceId;
    use cadmpeg_ir::products::{
        AssemblyJoint, ExternalDocument, JointConnector, JointId, JointOperand, Occurrence,
        OperandContainer, PairedJointKind,
    };
    use cadmpeg_ir::report::decode::DecodeTransfer;

    const FIRST: &str = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
    const SECOND: &str = "ffffffff-bbbb-4ccc-8ddd-eeeeeeeeeeee";
    const ROOT_KEY: &str = "11111111-bbbb-4ccc-8ddd-eeeeeeeeeeee";
    let root = f3d_without_brep("assembly-design", "root.f3d", &[]);
    let bytes = f3z_archive("root.f3d", &[("root.f3d", &root)]);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, bytes) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let scan = crate::container::scan(&ctx, bytes).unwrap();
    let session = crate::F3zSession::open_scanned(&ctx, scan.fork()).unwrap();
    let archive = &session.archive;
    let reference: XrefReference = serde_json::from_value(serde_json::json!({
        "id":"f3d:xref:reference#0", "ordinal":0, "from":"root.f3d",
        "relative_path":"member.f3d", "neutron_role":"member", "neutron_data":""
    }))
    .unwrap();
    let table = crate::xref::XrefTable {
        references: vec![reference.clone()],
        ..Default::default()
    };
    for (keys, duplicate, keyed_root, target, remaining) in [
        (
            vec![FIRST.to_ascii_uppercase(), SECOND.into(), "face".into()],
            false,
            false,
            Some("deep"),
            vec!["face"],
        ),
        (
            vec![FIRST.into(), SECOND.into()],
            true,
            false,
            None,
            vec![FIRST, SECOND],
        ),
        (vec!["missing".into()], false, false, None, vec!["missing"]),
        (
            vec![FIRST.into(), "missing".into()],
            false,
            false,
            Some("child"),
            vec!["missing"],
        ),
        // A member root that has its own key is a keyed child, not a
        // passthrough: its key is consumed.
        (
            vec![ROOT_KEY.into(), FIRST.into(), SECOND.into()],
            false,
            true,
            Some("deep"),
            Vec::new(),
        ),
        (vec![FIRST.into()], false, true, None, vec![FIRST]),
    ] {
        let connector = |operand| JointConnector {
            operand,
            frame: Transform::identity(),
            detached: false,
        };
        let joint = |name, operand, other| {
            AssemblyJoint::paired(
                JointId::mint(format!("f3d:model:joint#{name}")).unwrap(),
                PairedJointKind::Fixed {
                    angle: None,
                    translation_offset: None,
                    angular_limits: None,
                    linear_limits: None,
                },
                [connector(operand), connector(other)],
                None,
            )
        };
        let mut component = cadmpeg_ir::CadIr::empty();
        component.source = Some(member_source());
        let definition = define_member(&ctx, &mut component, "member", "member.f3d").unwrap();
        let component_of = |name: &str| {
            cadmpeg_ir::ids::ProductDefinitionId::mint(format!("f3d:model:component#{name}"))
                .unwrap()
        };
        let occurrence = |name: &str, native: Option<&str>| {
            let mut occurrence = Occurrence::new(
                OccurrenceId::mint(format!("f3d:model:occurrence#{name}")).unwrap(),
                component_of(name),
                Some(Transform::identity()),
            );
            occurrence.native_ref = native.map(str::to_owned);
            occurrence
        };
        for name in ["root", "child", "deep", "duplicate"] {
            component
                .model
                .product_definitions
                .push(cadmpeg_ir::products::ProductDefinition::new(
                    component_of(name),
                    cadmpeg_ir::products::Storage::Embedded,
                ));
        }
        let mut owners = vec![(definition.clone(), "root")];
        let mut carriers = Vec::new();
        let root_native = "f3d:Design/BulkStream.dat:design-component-occurrence#4";
        component
            .model
            .occurrences
            .push(occurrence("root", keyed_root.then_some(root_native)));
        if keyed_root {
            carriers.push(
                DesignComponentOccurrence::try_new(DesignComponentOccurrenceDraft {
                    id: root_native.to_owned(),
                    class_tag: "292".to_owned().try_into().unwrap(),
                    record_index: 4,
                    byte_offset: 0,
                    component_record_index: 1,
                    component_guid: FIRST.to_owned().try_into().unwrap(),
                    occurrence_guid: ROOT_KEY.to_owned().try_into().unwrap(),
                    placement: DesignComponentOccurrencePlacement::Base,
                    owner_entity: 1,
                })
                .unwrap(),
            );
        }
        for (index, name, parent, key) in [
            (1, "child", "root", FIRST),
            (2, "deep", "child", SECOND),
            (3, "duplicate", "child", SECOND),
        ] {
            if name == "duplicate" && !duplicate {
                continue;
            }
            let stream = if name == "duplicate" {
                "OtherDesign"
            } else {
                "Design"
            };
            let id = format!("f3d:{stream}/BulkStream.dat:design-component-occurrence#{index}");
            component
                .model
                .occurrences
                .push(occurrence(name, Some(&id)));
            owners.push((component_of(parent), name));
            carriers.push(
                DesignComponentOccurrence::try_new(DesignComponentOccurrenceDraft {
                    id,
                    class_tag: "292".to_owned().try_into().unwrap(),
                    record_index: index,
                    byte_offset: 0,
                    component_record_index: 1,
                    component_guid: FIRST.to_owned().try_into().unwrap(),
                    occurrence_guid: key.to_owned().try_into().unwrap(),
                    placement: DesignComponentOccurrencePlacement::Base,
                    owner_entity: 1,
                })
                .unwrap(),
            );
        }
        let external =
            JointOperand::external(ExternalDocument::path("other.f3d"), "object", keys.clone());
        component.model.assembly_joints.push(joint(
            "member",
            JointOperand::root("object", keys.clone()),
            external.clone(),
        ));
        for (owner, name) in owners {
            crate::tree::place_in(
                &mut component.model,
                &owner,
                OccurrenceId::mint(format!("f3d:model:occurrence#{name}")).unwrap(),
            );
        }
        let member_scope = occurrence_key(&reference);
        let mut ir = merged_member_in(&ctx, component, &member_scope);
        let member = MemberDefinition {
            definition: ir.model.product_definitions[0].id.clone(),
            scope: member_scope.clone(),
            occurrences: std::mem::take(&mut ir.model.occurrences),
            joints: std::mem::take(&mut ir.model.assembly_joints),
            table: crate::xref::XrefTable::default(),
        };
        let mut native = Native::default();
        native
            .namespace_mut(cadmpeg_ir::codec_id!("f3d"))
            .set_arena(&ctx, "design_component_occurrences", &carriers)
            .unwrap();
        extend_native(&ctx, &mut ir.native, native, &member_scope).unwrap();
        ir.native
            .namespace_mut(cadmpeg_ir::codec_id!("f3d"))
            .set_arena(&ctx, "xref_references", &table.references)
            .unwrap();
        let placing = crate::ids::neutral_xref_occurrence_id(0, 0);
        let document = crate::xref::document_definition_id("member.f3d");
        ir.model
            .product_definitions
            .push(cadmpeg_ir::products::ProductDefinition::new(
                document.clone(),
                cadmpeg_ir::products::Storage::External(ExternalDocument::path("member.f3d")),
            ));
        let mut placing_occurrence =
            Occurrence::new(placing.clone(), document, Some(Transform::identity()));
        placing_occurrence.native_ref = Some(reference.id.clone());
        ir.model.occurrences.push(placing_occurrence);
        crate::tree::place_top_level(&mut ir.model, placing.clone());
        let root_operand = JointOperand::root("object", keys.clone());
        ir.model.assembly_joints.push(joint(
            "root",
            JointOperand::occurrence(placing.clone(), "object", keys.clone()),
            root_operand.clone(),
        ));
        let mut report =
            cadmpeg_ir::codec::DecodeBody::new(DecodeTransfer::at(DecodeDepth::Full, false));
        assert_eq!(
            MergeSession {
                ctx: &ctx,
                scan: &scan,
                archive,
                session: &session,
                decoded: &mut std::collections::BTreeMap::new(),
                stack: vec!["root.f3d".into()],
                members: std::collections::BTreeMap::from([("member.f3d".into(), member)]),
                failed: std::collections::BTreeMap::new(),
                scene_sources: &mut crate::scene::Sources::new(),
                operation_poses: std::collections::BTreeMap::new(),
                operation_protected: std::collections::BTreeSet::new(),
                component_path_tables: Vec::new(),
                instance_scopes: &mut crate::f3z::merge::InstanceScopes::new(),
            }
            .merge(
                &mut ir,
                &mut report,
                &mut cadmpeg_ir::SourceFidelity::new(Sha256Digest::digest(b"")),
                &table
            )
            .unwrap(),
            1
        );
        assert_eq!(ir.model.assembly_joints.len(), 2);
        let scope = occurrence_key(&reference);
        let selected = target.map_or_else(
            || placing.clone(),
            |name| OccurrenceId::mint(format!("f3d:{scope}@model:occurrence#{name}")).unwrap(),
        );
        // A member joint's paths start below its member root, so the keyed
        // root rows check only the placing document's joint.
        let checked_joints = if keyed_root { 1 } else { 2 };
        for joint in ir.model.assembly_joints.iter().take(checked_joints) {
            let operand = &joint.connectors().next().unwrap().operand;
            assert_eq!(
                operand.container,
                OperandContainer::Occurrence {
                    occurrence: selected.clone(),
                    occurrence_path: Vec::new(),
                }
            );
            assert_eq!(operand.object, "object");
            assert_eq!(operand.subelements, remaining);
        }
        assert_eq!(
            ir.model.assembly_joints[0]
                .connectors()
                .nth(1)
                .unwrap()
                .operand,
            root_operand
        );
        assert_eq!(
            ir.model.assembly_joints[1]
                .connectors()
                .nth(1)
                .unwrap()
                .operand,
            external
        );
        let ambiguous = report
            .losses
            .iter()
            .filter(|loss| loss.code == F3dLossCode::AssemblyOperandDescentAmbiguous.kind())
            .count();
        assert_eq!(ambiguous, if duplicate { 2 } else { 0 });
        ir.finalize();
        let checked = cadmpeg_ir::validate::validate_neutral(&ir, Vec::new()).unwrap();
        assert!(checked.is_ok(), "{:?}", checked.findings);
    }
}

#[test]
fn product_history_owner_rescopes_feature_and_parameter_links() {
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let mut document = cadmpeg_ir::CadIr::empty();
    document.source = Some(member_source());
    document.model = parameter_document(&ctx, 1.0).0;
    let owner = crate::f3z::merge::define_member(&ctx, &mut document, "synthetic", "synthetic.f3d")
        .unwrap();
    let mut component = document.model;
    let mut row = feature("f3d:model:feature#owned", 0);
    row.product_definition = Some(owner.clone());
    component.features.push(row);
    for parameter in &mut component.parameters {
        parameter.product_definition = Some(owner.clone());
    }
    super::super::scope_document_parameters(&ctx, &mut component).unwrap();
    append_feature_history(&Model::default(), &mut component).unwrap();
    let mut output = Model::default();
    output
        .extend_rewritten(
            component,
            &mut OccurrenceScope {
                native_text: &TextReplacements::default(),
                ctx: &ctx,
                occurrence: "left",
            },
        )
        .unwrap();
    let rescoped = &output.product_definitions[0].id;
    assert_ne!(rescoped, &owner);
    assert!(output
        .features
        .iter()
        .all(|row| row.product_definition.as_ref() == Some(rescoped)));
    assert!(output
        .parameters
        .iter()
        .all(|row| row.product_definition.as_ref() == Some(rescoped)));
}

#[test]
fn a_refused_joint_leaves_its_feature_native() {
    use cadmpeg_ir::features::FeatureOperation;
    use cadmpeg_ir::products::{
        AssemblyJoint, JointConnector, JointId, JointOperand, PairedJointKind,
    };
    let joint = |id: &str, scope: &str| {
        let mut joint = AssemblyJoint::paired(
            JointId::mint(id).unwrap(),
            PairedJointKind::Fixed {
                angle: None,
                translation_offset: None,
                angular_limits: None,
                linear_limits: None,
            },
            [
                JointConnector {
                    operand: JointOperand::root("first", Vec::new()),
                    frame: Transform::identity(),
                    detached: false,
                },
                JointConnector {
                    operand: JointOperand::root("second", Vec::new()),
                    frame: Transform::identity(),
                    detached: false,
                },
            ],
            None,
        );
        joint.native_ref = Some(scope.to_owned());
        joint
    };
    let mut retained_feature = feature("f3d:model:feature#retained", 0);
    retained_feature.native_ref = Some("f3d:scope#retained".to_owned());
    let mut refused_feature = feature("f3d:model:feature#refused", 1);
    refused_feature.native_ref = Some("f3d:scope#refused".to_owned());
    let mut features = [retained_feature, refused_feature];
    let complete = std::collections::BTreeSet::from([
        "f3d:scope#retained".to_owned(),
        "f3d:scope#refused".to_owned(),
    ]);
    super::super::bind_retained_joint_features(
        &mut features,
        &[joint("f3d:model:joint#retained", "f3d:scope#retained")],
        &complete,
    );
    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::AssemblyJoint { .. })
    ));
    assert!(matches!(
        features[1].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Native { .. })
    ));
}

/// A standalone model with one insertion occurrence, one `Assemble` feature
/// and one joint of the same scope whose first operand descends by `keys`.
fn standalone_joint_model(keys: Vec<String>) -> (Model, cadmpeg_ir::ids::OccurrenceId) {
    use cadmpeg_ir::ids::OccurrenceId;
    use cadmpeg_ir::products::{
        AssemblyJoint, JointConnector, JointId, JointOperand, Occurrence, PairedJointKind,
        ProductDefinition, Storage,
    };
    let insertion = OccurrenceId::mint("f3d:model:occurrence#insertion").unwrap();
    let mut joint = AssemblyJoint::paired(
        JointId::mint("f3d:model:joint#standalone").unwrap(),
        PairedJointKind::Fixed {
            angle: None,
            translation_offset: None,
            angular_limits: None,
            linear_limits: None,
        },
        [
            JointConnector {
                operand: JointOperand::occurrence(
                    insertion.clone(),
                    "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee",
                    keys,
                ),
                frame: Transform::identity(),
                detached: false,
            },
            JointConnector {
                operand: JointOperand::root("root", Vec::new()),
                frame: Transform::identity(),
                detached: false,
            },
        ],
        None,
    );
    joint.native_ref = Some("f3d:scope#standalone".to_owned());
    let mut assemble = feature("f3d:model:feature#standalone", 0);
    assemble.native_ref = Some("f3d:scope#standalone".to_owned());
    let mut model = Model::default();
    model.features = vec![assemble];
    model.assembly_joints = vec![joint];
    let unresolved =
        cadmpeg_ir::ids::ProductDefinitionId::mint(format!("{insertion}/unresolved")).unwrap();
    model.product_definitions = vec![ProductDefinition::new(
        unresolved.clone(),
        Storage::External(cadmpeg_ir::products::ExternalDocument::default()),
    )];
    model.occurrences = vec![Occurrence::new(
        insertion.clone(),
        unresolved,
        Some(Transform::identity()),
    )];
    crate::tree::place_top_level(&mut model, insertion.clone());
    (model, insertion)
}

#[test]
fn standalone_joint_of_a_complete_scope_binds_its_feature() {
    use cadmpeg_ir::features::FeatureOperation;
    let (mut model, insertion) = standalone_joint_model(Vec::new());
    let complete = std::collections::BTreeSet::from(["f3d:scope#standalone".to_owned()]);
    let children = std::collections::BTreeMap::new();
    let mut losses = Vec::new();
    super::super::rebind_joints(
        &mut super::super::JointRows {
            occurrences: &model.occurrences,
            joints: &mut model.assembly_joints,
            features: &mut model.features,
        },
        &children,
        &children,
        &complete,
        super::super::JointRebind::CompleteScopes,
        &mut losses,
    );
    assert!(losses.is_empty());
    assert_eq!(model.assembly_joints.len(), 1);
    assert!(model
        .assembly_joints
        .iter()
        .flat_map(cadmpeg_ir::products::AssemblyJoint::connectors)
        .any(|connector| matches!(
            &connector.operand.container,
            cadmpeg_ir::products::OperandContainer::Occurrence { occurrence, .. }
                if occurrence == &insertion
        )));
    assert!(matches!(
        model.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::AssemblyJoint { .. })
    ));
}

#[test]
fn standalone_joint_inside_an_unmerged_document_keeps_a_nested_occurrence_path() {
    use cadmpeg_ir::features::FeatureOperation;
    use cadmpeg_ir::products::OperandContainer;
    const NESTED: &str = "ffffffff-bbbb-4ccc-8ddd-eeeeeeeeeeee";
    let (mut model, insertion) = standalone_joint_model(vec![NESTED.into()]);
    let complete = std::collections::BTreeSet::from(["f3d:scope#standalone".to_owned()]);
    let children = std::collections::BTreeMap::new();
    let mut losses = Vec::new();
    super::super::rebind_joints(
        &mut super::super::JointRows {
            occurrences: &model.occurrences,
            joints: &mut model.assembly_joints,
            features: &mut model.features,
        },
        &children,
        &children,
        &complete,
        super::super::JointRebind::CompleteScopes,
        &mut losses,
    );
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::F3dLossCode::AssemblyOperandNestedPathRetained.kind()
    );
    let operand = &model.assembly_joints[0]
        .connectors()
        .next()
        .unwrap()
        .operand;
    assert_eq!(
        operand.container,
        OperandContainer::Occurrence {
            occurrence: insertion,
            occurrence_path: vec![NESTED.to_owned()],
        }
    );
    assert!(operand.subelements.is_empty());
    assert!(matches!(
        model.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::AssemblyJoint { .. })
    ));
}

#[test]
fn standalone_joint_with_an_unknown_merged_child_key_keeps_its_decoded_operand() {
    use cadmpeg_ir::features::FeatureOperation;
    use cadmpeg_ir::ids::OccurrenceId;
    let (mut model, insertion) =
        standalone_joint_model(vec!["ffffffff-bbbb-4ccc-8ddd-eeeeeeeeeeee".into()]);
    let decoded = model.assembly_joints.clone();
    let complete = std::collections::BTreeSet::from(["f3d:scope#standalone".to_owned()]);
    let child = OccurrenceId::mint("f3d:model:occurrence#child").unwrap();
    let children = std::collections::BTreeMap::from([(
        (insertion, "11111111-bbbb-4ccc-8ddd-eeeeeeeeeeee".to_owned()),
        Some(&child),
    )]);
    let mut losses = Vec::new();
    super::super::rebind_joints(
        &mut super::super::JointRows {
            occurrences: &model.occurrences,
            joints: &mut model.assembly_joints,
            features: &mut model.features,
        },
        &children,
        &children,
        &complete,
        super::super::JointRebind::CompleteScopes,
        &mut losses,
    );
    assert_eq!(model.assembly_joints, decoded);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::F3dLossCode::AssemblyJointSourceBindingAssumed.kind()
    );
    assert!(matches!(
        model.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::AssemblyJoint { .. })
    ));
}

#[test]
fn complete_joint_descent_consumes_occurrence_keys_in_each_insertion() {
    use cadmpeg_ir::ids::OccurrenceId;
    use cadmpeg_ir::products::{
        AssemblyJoint, JointConnector, JointId, JointOperand, OperandContainer, PairedJointKind,
    };
    const FIRST: &str = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
    const SECOND: &str = "ffffffff-bbbb-4ccc-8ddd-eeeeeeeeeeee";
    let id = |key: &str| OccurrenceId::mint(format!("f3d:model:occurrence#{key}")).unwrap();
    let roots = [id("insertion-a"), id("insertion-b")];
    let middles = [id("child-a"), id("child-b")];
    let leaves = [id("leaf-a"), id("leaf-b")];
    let children = std::collections::BTreeMap::from([
        ((roots[0].clone(), FIRST.to_owned()), Some(&middles[0])),
        ((roots[1].clone(), FIRST.to_owned()), Some(&middles[1])),
        ((middles[0].clone(), SECOND.to_owned()), Some(&leaves[0])),
        ((middles[1].clone(), SECOND.to_owned()), Some(&leaves[1])),
    ]);
    let instances = roots
        .iter()
        .chain(&middles)
        .chain(&leaves)
        .collect::<std::collections::BTreeSet<_>>();
    for insertion in 0..2 {
        let make_joint = |keys: Vec<String>| {
            AssemblyJoint::paired(
                JointId::mint("f3d:model:joint#descent").unwrap(),
                PairedJointKind::Fixed {
                    angle: None,
                    translation_offset: None,
                    angular_limits: None,
                    linear_limits: None,
                },
                [
                    JointConnector {
                        operand: JointOperand::occurrence(roots[insertion].clone(), FIRST, keys),
                        frame: Transform::identity(),
                        detached: false,
                    },
                    JointConnector {
                        operand: JointOperand::root("root", Vec::new()),
                        frame: Transform::identity(),
                        detached: false,
                    },
                ],
                None,
            )
        };
        let mut joint = make_joint(vec![FIRST.to_ascii_uppercase(), SECOND.to_owned()]);
        let frames = joint.connectors().map(|c| c.frame).collect::<Vec<_>>();
        let mut losses = Vec::new();
        assert!(super::super::retain_complete_joint(
            &mut joint,
            &children,
            &instances,
            super::super::JointRebind::All,
            &mut losses
        ));
        let operand = &joint.connectors().next().unwrap().operand;
        assert_eq!(
            operand.container,
            OperandContainer::Occurrence {
                occurrence: leaves[insertion].clone(),
                occurrence_path: Vec::new()
            }
        );
        assert!(operand.subelements.is_empty());
        assert_eq!(
            joint.connectors().map(|c| c.frame).collect::<Vec<_>>(),
            frames
        );
        assert!(losses.is_empty());
        let mut external = make_joint(Vec::new());
        external.connectors_mut().next().unwrap().operand = JointOperand::external(
            cadmpeg_ir::products::ExternalDocument::path("missing.f3d"),
            FIRST,
            Vec::new(),
        );
        let mut losses = Vec::new();
        assert!(!super::super::retain_complete_joint(
            &mut external,
            &children,
            &instances,
            super::super::JointRebind::All,
            &mut losses
        ));
        assert_eq!(
            losses[0].code,
            crate::loss::F3dLossCode::AssemblyOperandDescentUnresolved.kind()
        );
        let mut absent = make_joint(Vec::new());
        absent.connectors_mut().next().unwrap().operand.container = OperandContainer::Occurrence {
            occurrence: id("absent"),
            occurrence_path: Vec::new(),
        };
        absent.connectors_mut().nth(1).unwrap().operand = JointOperand::external(
            cadmpeg_ir::products::ExternalDocument::document_id(FIRST),
            SECOND,
            Vec::new(),
        );
        let mut losses = Vec::new();
        assert!(!super::super::retain_complete_joint(
            &mut absent,
            &children,
            &instances,
            super::super::JointRebind::All,
            &mut losses
        ));
        assert_eq!(
            losses[0].code,
            crate::loss::F3dLossCode::AssemblyOperandDescentUnresolved.kind()
        );
        for (keys, ambiguous) in [
            (vec![FIRST.to_owned(), "missing".to_owned()], false),
            (vec![FIRST.to_owned(), SECOND.to_owned()], true),
        ] {
            let mut invalid = make_joint(keys);
            let mut missing = children.clone();
            if ambiguous {
                missing.insert((middles[insertion].clone(), SECOND.to_owned()), None);
            }
            let mut losses = Vec::new();
            assert!(!super::super::retain_complete_joint(
                &mut invalid,
                &missing,
                &instances,
                super::super::JointRebind::All,
                &mut losses
            ));
            assert_eq!(losses.len(), 1);
            assert_eq!(
                losses[0].code,
                crate::loss::F3dLossCode::AssemblyOperandDescentUnresolved.kind()
            );
        }
    }
}

#[test]
fn qualified_external_joint_binds_without_changing_source_operands() {
    use cadmpeg_ir::products::{ExternalDocument, JointOperand};
    let (mut model, _) = standalone_joint_model(Vec::new());
    for (ordinal, connector) in model.assembly_joints[0].connectors_mut().enumerate() {
        connector.operand = JointOperand::external(
            ExternalDocument::document_id("11111111-2222-3333-4444-555555555555"),
            format!("source-occurrence-{ordinal}"),
            vec!["nested-source-occurrence".into()],
        );
    }
    let decoded = model.assembly_joints.clone();
    let complete = std::collections::BTreeSet::from(["f3d:scope#standalone".to_owned()]);
    let children = std::collections::BTreeMap::new();
    for mode in [
        super::super::JointRebind::CompleteScopes,
        super::super::JointRebind::All,
    ] {
        let mut model = model.clone();
        let mut losses = Vec::new();
        super::super::rebind_joints(
            &mut super::super::JointRows {
                occurrences: &model.occurrences,
                joints: &mut model.assembly_joints,
                features: &mut model.features,
            },
            &children,
            &children,
            &complete,
            mode,
            &mut losses,
        );
        assert_eq!(model.assembly_joints, decoded);
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            crate::loss::F3dLossCode::AssemblyJointSourceBindingAssumed.kind()
        );
        assert_eq!(
            model.features[0].evaluation.definition(),
            &FeatureDefinition::Operation(FeatureOperation::AssemblyJoint {
                joint: decoded[0].id.clone(),
            })
        );
    }
}
