// SPDX-License-Identifier: Apache-2.0
//! Preserve source dependency for carriers without standalone topology.

use super::EntityStatus;
use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::{geometry::Curve, CadIr, CodecFormat};
use std::collections::BTreeMap;

pub(super) struct SourceCurveOwnership {
    statuses: BTreeMap<u32, EntityStatus>,
}

impl SourceCurveOwnership {
    pub(super) fn build(ctx: &DecodeContext<'_>, ir: &CadIr) -> Result<Self, CodecError> {
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
        Ok(Self { statuses })
    }

    pub(super) fn status(&self, curve: &Curve) -> EntityStatus {
        curve
            .source_object
            .as_ref()
            .filter(|source| source.format == CodecFormat::Iges)
            .and_then(|source| source.object_id.as_str().strip_prefix('D'))
            .and_then(|sequence| sequence.parse::<u32>().ok())
            .and_then(|sequence| self.statuses.get(&sequence))
            .copied()
            .unwrap_or_else(|| {
                if curve.source_object.as_ref().is_some_and(|source| {
                    source.geometry_role == Some(cadmpeg_ir::SourceGeometryRole::Support)
                }) {
                    EntityStatus::PhysicallyDependent
                } else {
                    EntityStatus::Independent
                }
            })
    }
}
