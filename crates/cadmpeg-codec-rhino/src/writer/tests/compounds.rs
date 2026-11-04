// SPDX-License-Identifier: Apache-2.0
//! Compound reductions retain their admitted child geometry.

use crate::{RhinoArchiveVersion, RhinoCodec};
use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::{
    CompoundComponent, CompoundCurveConstruction, ProceduralCurveDefinition,
};
use cadmpeg_ir::report::Severity;
use std::io::Cursor;

fn compound_document() -> cadmpeg_ir::CadIr {
    RhinoCodec
        .decode(
            &mut Cursor::new(include_bytes!(
                "../../../tests/golden/fixtures/polycurve.3dm"
            )),
            &DecodeOptions::default(),
        )
        .expect("compound fixture")
        .ir()
        .clone()
}

#[test]
fn solved_offset_names_its_discarded_construction() {
    use cadmpeg_ir::geometry::{
        curve_payloads::OffsetCurveConstruction, OffsetSide, ProceduralCurve,
    };
    use cadmpeg_ir::math::Vector3;

    let source = compound_document();
    let mut carrier = source
        .model
        .curves
        .iter()
        .find(|curve| {
            matches!(
                curve.geometry,
                cadmpeg_ir::geometry::CurveGeometry::Solved(_)
            )
        })
        .expect("solved curve")
        .clone();
    carrier.source_object = None;
    carrier.native_ref = None;
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.curves.push(carrier.clone());
    carrier.id = "test:model:curve#offset"
        .try_into()
        .expect("curve identity");
    let owner = carrier.id.clone();
    ir.model.curves.push(carrier);
    let source_id = ir.model.curves[0].id.clone();
    let construction = "test:model:procedural-curve#offset"
        .try_into()
        .expect("construction identity");
    ir.model
        .add_procedural_curve(
            &owner,
            ProceduralCurve::new(
                construction,
                ProceduralCurveDefinition::Offset(
                    OffsetCurveConstruction::try_new(
                        source_id,
                        0.0,
                        OffsetSide::PlaneNormal {
                            normal: Vector3::new(0.0, 0.0, 1.0),
                        },
                        None,
                    )
                    .expect("offset"),
                ),
            ),
        )
        .expect("attach construction");
    let mut bytes = Vec::new();
    let report = RhinoCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("plan offset")
        .write_to(&mut bytes)
        .expect("write offset");
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.message.contains("test:model:procedural-curve#offset")));
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("read solved carriers");
    assert_eq!(decoded.ir().model.curves.len(), 2);
}

#[test]
fn unsupported_compound_convention_keeps_writable_components() {
    let mut ir = compound_document();
    let children = crate::writer::compounds::children(&ir);
    let parent = ir
        .model
        .curves
        .iter()
        .find(|curve| {
            curve.geometry.procedural_construction().is_some() && !children.contains(&curve.id)
        })
        .expect("root compound");
    let construction = parent
        .geometry
        .procedural_construction()
        .expect("construction")
        .clone();
    let parent_id = parent.id.to_string();
    ir.model
        .procedural_curves
        .iter_mut()
        .find(|record| record.id == construction)
        .expect("held construction")
        .edit_definition(|definition| {
            let ProceduralCurveDefinition::Compound(compound) = definition else {
                panic!("compound")
            };
            let components = compound
                .components()
                .iter()
                .map(|component| CompoundComponent {
                    parameter: component.parameter.get(),
                    component: component.component.clone(),
                })
                .collect();
            *compound = CompoundCurveConstruction::try_new(Vec::new(), components, None)
                .expect("valid neutral convention without native interval spelling");
        });
    let mut bytes = Vec::new();
    let report = RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .expect("degraded plan")
        .write_to(&mut bytes)
        .expect("degraded archive");
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.severity == Severity::Error && loss.message.contains(&parent_id)));
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("children remain valid native objects");
    assert_eq!(
        decoded.ir().model.curves.len(),
        ir.model.curves.len() - 1,
        "only the unrepresentable enclosing compound is omitted"
    );
}

#[test]
fn compound_cycle_is_not_hidden_when_it_has_no_standalone_root() {
    let mut ir = compound_document();
    let mut sibling = ir
        .model
        .curves
        .iter()
        .find(|curve| {
            curve.geometry.solved().is_some() && curve.geometry.procedural_construction().is_none()
        })
        .expect("solved child")
        .clone();
    sibling.id = "test:model:curve#independent".try_into().expect("identity");
    sibling.source_object = None;
    sibling.native_ref = None;
    ir.model.curves.push(sibling.clone());
    let children = crate::writer::compounds::children(&ir);
    let parent = ir
        .model
        .curves
        .iter()
        .find(|curve| {
            curve.geometry.procedural_construction().is_some() && !children.contains(&curve.id)
        })
        .expect("root compound");
    let construction = parent
        .geometry
        .procedural_construction()
        .expect("construction")
        .clone();
    let parent_id = parent.id.clone();
    ir.model
        .procedural_curves
        .iter_mut()
        .find(|record| record.id == construction)
        .expect("held construction")
        .replace_definition(ProceduralCurveDefinition::Compound(
            CompoundCurveConstruction::try_new(
                vec![0.0, 1.0],
                vec![CompoundComponent {
                    parameter: 0.0,
                    component: parent_id.clone(),
                }],
                None,
            )
            .expect("typed cyclic graph"),
        ));
    let mut bytes = Vec::new();
    let report = RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .expect("plan independent geometry")
        .write_to(&mut bytes)
        .expect("write independent geometry");
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.message.contains(parent_id.as_str())));
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode independent geometry");
    assert!(decoded
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.geometry == sibling.geometry));
}

