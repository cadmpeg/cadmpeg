// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::scopes::derived_instance::construction;
use crate::records::feature::assembly_features::{
    DesignComponentOccurrence, DesignDerivedInstanceConstruction, DesignDerivedInstancePlacement,
};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::features::{
    Feature, FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation, InsertedComponents,
};
use std::collections::{BTreeMap, BTreeSet};

const COMPONENT: &str = "11111111-2222-4333-8444-555555555555";
const OCCURRENCE: &str = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
const TRAILER: &str = "12345678-1234-4234-8234-123456789abc";
const IDENTITY: [[f64; 4]; 4] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

fn read(
    bytes: &[u8],
    scope: &DesignParameterScope,
    occurrences: &[DesignComponentOccurrence],
) -> Option<DesignDerivedInstanceConstruction> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::default()).unwrap();
    let records = crate::design::test_support::indexed_record_offsets_for_test(bytes);
    construction(&ctx, bytes, &records, scope, occurrences).unwrap()
}

#[test]
fn counted_source_matrices_bind_forward_relations_without_class_numbers() {
    let (bytes, scope, occurrences) = fixture(&[(700, Some(IDENTITY))], None, false);
    let construction = read(&bytes, &scope, &occurrences).unwrap();
    assert_eq!(construction.reference.target, 305);
    assert_eq!(construction.placements.len(), 1);
    assert_eq!(construction.placements[0].reference.target, 700);
    assert!(matches!(
        construction.placements[0].placement,
        DesignDerivedInstancePlacement::Matrix { offset: 48, .. }
    ));
    assert_eq!(construction.placements[0].placement.value(), IDENTITY);
    assert_eq!(
        construction.auxiliary_pairs[0].auxiliary.target,
        4_294_967_301
    );
    assert_eq!(construction.auxiliary_pairs[0].relation.target, 700);
    assert_eq!(construction.primary_reference.target, 700);
    assert_eq!(construction.trailer_guid.as_str(), TRAILER);
    let relation = construction.sources[0].relation.as_ref().unwrap();
    assert_eq!(relation.carrier_reference.target, 800);
    assert_eq!(relation.middle_reference.target, 1000);
    let carrier = relation.carrier.as_ref().unwrap();
    assert_eq!(carrier.component_record_index, 305);
    assert_eq!(carrier.occurrence_guid.as_str(), OCCURRENCE);
    assert!(construction.sources[0].relation.is_some());
}

#[test]
fn omitted_source_and_identity_selector_are_distinct_placements() {
    let (bytes, scope, occurrences) = fixture(&[], None, false);
    let omitted = read(&bytes, &scope, &occurrences).unwrap();
    assert!(omitted.placements.is_empty());
    assert!(omitted.auxiliary_pairs.is_empty());
    assert_eq!(omitted.placement_for(700), None);
    assert!(matches!(
        omitted.context_placement,
        DesignDerivedInstancePlacement::Identity { flag_offset: 60 }
    ));
    assert!(omitted.sources[0]
        .relation
        .as_ref()
        .unwrap()
        .carrier
        .is_some());
    let (bytes, scope, occurrences) = fixture(&[(700, None)], None, false);
    let identity = read(&bytes, &scope, &occurrences).unwrap();
    assert!(matches!(
        identity.placement_for(700),
        Some(DesignDerivedInstancePlacement::Identity { flag_offset: 47 })
    ));
    assert_eq!(identity.placement_for(700).unwrap().value(), IDENTITY);
}

#[test]
fn source_and_reference_orders_and_the_context_matrix_remain_independent() {
    let mut context = IDENTITY;
    context[0][0] = 2.0;
    context[1][3] = -3.5;
    let (bytes, scope, occurrences) =
        fixture(&[(701, None), (700, Some(IDENTITY))], Some(context), true);
    let construction = read(&bytes, &scope, &occurrences).unwrap();
    assert_eq!(
        construction
            .placements
            .iter()
            .map(|p| p.reference.target)
            .collect::<Vec<_>>(),
        [701, 700]
    );
    assert_eq!(
        construction
            .sources
            .iter()
            .map(|s| s.reference.target)
            .collect::<Vec<_>>(),
        [700, 701]
    );
    assert_eq!(
        construction
            .auxiliary_pairs
            .iter()
            .map(|p| p.relation.target)
            .collect::<Vec<_>>(),
        [700, 701]
    );
    assert_eq!(construction.context_placement.value(), context);
    assert!(construction
        .sources
        .iter()
        .all(|s| s.relation.as_ref().unwrap().carrier.is_some()));
    assert_eq!(
        construction.sources[1]
            .relation
            .as_ref()
            .unwrap()
            .middle_reference
            .target,
        1001
    );
}

