// SPDX-License-Identifier: Apache-2.0
//! Complete semantic-writer vertex records through the decoder.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::scalar::PositiveReal;

#[test]
fn emitted_vertex_records_keep_body_ownership_and_optional_tolerance() {
    for tolerance in [None, Some(PositiveReal::new(0.125).unwrap())] {
        let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
        for vertex in &mut ir.model.vertices {
            vertex.tolerance = tolerance;
        }
        let body = crate::writer::brep_body(&ir, 0.001, false).unwrap();
        let native = crate::test_support::container::sldprt_with_body(&body);
        let decoded = crate::SldprtCodec
            .decode(&mut Cursor::new(native), &DecodeOptions::default())
            .unwrap();
        let model = &decoded.ir().model;
        assert_eq!(model.bodies.len(), 1, "{:?}", decoded.report().losses);
        assert_eq!(model.faces.len(), 6);
        assert_eq!(model.edges.len(), 12);
        assert_eq!(model.vertices.len(), 8);
        assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
            .unwrap()
            .is_ok());
        for vertex in &model.vertices {
            match (vertex.tolerance, tolerance) {
                (None, None) => {}
                (Some(actual), Some(expected)) => {
                    assert!((actual.get() - expected.get()).abs() < 1.0e-12);
                }
                other => panic!("vertex tolerance changed: {other:?}"),
            }
        }
    }
}
