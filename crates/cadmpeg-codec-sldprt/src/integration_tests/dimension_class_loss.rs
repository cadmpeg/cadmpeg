// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::report::export::WritePath;
use cadmpeg_ir::scalar::PositiveLength;

use crate::loss::SldprtLossCode;
use crate::native::SldprtNative;
use crate::test_support::container::{make_block, sldprt_with_body};
use crate::test_support::history::resolved_features_payload_with_names_relation_and_scalar;
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;

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

/// The stream that holds the feature-input lane of [`token_dimension_part`].
#[derive(Clone, Copy)]
enum LaneStream {
    /// `Contents/Config-0-ResolvedFeatures`.
    ResolvedFeatures,
    /// `Contents/Config-0`, which declares `sgSketch`, beside a
    /// `ResolvedFeatures` stream that holds no dimension.
    Configuration,
}

/// The driving value of the dimension `D2` of [`token_dimension_part`].
const D2_DRIVING_METRES: f64 = 0.025;

/// The marker-address cell value of an operand that names no marker.
const NO_MARKER: [u8; 4] = [0xff; 4];

/// The operand-cell tag of a point operand.
const POINT_CELL: [u8; 2] = [0xd6, 0x80];

/// The operand-cell tag that the angle family's signature takes as a line.
const LINE_CELL: [u8; 2] = [0x86, 0x83];

/// The relation-class token and the parameter-class token of the token form.
/// The order rule of `sldprt.md` §2 gives them `sgPntPntDist` and
/// `moLengthParameter_c`, the first two declarations of their slots.
const RELATION_TOKEN: u16 = 0x8030;
const PARAMETER_TOKEN: u16 = 0x8032;

/// Appends the class declaration of `class`.
fn declare(bytes: &mut Vec<u8>, class: &str) {
    bytes.extend_from_slice(&[0xff, 0xff, 0x01, 0x00]);
    bytes.extend_from_slice(&u16::try_from(class.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(class.as_bytes());
}

/// Appends the name record of `name`.
fn name_record(bytes: &mut Vec<u8>, name: &str) {
    bytes.extend_from_slice(&[0x04, 0x80, 0xff, 0xfe, 0xff]);
    bytes.push(u8::try_from(name.encode_utf16().count()).unwrap());
    for unit in name.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
}

/// The scalar header between a scalar's name record and its f64 value; a
/// value-only scalar stores its first fourteen bytes and four zero bytes.
const SCALAR_HEADER: [u8; 22] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00,
    0xff, 0xfe, 0xff, 0x00, 0x00, 0x00,
];

/// Appends a scalar's value `metres` and its trailer: the object identifier
/// `object` at +3, two operand cells at +27, the role at +29, and then the
/// two cells of `tag`, each naming the sketch point at `points`.
fn scalar_value(
    bytes: &mut Vec<u8>,
    metres: f64,
    object: u32,
    role: u8,
    tag: [u8; 2],
    points: [u16; 2],
) {
    bytes.extend_from_slice(&metres.to_le_bytes());
    let at = bytes.len();
    bytes.resize(at + 35, 0);
    bytes[at + 3..at + 7].copy_from_slice(&object.to_le_bytes());
    bytes[at + 27] = 2;
    bytes[at + 29] = role;
    for point in points {
        bytes.extend_from_slice(&tag);
        bytes.extend_from_slice(&point.to_le_bytes());
        bytes.extend_from_slice(&NO_MARKER);
        bytes.extend_from_slice(&[0; 4]);
    }
}

/// Appends the point marker of `ordinal` at `coordinates` metres: native
/// code `0` at +17, the profile-curve locus at +21, role `1` at +27, the
/// state at +48, the coordinate tag `1e 00` at +64 and the two coordinates
/// at +66, and the local identifier at +88.
fn point_marker(bytes: &mut Vec<u8>, ordinal: u32, [x, y]: [f64; 2]) {
    let at = bytes.len();
    bytes.extend_from_slice(&[0xff, 0xff, 0x1f, 0x00, 0x03]);
    bytes.extend_from_slice(&[0xff; 8]);
    bytes.extend_from_slice(&(-1.0f32).to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&[0x00, 0x00, 0x04, 0x00, 0x02, 0x00]);
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&[0, 0]);
    bytes.extend_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    bytes.resize(at + 48, 0);
    bytes.extend_from_slice(&(f64::from(ordinal) + 1.0).to_le_bytes());
    bytes.resize(at + 64, 0);
    bytes.extend_from_slice(&[0x1e, 0x00]);
    bytes.extend_from_slice(&x.to_le_bytes());
    bytes.extend_from_slice(&y.to_le_bytes());
    bytes.resize(at + 88, 0);
    bytes.extend_from_slice(&(ordinal + 1).to_le_bytes());
}

