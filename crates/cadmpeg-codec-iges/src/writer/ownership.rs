// SPDX-License-Identifier: Apache-2.0
//! Preserve source dependency for carriers without standalone topology.

use super::EntityStatus;
use cadmpeg_ir::geometry::Surface;
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::{CadIr, CodecFormat, SourceGeometryRole, SourceObjectAssociation};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn report_unowned_support(losses: &mut Vec<super::LossNote>, id: &str) {
    let code = super::IgesLossCode::WriterSupportGeometryNotRepresented;
    losses.push(code.note(format!(
        "1 support geometry record ({id}) has no exported owner and was withheld"
    )));
}

/// Every Directory sequence a native record names, so a referenced carrier
/// writes under its owner instead of withholding as unowned support.
fn collect_directory_sequences(
    fields: &serde_json::Map<String, serde_json::Value>,
    owned: &mut BTreeSet<u32>,
) {
    fn walk(value: &serde_json::Value, owned: &mut BTreeSet<u32>) {
        match value {
            serde_json::Value::String(text) => {
                for prefix in ["iges:entity:directory#", "iges:presentation:annotation#D"] {
                    if let Some(sequence) = text
                        .strip_prefix(prefix)
                        .and_then(|sequence| sequence.parse::<u32>().ok())
                    {
                        owned.insert(sequence);
                    }
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    walk(value, owned);
                }
            }
            serde_json::Value::Object(fields) => {
                // Record identities and source_entity links name the record's
                // own entity and never count as ownership.
                for (key, value) in fields {
                    if matches!(key.as_str(), "id" | "source_entity") {
                        continue;
                    }
                    walk(value, owned);
                }
            }
            _ => {}
        }
    }
    for (key, value) in fields {
        if matches!(key.as_str(), "id" | "source_entity") {
            continue;
        }
        walk(value, owned);
    }
}

pub(super) struct SourceOwnership<'ir> {
    statuses: BTreeMap<u32, EntityStatus>,
    owned_surfaces: BTreeSet<&'ir SurfaceId>,
    owned: BTreeSet<u32>,
    annotation_use: BTreeSet<u32>,
}

impl<'ir> SourceOwnership<'ir> {
    pub(super) fn build(ir: &'ir CadIr) -> Self {
        let mut owned_surfaces = BTreeSet::new();
        for face in &ir.model.faces {
            owned_surfaces.insert(&face.surface);
        }
        let mut statuses = BTreeMap::new();
        for record in ir
            .native
            .namespace("iges")
            .into_iter()
            .filter_map(|namespace| namespace.arenas().get("entities"))
            .flatten()
        {
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
            statuses.insert(
                match record
                    .field("directory_sequence")
                    .and_then(|value| value.as_u64())
                    .and_then(|sequence| u32::try_from(sequence).ok())
                {
                    Some(sequence) => sequence,
                    None => continue,
                },
                status,
            );
        }
        let mut owned = BTreeSet::new();
        let mut annotation_use = BTreeSet::new();
        if let Some(namespace) = ir.native.namespace("iges") {
            for (arena, rows) in namespace.arenas() {
                if arena == "entities" {
                    for record in rows {
                        let Ok(fields) = record.fields() else {
                            continue;
                        };
                        let sequence = fields
                            .get("directory_sequence")
                            .and_then(serde_json::Value::as_u64)
                            .and_then(|sequence| u32::try_from(sequence).ok());
                        if fields.get("use_flag").and_then(serde_json::Value::as_i64) == Some(1) {
                            if let Some(sequence) = sequence {
                                annotation_use.insert(sequence);
                            }
                        }
                    }
                    continue;
                }
                for record in rows {
                    let Ok(fields) = record.fields() else {
                        continue;
                    };
                    collect_directory_sequences(&fields, &mut owned);
                }
            }
        }
        Self {
            statuses,
            owned_surfaces,
            owned,
            annotation_use,
        }
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
            physically_dependent(status)
        } else {
            status
        }
    }

    /// Whether the source directory sequence is named by another native
    /// record, in which case the carrier writes under its owner instead of
    /// withholding as unowned support.
    pub(super) fn is_owned(&self, source: Option<&SourceObjectAssociation>) -> bool {
        Self::sequence(source).is_some_and(|sequence| self.owned.contains(&sequence))
    }

    /// Whether the source directory entry carries the annotation use flag.
    /// An owned carrier with annotation use writes as annotation-use
    /// dependent so the re-decode admits it as annotation geometry.
    pub(super) fn is_annotation_use(&self, source: Option<&SourceObjectAssociation>) -> bool {
        Self::sequence(source).is_some_and(|sequence| self.annotation_use.contains(&sequence))
    }

    fn sequence(source: Option<&SourceObjectAssociation>) -> Option<u32> {
        source
            .filter(|source| source.format == CodecFormat::Iges)
            .and_then(|source| source.object_id.as_str().strip_prefix('D'))
            .and_then(|sequence| sequence.parse::<u32>().ok())
    }

    /// The status of a carrier that exported topology uses. A carrier with no
    /// stated geometry role is support of that topology; only an explicit
    /// `independent` role also makes it standalone geometry.
    pub(super) fn topology_status(&self, source: Option<&SourceObjectAssociation>) -> EntityStatus {
        let status = self.status(source);
        if source.and_then(|source| source.geometry_role).is_none() {
            physically_dependent(status)
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

pub(super) fn wrapped_surface_status(surface: &Surface, status: EntityStatus) -> EntityStatus {
    if surface
        .source_object
        .as_ref()
        .is_some_and(|source| source.format == cadmpeg_ir::CodecFormat::Iges)
    {
        status
    } else {
        EntityStatus::PhysicallyDependent
    }
}

/// `status` with physical dependence added.
const fn physically_dependent(status: EntityStatus) -> EntityStatus {
    match status {
        EntityStatus::Independent | EntityStatus::Definition => EntityStatus::PhysicallyDependent,
        EntityStatus::LogicallyDependent => EntityStatus::BothDependent,
        status => status,
    }
}
