// SPDX-License-Identifier: Apache-2.0
//! Revolution profile selection and consumer admission.

#[test]
fn circles_without_revolutions_build_no_profile_index() {
    let bytes = crate::test_support::test_b2::b2_circle_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    assert!(crate::test_support::with_materialized_limit(0, |ctx| {
        super::super::b2_resolved_revolutions_from_records(ctx, &bytes, &records)
    })
    .expect("no profile consumer")
    .is_empty());
}

fn revolution_profiles(unrelated: u16, requests: usize, fallback: bool) -> Vec<u8> {
    let stream = crate::test_support::test_b2::b2_resolved_revolution_stream();
    let mut bytes = stream[..57].to_vec();
    for index in 0..unrelated {
        let mut circle = crate::test_support::test_b2::b2_circle_stream();
        circle[6..8].copy_from_slice(&(0x2000 + index).to_le_bytes());
        bytes.extend(circle);
    }
    let mut revolution = stream[57..].to_vec();
    if fallback {
        revolution[6..8].copy_from_slice(&0x7777u16.to_le_bytes());
    }
    for _ in 0..requests {
        bytes.extend_from_slice(&revolution);
    }
    bytes
}

#[test]
fn exact_revolution_identities_build_no_interval_index() {
    use cadmpeg_core::decode::{refusal_probe::RefusalProbe, ResourceDimension};
    let bytes = revolution_profiles(128, 512, false);
    let records = crate::wire::records::consolidated_records(&bytes);
    let _probe = RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "catia_b2_revolution_interval_profiles",
        None,
    );
    let resolved = crate::test_support::with_work_limit(u64::MAX, |ctx| {
        super::super::b2_resolved_revolutions_from_records(ctx, &bytes, &records)
    })
    .expect("identity-only bindings");
    assert_eq!(resolved.len(), 512);
    for (index, value) in resolved.iter().enumerate() {
        assert_eq!(value.revolution_index, index);
        assert_eq!(value.profile.record_id, 0x1234);
        assert_eq!(value.profile.range.endpoints(), [-4.0, 9.0]);
    }
}

#[test]
fn singleton_revolution_miss_uses_direct_interval_uniqueness() {
    use cadmpeg_core::decode::{refusal_probe::RefusalProbe, ResourceDimension};
    let bytes = revolution_profiles(128, 1, true);
    let records = crate::wire::records::consolidated_records(&bytes);
    for operation in [
        "catia_b2_revolution_identity_profiles",
        "catia_b2_revolution_interval_profiles",
    ] {
        let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
        let resolved = crate::test_support::with_work_limit(u64::MAX, |ctx| {
            super::super::b2_resolved_revolutions_from_records(ctx, &bytes, &records)
        })
        .expect("one lookup needs no index");
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].profile.record_id, 0x1234);
    }
    let refused =
        crate::test_support::with_work_refusal("catia_b2_revolution_interval_scan", |ctx| {
            super::super::b2_resolved_revolutions_from_records(ctx, &bytes, &records)
        });
    assert!(
        matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_b2_revolution_interval_scan")
    );
}

#[test]
fn repeated_revolution_misses_share_a_unique_interval_index() {
    let bytes = revolution_profiles(128, 512, true);
    let records = crate::wire::records::consolidated_records(&bytes);
    let resolved = crate::test_support::with_service_context(|ctx| {
        super::super::b2_resolved_revolutions_from_records(ctx, &bytes, &records)
    })
    .expect("shared lookup");
    assert_eq!(resolved.len(), 512);
    for (index, value) in resolved.iter().enumerate() {
        assert_eq!(value.revolution_index, index);
        assert_eq!(value.profile.record_id, 0x1234);
    }
    for operation in [
        "catia_b2_revolution_identity_profiles",
        "catia_b2_revolution_identity_lookup",
        "catia_b2_revolution_interval_profiles",
        "catia_b2_revolution_interval_lookup",
    ] {
        let refused = crate::test_support::with_work_refusal(operation, |ctx| {
            super::super::b2_resolved_revolutions_from_records(ctx, &bytes, &records)
        });
        assert!(
            matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == operation)
        );
    }
    // Three identical interval keys must keep a tombstone after repetition.
    let mut ambiguous = bytes;
    for identity in [0x8000u16, 0x8001] {
        let mut circle = ambiguous[..57].to_vec();
        circle[6..8].copy_from_slice(&identity.to_le_bytes());
        ambiguous.extend(circle);
    }
    let records = crate::wire::records::consolidated_records(&ambiguous);
    assert!(crate::test_support::with_service_context(|ctx| {
        super::super::b2_resolved_revolutions_from_records(ctx, &ambiguous, &records)
    })
    .expect("duplicate intervals stay ambiguous")
    .is_empty());
}

#[test]
fn indexed_revolution_identity_ambiguity_does_not_fall_back() {
    let mut bytes = revolution_profiles(128, 512, false);
    for range in [[20.0f64, 30.0], [40.0, 50.0]] {
        let mut circle = bytes[..57].to_vec();
        circle[32..40].copy_from_slice(&range[0].to_le_bytes());
        circle[40..48].copy_from_slice(&range[1].to_le_bytes());
        bytes.extend(circle);
    }
    let records = crate::wire::records::consolidated_records(&bytes);
    // The original interval [-4, 9] remains unique, but the identity occurs
    // three times. An identity tombstone cannot become an interval miss.
    assert!(crate::test_support::with_service_context(|ctx| {
        super::super::b2_resolved_revolutions_from_records(ctx, &bytes, &records)
    })
    .expect("identity ambiguity stays unresolved")
    .is_empty());
}
