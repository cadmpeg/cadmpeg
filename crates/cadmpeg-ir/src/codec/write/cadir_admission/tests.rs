// SPDX-License-Identifier: Apache-2.0

use crate::codec::write::target::TargetRequest;
use crate::codec::write::{
    ArenaCoverage, ArenaDisposition, CadirEncoder, EncodeInput, Encoder, ExportLossCode,
};
use crate::features::{Feature, FeatureDefinition, FeatureOperation};
use crate::schema::EntityKind;
use crate::CadIr;

fn feature(key: &str, ordinal: u64) -> Feature {
    Feature {
        product_definition: None,
        needs_recompute: false,
        id: format!("test:cadir-admission:feature#{key}")
            .try_into()
            .expect("a valid feature id"),
        ordinal,
        name: None,
        suppressed: None,
        dependencies: crate::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: crate::features::FeatureContent::default(),
        evaluation: crate::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}),
        ),
        native_ref: None,
    }
}

/// A child whose regeneration parent is absent from the document.
fn orphaned_child() -> CadIr {
    let mut ir = CadIr::empty();
    ir.model.features.push(feature("0-parent", 0));
    ir.model.features.push(feature("1-child", 1));
    ir.model
        .set_feature_regeneration_parent(
            &("test:cadir-admission:feature#1-child"
                .try_into()
                .expect("a valid feature id")),
            &("test:cadir-admission:feature#0-parent"
                .try_into()
                .expect("a valid feature id")),
        )
        .expect("the parent precedes the child");
    ir.model.features.remove(0);
    ir
}

#[test]
fn a_feature_with_a_missing_parent_is_omitted_with_a_loss_that_names_it() {
    let ir = orphaned_child();
    assert!(serde_json::to_value(&ir.model).is_err());
    let plan = CadirEncoder
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("a refused feature does not refuse the write");
    let report = plan.report();
    let code = ExportLossCode::ArenaOmitted(EntityKind::Feature)
        .note(String::new())
        .code;
    assert!(report.losses.iter().any(|loss| loss.code == code
        && loss
            .message
            .contains("test:cadir-admission:feature#1-child")));
    assert!(matches!(
        report.coverage,
        ArenaCoverage::Declared(dispositions)
            if dispositions.features == ArenaDisposition::Reported
    ));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("an in-memory write");
    let written = CadIr::from_json(std::str::from_utf8(&bytes).expect("UTF-8 JSON"))
        .expect("the written document reads");
    assert!(written.model.features.is_empty());
}

#[test]
fn a_serializable_document_is_written_whole() {
    let ir = crate::examples::unit_cube().expect("valid unit cube fixture");
    let mut losses = Vec::new();
    assert!(super::admit(&ir, &mut losses).is_none());
    assert!(losses.is_empty());
    assert_eq!(super::coverage(&ir, None), ArenaCoverage::Complete);
}