#[test]
fn missing_carriers_and_invalid_backlinks_preserve_the_decoded_prefix() {
    let (mut bytes, scope, occurrences) = fixture(&[(700, Some(IDENTITY))], None, false);
    let missing = read(&bytes, &scope, &[]).unwrap();
    assert_eq!(missing.placements[0].placement.value(), IDENTITY);
    assert!(missing.sources[0]
        .relation
        .as_ref()
        .unwrap()
        .carrier
        .is_none());
    let relation_at = usize::try_from(scope.paired_byte_offset()).unwrap() + 11;
    bytes[relation_at + 47..relation_at + 51].copy_from_slice(&901u32.to_le_bytes());
    let unbound = read(&bytes, &scope, &occurrences).unwrap();
    assert_eq!(unbound.sources[0].relation, None);
    assert_eq!(unbound.placements.len(), 1);
}

#[test]
fn repeated_relations_must_match_the_interleaved_pairs_and_matrices_must_be_finite() {
    let (mut bytes, scope, occurrences) = fixture(&[(700, Some(IDENTITY))], None, false);
    bytes[207..211].copy_from_slice(&701u32.to_le_bytes());
    assert!(read(&bytes, &scope, &occurrences).is_none());
    let (mut bytes, scope, occurrences) = fixture(&[(700, Some(IDENTITY))], None, false);
    bytes[48..56].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(read(&bytes, &scope, &occurrences).is_none());
    let (mut bytes, scope, occurrences) = fixture(&[(700, Some(IDENTITY))], None, false);
    bytes[32..36].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(read(&bytes, &scope, &occurrences).is_none());
}

#[test]
fn competing_relation_frames_do_not_choose_one_carrier() {
    let (mut bytes, scope, occurrences) = fixture(&[(700, Some(IDENTITY))], None, false);
    let relation_at = usize::try_from(scope.paired_byte_offset()).unwrap() + 11;
    let relation = bytes[relation_at..relation_at + 57].to_vec();
    bytes.truncate(bytes.len() - 11);
    bytes.extend_from_slice(&relation);
    header(&mut bytes, b"735", 1002);
    let construction = read(&bytes, &scope, &occurrences).unwrap();
    assert_eq!(construction.sources[0].relation, None);
}

fn fixture(
    placements: &[(u32, Option<[[f64; 4]; 4]>)],
    context: Option<[[f64; 4]; 4]>,
    extended: bool,
) -> (
    Vec<u8>,
    DesignParameterScope,
    Vec<DesignComponentOccurrence>,
) {
    reference_fixture(placements, context, extended, false)
}