/// A compound-file part whose sketch `Sketch1` holds four points, the fourth
/// `reach` metres along the x axis from the second, and three dimensions:
///
/// - `D1`, a value-only scalar that `sgPntPntDist`, declared with
///   `moLengthParameter_c` inside its relation object, claims;
/// - `D1` again, a display and a driving scalar in the token form whose
///   relation class `sgLLDist` is declared in place, so that its
///   parameter-class token states a length;
/// - `D2`, a display and a driving scalar in the token form that the
///   `sgAnglDim` declaration before them claims, whose record states a
///   length, and whose relation-class token names `sgPntPntDist`; their
///   operands name the second and the fourth point, and the driving value is
///   [`D2_DRIVING_METRES`].
fn token_dimension_part(reach: f64, stream: LaneStream) -> Vec<u8> {
    let mut lane = vec![0x02, 0x00];
    for class in ["sgPointHandle", "sgLineHandle", "sgArcHandle"] {
        declare(&mut lane, class);
    }
    if matches!(stream, LaneStream::Configuration) {
        declare(&mut lane, "sgSketch");
    }
    name_record(&mut lane, "Sketch1");
    declare(&mut lane, "sgPntPntDist");
    lane.extend_from_slice(&[0; 4]);
    declare(&mut lane, "moLengthParameter_c");
    name_record(&mut lane, "D1");
    lane.extend_from_slice(&SCALAR_HEADER[..14]);
    lane.extend_from_slice(&[0; 4]);
    scalar_value(&mut lane, 0.0215, 0x40, 0, POINT_CELL, [1, 2]);
    for (object, (name, metres, role, tag, points)) in (0x42u32..).step_by(2).zip([
        ("D1", 0.0412, 1, POINT_CELL, [1, 2]),
        ("D1", 0.045, 0, POINT_CELL, [1, 2]),
        ("D2", 0.0176, 1, LINE_CELL, [1, 3]),
        ("D2", D2_DRIVING_METRES, 0, LINE_CELL, [1, 3]),
    ]) {
        match object {
            0x42 => declare(&mut lane, "sgLLDist"),
            0x46 => {
                declare(&mut lane, "sgAnglDim");
                lane.extend_from_slice(&RELATION_TOKEN.to_le_bytes());
            }
            _ => lane.extend_from_slice(&RELATION_TOKEN.to_le_bytes()),
        }
        lane.extend_from_slice(&[0; 4]);
        lane.extend_from_slice(&PARAMETER_TOKEN.to_le_bytes());
        name_record(&mut lane, name);
        lane.extend_from_slice(&SCALAR_HEADER);
        scalar_value(&mut lane, metres, object, role, tag, points);
    }
    for (ordinal, point) in (0u32..).zip([[1.0, 1.0], [0.0, 0.0], [5.0, 5.0], [reach, 0.0]]) {
        point_marker(&mut lane, ordinal, point);
    }
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    match stream {
        LaneStream::ResolvedFeatures => {
            source.extend(make_block(
                0x45,
                "Contents/Config-0-ResolvedFeatures",
                &lane,
            ));
        }
        LaneStream::Configuration => {
            let mut resolved = vec![0x02, 0x00];
            declare(&mut resolved, "sgPointHandle");
            source.extend(make_block(
                0x45,
                "Contents/Config-0-ResolvedFeatures",
                &resolved,
            ));
            source.extend(make_block(0x45, "Contents/Config-0", &lane));
        }
    }
    source
}

