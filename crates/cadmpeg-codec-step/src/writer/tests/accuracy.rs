// SPDX-License-Identifier: Apache-2.0
//! Retain topological accuracy in the STEP representation context.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::examples::unit_cube;

use crate::export::write_step;
use crate::{StepCodec, StepSchema, StepWriteOptions};

#[test]
fn context_preserves_edge_accuracy_without_lowering_document_accuracy() {
    for document_accuracy in [1.0e-6, 0.05] {
        let mut ir = unit_cube().expect("cube");
        ir.tolerances.linear = cadmpeg_ir::scalar::PositiveLength::new(document_accuracy)
            .expect("positive document accuracy");
        ir.model.edges[0].tolerance = cadmpeg_ir::scalar::PositiveReal::new(0.025);
        let mut bytes = Vec::new();
        write_step(
            &ir,
            &mut bytes,
            StepSchema::Ap214,
            &StepWriteOptions::default(),
        )
        .expect("write toleranced edge");
        let decoded = StepCodec::default()
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("read edge accuracy");
        let expected = document_accuracy.max(0.025);
        assert!((decoded.ir().tolerances.linear.get() - expected).abs() <= 1.0e-12);
    }
}

#[test]
fn context_preserves_vertex_and_face_accuracy() {
    for (document_accuracy, expected) in [(1.0e-6, 0.06), (0.1, 0.1)] {
        let mut ir = unit_cube().expect("cube");
        ir.tolerances.linear =
            cadmpeg_ir::scalar::PositiveLength::new(document_accuracy).expect("document accuracy");
        ir.model.edges[0].tolerance = cadmpeg_ir::scalar::PositiveReal::new(0.025);
        ir.model.vertices[0].tolerance = cadmpeg_ir::scalar::PositiveReal::new(0.04);
        ir.model.faces[0].tolerance = cadmpeg_ir::scalar::PositiveReal::new(0.06);
        let mut bytes = Vec::new();
        write_step(
            &ir,
            &mut bytes,
            StepSchema::Ap214,
            &StepWriteOptions::default(),
        )
        .expect("write topological accuracy");
        let decoded = StepCodec::default()
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("read context accuracy");
        assert!((decoded.ir().tolerances.linear.get() - expected).abs() <= 1.0e-12);
    }
}
