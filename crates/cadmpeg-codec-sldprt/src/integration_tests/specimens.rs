// SPDX-License-Identifier: Apache-2.0
//! Regression tests over the controlled specimen pairs in `corpus/sldprt/`.

use std::fs;
use std::path::PathBuf;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::{ExtrudeDirection, FeatureOperation};
use cadmpeg_ir::sketches::SketchGeometryDefinition;

use crate::SldprtCodec;

fn decode_specimen(relative: &str) -> cadmpeg_ir::codec::DecodeResult {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/sldprt")
        .join(relative);
    let mut file = fs::File::open(&path).expect("specimen file exists");
    SldprtCodec
        .decode(&mut file, &DecodeOptions::default())
        .expect("specimen decodes")
}

#[test]
fn pair1_front_flip_decodes_reversed_profile_normal_di16() {
    let blind = decode_specimen("Pair1/plane_front_blind.SLDPRT");
    let flip = decode_specimen("Pair1/plane_front_flip.SLDPRT");

    for (result, expected_reversed) in [(&blind, false), (&flip, true)] {
        let extrude = result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some("Boss-Extrude1"))
            .expect("Boss-Extrude1");
        let FeatureOperation::Extrude { direction, .. } =
            extrude.evaluation.definition().operation()
        else {
            panic!("expected Extrude operation");
        };
        if expected_reversed {
            assert!(matches!(
                direction,
                ExtrudeDirection::ReversedProfileNormal {}
            ));
        } else {
            assert!(matches!(direction, ExtrudeDirection::ProfileNormal {}));
        }
        assert!(
            !result.report().losses.iter().any(|loss| {
                loss.code.to_string() == "sldprt/feature.extrude-direction-unresolved"
            }),
            "unexpected extrude-direction-unresolved loss"
        );
    }
}

#[test]
fn pair1_right_plane_resolves_right_principal_frame_sf02() {
    let right = decode_specimen("Pair1/plane_right_blind.SLDPRT");
    let sketch = &right.ir().model.sketches[0];
    let (_, normal, u_axis) = sketch.resolved_placement().expect("resolved placement");
    assert_eq!(normal.get().x, 1.0);
    assert_eq!(normal.get().y, 0.0);
    assert_eq!(normal.get().z, 0.0);
    assert_eq!(u_axis.get().x, 0.0);
    assert_eq!(u_axis.get().y, 0.0);
    assert_eq!(u_axis.get().z, -1.0);
}

#[test]
fn pair3_concentric_circles_decode_both_radii_di70_di71() {
    for path in [
        "Pair3/sketch_concentric_one_dim.SLDPRT",
        "Pair3/sketch_concentric_ring.SLDPRT",
    ] {
        let result = decode_specimen(path);
        let mut radii: Vec<f64> = result
            .ir()
            .model
            .sketch_entities
            .iter()
            .filter_map(|entity| match entity.geometry.definition() {
                SketchGeometryDefinition::Circle { radius, .. } => Some(radius.get()),
                _ => None,
            })
            .collect();
        radii.sort_by(f64::total_cmp);
        radii.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
        assert_eq!(radii.len(), 2, "{path}: expected both r=5 and r=10 circles");
        assert!((radii[0] - 5.0).abs() < 1e-6);
        assert!((radii[1] - 10.0).abs() < 1e-6);
    }
}

#[test]
fn pair4_arcs_cw_ccw_decodes_ffff_arc_ev26() {
    let result = decode_specimen("Pair4/sketch_arcs_cw_ccw.SLDPRT");
    let has_r10_arc = result.ir().model.sketch_entities.iter().any(|entity| {
        matches!(
            entity.geometry.definition(),
            SketchGeometryDefinition::Arc { center, radius, .. }
                if (center.u - 5.0).abs() < 1e-4
                    && (center.v - 18.660_254).abs() < 1e-4
                    && (radius.get() - 10.0).abs() < 1e-4
        )
    });
    assert!(
        has_r10_arc,
        "expected 0xffff arc centered at (5, 18.660254) with r=10; got: {:#?}",
        result
            .ir()
            .model
            .sketch_entities
            .iter()
            .map(|e| (e.id().to_string(), e.geometry.definition()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn pair5_revolve_90_default_vs_flipped_direction_ev25() {
    let default = decode_specimen("Pair5/revolve_90_default.SLDPRT");
    let flipped = decode_specimen("Pair5/revolve_90_flipped.SLDPRT");

    let axis_y = |result: &cadmpeg_ir::codec::DecodeResult| {
        let revolve = result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some("Revolve1"))
            .expect("Revolve1");
        let FeatureOperation::Revolve { construction, .. } =
            revolve.evaluation.definition().operation()
        else {
            panic!("expected Revolve");
        };
        let cadmpeg_ir::features::RevolveConstruction::Resolved { axis, .. } = construction else {
            panic!("expected resolved revolve");
        };
        axis.direction.get().y
    };

    assert!((axis_y(&default) - 1.0).abs() < 1e-9);
    assert!((axis_y(&flipped) + 1.0).abs() < 1e-9);
    assert!(
        !flipped
            .report()
            .losses
            .iter()
            .any(|loss| loss.code.to_string() == "sldprt/feature.input-archive-walk-stopped"),
        "flipped revolve should not stop the CArchive walk"
    );
}
