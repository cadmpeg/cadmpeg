use crate::records::{SketchInputEntity, SketchInputKind};
use crate::resolved_features::markers::current_reverse_incidence_endpoint_offsets;
use crate::resolved_features::SKETCH_MARKER;

#[test]
fn current_indexed_line_uses_its_unique_reverse_incidence_pair() {
    let first = 84;
    let second = first + 154;
    let end = second + 154;
    let mut payload = vec![0; end + SKETCH_MARKER.len()];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..60].copy_from_slice(&[2, 0, 5, 0]);
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());

    for (offset, coordinates, other) in [
        (first, [1.0f64, 2.0], 11u16),
        (second, [3.0f64, 4.0], 12u16),
    ] {
        payload[offset..offset + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
        payload[offset + 5..offset + 13].fill(0xff);
        payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[offset + 23..offset + 29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
        payload[offset + 31..offset + 39]
            .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
        payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
        payload[offset + 56..offset + 58].copy_from_slice(&[0x1e, 0x00]);
        payload[offset + 58..offset + 66].copy_from_slice(&coordinates[0].to_le_bytes());
        payload[offset + 66..offset + 74].copy_from_slice(&coordinates[1].to_le_bytes());
        payload[offset + 76..offset + 78].copy_from_slice(&2u16.to_le_bytes());
        for (start, selector, id) in [(78, 0x8178u16, 7u16), (90, 0x8132u16, other)] {
            payload[offset + start..offset + start + 2].copy_from_slice(&selector.to_le_bytes());
            payload[offset + start + 2..offset + start + 4].copy_from_slice(&id.to_le_bytes());
            payload[offset + start + 4..offset + start + 8].fill(0xff);
        }
        payload[offset + 102..offset + 108].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    }
    payload[end..].copy_from_slice(SKETCH_MARKER);
    let entity = |id: &str, offset, object_index| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker = SketchInputEntity::new(
            marker_id,
            marker_parent,
            0,
            offset,
            SketchInputKind::LineOrCircle,
        );
        constructed_marker.feature_ref = Some("profile".into());
        constructed_marker = constructed_marker.with_test_identity(object_index, None);
        constructed_marker.state_value = cadmpeg_ir::scalar::FiniteReal::new(1.0);
        constructed_marker.coordinates_m = None;
        constructed_marker.links = None;
        constructed_marker
    };
    let entities = [
        entity("curve", 0, Some(7)),
        entity(
            "first",
            cadmpeg_core::decode::u64_from_index(first),
            Some(20),
        ),
        entity(
            "second",
            cadmpeg_core::decode::u64_from_index(second),
            Some(21),
        ),
    ];
    let markers = entities.iter().collect::<Vec<_>>();

    assert_eq!(
        current_reverse_incidence_endpoint_offsets(
            &cadmpeg_test_support::service_decode_context(),
            &payload,
            &entities[0],
            &markers
        )
        .unwrap(),
        Some([
            cadmpeg_core::decode::u64_from_index(first),
            cadmpeg_core::decode::u64_from_index(second)
        ])
    );
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let cache = std::cell::OnceCell::new();
    assert_eq!(
        crate::resolved_features::markers::current_reverse_incidence_endpoint_offsets_cached(
            &ctx,
            &payload,
            &entities[1],
            &markers,
            &cache
        )
        .unwrap(),
        None
    );
    assert!(cache.get().is_none());
    let expected =
        current_reverse_incidence_endpoint_offsets(&ctx, &payload, &entities[0], &markers).unwrap();
    assert_eq!(
        crate::resolved_features::markers::current_reverse_incidence_endpoint_offsets_cached(
            &ctx,
            &payload,
            &entities[0],
            &markers,
            &cache
        )
        .unwrap(),
        expected
    );
    {
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "index SLDPRT reverse incidence endpoints",
            None,
        );
        assert_eq!(
            crate::resolved_features::markers::current_reverse_incidence_endpoint_offsets_cached(
                &ctx,
                &payload,
                &entities[0],
                &markers,
                &cache
            )
            .unwrap(),
            expected
        );
    }
    let curves =
        crate::resolved_features::typed_relations::CurveMarkers::new(&ctx, &markers).unwrap();
    assert_eq!(
        curves
            .reverse_endpoint_offsets(&ctx, &payload, &entities[0])
            .unwrap(),
        expected
    );
    {
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "index SLDPRT reverse incidence endpoints",
            None,
        );
        assert_eq!(
            curves
                .reverse_endpoint_offsets(&ctx, &payload, &entities[0])
                .unwrap(),
            expected
        );
    }
    let duplicated = [&entities[2], &entities[1], &entities[1], &entities[0]];
    let (index, _storage) =
        crate::resolved_features::markers::ReverseIncidenceIndex::new(&ctx, &payload, &duplicated)
            .unwrap();
    assert_eq!(
        crate::resolved_features::markers::current_reverse_incidence_endpoint_offsets_in(
            &ctx,
            &payload,
            &entities[0],
            &index
        )
        .unwrap(),
        Some([
            cadmpeg_core::decode::u64_from_index(first),
            cadmpeg_core::decode::u64_from_index(second)
        ])
    );
    let mut foreign = entities[0].clone();
    foreign.feature_ref = Some("other".into());
    assert_eq!(
        crate::resolved_features::markers::current_reverse_incidence_endpoint_offsets_in(
            &ctx, &payload, &foreign, &index
        )
        .unwrap(),
        None
    );
    crate::test_support::work_refusal_at("index SLDPRT reverse incidence endpoints", |ctx| {
        crate::resolved_features::markers::ReverseIncidenceIndex::new(ctx, &payload, &markers)
            .map(|_| ())
    });
    for offset in [first, second] {
        payload[offset + 92..offset + 94].copy_from_slice(&7u16.to_le_bytes());
    }
    let (ambiguous, _storage) =
        crate::resolved_features::markers::ReverseIncidenceIndex::new(&ctx, &payload, &markers)
            .unwrap();
    assert_eq!(
        crate::resolved_features::markers::current_reverse_incidence_endpoint_offsets_in(
            &ctx,
            &payload,
            &entities[0],
            &ambiguous
        )
        .unwrap(),
        None
    );
}