fn reference_fixture(
    placements: &[(u32, Option<[[f64; 4]; 4]>)],
    context: Option<[[f64; 4]; 4]>,
    extended: bool,
    typed: bool,
) -> (
    Vec<u8>,
    DesignParameterScope,
    Vec<DesignComponentOccurrence>,
) {
    let relation_type = typed.then_some("2D6E13A1-BBEF-4FB3-9277-00D99F634136");
    let mut bytes = Vec::new();
    header(&mut bytes, b"731", 900);
    bytes.extend_from_slice(&[0; 10]);
    reference(
        &mut bytes,
        305,
        typed.then_some("71BBEDD5-81F8-4585-816B-05908C207204"),
    );
    bytes.extend_from_slice(&u32::try_from(placements.len()).unwrap().to_le_bytes());
    for (relation, matrix) in placements {
        reference(&mut bytes, u64::from(*relation), relation_type);
        placement(&mut bytes, *matrix);
    }
    let relations = if placements.len() == 2 {
        vec![700, 701]
    } else {
        vec![700]
    };
    let pairs = if placements.is_empty() {
        &[][..]
    } else {
        relations.as_slice()
    };
    bytes.extend_from_slice(&u32::try_from(pairs.len()).unwrap().to_le_bytes());
    for relation in pairs {
        reference(
            &mut bytes,
            4_294_967_301,
            typed.then_some("2E3AC990-483D-4A55-8662-C064B976A957"),
        );
        reference(&mut bytes, u64::from(*relation), relation_type);
    }
    bytes.extend_from_slice(&u32::try_from(pairs.len()).unwrap().to_le_bytes());
    for relation in pairs {
        reference(&mut bytes, u64::from(*relation), relation_type);
    }
    bytes.extend_from_slice(&[0; 4]);
    reference(&mut bytes, 700, relation_type);
    bytes.push(1);
    placement(&mut bytes, context);
    crate::test_support::lp_utf16(&mut bytes, TRAILER);
    bytes.extend_from_slice(&[0; 3]);
    let prefix_len = u64::try_from(bytes.len()).unwrap();
    bytes.extend_from_slice(&u32::try_from(relations.len()).unwrap().to_le_bytes());
    for relation in &relations {
        reference(&mut bytes, u64::from(*relation), relation_type);
    }
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    crate::test_support::lp_utf16(&mut bytes, "DerivedInstance");
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#0",
        DesignFeatureKind::DerivedInstance,
        900,
    );
    let stride = if typed { 51 } else { 11 };
    scope
        .try_edit(|draft| {
            draft.reference_count_offset = prefix_len;
            draft.reference_members = crate::records::identity::ReferenceRun::located(
                relations
                    .iter()
                    .enumerate()
                    .map(|(ordinal, value)| crate::records::identity::Located {
                        value: *value,
                        offset: prefix_len + 5 + stride * u64::try_from(ordinal).unwrap(),
                    })
                    .collect(),
            );
            draft.kind_offset = prefix_len + 12 + stride * u64::try_from(relations.len()).unwrap();
            draft.paired_byte_offset = draft.kind_offset + if typed { 252 } else { 128 };
            draft.frame_length = draft.paired_byte_offset;
            draft.layout_fixture_tail();
            if typed {
                draft.feature_ordinal_offset = draft.paired_byte_offset - 196;
            }
        })
        .unwrap();
    scope.class_tag = "731".to_owned().try_into().unwrap();
    scope.paired_class_tag = "732".to_owned().try_into().unwrap();
    bytes.resize(usize::try_from(scope.paired_byte_offset()).unwrap(), 0);
    let ordinal_at = usize::try_from(scope.feature_ordinal_offset()).unwrap();
    bytes[ordinal_at..ordinal_at + 4].copy_from_slice(&1u32.to_le_bytes());
    header(&mut bytes, b"732", 900);
    let mut occurrences = Vec::new();
    for (ordinal, relation) in relations.into_iter().enumerate() {
        let at = bytes.len();
        header(&mut bytes, b"733", relation);
        bytes.extend_from_slice(&[0; 10]);
        reference(
            &mut bytes,
            800 + u64::try_from(ordinal).unwrap(),
            typed.then_some("CE2913AA-CFE0-4F04-9102-24424ED3BCFA"),
        );
        bytes.extend_from_slice(if extended {
            &[1, 0, 0][..]
        } else {
            &[0, 0][..]
        });
        reference(
            &mut bytes,
            1000 + u64::try_from(ordinal).unwrap(),
            typed.then_some("6CCF41D5-40BE-48ED-A834-18F3EAED6C57"),
        );
        bytes.push(0);
        reference(
            &mut bytes,
            900,
            typed.then_some("66E375EA-40F1-4115-8D83-0F23DD30593E"),
        );
        assert_eq!(
            bytes.len() - at,
            if typed {
                if extended {
                    178
                } else {
                    177
                }
            } else if extended {
                58
            } else {
                57
            }
        );
        let key = if ordinal == 0 {
            OCCURRENCE
        } else {
            "bbbbbbbb-cccc-4ddd-8eee-ffffffffffff"
        };
        occurrences.push(DesignComponentOccurrence::try_new(crate::records::feature::assembly_features::DesignComponentOccurrenceDraft {
            id:format!("f3d:Design/BulkStream.dat:design-component-occurrence#{}",800+ordinal),class_tag:"734".to_owned().try_into().unwrap(),record_index:800+u32::try_from(ordinal).unwrap(),byte_offset:0,component_record_index:305,component_guid:COMPONENT.to_owned().try_into().unwrap(),occurrence_guid:key.to_owned().try_into().unwrap(),placement:crate::records::feature::assembly_features::DesignComponentOccurrencePlacement::Explicit {
                ordinal:std::num::NonZeroU32::MIN,transform:IDENTITY.try_into().unwrap(),
            },owner_entity:0,
        }).unwrap());
    }
    header(&mut bytes, b"735", 1002);
    (bytes, scope, occurrences)
}

