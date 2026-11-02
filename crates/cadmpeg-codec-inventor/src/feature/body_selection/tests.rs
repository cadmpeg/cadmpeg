// SPDX-License-Identifier: Apache-2.0
use super::*;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::features::{
    BooleanKind, FeatureResultMembers, FeatureResultTopology, NativeSelections,
    SplitBodyTool,
};
use cadmpeg_ir::ids::FeatureResultTopologyId;

pub(crate) fn body(value: &str) -> BodyId {
    BodyId::mint(format!("test:model:body#{value}")).expect("synthetic body id")
}

fn joins() -> HashMap<String, BodyId> {
    [
        ("target".into(), body("body:target")),
        ("tool".into(), body("body:tool")),
        ("other".into(), body("body:other")),
    ]
    .into()
}

fn combine(tools: BodySelection) -> FeatureDefinition {
    FeatureDefinition::Operation(FeatureOperation::Combine {
        operands: CombineOperands::new(BodySelection::Native("target".into()), tools)
            .expect("distinct operands"),
        op: BooleanKind::Cut,
        keep_tools: false,
    })
}

#[test]
fn combine_resolves_single_and_multiple_tools_and_retains_the_native_selection() {
    for tools in [
        BodySelection::Native("tool".into()),
        BodySelection::NativeSet(
            NativeSelections::try_from(vec!["tool".into(), "other".into()])
                .expect("distinct wrappers"),
        ),
    ] {
        let source = combine(tools);
        let mut actual = source.clone();
        definition(&mut actual, &joins(), Some("tools"));
        let FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) = &actual
        else {
            panic!("combine")
        };
        let (target, tools) = operands.clone().into_parts();
        assert_eq!(
            target,
            BodySelection::Resolved {
                bodies: vec![body("body:target")].try_into().expect("distinct"),
                native: "target".into()
            }
        );
        let BodySelection::Resolved { bodies, native } = tools else {
            panic!("resolved tools")
        };
        assert_eq!(bodies[0], body("body:tool"));
        assert_eq!(native, if bodies.len() == 1 { "tool" } else { "tools" });
        assert!(matches(source.clone(), &actual, &joins(), Some("tools")));
        let mut wrong = joins();
        wrong.insert("target".into(), body("body:other"));
        assert!(!matches(source.clone(), &actual, &wrong, Some("tools")));
        assert!(!matches(source, &actual, &HashMap::new(), Some("tools")));
    }
}

#[test]
fn partial_tools_and_aliased_tools_keep_the_whole_tool_selection_native() {
    let tools = BodySelection::NativeSet(
        NativeSelections::try_from(vec!["tool".into(), "other".into()]).expect("distinct wrappers"),
    );
    for missing in ["other", "tool"] {
        let mut bodies = joins();
        bodies.remove(missing);
        let mut actual = combine(tools.clone());
        definition(&mut actual, &bodies, Some("tools"));
        let FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) = actual
        else {
            panic!("combine")
        };
        assert_eq!(operands.into_parts().1, tools);
    }
    let mut bodies = joins();
    bodies.insert("other".into(), body("body:tool"));
    let mut actual = combine(tools.clone());
    definition(&mut actual, &bodies, Some("tools"));
    let FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) = actual else {
        panic!("combine")
    };
    assert_eq!(operands.into_parts().1, tools);
}

#[test]
fn split_and_body_motion_resolve_and_validate_only_complete_distinct_selections() {
    let plane = FeatureId::mint("test:model:feature#plane").expect("synthetic plane");
    let split = FeatureDefinition::Operation(FeatureOperation::SplitBody {
        targets: BodySelection::Native("target".into()),
        tool: SplitBodyTool::Plane { plane },
    });
    let motion = FeatureDefinition::Operation(FeatureOperation::MoveBody {
        bodies: BodySelection::local(vec!["target".into(), "tool".into()], "selection".into())
            .expect("distinct wrappers"),
        translation: [cadmpeg_ir::Dimension::Literal(cadmpeg_ir::scalar::Length::ZERO), cadmpeg_ir::Dimension::Literal(cadmpeg_ir::scalar::Length::ZERO), cadmpeg_ir::Dimension::Literal(cadmpeg_ir::scalar::Length::ZERO)],
        rotation: None,
        copies: cadmpeg_ir::Dimension::Literal(0),
    });
    let surface_split = FeatureDefinition::Operation(FeatureOperation::SplitBody {
        targets: BodySelection::Native("target".into()),
        tool: SplitBodyTool::Faces(cadmpeg_ir::features::FaceSelection::Native(
            "surface".into(),
        )),
    });
    for source in [split, surface_split, motion] {
        let mut actual = source.clone();
        definition(&mut actual, &joins(), None);
        assert_ne!(source, actual);
        assert!(matches(source.clone(), &actual, &joins(), None));
        assert!(!matches(source.clone(), &source, &joins(), None));
        assert!(!matches(source.clone(), &actual, &HashMap::new(), None));
        let mut wrong = joins();
        wrong.insert("target".into(), body("body:other"));
        assert!(!matches(source.clone(), &actual, &wrong, None));
        let mut refused = source.clone();
        definition(&mut refused, &HashMap::new(), None);
        assert_eq!(source, refused);
    }
}

