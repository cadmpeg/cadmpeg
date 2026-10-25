// SPDX-License-Identifier: Apache-2.0
use super::*;

#[test]
fn distinct_equal_rows_keep_cardinality_and_same_identity_can_reuse_its_slot() {
    let mut generated = Generated {
        version: 3,
        polygon_sources: HashMap::new(),
        polygon_ids: BTreeMap::new(),
        polygon_order: Default::default(),
        tables: Default::default(),
        shapes: vec![],
        roots: String::new(),
    };
    assert_eq!(
        generated.push_polygon(3, "row A", "same".into()).unwrap(),
        1
    );
    assert_eq!(
        generated.push_polygon(3, "row B", "same".into()).unwrap(),
        2
    );
    assert_eq!(
        generated.push_polygon(3, "row A", "same".into()).unwrap(),
        1
    );
    generated.order_polygons().unwrap();
    assert_eq!(generated.tables[3], vec!["same", "same"]);
}

#[test]
fn native_order_relocates_all_current_and_native_edge_polygon_operands_only() {
    let mut generated = Generated { version:3, polygon_sources: HashMap::new(), polygon_ids: BTreeMap::new(),polygon_order: Default::default(),tables: Default::default(),shapes: vec!["Ed 0.25 1 0 0 1 8 9 -2 3 2 5 6 7 -4 5 3 2 3 C1 4 5 6 7 4 C0 1 2 3 4 5 1 7 6 1 8 9 7 1 2 8 9 0 0101000 +2 7 *".into()],roots: String::new() };
    generated.tables[3] = vec!["B".into(), "A".into(), "unowned".into()];
    generated.tables[4] = vec!["Y".into(), "X".into()];
    generated.polygon_sources =
        HashMap::from([("A".to_owned(), vec![2]), ("B".to_owned(), vec![1])]);
    generated.push_polygon(3, "A", "A".into()).unwrap();
    generated.push_polygon(3, "B", "B".into()).unwrap();
    generated.push_polygon(4, "X", "X".into()).unwrap();
    generated.push_polygon(4, "Y", "Y".into()).unwrap();
    generated.order_polygons().unwrap();
    assert_eq!(generated.tables[3], vec!["A", "B", "unowned"]);
    assert_eq!(generated.tables[4], vec!["X", "Y", "Y", "X"]);
    assert_eq!(generated.shapes[0],"Ed 0.25 1 0 0 1 8 9 -2 3 2 5 6 7 -4 5 3 2 3 C1 4 5 6 7 4 C0 1 2 3 4 5 2 7 6 3 8 9 7 3 4 8 9 0 0101000 +2 7 *");
}

#[test]
fn equal_values_do_not_replace_actual_forward_native_identity_or_imply_an_association() {
    let mut generated = Generated {
        version: 3,
        polygon_sources: HashMap::from([("A".into(), vec![2]), ("B".into(), vec![1])]),
        polygon_ids: BTreeMap::new(),
        polygon_order: Default::default(),
        tables: Default::default(),
        shapes: vec!["Ed 0 1 1 0 5 1 0 0 0101000 *".into()],
        roots: String::new(),
    };
    generated.tables[3] = vec!["same".into(), "same".into()];
    assert_eq!(generated.push_polygon(3, "A", "same".into()).unwrap(), 2);
    assert_eq!(generated.push_polygon(3, "B", "same".into()).unwrap(), 1);
    generated.order_polygons().unwrap();
    assert_eq!(generated.shapes[0], "Ed 0 1 1 0 5 2 0 0 0101000 *");

    let mut unrelated = Generated {
        version: 3,
        polygon_sources: HashMap::new(),
        polygon_ids: BTreeMap::new(),
        polygon_order: Default::default(),
        tables: Default::default(),
        shapes: vec!["Ed 0 1 1 0 5 1 0 0 0101000 *".into()],
        roots: String::new(),
    };
    unrelated.tables[3] = vec!["same".into()];
    assert_eq!(
        unrelated
            .push_polygon(3, "new native row", "same".into())
            .unwrap(),
        2
    );
    unrelated.order_polygons().unwrap();
    assert_eq!(unrelated.tables[3].len(), 2);
    assert_eq!(unrelated.shapes[0], "Ed 0 1 1 0 5 2 0 0 0101000 *");
}
