// SPDX-License-Identifier: Apache-2.0

use crate::loss::SldprtLossCode;
use crate::test_support::container::{make_block, sldprt_with_body};
use crate::test_support::history::resolved_features_payload_with_names_relation_and_scalar;
use crate::test_support::parasolid::triangle_body;

use super::decode;

/// The name record that opens the scalar `D1`.
const D1_NAME: [u8; 10] = [0x04, 0x80, 0xff, 0xfe, 0xff, 2, b'D', 0, b'1', 0];

/// A part whose sketch `Sketch1` stores the dimension `D1` with an angle
/// relation class, line operands, and a length parameter class, with
/// `opening` before the feature-input lane.
fn refused_dimension_part(opening: &[u8]) -> Vec<u8> {
    let mut payload = resolved_features_payload_with_names_relation_and_scalar(
        &[0, 1, 1, 1],
        &["Sketch1", "D1"],
        "sgAnglDim",
        0.025,
    );
    let name = payload
        .windows(D1_NAME.len())
        .position(|window| window == D1_NAME)
        .expect("the name record of D1");
    for cell in payload[name..]
        .windows(2)
        .enumerate()
        .filter_map(|(at, window)| (window == [0xd6, 0x80]).then_some(name + at))
        .collect::<Vec<_>>()
    {
        payload[cell..cell + 2].copy_from_slice(&[0x86, 0x83]);
    }
    let parameter = "moLengthParameter_c";
    let mut declaration = vec![0xff, 0xff, 0x01, 0x00];
    declaration.extend_from_slice(&u16::try_from(parameter.len()).unwrap().to_le_bytes());
    declaration.extend_from_slice(parameter.as_bytes());
    payload.splice(name..name, declaration);
    let mut lane = opening.to_vec();
    lane.extend(payload);
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &lane,
    ));
    source
}

/// The number of `sketch.dimension-class-unknown` losses in the decode
/// report of `source`.
fn unknown_class_losses(source: Vec<u8>) -> usize {
    let code = SldprtLossCode::SketchDimensionClassUnknown.kind();
    decode(source)
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == code)
        .count()
}

/// In a compound-file lane, a sketch dimension whose relation class is
/// refused and outside the point-distance families stays native, and the
/// decode report charges its loss note.
#[test]
fn a_refused_sketch_dimension_of_a_compound_lane_reaches_the_decode_report() {
    assert_eq!(
        unknown_class_losses(refused_dimension_part(&[0x02, 0x00])),
        1
    );
    assert_eq!(unknown_class_losses(refused_dimension_part(&[])), 0);
}
