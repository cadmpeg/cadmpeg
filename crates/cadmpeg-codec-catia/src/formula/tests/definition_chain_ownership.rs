// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::document::CadIr;

#[test]
fn standalone_class_selector_is_not_a_named_parameter() {
    for (role, suffix) in [
        (
            "Real",
            vec![
                0x84, 0x88, 0x82, 0x32, 4, 0, 0, 0, 0xe6, 0, 0, 0, 0, 0, 0, 0xf0, 0x3f, 0x81, 0x4a,
            ],
        ),
        ("Boolean", vec![0x84, 0x88, 0x82, 0x32, 4, 0, 0, 0, 0x81]),
    ] {
        let bytes = crate::test_support::test_formula::standard_catpart_with_definition_chain_type(
            role, &suffix,
        );
        let native = crate::native::CatiaNative::decode(&bytes);
        let retained = native
            .entity_records
            .iter()
            .find_map(|entity| entity.definition_chain_value())
            .expect("complete native value");
        assert_eq!(retained.selector.value, "FeatureFEDGE");
        assert_eq!(retained.role.value, role);
        let mut ir = CadIr::empty();
        crate::test_support::with_service_context(|ctx| {
            crate::formula::transfer_parameters(
                ctx,
                &mut ir,
                &native,
                &mut cadmpeg_ir::Annotations::default(),
                &crate::decode::ModelingGraphScope::Unscoped,
            )
        })
        .expect("retain raw value without a parameter");
        assert!(ir.model.parameters.is_empty());
        assert_eq!(
            native
                .entity_records
                .iter()
                .filter(|entity| entity.definition_chain_value().is_some())
                .count(),
            1
        );
    }
}
