use super::*;

fn owner_run_fixture(
    version: u32,
    count: u32,
    expanded: bool,
    identity: bool,
    name: &str,
) -> (
    Vec<u8>,
    DesignParameterScope,
    Vec<DesignParameterOwner>,
    StreamTypes<'static>,
) {
    let (mut bytes, mut scope, mut owners, types) =
        named_fixture(version, 4 + count, false, expanded, identity, 0, name);
    let grammar = parse(&bytes, &scope, &types).unwrap();
    let at = grammar.additional_owner_start;
    let count_at = usize::try_from(grammar.additional_owner_count.offset).unwrap();
    bytes[count_at..count_at + 4].copy_from_slice(&count.to_le_bytes());
    let indices = [160_u32, 112, 190, 115, 180, 170];
    let mut run = Vec::new();
    for (ordinal, index) in indices
        .into_iter()
        .take(usize::try_from(count).unwrap())
        .enumerate()
    {
        reference(&mut run, index, false);
        owners.push(
            DesignParameterOwner::try_from(DesignParameterOwnerWire {
                id: format!("f3d:Design/BulkStream.dat:design-parameter-owner#{index}"),
                byte_offset: u64::from(index) * 200,
                frame_length: 104,
                class_tag: "457".to_owned().try_into().unwrap(),
                record_index: index,
                scope_record_index: 30,
                local_ordinal: u32::try_from(ordinal).unwrap() + 4,
                evaluated_value: [-0.75, 1.25, 0.5, 2.0, -3.0, 4.0][ordinal],
                evaluated_value_offset: u64::from(index) * 200 + 40,
                parameter_record_index: index + 1,
                owned_ordinal: u32::try_from(ordinal).unwrap() + 4,
                variant: Some(0),
                companion_record_index: index + 2,
            })
            .unwrap(),
        );
    }
    let shift = u64_from_index(run.len());
    bytes.splice(at..at, run);
    let table = scope
        .reference_members()
        .values()
        .zip(scope.reference_members().offsets())
        .enumerate()
        .map(|(ordinal, (index, offset))| {
            let value = if ordinal < 4 {
                *index
            } else {
                indices[ordinal - 4]
            };
            let offset = offset + shift;
            let target = usize::try_from(offset).unwrap();
            bytes[target..target + 8].copy_from_slice(&u64::from(value).to_le_bytes());
            Located { value, offset }
        })
        .collect::<Vec<_>>();
    scope
        .try_edit(|draft| {
            draft.frame_length += shift;
            draft.paired_byte_offset += shift;
            draft.reference_count_offset += shift;
            draft.kind_offset += shift;
            draft.feature_ordinal_offset += shift;
            draft.reference_members = ReferenceRun::located(table);
        })
        .unwrap();
    if name != "Assemble" {
        scope.source_kind = Some(name.to_owned());
    }
    assert_eq!(scope.source_kind_name(), name);
    (bytes, scope, owners, types)
}

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

#[test]
fn counted_owner_runs_keep_four_alignment_lanes_and_exact_source_positions() {
    for version in [4, 5] {
        for count in [0, 1, 2, 3, 4, 6] {
            for (expanded, identity, name) in [
                (false, false, "Assemble"),
                (true, false, "Assemble"),
                (true, true, "Zusammenfügen"),
            ] {
                let (bytes, scope, owners, types) =
                    owner_run_fixture(version, count, expanded, identity, name);
                let expected = if version == 5 { 809 } else { 654 }
                    + 22 * u64::from(count)
                    + 4 * u64::from(expanded)
                    - 128 * u64::from(identity)
                    + 2 * u64_from_index(name.encode_utf16().count())
                    - 16;
                assert_eq!(scope.frame_length(), expected);
                let decoded = decode(&bytes, &scope, &owners, &types).unwrap_or_else(|| {
                    panic!("version={version} count={count} expanded={expanded} identity={identity} name={name} parsed={}", parse(&bytes, &scope, &types).is_some())
                });
                assert!(valid_alignment(&scope, &decoded, &types, &owners));
                if count != 0 {
                    assert!(relation_only(&scope, &decoded, &types));
                }
                assert_eq!(decoded.owners.len(), 4);
                assert_eq!(decoded.offset(), [2.0, -3.0, 4.0]);
                assert!((decoded.angle() - std::f64::consts::FRAC_PI_2).abs() < EPS_ALIGNMENT);
                if count == 0 {
                    assert!(decoded.additional_owner_run.is_none());
                } else {
                    let run = decoded.additional_owner_run.as_ref().unwrap();
                    let at = if version == 5 { 529 } else { 374 } + 4 * u64::from(expanded)
                        - 128 * u64::from(identity);
                    assert_eq!(
                        run.count,
                        Located {
                            value: count,
                            offset: at
                        }
                    );
                    assert_eq!(
                        run.references
                            .iter()
                            .map(|reference| reference.value)
                            .collect::<Vec<_>>(),
                        [160, 112, 190, 115, 180, 170][..usize::try_from(count).unwrap()]
                    );
                    for (ordinal, reference) in run.references.iter().enumerate() {
                        assert_eq!(reference.offset, at + 4 + 11 * u64_from_index(ordinal));
                    }
                }
                let wire = serde_json::to_value(&decoded).unwrap();
                let round_trip: DesignAssemblyAlignment = serde_json::from_value(wire).unwrap();
                assert_eq!(round_trip, decoded);
            }
        }
    }
}

