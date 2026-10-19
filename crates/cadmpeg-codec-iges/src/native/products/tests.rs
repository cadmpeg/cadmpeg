// SPDX-License-Identifier: Apache-2.0
//! Nested reusable source definitions keep their neutral placement tree.

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::products::OccurrenceParent;
use std::io::Cursor;

#[test]
fn nested_subfigures_project_names_and_local_placements() {
    let decoded = crate::IgesCodec
        .decode(
            &mut Cursor::new(
                crate::test_support::test_drawing_and_trimming::nested_subfigure_file(),
            ),
            &DecodeOptions::default(),
        )
        .unwrap();
    let ir = decoded.ir();
    assert_eq!(ir.model.product_definitions.len(), 2);
    let labels = ir
        .model
        .product_definitions
        .iter()
        .map(|definition| definition.label.as_deref().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        labels,
        std::collections::BTreeSet::from(["CHILD", "PARENT"])
    );
    assert_eq!(ir.model.occurrences.len(), 2);
    let root = ir
        .model
        .occurrences
        .iter()
        .find(|occurrence| matches!(occurrence.parent, OccurrenceParent::Root {}))
        .unwrap();
    let child = ir.model.occurrences.iter().find(|occurrence| matches!(&occurrence.parent, OccurrenceParent::Occurrence { occurrence: parent } if *parent == root.id)).unwrap();
    assert_eq!(
        root.transform.affine_rows(),
        [
            [2.0, 0.0, 0.0, 10.0],
            [0.0, 2.0, 0.0, 20.0],
            [0.0, 0.0, 2.0, 30.0]
        ]
    );
    assert_eq!(
        child.transform.affine_rows(),
        [
            [0.5, 0.0, 0.0, 1.0],
            [0.0, 0.5, 0.0, 2.0],
            [0.0, 0.0, 0.5, 3.0]
        ]
    );
    for record in &ir.native.namespace("iges").unwrap().arenas()["product_occurrences"] {
        assert!(record.field("world_transform").is_none());
    }
}
