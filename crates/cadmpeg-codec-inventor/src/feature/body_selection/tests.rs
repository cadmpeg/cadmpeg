// SPDX-License-Identifier: Apache-2.0
use super::*;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::features::{
    BooleanKind, FeatureResultMembers, FeatureResultTopology, FiniteVector3, NativeSelections,
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
        translation: FiniteVector3::ZERO,
        rotation: None,
        copies: 0,
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
        carrier_offset: 0,
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
        body: body("body:target"),
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
        id: key.body.clone(),
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
        Some(&key.body)
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

#[test]
fn trim_body_selections_resolve_distinct_bodies_and_report_native_operands() {
    use cadmpeg_ir::features::{
        BodyTrimSide, DistinctMembers, Feature, FeatureContent, FeatureEvaluation,
    };
    let source = FeatureDefinition::Operation(FeatureOperation::TrimBodies {
        operands: TrimBodyOperands::new(
            BodySelection::Native("target".into()),
            BodySelection::Native("tool".into()),
        )
        .expect("distinct operands"),
        keep: BodyTrimSide::Unresolved,
    });
    for missing in [false, true] {
        let mut bodies = joins();
        if missing {
            bodies.remove("tool");
        }
        let mut actual = source.clone();
        definition(&mut actual, &bodies, None);
        let FeatureDefinition::Operation(FeatureOperation::TrimBodies { operands, keep }) = &actual
        else {
            panic!("trim")
        };
        assert_eq!(*keep, BodyTrimSide::Unresolved);
        assert!(matches!(operands.targets(), BodySelection::Resolved { .. }));
        assert_eq!(
            matches!(operands.tools(), BodySelection::Native(_)),
            missing
        );
        assert!(matches(source.clone(), &actual, &bodies, None));
        let mut model = Model::default();
        model.features.push(Feature {
            id: FeatureId::mint("test:model:feature#trim").expect("feature"),
            ordinal: 0,
            name: None,
            suppressed: None,
            dependencies: DistinctMembers::default(),
            source_properties: std::collections::BTreeMap::default(),
            source_tag: Some("trim_bodies".into()),
            source_text: None,
            source_content: FeatureContent::default(),
            evaluation: FeatureEvaluation::from_definition(actual),
            native_ref: None,
            product_definition: None,
        });
        let losses = native_losses(&model, &bodies);
        assert_eq!(losses.len(), usize::from(missing));
        if missing {
            assert_eq!(losses[0].code.local_code(), "feature.body-selection-native");
        }
    }
    let mut aliases = joins();
    aliases.insert("tool".into(), body("body:target"));
    let mut actual = source.clone();
    definition(&mut actual, &aliases, None);
    assert_eq!(actual, source);
}

fn wrapper_at(ordinal: u32) -> PmDcFeatureProperty {
    let mut property = crate::feature::tests::test_property(
        ordinal,
        super::super::PmDcFeaturePropertyKind::SurfaceBody {
            body: crate::pmdc::PmDcReference::from_packed(1),
        },
    );
    property.identity.type_id =
        crate::record_identity::RecordTypeId::from_bytes(super::super::SURFACE_BODY_TYPE);
    property
}

fn model_with_bodies(names: &[(&str, Option<&str>)]) -> Model {
    let mut model = Model::default();
    for (id, name) in names {
        model.bodies.push(cadmpeg_ir::topology::Body {
            id: body(id),
            kind: cadmpeg_ir::topology::BodyKind::default(),
            regions: vec![],
            transform: None,
            name: name.map(str::to_owned),
            color: None,
            visible: None,
        });
    }
    model
}

fn names(model: &Model) -> Vec<Option<&str>> {
    model
        .bodies
        .iter()
        .map(|body| body.name.as_deref())
        .collect()
}

/// A label owned by the wrapper record `owner`; its stored name is
/// `Feature {index}`.
fn name_label(owner: u32, index: u32) -> crate::feature::PmDcFeatureLabel {
    crate::feature::tests::test_label(owner, index, crate::feature::FILLET_CLASS_ID, &[])
}

fn named(
    model: &mut Model,
    wrappers: Vec<PmDcFeatureProperty>,
    labels: Vec<crate::feature::PmDcFeatureLabel>,
    joined: &[(&PmDcFeatureProperty, &str)],
) {
    let joins = joined
        .iter()
        .map(|(wrapper, id)| (wrapper.id(), body(id)))
        .collect::<HashMap<_, _>>();
    let inventory = FeatureInventory {
        properties: wrappers,
        labels,
        ..FeatureInventory::default()
    };
    project(model, &joins, &inventory);
}

#[test]
fn body_takes_the_stored_name_of_the_label_that_owns_its_wrapper() {
    let (first, second) = (wrapper_at(7), wrapper_at(9));
    let mut model = model_with_bodies(&[("body:a", None), ("body:b", None)]);
    named(
        &mut model,
        vec![first.clone(), second.clone()],
        vec![name_label(7, 3), name_label(9, 4)],
        &[(&first, "body:a"), (&second, "body:b")],
    );
    assert_eq!(names(&model), [Some("Feature 3"), Some("Feature 4")]);
}

#[test]
fn a_label_names_only_the_body_of_its_own_wrapper() {
    let (first, second) = (wrapper_at(7), wrapper_at(9));
    let mut model = model_with_bodies(&[("body:a", None), ("body:b", None)]);
    named(
        &mut model,
        vec![first.clone(), second.clone()],
        vec![name_label(7, 3)],
        &[(&first, "body:a"), (&second, "body:b")],
    );
    assert_eq!(names(&model), [Some("Feature 3"), None]);
}

#[test]
fn a_body_without_a_label_or_join_keeps_no_name() {
    let wrapper = wrapper_at(7);
    // No label owns the wrapper.
    let mut model = model_with_bodies(&[("body:a", None)]);
    named(
        &mut model,
        vec![wrapper.clone()],
        vec![name_label(8, 3)],
        &[(&wrapper, "body:a")],
    );
    assert_eq!(names(&model), [None]);
    // The label owns the wrapper but the wrapper joins no body.
    let mut model = model_with_bodies(&[("body:a", None)]);
    named(&mut model, vec![wrapper], vec![name_label(7, 3)], &[]);
    assert_eq!(names(&model), [None]);
}

#[test]
fn ambiguous_label_or_wrapper_joins_leave_the_body_unnamed() {
    let (first, second) = (wrapper_at(7), wrapper_at(9));
    // Two labels own one wrapper.
    let mut model = model_with_bodies(&[("body:a", None)]);
    named(
        &mut model,
        vec![first.clone()],
        vec![name_label(7, 3), name_label(7, 4)],
        &[(&first, "body:a")],
    );
    assert_eq!(names(&model), [None]);
    // Two wrappers join one body.
    let mut model = model_with_bodies(&[("body:a", None)]);
    named(
        &mut model,
        vec![first.clone(), second.clone()],
        vec![name_label(7, 3), name_label(9, 4)],
        &[(&first, "body:a"), (&second, "body:a")],
    );
    assert_eq!(names(&model), [None]);
}

#[test]
fn only_a_surface_body_wrapper_supplies_a_body_name() {
    let mut other = wrapper_at(7);
    other.identity.type_id = crate::record_identity::RecordTypeId::from_bytes([1; 16]);
    let mut model = model_with_bodies(&[("body:a", None)]);
    named(
        &mut model,
        vec![other.clone()],
        vec![name_label(7, 3)],
        &[(&other, "body:a")],
    );
    assert_eq!(names(&model), [None]);
}

#[test]
fn an_existing_body_name_is_not_replaced() {
    let wrapper = wrapper_at(7);
    let mut model = model_with_bodies(&[("body:a", Some("stored"))]);
    named(
        &mut model,
        vec![wrapper.clone()],
        vec![name_label(7, 3)],
        &[(&wrapper, "body:a")],
    );
    assert_eq!(names(&model), [Some("stored")]);
}

#[test]
fn a_composite_member_link_join_does_not_count_as_a_second_wrapper() {
    let wrapper = wrapper_at(7);
    let mut model = model_with_bodies(&[("body:a", None)]);
    let joins = HashMap::from([
        (wrapper.id(), body("body:a")),
        (
            "inventor:design:composite-member-link#1".to_owned(),
            body("body:a"),
        ),
    ]);
    let inventory = FeatureInventory {
        properties: vec![wrapper],
        labels: vec![name_label(7, 3)],
        ..FeatureInventory::default()
    };
    project(&mut model, &joins, &inventory);
    assert_eq!(names(&model), [Some("Feature 3")]);
}

#[test]
fn body_mirror_pattern_seeds_resolve_with_the_wrapper_joins_and_stay_local_without_them() {
    use cadmpeg_ir::features::patterns::{PatternKind, PatternSeed, PatternTransform};
    use cadmpeg_ir::features::FeatureOperation;
    use cadmpeg_ir::features::{DistinctMembers, Feature, FeatureContent, FeatureEvaluation};
    use cadmpeg_ir::features::{FeatureDirection3, FinitePoint3};
    use cadmpeg_ir::math::{Point3, Vector3};
    let plane_normal = FeatureDirection3::from(
        cadmpeg_ir::units::UnitVector3::normalized(Vector3::new(1.0, 0.0, 0.0))
            .expect("unit normal"),
    );
    let source = FeatureDefinition::Operation(FeatureOperation::Pattern {
        seeds: vec![PatternSeed::Bodies(
            BodySelection::local(vec!["target".into(), "tool".into()], "group".into())
                .expect("local selection"),
        )],
        pattern: PatternKind::new(PatternTransform::Mirror {
            plane_origin: FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).expect("origin"),
            plane_normal,
        })
        .expect("mirror"),
    });
    let mut actual = source.clone();
    definition(&mut actual, &joins(), None);
    let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) = &actual else {
        panic!("pattern")
    };
    assert_eq!(
        seeds,
        &vec![PatternSeed::Bodies(BodySelection::Resolved {
            bodies: vec![body("body:target"), body("body:tool")]
                .try_into()
                .expect("two bodies"),
            native: "group".into()
        })]
    );
    let mut missing = joins();
    missing.remove("tool");
    let mut unresolved = source.clone();
    definition(&mut unresolved, &missing, None);
    assert_eq!(unresolved, source);
    let feature_seed = FeatureDefinition::Operation(FeatureOperation::Pattern {
        seeds: vec![PatternSeed::Feature(
            FeatureId::mint("test:model:feature#seed").expect("feature"),
        )],
        pattern: PatternKind::new(PatternTransform::Mirror {
            plane_origin: FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).expect("origin"),
            plane_normal,
        })
        .expect("mirror"),
    });
    let model_of = |definition: FeatureDefinition| {
        let mut model = Model::default();
        model.features.push(Feature {
            id: FeatureId::mint("test:model:feature#pattern").expect("feature"),
            ordinal: 0,
            name: None,
            suppressed: None,
            dependencies: DistinctMembers::default(),
            source_properties: std::collections::BTreeMap::default(),
            source_tag: Some("pattern".into()),
            source_text: None,
            source_content: FeatureContent::default(),
            evaluation: FeatureEvaluation::from_definition(definition),
            native_ref: None,
            product_definition: None,
        });
        model
    };
    assert!(native_losses(&model_of(actual), &joins()).is_empty());
    let losses = native_losses(&model_of(unresolved), &missing);
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "feature.body-selection-native");
    assert!(native_losses(&model_of(feature_seed), &joins()).is_empty());
}

