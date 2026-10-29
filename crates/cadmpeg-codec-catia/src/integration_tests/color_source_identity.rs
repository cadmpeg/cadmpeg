// SPDX-License-Identifier: Apache-2.0
use crate::test_support::test_bytes::be32;
use crate::test_support::test_container::standard_catpart;
use crate::test_support::test_object_graph::{catalog_stream, value_block_stream};

#[test]
fn equal_value_packet_colors_keep_actual_native_identity_through_public_decode() {
    let mut stream = value_block_stream(&[
        0x8e, 0xeb, 0x84, 1, 0x10, 0x20, 0x30, 0x8e, 0xeb, 0x84, 1, 0x10, 0x20, 0x30,
    ]);
    stream.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "VPGlobal",
    ]));
    let mut bytes = standard_catpart();
    bytes.splice(16..16, stream);
    let file_len = u32::try_from(bytes.len()).unwrap();
    bytes[8..12].copy_from_slice(&be32(file_len));
    let decoded = super::decode(bytes);
    let namespace = decoded.ir().native.namespace("catia").unwrap();
    let display_colors = crate::test_support::with_service_context(|ctx| {
        namespace
            .arena_iter_as_for_decode::<crate::native::CatiaDisplayColorRecord>(
                ctx,
                "display_colors",
            )
            .collect::<Result<Vec<_>, _>>()
    })
    .unwrap();
    let packets: Vec<_> = display_colors
        .iter()
        .filter(|source| source.source_kind == "value-packet")
        .collect();
    assert_eq!(packets.len(), 2);
    assert_ne!(packets[0].id, packets[1].id);
    assert_ne!(packets[0].source_record, packets[1].source_record);
    assert_eq!(decoded.ir().model.appearances.len(), display_colors.len());
    let mut packet_ids = std::collections::BTreeSet::new();
    for source in packets {
        let appearances: Vec<_> = decoded
            .ir()
            .model
            .appearances
            .iter()
            .filter(|appearance| appearance.native_ref.as_deref() == Some(source.id.as_str()))
            .collect();
        assert_eq!(appearances.len(), 1);
        assert_eq!(
            appearances[0].base_color,
            Some(cadmpeg_ir::topology::Color::from_rgba8(
                0x10, 0x20, 0x30, 0xff
            ))
        );
        packet_ids.insert(appearances[0].id.clone());
    }
    assert_eq!(packet_ids.len(), 2);
    for source in &display_colors {
        assert_eq!(
            decoded
                .ir()
                .model
                .appearances
                .iter()
                .filter(|appearance| appearance.native_ref.as_deref() == Some(source.id.as_str()))
                .count(),
            1
        );
    }
    for record in &namespace.arenas()["display_colors"] {
        assert!(!record.fields().unwrap().contains_key("appearance"));
    }
    for binding in &decoded.ir().model.appearance_bindings {
        assert!(decoded
            .ir()
            .model
            .appearances
            .iter()
            .any(|appearance| appearance.id == binding.appearance));
    }
    assert!(decoded
        .ir()
        .model
        .faces
        .iter()
        .all(|face| face.color.is_none()));
    assert!(decoded
        .ir()
        .model
        .bodies
        .iter()
        .all(|body| body.color.is_none()));
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
        .unwrap()
        .is_ok());
    let baseline_findings = crate::test_support::with_service_context(|ctx| {
        <crate::CatiaCodec as cadmpeg_ir::codec::CodecBackend>::validate_native(
            ctx,
            decoded.ir(),
            None,
        )
    })
    .unwrap();
    assert!(baseline_findings.is_empty());
    let mut duplicated = decoded.ir().clone();
    duplicated.model.appearances[1].native_ref = duplicated.model.appearances[0].native_ref.clone();
    let findings = crate::test_support::with_service_context(|ctx| {
        <crate::CatiaCodec as cadmpeg_ir::codec::CodecBackend>::validate_native(
            ctx,
            &duplicated,
            None,
        )
    })
    .unwrap();
    assert!(findings.iter().any(|finding| finding.entity.as_deref()
        == duplicated.model.appearances[0].native_ref.as_deref()));
}
