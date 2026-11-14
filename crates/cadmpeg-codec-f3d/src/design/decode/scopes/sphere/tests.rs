use super::decode;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::parameters::{DesignParameterOwner, DesignParameterOwnerWire};

fn fixture(
    operation_at: usize,
    reference_at: usize,
    typed: bool,
) -> (Vec<u8>, DesignParameterScope, DesignParameterOwner) {
    let mut bytes = vec![0; 400];
    bytes[operation_at..operation_at + 4].copy_from_slice(&2u32.to_le_bytes());
    bytes[operation_at + 4] = 1;
    for (ordinal, (target, guid)) in [
        (40u32, "7B6C3C2D-4096-4E6D-B225-699E8C2D9355"),
        (50, "D91D429C-1BB5-4640-B49F-69AF14F463E9"),
        (60, "B556C40D-B680-4BD8-9623-028CD43307EC"),
    ]
    .into_iter()
    .enumerate()
    {
        let at = reference_at + ordinal * if typed { 51 } else { 11 };
        bytes[at] = 1;
        bytes[at + 1..at + 5].copy_from_slice(&target.to_le_bytes());
        if typed {
            bytes[at + 9..at + 13].copy_from_slice(&36u32.to_le_bytes());
            bytes[at + 13..at + 49].copy_from_slice(guid.as_bytes());
        }
    }
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#7",
        DesignFeatureKind::SpherePrimitive,
        7,
    );
    scope
        .try_edit(|draft| {
            draft.frame_length = 400;
            draft.paired_byte_offset = 400;
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![30, 40, 50, 60]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let owner = DesignParameterOwner::try_from(DesignParameterOwnerWire {
        id: "f3d:Design/BulkStream.dat:owner#400".into(),
        byte_offset: 400,
        frame_length: 104,
        class_tag: "123".to_owned().try_into().unwrap(),
        record_index: 50,
        scope_record_index: 7,
        local_ordinal: 0,
        evaluated_value: 1.7,
        evaluated_value_offset: 440,
        parameter_record_index: 51,
        owned_ordinal: 1,
        variant: Some(0),
        companion_record_index: 52,
    })
    .unwrap();
    (bytes, scope, owner)
}

#[test]
fn reference_placed_sphere_prefixes_bind_evaluated_diameters() {
    for (operation, references, typed) in [(25, 30, false), (21, 26, false), (20, 25, true)] {
        let (bytes, scope, owner) = fixture(operation, references, typed);
        let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
        let sphere = decode(&bytes, &records, &scope, &[owner], &[]).unwrap();
        assert_eq!(sphere.diameter.get(), 1.7);
        assert_eq!(sphere.diameter_record_index, 50);
        assert_eq!(sphere.placement_record_index, Some(40));
        assert_eq!(
            sphere.operation,
            crate::records::feature::extrude::DesignExtrudeOperation::Cut
        );
        assert_eq!(
            sphere.operation_offset,
            cadmpeg_core::decode::u64_from_index(operation)
        );
        assert!(!sphere.diameter_owner_assumed);
        assert!(sphere.diameter_source_assumed);
    }
}

#[test]
fn sphere_prefixes_refuse_truncation_corruption_and_duplicate_owners() {
    for (operation, references, typed) in [(25, 30, false), (21, 26, false), (20, 25, true)] {
        let (bytes, scope, owner) = fixture(operation, references, typed);
        let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
        for end in [0, references, references + 10, 399] {
            assert!(decode(
                &bytes[..end],
                &records,
                &scope,
                std::slice::from_ref(&owner),
                &[]
            )
            .is_none());
        }
        for at in [operation, operation + 4, references, references + 1] {
            let mut corrupt = bytes.clone();
            corrupt[at] = 9;
            assert!(decode(
                &corrupt,
                &records,
                &scope,
                std::slice::from_ref(&owner),
                &[]
            )
            .is_none());
        }
        assert!(decode(&bytes, &records, &scope, &[owner.clone(), owner], &[]).is_none());
    }
}

#[test]
fn sphere_recovers_scalar_when_owner_record_is_unretained() {
    let (mut bytes, scope, _) = fixture(25, 30, false);
    let mut scalar = vec![0; 104];
    scalar[..4].copy_from_slice(&3u32.to_le_bytes());
    scalar[4..7].copy_from_slice(b"123");
    scalar[7..11].copy_from_slice(&50u32.to_le_bytes());
    scalar[40..48].copy_from_slice(&1.7f64.to_le_bytes());
    bytes.extend_from_slice(&scalar);
    crate::test_support::indexed_header(&mut bytes, *b"124", 50);
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let sphere = decode(&bytes, &records, &scope, &[], &[]).unwrap();
    assert!(sphere.diameter_owner_assumed && sphere.diameter_source_assumed);
    assert_eq!(sphere.diameter.get(), 1.7);
    for value in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        bytes[440..448].copy_from_slice(&value.to_le_bytes());
        assert!(decode(&bytes, &records, &scope, &[], &[]).is_none());
    }
}