fn plain_feature(
    name: &str,
    ordinal: u64,
    suppressed: Option<bool>,
) -> cadmpeg_ir::features::Feature {
    use cadmpeg_ir::features::{DistinctMembers, Feature, FeatureContent, FeatureEvaluation};
    Feature {
        id: FeatureId::mint(format!("test:model:feature#{name}")).expect("synthetic feature"),
        ordinal,
        name: None,
        suppressed,
        dependencies: DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(combine(BodySelection::Native(
            "tool".into(),
        ))),
        native_ref: None,
        product_definition: None,
    }
}

fn result_of(feature: &cadmpeg_ir::features::Feature, wrappers: &[&str]) -> FeatureResultTopology {
    FeatureResultTopology::new(
        FeatureResultTopologyId::mint(format!(
            "test:model:feature-result#{}",
            feature.id.as_str().rsplit('#').next().expect("name")
        ))
        .expect("synthetic result"),
        feature.id.clone(),
        FeatureResultMembers::new(
            wrappers
                .iter()
                .map(|wrapper| NonBlankString::new(*wrapper).expect("wrapper"))
                .collect(),
            vec![],
            vec![],
            vec![],
        )
        .expect("members"),
        None,
    )
}

#[test]
fn produced_wrappers_come_from_earlier_unsuppressed_features_only() {
    let earlier = plain_feature("earlier", 3, Some(false));
    let suppressed = plain_feature("suppressed", 4, Some(true));
    let later = plain_feature("later", 9, Some(false));
    let consumer = plain_feature("consumer", 6, Some(false));
    let features = [earlier, suppressed, later, consumer];
    let results = [
        result_of(&features[0], &["made"]),
        result_of(&features[1], &["skipped"]),
        result_of(&features[2], &["future"]),
    ];
    let produced = produced_before(&features, &results, &features[3], &empty_producer_records());
    assert_eq!(produced, HashSet::from(["made".to_owned()]));
}