fn reference(bytes: &mut Vec<u8>, target: u64, type_guid: Option<&str>) {
    bytes.push(1);
    bytes.extend_from_slice(&target.to_le_bytes());
    if let Some(guid) = type_guid {
        bytes.extend_from_slice(&36u32.to_le_bytes());
        bytes.extend_from_slice(guid.as_bytes());
    }
    bytes.extend_from_slice(&[0, 0]);
}

fn header(bytes: &mut Vec<u8>, tag: &[u8; 3], record: u32) {
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(tag);
    bytes.extend_from_slice(&record.to_le_bytes());
}
fn placement(bytes: &mut Vec<u8>, matrix: Option<[[f64; 4]; 4]>) {
    match matrix {
        None => bytes.push(1),
        Some(matrix) => {
            bytes.push(0);
            for value in matrix.into_iter().flatten() {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
    }
}

#[test]
fn decoded_multiple_sources_share_their_component_record_definition() {
    let (bytes, mut scope, mut occurrences) =
        fixture(&[(701, None), (700, Some(IDENTITY))], None, true);
    occurrences[1].component_guid = "22222222-3333-4444-8555-666666666666"
        .to_owned()
        .try_into()
        .unwrap();
    let construction = read(&bytes, &scope, &occurrences).unwrap();
    if let crate::records::feature::scope::DesignScopePayloadMut::DerivedInstance(slot) =
        scope.payload_mut()
    {
        *slot = Some(construction);
    }
    let projection = project(&scope, &occurrences, &[], &BTreeSet::new());
    assert_eq!(projection.definitions.len(), 1);
    assert_eq!(projection.occurrences.len(), 2);
    assert_eq!(
        projection.definitions[0].id,
        crate::ids::neutral_component_record_id("f3d:Design/BulkStream.dat", 305)
    );
    assert!(projection.losses.iter().any(|loss| loss.code
        == crate::loss::F3dLossCode::FeatureDerivedInstanceDefinitionUnresolved.kind()));
    let paths = crate::design::components::paths::LocalOccurrencePaths::new(
        &occurrences,
        &[],
        &projection.occurrences,
        std::slice::from_ref(&scope),
    );
    let mut feature = derived_feature(&scope);
    assert!(
        crate::design::components::project_derived_instance_features(
            std::slice::from_mut(&mut feature),
            std::slice::from_ref(&scope),
            &paths,
            &projection.occurrences
        )
        .unwrap()
        .is_empty()
    );
    assert!(
        matches!(feature.evaluation.definition(),FeatureDefinition::Operation(FeatureOperation::InsertComponents {
        components:InsertedComponents::Resolved { occurrences: selected,native }
    }) if selected.len()==2 && selected[0]==projection.occurrences[0].id && native.as_str()==scope.id)
    );
}

#[test]
fn one_missing_source_keeps_the_known_occurrence_and_all_source_matrices() {
    let (bytes, mut scope, occurrences) =
        fixture(&[(701, None), (700, Some(IDENTITY))], None, false);
    let construction = read(&bytes, &scope, &occurrences[..1]).unwrap();
    assert_eq!(construction.placements.len(), 2);
    assert!(construction.sources[1]
        .relation
        .as_ref()
        .unwrap()
        .carrier
        .is_none());
    if let crate::records::feature::scope::DesignScopePayloadMut::DerivedInstance(slot) =
        scope.payload_mut()
    {
        *slot = Some(construction);
    }
    let projection = project(&scope, &occurrences[..1], &[], &BTreeSet::new());
    assert_eq!(projection.occurrences.len(), 1);
    let paths = crate::design::components::paths::LocalOccurrencePaths::new(
        &occurrences[..1],
        &[],
        &projection.occurrences,
        std::slice::from_ref(&scope),
    );
    let mut feature = derived_feature(&scope);
    let losses = crate::design::components::project_derived_instance_features(
        std::slice::from_mut(&mut feature),
        std::slice::from_ref(&scope),
        &paths,
        &projection.occurrences,
    )
    .unwrap();
    assert!(
        matches!(feature.evaluation.definition(),FeatureDefinition::Operation(FeatureOperation::InsertComponents {
        components:InsertedComponents::Partial { occurrences:selected,native }
    }) if selected.as_slice()==[projection.occurrences[0].id.clone()] && native.as_str()==scope.id)
    );
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::F3dLossCode::FeatureDerivedInstanceSourcesUnresolved.kind()
    );
}

#[test]
fn source_pose_disagreement_and_omission_have_separate_losses() {
    let mut source = IDENTITY;
    source[0][3] = 2.5;
    let mut context = IDENTITY;
    context[1][3] = 7.0;
    for (placements, expected, code) in [
        (
            vec![(700, Some(source))],
            25.0,
            crate::loss::F3dLossCode::FeatureDerivedInstanceScopePlacementAssumed,
        ),
        (
            vec![],
            0.0,
            crate::loss::F3dLossCode::FeatureDerivedInstanceCarrierPlacementAssumed,
        ),
    ] {
        let (bytes, mut scope, occurrences) = fixture(&placements, Some(context), false);
        let construction = read(&bytes, &scope, &occurrences).unwrap();
        if let crate::records::feature::scope::DesignScopePayloadMut::DerivedInstance(slot) =
            scope.payload_mut()
        {
            *slot = Some(construction);
        }
        let projection = project(&scope, &occurrences, &[], &BTreeSet::new());
        assert_eq!(projection.occurrences.len(), 1);
        assert_eq!(projection.occurrences[0].transform.rows()[0][3], expected);
        assert_eq!(projection.occurrences[0].transform.rows()[1][3], 0.0);
        assert!(projection
            .losses
            .iter()
            .any(|loss| loss.code == code.kind()));
        assert!(projection.losses.iter().any(|loss| loss.code
            == crate::loss::F3dLossCode::FeatureDerivedInstanceContextTransformAssumed.kind()));
    }
}

fn derived_feature(scope: &DesignParameterScope) -> Feature {
    Feature {
        product_definition: None,
        id: FeatureId::mint("f3d:model:feature#derived").unwrap(),
        ordinal: 1,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::default(),
        source_tag: Some("DerivedInstance".into()),
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Native {
                kind: "DerivedInstance".into(),
                parameters: BTreeMap::default(),
            },
        )),
        native_ref: Some(scope.id.clone()),
    }
}