#[test]
fn orphaned_compound_does_not_hide_an_independent_child() {
    let mut ir = compound_document();
    let children = crate::writer::compounds::children(&ir);
    let parent = ir
        .model
        .curves
        .iter()
        .find(|curve| {
            curve.geometry.procedural_construction().is_some() && !children.contains(&curve.id)
        })
        .expect("compound parent")
        .id
        .clone();
    ir.model.curves.retain(|curve| curve.id != parent);
    let child = ir
        .model
        .curves
        .iter_mut()
        .find(|curve| children.contains(&curve.id) && curve.geometry.solved().is_some())
        .expect("solved child");
    child.source_object = None;
    let expected = child.geometry.clone();
    let mut bytes = Vec::new();
    RhinoCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("write independent child");
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode independent child");
    assert!(decoded
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.geometry == expected));
}

/// Check the native grammar independently of the writer's direct-byte buffer.
fn assert_compound_class_checksum(
    bytes: &[u8],
    range: std::ops::Range<usize>,
    archive: crate::chunks::ArchiveVersion,
) -> usize {
    use crate::chunks::{
        chunk_at, verify_checksum, verify_checksum_ranges, BoundedReader, ChecksumStatus,
    };

    let header = crate::layout::long_chunk_header_wide::LEN;
    let chunk = chunk_at(bytes, range.start - header, bytes.len(), archive, false)
        .expect("class-data chunk is bounded");
    assert_eq!(chunk.body(), range);
    let mut reader =
        BoundedReader::new(bytes, range.start, range.end).expect("compound body is bounded");
    assert_eq!(reader.u8().expect("version"), 0x10);
    let count =
        usize::try_from(reader.i32().expect("segment count")).expect("positive segment count");
    assert!(count > 0);
    assert_eq!(reader.i32().expect("first reserved word"), 0);
    assert_eq!(reader.i32().expect("second reserved word"), 0);
    for _ in 0..6 {
        reader.f64().expect("reserved bounding-box coordinate");
    }
    assert_eq!(
        usize::try_from(reader.i32().expect("parameter count"))
            .expect("nonnegative parameter count"),
        count + 1
    );
    for _ in 0..=count {
        assert!(reader.f64().expect("segment parameter").is_finite());
    }
    let direct_end = reader.position();
    assert_eq!(
        verify_checksum_ranges(bytes, &chunk, std::iter::once(range.start..direct_end)),
        Ok(ChecksumStatus::Valid),
        "the parent CRC covers its scalar prefix, excluding complete child wrappers"
    );
    assert_ne!(
        verify_checksum(bytes, &chunk),
        Ok(ChecksumStatus::Valid),
        "including the child chunks is the incorrect native CRC convention"
    );
    let mut compounds = 1;
    for _ in 0..count {
        let start = reader.position();
        let child = chunk_at(bytes, start, range.end, archive, false)
            .expect("complete child class wrapper");
        let mut warnings = crate::loss::Diagnostics::new();
        let class =
            crate::objects::parse_class_wrapper(bytes, child.range(), archive, &mut warnings)
                .expect("child class grammar");
        if class.class_uuid == crate::curves::POLYCURVE {
            compounds += assert_compound_class_checksum(bytes, class.class_data_range, archive);
        }
        reader
            .skip(child.next_offset() - start)
            .expect("child fits its parent");
    }
    assert_eq!(reader.remaining(), 0);
    compounds
}

/// Relative bound on a round-tripped curve coordinate, knot or weight. The
/// writer re-encodes each value, so equal geometry can differ in the last
/// bits.
const EPS_COMPOUND_GEOMETRY_ROUND_TRIP: f64 = 1.0e-12;

/// Each curve's solved geometry as JSON. A written archive mints its own curve
/// identities and order, so curves pair by geometry.
fn curve_geometry(ir: &cadmpeg_ir::CadIr) -> Vec<serde_json::Value> {
    ir.model
        .curves
        .iter()
        .map(|curve| {
            serde_json::to_value(curve.geometry.solved()).expect("solved curve geometry serializes")
        })
        .collect()
}

