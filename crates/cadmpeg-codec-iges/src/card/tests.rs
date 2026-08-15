// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(unused_imports)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{self, Cursor, Read, Seek, SeekFrom};

use cadmpeg_core::decode::DecodeMode;
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions, EncodeInput, Encoder};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, NurbsCurve, NurbsSurface, Pcurve, PcurveGeometry, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId, ShellId,
    SurfaceId, VertexId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::report::WritePath;
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, LoopBoundaryRole, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::CadIr;

use crate::test_support::*;
use crate::{IgesCodec, IgesEncoder, IgesVersion, IgesWriteOptions};

#[test]
fn overlong_preterminate_physical_line_is_malformed() {
    let mut bytes = point_file();
    let line_end = bytes
        .iter()
        .position(|byte| *byte == b'\n')
        .expect("Start line ending");
    bytes.insert(line_end, b'x');

    let error = IgesCodec
        .inspect(
            &mut Cursor::new(bytes),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap_err();
    assert!(matches!(error, CodecError::Malformed(_)));
}

#[test]
fn malformed_sequence_padding_is_rejected_without_panicking() {
    let mut bytes = point_file();
    bytes[73..80].copy_from_slice(b"     1 ");

    assert_eq!(IgesCodec.detect(&bytes), Confidence::No);
    assert_eq!(
        IgesCodec
            .inspect(
                &mut Cursor::new(bytes),
                &cadmpeg_core::decode::InspectOptions::default()
            )
            .unwrap_err()
            .to_string(),
        "not the expected format: unrecognized IGES representation"
    );
}

#[test]
fn inspect_reports_sections_and_physical_line_endings() {
    let mut bytes = card_with_ending(b"original fixture", b'S', 1, b"\r\n");
    bytes.extend(card_with_ending(
        b"1H,,1H;,1Hp,1Hf,1Hs,1Hv,32,38,6,308,15,,1,2,2HMM,1,1,1Hd,0,0,,,11;",
        b'G',
        1,
        b"\n",
    ));
    bytes.extend(card_with_ending(
        b"S0000001G0000001D0000000P0000000",
        b'T',
        1,
        b"\r",
    ));

    let summary = IgesCodec
        .inspect(
            &mut Cursor::new(bytes),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();

    assert_eq!(summary.format, "iges");
    assert_eq!(summary.container_kind, "fixed-ascii");
    assert_eq!(summary.entries.len(), 3);
    assert_eq!(summary.entries[0].name, "start");
    assert_eq!(summary.entries[0].attributes["line_endings"], "crlf:1");
    assert_eq!(summary.entries[1].attributes["line_endings"], "lf:1");
    assert_eq!(summary.entries[2].attributes["line_endings"], "cr:1");
}

#[test]
fn decode_rejects_extended_physical_records_before_terminate() {
    let mut bytes = point_file();
    let mut inserted = b"short record\n".to_vec();
    inserted.extend(std::iter::repeat_n(b'x', 81));
    inserted.push(b'\n');
    bytes.splice(162..162, inserted);

    let error = IgesCodec
        .inspect(
            &mut Cursor::new(bytes.as_slice()),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap_err();
    assert!(matches!(error, CodecError::Malformed(_)));
}

#[test]
fn inspect_rejects_terminate_count_mismatch() {
    let mut bytes = card(b"original fixture", b'S', 1);
    bytes.extend(card(b"1H,,1H;,,;", b'G', 1));
    bytes.extend(card(b"S0000001G0000002D0000000P0000000", b'T', 1));

    let error = IgesCodec
        .inspect(
            &mut Cursor::new(bytes),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "malformed container: IGES Terminate count for global is 2, actual 1"
    );
}

#[test]
fn inspect_accepts_space_padded_terminate_counts() {
    let mut bytes = card(b"original fixture", b'S', 1);
    bytes.extend(card(
        b"1H,,1H;,1Hp,1Hf,1Hs,1Hv,32,38,6,308,15,,1,2,2HMM,1,1,1Hd,0,0,,,11;",
        b'G',
        1,
    ));
    bytes.extend(card(b"S      1G      1D      0P      0", b'T', 1));

    IgesCodec
        .inspect(
            &mut Cursor::new(bytes),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();
}

#[test]
fn decode_retains_post_terminate_physical_record() {
    let mut bytes = point_file();
    bytes.extend_from_slice(b"transport padding\r\n");

    let result = IgesCodec
        .decode(
            &mut Cursor::new(bytes.as_slice()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(
        result.ir().native.namespace("iges").unwrap().arenas["cards"].len(),
        8
    );
}

#[test]
fn terminate_card_remainder_is_retained_after_terminate() {
    let mut bytes = point_file();
    let line_end = bytes.len() - 1;
    bytes.insert(line_end, b'x');

    let result = IgesCodec
        .decode(
            &mut Cursor::new(bytes.as_slice()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(
        result.ir().native.namespace("iges").unwrap().arenas["cards"].len(),
        8
    );
}
