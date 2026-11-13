// SPDX-License-Identifier: Apache-2.0
//! Preserve the document uncertainty independently of topological tolerances.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::examples::unit_cube;

use crate::export::write_step;
use crate::{StepCodec, StepSchema, StepWriteOptions};

#[test]
fn context_keeps_document_accuracy_when_entity_tolerances_differ() {
    for document_tolerance in [1.0e-6, 0.025, 0.1] {
        let mut ir = unit_cube().expect("cube");
        ir.tolerances.linear = cadmpeg_ir::scalar::PositiveLength::new(document_tolerance)
            .expect("document tolerance");
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
        .expect("write independent entity tolerances");
        let decoded = StepCodec::default()
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("read document uncertainty");
        assert!((decoded.ir().tolerances.linear.get() - document_tolerance).abs() <= 1.0e-12);
    }
}