#[test]
fn unplaced_producer_is_not_an_earlier_producer() {
    let unplaced = plain_feature("unplaced", 1, None);
    let consumer = plain_feature("consumer", 6, Some(false));
    let features = [unplaced, consumer];
    let results = [result_of(&features[0], &["made"])];
    assert!(
        produced_before(&features, &results, &features[1], &empty_producer_records()).is_empty()
    );
}

#[test]
fn unplaced_combine_has_no_earlier_producers() {
    let producer = plain_feature("producer", 1, Some(false));
    let consumer = plain_feature("consumer", 6, None);
    let features = [producer, consumer];
    let results = [result_of(&features[0], &["made"])];
    assert!(
        produced_before(&features, &results, &features[1], &empty_producer_records()).is_empty()
    );
    let mut actual = combine(BodySelection::Native("tool".into()));
    combine_local(&mut actual, &HashSet::new(), None);
    assert_eq!(actual, combine(BodySelection::Native("tool".into())));
}

#[test]
fn unjoined_combine_operands_stay_local_only_when_every_wrapper_is_produced_earlier() {
    let tools = || {
        BodySelection::NativeSet(
            NativeSelections::try_from(vec!["tool".into(), "other".into()])
                .expect("distinct wrappers"),
        )
    };
    let operands = |actual: &FeatureDefinition| {
        let FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) = actual
        else {
            panic!("combine")
        };
        operands.clone().into_parts()
    };
    let all: HashSet<String> = ["target", "tool", "other"].map(String::from).into();
    let mut actual = combine(tools());
    combine_local(&mut actual, &all, Some("tools"));
    let (target, tools_after) = operands(&actual);
    assert_eq!(
        target,
        BodySelection::local(vec!["target".into()], "target".into()).expect("local")
    );
    assert_eq!(
        tools_after,
        BodySelection::local(vec!["tool".into(), "other".into()], "tools".into()).expect("local")
    );
    assert!(combine_matches(
        combine(tools()),
        &actual,
        &HashMap::new(),
        Some("tools"),
        &all
    ));
    // A wrapper that no earlier feature produces keeps its selection native.
    let partial: HashSet<String> = ["target", "tool"].map(String::from).into();
    let mut actual = combine(tools());
    combine_local(&mut actual, &partial, Some("tools"));
    let (target, tools_after) = operands(&actual);
    assert!(matches!(target, BodySelection::Local { .. }));
    assert_eq!(tools_after, tools());
    // A set without its collection identity stays native.
    let mut actual = combine(tools());
    combine_local(&mut actual, &all, None);
    assert_eq!(operands(&actual).1, tools());
    // A complete current-body join wins over the local form.
    let mut actual = combine(tools());
    definition(&mut actual, &joins(), Some("tools"));
    combine_local(&mut actual, &all, Some("tools"));
    assert!(matches!(
        operands(&actual).0,
        BodySelection::Resolved { .. }
    ));
}

