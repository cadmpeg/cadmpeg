// SPDX-License-Identifier: Apache-2.0
use cadmpeg_asm::brep::records::BodyNativeKey;
use std::rc::Rc;

use cadmpeg_container::compound::CompoundSnapshot;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::report::loss::LossKind;

use crate::feature::base::{
    current_body, entry, history_candidate, inspect_source_body, reference_counts,
    unique_link_header, wrappers,
};
use crate::feature::history_views::{ResultStates, StateModel};
use crate::feature::tests::{reference, test_feature, test_header};
use crate::feature::{
    PmDcFeatureProperty, PmDcFeaturePropertyKind, PmDcFeaturePropertyPayload, PmDcKernelEntityLink,
    PmDcKernelEntityLinkPayload, KERNEL_ENTITY_LINK_TYPE, SURFACE_BODY_TYPE,
};
use crate::layout::{meta_external_reference as external, meta_external_revision as revision};
use crate::loss::InventorLossCode;
use crate::pmdc::PmDcReference;
use crate::record_identity::{Located, RecordTypeId};
use crate::records::MetaSection;
use crate::rse::RseInventory;
use crate::test_support::base_envelope::{
    decode, derived_at_version, fixtures, state_model, SyntheticRecords, RESULT_KEY, SOURCE_INDEX,
};
use crate::test_support::test_fixtures::{
    acis_sphere_kernel_stream_with_body_key, base_envelope_fixture, BaseEnvelopeRecords,
};

fn wrapper(body: u32) -> PmDcFeatureProperty {
    Located::new(
        PmDcFeaturePropertyPayload {
            save_version_major: 18,
            header: test_header(),
            kind: PmDcFeaturePropertyKind::SurfaceBody {
                body: PmDcReference::from_packed(body),
            },
        },
        RecordTypeId::from_bytes(SURFACE_BODY_TYPE),
        cadmpeg_ir::identity_key!("dc"),
        2,
    )
}

fn link() -> PmDcKernelEntityLink {
    Located::new(
        PmDcKernelEntityLinkPayload {
            save_version_major: 18,
            header_value: 0,
            header_id: 77,
            selection_source: 0,
            state: 0,
            body: reference(1),
            entity_id: 7,
            entity_type: 42,
        },
        RecordTypeId::from_bytes(KERNEL_ENTITY_LINK_TYPE),
        cadmpeg_ir::identity_key!("seg"),
        1,
    )
}

fn body_key() -> BodyNativeKey {
    BodyNativeKey {
        source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
            cadmpeg_asm::asm_format!("inventor"),
        ),
        record_index: 1,
        body: cadmpeg_ir::ids::BodyId::mint("inventor:brep:entity#1").expect("synthetic body"),
        body_ordinal: 0,
        source_brep: Some("Bseg:record:0".into()),
        asm_body_key: Some(7),
    }
}

#[test]
fn current_body_requires_one_carrier_scoped_body_key() {
    let output = wrapper(1);
    let link = link();
    let key = body_key();
    assert!(current_body(&output, &link, "seg", 0, std::slice::from_ref(&key)).is_some());
    assert!(current_body(&output, &link, "seg", 0, &[]).is_none());
    assert!(current_body(&output, &link, "other", 0, std::slice::from_ref(&key)).is_none());
    assert!(current_body(&output, &link, "seg", 0, &[key.clone(), key.clone()]).is_none());
    let mut other = key.clone();
    other.source_brep = Some("Bother:record:0".into());
    assert!(current_body(&output, &link, "seg", 0, &[key, other]).is_some());
    for output in [wrapper(0), wrapper(0x8000_0001)] {
        assert!(current_body(&output, &link, "seg", 0, &[body_key()]).is_none());
    }
}

#[test]
fn current_body_checks_source_state_and_carrier_reference() {
    for field in 0..6 {
        let mut payload = (*link()).clone();
        match field {
            0 => payload.state = 1,
            1 => payload.selection_source = 1,
            2 => payload.body = PmDcReference::from_packed(1),
            3 => payload.body = reference(2),
            4 => payload.entity_id = 8,
            _ => payload.header_value = 1,
        }
        let changed = Located::new(
            payload,
            RecordTypeId::from_bytes(KERNEL_ENTITY_LINK_TYPE),
            cadmpeg_ir::identity_key!("seg"),
            1,
        );
        assert!(current_body(&wrapper(1), &changed, "seg", 0, &[body_key()]).is_none());
    }
}