#[test]
fn unresolved_result_wrappers_emit_the_pinned_loss() {
    let mut model = Model::default();
    model
        .feature_result_topologies
        .push(FeatureResultTopology::new(
            FeatureResultTopologyId::mint("test:model:feature-result#result")
                .expect("synthetic result"),
            FeatureId::mint("test:model:feature#feature").expect("synthetic feature"),
            FeatureResultMembers::new(
                vec![NonBlankString::new("target").expect("wrapper")],
                vec![],
                vec![],
                vec![],
            )
            .expect("members"),
            Some("selection".into()),
        ));
    let losses = native_losses(&model, &HashMap::new());
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "feature.body-selection-native");
    assert_eq!(
        losses[0].code,
        crate::loss::InventorLossCode::FeatureBodySelectionNative.kind()
    );
    assert!(native_losses(&model, &joins()).is_empty());
    let feature = model.feature_result_topologies[0].output_of.clone();
    assert!(
        outputs(&feature, &model.feature_result_topologies, &HashMap::new())
            .expect("one result")
            .is_empty()
    );
    assert_eq!(
        &*outputs(&feature, &model.feature_result_topologies, &joins()).expect("one result"),
        &[body("body:target")]
    );
    assert!(outputs(&feature, &[], &joins()).is_none());
    model
        .feature_result_topologies
        .push(model.feature_result_topologies[0].clone());
    assert!(outputs(&feature, &model.feature_result_topologies, &joins()).is_none());
    assert_eq!(native_losses(&model, &joins()).len(), 1);
}

pub(crate) fn metadata() -> crate::native::SegmentMetaRecord {
    crate::native::SegmentMetaRecord {
        id: "metadata".into(),
        token: "seg".into(),
        version: 8,
        kind: "pm_brep".into(),
        display_name: "model".into(),
        segment_id: "segment".into(),
        header_values: [0; 8],
        state_words: [0; 3],
        created: String::new(),
        modified: String::new(),
        body_form: 1,
        expanded_body_len: 0,
        expanded_body_sha256: String::new(),
        table_prefix: [0; 7],
        block_count: 0,
        type_count: 0,
        terminal_id: String::new(),
    }
}

pub(crate) fn root() -> crate::design::scope::PmDcModelRoot {
    use crate::design::scope::PmDcModelRootPayload;
    crate::record_identity::Located::new(
        PmDcModelRootPayload {
            save_version_major: 19,
            header_value: 0,
            header_id: 1,
            next: crate::pmdc::PmDcReference::from_packed(0),
            flags: 0,
            flags_extension: None,
            identifier: String::new(),
            name: String::new(),
            members: crate::pmdc::PmDcReferenceList::new(2, None, vec![]).expect("empty list"),
            values: [0; 15],
            parameter_table: crate::pmdc::PmDcReference::from_packed(0),
            brep_segment: "model".into(),
            scale: cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite"),
            terminal_value: 0,
        },
        crate::record_identity::RecordTypeId::from_bytes([0; 16]),
        cadmpeg_ir::identity_key!("dc"),
        0,
    )
}

fn wrapper() -> PmDcFeatureProperty {
    let mut property = crate::feature::tests::test_property(
        1,
        super::super::PmDcFeaturePropertyKind::SurfaceBody {
            body: crate::pmdc::PmDcReference::from_packed(1),
        },
    );
    property.identity.type_id =
        crate::record_identity::RecordTypeId::from_bytes(super::super::SURFACE_BODY_TYPE);
    property
}

