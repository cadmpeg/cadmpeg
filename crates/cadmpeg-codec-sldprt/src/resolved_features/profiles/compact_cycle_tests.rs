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

/// The configuration pass reads the compact records again after the main
/// pass replaced the sketch they gave with the one read from the feature's
/// marker records. It binds that sketch and leaves no compact sketch beside
/// it.
#[test]
fn the_configuration_pass_reads_no_compact_sketch_beside_its_marker_sketch() {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, SketchFeatureBinding};
    use cadmpeg_ir::sketches::{Sketch, SketchId, SketchPlacement, SketchProfiles};

    let (history, lane, feature) =
        super::detached_legacy_sketch_tests::compact_profile_projection_fixture();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &lane.native_payload,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let pass = |sketches: &mut Vec<Sketch>| {
        let mut features = [feature.clone()];
        let mut entities = Vec::new();
        let mut losses = Vec::new();
        super::project_compact_sketch_profiles(
            &ctx,
            &mut features,
            sketches,
            &mut entities,
            std::slice::from_ref(&history),
            std::slice::from_ref(&lane),
            &mut losses,
        )
        .unwrap();
        let [feature] = features;
        (feature, entities, losses)
    };

    let mut main = Vec::new();
    let (_, entities, _) = pass(&mut main);
    assert_eq!(
        main.iter()
            .map(|sketch| sketch.id.as_str())
            .collect::<Vec<_>>(),
        ["sldprt:model:sketch#compact:1:30"]
    );
    assert_eq!(entities.len(), 4);

    let markers = SketchId::mint("sldprt:model:sketch#markers:1:30").unwrap();
    let mut sketches = vec![Sketch {
        id: markers.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved {},
        profiles: SketchProfiles::default(),
        native_ref: None,
    }];
    let (feature, entities, losses) = pass(&mut sketches);
    assert_eq!(
        sketches.iter().map(|sketch| &sketch.id).collect::<Vec<_>>(),
        [&markers]
    );
    assert!(entities.is_empty());
    assert!(losses.is_empty());
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: SketchFeatureBinding::Planar(Some(id)),
        }) if *id == markers
    ));
}