#[test]
fn partial_component_definition_retains_explicit_body_membership() {
    let (bytes, mut scope, occurrences) = fixture(&[(700, Some(IDENTITY))], None, false);
    let construction = read(&bytes, &scope, &occurrences).unwrap();
    if let crate::records::feature::scope::DesignScopePayloadMut::DerivedInstance(slot) =
        scope.payload_mut()
    {
        *slot = Some(construction);
    }
    let mut ownership = 1u32.to_le_bytes().to_vec();
    for (target, guid) in [
        (42u64, "D3937028-C20C-4E65-B010-94AD418A5C20"),
        (305u64, "E03784ED-5E19-4E14-B9F2-3B07017018CD"),
    ] {
        ownership.push(1);
        ownership.extend_from_slice(&target.to_le_bytes());
        ownership.extend_from_slice(&36u32.to_le_bytes());
        ownership.extend_from_slice(guid.as_bytes());
        ownership.extend_from_slice(&[0, 0]);
    }
    let proof = crate::records::body_ownership::membership(&ownership)
        .unwrap()
        .proof(1200, 0, 0)
        .unwrap();
    let body = cadmpeg_ir::ids::BodyId::mint("f3d:brep/fixture.smbh/brep:entity#1").unwrap();
    let binding:crate::records::bodies::DesignBodyBinding=serde_json::from_value(serde_json::json!({
        "id":"f3d:Design/BulkStream.dat:design-body-binding#0","stream":"Design/BulkStream.dat","pair_count":1,"pair_ordinal":0,"asm_body_key":11,"asm_body_key_offset":0,"entity_suffix":42,"entity_suffix_offset":8,"blob_name":"BREP.","blob_name_offset":32,"body":body,"component_owner":proof,
    })).unwrap();
    let bodies = BTreeSet::from([body.clone()]);
    let projection = project(&scope, &occurrences, &[binding], &bodies);
    assert_eq!(projection.definitions.len(), 1);
    assert_eq!(projection.definitions[0].bodies, [body]);
    assert!(projection.losses.iter().any(|loss| loss.code
        == crate::loss::F3dLossCode::FeatureDerivedInstanceDefinitionUnresolved.kind()));
}

