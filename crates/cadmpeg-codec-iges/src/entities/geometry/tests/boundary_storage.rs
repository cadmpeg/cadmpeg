// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use crate::IgesCodec;
use crate::test_support::test_surface_fixtures::bounded_plane_file;
use std::io::Cursor;

fn assert_trimming_retained_refusal(bytes: &[u8], operation: &str) {
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        operation,
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            IgesCodec
                .decode(
                    &mut Cursor::new(bytes),
                    &DecodeOptions {
                        policy,
                        ..DecodeOptions::default()
                    },
                )
                .map(|_| ())
                .map_err(|error| match error {
                    cadmpeg_ir::codec::DecodeFailure::Codec(error) => error,
                    other => panic!("unexpected decode refusal: {other:?}"),
                })
        },
    );
}

#[test]
fn trimming_projection_refuses_retained_boundary_source_text() {
    let bytes = bounded_plane_file();
    for operation in [
        "iges boundary derivation edge text",
        "iges boundary derivation source text",
    ] {
        assert_trimming_retained_refusal(&bytes, operation);
    }
}
