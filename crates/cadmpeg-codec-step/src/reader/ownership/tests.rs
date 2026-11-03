// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_ir::{Codec, DecodeOptions, SourceGeometryRole};
use std::io::Cursor;

#[test]
fn orphaned_carriers_of_a_failed_face_are_written_as_roots() {
    let source = include_str!("../../../tests/fixtures/ap214_sheet.p21");
    let source = source
        .lines()
        .filter(|line| !(50..=57).any(|id| line.starts_with(&format!("#{id}="))))
        .collect::<Vec<_>>()
        .join("\n")
        .replace("#7,#57,", "#7,#16,");
    let changed = source.replace("ADVANCED_FACE('',(#26)", "ADVANCED_FACE('',()");
    assert_ne!(changed, source);
    let decoded = crate::StepCodec::default()
        .decode(&mut Cursor::new(changed), &DecodeOptions::default())
        .unwrap();
    assert!(decoded.ir().model.faces.is_empty());
    assert_eq!(decoded.ir().model.surfaces.len(), 1);
    for source in decoded
        .ir()
        .model
        .surfaces
        .iter()
        .filter_map(|s| s.source_object.as_ref())
        .chain(
            decoded
                .ir()
                .model
                .curves
                .iter()
                .filter_map(|c| c.source_object.as_ref()),
        )
        .chain(
            decoded
                .ir()
                .model
                .points
                .iter()
                .filter_map(|p| p.source_object.as_ref()),
        )
    {
        assert_eq!(source.geometry_role, Some(SourceGeometryRole::Support));
    }
    let mut output = Vec::new();
    let report = crate::export::write_step(
        decoded.ir(),
        &mut output,
        crate::StepSchema::Ap242Edition3,
        &crate::StepWriteOptions::default(),
    )
    .unwrap();
    // No live record references the carriers: their owners did not survive
    // the decode, so the support role is vacuous and the carriers are
    // written as roots so they round-trip (#233 §5.1).
    let written = std::str::from_utf8(&output).unwrap();
    assert!(written.contains("PLANE("));
    assert!(written.contains("LINE("));
    assert!(written.contains("CARTESIAN_POINT("));
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::StepLossCode::GeometryCarrierNotWritten.kind()));
    let roundtrip = crate::StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        roundtrip.ir().model.surfaces.len(),
        decoded.ir().model.surfaces.len()
    );
    assert_eq!(
        roundtrip.ir().model.curves.len(),
        decoded.ir().model.curves.len()
    );
    assert_eq!(
        roundtrip.ir().model.points.len(),
        decoded.ir().model.points.len()
    );
}

#[test]
fn an_explicit_source_geometry_root_remains_independent_of_a_failed_face() {
    let source = include_str!("../../../tests/fixtures/ap214_sheet.p21");
    let changed = source
        .replace("ADVANCED_FACE('',(#26)", "ADVANCED_FACE('',()")
        .replace(
            "ENDSEC;\nEND-ISO",
            "#90=GEOMETRIC_SET('',(#28));\nENDSEC;\nEND-ISO",
        );
    assert!(changed.contains("#90="));
    let decoded = crate::StepCodec::default()
        .decode(&mut Cursor::new(changed), &DecodeOptions::default())
        .unwrap();
    assert!(decoded.ir().model.faces.is_empty());
    assert_eq!(
        decoded.ir().model.surfaces[0]
            .source_object
            .as_ref()
            .unwrap()
            .geometry_role,
        Some(SourceGeometryRole::Independent)
    );
    let mut output = Vec::new();
    crate::export::write_step(
        decoded.ir(),
        &mut output,
        crate::StepSchema::Ap242Edition3,
        &crate::StepWriteOptions::default(),
    )
    .unwrap();
    // The independent root is exported as a representation member. A set
    // wrapper with no product binding would re-decode as opaque (#233 §5.4).
    assert!(std::str::from_utf8(&output)
        .unwrap()
        .contains("SHAPE_REPRESENTATION('',("));
}
