// SPDX-License-Identifier: Apache-2.0
//! Preserve source dependency for carriers without standalone topology.

use super::EntityStatus;
use cadmpeg_ir::geometry::Surface;
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::schema::rewrite::typed::RewriteIdentities;
use cadmpeg_ir::{CadIr, CodecFormat, SourceGeometryRole, SourceObjectAssociation};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn report_unowned_support(losses: &mut Vec<super::LossNote>, id: &str) {
    let code = super::IgesLossCode::WriterSupportGeometryNotRepresented;
    losses.push(code.note(format!(
        "1 support geometry record ({id}) has no exported owner and was withheld"
    )));
}

/// Identities that another geometry or topology record names. The writer
/// writes such a carrier through that record. `None` when the encode depth
/// refuses a reference walk, so the named set is unknown.
fn neutral_children(ir: &CadIr) -> Option<BTreeSet<String>> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let ctx = cadmpeg_core::decode::DecodeContext::for_encode(&arena, &policy);
    let mut children = BTreeSet::new();
    macro_rules! visit {
        ($($arena:ident),* $(,)?) => {$(
            for record in &ir.model.$arena {
                let own = record.id.as_str();
                record
                    .visit_identity_references::<cadmpeg_core::decode::ResourceLimit>(
                        &ctx,
                        &mut |identity| {
                            if identity != own {
                                children.insert(identity.to_owned());
                            }
                            Ok(())
                        },
                    )
                    .ok()?;
            }
        )*};
    }
    visit!(
        curves,
        procedural_curves,
        surfaces,
        procedural_surfaces,
        vertices,
        edges,
        pcurves,
        coedges,
        loops,
        faces
    );
    Some(children)
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
    neutral_children: Option<BTreeSet<String>>,
    topology_free: bool,
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
            neutral_children: neutral_children(ir),
            topology_free: ir.model.faces.is_empty() && ir.model.pcurves.is_empty(),
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

    /// The status of `surface`. A physically dependent surface that no
    /// exported face uses writes as standalone geometry.
    pub(super) fn surface_status(&self, surface: &Surface) -> EntityStatus {
        let status = self.status(surface.source_object.as_ref());
        if self.is_unowned_support(surface) {
            without_physical_dependence(status)
        } else {
            status
        }
    }

    /// Whether `surface` is physically dependent in the source and no exported
    /// face uses it.
    pub(super) fn is_unowned_support(&self, surface: &Surface) -> bool {
        self.status(surface.source_object.as_ref())
            .is_physically_dependent()
            && !self.owned_surfaces.contains(&surface.id)
    }

    /// Whether a physically dependent carrier `id` without a written source
    /// parent writes as standalone geometry. A face or parameter curve can
    /// write a dependent carrier as its own part, and another geometry or
    /// topology record that names `id` writes it; only a document without
    /// faces and parameter curves leaves an unnamed carrier no other writer.
    pub(super) fn writes_standalone(&self, id: &str) -> bool {
        self.topology_free
            && self
                .neutral_children
                .as_ref()
                .is_some_and(|children| !children.contains(id))
    }

    /// Whether the carrier `id` of `ir` is physically dependent in the source,
    /// has no exported owner, and does not write standalone.
    pub(super) fn withholds(&self, ir: &CadIr, id: &str) -> bool {
        let source = ir
            .model
            .curves
            .iter()
            .find(|curve| curve.id.as_str() == id)
            .map(|curve| curve.source_object.as_ref())
            .or_else(|| {
                ir.model
                    .surfaces
                    .iter()
                    .find(|surface| surface.id.as_str() == id)
                    .map(|surface| surface.source_object.as_ref())
            })
            .or_else(|| {
                ir.model
                    .points
                    .iter()
                    .find(|point| point.id.as_str() == id)
                    .map(|point| point.source_object.as_ref())
            });
        let Some(source) = source else {
            return false;
        };
        self.status(source).is_physically_dependent()
            && !self.is_owned(source)
            && !self.writes_standalone(id)
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

/// `status` without physical dependence, for a surface that no exported face
/// uses.
const fn without_physical_dependence(status: EntityStatus) -> EntityStatus {
    match status {
        EntityStatus::PhysicallyDependent
        | EntityStatus::PhysicallyDependentEdgeList
        | EntityStatus::ParameterCurve => EntityStatus::Independent,
        EntityStatus::BothDependent => EntityStatus::LogicallyDependent,
        EntityStatus::AnnotationDependent => EntityStatus::Annotation,
        status => status,
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