fn plane_trim(targets: BodySelection) -> FeatureDefinition {
    FeatureDefinition::Operation(FeatureOperation::TrimBodiesAtPlane {
        targets,
        plane: FeatureId::mint("test:model:feature#plane").expect("plane"),
        keep: cadmpeg_ir::features::BodyTrimSide::Forward,
    })
}

fn plane_trim_targets(definition: &FeatureDefinition) -> &BodySelection {
    let FeatureDefinition::Operation(FeatureOperation::TrimBodiesAtPlane { targets, .. }) =
        definition
    else {
        panic!("plane trim")
    };
    targets
}

#[test]
fn unjoined_plane_trim_target_stays_local_only_when_its_wrapper_is_produced_earlier() {
    let produced: HashSet<String> = ["target".to_owned()].into();
    // An earlier producer makes the wrapper a persistent input body.
    let mut actual = plane_trim(BodySelection::Native("target".into()));
    split_local(&mut actual, &produced, true);
    assert_eq!(
        plane_trim_targets(&actual),
        &BodySelection::local(vec!["target".into()], "target".into()).expect("local")
    );
    assert!(split_matches(
        plane_trim(BodySelection::Native("target".into())),
        &actual,
        &HashMap::new(),
        &produced,
        true
    ));
    // The expected form of a Local selection is not accepted without a producer.
    assert!(!split_matches(
        plane_trim(BodySelection::Native("target".into())),
        &actual,
        &HashMap::new(),
        &HashSet::new(),
        true
    ));
    // A wrapper that no earlier feature produces keeps its selection native.
    let mut actual = plane_trim(BodySelection::Native("target".into()));
    split_local(&mut actual, &["other".to_owned()].into(), true);
    assert_eq!(
        plane_trim_targets(&actual),
        &BodySelection::Native("target".into())
    );
    split_local(&mut actual, &HashSet::new(), true);
    assert_eq!(
        plane_trim_targets(&actual),
        &BodySelection::Native("target".into())
    );
    // A complete current-body join wins over the local form.
    let mut actual = plane_trim(BodySelection::Native("target".into()));
    definition(&mut actual, &joins(), None);
    split_local(&mut actual, &produced, true);
    assert!(matches!(
        plane_trim_targets(&actual),
        BodySelection::Resolved { .. }
    ));
}

