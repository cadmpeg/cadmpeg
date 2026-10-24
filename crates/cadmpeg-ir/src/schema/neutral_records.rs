// SPDX-License-Identifier: Apache-2.0
//! Borrowed registered-record indices and exact full-record projection checks.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::document::Model;
use crate::ids::{CurveId, SurfaceId};
use crate::schema::EntitySchema;

struct Identity {
    arena: &'static str,
    bytes: Vec<u8>,
}

/// Actual identities captured from a registered producer population.
///
/// This is transient working state, not a document field or an allowance.
pub struct RecordIdentities {
    records: Vec<Identity>,
    distinct: bool,
}

impl RecordIdentities {
    /// Borrow actual captured identity bytes without interpreting their spelling.
    pub fn iter(&self) -> impl Iterator<Item = (&'static str, &[u8])> {
        self.records
            .iter()
            .map(|record| (record.arena, record.bytes.as_slice()))
    }

    /// Whether the actual captured population contains this complete identity.
    #[must_use]
    pub fn contains(&self, arena: &str, id: &str) -> bool {
        self.get(arena, id).is_some()
    }

    /// Borrow the actual stored identity after an exact arena and byte lookup.
    #[must_use]
    pub fn get(&self, arena: &str, id: &str) -> Option<&[u8]> {
        let index = self
            .records
            .binary_search_by(|record| {
                record
                    .arena
                    .cmp(arena)
                    .then_with(|| record.bytes.as_slice().cmp(id.as_bytes()))
            })
            .ok()?;
        Some(self.records[index].bytes.as_slice())
    }
}

/// Actual registered record identities grouped by their declared native reference.
pub struct NativeReferenceGroups<'a> {
    groups: Vec<(&'a str, RecordIdentities)>,
}
impl NativeReferenceGroups<'_> {
    /// Records with this exact declared reference, when any are actually held.
    #[must_use]
    pub fn get(&self, native: &str) -> Option<&RecordIdentities> {
        self.groups
            .binary_search_by_key(&native, |(key, _)| *key)
            .ok()
            .map(|at| &self.groups[at].1)
    }
}

struct Index<'a> {
    arena: &'static str,
    id: &'a str,
    at: usize,
}

/// Actual neutral identities indexed without copying record values.
///
/// Ambiguous duplicate identities never prove a comparison. Document validity
/// remains the input framework's responsibility; this index issues no finding.
pub struct ModelRecordIndex<'a> {
    model: &'a Model,
    records: Vec<Index<'a>>,
    distinct: bool,
    curve_owners: Vec<(&'a str, &'a CurveId)>,
    surface_owners: Vec<(&'a str, &'a SurfaceId)>,
}

impl<'a> ModelRecordIndex<'a> {
    /// Build fallible working metadata under the caller's account.
    pub fn new(ctx: &DecodeContext<'_>, model: &'a Model) -> Result<Self, CodecError> {
        let mut records = Vec::new();
        ctx.reserve_working(&mut records, 0, "neutral record identity index")?;
        macro_rules! index {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {$(
                ctx.reserve_working(&mut records, model.$field.len(), "neutral record identity index")?;
                for (at, record) in model.$field.iter().enumerate() {
                    records.push(Index { arena: stringify!($field), id: record.identity(), at });
                }
            )*};
        }
        crate::document::arena_registry!(index);
        records.sort_unstable_by(|left, right| left.id.cmp(right.id));
        let distinct = !records.windows(2).any(|pair| pair[0].id == pair[1].id);
        records.sort_unstable_by(|left, right| (left.arena, left.id).cmp(&(right.arena, right.id)));
        let mut curve_owners = Vec::new();
        let curves = model.curves.iter().filter_map(|carrier| {
            carrier
                .geometry
                .procedural_construction()
                .map(|id| (id.as_str(), &carrier.id))
        });
        ctx.reserve_working(
            &mut curve_owners,
            curves.clone().count(),
            "neutral procedural curve owners",
        )?;
        curve_owners.extend(curves);
        curve_owners.sort_unstable_by_key(|(id, _)| *id);
        let mut surface_owners = Vec::new();
        let surfaces = model.surfaces.iter().filter_map(|carrier| {
            carrier
                .geometry
                .procedural_construction()
                .map(|id| (id.as_str(), &carrier.id))
        });
        ctx.reserve_working(
            &mut surface_owners,
            surfaces.clone().count(),
            "neutral procedural surface owners",
        )?;
        surface_owners.extend(surfaces);
        surface_owners.sort_unstable_by_key(|(id, _)| *id);
        Ok(Self {
            model,
            records,
            distinct,
            curve_owners,
            surface_owners,
        })
    }