/// Structural equality in which numbers agree within
/// `EPS_COMPOUND_GEOMETRY_ROUND_TRIP` relative to their magnitude.
fn json_numbers_close(actual: &serde_json::Value, expected: &serde_json::Value) -> bool {
    use serde_json::Value;
    match (actual, expected) {
        (Value::Number(actual), Value::Number(expected)) => {
            match (actual.as_f64(), expected.as_f64()) {
                (Some(actual), Some(expected)) => {
                    (actual - expected).abs()
                        <= EPS_COMPOUND_GEOMETRY_ROUND_TRIP
                            * actual.abs().max(expected.abs()).max(1.0)
                }
                _ => actual == expected,
            }
        }
        (Value::Array(actual), Value::Array(expected)) => {
            actual.len() == expected.len()
                && actual
                    .iter()
                    .zip(expected)
                    .all(|(actual, expected)| json_numbers_close(actual, expected))
        }
        (Value::Object(actual), Value::Object(expected)) => {
            actual.len() == expected.len()
                && expected.iter().all(|(key, expected)| {
                    actual
                        .get(key)
                        .is_some_and(|actual| json_numbers_close(actual, expected))
                })
        }
        _ => actual == expected,
    }
}

#[test]
fn nested_compound_crc_excludes_child_chunks_and_preserves_all_curve_geometry() {
    let inputs: [(&str, &[u8]); 2] = [
        (
            "polycurve",
            include_bytes!("../../../tests/golden/fixtures/polycurve.3dm"),
        ),
        (
            "curves_mixed",
            include_bytes!("../../../tests/golden/fixtures/curves_mixed.3dm"),
        ),
    ];
    for (name, input) in inputs {
        let source = RhinoCodec
            .decode(&mut Cursor::new(input), &DecodeOptions::default())
            .expect("frozen compound source is admitted");
        let expected = curve_geometry(source.ir());
        for version in [RhinoArchiveVersion::V5, RhinoArchiveVersion::V8] {
            let mut bytes = Vec::new();
            RhinoCodec
                .plan(
                    EncodeInput::new(source.ir(), None),
                    TargetRequest::Explicit(version.descriptor().id.as_str()),
                )
                .expect("supported compound target")
                .write_to(&mut bytes)
                .expect("complete compound archive");
            let scan = crate::container::scan_owned(bytes.clone())
                .expect("written archive framing is valid");
            let mut compounds = 0;
            for object in &scan.objects {
                let object = object.framed().expect("written object is fully framed");
                if object.class_uuid == crate::curves::POLYCURVE {
                    compounds += assert_compound_class_checksum(
                        scan.data,
                        object.class_data_range.clone(),
                        scan.archive,
                    );
                }
            }
            assert_eq!(compounds, 2, "{name} {version:?}: both compound levels");
            let decoded = RhinoCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .expect("compound geometry decodes");
            let actual = curve_geometry(decoded.ir());
            assert_eq!(
                actual.len(),
                expected.len(),
                "{name} {version:?}: curve count"
            );
            let mut unmatched = actual;
            for geometry in &expected {
                let index = unmatched
                    .iter()
                    .position(|candidate| json_numbers_close(candidate, geometry))
                    .unwrap_or_else(|| {
                        panic!("{name} {version:?}: no written curve matches {geometry}")
                    });
                unmatched.swap_remove(index);
            }
        }
    }
}

#[test]
fn solved_composite_writes_each_exact_segment() {
    use cadmpeg_ir::geometry::{
        nurbs::NurbsCurve, CompositeCurveSegment, CompositeCurveTransition, Curve, CurveGeometry,
        SolvedCurveGeometry,
    };
    use cadmpeg_ir::math::Point3;
    let mut ir = cadmpeg_ir::CadIr::empty();
    for (id, start, end) in [
        ("test:model:curve#a", 0.0, 1.0),
        ("test:model:curve#b", 1.0, 2.0),
    ] {
        ir.model.curves.push(Curve {
            id: id.try_into().unwrap(),
            native_ref: None,
            parameter_range: None,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                NurbsCurve::from_lanes(
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![Point3::new(start, 0.0, 0.0), Point3::new(end, 0.0, 0.0)],
                    None,
                    false,
                )
                .unwrap(),
            )),
            source_object: None,
        });
    }
    let segments = ir
        .model
        .curves
        .iter()
        .map(|curve| CompositeCurveSegment {
            curve: curve.id.clone(),
            same_sense: true,
            transition: CompositeCurveTransition::Continuous,
        })
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    ir.model.curves.push(Curve {
        id: "test:model:curve#composite".try_into().unwrap(),
        native_ref: None,
        parameter_range: None,
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Composite {
            segments,
            self_intersect: None,
        }),
        source_object: None,
    });
    let plan = RhinoCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("plan composite segments");
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("test:model:curve#composite")));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.curves.len(), 2);
    let spans = decoded
        .ir()
        .model
        .curves
        .iter()
        .map(|curve| {
            let point = |parameter| {
                cadmpeg_ir::eval::decode::curve_point(
                    cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
                    &curve.geometry,
                    parameter,
                )
                .unwrap()
                .get()
            };
            (point(0.0), point(1.0))
        })
        .collect::<Vec<_>>();
    assert!(spans.contains(&(Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0))));
    assert!(spans.contains(&(Point3::new(1.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0))));
}
