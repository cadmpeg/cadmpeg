// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_ir::ids::{CurveId, SurfaceId};

#[test]
fn synthetic_annotations_use_record_keys_independent_of_id_text() {
    let records = [Record {
        index: 37,
        name: "spline".into(),
        tokens: Vec::new().into(),
        offset: 1234,
        len: 0,
    }];
    let by_index = records
        .iter()
        .map(|record| (record.index as i64, record))
        .collect();
    let mut out = AsmBrep::default();
    let carriers = Carriers {
        procedural_support_sources: vec![(37, SurfaceId::mint("f3d:child:support#named").unwrap())],
        procedural_curve_child_sources: vec![(37, CurveId::mint("f3d:child:curve#named").unwrap())],
        ..Carriers::default()
    };
    emit_annotation_records(
        &mut out,
        &records,
        &by_index,
        &carriers,
        "source",
        IdFormat("f3d"),
    )
    .unwrap();
    let annotations: Vec<_> = out
        .annotation_records
        .iter()
        .map(|record| {
            (
                record.id.as_str(),
                record.offset,
                record.tag.as_str(),
                record.stream.as_str(),
            )
        })
        .collect();
    assert_eq!(
        annotations,
        [
            (
                "f3d:child:support#named",
                1234,
                "procedural_support",
                "source"
            ),
            (
                "f3d:child:curve#named",
                1234,
                "procedural_curve_child",
                "source"
            ),
        ]
    );
    let serde_value::Value::Map(wire) = serde_value::to_value(&out).unwrap() else {
        panic!("ASM graph object")
    };
    assert!(!wire.contains_key(&serde_value::Value::String(
        "procedural_support_sources".into()
    )));
    assert!(!wire.contains_key(&serde_value::Value::String(
        "procedural_curve_child_sources".into()
    )));
}