    /// The same borrowed model whose actual records were indexed.
    #[must_use]
    pub const fn model(&self) -> &'a Model {
        self.model
    }

    fn position(&self, arena: &str, id: &[u8]) -> Option<usize> {
        self.records
            .binary_search_by(|record| {
                record
                    .arena
                    .cmp(arena)
                    .then_with(|| record.id.as_bytes().cmp(id))
            })
            .ok()
            .map(|at| self.records[at].at)
    }

    /// Capture actual IDs as fallible working state before later enrichment.
    pub fn owned_identities(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<RecordIdentities, CodecError> {
        let mut identities = Vec::new();
        ctx.reserve_working(
            &mut identities,
            self.records.len(),
            "neutral projection identities",
        )?;
        for record in &self.records {
            let mut bytes = Vec::new();
            ctx.reserve_working(&mut bytes, record.id.len(), "neutral projection identity")?;
            bytes.extend_from_slice(record.id.as_bytes());
            identities.push(Identity {
                arena: record.arena,
                bytes,
            });
        }
        Ok(RecordIdentities {
            records: identities,
            distinct: self.distinct,
        })
    }

    /// Capture declared native-reference groups once without copying record values.
    ///
    /// The owning producer must establish whether these references cover its
    /// whole projection; records without a native reference are not inferred.
    pub fn native_reference_groups(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<NativeReferenceGroups<'a>, CodecError> {
        let mut referenced = Vec::new();
        ctx.reserve_working(&mut referenced, 0, "neutral native-reference groups")?;
        macro_rules! collect {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {$(
                for record in &self.model.$field {
                    if let Some(native) = record.native_reference() {
                        if referenced.len() == referenced.capacity() {
                            let additional = referenced.len().max(1);
                            ctx.reserve_working(&mut referenced, additional, "neutral native-reference groups")?;
                        }
                        referenced.push((native, stringify!($field), record.identity()));
                    }
                }
            )*};
        }
        crate::document::arena_registry!(collect);
        referenced.sort_unstable();
        let mut groups = Vec::new();
        ctx.reserve_working(&mut groups, 0, "neutral native-reference populations")?;
        let mut at = 0;
        while let Some((native, _, _)) = referenced.get(at).copied() {
            let end = at + referenced[at..].partition_point(|(value, _, _)| *value == native);
            let mut records = Vec::new();
            ctx.reserve_working(
                &mut records,
                end - at,
                "neutral native-reference identities",
            )?;
            for (_, arena, id) in &referenced[at..end] {
                let mut bytes = Vec::new();
                ctx.reserve_working(&mut bytes, id.len(), "neutral native-reference identity")?;
                bytes.extend_from_slice(id.as_bytes());
                records.push(Identity {
                    arena: *arena,
                    bytes,
                });
            }
            if groups.len() == groups.capacity() {
                let additional = groups.len().max(1);
                ctx.reserve_working(
                    &mut groups,
                    additional,
                    "neutral native-reference populations",
                )?;
            }
            groups.push((
                native,
                RecordIdentities {
                    records,
                    distinct: self.distinct,
                },
            ));
            at = end;
        }
        Ok(NativeReferenceGroups { groups })
    }

    /// Compare complete typed records at the producer's actual captured IDs.
    ///
    /// Both sides must contain every captured ID. No field is stripped and no
    /// value tolerance or ID remapping applies. The owner separately checks
    /// additional current native-reference claims over the complete registry.
    pub fn agrees_with_projection(
        &self,
        ctx: &DecodeContext<'_>,
        projection: &Model,
        identities: &RecordIdentities,
    ) -> Result<bool, CodecError> {
        let expected = ModelRecordIndex::new(ctx, projection)?;
        self.agrees_with_index(ctx, &expected, identities)
    }

    /// Whether any captured actual producer ID is still held in this model.
    pub fn contains_any(
        &self,
        ctx: &DecodeContext<'_>,
        identities: &RecordIdentities,
    ) -> Result<bool, CodecError> {
        if let Some(refusal) = ctx.resource_refusal() {
            return Err(refusal.into());
        }
        Ok(identities
            .records
            .iter()
            .any(|record| self.position(record.arena, &record.bytes).is_some()))
    }

    /// Reuse both complete borrowed indices while checking only captured IDs.
    pub fn agrees_with_index(
        &self,
        ctx: &DecodeContext<'_>,
        projection: &ModelRecordIndex<'_>,
        identities: &RecordIdentities,
    ) -> Result<bool, CodecError> {
        if let Some(refusal) = ctx.resource_refusal() {
            return Err(refusal.into());
        }
        if !self.distinct || !projection.distinct || !identities.distinct {
            return Ok(false);
        }
        for record in &identities.records {
            let Some(expected) = projection.position(record.arena, &record.bytes) else {
                return Ok(false);
            };
            let Some(current) = self.position(record.arena, &record.bytes) else {
                return Ok(false);
            };
            let mut same = false;
            macro_rules! compare {
                ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {$(
                    if record.arena == stringify!($field) {
                        same = self.model.$field.get(current) == projection.model.$field.get(expected);
                    }
                )*};
            }
            crate::document::arena_registry!(compare);
            if !same {
                return Ok(false);
            }
            if record.arena == "procedural_curves" {
                let Some(current) = self.model.procedural_curves.get(current) else {
                    return Ok(false);
                };
                let Some(expected) = projection.model.procedural_curves.get(expected) else {
                    return Ok(false);
                };
                let Some(owner) = unique_owner(&self.curve_owners, current.id.as_str()) else {
                    return Ok(false);
                };
                if Some(owner) != unique_owner(&projection.curve_owners, expected.id.as_str()) {
                    return Ok(false);
                }
            }
            if record.arena == "procedural_surfaces" {
                let Some(current) = self.model.procedural_surfaces.get(current) else {
                    return Ok(false);
                };
                let Some(expected) = projection.model.procedural_surfaces.get(expected) else {
                    return Ok(false);
                };
                let Some(owner) = unique_owner(&self.surface_owners, current.id.as_str()) else {
                    return Ok(false);
                };
                if Some(owner) != unique_owner(&projection.surface_owners, expected.id.as_str()) {
                    return Ok(false);
                }
            }
            if record.arena == "features" {
                let Some(current) = self.model.features.get(current) else {
                    return Ok(false);
                };
                let Some(expected) = projection.model.features.get(expected) else {
                    return Ok(false);
                };
                if self.model.feature_regeneration_parent(&current.id)
                    != projection.model.feature_regeneration_parent(&expected.id)
                {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
}

fn unique_owner<'a, T>(owners: &[(&str, &'a T)], id: &str) -> Option<&'a T> {
    let first = owners.partition_point(|(construction, _)| *construction < id);
    let end = owners.partition_point(|(construction, _)| *construction <= id);
    (end.checked_sub(first) == Some(1)).then(|| owners[first].1)
}

#[cfg(test)]
mod tests;
