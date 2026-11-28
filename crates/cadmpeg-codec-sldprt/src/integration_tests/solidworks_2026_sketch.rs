// SPDX-License-Identifier: Apache-2.0
//! The `SolidWorks` 2026 P01a part: a 20 x 30 mm rectangle sketch, extruded.
//!
//! The point records of a `SolidWorks` 2026 `sgSketch` list carry object
//! indices 1, 2, null, null, null, null, 3 and 4; the endpoint values of its
//! curve records plus one name no point record for two of them, so the list
//! is indexed by position.

use std::io::Cursor;
use std::path::Path;

use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::features::profiles::{PlanarProfileRef, ProfileRef};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
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

#[test]
fn the_position_reading_of_p01a_states_its_assumption_once_per_line() {
    let result = decode_p01a();
    let charged = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == SldprtLossCode::SketchPointRecordIndexBasisAssumed.kind())
        .count();
    assert_eq!(charged, 4, "one per line of the rectangle");
}

#[test]
fn the_extrusion_of_p01a_consumes_the_sketch_profile() {
    let result = decode_p01a();
    let profiles = result
        .ir()
        .model
        .features
        .iter()
        .filter_map(|feature| match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::Extrude { profile, .. }) => {
                Some(profile)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(profiles.len(), 1, "one extrusion");
    assert!(
        !matches!(
            profiles[0],
            ProfileRef::Planar(PlanarProfileRef::Unresolved(_) | PlanarProfileRef::Native(_))
        ),
        "the profile is {:?}",
        profiles[0]
    );
}
