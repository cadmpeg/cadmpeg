// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_ir::sketches::SketchGeometryDefinition;

#[test]
fn a_paired_point_tail_keeps_a_closed_profile() {
    let (mut inventory, _) = closed_square_inventory();
    assert!(super::super::point_tail::paired_association_loss(&inventory).is_none());
    let mut bytes = content_at(3, 25, &[-1, 2500]);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&3u32.to_le_bytes());
    for coordinate in [0.0f64, 0.0] {
        bytes.extend_from_slice(&coordinate.to_le_bytes());
    }
    bytes.extend(list(2, &[]));
    bytes.extend(list(2, &[]));
    for value in [2u32, 0x3000_0002, 2, 2, 0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for scalar in [0.25f64, 0.75] {
        bytes.extend_from_slice(&scalar.to_le_bytes());
    }
    for value in [0x3000_0002u32, 2, 2, 7] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for operand in [0x8000_0008u32, 0x8000_000b] {
        bytes.extend(list(2, &[]));
        bytes.extend_from_slice(&operand.to_le_bytes());
    }
    let point = parse(&bytes, |_, source| {
        parse_entity(SketchEntityTag::Point, source, 25).expect("paired point")
    });
    inventory.entities[0] = located(point, POINT_TYPE, 3);
    let loss = super::super::point_tail::paired_association_loss(&inventory)
        .expect("unprojected paired association loss");
    assert_eq!(loss.code.namespace(), "inventor");
    assert_eq!(
        loss.code.local_code(),
        "sketch.paired-association-unprojected"
    );
    assert_eq!(loss.severity, cadmpeg_ir::report::Severity::Warning);

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("projection context");
    let projected = project(&ctx, &inventory, &[]).expect("sketch projection");
    assert_eq!(projected.unresolved_sketches, 0);
    assert_eq!(projected.unresolved_entities, 0);
    assert_eq!(projected.sketches.len(), 1);
    assert_eq!(projected.entities.len(), 8);
    assert_eq!(projected.sketches[0].profiles[0].len(), 4);
    let native_point = inventory.entities[0].id();
    let point = projected
        .entities
        .iter()
        .find(|entity| entity.native_ref.as_deref() == Some(native_point.as_str()))
        .expect("projected paired point")
        .geometry
        .definition();
    assert!(matches!(
        point,
        SketchGeometryDefinition::Point { position } if *position == Point2::new(0.0, 0.0)
    ));
}
