// SPDX-License-Identifier: Apache-2.0
//! Native layer content remains intact behind an inward neutral link.

use crate::chunks::ArchiveVersion;
use crate::test_support::test_dump::{crc_chunk, utf16_bytes};

#[test]
fn layer_projection_preserves_native_values_and_links_inward() {
    let archive = ArchiveVersion::V8;
    let rendering = crc_chunk(archive, 0x4000_8000, &[1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let mut extension = vec![37];
    extension.extend(utf16_bytes("source description"));
    extension.push(0);
    let record = super::layer_record_with_rendering_and_extensions(archive, &rendering, &extension);
    let decoded = super::decode(super::document(archive, record));
    let layers = &decoded.ir().model.presentation_layers;
    assert_eq!(layers.len(), 1);
    let native = &decoded
        .ir()
        .native
        .namespace("rhino")
        .expect("Rhino namespace")
        .arenas()["layers"];
    assert_eq!(native.len(), 1);
    assert_eq!(layers[0].native_ref.as_deref(), Some(native[0].id()));
    assert_eq!(layers[0].name, "layer-witness");
    assert_eq!(layers[0].description.as_deref(), Some("source description"));
    assert_eq!(layers[0].visible, Some(true));
    assert_eq!(
        native[0].field("name"),
        Some(serde_json::json!("layer-witness"))
    );
    assert_eq!(
        native[0].field("description"),
        Some(serde_json::json!("source description"))
    );
    assert_eq!(native[0].field("visible"), Some(serde_json::json!(true)));
    assert_eq!(
        native[0].field("parent_uuid"),
        Some(serde_json::Value::Null)
    );
    assert_eq!(native[0].field("neutral_ref"), None);
    super::assert_valid(&decoded);
}

fn hierarchical_layer(index: i32, id: [u8; 16], parent: [u8; 16]) -> Vec<u8> {
    let archive = ArchiveVersion::V8;
    let mut payload = vec![0x16];
    payload.extend(0_i32.to_le_bytes());
    payload.extend(index.to_le_bytes());
    payload.extend((-1_i32).to_le_bytes());
    payload.extend((-1_i32).to_le_bytes());
    payload.extend(0_i32.to_le_bytes());
    payload.extend([10, 20, 30, 0]);
    payload.extend(0_i16.to_le_bytes());
    payload.extend(0_i16.to_le_bytes());
    payload.extend(0.0_f64.to_le_bytes());
    payload.extend(1.0_f64.to_le_bytes());
    payload.extend(utf16_bytes("hierarchical layer"));
    payload.push(0);
    payload.extend((-1_i32).to_le_bytes());
    payload.extend([0; 4]);
    payload.extend(0.0_f64.to_le_bytes());
    payload.push(0);
    payload.extend(id);
    payload.extend(parent);
    payload.push(1);
    let class = crate::test_support::test_dump::class_wrapper_with_userdata(
        archive,
        super::LAYER_CLASS,
        &payload,
        &[],
    );
    crate::test_support::test_dump::crc_chunk_excluding(
        archive,
        0x2000_8050,
        &class,
        std::slice::from_ref(&(0..class.len())),
    )
}

#[test]
fn layer_parent_native_uuid_is_preserved_after_neutral_parent_projection() {
    let archive = ArchiveVersion::V8;
    let parent = [0x11; 16];
    let child = [0x22; 16];
    let records = [
        hierarchical_layer(0, parent, [0; 16]),
        hierarchical_layer(1, child, parent),
    ];
    let source = crate::test_support::test_dump::minimal_document(
        "80",
        &[
            crate::test_support::test_dump::table(
                archive,
                0x1000_0014,
                &[crate::test_support::test_dump::short_chunk(
                    archive,
                    0xa000_0026,
                    202_608_010,
                )],
            ),
            crate::test_support::test_dump::table(archive, 0x1000_0015, &[]),
            crate::test_support::test_dump::table(archive, 0x1000_0011, &records),
            crate::test_support::test_dump::table(archive, 0x1000_0013, &[]),
        ],
    );
    let decoded = super::decode(source);
    let layers = &decoded.ir().model.presentation_layers;
    let native = &decoded
        .ir()
        .native
        .namespace("rhino")
        .expect("Rhino namespace")
        .arenas()["layers"];
    assert_eq!(layers.len(), 2);
    assert_eq!(native.len(), 2);
    assert_eq!(layers[1].parent.as_ref(), Some(&layers[0].id));
    let parent_uuid = crate::wire::Uuid::from_canonical(parent).to_string();
    assert_eq!(
        native[1].field("parent_uuid"),
        Some(serde_json::json!(parent_uuid))
    );
    for (layer, source) in layers.iter().zip(native) {
        assert_eq!(layer.native_ref.as_deref(), Some(source.id()));
        assert_eq!(layer.name, "hierarchical layer");
        assert_eq!(layer.visible, Some(false));
        assert_eq!(
            source.field("name"),
            Some(serde_json::json!("hierarchical layer"))
        );
        assert_eq!(source.field("visible"), Some(serde_json::json!(false)));
        assert_eq!(source.field("neutral_ref"), None);
    }
    super::assert_valid(&decoded);
}
