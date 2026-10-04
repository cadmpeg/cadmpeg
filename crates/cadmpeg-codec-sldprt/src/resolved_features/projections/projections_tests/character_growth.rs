#[test]
fn absent_surface_component_character_propagates_work_refusal() {
    let components = [crate::records::FeatureInputComponentPathEntry {
        instance: None, type_signature: [0; 12], local_id: None,
    }];
    let format = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        super::super::format_surface_path_set(ctx, 1, |_| &components, "set:", "format surface test path")
    };
    assert_eq!(format(&cadmpeg_test_support::service_decode_context()).unwrap(),
        "sldprt:feature-input:surface-component-ids:_");
    crate::test_support::work_refusal_at("format SLDPRT absent surface component", format);
}
