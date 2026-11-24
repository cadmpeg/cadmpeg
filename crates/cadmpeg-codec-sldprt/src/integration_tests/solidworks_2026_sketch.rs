// SPDX-License-Identifier: Apache-2.0
//! The `SolidWorks` 2026 P01a part: a 20 x 30 mm rectangle sketch, extruded.
//!
//! A `SolidWorks` 2026 `sgSketch` lists point records that no curve names
//! (their object index is null), so its curve records index the list by
//! position and not by object index.

use std::io::Cursor;
use std::path::Path;

use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::sketches::SketchGeometryDefinition;

use crate::loss::SldprtLossCode;
use crate::SldprtCodec;

fn decode_p01a() -> DecodeResult {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/sldprt/solidworks-2026_P01a.sldprt");
    let bytes =
        std::fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("the SolidWorks 2026 P01a part decodes")
}

#[test]
fn the_sketch_of_p01a_resolves_to_four_lines() {
    let result = decode_p01a();
    let entities = &result.ir().model.sketch_entities;
    let mut lines = Vec::new();
    for entity in entities {
        match entity.geometry.definition() {
            SketchGeometryDefinition::Point { .. } => {}
            SketchGeometryDefinition::Line { start, end } => {
                let mut ends = [(start.u, start.v), (end.u, end.v)];
                ends.sort_by(|a, b| a.partial_cmp(b).expect("finite"));
                lines.push(ends);
            }
            other => panic!("{}: unexpected geometry {other:?}", entity.id()),
        }
    }
    let corner = |u: f64, v: f64| (u * 10.0, v * 15.0);
    let mut expected = [
        [corner(-1.0, -1.0), corner(1.0, -1.0)],
        [corner(-1.0, -1.0), corner(-1.0, 1.0)],
        [corner(-1.0, 1.0), corner(1.0, 1.0)],
        [corner(1.0, -1.0), corner(1.0, 1.0)],
    ];
    let key = |line: &[(f64, f64); 2]| {
        [line[0].0, line[0].1, line[1].0, line[1].1].map(|value| format!("{:.6}", value + 0.0))
    };
    lines.sort_by_key(key);
    expected.sort_by_key(key);
    assert_eq!(
        lines.iter().map(key).collect::<Vec<_>>(),
        expected.iter().map(key).collect::<Vec<_>>(),
        "the four edges of the 20 x 30 rectangle"
    );
    let losses = &result.report().losses;
    for code in [
        SldprtLossCode::SketchHandleUnresolved,
        SldprtLossCode::SketchNativeGeometry,
        SldprtLossCode::DesignFeatureUnresolved,
    ] {
        assert!(
            losses.iter().all(|loss| loss.code != code.kind()),
            "{} is charged",
            code.kind().local_code()
        );
    }
}