#[test]
fn local_plane_trim_target_counts_in_the_unjoined_body_loss() {
    use cadmpeg_ir::features::{DistinctMembers, Feature, FeatureContent, FeatureEvaluation};
    let mut actual = plane_trim(BodySelection::Native("target".into()));
    split_local(&mut actual, &["target".to_owned()].into(), true);
    let mut model = Model::default();
    model.features.push(Feature {
        id: FeatureId::mint("test:model:feature#trim").expect("feature"),
        ordinal: 1,
        name: None,
        suppressed: Some(false),
        dependencies: DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: Some("trim_bodies".into()),
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(actual),
        native_ref: None,
        product_definition: None,
    });
    let losses = native_losses(&model, &HashMap::new());
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "feature.body-selection-native");
}

fn empty_producer_records() -> ProducerRecords<'static> {
    ProducerRecords {
        features: &[],
        labels: &[],
        properties: &[],
    }
}

fn native_derived_producer() -> (cadmpeg_ir::features::Feature, FeatureInventory, String) {
    use crate::feature::tests::{test_feature, test_label, test_property};
    use crate::feature::PmDcFeaturePropertyKind;
    let mut source = test_feature(10, 3, &[(0, 20), (1, 21), (2, 22)]);
    let mut input = test_property(
        20,
        PmDcFeaturePropertyKind::SurfaceBody {
            body: crate::pmdc::PmDcReference::from_packed(0x8000_001f),
        },
    );
    let mut output = test_property(
        21,
        PmDcFeaturePropertyKind::SurfaceBody {
            body: crate::pmdc::PmDcReference::from_packed(1),
        },
    );
    for wrapper in [&mut input, &mut output] {
        wrapper.identity.type_id =
            crate::record_identity::RecordTypeId::from_bytes(crate::feature::SURFACE_BODY_TYPE);
    }
    source.edit(|payload| payload.save_version_major = 19);
    input.edit(|payload| payload.save_version_major = 19);
    output.edit(|payload| payload.save_version_major = 19);
    let wrapper = output.id();
    let mut producer = plain_feature("derived", 1, Some(false));
    producer.native_ref = Some(source.id());
    producer.evaluation = cadmpeg_ir::features::FeatureEvaluation::from_definition(
        FeatureDefinition::Operation(FeatureOperation::Native {
            kind: cadmpeg_ir::features::NativeFeatureKind::Other("derived".into()),
            parameters: std::collections::BTreeMap::default(),
        }),
    );
    let inventory = FeatureInventory {
        features: vec![source],
        labels: vec![test_label(10, 1, crate::feature::derived::CLASS_ID, &[])],
        properties: vec![input, output],
        ..FeatureInventory::default()
    };
    (producer, inventory, wrapper)
}

fn check_derived_selection(trim: bool, ordinal: u64, suppressed: Option<bool>, joined: bool) {
    let (mut producer, inventory, wrapper) = native_derived_producer();
    producer.ordinal = ordinal;
    producer.suppressed = suppressed;
    let original = if trim {
        plane_trim(BodySelection::Native(wrapper.clone()))
    } else {
        combine(BodySelection::Native(wrapper.clone()))
    };
    let mut consumer = plain_feature("consumer", 6, Some(false));
    consumer.evaluation =
        cadmpeg_ir::features::FeatureEvaluation::from_definition(original.clone());
    let mut model = Model::default();
    model.features = vec![producer, consumer];
    let bodies = if joined {
        HashMap::from([(wrapper.clone(), body("derived"))])
    } else {
        HashMap::new()
    };
    project(&mut model, &bodies, &inventory);
    assert!(model.feature_result_topologies.is_empty());
    let actual = model.features[1].evaluation.definition();
    let expected = if joined {
        BodySelection::Resolved {
            bodies: vec![body("derived")].try_into().expect("body"),
            native: wrapper.clone(),
        }
    } else if ordinal < 6 && suppressed == Some(false) {
        BodySelection::local(vec![wrapper.clone()], wrapper).expect("local")
    } else {
        BodySelection::Native(wrapper)
    };
    if trim {
        assert_eq!(plane_trim_targets(actual), &expected);
    } else {
        let FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) = actual
        else {
            panic!("combine")
        };
        assert_eq!(operands.clone().into_parts().1, expected);
    }
    let earlier = produced_before(
        &model.features,
        &model.feature_result_topologies,
        &model.features[1],
        &ProducerRecords {
            features: &inventory.features,
            labels: &inventory.labels,
            properties: &inventory.properties,
        },
    );
    if trim {
        assert!(split_matches(original, actual, &bodies, &earlier, true));
    } else {
        assert!(combine_matches(original, actual, &bodies, None, &earlier));
    }
}

