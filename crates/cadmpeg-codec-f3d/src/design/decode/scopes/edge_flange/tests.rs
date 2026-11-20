use super::*;
use crate::design::test_support::{lengthen_scope_frame, name_scope_references};
use crate::test_support::{indexed_header, with_decode_context};

fn reference(out: &mut Vec<u8>, index: Option<u32>, typed: bool) {
    let Some(index) = index else {
        out.push(0);
        return;
    };
    out.push(1);
    out.extend(u64::from(index).to_le_bytes());
    if typed {
        out.extend(36_u32.to_le_bytes());
        out.extend(OPERATION_GUID.as_bytes());
    }
    out.extend([0, 0]);
}

fn scope() -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:test/Design1/BulkStream.dat:scope#10",
        DesignFeatureKind::EdgeFlange,
        10,
    );
    name_scope_references(&mut scope, [20, 21, 22, 23, 30, 40, 50, 60, 70, 71, 72, 73]);
    scope
        .try_edit(|draft| lengthen_scope_frame(draft, 4096))
        .expect("bounded scope");
    scope
}

#[test]
fn reference_edge_local_zero_is_distinct_from_a_null_reference() {
    let mut bytes = Vec::new();
    reference(&mut bytes, Some(0), false);
    reference(&mut bytes, None, false);
    let mut view = View::over_retained(&bytes);
    let present = local(&mut view, &HashMap::new())
        .expect("local reference")
        .into_target()
        .expect("present target");
    assert_eq!(present.value, 0);
    assert_eq!(view.position(), 11);
    assert!(local(&mut view, &HashMap::new())
        .expect("null reference")
        .into_target()
        .is_none());
    assert_eq!(view.position(), 12);
}

fn operation(count: u32, results: u32, wide: bool, typed: bool, missing_height: bool) -> Vec<u8> {
    let mut bytes = vec![0; 21];
    bytes.extend(2_u32.to_le_bytes());
    bytes.extend(count.to_le_bytes());
    for index in 0..count {
        reference(&mut bytes, Some(20 + index), typed);
    }
    reference(&mut bytes, Some(30), typed);
    bytes.extend(2_u32.to_le_bytes());
    reference(&mut bytes, Some(40), typed);
    reference(&mut bytes, (!missing_height).then_some(50), typed);
    bytes.extend(4_u32.to_le_bytes());
    bytes.extend(vec![0; if wide { 11 } else { 2 }]);
    bytes.extend(0.25_f64.to_le_bytes());
    bytes.extend(vec![0; if wide { 6 } else { 4 }]);
    bytes.extend(results.to_le_bytes());
    for ordinal in 0..results {
        reference(&mut bytes, Some(100 + ordinal), typed);
        bytes.extend(u32::from(ordinal + 1 < results).to_le_bytes());
    }
    bytes.extend(1_u32.to_le_bytes());
    reference(&mut bytes, Some(60), typed);
    bytes.extend(vec![0; if wide { 12 } else { 8 }]);
    bytes.extend(count.to_le_bytes());
    for index in 0..count {
        reference(&mut bytes, Some(70 + index), typed);
    }
    bytes
}

#[test]
fn reference_edge_reads_counts_padding_reference_widths_and_null_owners() {
    with_decode_context(|ctx| {
        for (count, results, wide) in [
            (1, 2, false),
            (1, 2, true),
            (2, 0, true),
            (2, 5, false),
            (4, 5, true),
        ] {
            for typed in [false, true] {
                for missing_height in [false, true] {
                    let mut scopes = [scope()];
                    let mut bytes = operation(count, results, wide, typed, missing_height);
                    bytes.resize(4096, 0);
                    let records = IndexedRecordOffsets::build(&bytes);
                    let types = HashMap::from([(10, (OPERATION_GUID, if wide { 6 } else { 4 }))]);
                    bind(ctx, &bytes, &records, &types, &mut scopes).expect("bounded native read");
                    let parsed = scopes[0].edge_flange_construction().expect("operation");
                    assert!(parsed.valid_for_scope(&scopes[0]));
                    let mut invalid = parsed.clone();
                    invalid.edge_count.as_mut().expect("edge count").value += 1;
                    assert!(!invalid.valid_for_scope(&scopes[0]));
                    assert_eq!(parsed.bend_position.as_ref().expect("position").value, 2);
                    assert_eq!(
                        parsed.radius_control.as_ref().expect("radius").value.get(),
                        0.25
                    );
                    assert_eq!(parsed.height_owner.is_none(), missing_height);
                    assert_eq!(parsed.angle_owner.as_ref().expect("angle").value, 40);
                    assert_eq!(
                        parsed.edges.len(),
                        usize::try_from(count).expect("edge count")
                    );
                    assert_eq!(
                        parsed.results.len(),
                        usize::try_from(results).expect("result count")
                    );
                    assert_eq!(
                        parsed
                            .edges
                            .last()
                            .expect("edge")
                            .group
                            .as_ref()
                            .expect("group")
                            .value,
                        69 + count
                    );
                }
            }
        }
    });
}

