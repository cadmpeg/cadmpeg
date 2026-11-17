use super::*;

#[test]
fn form_tracking_zero_retains_tracking_count_with_unknown_cage_membership() {
    let bytes = form_selection_group_frame(226, 220, 0);
    let meta = form_meta(&[("E079B079-016A-43C2-ACC7-9627B32A8960", 0, &[226])]);
    let construction = body_construction(&bytes, &meta, &form_scope(220, vec![224, 226]), &[]);
    assert!(!construction.membership_is_complete());
    assert_eq!(construction.inputs().lists[0].declared_count, 0);
    assert_eq!(
        construction.inputs().lists[0].role,
        FormListRole::TrackingSelection
    );
    assert!(matches!(
        construction.membership(),
        FormMembership::Partial {
            cardinality: None,
            ..
        }
    ));
}

fn routed_tracking_construction(
    bytes: &[u8],
    references: Vec<u32>,
    groups: &[u32],
    version: u32,
) -> FormConstruction {
    use std::io::{Cursor, Write};
    let scope = form_scope(220, references);
    let records = crate::design::test_support::indexed_record_offsets_for_test(bytes);
    let entities = groups
        .iter()
        .map(|&group| u64::from(group))
        .collect::<Vec<_>>();
    let offsets = groups
        .iter()
        .map(|&group| {
            (
                u64::from(group),
                u64::try_from(records.offsets(group)[0]).unwrap(),
            )
        })
        .collect::<Vec<_>>();
    let meta = crate::test_support::streams_test::design_metastream_with_records(
        &[(
            "E079B079-016A-43C2-ACC7-9627B32A8960",
            "60F291CB-499B-4AB1-B0E5-340368671724",
            version,
            "Fusion",
            &entities,
        )],
        &offsets,
    );
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    crate::test_support::manifest_test::write_synthetic_manifests(&mut archive, stored);
    for (entry, contents) in [
        ("FusionAssetName[Active]/Design1/BulkStream.dat", bytes),
        (
            "FusionAssetName[Active]/Design1/MetaStream.dat",
            meta.as_slice(),
        ),
    ] {
        archive.start_file(entry, stored).unwrap();
        archive.write_all(contents).unwrap();
    }
    let archive = archive.finish().unwrap().into_inner();
    let mut result = None;
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        crate::design::test_support::with_test_decode_context(|ctx| {
            let mut facts = super::super::FormFacts {
                inputs: super::super::form_inputs(&scope)?,
                observations: Vec::new(),
            };
            let membership = super::super::collect_membership(
                ctx,
                scan,
                super::super::FormSource {
                    bytes,
                    records: &records,
                    scope: &scope,
                    cages: &[],
                    stream: "FusionAssetName[Active]/Design1/BulkStream.dat",
                },
                std::slice::from_ref(&scope),
                &mut facts,
                &mut super::super::ReferenceGroups::default(),
                &mut Vec::new(),
            )?;
            result = Some(
                FormConstruction::try_new(facts.inputs, facts.observations, membership).unwrap(),
            );
            Ok::<(), cadmpeg_core::CodecError>(())
        })
    })
    .unwrap();
    result.unwrap()
}

