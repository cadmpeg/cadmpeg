// SPDX-License-Identifier: Apache-2.0
//! Collection limits for occurrence placement lookup.

const CONTEXT_SOURCE: &[u8] = include_bytes!("../../../../tests/fixtures/ap242_assembly.p21");
const MAPPED_SOURCE: &[u8] = include_bytes!("../../../../tests/fixtures/ap242_mapped_assembly.p21");

fn duplicate_context_source() -> String {
    String::from_utf8_lossy(CONTEXT_SOURCE).replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#39=CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#37,#13);\nENDSEC;\nEND-ISO-10303-21;",
    )
}

fn occurrence_mapped_source(duplicate: bool) -> String {
    let source = String::from_utf8_lossy(MAPPED_SOURCE)
        .replace(
            "#22=SHAPE_REPRESENTATION('root shape',(#40),#21);",
            if duplicate {
                "#22=SHAPE_REPRESENTATION('root shape',(#40,#42),#21);"
            } else {
                "#22=SHAPE_REPRESENTATION('root shape',(#40),#21);"
            },
        )
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#13=PRODUCT_DEFINITION_SHAPE('','',#12);\n#41=SHAPE_DEFINITION_REPRESENTATION(#13,#22);\n#42=MAPPED_ITEM('Second child',#39,#35);\nENDSEC;\nEND-ISO-10303-21;",
        );
    source
}

fn competing_source() -> String {
    String::from_utf8_lossy(CONTEXT_SOURCE)
        .replace(
            "#22=SHAPE_REPRESENTATION('root shape',(),#21);",
            "#22=SHAPE_REPRESENTATION('root shape',(#45),#21);",
        )
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#43=SHAPE_DEFINITION_REPRESENTATION(#13,#22);\n#44=REPRESENTATION_MAP(#34,#23);\n#45=MAPPED_ITEM('Mapped child',#44,#35);\nENDSEC;\nEND-ISO-10303-21;",
        )
}

#[test]
fn context_candidate_groups_refuse_collection_limit() {
    super::resource_limits::product_collection_refuses_source(
        CONTEXT_SOURCE,
        "step_context_candidate_groups",
    );
}

#[test]
fn context_candidate_members_refuse_collection_limit() {
    super::resource_limits::product_collection_refuses_source(
        CONTEXT_SOURCE,
        "step_context_candidate_members",
    );
}

#[test]
fn occurrence_placement_results_refuse_collection_limit() {
    super::resource_limits::product_collection_refuses_source(
        CONTEXT_SOURCE,
        "step_occurrence_placement_results",
    );
}

#[test]
fn sibling_usage_counts_refuse_collection_limit() {
    super::resource_limits::product_collection_refuses_source(
        CONTEXT_SOURCE,
        "step_sibling_usage_counts",
    );
}

#[test]
fn ambiguous_context_source_copy_refuses_collection_limit() {
    let source = duplicate_context_source();
    super::resource_limits::product_collection_refuses_source(
        source.as_bytes(),
        "step_ambiguous_context_source_copy",
    );
}

#[test]
fn ambiguous_placement_groups_refuse_collection_limit() {
    let source = duplicate_context_source();
    super::resource_limits::product_collection_refuses_source(
        source.as_bytes(),
        "step_ambiguous_placement_groups",
    );
}

#[test]
fn occurrence_representation_groups_refuse_collection_limit() {
    let source = occurrence_mapped_source(false);
    super::resource_limits::product_collection_refuses_source(
        source.as_bytes(),
        "step_occurrence_representation_groups",
    );
}

#[test]
fn occurrence_representation_members_refuse_collection_limit() {
    let source = occurrence_mapped_source(false);
    super::resource_limits::product_collection_refuses_source(
        source.as_bytes(),
        "step_occurrence_representation_members",
    );
}

#[test]
fn occurrence_placement_candidates_refuse_collection_limit() {
    let source = occurrence_mapped_source(false);
    super::resource_limits::product_collection_refuses_source(
        source.as_bytes(),
        "step_occurrence_placement_candidates",
    );
}

#[test]
fn ambiguous_mapped_sources_refuse_collection_limit() {
    let source = occurrence_mapped_source(true);
    super::resource_limits::product_collection_refuses_source(
        source.as_bytes(),
        "step_ambiguous_mapped_sources",
    );
}

#[test]
fn competing_context_source_copy_refuses_collection_limit() {
    let source = competing_source();
    super::resource_limits::product_collection_refuses_source(
        source.as_bytes(),
        "step_competing_context_source_copy",
    );
}

