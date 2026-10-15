use crate::families::standard::decode::{
    include_native_endpoint_pairs, standard_native_support_edge_ids,
    standard_serialized_endpoint_pairs, standard_successor_endpoint_points,
};
use crate::families::standard::records::{StandardCurveGeometry, StandardCurveSupport};
use std::collections::{BTreeMap, HashMap};

#[test]
fn standard_native_binding_arrays_refuse_before_each_collection() {
    use cadmpeg_core::CodecError;
    use std::collections::HashSet;

    let supports = [StandardCurveSupport {
        pos: 0,
        tag: 70,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Line,
    }];
    let native_edges = BTreeMap::from([(70, [100, 300])]);
    let native_support_ids = HashMap::from([(70, true)]);
    let mut operations = HashSet::new();
    for limit in 0..=10 {
        match crate::test_support::with_collection_limit(limit, |ctx| {
            standard_serialized_endpoint_pairs(ctx, &supports, &native_edges, &[100, 300])
        }) {
            Err(CodecError::ResourceLimit(error)) => {
                operations.insert(error.operation);
            }
            Ok(Some(_)) => break,
            outcome => panic!("unexpected roster binding: {outcome:?}"),
        }
    }
    for operation in [
        "catia_roster_point_identities",
        "catia_roster_endpoint_pairs",
    ] {
        assert!(operations.contains(operation), "no refusal at {operation}");
    }
    operations.clear();
    for limit in 0..=10 {
        match crate::test_support::with_collection_limit(limit, |ctx| {
            standard_native_support_edge_ids(ctx, &supports, &native_support_ids)
        }) {
            Err(CodecError::ResourceLimit(error)) => {
                operations.insert(error.operation);
            }
            Ok(ids) if ids == [Some(70)] => break,
            outcome => panic!("unexpected support binding: {outcome:?}"),
        }
    }
    for operation in [
        "catia_native_support_row_counts",
        "catia_native_support_edge_ids",
    ] {
        assert!(operations.contains(operation), "no refusal at {operation}");
    }
    operations.clear();
    for limit in 0..=10 {
        match crate::test_support::with_collection_limit(limit, |ctx| {
            standard_successor_endpoint_points(ctx, &supports, &[71, 72])
        }) {
            Err(CodecError::ResourceLimit(error)) => {
                operations.insert(error.operation);
            }
            Ok(points) if points == [[Some(0), Some(1)]] => break,
            outcome => panic!("unexpected successor binding: {outcome:?}"),
        }
    }
    for operation in [
        "catia_successor_point_identities",
        "catia_successor_endpoint_points",
    ] {
        assert!(operations.contains(operation), "no refusal at {operation}");
    }
    let mut candidates = [Vec::new()];
    assert!(matches!(
        crate::test_support::with_collection_limit(0, |ctx| include_native_endpoint_pairs(ctx, &mut candidates, &[Some([0, 1])])),
        Err(CodecError::ResourceLimit(error)) if error.operation == "catia_native_endpoint_domain_points"
    ));
    assert!(candidates[0].is_empty());
}
