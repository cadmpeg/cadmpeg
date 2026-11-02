// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::DecodeDepth;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::sketches::SketchGeometryDefinition;

use crate::loss::InventorLossCode;
use crate::pmdc::PmDcReference;
use crate::sketch::{PmDcSketchEntity, PmDcSketchEntityKind};
use crate::test_support::test_fixtures::{
    acis_sphere_kernel_stream, content, primary_envelope_fixture_with_design, push_u16, push_u32,
};
use crate::InventorCodec;

fn planar_type(time_low: u32) -> [u8; 16] {
    let first = time_low.to_le_bytes();
    [
        first[0], first[1], first[2], first[3], 0xd0, 0x11, 0xd0, 0xd2, 0x00, 0x08, 0xcc, 0xbc,
        0x06, 0x63, 0xdc, 0x09,
    ]
}

fn references(bytes: &mut Vec<u8>, marker: u16, indices: &[u32]) {
    push_u16(bytes, marker);
    push_u16(bytes, 0x3000);
    push_u32(
        bytes,
        u32::try_from(indices.len()).expect("synthetic count"),
    );
    if !indices.is_empty() {
        if marker == 8 {
            push_u16(bytes, 0);
            push_u16(bytes, 0);
        } else {
            push_u32(bytes, 0);
            push_u32(bytes, 0);
        }
        for &index in indices {
            push_u32(bytes, 0x8000_0000 | index);
        }
    }
}

fn entity_prefix(ordinal: u32) -> Vec<u8> {
    let mut bytes = content(ordinal);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, 0x8000_0003);
    bytes
}

fn arc_envelope(listed: bool, radius: f64) -> Vec<u8> {
    let mut transform = content(0);
    push_u16(&mut transform, 0x8421);
    push_u16(&mut transform, 0x7bde);
    let mut direction = content(1);
    push_u32(&mut direction, 0);
    for value in [0.0_f64, 0.0, 0.0, 1.0] {
        direction.extend_from_slice(&value.to_le_bytes());
    }
    let mut sketch = content(2);
    push_u32(&mut sketch, 0);
    push_u32(&mut sketch, 0);
    references(
        &mut sketch,
        8,
        if listed { &[4, 5, 6, 7] } else { &[4, 5, 6] },
    );
    for value in [0x8000_0001, 0x8000_0002, 0, 0] {
        push_u32(&mut sketch, value);
    }
    let mut records = vec![
        (crate::sketch::TRANSFORM_TYPE, transform),
        (planar_type(0xce52_df40), direction),
        (crate::pmdc::inventor_id(0x9087_4d11), sketch),
    ];
    for (index, position) in [[0.0_f64, 0.0], [1.0, 0.0], [0.0, 1.0]]
        .into_iter()
        .enumerate()
    {
        let mut point = entity_prefix(u32::try_from(index).expect("point ordinal") + 3);
        for value in position {
            point.extend_from_slice(&value.to_le_bytes());
        }
        references(&mut point, 2, &[]);
        references(&mut point, 2, &[]);
        records.push((planar_type(0xce52_df35), point));
    }
    let mut arc = entity_prefix(6);
    references(&mut arc, 2, &[5, 6]);
    references(&mut arc, 2, &[]);
    push_u32(&mut arc, 0x8000_0004);
    arc.extend_from_slice(&radius.to_le_bytes());
    arc.push(0);
    records.push((planar_type(0xce52_df3b), arc));
    primary_envelope_fixture_with_design(&acis_sphere_kernel_stream(21_800), &records)
}

#[test]
fn unlisted_and_inconsistent_arcs_retain_their_records_with_a_pinned_graph_loss() {
    for (listed, radius, sketches, entities, arcs, retained) in [
        (true, 1.0, 1, 4, 1, None),
        (false, 1.0, 1, 3, 0, Some((0, 1))),
        (true, 2.0, 0, 0, 0, Some((1, 4))),
    ] {
        let bytes = arc_envelope(listed, radius);
        let decoded = InventorCodec
            .decode(
                &mut std::io::Cursor::new(bytes),
                &DecodeOptions {
                    depth: DecodeDepth::Full,
                    ..DecodeOptions::default()
                },
            )
            .expect("synthetic sketch envelope");
        let ir = decoded.ir();
        assert_eq!(ir.model.sketches.len(), sketches);
        assert_eq!(ir.model.sketch_entities.len(), entities);
        assert_eq!(
            ir.model
                .sketch_entities
                .iter()
                .filter(|entity| matches!(
                    entity.geometry.definition(),
                    SketchGeometryDefinition::Arc { .. }
                ))
                .count(),
            arcs
        );
        let native = ir
            .native
            .namespace("inventor")
            .expect("native records")
            .arena_as::<PmDcSketchEntity>("pm_dc_sketch_entities")
            .expect("typed entity records");
        assert_eq!(native.len(), 4);
        let stored = native
            .iter()
            .find_map(|entity| match &entity.kind {
                PmDcSketchEntityKind::Circle {
                    points,
                    radius,
                    state,
                    ..
                } => Some((points, radius, state)),
                _ => None,
            })
            .expect("retained arc operands");
        assert_eq!(stored.1.get(), radius);
        assert_eq!(*stored.2, 0);
        assert_eq!(
            stored.0.references(),
            &[
                PmDcReference::from_packed(0x8000_0005),
                PmDcReference::from_packed(0x8000_0006),
            ]
        );
        let notes = decoded
            .report()
            .losses
            .iter()
            .filter(|note| note.code == InventorLossCode::SketchGraphOpen.kind())
            .collect::<Vec<_>>();
        if let Some((sketches, entities)) = retained {
            let [note] = notes.as_slice() else {
                panic!("one graph loss")
            };
            assert_eq!(note.code.namespace(), "inventor");
            assert_eq!(note.code.local_code(), "sketch.graph-open");
            assert!(note.message.contains(&format!(
                "Retained {sketches} Inventor sketch record(s), {entities} sketch-entity record(s), and 0 sketch-constraint record(s)"
            )));
        } else {
            assert!(notes.is_empty());
        }
    }
}
