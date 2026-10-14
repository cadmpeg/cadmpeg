// SPDX-License-Identifier: Apache-2.0
use crate::design::feature_project::native_scope_definition;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};

#[test]
fn native_scope_kind_keeps_source_name() {
    let kind = "source雪%";
    let scope = DesignParameterScope::empty(
        "f3d:test:design-parameter-scope#10",
        DesignFeatureKind::try_from(kind.to_owned()).unwrap(),
        10,
    );
    let definition = native_scope_definition(&scope, &[]).unwrap();
    assert!(
        matches!(definition, cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::Native {
            kind: cadmpeg_ir::features::NativeFeatureKind::Other(ref name), .. }) if name == kind)
    );
}
