// SPDX-License-Identifier: Apache-2.0

#[test]
fn compact_cycle_projection_orders_closed_cycle() {
    let (history, mut lane, feature) =
        super::detached_legacy_sketch_tests::compact_profile_projection_fixture();
    lane.native_payload.extend(b"moSketchRegion_c");
    lane.native_payload.extend(0x8060u16.to_le_bytes());
    lane.native_payload.extend(4u16.to_le_bytes());
    // These paired region and chain addresses form four edges in a closed cycle.
    for address in [2u16, 1, 4, 3] {
        lane.native_payload.extend(0x80e1u16.to_le_bytes());
        lane.native_payload.extend(address.to_le_bytes());
        lane.native_payload.extend([0xff; 4]);
        lane.native_payload.extend([0; 4]);
    }
    lane.classes.push(crate::records::FeatureInputClass {
        id: "line-class".into(),
        parent: lane.id.clone(),
        ordinal: 0,
        offset: 0,
        name: "sgLineHandle".into(),
    });
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &lane.native_payload,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let mut features = [feature];
    let mut sketches = Vec::new();
    let mut entities = Vec::new();
    let mut losses = Vec::new();
    super::project_compact_sketch_profiles(
        &ctx,
        &mut features,
        &mut sketches,
        &mut entities,
        std::slice::from_ref(&history),
        std::slice::from_ref(&lane),
        &mut losses,
    )
    .unwrap();
    let expected = (features, sketches, entities, losses);
    assert_eq!(expected.1.len(), 1);
    assert_eq!(expected.2.len(), 4);
    assert_eq!(expected.1[0].profiles[0].len(), 4);
    assert!(expected.3.is_empty());
    assert_eq!(
        expected.1[0].profiles[0]
            .iter()
            .map(|use_| use_.entity.as_str())
            .collect::<Vec<_>>(),
        [
            "sldprt:model:sketch-entity#compact:1:30:0",
            "sldprt:model:sketch-entity#compact:1:30:3",
            "sldprt:model:sketch-entity#compact:1:30:2",
            "sldprt:model:sketch-entity#compact:1:30:1",
        ]
    );
}