/// The identities a loss note names, each in backticks.
fn named(message: &str) -> BTreeSet<&str> {
    message.split('`').skip(1).step_by(2).collect()
}

/// The scalars of the lane of `native` whose identity contains `lane` and
/// whose name record is `name`.
fn named_scalars(native: &SldprtNative, lane: &str, name: &str) -> BTreeSet<String> {
    let lane = native
        .feature_input_lanes
        .iter()
        .find(|candidate| candidate.id.contains(lane))
        .expect("the lane");
    lane.scalars
        .iter()
        .filter(|scalar| {
            lane.names
                .iter()
                .any(|record| record.id == scalar.name && record.value == name)
        })
        .map(|scalar| scalar.id.clone())
        .collect()
}

/// In a compound-file lane of a `Config-N` stream, a refused sketch
/// dimension forms where its sketch points measure its value. Elsewhere the
/// decode report charges one `sketch.dimension-class-unknown` note that names
/// the dimension's scalars.
#[test]
fn a_refused_sketch_dimension_of_a_configuration_stream_lane_reaches_the_decode_report() {
    const OFF_VALUE_METRES: f64 = 1.0e-4;
    let code = SldprtLossCode::SketchDimensionClassUnknown.kind();
    for (reach, forms) in [
        (D2_DRIVING_METRES, true),
        (D2_DRIVING_METRES + OFF_VALUE_METRES, false),
    ] {
        let decoded = decode(token_dimension_part(reach, LaneStream::Configuration));
        let native = SldprtNative::load(
            decoded
                .ir()
                .native
                .namespace("sldprt")
                .expect("native records"),
            decoded.ir().tolerances.linear,
        )
        .unwrap();
        let dimension = named_scalars(&native, ":config-objects#", "D2");
        assert_eq!(dimension.len(), 2);
        let notes = decoded
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == code)
            .map(|loss| {
                named(&loss.message)
                    .into_iter()
                    .map(str::to_owned)
                    .collect()
            })
            .collect::<Vec<BTreeSet<String>>>();
        assert_eq!(
            notes,
            if forms { Vec::new() } else { vec![dimension] },
            "{reach}"
        );
    }
}

/// A part decoded with a refused sketch dimension that its points measure
/// only beyond the decode's linear tolerance, written under a linear
/// tolerance that measures it, holds native relation instances that the
/// document's tolerance forms otherwise. The write, with and without the
/// decode sidecar, synthesizes the part without its native records and
/// charges `export:native.records-omitted`.
#[test]
fn a_part_written_under_another_linear_tolerance_omits_its_stale_native_records() {
    const OFF_VALUE_METRES: f64 = 1.0e-4;
    const WRITE_TOLERANCE_MM: f64 = 1.0;
    let decoded = decode(token_dimension_part(
        D2_DRIVING_METRES + OFF_VALUE_METRES,
        LaneStream::ResolvedFeatures,
    ));
    let mut ir = decoded.ir().clone();
    ir.tolerances.linear = PositiveLength::new(WRITE_TOLERANCE_MM).unwrap();
    let namespace = ir.native.namespace("sldprt").expect("native records");
    assert!(SldprtNative::load(namespace, decoded.ir().tolerances.linear).is_ok());
    assert!(SldprtNative::load(namespace, ir.tolerances.linear).is_err());

    crate::test_support::plan_inherited_write_declined(&ir, decoded.source_fidelity());

    let mut output = Vec::new();
    let report = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap()
        .write_to(&mut output)
        .unwrap();
    assert!(
        matches!(report.write_path(), WritePath::Synthesized { .. }),
        "{:?}",
        report.write_path()
    );
    assert!(
        report
            .losses
            .iter()
            .any(|loss| loss.code.local_code() == "native.records-omitted"),
        "{:?}",
        report.losses
    );
    decode(output);
}