#[test]
fn form_tracking_counts_stay_independent_of_complete_owned_cage_lists() {
    for (tail, count, owned_list, version) in [
        (52, 0u32, false, 0),
        (53, 0, false, 0),
        (53, 2, true, 0),
        (52, 0, false, 1),
    ] {
        let mut bytes = Vec::new();
        if owned_list {
            let mut owned = indexed_frame(b"402", 225, 88);
            owned[21] = 1;
            owned[22..30].copy_from_slice(&220u64.to_le_bytes());
            bytes.extend_from_slice(&owned);
            bytes.extend_from_slice(&indexed_frame(b"264", 225, 15));
        }
        let mut group = indexed_frame(
            b"429",
            226,
            36 + usize::try_from(count).unwrap() * 11 + tail,
        );
        group[21] = 1;
        group[22..30].copy_from_slice(&220u64.to_le_bytes());
        group[32..36].copy_from_slice(&count.to_le_bytes());
        for ordinal in 0..usize::try_from(count).unwrap() {
            let at = 36 + ordinal * 11;
            group[at] = 1;
            group[at + 1..at + 9]
                .copy_from_slice(&(228u64 + u64::try_from(ordinal).unwrap()).to_le_bytes());
        }
        bytes.extend_from_slice(&group);
        bytes.extend_from_slice(&indexed_frame(b"264", 226, 15));
        let references = if owned_list {
            vec![225, 226]
        } else {
            vec![226]
        };
        let construction = routed_tracking_construction(&bytes, references, &[226], version);
        assert_eq!(construction.membership_is_complete(), owned_list);
        let tracking = construction
            .inputs()
            .lists
            .iter()
            .find(|list| list.role == FormListRole::TrackingSelection);
        if version == 0 {
            let tracking = tracking.unwrap();
            assert_eq!(tracking.declared_count, count);
            assert_eq!(tracking.members.len(), usize::try_from(count).unwrap());
        } else {
            assert!(tracking.is_none());
            assert_eq!(construction.inputs().references.len(), 1);
        }
        if owned_list {
            assert_eq!(construction.committed_cages().count(), 0);
            assert!(construction
                .inputs()
                .lists
                .iter()
                .any(|list| list.role == FormListRole::CageObjects && list.declared_count == 0));
        } else {
            assert!(matches!(
                construction.membership(),
                FormMembership::Partial {
                    cardinality: None,
                    ..
                }
            ));
        }
    }
}

#[test]
fn form_tracking_lists_follow_direct_reference_order_and_normalize_repeated_inputs() {
    let mut bytes = Vec::new();
    for group in [226, 227] {
        bytes.extend_from_slice(&form_selection_group_frame(group, 220, 0));
    }
    let first = routed_tracking_construction(&bytes, vec![227, 226, 227], &[226, 227], 0);
    let second = routed_tracking_construction(&bytes, vec![227, 226, 227], &[226, 227], 0);
    assert_eq!(first, second);
    assert_eq!(first.inputs().references.len(), 3);
    assert_eq!(first.inputs().lists.len(), 2);
    assert!(first.inputs().lists[0]
        .source
        .references
        .last()
        .unwrap()
        .as_str()
        .ends_with("#227"));
    assert!(first.inputs().lists[1]
        .source
        .references
        .last()
        .unwrap()
        .as_str()
        .ends_with("#226"));
}

#[test]
fn form_tracking_fixed_slots_retain_members_after_malformed_and_null_slots() {
    let mut bytes = indexed_frame(b"429", 226, 133);
    bytes[21] = 1;
    bytes[22..30].copy_from_slice(&220u64.to_le_bytes());
    bytes[32..36].copy_from_slice(&4u32.to_le_bytes());
    bytes[36] = 1;
    bytes[37..45].copy_from_slice(&4_294_967_297u64.to_le_bytes());
    bytes[47] = 2;
    bytes[69] = 1;
    bytes[70..78].copy_from_slice(&230u64.to_le_bytes());
    bytes.extend_from_slice(&indexed_frame(b"264", 226, 30));
    let meta = form_meta(&[("E079B079-016A-43C2-ACC7-9627B32A8960", 0, &[226])]);
    let construction = body_construction(&bytes, &meta, &form_scope(220, vec![226]), &[]);
    let list = construction
        .inputs()
        .lists
        .iter()
        .find(|list| list.role == FormListRole::TrackingSelectionFixedAssumed)
        .unwrap();
    assert_eq!(list.declared_count, 4);
    assert_eq!(
        list.members
            .iter()
            .map(|member| member.ordinal)
            .collect::<Vec<_>>(),
        [0, 3]
    );
    assert!(list.members[0].reference.as_str().ends_with("#4294967297"));
    assert!(list.members[1].reference.as_str().ends_with("#230"));
    assert!(matches!(
        construction.membership(),
        FormMembership::Partial {
            cardinality: None,
            ..
        }
    ));
}

