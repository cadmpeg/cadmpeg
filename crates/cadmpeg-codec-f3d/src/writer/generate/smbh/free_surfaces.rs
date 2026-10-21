// SPDX-License-Identifier: Apache-2.0
//! Saved current surface roots without fabricated face or body topology.

use super::*;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::schema::EntitySchema;

pub(super) fn encode(
    ctx: &DecodeContext<'_>,
    target: &CadIr,
    native: &F3dNative,
) -> Result<Vec<u8>, CodecError> {
    let model = &target.model;
    if !model.bodies.is_empty()
        || !model.regions.is_empty()
        || !model.shells.is_empty()
        || !model.faces.is_empty()
        || !model.loops.is_empty()
        || !model.coedges.is_empty()
        || !model.edges.is_empty()
        || !model.vertices.is_empty()
        || !model.points.is_empty()
    {
        return Err(CodecError::UnsupportedContent(
            "standalone F3D surface generation requires free construction geometry without topology".into(),
        ));
    }
    let roots = saved_free_surface_ids(ctx, model)?;
    if roots.is_empty() {
        return Err(CodecError::UnsupportedContent(
            "standalone F3D geometry has no supported saved surface root".into(),
        ));
    }
    // Native procedural records serialize their supports inline. Admit only
    // that actual dependency closure, never discard unrelated geometry while
    // claiming the entire current document was written.
    let mut closure = roots
        .iter()
        .map(|id| id.as_str().to_owned())
        .collect::<BTreeSet<_>>();
    let mut visited = BTreeSet::new();
    loop {
        let before = visited.len();
        macro_rules! visit { ($($field:ident),*) => {$({
            for record in &model.$field {
                if closure.contains(record.identity()) && visited.insert(record.identity().to_owned()) {
                    record.visit_references(ctx, &mut |id| { closure.insert(id.to_owned()); Ok(()) })?;
                }
            }
        })*}; }
        visit!(
            surfaces,
            curves,
            pcurves,
            procedural_surfaces,
            procedural_curves
        );
        if visited.len() == before {
            break;
        }
    }
    macro_rules! require { ($($field:ident),*) => {$({
        if let Some(record) = model.$field.iter().find(|record| !closure.contains(record.identity())) {
            return Err(CodecError::UnsupportedContent(format!(
                "standalone F3D surface roots cannot encode unrelated model.{} record {}", stringify!($field), record.identity()
            )));
        }
    })*}; }
    require!(
        surfaces,
        curves,
        pcurves,
        procedural_surfaces,
        procedural_curves
    );
    let mut bytes = native_smbh_header(target, roots.len())?;
    native_ident(&mut bytes, "asmheader")?;
    native_string(&mut bytes, "231.6.3.65535")?;
    bytes.push(0x11);
    for surface in model
        .surfaces
        .iter()
        .filter(|surface| roots.contains(&surface.id))
    {
        encode_surface_record(ctx, &mut bytes, target, surface)?;
    }
    native_history_tail(&mut bytes, native)?;
    Ok(bytes)
}
