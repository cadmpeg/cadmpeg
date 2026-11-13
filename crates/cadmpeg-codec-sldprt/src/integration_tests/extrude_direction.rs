// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::{ExtrudeDirection, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::math::Vector3;

use crate::loss::SldprtLossCode;
use crate::test_support::container::{
    add_solidworks_version, make_block, native_envelope_fixture, sldprt_with_body,
};
use crate::test_support::history::resolved_feature_classes_with_ids;
use crate::test_support::parasolid::owned_triangle;
use crate::SldprtCodec;

fn extrusion(native: bool, direction: Option<&str>) -> Vec<u8> {
    let direction = direction
        .map(|value| format!(" Direction=\"{value}\""))
        .unwrap_or_default();
    let keywords = format!(
        "<Keywords><Extrusion Name=\"Extrude\" Type=\"BossExtrude\" id=\"31\" EndCondition=\"Blind\"{direction}><Dimension Name=\"Depth\">10mm</Dimension></Extrusion></Keywords>"
    );
    let mut source = sldprt_with_body(&owned_triangle(0, 700, 0.0));
    source.extend(make_block(0x42, "Contents/Keywords", keywords.as_bytes()));
    add_solidworks_version(&mut source, 18000);
    if native {
        source.extend(make_block(
            0x43,
            "Contents/Config-0-ResolvedFeatures",
            &resolved_feature_classes_with_ids(&[("moExtrusion_c", "Extrude", 31)]),
        ));
    }
    native_envelope_fixture(&source)
}

#[test]
fn uncontradicted_travel_preserves_native_and_mirrored_controls() {
    for (native, vector) in [
        (false, None),
        (true, None),
        (true, Some(("0,0,1", 1.0))),
        (true, Some(("0,0,-1", -1.0))),
    ] {
        let decoded = SldprtCodec
            .decode(
                &mut Cursor::new(extrusion(native, vector.map(|(literal, _)| literal))),
                &DecodeOptions::default(),
            )
            .expect("synthetic extrusion decodes");
        let direction = decoded
            .ir()
            .model
            .features
            .iter()
            .find_map(|feature| match feature.evaluation.definition() {
                FeatureDefinition::Operation(FeatureOperation::Extrude { direction, .. }) => {
                    Some(direction)
                }
                _ => None,
            })
            .expect("extrusion definition");
        match vector {
            Some((_, z)) => assert!(matches!(direction,
                ExtrudeDirection::Explicit { vector, .. }
                    if vector.get() == Vector3::new(0.0, 0.0, z))),
            None => assert!(matches!(direction, ExtrudeDirection::ProfileNormal {})),
        }
        assert!(!decoded
            .report()
            .losses
            .iter()
            .any(|loss| { loss.code == SldprtLossCode::FeatureExtrudeDirectionUnresolved.kind() }));
    }
}

#[test]
fn decoded_opposite_cap_retains_travel_with_a_mirrored_cap_control() {
    use crate::test_support::history::sldprt_with_nested_sketch_profile;
    for side in [-1.0, 1.0] {
        let mut body = cap(0, 700, 0.0, 1.0);
        body.extend(cap(200, 701, side, side));
        let mut source = sldprt_with_nested_sketch_profile(&body);
        source.extend(make_block(0x42,"Contents/Keywords",br#"<Keywords><Sketch Name="Sketch1" Type="Sketch" id="1"/><Extrusion Name="Extrude" Type="BossExtrude" id="31" Profile="1" Operation="Join" EndCondition="Blind"><Dimension Name="Depth">1m</Dimension></Extrusion></Keywords>"#));
        add_solidworks_version(&mut source, 18000);
        let decoded = SldprtCodec
            .decode(
                &mut Cursor::new(native_envelope_fixture(&source)),
                &DecodeOptions::default(),
            )
            .expect("synthetic cap pair");
        let direction = decoded
            .ir()
            .model
            .features
            .iter()
            .find_map(|f| match f.evaluation.definition().operation() {
                FeatureOperation::Extrude { direction, .. } => Some(direction),
                _ => None,
            })
            .expect("extrusion");
        assert_eq!(
            matches!(direction, ExtrudeDirection::Unresolved {}),
            side < 0.0,
            "side={side}",
        );
        assert_eq!(
            decoded
                .report()
                .losses
                .iter()
                .any(|loss| loss.code == SldprtLossCode::FeatureExtrudeDirectionUnresolved.kind()),
            side < 0.0
        );
    }
}

fn cap(base: u16, owner: u16, z: f64, normal: f64) -> Vec<u8> {
    use crate::test_support::parasolid::{
        bridge_owned, coedge, edge_use_with_canonical, line_carrier, loop_head, plane_carrier,
        typed_single_face_ownership, vertex_use, world_point,
    };
    let points = [[0.0, 0.0, z], [1.0, 0.0, z], [0.0, 1.0, z]];
    let mut body = plane_carrier(
        base + 100,
        [0.0, 0.0, z],
        [0.0, 0.0, normal],
        [1.0, 0.0, 0.0],
    );
    body.extend(bridge_owned(base + 10, base + 20, base + 100, owner));
    body.extend(loop_head(base + 20, base + 30, base + 10));
    for index in 0_u16..3 {
        let next = (index + 1) % 3;
        let start = points[usize::from(index)];
        let end = points[usize::from(next)];
        let direction = Vector3::new(end[0] - start[0], end[1] - start[1], end[2] - start[2])
            .unit()
            .expect("unit cap edge");
        body.extend(line_carrier(
            base + 90 + index,
            start,
            [direction.x, direction.y, direction.z],
        ));
        body.extend(coedge(
            base + 30 + index,
            base + 20,
            base + 30 + next,
            base + 50 + index,
            0,
            base + 40 + index,
            false,
        ));
        body.extend(edge_use_with_canonical(
            base + 40 + index,
            0,
            base + 90 + index,
            owner,
        ));
        body.extend(vertex_use(base + 50 + index, base + 60 + index));
        body.extend(world_point(base + 60 + index, start));
    }
    typed_single_face_ownership(
        &mut body,
        owner,
        base + 70,
        base + 80,
        base + 10,
        base + 100,
        1,
    );
    body
}
