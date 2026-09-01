// SPDX-License-Identifier: Apache-2.0
//! Recovered card framing: fused lines, positional sequences, census counts.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::decode::DecodeMode;
use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions};
use cadmpeg_ir::report::DecodeReport;

use crate::loss::IgesLossCode;
use crate::test_support::{
    owned_test_file, OwnedTestEntity, CARD_COLUMNS, CARD_DATA_COLUMNS, CARD_LINE_BYTES,
};
use crate::IgesCodec;

/// Authored coordinates of the single Type 116 point in every fixture here.
const COORDINATES: [f64; 3] = [11.5, -3.25, 7.0];

fn strict_options() -> DecodeOptions {
    let mut options = DecodeOptions::default();
    options.policy.mode = DecodeMode::Strict;
    options
}

fn framing_losses(report: &DecodeReport) -> usize {
    report
        .losses
        .iter()
        .filter(|loss| loss.code == IgesLossCode::CardFramingRecovered.kind())
        .count()
}

fn point_file() -> Vec<u8> {
    owned_test_file(&[OwnedTestEntity {
        entity_type: 116,
        form: 0,
        label: "POINT".into(),
        status: "00000000",
        parameters: format!(
            "116,{},{},{},0;",
            COORDINATES[0], COORDINATES[1], COORDINATES[2]
        ),
    }])
}

fn decode(bytes: Vec<u8>) -> cadmpeg_ir::codec::DecodeResult {
    IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap()
}

fn decoded_position(result: &cadmpeg_ir::codec::DecodeResult) -> [f64; 3] {
    let point = &result.ir().model.points[0];
    [point.position.x, point.position.y, point.position.z]
}

/// Drop every line terminator, leaving one uninterrupted 80-column stride.
fn stride(bytes: &[u8]) -> Vec<u8> {
    bytes
        .iter()
        .copied()
        .filter(|byte| *byte != b'\n')
        .collect()
}

/// Join the card carrying `section` at `index` onto the following card.
fn fuse(bytes: &[u8], section: u8, index: usize) -> Vec<u8> {
    let card = bytes
        .chunks_exact(CARD_LINE_BYTES)
        .enumerate()
        .filter(|(_, line)| line[CARD_DATA_COLUMNS] == section)
        .nth(index)
        .map(|(line, _)| line)
        .expect("section card");
    let mut fused = bytes.to_vec();
    assert_eq!(fused[card * CARD_LINE_BYTES + CARD_COLUMNS], b'\n');
    fused.remove(card * CARD_LINE_BYTES + CARD_COLUMNS);
    fused
}

/// Replace every declared sequence in `section` with an unrelated value.
fn misnumber(bytes: &[u8], section: u8) -> Vec<u8> {
    let mut misnumbered = bytes.to_vec();
    for line in misnumbered
        .chunks_exact_mut(CARD_LINE_BYTES)
        .filter(|line| line[CARD_DATA_COLUMNS] == section)
    {
        line[CARD_DATA_COLUMNS + 1..CARD_COLUMNS].copy_from_slice(b"9999999");
    }
    misnumbered
}

#[test]
fn a_line_carrying_two_cards_divides_into_cards_with_one_framing_loss() {
    let bytes = fuse(&point_file(), b'D', 0);

    let result = decode(bytes);

    assert_eq!(decoded_position(&result), COORDINATES);
    assert_eq!(
        result.ir().native.namespace("iges").unwrap().arenas["cards"].len(),
        7
    );
    assert_eq!(framing_losses(result.report()), 1);
    assert_eq!(result.report().losses.len(), 1);
}

#[test]
fn a_terminator_free_card_stride_decodes_its_authored_coordinates() {
    let bytes = stride(&point_file());
    assert_eq!(bytes.len() % 80, 0);
    assert_eq!(IgesCodec.detect(&bytes), Confidence::High);

    let result = decode(bytes);

    assert_eq!(decoded_position(&result), COORDINATES);
    assert_eq!(framing_losses(result.report()), 1);
    assert_eq!(result.report().losses.len(), 1);
}

#[test]
fn a_misnumbered_section_charges_one_framing_loss_and_keeps_positional_identity() {
    let bytes = misnumber(&misnumber(&point_file(), b'D'), b'P');

    let result = decode(bytes);

    let native = result.ir().native.namespace("iges").unwrap();
    assert_eq!(native.arenas["entities"][0].id(), "iges:entity:directory#1");
    assert_eq!(decoded_position(&result), COORDINATES);
    assert_eq!(framing_losses(result.report()), 2);
    assert_eq!(result.report().losses.len(), 2);
}

#[test]
fn strict_decode_refuses_recovered_framing_that_salvage_admits() {
    for bytes in [
        fuse(&point_file(), b'D', 0),
        stride(&point_file()),
        misnumber(&point_file(), b'D'),
    ] {
        let container_only = IgesCodec
            .decode(
                &mut Cursor::new(bytes.clone()),
                &DecodeOptions {
                    container_only: true,
                    ..DecodeOptions::default()
                },
            )
            .unwrap();
        assert_eq!(framing_losses(container_only.report()), 1);

        let error = IgesCodec
            .decode(&mut Cursor::new(bytes.clone()), &strict_options())
            .unwrap_err();
        match error {
            cadmpeg_ir::codec::DecodeFailure::StrictRejected { loss_code, .. } => assert_eq!(
                loss_code,
                IgesLossCode::CardFramingRecovered.kind().as_str()
            ),
            other => panic!("expected a strict refusal, got {other:?}"),
        }
    }
}