#[test]
fn form_tracking_variable_references_retain_inline_typed_members() {
    for (guid, matches) in [
        ("C46D3EEB-40E1-43FA-83DC-9CB204335417", true),
        ("00000000-0000-4000-8000-000000000001", false),
    ] {
        let mut bytes = indexed_frame(b"429", 226, 36);
        bytes[21] = 1;
        bytes[22..30].copy_from_slice(&220u64.to_le_bytes());
        bytes[32..36].copy_from_slice(&2u32.to_le_bytes());
        for (target, inline) in [(228u64, true), (230, false)] {
            bytes.push(1);
            bytes.extend_from_slice(&target.to_le_bytes());
            if inline {
                bytes.extend_from_slice(&36u32.to_le_bytes());
                bytes.extend_from_slice(guid.as_bytes());
            }
            bytes.extend_from_slice(&[0, 0]);
        }
        bytes.extend_from_slice(&[0; 52]);
        bytes.extend_from_slice(&indexed_frame(b"264", 226, 30));
        let meta = form_meta(&[
            ("E079B079-016A-43C2-ACC7-9627B32A8960", 0, &[226]),
            ("C46D3EEB-40E1-43FA-83DC-9CB204335417", 1, &[228]),
        ]);
        let construction = body_construction(&bytes, &meta, &form_scope(220, vec![226]), &[]);
        let list = &construction.inputs().lists[0];
        assert_eq!(list.declared_count, 2);
        assert_eq!(list.members.len(), if matches { 2 } else { 1 });
        if matches {
            assert!(list.members[0].reference.as_str().ends_with("#228"));
        }
        assert!(list
            .members
            .last()
            .unwrap()
            .reference
            .as_str()
            .ends_with("#230"));
        assert_eq!(
            construction
                .inputs()
                .reference_evidence
                .iter()
                .find(|reference| reference.role == FormReferenceRole::TrackingMember)
                .unwrap()
                .inline_type_guid
                .as_deref(),
            Some(guid)
        );
    }
}

#[test]
fn form_tracking_foreign_member_and_conflicting_owner_keep_their_typed_evidence() {
    let mut bytes = indexed_frame(b"429", 226, 21);
    bytes.push(1);
    bytes.extend_from_slice(&220u64.to_le_bytes());
    bytes.extend_from_slice(&36u32.to_le_bytes());
    bytes.extend_from_slice(b"00000000-0000-4000-8000-000000000001");
    bytes.extend_from_slice(&[0, 0]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&228u64.to_le_bytes());
    bytes.extend_from_slice(&[0, 1]);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    push_marked(&mut bytes, 230);
    bytes.extend_from_slice(&[0; 52]);
    bytes.extend_from_slice(&indexed_frame(b"264", 226, 30));
    let meta = form_meta(&[
        ("E079B079-016A-43C2-ACC7-9627B32A8960", 0, &[226]),
        ("11111111-1111-4111-8111-111111111111", 0, &[220]),
    ]);
    let result = body_construction(&bytes, &meta, &form_scope(220, vec![226]), &[]);
    let list = &result.inputs().lists[0];
    assert_eq!(list.declared_count, 2);
    assert_eq!(list.members.len(), 1);
    assert_eq!(list.members[0].ordinal, 1);
    assert!(list.members[0].reference.as_str().ends_with("#230"));
    assert!(result
        .inputs()
        .reference_evidence
        .iter()
        .any(
            |reference| reference.role == FormReferenceRole::TrackingOwner
                && reference.inline_type_guid.as_deref()
                    == Some("00000000-0000-4000-8000-000000000001")
        ));
    assert!(result
        .inputs()
        .reference_evidence
        .iter()
        .any(
            |reference| reference.role == FormReferenceRole::TrackingMember
                && reference.ordinal == 0
                && matches!(
                    &reference.target,
                    FormNativeReferenceTarget::Segment {
                        target: 228,
                        segment: 7,
                        ..
                    }
                )
        ));
}

#[test]
fn form_tracking_short_owner_retains_its_source_and_target() {
    let bytes = form_selection_group_frame(226, 220, 0);
    let result = routed_tracking_construction(&bytes, vec![226], &[226], 0);
    assert!(result.inputs().reference_evidence.iter().any(|reference| {
        reference.role == FormReferenceRole::TrackingOwner
            && reference.source.as_str().ends_with("#226")
            && matches!(&reference.target, FormNativeReferenceTarget::Local { target } if target.as_str().ends_with("#220"))
    }));
}
