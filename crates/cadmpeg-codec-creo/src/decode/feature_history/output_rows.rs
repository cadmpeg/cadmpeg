// SPDX-License-Identifier: Apache-2.0
//! Unique topology ownership and source-ordered generated surface outputs.

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::topology::{Face, Loop, Region, Shell};
use std::collections::{btree_map::Entry, BTreeMap};

pub(super) struct CoedgeOwners<'ir, 'ctx> {
    loops: BTreeMap<&'ir str, Option<&'ir Loop>>,
    faces: BTreeMap<&'ir str, Option<&'ir Face>>,
    _storage: ScopedReservation<'ctx>,
}

impl<'ir, 'ctx> CoedgeOwners<'ir, 'ctx> {
    pub(super) fn new(ctx: &'ctx DecodeContext<'_>, ir: &'ir CadIr) -> Result<Self, CodecError> {
        let mut storage = ctx.reserve_scoped(0, "creo selected coedge owner index")?;
        let mut loops = BTreeMap::new();
        for row in ctx.admit_iter(&ir.model.loops, "creo selected edge loops")? {
            match storage.with_storage(|| {
                ctx.entry_btree_map(
                    &mut loops,
                    row.id.as_str(),
                    "creo selected loop index nodes",
                )
            })? {
                Entry::Vacant(entry) => {
                    entry.insert(Some(row));
                }
                Entry::Occupied(mut entry) => {
                    entry.insert(None);
                }
            }
        }
        let mut faces = BTreeMap::new();
        if !loops.is_empty() {
            for row in ctx.admit_iter(&ir.model.faces, "creo selected loop face lookup")? {
                match storage.with_storage(|| {
                    ctx.entry_btree_map(
                        &mut faces,
                        row.id.as_str(),
                        "creo selected face index nodes",
                    )
                })? {
                    Entry::Vacant(entry) => {
                        entry.insert(Some(row));
                    }
                    Entry::Occupied(mut entry) => {
                        entry.insert(None);
                    }
                }
            }
        }
        Ok(Self {
            loops,
            faces,
            _storage: storage,
        })
    }

    pub(super) fn face(
        &self,
        ctx: &DecodeContext<'_>,
        owner_loop: &str,
    ) -> Result<Option<&'ir Face>, CodecError> {
        let Some(lp) = ctx
            .get_btree_map(
                &self.loops,
                owner_loop,
                "creo selected edge loop identity comparison",
            )?
            .copied()
            .flatten()
        else {
            return Ok(None);
        };
        Ok(ctx
            .get_btree_map(
                &self.faces,
                lp.face.as_str(),
                "creo selected loop face identity comparison",
            )?
            .copied()
            .flatten())
    }
}

pub(super) struct ShellRegions<'ir, 'ctx> {
    shells: BTreeMap<&'ir str, Option<&'ir Shell>>,
    regions: BTreeMap<&'ir str, Option<&'ir Region>>,
    _storage: ScopedReservation<'ctx>,
}

impl<'ir, 'ctx> ShellRegions<'ir, 'ctx> {
    pub(super) fn new(ctx: &'ctx DecodeContext<'_>, ir: &'ir CadIr) -> Result<Self, CodecError> {
        let mut storage = ctx.reserve_scoped(0, "creo output shell region index")?;
        let mut shells = BTreeMap::new();
        for row in ctx.admit_iter(&ir.model.shells, "creo output shell index rows")? {
            match storage.with_storage(|| {
                ctx.entry_btree_map(
                    &mut shells,
                    row.id.as_str(),
                    "creo output shell index nodes",
                )
            })? {
                Entry::Vacant(entry) => {
                    entry.insert(Some(row));
                }
                Entry::Occupied(mut entry) => {
                    entry.insert(None);
                }
            }
        }
        let mut regions = BTreeMap::new();
        if !shells.is_empty() {
            for row in ctx.admit_iter(&ir.model.regions, "creo output region index rows")? {
                match storage.with_storage(|| {
                    ctx.entry_btree_map(
                        &mut regions,
                        row.id.as_str(),
                        "creo output region index nodes",
                    )
                })? {
                    Entry::Vacant(entry) => {
                        entry.insert(Some(row));
                    }
                    Entry::Occupied(mut entry) => {
                        entry.insert(None);
                    }
                }
            }
        }
        Ok(Self {
            shells,
            regions,
            _storage: storage,
        })
    }

    pub(super) fn body(
        &self,
        ctx: &DecodeContext<'_>,
        shell: &str,
    ) -> Result<Option<&'ir BodyId>, CodecError> {
        let Some(shell) = ctx
            .get_btree_map(&self.shells, shell, "creo output shell identity lookup")?
            .copied()
            .flatten()
        else {
            return Ok(None);
        };
        Ok(ctx
            .get_btree_map(
                &self.regions,
                shell.region.as_str(),
                "creo output region identity lookup",
            )?
            .copied()
            .flatten()
            .map(|region| &region.body))
    }
}

pub(super) struct SurfaceOutputs<'ir, 'ctx> {
    ir: &'ir CadIr,
    bodies: BTreeMap<&'ir str, Vec<&'ir BodyId>>,
    built: bool,
    storage: ScopedReservation<'ctx>,
}

