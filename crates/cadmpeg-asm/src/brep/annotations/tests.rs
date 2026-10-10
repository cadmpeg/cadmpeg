// SPDX-License-Identifier: Apache-2.0

use super::emit_annotation_records;
use crate::brep::{AsmBrep, Carriers};
use cadmpeg_ir::ids::{CurveId, SurfaceId};

fn source_curves(count: usize) -> AsmBrep {
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
    AsmBrep {
        curves: (1..=count)
            .map(|index| Curve {
                id: CurveId::mint(format!("f3d:brep:entity#{index}")).unwrap(),
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
                source_object: None,
            })
            .collect(),
        ..Default::default()
    }
}

fn annotation_source_refusal(allocation: bool) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let mut out = source_curves(3);
    let expected = source_curves(3);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // The first source visit and the empty-map key probe precede allocation.
    policy.limits.max_work_units = if allocation {
        1 + u64::try_from(out.curves[0].id.as_str().len()).unwrap()
    } else {
        0
    };
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(first)) = emit_annotation_records(
        &ctx,
        &mut out,
        &[],
        &std::collections::HashMap::new(),
        &mut Carriers::default(),
        "source",
        crate::asm_format!("f3d"),
    ) else {
        panic!("expected annotation source or map allocation refusal");
    };
    if allocation {
        assert_eq!(first.dimension, ResourceDimension::CollectionItems);
        assert_eq!(first.operation, "ASM annotation curve geometry index");
    } else {
        assert_eq!(first.dimension, ResourceDimension::WorkUnits);
        assert_eq!(first.operation, "ASM annotation source arena");
    }
    assert_eq!((first.limit, first.used, first.additional), (0, 0, 1));
    assert_eq!(out.curves, expected.curves);
    assert!(out.annotation_records.is_empty());
    for _ in 0..64 {
        for mut replay in [source_curves(3), AsmBrep::default()] {
            assert!(matches!(emit_annotation_records(
                &ctx, &mut replay, &[], &std::collections::HashMap::new(),
                &mut Carriers::default(), "source", crate::asm_format!("f3d"),
            ), Err(CodecError::ResourceLimit(last)) if last == first));
        }
    }
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn annotation_index_source_refuses_before_first_curve() {
    annotation_source_refusal(false);
}

#[test]
fn annotation_index_allocation_refuses_after_one_visit_without_admitting_tail() {
    annotation_source_refusal(true);
}

#[test]
fn annotation_index_sources_empty_input_executes_no_work() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut out = AsmBrep::default();
    emit_annotation_records(
        &ctx,
        &mut out,
        &[],
        &std::collections::HashMap::new(),
        &mut Carriers::default(),
        "source",
        crate::asm_format!("f3d"),
    )
    .unwrap();
    assert!(out.annotation_records.is_empty());
    ctx.finish_session().unwrap();
}

#[test]
fn annotation_index_sources_borrow_curve_ids_without_retained_copy() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let mut out = source_curves(3);
    let expected = source_curves(3);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    emit_annotation_records(
        &ctx,
        &mut out,
        &[],
        &std::collections::HashMap::new(),
        &mut Carriers::default(),
        "source",
        crate::asm_format!("f3d"),
    )
    .unwrap();
    assert_eq!(out.curves, expected.curves);
    assert!(out.annotation_records.is_empty());
    ctx.finish_session().unwrap();
}

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
        &mut Carriers::default(),
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
fn annotation_stream_refuses_retained_limit() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};

    let records = [crate::test_support::sab::record(
        1,
        "straight".into(),
        Vec::new().into(),
        0,
        0,
    )];
    let make_out = || AsmBrep {
        curves: vec![Curve {
            id: CurveId::mint("f3d:brep:entity#1").unwrap(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
            source_object: None,
        }],
        ..AsmBrep::default()
    };
    let by_index = std::collections::HashMap::from([(1, &records[0])]);
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "ASM annotation stream",
        |cap| {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            emit_annotation_records(
                &ctx,
                &mut make_out(),
                &records,
                &by_index,
                &mut Carriers::default(),
                "source",
                crate::asm_format!("f3d"),
            )
        },
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("expected retained refusal: {error:?}");
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(limit.operation, "ASM annotation stream");
}

