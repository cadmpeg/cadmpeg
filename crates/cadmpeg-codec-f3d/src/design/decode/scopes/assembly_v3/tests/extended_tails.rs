use super::*;

fn decode(
    bytes: &[u8],
    scope: &DesignParameterScope,
    owners: &[DesignParameterOwner],
    types: &StreamTypes<'_>,
) -> Option<DesignAssemblyAlignment> {
    crate::test_support::with_decode_context(|ctx| {
        alignment(
            ctx,
            bytes,
            &IndexedRecordOffsets::build(bytes),
            scope,
            owners,
            types,
        )
    })
    .unwrap()
}

fn extended_fixture(
    typed: bool,
    expanded: bool,
    identity: bool,
    first_owner: u32,
    additional: u32,
) -> (
    Vec<u8>,
    DesignParameterScope,
    Vec<DesignParameterOwner>,
    StreamTypes<'static>,
) {
    let (mut bytes, mut scope, mut owners, mut types) =
        fixture(typed, expanded, identity, first_owner);
    let grammar = parse(&bytes, &scope, &types).unwrap();
    let width = if typed { 51 } else { 11 };
    if additional != 0 {
        let count_at = usize::try_from(grammar.additional_owner_count.offset).unwrap();
        bytes[count_at..count_at + 4].copy_from_slice(&additional.to_le_bytes());
        let mut run = Vec::new();
        for index in [112, 115]
            .into_iter()
            .take(usize::try_from(additional).unwrap())
        {
            reference(&mut run, index, typed);
        }
        let shift = u64_from_index(run.len());
        bytes.splice(
            grammar.additional_owner_start..grammar.additional_owner_start,
            run,
        );
        let refs = scope
            .reference_members()
            .values()
            .zip(scope.reference_members().offsets())
            .map(|(value, offset)| Located {
                value: *value,
                offset: offset + shift,
            })
            .collect();
        scope
            .try_edit(|draft| {
                draft.frame_length += shift;
                draft.paired_byte_offset += shift;
                draft.reference_count_offset += shift;
                draft.kind_offset += shift;
                draft.feature_ordinal_offset += shift;
                draft.reference_members = ReferenceRun::located(refs);
            })
            .unwrap();
        owners.truncate(usize::try_from(4 + additional).unwrap());
    }
    let at = usize::try_from(scope.feature_ordinal_offset()).unwrap() + 19 + width;
    if !typed {
        bytes.remove(at);
    }
    let mut extra = Vec::new();
    reference(&mut extra, 63, typed);
    if typed {
        extra[13..49].copy_from_slice(ADDITIONAL_TAIL_TYPE_GUID.as_bytes());
    }
    let shift = u64_from_index(extra.len() - usize::from(!typed));
    bytes.splice(at..at, extra);
    scope
        .try_edit(|draft| {
            draft.frame_length += shift;
            draft.paired_byte_offset += shift;
        })
        .unwrap();
    types.insert(63, (ADDITIONAL_TAIL_TYPE_GUID, 1));
    (bytes, scope, owners, types)
}

#[test]
fn extended_tails_preserve_reference_offsets_and_independent_alignment_lanes() {
    for typed in [false, true] {
        for expanded in [false, true] {
            for identity in [false, true] {
                for first_owner in [0, 4, 8] {
                    let (bytes, scope, owners, mut types) =
                        extended_fixture(typed, expanded, identity, first_owner, 0);
                    for joint_version in [3, 4, 5] {
                        types.insert(29, (JOINT_TYPE_GUID, joint_version));
                        let decoded = decode(&bytes, &scope, &owners, &types).unwrap();
                        let width = if typed { 51 } else { 11 };
                        assert_eq!(
                            scope.frame_length(),
                            190 + 25 * width + 256 - 128 * u64::from(identity)
                                + 4 * u64::from(expanded)
                        );
                        assert_eq!(
                            decoded.additional_tail_reference,
                            Some(Located {
                                value: 63,
                                offset: scope.feature_ordinal_offset() + 19 + width,
                            })
                        );
                        assert_eq!(decoded.offset(), [2.0, -3.0, 4.0]);
                        assert!(
                            (decoded.angle() - std::f64::consts::FRAC_PI_2).abs() < EPS_ALIGNMENT
                        );
                        assert!(valid_alignment(&scope, &decoded, &types, &owners));
                        let wire = serde_json::to_value(&decoded).unwrap();
                        let round_trip: DesignAssemblyAlignment =
                            serde_json::from_value(wire).unwrap();
                        assert_eq!(decoded, round_trip);
                    }
                }
            }
        }
    }
}

