// SPDX-License-Identifier: Apache-2.0
//! The document tolerances an NX part states: the Parasolid modeller
//! precision of its bodies.

use std::io::Cursor;

use cadmpeg_core::decode::DecodeDepth;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::loss::NxLossCode;
use crate::test_support::test_prt::{prt_with_partition, topology_part_prt};
use crate::test_support::test_streams::primary_stopped_topology_stream;
use crate::NxCodec;

const MODELLER_DEFAULT_LINEAR_MM: f64 = 1.0e-5;
const MODELLER_ANGULAR_RADIANS: f64 = 1.0e-11;
const COMPARISON_SLACK: f64 = 1.0e-25;

fn decode(file: Vec<u8>, depth: DecodeDepth) -> cadmpeg_ir::codec::DecodeResult {
    let options = DecodeOptions {
        depth,
        ..DecodeOptions::default()
    };
    NxCodec
        .decode(&mut Cursor::new(file), &options)
        .expect("synthetic part decodes")
}

fn tolerances(depth: DecodeDepth) -> (usize, f64, f64) {
    let decoded = decode(topology_part_prt(), depth);
    let ir = decoded.ir();
    (
        ir.geometry.bodies.len(),
        ir.tolerances.linear.get(),
        ir.tolerances.angular.get(),
    )
}

/// The messages of the losses `decoded` charges under `code`.
fn messages(decoded: &cadmpeg_ir::codec::DecodeResult, code: NxLossCode) -> Vec<String> {
    let kind = code.kind();
    decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == kind)
        .map(|loss| loss.message.clone())
        .collect()
}

#[test]
fn a_body_whose_resolution_is_unread_states_the_modeller_default_and_a_loss() {
    let decoded = decode(topology_part_prt(), DecodeOptions::default().depth);
    let ir = decoded.ir();
    let [body] = ir.geometry.bodies.as_slice() else {
        panic!("one body: {:?}", ir.geometry.bodies.len());
    };
    assert!((ir.tolerances.linear.get() - MODELLER_DEFAULT_LINEAR_MM).abs() <= COMPARISON_SLACK);
    assert!((ir.tolerances.angular.get() - MODELLER_ANGULAR_RADIANS).abs() <= COMPARISON_SLACK);
    let unread = messages(&decoded, NxLossCode::BodyResolutionUnread);
    assert_eq!(unread.len(), 1, "{unread:?}");
    assert!(unread[0].contains(body.id.as_str()), "{unread:?}");
}

#[test]
fn a_body_states_its_stored_resolution_in_millimetres() {
    let decoded = decode(
        prt_with_partition(&primary_stopped_topology_stream(1.0e-6)),
        DecodeOptions::default().depth,
    );
    let ir = decoded.ir();
    assert_eq!(ir.geometry.bodies.len(), 1);
    assert_eq!(
        Some(ir.tolerances.linear),
        cadmpeg_parasolid::precision::linear_precision(1.0e-6)
    );
    assert!(messages(&decoded, NxLossCode::BodyResolutionUnread).is_empty());
    assert!(messages(&decoded, NxLossCode::BodyResolutionInvalid).is_empty());
}

#[test]
fn a_body_that_stores_no_precision_states_the_modeller_default_and_a_loss() {
    let decoded = decode(
        prt_with_partition(&primary_stopped_topology_stream(-1.0e-6)),
        DecodeOptions::default().depth,
    );
    let ir = decoded.ir();
    let [body] = ir.geometry.bodies.as_slice() else {
        panic!("one body: {:?}", ir.geometry.bodies.len());
    };
    assert!((ir.tolerances.linear.get() - MODELLER_DEFAULT_LINEAR_MM).abs() <= COMPARISON_SLACK);
    let invalid = messages(&decoded, NxLossCode::BodyResolutionInvalid);
    assert_eq!(invalid.len(), 1, "{invalid:?}");
    assert!(invalid[0].contains(body.id.as_str()), "{invalid:?}");
}

#[test]
fn a_container_decode_states_the_modeller_precision() {
    let (bodies, linear, angular) = tolerances(DecodeDepth::Container);
    assert_eq!(bodies, 0);
    assert!((linear - MODELLER_DEFAULT_LINEAR_MM).abs() <= COMPARISON_SLACK);
    assert!((angular - MODELLER_ANGULAR_RADIANS).abs() <= COMPARISON_SLACK);
}