#[test]
fn ambiguous_occurrence_identity_retains_an_unresolved_component_selection() {
    let (bytes, mut scope, mut occurrences) = fixture(&[(700, None), (701, None)], None, false);
    occurrences[1].occurrence_guid = occurrences[0].occurrence_guid.clone();
    let construction = read(&bytes, &scope, &occurrences).unwrap();
    if let crate::records::feature::scope::DesignScopePayloadMut::DerivedInstance(slot) =
        scope.payload_mut()
    {
        *slot = Some(construction);
    }
    let projection = project(&scope, &occurrences, &[], &BTreeSet::new());
    assert!(projection.occurrences.is_empty());
    let paths = crate::design::components::paths::LocalOccurrencePaths::new(
        &occurrences,
        &[],
        &projection.occurrences,
        std::slice::from_ref(&scope),
    );
    let mut feature = derived_feature(&scope);
    let losses = crate::design::components::project_derived_instance_features(
        std::slice::from_mut(&mut feature),
        std::slice::from_ref(&scope),
        &paths,
        &projection.occurrences,
    )
    .unwrap();
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::InsertComponents {
            components: InsertedComponents::Native { .. }
        })
    ));
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::F3dLossCode::FeatureDerivedInstanceSourcesUnresolved.kind()
    );
}

#[test]
fn repeated_relation_order_stays_distinct_from_the_auxiliary_pairs() {
    let (mut bytes, scope, occurrences) =
        fixture(&[(701, None), (700, Some(IDENTITY))], None, false);
    bytes[241..249].copy_from_slice(&701u64.to_le_bytes());
    bytes[252..260].copy_from_slice(&700u64.to_le_bytes());
    let construction = read(&bytes, &scope, &occurrences).unwrap();
    assert_eq!(
        construction
            .auxiliary_pairs
            .iter()
            .map(|pair| pair.relation.target)
            .collect::<Vec<_>>(),
        [700, 701]
    );
    assert_eq!(
        construction
            .repeated_references
            .iter()
            .map(|reference| reference.target)
            .collect::<Vec<_>>(),
        [701, 700]
    );
}

fn project(
    scope: &DesignParameterScope,
    occurrences: &[DesignComponentOccurrence],
    bindings: &[crate::records::bodies::DesignBodyBinding],
    bodies: &BTreeSet<cadmpeg_ir::ids::BodyId>,
) -> crate::design::components::LocalComponentProjection {
    crate::design::components::project_local_components(
        &crate::design::components::LocalComponentSources {
            scopes: std::slice::from_ref(scope),
            occurrences,
            paths: &[],
            components: &BTreeMap::new(),
            body_bindings: bindings,
            bodies: Some(bodies),
            hidden_bodies: None,
            root_ordinal_start: 0,
            limits: cadmpeg_core::decode::ResourceLimits::default(),
        },
    )
    .unwrap()
}

#[test]
fn typed_local_references_preserve_their_guid_metadata_and_matrix_offsets() {
    let (bytes, scope, occurrences) =
        reference_fixture(&[(700, Some(IDENTITY))], None, false, true);
    let decoded = read(&bytes, &scope, &occurrences).unwrap();
    assert_eq!(decoded.reference.target, 305);
    assert_eq!(
        decoded.reference.type_guid.as_ref().unwrap().as_str(),
        "71BBEDD5-81F8-4585-816B-05908C207204"
    );
    assert_eq!(decoded.placements[0].reference.target, 700);
    assert_eq!(
        decoded.placements[0]
            .reference
            .type_guid
            .as_ref()
            .unwrap()
            .as_str(),
        "2D6E13A1-BBEF-4FB3-9277-00D99F634136"
    );
    assert!(matches!(
        decoded.placements[0].placement,
        DesignDerivedInstancePlacement::Matrix { offset: 128, .. }
    ));
    assert_eq!(decoded.auxiliary_pairs[0].auxiliary.target, 4_294_967_301);
    assert_eq!(
        decoded.auxiliary_pairs[0]
            .auxiliary
            .type_guid
            .as_ref()
            .unwrap()
            .as_str(),
        "2E3AC990-483D-4A55-8662-C064B976A957"
    );
    assert_eq!(decoded.repeated_references[0].target, 700);
    assert_eq!(decoded.primary_reference.target, 700);
    assert_eq!(
        decoded.sources[0]
            .reference
            .type_guid
            .as_ref()
            .unwrap()
            .as_str(),
        "2D6E13A1-BBEF-4FB3-9277-00D99F634136"
    );
    let relation = decoded.sources[0].relation.as_ref().unwrap();
    assert_eq!(relation.carrier_reference.target, 800);
    assert_eq!(
        relation
            .carrier_reference
            .type_guid
            .as_ref()
            .unwrap()
            .as_str(),
        "CE2913AA-CFE0-4F04-9102-24424ED3BCFA"
    );
    assert_eq!(relation.middle_reference.target, 1000);
    assert_eq!(
        relation
            .middle_reference
            .type_guid
            .as_ref()
            .unwrap()
            .as_str(),
        "6CCF41D5-40BE-48ED-A834-18F3EAED6C57"
    );
    assert_eq!(relation.scope_reference.target, 900);
    assert_eq!(
        relation
            .scope_reference
            .type_guid
            .as_ref()
            .unwrap()
            .as_str(),
        "66E375EA-40F1-4115-8D83-0F23DD30593E"
    );
    assert!(relation.carrier.is_some());
    let wire = serde_json::to_value(&decoded).unwrap();
    assert_eq!(
        serde_json::from_value::<DesignDerivedInstanceConstruction>(wire).unwrap(),
        decoded
    );
}

