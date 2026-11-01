// SPDX-License-Identifier: Apache-2.0
//! Configuration sketch states bind the profile bodies of a whole archive
//! walk (spec §2.3).

use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, SketchFeatureBinding};
use cadmpeg_ir::CadIr;

use crate::history::hash::configuration_feature_state_hash;
use crate::history::write::configurations::prepare_configurations_for_write;
use crate::resolved_features::profiles::ExactProfiles;
use crate::test_support::container::{make_block, scan};
use crate::test_support::history::sldprt_with_nested_sketch_profiles;
use crate::test_support::native::sldprt_native;
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;

/// A decoded part whose lane holds two nested sketch streams after the name of
/// `Sketch1`, so its interval holds both, and a profile body held by
/// `Sketch1`'s object that binds the second.
fn decoded() -> (CadIr, cadmpeg_ir::Annotations, ExactProfiles) {
    let mut file = sldprt_with_nested_sketch_profiles(&triangle_body(), 2);
    file.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Default" id="0" SourceIndex="0"/><Sketch Name="Sketch1" Type="Sketch" id="0"/></Keywords>"#,
    ));
    let stream = scan(&file)
        .sections()
        .find(|section| {
            section
                .name()
                .is_some_and(|name| name.ends_with("ResolvedFeatures"))
        })
        .and_then(|section| section.ps_streams().get(1).map(|stream| stream.offset))
        .unwrap();
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    let (ir, _, fidelity) = decoded.into_parts();
    let native = sldprt_native(&ir);
    let lane = &native.feature_input_lanes[0];
    let name = lane
        .names
        .iter()
        .find(|name| name.value == "Sketch1")
        .unwrap();
    let start = usize::try_from(name.offset).unwrap();
    let mut exact = ExactProfiles::default();
    exact.insert(
        &lane.id,
        vec![crate::carchive::SketchProfile {
            stream,
            owner_start: start,
            owner_end: start + 1,
        }],
    );
    (ir, fidelity.annotations, exact)
}

/// `ir` with its configuration sketch states projected with `exact`.
fn projected(ir: &CadIr, annotations: &cadmpeg_ir::Annotations, exact: &ExactProfiles) -> CadIr {
    let native = sldprt_native(ir);
    let bytes = native
        .feature_input_lanes
        .iter()
        .flat_map(|lane| lane.native_payload.iter().copied())
        .collect::<Vec<_>>();
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let mut projected = ir.clone();
    super::project_configuration_sketch_states(
        &ctx,
        &mut projected,
        &native.feature_histories,
        &native.feature_input_lanes,
        exact,
        &mut annotations.clone(),
    )
    .unwrap();
    projected
}

fn sketch1_binding(ir: &CadIr) -> Option<cadmpeg_ir::sketches::SketchId> {
    let feature = ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .unwrap();
    match &ir.model.configurations[0].feature_states[&feature.id].definition {
        FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: SketchFeatureBinding::Planar(sketch),
        }) => sketch.clone(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_configuration_binds_the_profile_body_the_interval_rule_leaves_unbound() {
    let (ir, annotations, exact) = decoded();
    assert_eq!(
        sketch1_binding(&projected(&ir, &annotations, &ExactProfiles::default())),
        None
    );
    let bound = sketch1_binding(&projected(&ir, &annotations, &exact)).unwrap();
    assert!(bound.as_str().ends_with(":1:0"), "{bound:?}");
}

#[test]
fn the_writer_projects_configuration_states_with_the_decoded_profile_bodies() {
    let (ir, annotations, exact) = decoded();
    let mut projected = projected(&ir, &annotations, &exact);
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let hash = configuration_feature_state_hash(&ctx, &projected.model.configurations).unwrap();
    let attributes = &mut projected.source.as_mut().unwrap().attributes;
    attributes.insert(
        cadmpeg_core::nonblank_literal!("sldprt_configuration_feature_states_local_sha256"),
        hash,
    );
    attributes.insert(
        cadmpeg_core::nonblank_literal!("sldprt_configuration_parameter_values_local_sha256"),
        "stale".into(),
    );
    let write = |ir: &CadIr, profiles: Option<ExactProfiles>| {
        prepare_configurations_for_write(ir, &mut Some(sldprt_native(ir)), &annotations, |_| {
            Ok(profiles)
        })
    };
    write(&projected, Some(exact.clone())).unwrap();

    // A configuration value edited against a changed native state.
    let mut edited = projected.clone();
    let value = edited.model.configurations[0]
        .parameter_values
        .values_mut()
        .next()
        .unwrap();
    *value = cadmpeg_ir::features::ParameterValue::Length(
        cadmpeg_ir::scalar::Length::new(30.0).unwrap(),
    );
    assert!(matches!(
        write(&edited, None),
        Err(cadmpeg_core::CodecError::NotImplemented(message))
            if message.contains("retained source image")
    ));
    assert!(matches!(
        write(&edited, Some(exact)),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));
}