#[test]
fn body_scope_requires_one_document_root_and_one_named_carrier_segment() {
    let mut wrapper = wrapper();
    wrapper.identity.segment_token = cadmpeg_ir::identity_key!("dc");
    let meta = metadata();
    let root = root();
    assert!(Scope {
        metadata: std::slice::from_ref(&meta),
        roots: std::slice::from_ref(&root)
    }
    .admits(&wrapper, "seg"));
    for (metadata, roots) in [
        (vec![], vec![root.clone()]),
        (vec![meta.clone()], vec![]),
        (vec![meta.clone(), meta.clone()], vec![root.clone()]),
        (vec![meta.clone()], vec![root.clone(), root.clone()]),
    ] {
        assert!(!Scope {
            metadata: &metadata,
            roots: &roots
        }
        .admits(&wrapper, "seg"));
    }
    assert!(!Scope {
        metadata: &[meta],
        roots: &[root]
    }
    .admits(&wrapper, "other"));
}

pub(crate) fn carrier() -> ActiveCarrierRecord {
    ActiveCarrierRecord::Selected {
        id: "carrier".into(),
        segment_token: "seg".into(),
        record_ordinal: 0,
        segment_version_major: 19,
        family: crate::kernel::KernelFamily::Asm,
        header_state: 0,
        header_kind: 0,
        header_value: 0,
        schema: 0,
        carrier_len: std::num::NonZeroU64::new(1).expect("nonzero"),
        carrier_sha256: String::new(),
        selected_key: 0x8000_0001,
        enabled: true,
        delta_state: 0,
        history_reference: 0,
    }
}

