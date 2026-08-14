// SPDX-License-Identifier: Apache-2.0
//! Material and face-color decode tests.
#![allow(clippy::unwrap_used)]
#![allow(unused_imports)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions, Encoder};

use crate::container;
use crate::test_support::*;
use crate::SldprtCodec;

#[test]
fn decode_transfers_body_material_color() {
    let f = sldprt_with_body_and_material(&triangle_body(), "Steel", [32, 64, 128]);
    let mut cur = Cursor::new(f);

    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    let color = result.ir().model.bodies[0].color.expect("body color");
    assert!((color.r - 32.0 / 255.0).abs() < 1e-6);
    assert!((color.g - 64.0 / 255.0).abs() < 1e-6);
    assert!((color.b - 128.0 / 255.0).abs() < 1e-6);
    assert_eq!(result.ir().model.appearances.len(), 1);
    assert_eq!(result.ir().model.appearance_bindings.len(), 1);
    assert_eq!(
        result.ir().model.appearances[0].name.as_deref(),
        Some("Steel")
    );
}

#[test]
fn decode_preserves_ambiguous_materials_without_fabricating_ownership() {
    let mut source = sldprt_with_body(&triangle_body());
    let mut materials = material_payload("Steel", [32, 64, 128]);
    materials.extend(material_payload("Aluminum", [160, 170, 180]));
    source.extend(make_block(0x40, "SWObjects", &materials));

    let mut result = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.appearances.len(), 2);
    assert!(result.ir().model.appearance_bindings.is_empty());
    assert!(result
        .ir()
        .model
        .bodies
        .iter()
        .all(|body| body.color.is_none() && body.name.is_none()));

    result.ir_mut().model.points[0].position.z += 1.0;
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(result.ir(), result.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(regenerated.ir().model.appearances.len(), 2);
    assert_eq!(
        regenerated
            .ir()
            .model
            .appearances
            .iter()
            .filter_map(|appearance| appearance.name.as_deref())
            .collect::<Vec<_>>(),
        vec!["Steel", "Aluminum"]
    );
    assert!(regenerated.ir().model.appearance_bindings.is_empty());
}

#[test]
fn decode_binds_entity53_color_to_face() {
    use cadmpeg_ir::appearance::AppearanceTarget;
    let mut body = Vec::new();
    body.extend(entity51(2, 500, 0x0017, &[700, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 700, 0x0015, &[0, 0, 0, 0, 0, 900]));
    body.extend(entity53_color(900, [0.25, 0.5, 0.75]));
    body.extend(owned_triangle(0, 700, 0.0));
    let mut cur = Cursor::new(sldprt_with_body(&body));
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        result.report().losses.len(),
        1,
        "{:#?}",
        result.report().losses
    );
    assert_eq!(
        result.report().losses[0].message,
        "1 configuration state(s) are inferred from geometry partitions without native configuration definitions."
    );
    let binding = result
        .ir()
        .model
        .appearance_bindings
        .iter()
        .find(|binding| matches!(binding.target, AppearanceTarget::Face(_)))
        .expect("face binding");
    let appearance = result
        .ir()
        .model
        .appearances
        .iter()
        .find(|appearance| appearance.id == binding.appearance)
        .unwrap();
    let color = appearance.base_color.unwrap();
    assert_eq!([color.r, color.g, color.b], [0.25, 0.5, 0.75]);
}

#[test]
fn decode_does_not_bind_color_to_an_unemitted_face() {
    use cadmpeg_ir::appearance::AppearanceTarget;

    let mut body = Vec::new();
    body.extend(entity51(1, 700, 0x0015, &[0, 0, 0, 0, 0, 900]));
    body.extend(entity53_color(900, [0.25, 0.5, 0.75]));
    body.extend(entity51(1, 701, 0x0015, &[0, 0, 0, 0, 0, 901]));
    body.extend(entity53_color(901, [0.75, 0.5, 0.25]));
    body.extend(owned_triangle(0, 700, 0.0));
    body.extend(plane_carrier(
        200,
        [2.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    ));
    body.extend(bridge_owned(110, 120, 200, 701));

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.appearances.len(), 2);
    assert_eq!(
        result
            .ir()
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| matches!(binding.target, AppearanceTarget::Face(_)))
            .count(),
        1
    );
    assert!(cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).is_ok());
}

#[test]
fn decode_binds_adjacent_entity53_color_to_disc14_face() {
    use cadmpeg_ir::appearance::AppearanceTarget;
    let mut body = Vec::new();
    body.extend(entity51(2, 500, 0x0017, &[700, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 700, 0x0014, &[0, 0, 0, 0, 0, 0]));
    body.extend(entity53_color(901, [1.0, 0.125, 0.0]));
    body.extend(owned_triangle(0, 700, 0.0));
    let mut cur = Cursor::new(sldprt_with_body(&body));
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    let binding = result
        .ir()
        .model
        .appearance_bindings
        .iter()
        .find(|binding| matches!(binding.target, AppearanceTarget::Face(_)))
        .expect("face binding");
    let color = result
        .ir()
        .model
        .appearances
        .iter()
        .find(|appearance| appearance.id == binding.appearance)
        .unwrap()
        .base_color
        .unwrap();
    assert_eq!([color.r, color.g, color.b], [1.0, 0.125, 0.0]);
}
