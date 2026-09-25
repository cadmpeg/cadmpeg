mod closure;
mod decode;
mod pcurves;

mod parameter_ranges;

#[test]
fn b5_topology_entity_limit_refuses_before_first_model_append() {
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let graph = crate::families::b5::graph::parse(&bytes, &mut crate::nurbs::LaneRefusals::new())
        .expect("closed B5 triangle graph");
    crate::test_support::with_entity_limit(0, |ctx| {
        let mut ir = cadmpeg_ir::CadIr::empty();
        let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        let error = super::transfer(
            &mut ir,
            &mut annotations,
            graph,
            &cadmpeg_ir::ids::UnknownId::mint("catia:payload:unknown#test".to_string())
                .expect("identity grammar"),
            &mut crate::nurbs::LaneRefusals::new(),
            &mut admission,
        )
        .expect_err("a B5 model record exceeds the zero-entity allowance");
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::Entities
                    && limit.operation == "admit CATIA family model entity"
        ));
        assert_eq!(ir.model.entity_count(), 0);
    });
}