#[test]
fn shared_body_join_requires_one_wrapper_link_key_and_document_body() {
    use crate::feature::{PmDcKernelEntityLinkPayload, KERNEL_ENTITY_LINK_TYPE};
    use crate::record_identity::{Located, RecordTypeId};
    let mut wrapper = wrapper();
    wrapper.identity.segment_token = cadmpeg_ir::identity_key!("dc");
    let link = Located::new(
        PmDcKernelEntityLinkPayload {
            save_version_major: 19,
            header_value: 0,
            header_id: 77,
            selection_source: wrapper.header.source_index,
            state: 0,
            body: crate::pmdc::PmDcReference::from_packed(0x8000_0001),
            entity_id: 7,
            entity_type: 42,
        },
        RecordTypeId::from_bytes(KERNEL_ENTITY_LINK_TYPE),
        cadmpeg_ir::identity_key!("seg"),
        1,
    );
    let key = BodyNativeKey {
        source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
            cadmpeg_asm::asm_format!("inventor"),
        ),
        record_index: 1,
        body_ordinal: 0,
        source_brep: Some("Bseg:record:0".into()),
        asm_body_key: Some(7),
    };
    let join = WrapperLink {
        id: "test:model:body-wrapper-link#one".into(),
        wrapper: wrapper.id(),
        link: link.id(),
        reference: crate::pmdc::PmDcReference::from_packed(1),
    };
    let mut model = Model::default();
    model.bodies.push(cadmpeg_ir::topology::Body {
        id: body("body:target"),
        native_ref: Some(key.id()),
        kind: cadmpeg_ir::topology::BodyKind::default(),
        regions: vec![],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    let metadata = [metadata()];
    let roots = [root()];
    let scope = Scope {
        metadata: &metadata,
        roots: &roots,
    };
    let map = |wrappers: &[PmDcFeatureProperty],
               links: &[PmDcKernelEntityLink],
               joins: &[WrapperLink],
               keys: &[BodyNativeKey],
               model: &Model| {
        body_map(wrappers, links, joins, &carrier(), keys, model, &scope)
    };
    assert_eq!(
        map(
            std::slice::from_ref(&wrapper),
            std::slice::from_ref(&link),
            std::slice::from_ref(&join),
            std::slice::from_ref(&key),
            &model
        )
        .get(&wrapper.id()),
        Some(&body("body:target"))
    );
    let mut wrong_reference = join.clone();
    wrong_reference.reference = crate::pmdc::PmDcReference::from_packed(2);
    assert!(map(
        std::slice::from_ref(&wrapper),
        std::slice::from_ref(&link),
        &[wrong_reference],
        std::slice::from_ref(&key),
        &model
    )
    .is_empty());
    let mut wrong_type = wrapper.clone();
    wrong_type.identity.type_id = RecordTypeId::from_bytes([0; 16]);
    assert!(map(
        &[wrong_type],
        std::slice::from_ref(&link),
        std::slice::from_ref(&join),
        std::slice::from_ref(&key),
        &model
    )
    .is_empty());
    let mut wrong_payloads = vec![(*link).clone(); 5];
    wrong_payloads[0].header_value = 1;
    wrong_payloads[1].state = 1;
    wrong_payloads[2].selection_source = wrapper.header.source_index + 1;
    wrong_payloads[3].body = crate::pmdc::PmDcReference::from_packed(1);
    wrong_payloads[4].body = crate::pmdc::PmDcReference::from_packed(0x8000_0002);
    let mut wrong_links: Vec<_> = wrong_payloads
        .into_iter()
        .map(|payload| {
            Located::new(
                payload,
                RecordTypeId::from_bytes(KERNEL_ENTITY_LINK_TYPE),
                link.identity.segment_token.clone(),
                link.identity.record_ordinal,
            )
        })
        .collect();
    let mut wrong_type = link.clone();
    wrong_type.identity.type_id = RecordTypeId::from_bytes([0; 16]);
    wrong_links.push(wrong_type);
    let mut wrong_segment = link.clone();
    wrong_segment.identity.segment_token = cadmpeg_ir::identity_key!("other");
    wrong_links.push(wrong_segment);
    for wrong in wrong_links {
        let mut relation = join.clone();
        relation.link = wrong.id();
        assert!(map(
            std::slice::from_ref(&wrapper),
            &[wrong],
            &[relation],
            std::slice::from_ref(&key),
            &model
        )
        .is_empty());
    }
    for reference in [0, 0x8000_0001] {
        let mut payload = (*wrapper).clone();
        payload.kind = super::super::PmDcFeaturePropertyKind::SurfaceBody {
            body: crate::pmdc::PmDcReference::from_packed(reference),
        };
        let wrong = Located::new(
            payload,
            RecordTypeId::from_bytes(super::super::SURFACE_BODY_TYPE),
            wrapper.identity.segment_token.clone(),
            wrapper.identity.record_ordinal,
        );
        let mut relation = join.clone();
        relation.reference = crate::pmdc::PmDcReference::from_packed(reference);
        assert!(map(
            &[wrong],
            std::slice::from_ref(&link),
            &[relation],
            std::slice::from_ref(&key),
            &model
        )
        .is_empty());
    }
    for (wrappers, links, joins, keys, model) in [
        (
            vec![],
            vec![link.clone()],
            vec![join.clone()],
            vec![key.clone()],
            model.clone(),
        ),
        (
            vec![wrapper.clone(), wrapper.clone()],
            vec![link.clone()],
            vec![join.clone()],
            vec![key.clone()],
            model.clone(),
        ),
        (
            vec![wrapper.clone()],
            vec![],
            vec![join.clone()],
            vec![key.clone()],
            model.clone(),
        ),
        (
            vec![wrapper.clone()],
            vec![link.clone(), link.clone()],
            vec![join.clone()],
            vec![key.clone()],
            model.clone(),
        ),
        (
            vec![wrapper.clone()],
            vec![link.clone()],
            vec![join.clone(), join.clone()],
            vec![key.clone()],
            model.clone(),
        ),
        (
            vec![wrapper.clone()],
            vec![link.clone()],
            vec![join.clone()],
            vec![],
            model.clone(),
        ),
        (
            vec![wrapper.clone()],
            vec![link.clone()],
            vec![join.clone()],
            vec![key.clone(), key.clone()],
            model.clone(),
        ),
        (
            vec![wrapper.clone()],
            vec![link.clone()],
            vec![join.clone()],
            vec![key.clone()],
            Model::default(),
        ),
    ] {
        assert!(map(&wrappers, &links, &joins, &keys, &model).is_empty());
    }
}

#[test]
fn body_selection_refuses_missing_groups_partial_motion_and_aliased_motion() {
    let tools = BodySelection::NativeSet(
        NativeSelections::try_from(vec!["tool".into(), "other".into()]).expect("distinct"),
    );
    let mut selected = tools.clone();
    selection(&mut selected, &joins(), None);
    assert_eq!(selected, tools);
    let local = BodySelection::local(vec!["target".into(), "tool".into()], "selection".into())
        .expect("distinct");
    let mut partial = joins();
    partial.remove("tool");
    let mut aliased = joins();
    aliased.insert("tool".into(), body("body:target"));
    for bodies in [partial, aliased] {
        let mut selected = local.clone();
        selection(&mut selected, &bodies, None);
        assert_eq!(selected, local);
    }
    let mut unknown = BodySelection::Native("absent".into());
    selection(&mut unknown, &joins(), None);
    assert_eq!(unknown, BodySelection::Native("absent".into()));
}

#[test]
fn combine_target_and_producing_feature_outputs_share_the_result_wrapper_join() {
    use cadmpeg_ir::features::{DistinctMembers, Feature, FeatureContent, FeatureEvaluation};
    let id = FeatureId::mint("test:model:feature#combine").expect("synthetic feature");
    let mut model = Model::default();
    model.features.push(Feature {
        id: id.clone(),
        ordinal: 0,
        name: None,
        suppressed: None,
        needs_recompute: false,
        dependencies: DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: Some("combine".into()),
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(combine(BodySelection::Native(
            "tool".into(),
        ))),
        native_ref: None,
        product_definition: None,
    });
    model
        .feature_result_topologies
        .push(FeatureResultTopology::new(
            FeatureResultTopologyId::mint("test:model:feature-result#result")
                .expect("synthetic result"),
            id.clone(),
            FeatureResultMembers::new(
                vec![NonBlankString::new("target").expect("wrapper")],
                vec![],
                vec![],
                vec![],
            )
            .expect("members"),
            Some("selector".into()),
        ));
    project(&mut model, &joins(), &FeatureInventory::default());
    let feature = &model.features[0];
    assert_eq!(feature.evaluation.outputs(), &[body("body:target")]);
    assert!(outputs_match(
        feature,
        &model.feature_result_topologies,
        &joins()
    ));
    assert!(!outputs_match(feature, &[], &joins()));
    assert!(!outputs_match(
        feature,
        &model.feature_result_topologies,
        &HashMap::new()
    ));
    let mut wrong = feature.clone();
    wrong
        .evaluation
        .set_outputs(vec![body("body:tool")].try_into().expect("one body"));
    assert!(!outputs_match(
        &wrong,
        &model.feature_result_topologies,
        &joins()
    ));
    let duplicate = [
        model.feature_result_topologies[0].clone(),
        model.feature_result_topologies[0].clone(),
    ];
    assert!(!outputs_match(feature, &duplicate, &joins()));
    wrong.evaluation.set_outputs(DistinctMembers::default());
    assert!(outputs_match(&wrong, &[], &joins()));
    assert!(!outputs_match(&wrong, &duplicate, &joins()));

    let FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) =
        feature.evaluation.definition()
    else {
        panic!("combine")
    };
    let (target, _) = operands.clone().into_parts();
    assert_eq!(
        target,
        BodySelection::Resolved {
            bodies: vec![body("body:target")].try_into().expect("one body"),
            native: "target".into()
        }
    );
    assert_eq!(
        feature.evaluation.outputs().as_slice(),
        &*outputs(&id, &model.feature_result_topologies, &joins()).expect("one result")
    );
    assert_ne!(
        feature.evaluation.outputs().as_slice(),
        &[body("body:tool")]
    );
    assert_eq!(
        model.feature_result_topologies[0].bodies()[0].as_str(),
        "target"
    );
}

