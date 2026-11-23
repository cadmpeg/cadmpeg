// SPDX-License-Identifier: Apache-2.0
//! A generated part carries the native skeleton: its object streams walk to
//! their end, and its default tree and stored body decode as a native part's
//! do.

use std::io::Cursor;

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

use crate::SldprtCodec;

fn written_cube() -> Vec<u8> {
    let ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("the cube plans");
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("the cube writes");
    bytes
}

#[test]
fn generated_part_object_streams_walk_to_their_end() {
    let bytes = written_cube();
    let scan = crate::test_support::container::scan(&bytes);
    let payload = |name: &str| {
        scan.sections()
            .find(|section| section.name() == Some(name))
            .map_or_else(
                || panic!("{name} is written"),
                crate::container::Section::payload,
            )
    };
    let config = payload("Contents/Config-0");
    let resolved = payload("Contents/Config-0-ResolvedFeatures");
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let version =
        crate::carchive::walk_version(crate::container::declared_sw_version(&scan), config);
    let count = crate::archive::resolved_count(resolved).expect("map count");
    let map = crate::archive::class_index_map(config, count, &ctx)
        .expect("class map")
        .expect("the Config-0 map reaches the ResolvedFeatures count");
    let walk = crate::carchive::walk_configuration(&ctx, &map, resolved, version)
        .expect("walk")
        .expect("the Config-0 class map reaches the ResolvedFeatures count");
    assert_eq!(walk.status, crate::carchive::Status::End);
    for stream in [
        "swXmlContents/KeyWords",
        "swXmlContents/Features",
        "Contents/OleItems",
        "Contents/eModelLic",
    ] {
        payload(stream);
    }
    assert!(scan
        .sections()
        .all(|section| section.name() != Some("Contents/SolidWorks")));
}

#[test]
fn generated_part_decodes_its_default_tree_and_stored_body() {
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(written_cube()), &DecodeOptions::default())
        .expect("the written cube decodes");
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| loss.code.local_code() != "feature.input-archive-walk-stopped"));
    let features = &decoded.ir().model.features;
    let named = |name: &str| {
        features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("{name} decodes"))
    };
    for plane in ["Front Plane", "Top Plane", "Right Plane", "Origin"] {
        named(plane);
    }
    assert!(matches!(
        named("Imported1").evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::StoredGeometry {})
    ));
}
