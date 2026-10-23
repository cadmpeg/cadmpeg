// SPDX-License-Identifier: Apache-2.0

use super::emit_annotation_records;
use crate::brep::{AsmBrep, Carriers};
use crate::sab::Record;
use cadmpeg_ir::ids::{CurveId, SurfaceId};

fn emit(
    out: &mut AsmBrep,
    records: &[Record],
    by_index: &std::collections::HashMap<i64, &Record>,
    carriers: &Carriers,
    format: crate::ids::IdFormat,
) -> Result<(), cadmpeg_core::CodecError> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    emit_annotation_records(&ctx, out, records, by_index, carriers, "source", format)
}

#[test]
fn every_annotation_copy_of_the_stream_name_is_charged() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    const SOURCES: usize = 1 << 6;
    const STREAM: usize = 1 << 12;
    let records = [Record {
        index: 37,
        name: "spline".into(),
        tokens: Vec::new().into(),
        offset: 0,
        len: 0,
    }];
    let by_index = records
        .iter()
        .map(|record| (i64::try_from(record.index).unwrap(), record))
        .collect();
    let carriers = Carriers {
        procedural_support_sources: (0..SOURCES)
            .map(|source| {
                (
                    37,
                    SurfaceId::mint(format!("f3d:child:support#{source}")).unwrap(),
                )
            })
            .collect(),
        ..Carriers::default()
    };
    let stream = "s".repeat(STREAM);
    let annotate = |policy: &DecodePolicy| {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy).unwrap();
        let mut out = AsmBrep::default();
        emit_annotation_records(
            &ctx,
            &mut out,
            &records,
            &by_index,
            &carriers,
            &stream,
            crate::asm_format!("f3d"),
        )
        .map(|()| out)
    };
    let out = annotate(&DecodePolicy::service()).unwrap();
    assert_eq!(out.annotation_records.len(), SOURCES);
    assert!(out
        .annotation_records
        .iter()
        .all(|annotation| annotation.stream == stream));

    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = cadmpeg_core::decode::u64_from_index(SOURCES * STREAM / 2);
    let Err(CodecError::ResourceLimit(limit)) = annotate(&policy) else {
        panic!("expected a retained-bytes refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
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
        .map(|record| {
            (
                i64::try_from(record.index).expect("test value fits"),
                record,
            )
        })
        .collect();
    let mut out = AsmBrep::default();
    let carriers = Carriers {
        procedural_support_sources: vec![(37, SurfaceId::mint("f3d:child:support#named").unwrap())],
        procedural_curve_child_sources: vec![(37, CurveId::mint("f3d:child:curve#named").unwrap())],
        ..Carriers::default()
    };
    emit(
        &mut out,
        &records,
        &by_index,
        &carriers,
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

#[test]
fn ellipse_edge_annotation_names_only_a_stored_carrier_range() {
    use crate::sab::Token;
    use cadmpeg_ir::ids::{EdgeId, VertexId};
    use cadmpeg_ir::topology::{Edge, EdgeCarrier};
    let mut tokens = vec![Token::Ref(-1); 9];
    tokens[8] = Token::Ref(2);
    let records = [
        Record {
            index: 1,
            name: "edge".into(),
            tokens: tokens.into(),
            offset: 0,
            len: 0,
        },
        Record {
            index: 2,
            name: "ellipse".into(),
            tokens: Vec::new().into(),
            offset: 0,
            len: 0,
        },
    ];
    let by_index = std::collections::HashMap::from([(1, &records[0]), (2, &records[1])]);
    for range in [None, Some([0.0, 1.0])] {
        let mut out = AsmBrep {
            edges: vec![Edge {
                native_ref: None,
                id: EdgeId::from(crate::brep::id(crate::asm_format!("sat"), 1)),
                carrier: EdgeCarrier::new(
                    Some(CurveId::from(crate::brep::id(crate::asm_format!("sat"), 2))),
                    range,
                )
                .unwrap(),
                start: VertexId::from(crate::brep::id(crate::asm_format!("sat"), 3)),
                end: VertexId::from(crate::brep::id(crate::asm_format!("sat"), 4)),
                tolerance: None,
            }],
            ..AsmBrep::default()
        };
        emit(
            &mut out,
            &records,
            &by_index,
            &Carriers::default(),
            crate::asm_format!("sat"),
        )
        .unwrap();
        let annotation = &out.annotation_records[0];
        assert_eq!(annotation.id, "sat:brep:entity#1");
        assert_eq!(
            annotation.derived_fields,
            if range.is_some() {
                vec!["carrier.param_range"]
            } else {
                vec![]
            }
        );
    }
}

#[test]
fn unresolved_analytic_carriers_have_no_derived_geometry_fields() {
    use cadmpeg_ir::geometry::{
        Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
    };
    let records = [
        Record {
            index: 0,
            name: "plane".into(),
            tokens: Vec::new().into(),
            offset: 0,
            len: 0,
        },
        Record {
            index: 1,
            name: "straight".into(),
            tokens: Vec::new().into(),
            offset: 0,
            len: 0,
        },
    ];
    let mut out = AsmBrep {
        surfaces: vec![Surface {
            native_ref: None,
            id: SurfaceId::from(crate::brep::id(crate::asm_format!("sat"), 0)),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
            source_object: None,
        }],
        curves: vec![Curve {
            native_ref: None,
            id: CurveId::from(crate::brep::id(crate::asm_format!("sat"), 1)),
            parameter_range: None,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
            source_object: None,
        }],
        ..AsmBrep::default()
    };
    emit(
        &mut out,
        &records,
        &std::collections::HashMap::new(),
        &Carriers::default(),
        crate::asm_format!("sat"),
    )
    .unwrap();
    assert_eq!(out.annotation_records.len(), 2);
    assert!(out
        .annotation_records
        .iter()
        .all(|annotation| annotation.derived_fields.is_empty()));
}
