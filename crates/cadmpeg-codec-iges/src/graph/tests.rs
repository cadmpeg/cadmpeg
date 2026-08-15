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

use super::{cyclic_transform_nodes, ReferenceEdge, ReferenceKind, Resolution};
use crate::loss::IgesLossCode;
use crate::test_support::*;
use crate::{IgesCodec, IgesEncoder, IgesVersion, IgesWriteOptions};

#[test]
fn transform_cycle_detection_does_not_rewalk_a_long_acyclic_prefix() {
    let chain_length = 100_000_u32;
    let edges = (1..=chain_length)
        .map(|source| {
            let target = source + 1;
            (
                source,
                vec![ReferenceEdge {
                    kind: ReferenceKind::Transform,
                    raw_pointer: i64::from(target),
                    target: Some(format!("iges:entity:directory#{target}")),
                    resolution: Resolution::Resolved,
                    expected: "type-124".into(),
                    parameter_index: None,
                }],
            )
        })
        .collect::<BTreeMap<_, _>>();

    assert!(cyclic_transform_nodes(&edges).is_empty());
}

#[test]
fn inspect_preserves_transform_cycles_as_named_reference_states() {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        ["124", "1", "0", "0", "0", "0", "3", "0", "00000000"],
        1,
    ));
    bytes.extend(directory_card(
        ["124", "0", "0", "1", "0", "", "", "XFORM", "1"],
        2,
    ));
    bytes.extend(directory_card(
        ["124", "2", "0", "0", "0", "0", "1", "0", "00000000"],
        3,
    ));
    bytes.extend(directory_card(
        ["124", "0", "0", "1", "0", "", "", "XFORM", "2"],
        4,
    ));
    let matrix = b"124,1.,0.,0.,0.,1.,0.,0.,0.,1.,0.,0.,0.;";
    bytes.extend(parameter_card(matrix, 1, 1));
    bytes.extend(parameter_card(matrix, 3, 2));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000004P0000002").as_bytes(),
        b'T',
        1,
    ));

    let summary = IgesCodec
        .inspect(
            &mut Cursor::new(bytes.as_slice()),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();

    assert!(summary.notes.contains(&"references.cyclic=2".into()));

    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(bytes.as_slice()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let cycle_losses = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == IgesLossCode::PointerUnresolved.kind())
        .collect::<Vec<_>>();
    assert_eq!(cycle_losses.len(), 2);
    assert!(cycle_losses
        .iter()
        .all(|loss| loss.message.contains("Cyclic resolution")));
}