#[test]
fn extended_tails_keep_counted_owners_and_project_unresolved_operands() {
    for count in [0, 2] {
        for qualified in [false, true] {
            let (mut bytes, mut scope, owners, mut types) =
                extended_fixture(false, true, false, 0, count);
            types.insert(29, (JOINT_TYPE_GUID, 5));
            if qualified {
                append_locator_paths(&mut bytes, &mut types, Some((false, 2)));
            }
            let decoded = decode(&bytes, &scope, &owners, &types).unwrap();
            assert_eq!(
                decoded
                    .additional_owner_run
                    .as_ref()
                    .map(|run| run.count.value),
                if count == 0 { None } else { Some(2) }
            );
            assert!(valid_alignment(&scope, &decoded, &types, &owners));
            if let DesignScopePayloadMut::Assemble(slot) = scope.payload_mut() {
                *slot = Some(decoded);
            }
            let projected = crate::design::assembly::project_assembly_joints(
                &[scope],
                &[],
                &crate::design::components::ComponentInserts::default(),
                &[],
            );
            assert_eq!(projected.joints.len(), 1);
            assert!(projected
                .losses
                .iter()
                .any(|loss| loss.code
                    == crate::loss::F3dLossCode::AssemblyExtendedTailAssumed.kind()));
        }
    }
}

#[test]
fn extended_tails_refuse_invalid_type_reference_state_extent_and_native_offsets() {
    let (bytes, scope, owners, types) = extended_fixture(false, false, true, 4, 0);
    let decoded = decode(&bytes, &scope, &owners, &types).unwrap();
    let at = usize::try_from(scope.feature_ordinal_offset()).unwrap();
    for (offset, value) in [
        (at + 30, 0),
        (at + 31, 0),
        (at + 39, 1),
        (at + 72, 2),
        (at + 73, 2),
        (at + 86, 1),
    ] {
        let mut damaged = bytes.clone();
        damaged[offset] = value;
        assert!(decode(&damaged, &scope, &owners, &types).is_none());
    }
    for end in 0..bytes.len() - 11 {
        assert!(decode(&bytes[..end], &scope, &owners, &types).is_none());
    }
    let mut nonfinite = bytes.clone();
    nonfinite[at + 49..at + 57].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(decode(&nonfinite, &scope, &owners, &types).is_none());
    for target in [(OTHER_GUID, 1), (ADDITIONAL_TAIL_TYPE_GUID, 0)] {
        let mut damaged = types.clone();
        damaged.insert(63, target);
        assert!(decode(&bytes, &scope, &owners, &damaged).is_none());
        assert!(!valid_alignment(&scope, &decoded, &damaged, &owners));
    }
    for reference in [
        None,
        Some(Located {
            value: 0,
            offset: u64_from_index(at + 30),
        }),
        Some(Located {
            value: 63,
            offset: u64_from_index(at + 31),
        }),
    ] {
        let mut damaged = decoded.clone();
        damaged.additional_tail_reference = reference;
        assert!(!valid_alignment(&scope, &damaged, &types, &owners));
    }
    let mut wire = serde_json::to_value(&decoded).unwrap();
    wire["additional_tail_reference"] = serde_json::Value::Null;
    assert!(serde_json::from_value::<DesignAssemblyAlignment>(wire).is_err());
}
