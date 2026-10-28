// SPDX-License-Identifier: Apache-2.0

use std::collections::{HashMap, HashSet};

use super::{parse_edge_use_candidates, select_edge_use, EdgeReferences, EdgeUse};

#[test]
fn bare_edge_with_a_null_curve_and_a_valued_tolerance_is_tolerant() {
    let mut bytes = vec![0, 0x10, 0x04, 0xbc, 0, 0, 0x07, 0xa1, 0, 1];
    bytes.extend(1.307_774_721_031_768e-8_f64.to_be_bytes());
    for reference in [0x056du16, 0x04bd, 0x04b9, 1, 1, 1, 3] {
        bytes.extend(reference.to_be_bytes());
    }
    let records = parse_edge_use_candidates(&bytes, 0);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].attr, 1212);
    assert_eq!(records[0].references.canonical(), Some(0x056d));
    assert!(records[0].references.tolerant());

    // A valued tolerance does not admit an edge-use that names a curve.
    bytes[24..26].copy_from_slice(&0x0500u16.to_be_bytes());
    assert!(parse_edge_use_candidates(&bytes, 0).is_empty());
}

#[test]
fn tolerant_edge_reading_yields_to_an_ordinary_reading_of_the_same_edge() {
    let reading = |curve| EdgeUse {
        attr: 1717,
        references: EdgeReferences::Bare([0, 1716, 1406, curve, 1, 1]),
        sequence: 0x1a7c,
        offset: 0,
    };
    let tolerant = reading(1);
    let ordinary = reading(3174);
    let coedges = HashMap::new();
    let curve_attrs = HashSet::from([3174]);
    for curves in [None, Some(&curve_attrs)] {
        let candidates = [tolerant.clone(), ordinary.clone()];
        assert_eq!(
            select_edge_use(&candidates, &coedges, curves),
            Some(ordinary.clone())
        );
        assert_eq!(
            select_edge_use(std::slice::from_ref(&tolerant), &coedges, curves),
            Some(tolerant.clone())
        );
    }
}
