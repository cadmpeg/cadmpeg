// SPDX-License-Identifier: Apache-2.0
//! Compound reductions retain their admitted child geometry.

use crate::{RhinoArchiveVersion, RhinoCodec};
use cadmpeg_core::CodecError;
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
                    component: parent_id,
                }],
                None,
            )
            .expect("typed cyclic graph"),
        ));
    let error = RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .expect_err("cyclic document");
    assert!(matches!(error, CodecError::Malformed(message) if message.contains("cyclic compound")));
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

/// Follow the construction's ordered children, independent of arena ordering.
fn compound_geometry(ir: &cadmpeg_ir::CadIr) -> Vec<cadmpeg_ir::geometry::SolvedCurveGeometry> {
    let children = crate::writer::compounds::children(ir);
    let root = ir
        .model
        .curves
        .iter()
        .find(|curve| {
            curve.geometry.procedural_construction().is_some() && !children.contains(&curve.id)
        })
        .expect("one compound root");
    let mut pending = vec![root];
    let mut geometry = Vec::new();
    while let Some(curve) = pending.pop() {
        let mut carrier = curve.geometry.solved().expect("held carrier").clone();
        if let cadmpeg_ir::geometry::SolvedCurveGeometry::Unknown { record } = &mut carrier {
            // The enclosing physical source record is reminted with its archive.
            // All typed leaf coordinates, poles, weights and domains stay exact.
            *record = None;
        }
        geometry.push(carrier);
        pending.extend(
            crate::writer::compounds::components(ir, curve)
                .into_iter()
                .rev(),
        );
    }
    geometry
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
        let expected = compound_geometry(source.ir());
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
            let actual = compound_geometry(decoded.ir());
            assert_eq!(
                actual, expected,
                "{name} {version:?}: curve geometry and order"
            );
        }
    }
}
