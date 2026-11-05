// SPDX-License-Identifier: Apache-2.0
use crate::records::bodies::{DesignBodyBinding, DesignBodyBindingWire};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

fn fixture() -> (DesignParameterScope, DesignBodyBinding) {
    let scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        DesignFeatureKind::BaseFeature,
        10,
    );
    let binding = DesignBodyBinding::try_from(DesignBodyBindingWire {
        component_owner: None,
        component_owner_unresolved: false,
        history_only: false,
        id: "f3d:Design/BulkStream.dat:design-body-binding#10".to_owned(),
        stream: "Design/BulkStream.dat".to_owned(),
        pair_count: 1,
        pair_ordinal: 0,
        asm_body_key: 7,
        asm_body_key_offset: 10,
        entity_suffix: 20,
        entity_suffix_offset: 18,
        blob_name: "BREP.body".to_owned(),
        blob_name_offset: 30,
        body: Some(cadmpeg_ir::ids::BodyId::try_from("f3d:model:body#1").unwrap()),
    })
    .unwrap();
    (scope, binding)
}

fn project_copied_body(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    binding: &DesignBodyBinding,
) -> Result<Vec<cadmpeg_ir::features::Feature>, CodecError> {
    let timeline = crate::records::entity_header::DesignFeatureTimeline::try_new(
        crate::ids::native_design_feature_timeline_id_in_stream("f3d:Design/BulkStream.dat", 0),
        crate::records::entity_header::DesignTimelineFrame::test_items(
            0,
            vec![crate::records::identity::Located {
                value: 10,
                offset: 0,
            }],
        ),
        crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
        std::num::NonZeroU64::new(1).unwrap(),
        0,
        std::num::NonZeroU64::new(1).unwrap(),
    )
    .unwrap();
    crate::design::feature_project::project_parameter_design_with_edge_identities(
        ctx,
        &crate::design::feature_project::ProjectInputs {
            scopes: std::slice::from_ref(scope),
            timelines: std::slice::from_ref(&timeline),
            body_bindings: std::slice::from_ref(binding),
            ..Default::default()
        },
    )
    .map(|projected| projected.features)
}

fn copied_body_fixture() -> (DesignParameterScope, DesignBodyBinding) {
    use crate::records::feature::body_ops::{DesignCopiedBody, DesignCopyPasteBodiesOperation};
    let (_, binding) = fixture();
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        DesignFeatureKind::CopyPasteBodies,
        10,
    );
    scope.feature_ordinal = std::num::NonZeroU32::new(1).unwrap();
    if let crate::records::feature::scope::DesignScopePayloadMut::CopyPasteBodies(slot) =
        scope.payload_mut()
    {
        *slot = Some(
            DesignCopyPasteBodiesOperation::try_new(
                vec![DesignCopiedBody {
                    operand: crate::records::identity::Located {
                        value: 502,
                        offset: 26,
                    },
                    source: crate::records::identity::Located {
                        value: 10,
                        offset: 25,
                    },
                    copied: crate::records::identity::Located {
                        value: 20,
                        offset: 40,
                    },
                }],
                crate::records::feature::body_ops::CopyPasteRecordLocation {
                    record_index: 501,
                    class_tag: crate::records::references::DesignClassTag::try_from(
                        "264".to_owned(),
                    )
                    .unwrap(),
                    byte_offset: 0,
                },
                crate::records::feature::body_ops::CopyPasteRecordLocation {
                    record_index: 503,
                    class_tag: crate::records::references::DesignClassTag::try_from(
                        "264".to_owned(),
                    )
                    .unwrap(),
                    byte_offset: 0,
                },
            )
            .unwrap(),
        );
    }
    (scope, binding)
}

#[test]
fn copied_body_output_preserves_resolved_body() {
    let (scope, binding) = copied_body_fixture();
    let features = crate::test_support::with_decode_context(|decode_ctx| {
        project_copied_body(decode_ctx, &scope, &binding)
    })
    .unwrap();
    assert_eq!(features.len(), 1);
    assert_eq!(
        features[0].evaluation.outputs().as_slice(),
        [binding.body.as_ref().unwrap().clone()]
    );
}

#[test]
fn stored_body_map_requires_complete_unique_membership() {
    use cadmpeg_ir::features::BodySelection;
    let (scope, binding) = fixture();
    let selected = super::super::design_body_selection(
        &scope,
        [20].into_iter(),
        std::slice::from_ref(&binding),
    );
    assert!(matches!(selected, BodySelection::Resolved { bodies, .. }
        if bodies.as_slice() == [binding.body.clone().unwrap()]));
    for body in [
        None,
        Some(cadmpeg_ir::ids::BodyId::try_from("f3d:model:body#2").unwrap()),
    ] {
        let mut other = binding.clone();
        other.body = body;
        let selected = super::super::design_body_selection(
            &scope,
            [20].into_iter(),
            &[binding.clone(), other],
        );
        assert!(matches!(selected, BodySelection::Native(_)));
    }
    let selected = super::super::design_body_selection(
        &scope,
        [21].into_iter(),
        std::slice::from_ref(&binding),
    );
    assert!(matches!(selected, BodySelection::Native(_)));
    let selected = super::super::design_body_selection(&scope, [].into_iter(), &[]);
    assert!(matches!(selected, BodySelection::Resolved { bodies, .. } if bodies.is_empty()));
}

