//! Declared legacy profile handle coordinate tests.

use super::{
    legacy_declared_handle_coordinates, marker_coordinates, raw2, sketch_input_entities,
    SketchInputKind, LEGACY_SKETCH_MARKER, SKETCH_MARKER,
};

#[test]
fn legacy_declared_handle_markers_decode_their_planar_coordinates() {
    let mut payload = vec![0; 170 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&[0x1e, 0x00]);
    payload[58..66].copy_from_slice(&0.045f64.to_le_bytes());
    payload[66..74].copy_from_slice(&(-0.0225f64).to_le_bytes());
    payload[74..84].copy_from_slice(&[0x00, 0x00, 0x03, 0x00, 0xff, 0xff, 0x01, 0x00, 0x0c, 0x00]);
    payload[84..96].copy_from_slice(b"sgLineHandle");
    payload[96..106].copy_from_slice(&[0x03, 0x00, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00]);
    payload[106..108].copy_from_slice(&[0x2d, 0x82]);
    payload[108..110].copy_from_slice(&4u16.to_le_bytes());
    payload[110..114].fill(0xff);
    payload[118..124].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    payload[170..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        raw2(legacy_declared_handle_coordinates(&payload, 0)),
        Some([0.045, -0.0225])
    );
    assert_eq!(
        raw2(marker_coordinates(&payload, 0)),
        Some([0.045, -0.0225])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind(),
        SketchInputKind::Point
    );

    let mut linked = payload.clone();
    linked[78..170].fill(0);
    linked[78..82].copy_from_slice(&[0x15, 0x84, 0x00, 0x00]);
    linked[82..86].fill(0xff);
    linked[90..96].copy_from_slice(&[0xff, 0xff, 0x01, 0x00, 0x0c, 0x00]);
    linked[96..108].copy_from_slice(b"sgLineHandle");
    linked[110..114].fill(0xff);
    linked[118..124].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    linked[166..170].copy_from_slice(&4u32.to_le_bytes());
    assert_eq!(
        raw2(legacy_declared_handle_coordinates(&linked, 0)),
        Some([0.045, -0.0225])
    );
    linked[90] = 0;
    assert_eq!(raw2(legacy_declared_handle_coordinates(&linked, 0)), None);

    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[170..].copy_from_slice(SKETCH_MARKER);
    assert_eq!(
        raw2(legacy_declared_handle_coordinates(&payload, 0)),
        Some([0.045, -0.0225])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind(),
        SketchInputKind::Point
    );
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[76..78].copy_from_slice(&2u16.to_le_bytes());
    payload[96..98].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        raw2(legacy_declared_handle_coordinates(&payload, 0)),
        Some([0.045, -0.0225])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind(),
        SketchInputKind::Point
    );
    payload[76..78].copy_from_slice(&3u16.to_le_bytes());
    assert_eq!(
        raw2(legacy_declared_handle_coordinates(&payload, 0)),
        Some([0.045, -0.0225])
    );
    payload[17..21].fill(0);
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[76..78].copy_from_slice(&3u16.to_le_bytes());
    payload[96..98].copy_from_slice(&3u16.to_le_bytes());
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[170..].copy_from_slice(LEGACY_SKETCH_MARKER);

    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[76..78].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(
        raw2(legacy_declared_handle_coordinates(&payload, 0)),
        Some([0.045, -0.0225])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind(),
        SketchInputKind::Point
    );

    payload[96..98].fill(0);
    assert_eq!(
        raw2(legacy_declared_handle_coordinates(&payload, 0)),
        Some([0.045, -0.0225])
    );
    payload[17..21].fill(0);
    payload[96..98].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        raw2(legacy_declared_handle_coordinates(&payload, 0)),
        Some([0.045, -0.0225])
    );
    payload[96..98].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(
        raw2(legacy_declared_handle_coordinates(&payload, 0)),
        Some([0.045, -0.0225])
    );
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[76..78].copy_from_slice(&3u16.to_le_bytes());
    payload[96..98].fill(0);
    assert_eq!(
        raw2(legacy_declared_handle_coordinates(&payload, 0)),
        Some([0.045, -0.0225])
    );
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[76..78].copy_from_slice(&2u16.to_le_bytes());
    payload[96..98].copy_from_slice(&3u16.to_le_bytes());
    assert_eq!(
        raw2(legacy_declared_handle_coordinates(&payload, 0)),
        Some([0.045, -0.0225])
    );
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[96..98].fill(0);
    assert_eq!(
        raw2(legacy_declared_handle_coordinates(&payload, 0)),
        Some([0.045, -0.0225])
    );
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[162..166].copy_from_slice(&3u32.to_le_bytes());
    payload[166..170].copy_from_slice(&3u32.to_le_bytes());
    assert_eq!(
        raw2(legacy_declared_handle_coordinates(&payload, 0)),
        Some([0.045, -0.0225])
    );
    payload[166..170].copy_from_slice(&4u32.to_le_bytes());
    assert_eq!(
        raw2(legacy_declared_handle_coordinates(&payload, 0)),
        Some([0.045, -0.0225])
    );
    payload[162..170].fill(0);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload.resize(177 + LEGACY_SKETCH_MARKER.len(), 0);
    payload[96..177].fill(0);
    payload[96..108].copy_from_slice(&[
        0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01, 0x00, 0x0b, 0x00,
    ]);
    payload[108..119].copy_from_slice(b"sgArcHandle");
    payload[119..121].copy_from_slice(&3u16.to_le_bytes());
    payload[121..125].fill(0xff);
    payload[125..131].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    payload[173..177].copy_from_slice(&2u32.to_le_bytes());
    payload[177..].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert_eq!(
        raw2(legacy_declared_handle_coordinates(&payload, 0)),
        Some([0.045, -0.0225])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind(),
        SketchInputKind::Point
    );
    let mut padded_handle = payload.clone();
    padded_handle.resize(185 + LEGACY_SKETCH_MARKER.len(), 0);
    padded_handle[96..98].copy_from_slice(&1u16.to_le_bytes());
    padded_handle[98..185].fill(0);
    padded_handle[98..106].copy_from_slice(&[0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00]);
    padded_handle[106..112].copy_from_slice(&[0xff, 0xff, 0x01, 0x00, 0x0b, 0x00]);
    padded_handle[112..123].copy_from_slice(b"sgArcHandle");
    padded_handle[125..129].fill(0xff);
    padded_handle[133..139].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    padded_handle[181..185].copy_from_slice(&5u32.to_le_bytes());
    padded_handle[185..].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert_eq!(
        raw2(legacy_declared_handle_coordinates(&padded_handle, 0)),
        Some([0.045, -0.0225])
    );
    padded_handle[181..185].fill(0);
    assert_eq!(
        raw2(legacy_declared_handle_coordinates(&padded_handle, 0)),
        None
    );
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        raw2(legacy_declared_handle_coordinates(&payload, 0)),
        Some([0.045, -0.0225])
    );
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    assert_eq!(
        raw2(legacy_declared_handle_coordinates(&payload, 0)),
        Some([0.045, -0.0225])
    );
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[17..21].fill(0);
    payload[96..98].copy_from_slice(&3u16.to_le_bytes());
    payload[119..121].fill(0);
    assert_eq!(
        raw2(legacy_declared_handle_coordinates(&payload, 0)),
        Some([0.045, -0.0225])
    );
    payload[96..98].fill(0);
    assert_eq!(raw2(legacy_declared_handle_coordinates(&payload, 0)), None);
    payload[119..121].fill(0xff);
    assert_eq!(raw2(legacy_declared_handle_coordinates(&payload, 0)), None);
    payload[96..98].copy_from_slice(&3u16.to_le_bytes());
    payload[119..121].fill(0);
    payload[84] = b'x';
    assert_eq!(raw2(legacy_declared_handle_coordinates(&payload, 0)), None);
}
