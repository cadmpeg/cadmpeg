// SPDX-License-Identifier: Apache-2.0
//! Exact canonical line conservation through a source-less native archive.

use std::io::Cursor;

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::analytic::LineCurve;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::topology::IncreasingParameterInterval;
use cadmpeg_ir::units::UnitVector3;
use cadmpeg_ir::CadIr;
use cadmpeg_test_support::comparison::{assert_decodes_agree, Allowances, Mode};

use crate::{RhinoArchiveVersion, RhinoCodec};

#[test]
fn source_less_solved_line_preserves_the_required_record_under_same_comparison() {
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        native_ref: None,
        id: "test:geometry:curve#0"
            .try_into()
            .expect("standalone curve identity"),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(LineCurve::new(
            FinitePoint3::ZERO,
            UnitVector3::X_AXIS,
        ))),
        parameter_range: Some(IncreasingParameterInterval::new([0.0, 1.0]).expect("finite range")),
        source_object: None,
    });
    let before = serde_json::to_value(&ir).expect("original canonical line document");
    let mut output = Vec::new();
    RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .expect("finite line can be authored")
        .write_to(&mut output)
        .expect("source-less finite line archive");
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("native finite line archive decodes");
    let after = serde_json::to_value(decoded.ir()).expect("redecoded complete document");
    assert_decodes_agree(&before, &after, &Allowances::default(), Mode::Same);
}