/// A current body at `suffix` whose Body container names `component`.
fn component_body(suffix: u64, body: &str, component: u64) -> DesignBodyBinding {
    use crate::records::body_ownership::{BodyOwnership, BodyOwnershipReferences};
    let reference = |target: u64| {
        let mut bytes = [0; 11];
        bytes[0] = 1;
        bytes[1..9].copy_from_slice(&target.to_le_bytes());
        cadmpeg_ir::native::bytes::NativeBytes::new(bytes)
    };
    let (_, mut binding) = fixture();
    binding.entity_suffix = suffix;
    binding.body = Some(cadmpeg_ir::ids::BodyId::try_from(body).unwrap());
    binding.component_owner = Some(BodyOwnership {
        container_record_index: 7,
        byte_offset: 0,
        byte_end: 26,
        body_count: 1,
        body_ordinal: 0,
        references: BodyOwnershipReferences::LocalSingle {
            body: reference(suffix),
            owner: reference(component),
        },
    });
    binding
}

/// A paste-owned Base Feature that places a copy of `source` as `copy`.
fn paste_scope(sources: &[u64], copies: &[u64]) -> DesignParameterScope {
    use crate::records::feature::base_feature::{
        DesignBaseFeatureConstruction, DesignBaseFeatureEntry, DesignPasteSource,
    };
    let (mut scope, _) = fixture();
    let entry = |value: u64| DesignBaseFeatureEntry {
        value,
        offset: 0,
        field: [0; 6],
    };
    if let crate::records::feature::scope::DesignScopePayloadMut::BaseFeature(slot) =
        scope.payload_mut()
    {
        *slot = Some(DesignBaseFeatureConstruction::TimelineOwnedBodies {
            parameter_bodies: copies.iter().map(|copy| entry(copy + 100)).collect(),
            bodies: copies.iter().copied().map(entry).collect(),
            owner_record: 9,
            owner_reference_offset: 0,
            copy_sources: sources
                .iter()
                .map(|entity_suffix| DesignPasteSource {
                    entity_suffix: *entity_suffix,
                    node_guid_offset: 0,
                })
                .collect(),
        });
    }
    scope
}

fn paste_copy(
    scope: &DesignParameterScope,
    bindings: &[DesignBodyBinding],
) -> Option<(
    cadmpeg_ir::features::FeatureDefinition,
    Vec<cadmpeg_ir::ids::BodyId>,
)> {
    super::super::same_component_paste_copy(
        scope,
        scope.base_feature_construction().unwrap(),
        bindings,
    )
}

#[test]
fn a_same_component_paste_of_copies_moves_one_copy_of_each_source_in_place() {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};
    let source = component_body(20, "f3d:model:body#1", 3);
    let copy = component_body(30, "f3d:model:body#2", 3);
    let (definition, copies) =
        paste_copy(&paste_scope(&[20], &[30]), &[source.clone(), copy.clone()])
            .expect("same-component copy");
    assert_eq!(copies, [copy.body.clone().unwrap()]);
    let FeatureDefinition::Operation(FeatureOperation::MoveBody {
        bodies: BodySelection::Resolved { bodies, .. },
        translation,
        rotation: None,
        copies: 1,
    }) = definition
    else {
        panic!("move with one copy: {definition:?}");
    };
    assert_eq!(bodies.as_slice(), [source.body.clone().unwrap()]);
    assert_eq!(
        translation.get(),
        cadmpeg_ir::math::Vector3::new(0.0, 0.0, 0.0)
    );
}

#[test]
fn a_paste_copy_needs_a_current_source_and_copy_in_one_component() {
    let source = component_body(20, "f3d:model:body#1", 3);
    let copy = component_body(30, "f3d:model:body#2", 3);
    let scope = paste_scope(&[20], &[30]);
    let mut history_only = source.clone();
    history_only.body = None;
    let mut unowned = source.clone();
    unowned.component_owner = None;
    let other_component = component_body(30, "f3d:model:body#2", 282);
    for bindings in [
        vec![history_only, copy.clone()],
        vec![unowned, copy.clone()],
        vec![source.clone(), other_component],
        vec![source.clone(), source.clone(), copy.clone()],
        vec![source.clone()],
    ] {
        assert!(paste_copy(&scope, &bindings).is_none(), "{bindings:?}");
    }
    for (sources, copies) in [(&[][..], &[30][..]), (&[], &[])] {
        assert!(paste_copy(
            &paste_scope(sources, copies),
            &[source.clone(), copy.clone()]
        )
        .is_none());
    }
}