#[test]
fn combine_keeps_native_operands_when_current_body_identities_overlap() {
    let source = combine(BodySelection::Native("tool".into()));
    let mut bodies = joins();
    bodies.insert("tool".into(), body("body:target"));
    let mut actual = source.clone();
    definition(&mut actual, &bodies, None);
    assert_eq!(source, actual);
    assert!(matches(source, &actual, &bodies, None));
}

#[test]
fn producing_outputs_keep_exact_joins_in_member_order_and_report_missing_wrappers() {
    let feature = FeatureId::mint("test:model:feature#producer").expect("synthetic feature");
    let mut model = Model::default();
    model
        .feature_result_topologies
        .push(FeatureResultTopology::new(
            FeatureResultTopologyId::mint("test:model:feature-result#ordered")
                .expect("synthetic result"),
            feature.clone(),
            FeatureResultMembers::new(
                ["other", "absent", "target"]
                    .into_iter()
                    .map(|value| NonBlankString::new(value).expect("wrapper"))
                    .collect(),
                vec![],
                vec![],
                vec![],
            )
            .expect("distinct wrappers"),
            Some("result".into()),
        ));
    assert_eq!(
        &*outputs(&feature, &model.feature_result_topologies, &joins()).expect("one result"),
        &[body("body:other"), body("body:target")]
    );
    assert_eq!(
        native_losses(&model, &joins())[0].code,
        crate::loss::InventorLossCode::FeatureBodySelectionNative.kind()
    );
    assert_eq!(model.feature_result_topologies[0].bodies().len(), 3);
}