#[test]
fn result_link_requires_one_external_header_in_its_segment() {
    let link = link();
    assert!(unique_link_header(&link, std::slice::from_ref(&link)));
    let mut duplicate = link.clone();
    duplicate.identity.record_ordinal += 1;
    assert!(!unique_link_header(
        &link,
        &[link.clone(), duplicate.clone()]
    ));
    duplicate.identity.segment_token = cadmpeg_ir::identity_key!("other");
    assert!(unique_link_header(&link, &[link.clone(), duplicate]));
}

#[test]
fn external_entries_require_exact_count_width_and_bounds() {
    let bytes = [0_u8; 16];
    let mut section = MetaSection {
        number: crate::records::MetaSectionNumber::Ten,
        discriminator: 2,
        payload: View::over_retained(&bytes),
    };
    assert!(entry(&section, 1, 8).is_some());
    assert!(entry(&section, 2, 8).is_none());
    section.discriminator = 1;
    assert!(entry(&section, 0, 8).is_none());
    section.discriminator = u32::MAX;
    assert!(entry(&section, 0, 8).is_none());
}

#[test]
fn external_reference_counts_reject_dangling_revision_indices() {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let mut revision_bytes = [0_u8; revision::LEN * 2];
    for (index, count) in [2_u16, 1].into_iter().enumerate() {
        let offset = index * revision::LEN + revision::REFERENCE_COUNT;
        revision_bytes[offset..offset + 2].copy_from_slice(&count.to_le_bytes());
    }
    let revisions = MetaSection {
        number: crate::records::MetaSectionNumber::Eight,
        discriminator: 2,
        payload: View::over_retained(&revision_bytes),
    };
    let mut reference_bytes = [0_u8; external::LEN * 3];
    reference_bytes[external::LEN..external::LEN + 4].copy_from_slice(&1_u32.to_le_bytes());
    let references = MetaSection {
        number: crate::records::MetaSectionNumber::Ten,
        discriminator: 3,
        payload: View::over_retained(&reference_bytes),
    };
    assert_eq!(
        reference_counts(&ctx, &revisions, &references).expect("reference counts"),
        Some(vec![2, 1])
    );
    let mut inconsistent_bytes = revision_bytes;
    inconsistent_bytes[revision::LEN + revision::REFERENCE_COUNT..][..2]
        .copy_from_slice(&2_u16.to_le_bytes());
    let inconsistent_revisions = MetaSection {
        payload: View::over_retained(&inconsistent_bytes),
        ..revisions
    };
    assert!(reference_counts(&ctx, &inconsistent_revisions, &references)
        .expect("inconsistent reference count")
        .is_none());
    let mut invalid_bytes = reference_bytes;
    invalid_bytes[external::LEN..external::LEN + 4].copy_from_slice(&2_u32.to_le_bytes());
    let references = MetaSection {
        payload: View::over_retained(&invalid_bytes),
        ..references
    };
    assert!(reference_counts(&ctx, &revisions, &references)
        .expect("invalid references")
        .is_none());
}

#[test]
fn base_slots_require_two_distinct_surface_wrappers_and_null_tail() {
    let source = test_feature(1, 3, &[(0, 2), (1, 3)]);
    let first = wrapper(0);
    let mut second = wrapper(1);
    second.identity.record_ordinal = 3;
    assert!(wrappers(&source, crate::feature::BASE_CLASS_ID, |ordinal| [
        &first, &second
    ]
    .into_iter()
    .find(|property| property.identity.record_ordinal == ordinal))
    .is_some());
    let repeated = test_feature(1, 3, &[(0, 2), (1, 2)]);
    assert!(wrappers(&repeated, crate::feature::BASE_CLASS_ID, |_| Some(&first)).is_none());
    for invalid in [
        test_feature(1, 2, &[(0, 2), (1, 3)]),
        test_feature(1, 3, &[(0, 2), (1, 3), (2, 4)]),
    ] {
        assert!(wrappers(&invalid, crate::feature::BASE_CLASS_ID, |_| Some(&first)).is_none());
    }
}

