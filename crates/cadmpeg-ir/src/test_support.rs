// SPDX-License-Identifier: Apache-2.0
//! Shared IR fixture builders for `#[cfg(test)]` suites.

use crate::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use crate::ids::UnknownId;

/// Replace the surface of the cube's first face with an unknown surface,
/// optionally linking a preserved record, and return the face id and its
/// surface id. Leaves every loop/coedge/edge of the face intact.
pub(crate) fn make_first_face_surface_unknown(
    ir: &mut crate::CadIr,
    record: Option<UnknownId>,
) -> String {
    let face = &ir.geometry.faces[0];
    let surface_id = face.surface.as_str().to_owned();
    for s in &mut ir.geometry.surfaces {
        if s.id.as_str() == surface_id {
            s.geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record });
            break;
        }
    }
    surface_id
}

/// Adds the root definition that a document holding a model record states
/// (part 10, Products), listing the bodies no other definition lists. A
/// document that states a root keeps it.
pub(crate) fn add_root(ir: &mut crate::CadIr) {
    use crate::products::{ExternalDocument, ProductDefinition};
    if ir
        .model
        .product_definitions
        .iter()
        .any(ProductDefinition::is_root)
    {
        return;
    }
    let listed: std::collections::BTreeSet<&crate::ids::BodyId> = ir
        .model
        .product_definitions
        .iter()
        .flat_map(|definition| {
            definition.bodies.iter().chain(
                definition
                    .instance_bodies
                    .iter()
                    .flat_map(|entry| &entry.bodies),
            )
        })
        .collect();
    let mut root = ProductDefinition::root(
        "test:model:product-definition#document"
            .try_into()
            .expect("valid identity"),
        ExternalDocument::default(),
    );
    root.bodies = ir
        .geometry
        .bodies
        .iter()
        .map(|body| body.id.clone())
        .filter(|body| !listed.contains(body))
        .collect();
    ir.model.product_definitions.push(root);
    ir.model
        .product_definitions
        .sort_by(|left, right| left.id.cmp(&right.id));
}

pub(crate) mod admissibility;
pub(crate) mod edit;
pub(crate) mod evaluation_cycles;
pub(crate) mod nurbs;
pub(crate) mod wire;

/// Run one IR operation under the service decode policy.
pub(crate) fn with_service_decode_context<T>(
    operation: impl FnOnce(
        &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<T, cadmpeg_core::CodecError>,
) -> Result<T, cadmpeg_core::CodecError> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)?;
    operation(&ctx)
}
