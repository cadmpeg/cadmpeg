// SPDX-License-Identifier: Apache-2.0
//! Occurrence-local identities for parameter-state graphs.

use super::super::*;
use crate::records::{
    parameter_state::{
        DesignParameterDocumentHeader, DesignParameterState, DesignParameterStateCompanion,
    },
    parameters::{DesignParameter, DesignParameterContent, DesignParameterSource},
};

#[test]
fn occurrence_merge_keeps_parameter_state_links_local_without_rewriting_parameter_text() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut component = Native::default();
    let state = DesignParameterState {
        id: "f3d:Design1:design-parameter-state#21".into(),
        class_tag: "257".to_owned().try_into().expect("state class"),
        record_index: 401,
        companion: "f3d:Design1:design-parameter-state-companion#82".into(),
        document_root: "f3d:Design1:design-parameter-document-header#0".into(),
    };
    let companion = DesignParameterStateCompanion {
        id: state.companion.clone(),
        class_tag: "258".to_owned().try_into().expect("companion class"),
        record_index: 702,
        control: 1,
        control_word: 1,
        state: state.id.clone(),
    };
    let header = DesignParameterDocumentHeader {
        id: state.document_root.clone(),
        class_tag: "256".to_owned().try_into().expect("root class"),
        record_index: 93,
    };
    let mut parameter = DesignParameter::authored(DesignParameterContent {
        id: "f3d:Design1:design-parameter#126".into(),
        class_tag: "259".to_owned().try_into().expect("parameter class"),
        record_index: 905,
        source_ordinal: 13,
        source: DesignParameterSource::authored("User Parameter".into(), None, None)
            .expect("source"),
        expression: "12 mm".into(),
        unit: Some("mm".into()),
        name: state.id.clone(),
        evaluated_value: 1.2,
    })
    .expect("parameter");
    parameter.set_parameter_state(state.id.clone());
    let namespace = component.namespace_mut("f3d");
    namespace
        .set_arena(
            &ctx,
            "design_parameter_states",
            std::slice::from_ref(&state),
        )
        .expect("state");
    namespace
        .set_arena(
            &ctx,
            "design_parameter_state_companions",
            std::slice::from_ref(&companion),
        )
        .expect("companion");
    namespace
        .set_arena(
            &ctx,
            "design_parameter_document_headers",
            std::slice::from_ref(&header),
        )
        .expect("root header");
    namespace
        .set_arena(&ctx, "design_parameters", std::slice::from_ref(&parameter))
        .expect("parameter");
    let mut merged = Native::default();
    extend_native(
        &ctx,
        &mut merged,
        component.clone(),
        "one",
        &PartIdentityMap::default(),
    )
    .expect("first occurrence");
    extend_native(
        &ctx,
        &mut merged,
        component,
        "two",
        &PartIdentityMap::default(),
    )
    .expect("second occurrence");
    let native = merged.namespace("f3d").expect("native");
    let states = native
        .arena_as::<DesignParameterState>("design_parameter_states")
        .expect("states");
    let companions = native
        .arena_as::<DesignParameterStateCompanion>("design_parameter_state_companions")
        .expect("companions");
    let headers = native
        .arena_as::<DesignParameterDocumentHeader>("design_parameter_document_headers")
        .expect("headers");
    let parameters = native
        .arena_as::<DesignParameter>("design_parameters")
        .expect("parameters");
    assert_eq!(
        (
            states.len(),
            companions.len(),
            headers.len(),
            parameters.len()
        ),
        (2, 2, 2, 2)
    );
    for (index, occurrence) in ["one", "two"].into_iter().enumerate() {
        assert_eq!(
            states[index].id,
            format!("f3d:xref/{occurrence}/Design1:design-parameter-state#21")
        );
        assert_eq!(states[index].companion, companions[index].id);
        assert_eq!(companions[index].state, states[index].id);
        assert_eq!(states[index].document_root, headers[index].id);
        let wire = serde_json::to_value(&parameters[index]).expect("parameter wire");
        assert_eq!(wire["parameter_state"], states[index].id);
        assert_eq!(wire["name"], state.id);
        assert_eq!(wire["expression"], "12 mm");
        assert_eq!(wire["record_index"], 905);
    }
    assert_ne!(states[0].id, states[1].id);
}
