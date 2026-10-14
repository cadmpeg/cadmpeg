// SPDX-License-Identifier: Apache-2.0
//! The fixed occurrence ceiling for product occurrence expansion.

fn occurrence_output_refuses(existing: usize) {
    crate::test_support::with_service_context(super::PRODUCT_STRING_LIMIT_SOURCE, |source, ctx| {
        let (exchange, _) = crate::parse::parse_inner(source, ctx).expect("valid product source");
        let mut ir = cadmpeg_ir::CadIr::empty();
        let (geometry, _) = crate::reader::geometry::decode(&exchange, &mut ir, ctx).unwrap();
        let index = crate::reader::index::CarrierIndex::from_ir(&ir);
        let topology = crate::reader::topology::decode(&exchange, &mut ir, &index, ctx).unwrap();
        let mut admitted = 0;
        super::super::decode(
            &exchange,
            &geometry.value,
            &topology.value,
            &mut ir,
            ctx,
            &mut admitted,
        )
        .expect("initial assembly fits");
        ir.model
            .occurrences
            .resize(existing, ir.model.occurrences[0].clone());
        let model_input =
            serde_json::to_vec(&ir).expect("synthetic existing assembly budget envelope");
        let error = crate::test_support::with_service_context(&model_input, |_, ctx| {
            super::super::decode(
                &exchange,
                &geometry.value,
                &topology.value,
                &mut ir,
                ctx,
                &mut admitted,
            )
            .err()
            .expect("occurrence slice must refuse")
        });
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::NotImplemented(message)
                if message == "STEP assembly expands to more than 100000 occurrences"
        ));
        assert_eq!(ir.model.occurrences.len(), 100_000);
    });
}

#[test]
fn root_occurrence_output_slice_refuses_before_insertion() {
    occurrence_output_refuses(100_000);
}

#[test]
fn child_occurrence_output_slice_refuses_instead_of_partial_success() {
    occurrence_output_refuses(99_999);
}