#[test]
fn typed_source_sets_project_all_decoded_occurrences() {
    let (bytes, mut scope, occurrences) =
        reference_fixture(&[(701, None), (700, Some(IDENTITY))], None, false, true);
    let decoded = read(&bytes, &scope, &occurrences).unwrap();
    if let crate::records::feature::scope::DesignScopePayloadMut::DerivedInstance(slot) =
        scope.payload_mut()
    {
        *slot = Some(decoded);
    }
    let projection = project(&scope, &occurrences, &[], &BTreeSet::new());
    assert_eq!(projection.occurrences.len(), 2);
    let paths = crate::design::components::paths::LocalOccurrencePaths::new(
        &occurrences,
        &[],
        &projection.occurrences,
        std::slice::from_ref(&scope),
    );
    let mut feature = derived_feature(&scope);
    assert!(
        crate::design::components::project_derived_instance_features(
            std::slice::from_mut(&mut feature),
            std::slice::from_ref(&scope),
            &paths,
            &projection.occurrences
        )
        .unwrap()
        .is_empty()
    );
    assert!(
        matches!(feature.evaluation.definition(),FeatureDefinition::Operation(FeatureOperation::InsertComponents {components:InsertedComponents::Resolved {occurrences:selected,..}}) if selected.len()==2)
    );
}

#[test]
fn an_unknown_generic_reference_type_keeps_the_decoded_prefix_and_unresolved_source() {
    let (mut bytes, scope, occurrences) =
        reference_fixture(&[(700, Some(IDENTITY))], None, false, true);
    bytes[570..606].copy_from_slice(b"11111111-2222-4333-8444-555555555555");
    let decoded = read(&bytes, &scope, &occurrences).unwrap();
    assert_eq!(decoded.placements[0].placement.value(), IDENTITY);
    assert_eq!(
        decoded.sources[0]
            .reference
            .type_guid
            .as_ref()
            .unwrap()
            .as_str(),
        "11111111-2222-4333-8444-555555555555"
    );
    assert_eq!(decoded.sources[0].relation, None);
}

#[test]
fn a_conflicting_placement_reference_type_uses_the_carrier_with_a_loss() {
    let mut matrix = IDENTITY;
    matrix[0][3] = 2.5;
    let (mut bytes, mut scope, occurrences) =
        reference_fixture(&[(700, Some(matrix))], None, false, true);
    bytes[89..125].copy_from_slice(b"11111111-2222-4333-8444-555555555555");
    let decoded = read(&bytes, &scope, &occurrences).unwrap();
    assert_eq!(decoded.placements[0].placement.value(), matrix);
    if let crate::records::feature::scope::DesignScopePayloadMut::DerivedInstance(slot) =
        scope.payload_mut()
    {
        *slot = Some(decoded);
    }
    let projection = project(&scope, &occurrences, &[], &BTreeSet::new());
    assert_eq!(projection.occurrences.len(), 1);
    assert_eq!(projection.occurrences[0].transform.rows()[0][3], 0.0);
    assert!(projection.losses.iter().any(|loss| loss.code
        == crate::loss::F3dLossCode::FeatureDerivedInstanceCarrierPlacementAssumed.kind()));
}

#[test]
fn a_reference_guid_budget_refusal_is_not_a_native_form_fallback() {
    let (bytes, scope, occurrences) = reference_fixture(&[(700, None)], None, false, true);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let error = construction(&ctx, &bytes, &records, &scope, &occurrences).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
}