#[test]
fn a_bounded_base_source_inspection_reports_the_loss() {
    use cadmpeg_asm::kernel_header::RefWidth;

    use crate::kernel::parse_carrier;
    use crate::loss::InventorLossCode;
    use crate::test_support::history_carrier::{carrier_bytes, framed_carrier, CURRENT_BODY_KEY};

    let bytes = framed_carrier(&carrier_bytes(RefWidth::Eight, false));
    let entity = u32::try_from(CURRENT_BODY_KEY).expect("fixture body key");
    let arena = DecodeArena::new();
    let (ordinary, view) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("input");
    let carrier =
        parse_carrier(view, &cadmpeg_ir::identity_key!("seg"), 0, 0, 18).expect("carrier");
    let mut losses = Vec::new();
    assert!(inspect_source_body(&ordinary, &carrier, entity, &mut losses).expect("source check"));
    assert!(losses.is_empty());

    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (bounded, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("input");
    assert!(!inspect_source_body(&bounded, &carrier, entity, &mut losses).expect("source check"));
    let [loss] = losses.as_slice() else {
        panic!("one bounded-source loss");
    };
    assert_eq!(loss.code, InventorLossCode::FeatureBaseSourceBounded.kind());
    assert!(loss.message.contains("Entities"), "{}", loss.message);
    assert!(bounded.resource_refusal().is_none());
}

#[test]
fn history_candidate_requires_the_carrier_link_and_no_final_body_with_the_key() {
    let output = wrapper(1);
    let link = link();
    let key = body_key();
    assert!(history_candidate(&output, &link, "seg", 0, &[]));
    // A final body that carries the key is not a candidate.
    assert!(!history_candidate(
        &output,
        &link,
        "seg",
        0,
        std::slice::from_ref(&key)
    ));
    // Duplicate final keys are still final bodies.
    assert!(!history_candidate(
        &output,
        &link,
        "seg",
        0,
        &[key.clone(), key.clone()]
    ));
    // A body of another carrier or another key does not hold the key.
    let mut other = key.clone();
    other.source_brep = Some("Bother:record:0".into());
    assert!(history_candidate(&output, &link, "seg", 0, &[other]));
    let mut other = key;
    other.asm_body_key = Some(8);
    assert!(history_candidate(&output, &link, "seg", 0, &[other]));
}

#[test]
fn history_candidate_refuses_a_link_that_does_not_select_the_carrier() {
    let output = wrapper(1);
    let link = link();
    assert!(!history_candidate(&output, &link, "other", 0, &[]));
    assert!(!history_candidate(&output, &link, "seg", 1, &[]));
    for output in [wrapper(0), wrapper(0x8000_0001)] {
        assert!(!history_candidate(&output, &link, "seg", 0, &[]));
    }
    let mut payload = (*link).clone();
    payload.state = 1;
    let changed = Located::new(
        payload,
        RecordTypeId::from_bytes(KERNEL_ENTITY_LINK_TYPE),
        cadmpeg_ir::identity_key!("seg"),
        1,
    );
    assert!(!history_candidate(&output, &changed, "seg", 0, &[]));
}

/// States of a fixed answer.
struct Fixed {
    introduces: bool,
    outcome: fn(&CadIr) -> StateModel,
    model: CadIr,
}

impl ResultStates for Fixed {
    fn after(
        &self,
        _: &DecodeContext<'_>,
        source_index: u32,
    ) -> Result<Rc<StateModel>, CodecError> {
        assert_eq!(source_index, SOURCE_INDEX);
        Ok(Rc::new((self.outcome)(&self.model)))
    }

    fn introduces(&self, source_index: u32, key: i64) -> bool {
        self.introduces && source_index == SOURCE_INDEX && key == RESULT_KEY
    }
}

fn holding(model: &CadIr) -> StateModel {
    StateModel::Model(Box::new(model.clone()))
}

/// Projects the two Base features of the fixture with a final model that
/// holds no body key, and returns the transferred count and the losses.
fn project_without_final_body(states: &Fixed) -> (usize, Vec<LossKind>) {
    project_records_without_final_body(fixtures(), states)
}

/// As `project_without_final_body`, for the given synthetic records.
fn project_records_without_final_body(
    (design_recs, base_recs): (SyntheticRecords, BaseEnvelopeRecords),
    states: &Fixed,
) -> (usize, Vec<LossKind>) {
    let bytes = base_envelope_fixture(
        &acis_sphere_kernel_stream_with_body_key(21_800, 7),
        &design_recs,
        &base_recs,
    );
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("context");
    let snapshot = CompoundSnapshot::new(&ctx, root).expect("compound");
    let document = RseInventory::build(&ctx, &snapshot).expect("RSe inventory");
    let features = crate::feature::inventory(&ctx, &document).expect("feature inventory");
    let design = crate::design::inventory(&ctx, &document).expect("design inventory");
    let sketches = crate::sketch::inventory(&ctx, &document).expect("sketch inventory");
    let mut ir = decode(&design_recs, &base_recs);
    ir.model.feature_result_topologies.clear();
    for feature in &mut ir.model.features {
        feature
            .evaluation
            .set_definition(FeatureDefinition::Operation(FeatureOperation::Native {
                kind: cadmpeg_ir::features::NativeFeatureKind::Other("Base".into()),
                parameters: std::collections::BTreeMap::new(),
            }));
    }
    let keys: Vec<BodyNativeKey> = Vec::new();
    let projection = crate::feature::base::project(
        &ctx,
        &crate::feature::base::Inputs {
            document: &document,
            inventory: &features,
            design: &design,
            transforms: &sketches.transforms,
            keys: &keys,
            states: Some(states),
        },
        &mut ir,
    )
    .expect("Base projection");
    let codes = projection
        .losses
        .iter()
        .map(|note| note.code.clone())
        .collect();
    if projection.transferred == 2 {
        assert_eq!(ir.model.feature_result_topologies.len(), 2);
        for result in &ir.model.feature_result_topologies {
            assert_eq!(result.bodies().len(), 1);
            assert!(result.bodies()[0].as_str().ends_with("-3"));
        }
        for feature in &ir.model.features {
            assert!(matches!(
                feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::StoredGeometry {})
            ));
            assert!(feature.source_properties.contains_key("result_body_link"));
        }
    } else {
        assert!(ir.model.feature_result_topologies.is_empty());
    }
    (projection.transferred, codes)
}