#[test]
fn native_derived_result_makes_plane_trim_target_local() {
    check_derived_selection(true, 1, Some(false), false);
}

#[test]
fn native_derived_result_makes_combine_tool_local() {
    check_derived_selection(false, 1, Some(false), false);
}

#[test]
fn later_unplaced_or_suppressed_native_derived_producer_keeps_selections_native() {
    for trim in [true, false] {
        for (ordinal, suppressed) in [(9, Some(false)), (1, None), (1, Some(true))] {
            check_derived_selection(trim, ordinal, suppressed, false);
        }
    }
}

#[test]
fn native_derived_result_current_body_join_wins() {
    for trim in [true, false] {
        check_derived_selection(trim, 1, Some(false), true);
    }
}

fn split_body_forms() -> Vec<FeatureDefinition> {
    [
        SplitBodyTool::Plane {
            plane: FeatureId::mint("test:model:feature#plane").expect("plane"),
        },
        SplitBodyTool::Faces(cadmpeg_ir::features::FaceSelection::Native("tool".into())),
    ]
    .into_iter()
    .map(|tool| {
        FeatureDefinition::Operation(FeatureOperation::SplitBody {
            targets: BodySelection::Native("target".into()),
            tool,
        })
    })
    .chain(std::iter::once(FeatureDefinition::Operation(
        FeatureOperation::TrimBodies {
            operands: TrimBodyOperands::new(
                BodySelection::Native("target".into()),
                BodySelection::Native("tool".into()),
            )
            .expect("distinct"),
            keep: cadmpeg_ir::features::BodyTrimSide::Unresolved,
        },
    )))
    .collect()
}

fn split_targets(definition: &FeatureDefinition) -> &BodySelection {
    match definition {
        FeatureDefinition::Operation(FeatureOperation::SplitBody { targets, .. }) => targets,
        FeatureDefinition::Operation(FeatureOperation::TrimBodies { operands, .. }) => {
            operands.targets()
        }
        _ => panic!("body split or trim"),
    }
}

#[test]
fn produced_split_body_targets_become_local_and_validate() {
    let earlier = HashSet::from(["target".to_owned()]);
    for original in split_body_forms() {
        let mut actual = original.clone();
        split_local(&mut actual, &earlier, true);
        assert_eq!(
            split_targets(&actual),
            &BodySelection::local(vec!["target".into()], "target".into()).expect("local")
        );
        assert!(split_matches(
            original.clone(),
            &actual,
            &HashMap::new(),
            &earlier,
            true
        ));
        assert!(!split_matches(
            original,
            &actual,
            &HashMap::new(),
            &HashSet::new(),
            true
        ));
    }
}

#[test]
fn split_body_targets_without_a_matching_producer_stay_native() {
    for earlier in [HashSet::new(), HashSet::from(["other".to_owned()])] {
        for original in split_body_forms() {
            let mut actual = original.clone();
            split_local(&mut actual, &earlier, true);
            assert_eq!(actual, original);
        }
    }
}

#[test]
fn split_body_complete_join_wins_over_local_producer() {
    let earlier = HashSet::from(["target".to_owned()]);
    for original in split_body_forms() {
        let mut actual = original.clone();
        definition(&mut actual, &joins(), None);
        split_local(&mut actual, &earlier, true);
        assert_eq!(
            split_targets(&actual),
            &BodySelection::Resolved {
                bodies: vec![body("body:target")].try_into().expect("body"),
                native: "target".into(),
            }
        );
        assert!(split_matches(original, &actual, &joins(), &earlier, true));
    }
}

#[test]
fn local_split_body_target_preserves_unjoined_loss_count() {
    let earlier = HashSet::from(["target".to_owned()]);
    for original in split_body_forms() {
        let mut model = Model::default();
        let mut feature = plain_feature("split", 2, Some(false));
        feature.evaluation = cadmpeg_ir::features::FeatureEvaluation::from_definition(original);
        model.features.push(feature);
        let before = native_losses(&model, &HashMap::new());
        model.features[0].evaluation.edit(|definition, _| {
            split_local(definition, &earlier, true);
        });
        let after = native_losses(&model, &HashMap::new());
        assert_eq!(before.len(), 1);
        assert_eq!(after.len(), 1);
        assert_eq!(before[0].code, after[0].code);
        assert_eq!(before[0].message, after[0].message);
    }
}

