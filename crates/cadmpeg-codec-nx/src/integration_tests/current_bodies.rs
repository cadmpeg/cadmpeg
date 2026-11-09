// SPDX-License-Identifier: Apache-2.0
//! End-to-end contracts for the selection of current bodies among partition
//! and plain cached-body streams.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::topology::BodyKind;

use crate::loss::NxLossCode;
use crate::NxCodec;

/// The vertex box of the one solid in the NIST CTC-03 revision C STEP export.
const CTC_03_VERTEX_BOX_MM: [[f64; 3]; 2] = [
    [-249.561_087_143, 0.0, 0.0],
    [53.402_246_443, 485.4448, 162.680_149_731],
];
const CTC_03_FACE_COUNT: usize = 139;
const CTC_03_VERTEX_BOX_TOLERANCE_MM: f64 = 1e-6;

#[test]
fn ctc_03_keeps_the_partition_solid_and_drops_plain_history_images() {
    let bytes = include_bytes!("../../../../corpus/nist-pmi/nx/nist_ctc_03_asme1_nx1980_rc.prt");
    let result = NxCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let model = &result.ir().model;

    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == NxLossCode::PlainHistoryImagesWithheld.kind()));
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == NxLossCode::SubBodyCompositionUnresolved.kind()));
    assert_eq!(model.bodies.len(), 1);
    assert_eq!(model.bodies[0].kind, BodyKind::Solid);
    assert_eq!(model.faces.len(), CTC_03_FACE_COUNT);
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for point in &model.points {
        let position = point.position().get();
        for (axis, value) in [position.x, position.y, position.z].into_iter().enumerate() {
            low[axis] = low[axis].min(value);
            high[axis] = high[axis].max(value);
        }
    }
    for axis in 0..3 {
        assert!((low[axis] - CTC_03_VERTEX_BOX_MM[0][axis]).abs() < CTC_03_VERTEX_BOX_TOLERANCE_MM);
        assert!(
            (high[axis] - CTC_03_VERTEX_BOX_MM[1][axis]).abs() < CTC_03_VERTEX_BOX_TOLERANCE_MM
        );
    }
}
