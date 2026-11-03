// SPDX-License-Identifier: Apache-2.0
use cadmpeg_asm::brep::records::BodyNativeKey;
use cadmpeg_core::decode::{DecodeContext, View};

use crate::feature::base::{current_body, entry, reference_counts, unique_link_header, wrappers};
use crate::feature::tests::{reference, test_feature, test_header};
use crate::feature::{
    PmDcFeatureProperty, PmDcFeaturePropertyKind, PmDcFeaturePropertyPayload, PmDcKernelEntityLink,
    PmDcKernelEntityLinkPayload, KERNEL_ENTITY_LINK_TYPE, SURFACE_BODY_TYPE,
};
use crate::layout::{meta_external_reference as external, meta_external_revision as revision};
use crate::pmdc::PmDcReference;
use crate::record_identity::{Located, RecordTypeId};
use crate::records::MetaSection;

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
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};

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