#[test]
fn synthetic_annotations_use_record_keys_independent_of_id_text() {
    let records = [crate::test_support::sab::record(
        37,
        "spline".into(),
        Vec::new().into(),
        1234,
        0,
    )];
    let by_index = records
        .iter()
        .map(|record| {
            (
                i64::try_from(record.index).expect("test value fits"),
                record,
            )
        })
        .collect();
    let mut out = AsmBrep::default();
    let mut carriers = Carriers {
        procedural_support_sources: vec![(37, SurfaceId::mint("f3d:child:support#named").unwrap())],
        procedural_curve_child_sources: vec![(37, CurveId::mint("f3d:child:curve#named").unwrap())],
        ..Carriers::default()
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::default(),
    )
    .expect("test decode context");
    emit_annotation_records(
        &ctx,
        &mut out,
        &records,
        &by_index,
        &mut carriers,
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

fn synthetic_first_visit_refusal(count: usize, support: bool) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let fixture = || {
        if support {
            Carriers {
                procedural_support_sources: (0..count)
                    .map(|_| (37, SurfaceId::mint("f3d:child:support#named").unwrap()))
                    .collect(),
                ..Carriers::default()
            }
        } else {
            Carriers {
                procedural_curve_child_sources: (0..count)
                    .map(|_| (37, CurveId::mint("f3d:child:curve#named").unwrap()))
                    .collect(),
                ..Carriers::default()
            }
        }
    };
    let mut carriers = fixture();
    let replays: Vec<_> = (0..64).map(|_| fixture()).collect();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_entities = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut out = AsmBrep::default();
    // The missing record must not be inspected before its source visit.
    let Err(CodecError::ResourceLimit(first)) = emit_annotation_records(
        &ctx,
        &mut out,
        &[],
        &std::collections::HashMap::new(),
        &mut carriers,
        "source",
        crate::asm_format!("f3d"),
    ) else {
        panic!("expected the first synthetic annotation source visit to refuse");
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(
        first.operation,
        if support {
            "ASM procedural support annotations"
        } else {
            "ASM procedural child annotations"
        }
    );
    assert_eq!((first.limit, first.used, first.additional), (0, 0, 1));
    assert!(out.annotation_records.is_empty());
    for mut replay in replays {
        assert!(matches!(emit_annotation_records(
            &ctx, &mut out, &[], &std::collections::HashMap::new(),
            &mut replay, "source", crate::asm_format!("f3d")),
            Err(CodecError::ResourceLimit(last)) if last == first));
        assert_eq!(
            replay.procedural_support_sources.len(),
            if support { count } else { 0 }
        );
        assert_eq!(
            replay.procedural_curve_child_sources.len(),
            if support { 0 } else { count }
        );
        assert!(matches!(emit_annotation_records(
            &ctx, &mut out, &[], &std::collections::HashMap::new(),
            &mut Carriers::default(), "source", crate::asm_format!("f3d")),
            Err(CodecError::ResourceLimit(last)) if last == first));
    }
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn synthetic_support_annotation_first_visit_refuses_without_admitting_tail() {
    for count in [4, 64] {
        synthetic_first_visit_refusal(count, true);
    }
}

#[test]
fn synthetic_child_annotation_first_visit_refuses_without_admitting_tail() {
    for count in [4, 64] {
        synthetic_first_visit_refusal(count, false);
    }
}

#[test]
fn edge_annotation_range_derivation_preserves_record_lookup_results() {
    use cadmpeg_ir::ids::{EdgeId, VertexId};
    use cadmpeg_ir::topology::{Edge, EdgeCarrier};
    for name in [Some("ellipse"), Some("straight"), None] {
        let mut tokens = vec![crate::sab::Token::Long(0); 8];
        tokens.push(crate::sab::Token::Ref(37));
        let mut records = vec![crate::test_support::sab::record(
            1,
            "edge".into(),
            tokens.into(),
            123,
            0,
        )];
        if let Some(name) = name {
            records.push(crate::test_support::sab::record(
                37,
                name.into(),
                Vec::new().into(),
                456,
                0,
            ));
        }
        let by_index = records
            .iter()
            .map(|record| (i64::try_from(record.index).unwrap(), record))
            .collect();
        let mut out = AsmBrep {
            edges: vec![Edge {
                id: EdgeId::mint("f3d:brep:entity#1").unwrap(),
                carrier: EdgeCarrier::unbounded(Some(CurveId::mint("f3d:brep:entity#37").unwrap())),
                start: VertexId::mint("f3d:brep:entity#2").unwrap(),
                end: VertexId::mint("f3d:brep:entity#3").unwrap(),
                tolerance: None,
            }],
            ..AsmBrep::default()
        };
        crate::test_support::with_service_context(&[], |ctx| {
            emit_annotation_records(
                ctx,
                &mut out,
                &records,
                &by_index,
                &mut Carriers::default(),
                "source",
                crate::asm_format!("f3d"),
            )
            .unwrap();
        })
        .unwrap();
        assert_eq!(out.annotation_records.len(), 1);
        let annotation = &out.annotation_records[0];
        assert_eq!(annotation.id, "f3d:brep:entity#1");
        assert_eq!(annotation.offset, 123);
        assert_eq!(annotation.tag.as_str(), "edge");
        assert_eq!(annotation.stream, "source");
        let expected: &[&str] = if name == Some("ellipse") {
            &["param_range"]
        } else {
            &[]
        };
        assert_eq!(annotation.derived_fields, expected);
    }
}
