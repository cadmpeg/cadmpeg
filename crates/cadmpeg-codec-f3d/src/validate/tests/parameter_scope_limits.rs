// SPDX-License-Identifier: Apache-2.0

fn native() -> crate::native::F3dNative {
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    crate::native::F3dNative {
        design_parameter_scopes: vec![DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#1",
            DesignFeatureKind::Fillet,
            1,
        )],
        ..Default::default()
    }
}

#[test]
fn invalid_parameter_scope_preserves_finding() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = native();
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
        let mut findings = Vec::new();
        super::super::validate_parameter_scopes(&ctx, &mut findings).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].message,
            "Fusion Design parameter scope has an invalid paired frame"
        );
    })
}
