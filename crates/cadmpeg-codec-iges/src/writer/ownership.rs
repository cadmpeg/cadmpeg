// SPDX-License-Identifier: Apache-2.0
//! Preserve source dependency for carriers without standalone topology.

use super::EntityStatus;
use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::geometry::Surface;
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::{CadIr, CodecFormat, SourceGeometryRole, SourceObjectAssociation};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct SourceOwnership<'ir> {
    statuses: BTreeMap<u32, EntityStatus>,
    owned_surfaces: BTreeSet<&'ir SurfaceId>,
}

impl<'ir> SourceOwnership<'ir> {
    pub(super) fn build(ctx: &DecodeContext<'_>, ir: &'ir CadIr) -> Result<Self, CodecError> {
        let mut owned_surfaces = BTreeSet::new();
        for face in &ir.model.faces {
            ctx.charge_work(1, "iges surface ownership scan")?;
            ctx.insert_btree_set(
                &mut owned_surfaces,
                &face.surface,
                "iges surface ownership index",
            )?;
        }
        let mut statuses = BTreeMap::new();
        for record in ir
            .native
            .namespace("iges")
            .into_iter()
            .filter_map(|namespace| namespace.arenas().get("entities"))
            .flatten()
        {
            ctx.charge_work(1, "iges carrier ownership scan")?;
            let status = match record
                .field("subordinate_status")
                .and_then(|value| value.as_i64())
            {
                Some(1) if record.field("use_flag").and_then(|value| value.as_i64()) == Some(5) => {
                    EntityStatus::ParameterCurve
                }
                Some(1) => EntityStatus::PhysicallyDependent,
                Some(2) => EntityStatus::LogicallyDependent,
                Some(3) => EntityStatus::BothDependent,
                _ => continue,
            };
            ctx.insert_btree_map(
                &mut statuses,
                match record
                    .field("directory_sequence")
                    .and_then(|value| value.as_u64())
                    .and_then(|sequence| u32::try_from(sequence).ok())
                {
                    Some(sequence) => sequence,
                    None => continue,
                },
                status,
                "iges carrier ownership index",
            )?;
        }
        Ok(Self {
            statuses,
            owned_surfaces,
        })
    }

    pub(super) fn status(&self, source: Option<&SourceObjectAssociation>) -> EntityStatus {
        let status = source
            .filter(|source| source.format == CodecFormat::Iges)
            .and_then(|source| source.object_id.as_str().strip_prefix('D'))
            .and_then(|sequence| sequence.parse::<u32>().ok())
            .and_then(|sequence| self.statuses.get(&sequence))
            .copied()
            .unwrap_or(EntityStatus::Independent);
        if source.is_some_and(|source| source.geometry_role == Some(SourceGeometryRole::Support)) {
            match status {
                EntityStatus::Independent | EntityStatus::Definition => {
                    EntityStatus::PhysicallyDependent
                }
                EntityStatus::LogicallyDependent => EntityStatus::BothDependent,
                status => status,
            }
        } else {
            status
        }
    }

    pub(super) fn surface_status(&self, surface: &Surface) -> Option<EntityStatus> {
        let status = self.status(surface.source_object.as_ref());
        // A dependent flag without an exported owner is still transferred as
        // standalone geometry by some importers. Withhold that orphan instead.
        (!status.is_physically_dependent() || self.owned_surfaces.contains(&surface.id))
            .then_some(status)
    }
}
