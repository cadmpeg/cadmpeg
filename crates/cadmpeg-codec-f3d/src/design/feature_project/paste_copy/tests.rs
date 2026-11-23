// SPDX-License-Identifier: Apache-2.0

use crate::design::feature_project::tests::body_selection::fixture;
use crate::records::bodies::DesignBodyBinding;
use crate::records::feature::scope::DesignParameterScope;

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

/// A paste-owned Base Feature that places a copy of each of `sources` as the
/// body at the same position in `copies`.
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
            owner_timeline_name: None,
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
    super::same_component_paste_copy(scope, scope.base_feature_construction().unwrap(), bindings)
}

#[test]
fn a_same_component_paste_of_copies_moves_one_copy_of_each_source_in_place() {
    use cadmpeg_ir::features::selections::BodySelection;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
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

#[test]
fn a_paste_copy_needs_distinct_copies_that_are_not_its_sources() {
    let bindings = [
        component_body(20, "f3d:model:body#1", 3),
        component_body(21, "f3d:model:body#2", 3),
        component_body(30, "f3d:model:body#3", 3),
        component_body(31, "f3d:model:body#4", 3),
    ];
    assert!(paste_copy(&paste_scope(&[20, 21], &[30, 31]), &bindings).is_some());
    for (sources, copies) in [
        (&[20, 21][..], &[30, 30][..]),
        (&[20], &[20]),
        (&[20, 21], &[30, 20]),
    ] {
        assert!(
            paste_copy(&paste_scope(sources, copies), &bindings).is_none(),
            "{sources:?} -> {copies:?}"
        );
    }
}

#[test]
fn a_paste_copy_keeps_every_pair_in_one_component() {
    let bindings = |second_component: u64| {
        [
            component_body(20, "f3d:model:body#1", 3),
            component_body(30, "f3d:model:body#3", 3),
            component_body(21, "f3d:model:body#2", second_component),
            component_body(31, "f3d:model:body#4", second_component),
        ]
    };
    let scope = paste_scope(&[20, 21], &[30, 31]);
    assert!(paste_copy(&scope, &bindings(3)).is_some());
    assert!(paste_copy(&scope, &bindings(282)).is_none());
}

fn cross_component_paste(
    scope: &DesignParameterScope,
    bindings: &[DesignBodyBinding],
) -> Option<(
    cadmpeg_ir::features::FeatureDefinition,
    Vec<cadmpeg_ir::ids::BodyId>,
)> {
    super::cross_component_paste(scope, scope.base_feature_construction().unwrap(), bindings)
}

#[test]
fn a_paste_into_a_component_without_its_sources_inserts_its_copies() {
    use cadmpeg_ir::features::selections::InsertedBodies;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    let bindings = [
        component_body(20, "f3d:model:body#1", 3),
        component_body(21, "f3d:model:body#2", 4),
        component_body(30, "f3d:model:body#3", 282),
        component_body(31, "f3d:model:body#4", 282),
    ];
    let scope = paste_scope(&[20, 21], &[30, 31]);
    let (definition, copies) =
        cross_component_paste(&scope, &bindings).expect("cross-component paste");
    assert_eq!(
        copies,
        [
            bindings[2].body.clone().unwrap(),
            bindings[3].body.clone().unwrap()
        ]
    );
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::InsertBodies {
            bodies: InsertedBodies::Resolved { native },
        }) if native == scope.id
    ));
    assert!(paste_copy(&scope, &bindings).is_none());
}

#[test]
fn an_inserting_paste_needs_its_copies_in_one_component_that_holds_no_source() {
    let source = component_body(20, "f3d:model:body#1", 3);
    let copy = component_body(30, "f3d:model:body#3", 282);
    let other_copy = component_body(31, "f3d:model:body#4", 283);
    let source_beside_copy = component_body(21, "f3d:model:body#2", 282);
    let mut history_only = copy.clone();
    history_only.body = None;
    let mut unowned = source.clone();
    unowned.component_owner = None;
    for (sources, copies, bindings) in [
        (
            &[20, 21][..],
            &[30, 31][..],
            vec![
                source.clone(),
                source_beside_copy.clone(),
                copy.clone(),
                other_copy,
            ],
        ),
        (
            &[20, 21],
            &[30, 30],
            vec![source.clone(), source_beside_copy.clone(), copy.clone()],
        ),
        (&[21], &[30], vec![source_beside_copy, copy.clone()]),
        (&[20], &[30], vec![source.clone(), history_only]),
        (&[20], &[30], vec![unowned, copy.clone()]),
        (&[20], &[30], vec![copy.clone()]),
        (&[], &[30], vec![source.clone(), copy.clone()]),
    ] {
        assert!(
            cross_component_paste(&paste_scope(sources, copies), &bindings).is_none(),
            "{sources:?} -> {copies:?}"
        );
    }
}
