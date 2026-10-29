// SPDX-License-Identifier: Apache-2.0
use crate::records::parameter_state::{DesignParameterState, DesignParameterStateCompanion};

fn state_findings(
    state_record_index: u64,
    keep_companion: bool,
) -> Vec<cadmpeg_ir::report::check::Finding> {
    crate::test_support::with_decode_context(|decode| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let mut native = crate::native::F3dNative::default();
        native.design_parameter_states.push(DesignParameterState {
            id: "f3d:Design1:design-parameter-state#61".into(),
            class_tag: "301".to_owned().try_into().unwrap(),
            record_index: 401,
            companion: "f3d:Design1:design-parameter-state-companion#122".into(),
            document_root: "f3d:Design1:design-parameter-document-header#0".into(),
        });
        if keep_companion {
            native
                .design_parameter_state_companions
                .push(DesignParameterStateCompanion {
                    id: "f3d:Design1:design-parameter-state-companion#122".into(),
                    class_tag: "302".to_owned().try_into().unwrap(),
                    record_index: 702,
                    state_record_index,
                    control: 1,
                    control_word: 1,
                });
        }
        let ctx = super::super::Ctx::new(&ir, &native, decode);
        let mut findings = Vec::new();
        super::super::validate_parameter_state_companions(&ctx, &mut findings);
        findings
    })
}

#[test]
fn a_state_companion_must_reference_its_state_entity_index() {
    assert!(state_findings(401, true).is_empty());
    let findings = state_findings(402, true);
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].entity.as_deref(),
        Some("f3d:Design1:design-parameter-state-companion#122")
    );
    // The check applies only when both sides are kept.
    assert!(state_findings(402, false).is_empty());
}