#[test]
fn counted_owner_runs_refuse_broken_counts_references_and_truncated_fields() {
    for version in [4, 5] {
        let (bytes, scope, owners, types) = owner_run_fixture(version, 2, false, false, "Assemble");
        let grammar = parse(&bytes, &scope, &types).unwrap();
        let count = usize::try_from(grammar.additional_owner_count.offset).unwrap();
        let first = grammar.additional_owner_start;
        for (at, value) in [
            (count - 1, 1),
            (count, 1),
            (count, 3),
            (first, 0),
            (first + 9, 1),
        ] {
            let mut damaged = bytes.clone();
            damaged[at] = value;
            assert!(decode(&damaged, &scope, &owners, &types).is_none());
        }
        for value in [0_u64, 100, 999, u64::from(u32::MAX) + 1] {
            let mut damaged = bytes.clone();
            damaged[first + 1..first + 9].copy_from_slice(&value.to_le_bytes());
            assert!(decode(&damaged, &scope, &owners, &types).is_none());
        }
        let mut damaged = bytes.clone();
        damaged[count..count + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse(&damaged, &scope, &types).is_none());
        for end in 0..bytes.len() {
            assert!(parse(&bytes[..end], &scope, &types).is_none());
        }
    }
}

#[test]
fn counted_owner_runs_validate_owner_membership_order_and_native_offsets() {
    let (bytes, scope, owners, types) = owner_run_fixture(5, 2, true, false, "Assemble");
    let decoded = decode(&bytes, &scope, &owners, &types).unwrap();
    assert!(!valid_alignment(&scope, &decoded, &types, &owners[..5]));
    let mut duplicate = owners.clone();
    duplicate.push(owners[4].clone());
    assert!(!valid_alignment(&scope, &decoded, &types, &duplicate));
    for field in ["local_ordinal", "scope_record_index"] {
        let mut changed = owners.clone();
        let mut wire = serde_json::to_value(&changed[4]).unwrap();
        wire[field] = serde_json::json!(9);
        changed[4] = serde_json::from_value(wire).unwrap();
        assert!(decode(&bytes, &scope, &changed, &types).is_none());
    }
    for mutation in 0..5 {
        let mut changed = decoded.clone();
        let run = changed.additional_owner_run.as_mut().unwrap();
        match mutation {
            0 => run.count.value = 1,
            1 => run.count.offset += 1,
            2 => run.references[0].offset += 1,
            3 => run.references.swap(0, 1),
            _ => run.references[0].value = 100,
        }
        assert!(!valid_alignment(&scope, &changed, &types, &owners));
    }
}

#[test]
fn counted_owner_runs_shift_qualified_locators_and_project_with_a_named_motion_assumption() {
    for qualified in [false, true] {
        let (mut bytes, mut scope, owners, mut types) =
            owner_run_fixture(5, 3, false, false, "Assemble");
        if qualified {
            append_locator_paths(&mut bytes, &mut types, Some((false, 2)));
        }
        let decoded = decode(&bytes, &scope, &owners, &types).unwrap();
        assert!(valid_alignment(&scope, &decoded, &types, &owners));
        assert_eq!(
            matches!(
                decoded.form,
                Some(DesignAssemblyAlignmentForm::Qualified(_))
            ),
            qualified
        );
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
        assert!(projected.losses.iter().any(|loss| loss.code
            == crate::loss::F3dLossCode::AssemblyAdditionalOwnerMotionAssumed.kind()));
    }
}
