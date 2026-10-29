// SPDX-License-Identifier: Apache-2.0
//! Neutral homes for model facts carried by native document metadata.

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::ProductDefinitionId;
use cadmpeg_ir::products::{ProductDefinition, ProductDefinitionKind};

use crate::native::F3dNative;

/// A standalone ACT root owns the local part when Design did not supply a
/// component graph. Its label remains available to neutral product tooling.
pub(super) fn standalone_product(ir: &mut CadIr, native: &F3dNative) {
    if !ir.scope.model || !ir.model.product_definitions.is_empty() {
        return;
    }
    let mut roots = native
        .act_component_links
        .iter()
        .filter(|link| link.tracks_document_root());
    let (Some(root), None) = (roots.next(), roots.next()) else {
        return;
    };
    let label = root.layout().display_name();
    ir.model.product_definitions.push(ProductDefinition {
        id: ProductDefinitionId::compose(
            &cadmpeg_ir::identity_namespace!("f3d", "model", "component"),
            cadmpeg_ir::identity_key!("act-root"),
        ),
        kind: ProductDefinitionKind::Part,
        source_name: None,
        label: (!label.is_empty()).then(|| label.to_owned()),
        description: None,
        part_number: None,
        bom_properties: std::collections::BTreeMap::default(),
        bodies: ir.model.bodies.iter().map(|body| body.id.clone()).collect(),
        sketches: Vec::new(),
        native_ref: Some(root.id().as_str().to_owned()),
    });
}
