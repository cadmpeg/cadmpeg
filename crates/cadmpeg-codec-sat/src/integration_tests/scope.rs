// SPDX-License-Identifier: Apache-2.0
use std::io::Cursor;

use cadmpeg_core::decode::DecodeDepth;
use cadmpeg_ir::{Codec, DecodeOptions};

use crate::test_support::test_streams::{
    acis_text_sphere_stream, binary_sphere_stream, text_sphere_stream, BinaryFixtureKind,
};
use crate::SatCodec;

#[test]
fn sat_scope_follows_real_framing_and_the_requested_design_class_for_every_encoding() {
    for bytes in [
        text_sphere_stream(1.0),
        acis_text_sphere_stream(21_800),
        binary_sphere_stream(BinaryFixtureKind::Asm),
        binary_sphere_stream(BinaryFixtureKind::Acis),
    ] {
        for depth in [
            DecodeDepth::Container,
            DecodeDepth::Geometry,
            DecodeDepth::Full,
        ] {
            let result = SatCodec
                .decode(
                    &mut Cursor::new(&bytes),
                    &DecodeOptions {
                        depth,
                        ..Default::default()
                    },
                )
                .expect("source stream");
            let scope = result.ir().scope;
            assert_eq!(scope.model, depth != DecodeDepth::Container);
            assert_eq!(scope.design, depth == DecodeDepth::Full);
            assert_eq!(result.report().scope(), scope);
            assert_eq!(
                result.report().geometry_transferred(),
                depth != DecodeDepth::Container
            );
        }
    }
}

#[test]
fn sat_container_request_does_not_create_a_whole_partition_source_home() {
    let bytes = text_sphere_stream(1.0);
    let result = SatCodec
        .decode(
            &mut Cursor::new(&bytes),
            &DecodeOptions {
                depth: DecodeDepth::Container,
                ..Default::default()
            },
        )
        .expect("header-only admission");
    assert!(result
        .source_fidelity()
        .retained_record("sat:source:omitted-partition#0")
        .is_none());
    assert!(result.source_fidelity().retained_records().is_empty());
    assert!(result.source_fidelity().unrepresented().is_empty());
}