#[test]
fn unqualified_split_body_target_stays_native_with_a_producer() {
    let earlier = HashSet::from(["target".to_owned()]);
    for original in split_body_forms().into_iter().take(2) {
        let mut actual = original.clone();
        split_local(&mut actual, &earlier, false);
        assert_eq!(actual, original);
    }
}

#[test]
fn split_local_target_checks_member_count_qualification_and_body_reference() {
    use crate::feature::{
        tests::reference_list, PmDcFeaturePropertyKind, PmDcFeatureReferenceFamily,
    };
    use crate::pmdc::PmDcReference;
    use crate::test_support::split::{plane_trim_fixture, set_kind, set_slot};
    let (source, properties) = plane_trim_fixture(false);
    assert!(qualified_split_target(&source, &properties));
    let mut unqualified = source.clone();
    set_slot(
        &mut unqualified,
        7,
        PmDcReference::new(source.properties.references()[7].index(), false).expect("reference"),
    );
    assert!(!qualified_split_target(&unqualified, &properties));
    for members in [vec![], vec![69, 70]] {
        let mut properties = properties.clone();
        set_kind(
            &mut properties,
            5,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::BodySelection,
                items: reference_list(&members),
            },
        );
        assert!(!qualified_split_target(&source, &properties));
    }
    let mut unqualified = properties.clone();
    unqualified[5].edit(|payload| {
        payload.kind = PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::BodySelection,
            items: crate::pmdc::PmDcReferenceList::new(
                2,
                Some(crate::pmdc::PmDcListMetadata::U32([1, 0])),
                vec![PmDcReference::new(69, false).expect("reference")],
            )
            .expect("one member"),
        };
    });
    assert!(!qualified_split_target(&source, &unqualified));
    for body in [PmDcReference::new(0, false), PmDcReference::new(1, true)] {
        let mut properties = properties.clone();
        set_kind(
            &mut properties,
            7,
            PmDcFeaturePropertyKind::SurfaceBody {
                body: body.expect("reference"),
            },
        );
        assert!(!qualified_split_target(&source, &properties));
    }
}

#[test]
fn split_body_projection_and_validator_share_producer_and_qualification_rules() {
    use crate::test_support::split::{plane_trim_fixture, set_slot};
    use cadmpeg_ir::features::FeatureEvaluation;
    for qualified in [true, false] {
        let (mut source, properties) = plane_trim_fixture(false);
        if !qualified {
            let index = source.properties.references()[7].index();
            set_slot(
                &mut source,
                7,
                crate::pmdc::PmDcReference::new(index, false).expect("reference"),
            );
        }
        let wrapper = properties[7].id();
        let original = FeatureDefinition::Operation(FeatureOperation::SplitBody {
            targets: BodySelection::Native(wrapper.clone()),
            tool: SplitBodyTool::Plane {
                plane: FeatureId::mint("test:model:feature#plane").expect("plane"),
            },
        });
        let producer = plain_feature("producer", 1, Some(false));
        let result = result_of(&producer, &[&wrapper]);
        let mut consumer = plain_feature("consumer", 2, Some(false));
        consumer.native_ref = Some(source.id());
        consumer.evaluation = FeatureEvaluation::from_definition(original.clone());
        let mut model = Model::default();
        model.features = vec![producer, consumer];
        model.feature_result_topologies.push(result);
        let inventory = FeatureInventory {
            features: vec![source],
            properties,
            ..FeatureInventory::default()
        };
        project(&mut model, &HashMap::new(), &inventory);
        let expected = if qualified {
            BodySelection::local(vec![wrapper.clone()], wrapper.clone()).expect("local")
        } else {
            BodySelection::Native(wrapper.clone())
        };
        let actual = model.features[1].evaluation.definition();
        assert_eq!(split_targets(actual), &expected);
        assert!(split_matches(
            original,
            actual,
            &HashMap::new(),
            &HashSet::from([wrapper]),
            qualified
        ));
    }
}

#[test]
fn native_derived_result_makes_split_body_target_local() {
    use crate::feature::{
        tests::reference_list, PmDcFeaturePropertyKind, PmDcFeatureReferenceFamily,
    };
    use crate::test_support::split::{plane_trim_fixture, set_kind};
    let (producer, mut inventory, wrapper) = native_derived_producer();
    let (source, mut properties) = plane_trim_fixture(false);
    set_kind(
        &mut properties,
        5,
        PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::BodySelection,
            items: reference_list(&[inventory.properties[1].identity.record_ordinal + 1]),
        },
    );
    let mut consumer = plain_feature("consumer", 6, Some(false));
    consumer.native_ref = Some(source.id());
    let original = FeatureDefinition::Operation(FeatureOperation::SplitBody {
        targets: BodySelection::Native(wrapper.clone()),
        tool: SplitBodyTool::Plane {
            plane: FeatureId::mint("test:model:feature#plane").expect("plane"),
        },
    });
    consumer.evaluation =
        cadmpeg_ir::features::FeatureEvaluation::from_definition(original.clone());
    inventory.features.push(source);
    inventory.properties.extend(properties);
    let mut model = Model::default();
    model.features = vec![producer, consumer];
    project(&mut model, &HashMap::new(), &inventory);
    let actual = model.features[1].evaluation.definition();
    assert_eq!(
        split_targets(actual),
        &BodySelection::local(vec![wrapper.clone()], wrapper.clone()).expect("local")
    );
    assert!(split_matches(
        original,
        actual,
        &HashMap::new(),
        &HashSet::from([wrapper]),
        true
    ));
    assert!(model.feature_result_topologies.is_empty());
}

