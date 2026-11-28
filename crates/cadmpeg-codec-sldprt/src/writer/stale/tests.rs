// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodeDepth, DecodePolicy};

/// The stale mark edits the stored text of a document whose external subset
/// declares an entity the text references.
#[test]
fn the_stale_mark_keeps_the_text_of_a_document_with_an_unread_external_subset() {
    let source = r#"<!DOCTYPE swSolidWorks SYSTEM 'sw.dtd'><swSolidWorks><swModel swNote="&units;"/><swConfiguration swName="A" swConfigurationNeedsUpdate="NO"/><swConfiguration swName="B" swID="1"/></swSolidWorks>"#;
    let expected = r#"<!DOCTYPE swSolidWorks SYSTEM 'sw.dtd'><swSolidWorks><swModel swNote="&units;"/><swConfiguration swName="A" swConfigurationNeedsUpdate="YES"/><swConfiguration swName="B" swConfigurationNeedsUpdate="YES" swID="1"/></swSolidWorks>"#;
    let names = BTreeSet::from(["A".to_owned(), "B".to_owned()]);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    let (marked, rows) = super::mark(&ctx, source.as_bytes(), &names)
        .unwrap()
        .unwrap();
    assert_eq!(rows, ["A", "B"]);
    assert_eq!(String::from_utf8(marked).unwrap(), expected);
}

#[test]
fn stale_current_state_marks_active_configuration() {
    use cadmpeg_ir::design::evaluation::{CurrentState, EvaluationStatus};
    use cadmpeg_ir::design::refs::{Member, Position};
    use cadmpeg_ir::design::scope::DesignScope;
    use std::collections::BTreeMap;

    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.design.scopes.push(DesignScope {
        definition: cadmpeg_ir::ids::ProductDefinitionId::mint("test:model:product-definition#one")
            .unwrap(),
        roots: Vec::new(),
        active_configuration: None,
        active_arrangements: BTreeSet::new(),
        occurrences: BTreeMap::new(),
        unproduced: BTreeSet::new(),
        current: vec![CurrentState {
            at: Position::After(Member::Feature(
                cadmpeg_ir::features::FeatureId::mint("test:design:feature#one").unwrap(),
            )),
            bodies: Vec::new(),
            poses: BTreeMap::new(),
            status: EvaluationStatus::Stale,
            native_ref: None,
        }],
    });
    let states = super::StaleStates::of(&ir, &[]);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    let source = br#"<swSolidWorks><swConfiguration swName="Default"/></swSolidWorks>"#;
    let (output, marked) = super::mark(&ctx, source, &states.configurations)
        .unwrap()
        .unwrap();
    assert!(String::from_utf8(output)
        .unwrap()
        .contains("swConfigurationNeedsUpdate=\"YES\""));
    assert!(states.losses(&marked.into_iter().collect()).is_empty());
    assert!(states
        .losses(&BTreeSet::new())
        .iter()
        .any(|loss| loss.message.contains("test:design:feature#one")));
}
