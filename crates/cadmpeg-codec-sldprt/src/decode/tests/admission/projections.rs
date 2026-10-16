// SPDX-License-Identifier: Apache-2.0
//! Decode projection resource admission tests.

use crate::test_support::container::make_block;
use crate::test_support::container::outer_header;
use crate::SldprtCodec;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn metadata_parameter_expression_refuses_nesting_limit() {
    let mut source = outer_header();
    source.extend(make_block(
        0x43, "Contents/Keywords",
        br#"<Keywords><Feature Name="Equations" Type="EquationDriven" id="10"><Dimension Name="A">+ + + + + + + + + + + + 1</Dimension></Feature></Keywords>"#,
    ));
    let mut options = DecodeOptions {
        depth: cadmpeg_core::decode::DecodeDepth::Container,
        ..DecodeOptions::default()
    };
    options.policy.limits.max_recursion_depth = 8;
    let error = SldprtCodec
        .decode(&mut Cursor::new(&source), &options)
        .expect_err("recursive parameter expression exceeds the nesting limit");
    assert!(matches!(error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RecursionDepth
                && limit.operation == "parse SLDPRT parameter unary"
    ));
}