fn check_native_base_split(
    ordinal: u64,
    suppressed: Option<bool>,
    joined: bool,
    select_source: bool,
) {
    use crate::feature::{
        tests::{reference_list, test_label},
        PmDcFeaturePropertyKind, PmDcFeatureReferenceFamily,
    };
    use crate::test_support::split::{plane_trim_fixture, set_kind, set_slot};
    let (mut producer, mut inventory, result) = native_derived_producer();
    producer.ordinal = ordinal;
    producer.suppressed = suppressed;
    producer.evaluation = cadmpeg_ir::features::FeatureEvaluation::from_definition(
        FeatureDefinition::Operation(FeatureOperation::Native {
            kind: cadmpeg_ir::features::NativeFeatureKind::Other("Base".into()),
            parameters: std::collections::BTreeMap::default(),
        }),
    );
    set_slot(
        &mut inventory.features[0],
        2,
        crate::pmdc::PmDcReference::from_packed(0),
    );
    inventory.labels = vec![test_label(10, 1, crate::feature::BASE_CLASS_ID, &[])];
    let selected = usize::from(!select_source);
    let wrapper = inventory.properties[selected].id();
    let (source, mut properties) = plane_trim_fixture(false);
    set_kind(
        &mut properties,
        5,
        PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::BodySelection,
            items: reference_list(&[inventory.properties[selected].identity.record_ordinal + 1]),
        },
    );
    let original = FeatureDefinition::Operation(FeatureOperation::SplitBody {
        targets: BodySelection::Native(wrapper.clone()),
        tool: SplitBodyTool::Plane {
            plane: FeatureId::mint("test:model:feature#plane").expect("plane"),
        },
    });
    let mut consumer = plain_feature("consumer", 6, Some(false));
    consumer.native_ref = Some(source.id());
    consumer.evaluation =
        cadmpeg_ir::features::FeatureEvaluation::from_definition(original.clone());
    inventory.features.push(source);
    inventory.properties.extend(properties);
    let mut model = Model::default();
    model.features = vec![producer, consumer];
    let bodies = if joined {
        HashMap::from([(result.clone(), body("base"))])
    } else {
        HashMap::new()
    };
    let earlier = produced_before(
        &model.features,
        &model.feature_result_topologies,
        &model.features[1],
        &ProducerRecords {
            features: &inventory.features,
            labels: &inventory.labels,
            properties: &inventory.properties,
        },
    );
    let eligible = ordinal < 6 && suppressed == Some(false);
    assert_eq!(
        earlier,
        if eligible {
            HashSet::from([result])
        } else {
            HashSet::new()
        }
    );
    project(&mut model, &bodies, &inventory);
    let expected = if joined {
        BodySelection::Resolved {
            bodies: vec![body("base")].try_into().expect("body"),
            native: wrapper,
        }
    } else if eligible && !select_source {
        BodySelection::local(vec![wrapper.clone()], wrapper).expect("local")
    } else {
        BodySelection::Native(wrapper)
    };
    let actual = model.features[1].evaluation.definition();
    assert_eq!(split_targets(actual), &expected);
    assert!(split_matches(original, actual, &bodies, &earlier, true));
    assert!(model.feature_result_topologies.is_empty());
}

#[test]
fn native_base_result_makes_split_body_target_local() {
    check_native_base_split(1, Some(false), false, false);
}

#[test]
fn later_unplaced_or_suppressed_native_base_keeps_split_target_native() {
    for (ordinal, suppressed) in [(9, Some(false)), (1, None), (1, Some(true))] {
        check_native_base_split(ordinal, suppressed, false, false);
    }
}

#[test]
fn native_base_result_current_body_join_wins_for_split() {
    check_native_base_split(1, Some(false), true, false);
}

#[test]
fn native_base_source_wrapper_keeps_split_target_native() {
    check_native_base_split(1, Some(false), false, true);
}