#[test]
fn competing_mapped_sources_refuse_collection_limit() {
    let source = competing_source();
    super::resource_limits::product_collection_refuses_source(
        source.as_bytes(),
        "step_competing_mapped_sources",
    );
}

#[test]
fn competing_source_copy_refuses_collection_limit() {
    let source = competing_source();
    super::resource_limits::product_collection_refuses_source(
        source.as_bytes(),
        "step_competing_source_copy",
    );
}

#[test]
fn competing_placement_groups_refuse_collection_limit() {
    let source = competing_source();
    super::resource_limits::product_collection_refuses_source(
        source.as_bytes(),
        "step_competing_placement_groups",
    );
}

#[test]
fn fallback_occurrence_placements_refuse_collection_limit() {
    super::resource_limits::product_collection_refuses_source(
        MAPPED_SOURCE,
        "step_fallback_occurrence_placements",
    );
}

#[test]
fn ambiguous_placement_source_text_refuses_work_limit() {
    // Detail text is scratch; its byte work is cumulative even below an earlier storage peak.
    let source = duplicate_context_source();
    super::resource_limits::product_text_work_refuses_source(
        source.as_bytes(),
        "step_ambiguous_placement_source_text",
    );
}

#[test]
fn ambiguous_placement_loss_text_refuses_retained_limit() {
    let source = duplicate_context_source();
    super::resource_limits::product_retained_refuses_source(
        source.as_bytes(),
        "step_ambiguous_placement_loss_text",
    );
}

#[test]
fn competing_placement_source_text_refuses_work_limit() {
    // Detail text is scratch; its byte work is cumulative even below an earlier storage peak.
    let source = competing_source();
    super::resource_limits::product_text_work_refuses_source(
        source.as_bytes(),
        "step_competing_placement_source_text",
    );
}

#[test]
fn competing_placement_loss_text_refuses_retained_limit() {
    let source = competing_source();
    super::resource_limits::product_retained_refuses_source(
        source.as_bytes(),
        "step_competing_placement_loss_text",
    );
}

#[test]
fn body_conflict_source_text_refuses_work_limit() {
    // Detail text is scratch; its byte work is cumulative even below an earlier storage peak.
    let source = super::resource_limits::mapped_body_placement_source();
    super::resource_limits::product_text_work_refuses_source(
        source.as_bytes(),
        "step_body_conflict_source_text",
    );
}

#[test]
fn body_conflict_loss_text_refuses_retained_limit() {
    let source = super::resource_limits::mapped_body_placement_source();
    super::resource_limits::product_retained_refuses_source(
        source.as_bytes(),
        "step_body_conflict_loss_text",
    );
}

#[test]
fn missing_shape_body_loss_text_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_ir::ids::BodyId;
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=PRODUCT('id','name','',$);#2=PRODUCT_DEFINITION_FORMATION('','',#1);#3=PRODUCT_DEFINITION('','',#2,$);#4=PRODUCT_DEFINITION_SHAPE('','',#3);#5=SHAPE_DEFINITION_REPRESENTATION(#4,#6);#6=SHAPE_REPRESENTATION('',(#7),$);#7=MANIFOLD_SOLID_BREP('',$);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::test_support::with_service_context(source, crate::parse::parse_inner).unwrap();
    crate::test_support::with_service_context(source, |_, owner| {
        let mut ir = cadmpeg_ir::CadIr::empty();
        let geometry = crate::reader::geometry::decode(&exchange, &mut ir, owner).unwrap();
        let index = crate::reader::index::CarrierIndex::from_ir(&ir, owner).unwrap();
        let mut topology = crate::reader::topology::decode(&exchange, &mut ir, &index, owner).unwrap();
        topology.value.body_by_root.insert(7, vec![BodyId::mint("step:data:body#7").unwrap()]);
        let stage = super::super::decode(&exchange, &geometry.value, &topology.value, &mut ir.clone(), owner, &mut 0).unwrap();
        assert!(stage.losses.iter().any(|loss| loss.message == "PRODUCT_DEFINITION #3 omitted uncommitted shape body reference(s): step:data:body#7"));
        drop(stage);
        cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            "step_missing_shape_body_loss_text",
            |limit| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes = limit;
                let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
                let result = super::super::decode(&exchange, &geometry.value, &topology.value, &mut ir.clone(), &ctx, &mut 0);
                if let Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal)) = result {
                    assert_eq!(ctx.resource_refusal(), Some(*refusal));
                }
                result.map(|_| ())
            },
        );
    });
}