#[test]
fn a_result_body_that_its_state_introduces_types_the_base() {
    let states = Fixed {
        introduces: true,
        outcome: holding,
        model: state_model(),
    };
    assert_eq!(project_without_final_body(&states), (2, Vec::new()));
}

#[test]
fn a_result_key_that_no_state_introduces_stays_native_without_a_loss() {
    let states = Fixed {
        introduces: false,
        outcome: holding,
        model: state_model(),
    };
    assert_eq!(project_without_final_body(&states), (0, Vec::new()));
}

#[test]
fn a_state_without_a_body_of_the_result_key_stays_native_with_its_loss() {
    let mut model = state_model();
    model
        .native
        .namespace_mut(cadmpeg_ir::codec_id!("inventor"))
        .arenas_mut()
        .remove("body_native_keys");
    let states = Fixed {
        introduces: true,
        outcome: holding,
        model,
    };
    let (transferred, codes) = project_without_final_body(&states);
    assert_eq!(transferred, 0);
    assert_eq!(codes, vec![state_loss(); 2]);
}

#[test]
fn a_state_that_does_not_decode_or_passed_its_cap_stays_native_with_its_loss() {
    for outcome in [
        |_: &CadIr| StateModel::Failed,
        |_: &CadIr| StateModel::Bounded,
    ] {
        let states = Fixed {
            introduces: true,
            outcome,
            model: CadIr::empty(),
        };
        let (transferred, codes) = project_without_final_body(&states);
        assert_eq!(transferred, 0);
        assert_eq!(codes, vec![state_loss(); 2]);
    }
    let states = Fixed {
        introduces: true,
        outcome: |_| StateModel::Unavailable,
        model: CadIr::empty(),
    };
    let (transferred, codes) = project_without_final_body(&states);
    assert_eq!(transferred, 0);
    assert_eq!(codes, vec![state_loss(); 2]);
}

fn state_loss() -> LossKind {
    InventorLossCode::FeatureBaseResultStateNative
        .note("state")
        .code
}

#[test]
fn a_derived_result_whose_state_does_not_decode_or_passed_its_cap_stays_native_with_its_loss() {
    let loss = InventorLossCode::FeatureDerivedResultNative
        .note("state")
        .code;
    for outcome in [
        |_: &CadIr| StateModel::Failed,
        |_: &CadIr| StateModel::Bounded,
        |_: &CadIr| StateModel::Unavailable,
    ] {
        let states = Fixed {
            introduces: true,
            outcome,
            model: CadIr::empty(),
        };
        let (transferred, codes) =
            project_records_without_final_body(derived_at_version(false, 19), &states);
        assert_eq!(transferred, 0);
        assert_eq!(codes, vec![loss.clone()]);
    }
}