impl<'ir, 'ctx> SurfaceOutputs<'ir, 'ctx> {
    pub(super) fn new(ctx: &'ctx DecodeContext<'_>, ir: &'ir CadIr) -> Result<Self, CodecError> {
        Ok(Self {
            ir,
            bodies: BTreeMap::new(),
            built: false,
            storage: ctx.reserve_scoped(0, "creo surface output index")?,
        })
    }

    pub(super) fn bodies(
        &mut self,
        ctx: &'ctx DecodeContext<'_>,
        surface: &str,
    ) -> Result<Option<&[&'ir BodyId]>, CodecError> {
        if !self.built && !self.ir.model.faces.is_empty() {
            let owners = ShellRegions::new(ctx, self.ir)?;
            for face in
                ctx.admit_iter(&self.ir.model.faces, "creo generated surface face lookup")?
            {
                let Some(body) = owners.body(ctx, face.shell.as_str())? else {
                    continue;
                };
                let outputs = self
                    .storage
                    .with_storage(|| {
                        ctx.entry_btree_map(
                            &mut self.bodies,
                            face.surface.as_str(),
                            "creo surface output index nodes",
                        )
                    })?
                    .or_default();
                self.storage.with_storage(|| {
                    ctx.push_vec(outputs, body, "creo surface output body references")
                })?;
            }
        }
        self.built = true;
        Ok(ctx
            .get_btree_map(
                &self.bodies,
                surface,
                "creo generated surface identity comparison",
            )?
            .map(Vec::as_slice))
    }
}

#[cfg(test)]
mod tests {
    use super::SurfaceOutputs;
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::ids::{BodyId, FaceId, RegionId, ShellId, SurfaceId};
    use cadmpeg_ir::topology::{Face, FaceLoops, Region, Sense, Shell};

    fn source() -> CadIr {
        let mut ir = CadIr::empty();
        for (ordinal, surface, body) in [(0, 8, 2), (1, 9, 3), (2, 8, 1)] {
            let face = FaceId::mint(format!("creo:test:face#{ordinal}")).expect("face ID");
            let shell = ShellId::mint(format!("creo:test:shell#{ordinal}")).expect("shell ID");
            let region = RegionId::mint(format!("creo:test:region#{ordinal}")).expect("region ID");
            ir.model.faces.push(Face {
                id: face.clone(),
                shell: shell.clone(),
                surface: SurfaceId::mint(format!("creo:test:surface#{surface}"))
                    .expect("surface ID"),
                sense: Sense::Forward,
                loops: FaceLoops::unspecified(Vec::new()),
                name: None,
                color: None,
                tolerance: None,
            });
            ir.model
                .shells
                .push(Shell::with_face(shell.clone(), region.clone(), face));
            ir.model.regions.push(Region {
                id: region,
                body: BodyId::mint(format!("creo:test:body#{body}")).expect("body ID"),
                shells: vec![shell],
            });
        }
        ir
    }

    #[test]
    fn surface_outputs_preserve_face_order_across_cached_queries() {
        let ir = source();
        crate::decode::with_test_decode_ctx(|ctx| {
            let mut outputs = SurfaceOutputs::new(ctx, &ir)?;
            let expected = [&ir.model.regions[0].body, &ir.model.regions[2].body];
            assert_eq!(
                outputs.bodies(ctx, "creo:test:surface#8")?,
                Some(expected.as_slice())
            );
            assert_eq!(
                outputs.bodies(ctx, "creo:test:surface#9")?,
                Some([&ir.model.regions[1].body].as_slice())
            );
            assert_eq!(
                outputs.bodies(ctx, "creo:test:surface#8")?,
                Some(expected.as_slice())
            );
            assert_eq!(outputs.bodies(ctx, "creo:test:surface#10")?, None);
            Ok::<_, cadmpeg_core::CodecError>(())
        })
        .expect("service cached surface outputs");
    }

    #[test]
    fn surface_outputs_skip_region_index_without_shells() {
        let mut ir = source();
        ir.model.shells.clear();
        let region_query = std::cell::Cell::new(false);
        let bodies = crate::test_support::assert_work_boundaries(
            &["creo generated surface face lookup"],
            |ctx| {
                let mut outputs = SurfaceOutputs::new(ctx, &ir)?;
                let result = outputs.bodies(ctx, "creo:test:surface#8");
                if let Err(cadmpeg_core::CodecError::ResourceLimit(resource)) = &result {
                    if matches!(
                        resource.operation,
                        "creo output region index rows" | "creo output region index nodes"
                    ) {
                        region_query.set(true);
                    }
                }
                result.map(|bodies| bodies.is_some())
            },
        );
        assert!(!bodies);
        assert!(!region_query.get());
    }

    #[test]
    fn surface_outputs_reject_duplicate_shell_and_region_owners() {
        let mut ir = source();
        ir.model.shells.push(ir.model.shells[2].clone());
        ir.model.regions.push(ir.model.regions[1].clone());
        crate::decode::with_test_decode_ctx(|ctx| {
            let mut outputs = SurfaceOutputs::new(ctx, &ir)?;
            assert_eq!(
                outputs.bodies(ctx, "creo:test:surface#8")?,
                Some([&ir.model.regions[0].body].as_slice())
            );
            assert_eq!(outputs.bodies(ctx, "creo:test:surface#9")?, None);
            Ok::<_, cadmpeg_core::CodecError>(())
        })
        .expect("service duplicate ownership queries");
    }
}
