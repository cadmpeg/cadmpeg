// SPDX-License-Identifier: Apache-2.0

use super::emit_annotation_records;
use crate::brep::{AsmBrep, Carriers};
use crate::sab::Record;
use cadmpeg_ir::ids::{CurveId, SurfaceId};

#[test]
fn annotation_curve_index_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};

    let mut out = AsmBrep {
        curves: vec![Curve {
            id: CurveId::mint("f3d:brep:entity#1").unwrap(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
            source_object: None,
        }],
        ..AsmBrep::default()
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = emit_annotation_records(
        &ctx,
        &mut out,
        &[],
        &std::collections::HashMap::new(),
        &Carriers::default(),
        "source",
        crate::asm_format!("f3d"),
    )
    .expect_err("one curve index entry exceeds zero items");
    let CodecError::ResourceLimit(limit) = error else {
        panic!("expected collection refusal: {error:?}");
    };
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
}

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
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[], &arena, &cadmpeg_core::decode::DecodePolicy::default(),
    ).expect("test decode context");
    emit_annotation_records(
        &ctx,
        &mut out,
        &records,
        &by_index,
        &carriers,
        "source",
        crate::asm_format!("f3d"),
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
