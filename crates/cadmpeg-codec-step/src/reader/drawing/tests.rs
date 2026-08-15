// SPDX-License-Identifier: Apache-2.0
//! STEP drawing-graph tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]
#![allow(unused_imports)]

use crate::loss::StepLossCode;
use crate::test_support::decode_inline;
use crate::{write_step, StepError, StepUnsupportedPolicy, StepWriteOptions};

#[test]
fn drawing_graph_transfers_pages_revisions_views_and_opaque_items() {
    let result = decode_inline(
        "#1=DRAWING_DEFINITION('Main','detail');
#2=DRAWING_REVISION('A',#1,'rev');
#3=REPRESENTATION_CONTEXT('','');
#4=PRESENTATION_VIEW('Front',(#5),#3);
#5=ITEM('opaque');
#6=DRAWING_SHEET_REVISION('Sheet',(#4),#3,#2);
#7=DRAWING_SHEET_REVISION_USAGE(#6,#2,'1');
#8=PRESENTATION_SIZE(#6,#9);
#9=DESCRIPTIVE_REPRESENTATION_ITEM('A3','');
#10=DRAUGHTING_MODEL('Drawing model',(#4),#3);
#11=ITEM('semantic');
#12=DRAUGHTING_MODEL_ITEM_ASSOCIATION('','',#11,#10,(#4));",
    );

    assert_eq!(result.ir().model.drawings.len(), 6);
    let page = result
        .ir()
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.runtime_type == "DRAWING_SHEET_REVISION")
        .expect("drawing sheet");
    assert!(matches!(page.kind, cadmpeg_ir::drawings::DrawingKind::Page));
    assert_eq!(page.parameters["name"], "Sheet");
    assert_eq!(page.parameters["usage_7_sequence"], "1");
    assert!(page.relationships["items"]
        .iter()
        .any(|target| { target.target.as_deref() == Some("step:drawing:presentation_view#4") }));
    assert!(page.relationships["drawing_revision"]
        .iter()
        .any(|target| { target.target.as_deref() == Some("step:drawing:drawing_revision#2") }));

    let view = result
        .ir()
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.runtime_type == "PRESENTATION_VIEW")
        .expect("presentation view");
    assert!(view.relationships["items"]
        .iter()
        .any(|target| { target.target.as_deref() == Some("step:data:item#5") }));
    assert_eq!(view.parameters["presentation_context"], "#3");

    let model = result
        .ir()
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.runtime_type == "DRAUGHTING_MODEL")
        .expect("draughting model");
    assert!(model.relationships["semantic_definition"]
        .iter()
        .any(|target| { target.target.as_deref() == Some("step:data:item#11") }));
    assert!(model.relationships["associated_items"]
        .iter()
        .any(|target| { target.target.as_deref() == Some("step:drawing:presentation_view#4") }));

    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
    assert!(result
        .ir()
        .native_unknowns("step")
        .expect("STEP native namespace")
        .iter()
        .any(|record| record.id.0 == "step:data:item#5"));
    assert!(result.report().losses.iter().all(|loss| {
        loss.code != StepLossCode::DrawingSheetRevisionUnresolved.kind()
            && loss.code != StepLossCode::DrawingRevisionSheetUnresolved.kind()
    }));

    let mut output = Vec::new();
    let error = write_step(
        result.ir(),
        &mut output,
        &StepWriteOptions {
            unsupported: StepUnsupportedPolicy::Reject,
            ..StepWriteOptions::default()
        },
    )
    .expect_err("strict STEP writing must refuse unrepresentable drawings");
    assert!(
        matches!(error, StepError::Unsupported(message) if message.contains("drawing/presentation"))
    );
}

#[test]
fn drawing_relationship_with_multiple_product_views_is_not_retargeted() {
    let result = decode_inline(
        "#1=APPLICATION_CONTEXT('mechanical design');
#2=PRODUCT_CONTEXT('',#1,'mechanical');
#3=PRODUCT('P','Part','',(#2));
#4=PRODUCT_DEFINITION_FORMATION('v1','',#3);
#5=PRODUCT_DEFINITION_CONTEXT('part definition',#1,'design');
#6=PRODUCT_DEFINITION('design view','',#4,#5);
#7=PRODUCT_DEFINITION_FORMATION('v2','',#3);
#8=PRODUCT_DEFINITION('manufacturing view','',#7,#5);
#9=REPRESENTATION_CONTEXT('','');
#10=PRESENTATION_VIEW('Front',(#3),#9);",
    );

    let view = result
        .ir()
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.runtime_type == "PRESENTATION_VIEW")
        .expect("presentation view");
    assert_eq!(view.parameters["items"], "(#3)");
    assert!(!view.relationships.contains_key("items"));
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::DrawingRelationshipTargetAmbiguous.kind()
            && loss.message.contains("multiple neutral identities")
            && loss.message.contains("no target was selected")
    }));
}
