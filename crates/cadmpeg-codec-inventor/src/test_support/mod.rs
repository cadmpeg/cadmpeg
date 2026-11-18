// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic Inventor byte-fixture builders and diagnostic readers for
//! crate tests.

pub(crate) mod coil;
pub(crate) mod combine;
pub(crate) mod rse_compression;
pub(crate) mod test_fixtures;
pub(crate) mod truncation;

pub(crate) mod sheet_metal;

pub(crate) mod mirror;

pub(crate) mod surface_extrusion;

pub(crate) mod sketch_curves;
pub(crate) mod sketch_relations;
pub(crate) mod split;
pub(crate) mod thicken;

pub(crate) mod thread;

pub(crate) fn root_part_ir() -> cadmpeg_ir::CadIr {
    use cadmpeg_ir::ids::ProductDefinitionId;
    use cadmpeg_ir::products::{ExternalDocument, ProductDefinition, ProductDefinitionKind};

    let id = ProductDefinitionId::mint("inventor:document:product#root").expect("root part id");
    let mut definition = ProductDefinition::root(id, ExternalDocument::default());
    definition.kind = Some(ProductDefinitionKind::Part);
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.product_definitions.push(definition);
    ir
}