#[test]
fn centimetre_overflow_retains_native_membership_and_can_use_a_finite_carrier() {
    use crate::records::feature::assembly_features::{
        DesignComponentOccurrenceDraft, DesignComponentOccurrencePlacement,
    };
    let mut large = IDENTITY;
    large[0][3] = 1.0e308;
    for (carrier_matrix, count, current, code) in [
        (
            large,
            0,
            false,
            crate::loss::F3dLossCode::FeatureDerivedInstancePlacementUnrepresentable,
        ),
        (
            IDENTITY,
            1,
            false,
            crate::loss::F3dLossCode::FeatureDerivedInstanceCarrierPlacementAssumed,
        ),
        (
            large,
            2,
            true,
            crate::loss::F3dLossCode::FeatureDerivedInstancePlacementUnrepresentable,
        ),
    ] {
        let (bytes, mut scope, _) = fixture(&[(700, Some(large))], None, false);
        let mut occurrences =
            [
                DesignComponentOccurrence::try_new(DesignComponentOccurrenceDraft {
                    id: "f3d:Design/BulkStream.dat:design-component-occurrence#800".into(),
                    class_tag: "734".to_owned().try_into().unwrap(),
                    record_index: 800,
                    byte_offset: 0,
                    component_record_index: 305,
                    component_guid: COMPONENT.to_owned().try_into().unwrap(),
                    occurrence_guid: OCCURRENCE.to_owned().try_into().unwrap(),
                    placement: DesignComponentOccurrencePlacement::Explicit {
                        ordinal: std::num::NonZeroU32::MIN,
                        transform: carrier_matrix.try_into().unwrap(),
                    },
                    owner_entity: 0,
                })
                .unwrap(),
            ];
        if current {
            occurrences[0].current_pose = Some(
                serde_json::from_value(serde_json::json!({
                    "carrier_record_index": 1200
                }))
                .unwrap(),
            );
        }
        let construction = read(&bytes, &scope, &occurrences).unwrap();
        assert_eq!(construction.placements[0].placement.value()[0][3], 1.0e308);
        if let crate::records::feature::scope::DesignScopePayloadMut::DerivedInstance(slot) =
            scope.payload_mut()
        {
            *slot = Some(construction);
        }
        let mut components = BTreeMap::new();
        if current {
            use crate::design::decode::components::{ComponentDefinition, StreamComponents};
            components.insert(
                "f3d:Design/BulkStream.dat".to_owned(),
                StreamComponents {
                    roots: BTreeMap::from([
                        (
                            305,
                            ComponentDefinition {
                                root_entity: 1,
                                context_uuid: COMPONENT.to_owned().try_into().unwrap(),
                                name: None,
                            },
                        ),
                        (
                            306,
                            ComponentDefinition {
                                root_entity: 0,
                                context_uuid: "99999999-2222-4333-8444-555555555555"
                                    .to_owned()
                                    .try_into()
                                    .unwrap(),
                                name: None,
                            },
                        ),
                    ]),
                    root_component: Some(306),
                    ..StreamComponents::default()
                },
            );
        }
        let projection = crate::design::components::project_local_components(
            &crate::design::components::LocalComponentSources {
                scopes: std::slice::from_ref(&scope),
                occurrences: &occurrences,
                paths: &[],
                components: &components,
                body_bindings: &[],
                bodies: Some(&BTreeSet::new()),
                hidden_bodies: None,
                root_ordinal_start: 0,
                limits: cadmpeg_core::decode::ResourceLimits::default(),
            },
        )
        .unwrap();
        assert_eq!(projection.occurrences.len(), count);
        assert!(projection
            .losses
            .iter()
            .any(|loss| loss.code == code.kind()));
        let paths = crate::design::components::paths::LocalOccurrencePaths::new(
            &occurrences,
            &[],
            &projection.occurrences,
            &[],
        );
        let mut feature = derived_feature(&scope);
        let losses = crate::design::components::project_derived_instance_features(
            std::slice::from_mut(&mut feature),
            std::slice::from_ref(&scope),
            &paths,
            &projection.occurrences,
        )
        .unwrap();
        if count == 0 {
            assert!(
                matches!(feature.evaluation.definition(), FeatureDefinition::Operation(
                FeatureOperation::InsertComponents { components: InsertedComponents::Native { native } }
            ) if native.as_str() == scope.id)
            );
            assert_eq!(losses.len(), 1);
            assert_eq!(
                losses[0].code,
                crate::loss::F3dLossCode::FeatureDerivedInstanceSourcesUnresolved.kind()
            );
        } else {
            let selected = projection
                .occurrences
                .iter()
                .find(|occurrence| {
                    occurrence.native_ref.as_deref() == Some(occurrences[0].id.as_str())
                })
                .unwrap();
            assert!(
                matches!(feature.evaluation.definition(), FeatureDefinition::Operation(
                FeatureOperation::InsertComponent { occurrence }
            ) if *occurrence == selected.id)
            );
            assert!(losses.is_empty());
        }
    }
}