#[test]
fn reference_edge_refuses_truncation_disagreement_and_ambiguous_windows() {
    with_decode_context(|ctx| {
        let scope = scope();
        let bytes = operation(1, 2, false, false, false);
        let mut view = View::over_retained(&bytes);
        view.skip(21).expect("prologue");
        assert!(candidate(view, &scope, &HashMap::new(), false, false).is_some());
        for end in 21..bytes.len() {
            let mut view = View::over_retained(&bytes[..end]);
            view.skip(21).expect("bounded prologue");
            assert!(candidate(view, &scope, &HashMap::new(), false, false).is_none());
        }
        let mut invalid = bytes.clone();
        invalid[25..29].copy_from_slice(&u32::MAX.to_le_bytes());
        let mut view = View::over_retained(&invalid);
        view.skip(21).expect("prologue");
        assert!(candidate(view, &scope, &HashMap::new(), false, false).is_none());
        invalid = bytes.clone();
        invalid[30..38].copy_from_slice(&999_u64.to_le_bytes());
        let mut view = View::over_retained(&invalid);
        view.skip(21).expect("prologue");
        assert!(candidate(view, &scope, &HashMap::new(), false, false).is_none());
        let mut duplicate = bytes.clone();
        duplicate.extend(bytes);
        duplicate.resize(4096, 0);
        let mut scopes = [scope];
        bind(
            ctx,
            &duplicate,
            &IndexedRecordOffsets::build(&duplicate),
            &HashMap::from([(10, (OPERATION_GUID, 6))]),
            &mut scopes,
        )
        .expect("ambiguous read");
        assert!(scopes[0].edge_flange_construction().is_none());
    });
}

#[test]
fn reference_edge_requires_its_registered_operation_family_and_version() {
    with_decode_context(|ctx| {
        let mut bytes = operation(1, 2, false, false, false);
        bytes.resize(4096, 0);
        let records = IndexedRecordOffsets::build(&bytes);
        for types in [
            HashMap::new(),
            HashMap::from([(10, (OPERATION_GUID, 3))]),
            HashMap::from([(10, (OPERATION_GUID, 7))]),
            HashMap::from([(10, ("AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE", 4))]),
        ] {
            let mut scopes = [scope()];
            bind(ctx, &bytes, &records, &types, &mut scopes).expect("unsupported registration");
            assert!(scopes[0].edge_flange_construction().is_none());
        }
    });
}

#[test]
fn reference_edge_wrapper_keeps_boundary_and_offset_owner_order() {
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"000", 20);
    bytes.extend([0; 13]);
    bytes.extend(5_u32.to_le_bytes());
    bytes.extend(1_u32.to_le_bytes());
    for index in [Some(70), Some(71), Some(72), None, Some(73)] {
        reference(&mut bytes, index, false);
    }
    bytes.extend([0; 2]);
    reference(&mut bytes, Some(24), false);
    bytes.push(0);
    reference(&mut bytes, Some(10), false);
    indexed_header(&mut bytes, *b"001", 20);
    let types = HashMap::from([(20, (OFFSET_WRAPPER_GUID, 1))]);
    let edge = DesignReferenceFlangeEdge {
        wrapper: Some(Located {
            value: 20,
            offset: 29,
        }),
        group: Some(Located {
            value: 70,
            offset: 150,
        }),
        ..Default::default()
    };
    let parsed = offset_wrapper(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &types,
        &scope(),
        &edge,
    )
    .expect("five member wrapper");
    assert_eq!(parsed.first_boundary.expect("first boundary").value, 71);
    assert_eq!(parsed.second_boundary.expect("second boundary").value, 72);
    assert!(parsed.first_owner.is_none());
    assert_eq!(parsed.second_owner.expect("second offset").value, 73);
    bytes[28..32].copy_from_slice(&2_u32.to_le_bytes());
    assert!(offset_wrapper(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &types,
        &scope(),
        &edge
    )
    .is_none());
}
