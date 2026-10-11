// SPDX-License-Identifier: Apache-2.0
//! Surface selection by configuration body ancestry.

use std::collections::HashSet;

pub(super) fn configuration_surface_carriers(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &cadmpeg_ir::CadIr,
    configuration_index: usize,
) -> Result<Vec<cadmpeg_ir::geometry::Surface>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "copy SLDPRT configuration surface carriers";
    let configuration = &ir.model.configurations[configuration_index];
    let Some(body_ids) = configuration.bodies.as_deref() else {
        // Unresolved membership uses every neutral surface carrier.
        let mut surfaces = Vec::new();
        ctx.reserve_capacity(&mut surfaces, ir.model.surfaces.len(), OPERATION)?;
        for surface in ctx.admit_iter(
            &ir.model.surfaces,
            "scan SLDPRT configuration_surface_carriers values",
        )? {
            ctx.push_vec(
                &mut (surfaces),
                surface.try_clone_for_decode(ctx, OPERATION)?,
                OPERATION,
            )?;
        }
        return Ok(surfaces);
    };
    let mut bodies_storage =
        ctx.reserve_scoped(0, "SLDPRT temporary configuration surface ancestry")?;
    let mut bodies = HashSet::new();
    for id in ctx.admit_iter(
        body_ids,
        "scan SLDPRT configuration_surface_carriers values",
    )? {
        bodies_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut bodies,
                id,
                "index SLDPRT configuration surface ancestry",
            )
        })?;
    }
    let mut regions_storage =
        ctx.reserve_scoped(0, "SLDPRT temporary configuration surface ancestry")?;
    let mut regions = HashSet::new();
    for body in ctx.admit_iter(
        &ir.model.bodies,
        "scan SLDPRT configuration_surface_carriers values",
    )? {
        if ctx.contains_hash_set(
            &bodies,
            &body.id,
            "match SLDPRT configuration surface ancestry",
        )? {
            for id in ctx.admit_iter(
                &body.regions,
                "scan SLDPRT configuration_surface_carriers values",
            )? {
                regions_storage.with_storage(|| {
                    ctx.insert_hash_set(
                        &mut regions,
                        id,
                        "index SLDPRT configuration surface ancestry",
                    )
                })?;
            }
        }
    }
    drop(bodies);
    drop(bodies_storage);
    let mut shells_storage =
        ctx.reserve_scoped(0, "SLDPRT temporary configuration surface ancestry")?;
    let mut shells = HashSet::new();
    for region in ctx.admit_iter(
        &ir.model.regions,
        "scan SLDPRT configuration_surface_carriers values",
    )? {
        if ctx.contains_hash_set(
            &regions,
            &region.id,
            "match SLDPRT configuration surface ancestry",
        )? {
            for id in ctx.admit_iter(
                &region.shells,
                "scan SLDPRT configuration_surface_carriers values",
            )? {
                shells_storage.with_storage(|| {
                    ctx.insert_hash_set(
                        &mut shells,
                        id,
                        "index SLDPRT configuration surface ancestry",
                    )
                })?;
            }
        }
    }
    drop(regions);
    drop(regions_storage);
    let mut faces_storage =
        ctx.reserve_scoped(0, "SLDPRT temporary configuration surface ancestry")?;
    let mut faces = HashSet::new();
    for shell in ctx.admit_iter(
        &ir.model.shells,
        "scan SLDPRT configuration_surface_carriers values",
    )? {
        if ctx.contains_hash_set(
            &shells,
            &shell.id,
            "match SLDPRT configuration surface ancestry",
        )? {
            for id in ctx.admit_iter(shell.faces(), "scan SLDPRT topology members")? {
                faces_storage.with_storage(|| {
                    ctx.insert_hash_set(
                        &mut faces,
                        id,
                        "index SLDPRT configuration surface ancestry",
                    )
                })?;
            }
        }
    }
    drop(shells);
    drop(shells_storage);
    let mut surface_ids_storage =
        ctx.reserve_scoped(0, "SLDPRT temporary configuration surface ancestry")?;
    let mut surface_ids = HashSet::new();
    for face in ctx.admit_iter(
        &ir.model.faces,
        "scan SLDPRT configuration_surface_carriers values",
    )? {
        if ctx.contains_hash_set(
            &faces,
            &face.id,
            "match SLDPRT configuration surface ancestry",
        )? {
            surface_ids_storage.with_storage(|| {
                ctx.insert_hash_set(
                    &mut surface_ids,
                    &face.surface,
                    "index SLDPRT configuration surface ancestry",
                )
            })?;
        }
    }
    drop(faces);
    drop(faces_storage);
    let mut surfaces = Vec::new();
    for surface in ctx.admit_iter(
        &ir.model.surfaces,
        "scan SLDPRT configuration_surface_carriers values",
    )? {
        if ctx.contains_hash_set(
            &surface_ids,
            &surface.id,
            "match SLDPRT configuration surface ancestry",
        )? {
            let surface = surface.try_clone_for_decode(ctx, OPERATION)?;
            ctx.push_vec(&mut surfaces, surface, OPERATION)?;
        }
    }
    Ok(surfaces)
}

#[cfg(test)]
mod tests;
