// SPDX-License-Identifier: Apache-2.0
//! Declaration owners keep exact positions independently of repeated names.

use super::capture;
use crate::records::FeatureInputLane;
use crate::resolved_features::body_sources::BodySources;
use crate::resolved_features::class_sources::ClassSources;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::SourceFidelity;

fn payload() -> Vec<u8> {
    crate::test_support::history::resolved_features_payload_with_names_relation_and_scalar(
        &[],
        &["D1"],
        "sgPntPntDist",
        0.025,
    )
}

fn lane(
    ctx: &DecodeContext<'_>,
    source: cadmpeg_core::decode::View<'_>,
) -> (FeatureInputLane, ClassSources) {
    let payload = source.window();
    let id = "lane";
    let classes = crate::resolved_features::names::class_declarations(payload, id).unwrap();
    let class_sources = ClassSources::from_source(ctx, source, &classes).unwrap();
    let names = crate::resolved_features::names::object_names(ctx, payload, id).unwrap();
    let scalars = crate::resolved_features::scalars::named_scalars(payload, id, &names)
        .unwrap()
        .records;
    let relation_bindings = crate::resolved_features::markers::relation_bindings(
        &class_sources,
        id,
        &classes,
        &scalars,
        &[],
    )
    .unwrap();
    let lane = FeatureInputLane {
        id: id.into(),
        configuration: None,
        native_payload: payload.to_vec().into(),
        classes,
        names,
        scalars,
        relation_bindings,
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };
    (lane, class_sources)
}

#[test]
fn declaration_pointer_is_lane_relative_and_reads_only_its_actual_frame() {
    let payload = payload();
    let mut bytes = vec![0xa5; 37];
    bytes.extend(&payload);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let source = root.child(37, bytes.len()).unwrap();
    let (mut lane, class_sources) = lane(&ctx, source);
    let mut sources = BodySources::from_views([(lane.id.clone(), source)]).unwrap();
    crate::resolved_features::bindings::finalize_lane_bindings(&ctx, &[], &mut lane, &mut sources)
        .unwrap();
    let [binding] = lane.relation_bindings.as_slice() else {
        panic!("one declaration binding")
    };
    let declaration = lane
        .classes
        .iter()
        .find(|class| class.id == binding.class_ref)
        .unwrap();
    let offset = class_sources.position(declaration).unwrap();
    let len = u64::try_from(6 + declaration.name.len()).unwrap();
    assert_eq!(binding.sequence, declaration.sequence);
    assert_eq!(binding.scalar_ref, lane.scalars[0].id);
    assert_eq!(lane.scalars[0].value.get(), 0.025);
    let wire = serde_json::to_value(binding).unwrap();
    assert!(wire.get("offset").is_none());
    let restored: crate::records::FeatureInputRelationBinding =
        serde_json::from_value(wire).unwrap();
    assert_eq!(&restored, binding);
    let id = binding.id.clone();
    // The supplemental finalization path must not open a duplicate scope.
    crate::resolved_features::bindings::finalize_lane_bindings(&ctx, &[], &mut lane, &mut sources)
        .unwrap();
    let mut fidelity = SourceFidelity::default();
    crate::pmi::layout::install(&ctx, sources.into_pointers(&ctx).unwrap(), &mut fidelity).unwrap();
    assert_eq!(fidelity.record_layout().len(), 1);
    let layout = &fidelity.record_layout()[&id];
    assert_eq!(layout.stream(), "file");
    assert_eq!((layout.offset(), layout.byte_len()), (37 + offset, len));
    assert_eq!(layout.pointers()["offset"], offset);
    assert_eq!(layout.reads()["file"], [37 + offset..37 + offset + len]);
    let restored: SourceFidelity =
        serde_json::from_value(serde_json::to_value(&fidelity).unwrap()).unwrap();
    assert_eq!(restored.record_layout(), fidelity.record_layout());
}

#[test]
fn repeated_declaration_names_keep_distinct_stable_class_locators() {
    let mut bytes = payload();
    bytes.extend(payload());
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let (lane, class_sources) = lane(&ctx, root);
    assert_eq!(lane.relation_bindings.len(), 2);
    let mut sources = BodySources::from_views([(lane.id.clone(), root)]).unwrap();
    capture(&ctx, &lane, &mut sources).unwrap();
    let mut fidelity = SourceFidelity::default();
    crate::pmi::layout::install(&ctx, sources.into_pointers(&ctx).unwrap(), &mut fidelity).unwrap();
    let layouts = lane
        .relation_bindings
        .iter()
        .map(|binding| {
            let class = lane
                .classes
                .iter()
                .find(|class| class.id == binding.class_ref)
                .unwrap();
            assert_eq!(class.name, "sgPntPntDist");
            let layout = &fidelity.record_layout()[&binding.id];
            assert_eq!(
                layout.pointers()["offset"],
                class_sources.position(class).unwrap()
            );
            layout
        })
        .collect::<Vec<_>>();
    assert_ne!(layouts[0].offset(), layouts[1].offset());
}

#[test]
fn missing_declaration_identity_does_not_fall_back_to_a_matching_name() {
    let bytes = payload();
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let (mut lane, _class_sources) = lane(&ctx, root);
    assert_eq!(lane.relation_bindings.len(), 1);
    lane.relation_bindings[0].class_ref = "absent-class".into();
    let mut sources = BodySources::from_views([(lane.id.clone(), root)]).unwrap();
    assert!(capture(&ctx, &lane, &mut sources).is_err());
    assert!(sources.pointers().is_empty());
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn declaration_family_mismatch_rolls_back_its_capture() {
    let bytes = payload();
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let (mut lane, _class_sources) = lane(&ctx, root);
    assert_eq!(lane.relation_bindings.len(), 1);
    lane.relation_bindings[0].family = crate::records::FeatureInputRelationFamily::LineLineDistance;
    let mut sources = BodySources::from_views([(lane.id.clone(), root)]).unwrap();
    assert!(capture(&ctx, &lane, &mut sources).is_err());
    assert!(sources.pointers().is_empty());
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn foreign_declaration_view_never_creates_a_record_scope() {
    let bytes = payload();
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let foreign_arena = DecodeArena::new();
    let (_foreign_ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &foreign_arena, &DecodePolicy::service()).unwrap();
    let (lane, _class_sources) = lane(&_foreign_ctx, root);
    assert_eq!(lane.relation_bindings.len(), 1);
    let mut sources = BodySources::from_views([(lane.id.clone(), root)]).unwrap();
    assert!(capture(&ctx, &lane, &mut sources).is_err());
    assert!(sources.pointers().is_empty());
    assert!(ctx.take_record_captures().unwrap().is_empty());
}
