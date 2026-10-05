// SPDX-License-Identifier: Apache-2.0
//! Parse dimension recipe, locus, and annotation frames.

use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::index_from_u32;

use crate::bytes::lp_ascii_filtered_view;
use crate::container::ContainerScan;
use crate::design::construction_recipe_family_name_len;
use crate::design::decode::byte_fields::{bytes_at, zeros_at};
use crate::design::decode::meta::stream_types_by_entity;
use crate::design::decode::record_streams::{in_stream, record_stream};
use crate::design::decode::reference_runs::admit_reference_values;
use crate::design::decode::sketch::{
    indexed_record_header_at, indexed_record_offsets, next_indexed_record_header,
    next_indexed_record_offset, sorted_distinct, IndexedRecordHeader, IndexedRecordOffsets,
    RecordOffsetCache, BULK_STREAM_FILE, META_STREAM_FILE,
};
use crate::design::decode::text::design_record_id_charged;
use crate::layout::grouped_recipe_reference_prefix as grouped_recipe;
use crate::records::{
    decal::DesignRecordHeader,
    dimensions::{
        DesignDimensionAnnotationFrame, DesignDimensionAnnotationOperand, DesignDimensionLocus,
        DesignDimensionLocusGroup, DesignDimensionLocusPair, DesignDimensionPresentationFrame,
        DesignDimensionPresentationOperand, DesignDimensionRecipeRecord,
    },
    entity_header::{DesignEntityHeader, SegmentType},
    feature::scope::DesignParameterScope,
    parameters::{
        DesignParameter, DesignParameterCompanion, DesignParameterKind, DesignParameterOwner,
    },
    recipes::ConstructionRecipe,
    sketch_geometry::{SketchCurveIdentity, SketchPoint},
    sketch_links::PersistentSubentityTag,
    sketch_placement::DesignSketchPlacement,
    topology::edge_identity::DesignEdgeOperand,
};
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::decode::ScopedReservation;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::num::NonZeroU32;

/// Record slices every dimension-record decode pass reads: the container scan
/// plus the parameter, owner, companion, scope, record-header, and sketch
/// geometry tables that locate each dimension's owning companion and geometry.
pub(crate) struct DimensionDecodeInputs<'a> {
    pub(crate) scan: &'a ContainerScan<'a>,
    pub(crate) placements: &'a [DesignSketchPlacement],
    pub(crate) parameters: &'a [DesignParameter],
    pub(crate) owners: &'a [DesignParameterOwner],
    pub(crate) companions: &'a [DesignParameterCompanion],
    pub(crate) scopes: &'a [DesignParameterScope],
    pub(crate) headers: &'a [DesignRecordHeader],
    pub(crate) points: &'a [SketchPoint],
    pub(crate) curves: &'a [SketchCurveIdentity],
}

/// Parameters keyed by stream scope and record index. A later record with the
/// same key replaces an earlier one.
type ParameterIndex<'a> = HashMap<(&'a str, u32), &'a DesignParameter>;

/// Distinct record keys by stream scope and record index, ascending.
type StreamRecordKeys<'a> = Vec<(&'a str, u32)>;

/// Sort `keys` and drop repeated keys.
fn sort_record_keys(
    ctx: &DecodeContext<'_>,
    keys: &mut StreamRecordKeys<'_>,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.stable_sort_by(&mut keys[..], |key| key, Ord::cmp, operation)?;
    ctx.dedup_vec(keys, operation)
}

/// Whether the sorted, distinct `keys` hold `key`.
fn has_record_key(
    ctx: &DecodeContext<'_>,
    keys: &[(&str, u32)],
    key: (&str, u32),
    operation: &'static str,
) -> Result<bool, CodecError> {
    Ok(ctx.binary_search(keys, &key, operation)?.is_ok())
}

/// Index `parameters` for one decode pass under the returned reservation.
fn dimension_parameter_index<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    parameters: &'a [DesignParameter],
    operation: &'static str,
) -> Result<(ParameterIndex<'a>, ScopedReservation<'ctx>), CodecError> {
    ctx.with_scoped_storage(operation, || {
        let mut index = HashMap::new();
        for parameter in ctx.admit_iter(parameters, operation)? {
            let Some(stream) = record_stream(ctx, &parameter.id)? else {
                continue;
            };
            ctx.insert_hash_map(
                &mut index,
                (stream, parameter.record_index),
                parameter,
                operation,
            )?;
        }
        Ok(index)
    })
}

/// Whether the parameter `record_index` of `stream` is a dimension.
fn is_dimension_parameter(
    ctx: &DecodeContext<'_>,
    parameters: &ParameterIndex<'_>,
    stream: &str,
    record_index: u32,
) -> Result<bool, CodecError> {
    Ok(ctx
        .get_hash_map(
            parameters,
            &(stream, record_index),
            "find F3D dimension parameter",
        )?
        .is_some_and(|parameter| parameter.kind() == DesignParameterKind::Dimension))
}

/// Companions, by stream scope and record index, that an owner of a
/// dimensional parameter names, under the returned reservation.
fn dimension_companion_keys<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    owners: &'a [DesignParameterOwner],
    parameters: &ParameterIndex<'_>,
    operation: &'static str,
) -> Result<(StreamRecordKeys<'a>, ScopedReservation<'ctx>), CodecError> {
    ctx.with_scoped_storage(operation, || {
        let mut keys = Vec::new();
        for owner in ctx.admit_iter(owners, operation)? {
            let Some(stream) = record_stream(ctx, owner.id())? else {
                continue;
            };
            if is_dimension_parameter(ctx, parameters, stream, owner.parameter_record_index())? {
                ctx.push_vec(
                    &mut keys,
                    (stream, owner.companion_record_index()),
                    operation,
                )?;
            }
        }
        sort_record_keys(ctx, &mut keys, operation)?;
        Ok(keys)
    })
}

/// Sorted, distinct record-index sets of one kind, each built for a stream on
/// first use and held until the decode pass ends.
struct StreamIndexSets<'a, 'ctx> {
    sets: HashMap<&'a str, Vec<u32>>,
    storage: ScopedReservation<'ctx>,
}

impl<'a, 'ctx> StreamIndexSets<'a, 'ctx> {
    fn new(ctx: &'ctx DecodeContext<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            sets: HashMap::new(),
            storage: ctx.reserve_scoped(0, "f3d dimension stream index sets")?,
        })
    }

    fn get(
        &mut self,
        ctx: &DecodeContext<'_>,
        stream: &'a str,
        build: impl FnOnce() -> Result<Vec<u32>, CodecError>,
    ) -> Result<&[u32], CodecError> {
        let sets = &mut self.sets;
        self.storage.with_storage(move || {
            match ctx.entry_hash_map(sets, stream, "f3d dimension stream index sets")? {
                Entry::Occupied(entry) => Ok(entry.into_mut().as_slice()),
                Entry::Vacant(entry) => Ok(entry.insert(build()?).as_slice()),
            }
        })
    }
}

/// Whether the sorted, distinct `set` holds `value`.
fn set_contains(ctx: &DecodeContext<'_>, set: &[u32], value: u32) -> Result<bool, CodecError> {
    Ok(ctx
        .binary_search(set, &value, "find F3D dimension record index")?
        .is_ok())
}

/// Record indices of the sketch points and curves in `stream`.
fn dimension_geometry_indices(
    ctx: &DecodeContext<'_>,
    stream: &str,
    points: &[SketchPoint],
    curves: &[SketchCurveIdentity],
) -> Result<Vec<u32>, CodecError> {
    let mut indices = Vec::new();
    for point in ctx.admit_iter(points, "scan F3D dimension sketch points")? {
        if in_stream(ctx, &point.id, stream)? {
            ctx.push_vec(
                &mut indices,
                point.record_index,
                "f3d dimension geometry indices",
            )?;
        }
    }
    for curve in ctx.admit_iter(curves, "scan F3D dimension sketch curves")? {
        if in_stream(ctx, &curve.id, stream)? {
            ctx.push_vec(
                &mut indices,
                curve.record_index,
                "f3d dimension geometry indices",
            )?;
        }
    }
    sorted_distinct(ctx, &mut indices, "order F3D dimension geometry indices")?;
    Ok(indices)
}

/// Entity suffixes of the sketch-module entities in `stream`.
fn dimension_sketch_entities(
    ctx: &DecodeContext<'_>,
    stream: &str,
    entities: &[DesignEntityHeader],
) -> Result<Vec<u32>, CodecError> {
    let mut indices = Vec::new();
    for entity in ctx.admit_iter(entities, "scan F3D dimension sketch entities")? {
        if !entity.in_sketch_module() || !in_stream(ctx, &entity.id, stream)? {
            continue;
        }
        let Ok(index) = u32::try_from(entity.entity_id.suffix()) else {
            continue;
        };
        ctx.push_vec(&mut indices, index, "f3d dimension sketch entities")?;
    }
    sorted_distinct(ctx, &mut indices, "order F3D dimension sketch entities")?;
    Ok(indices)
}

/// A record header that a scope of its stream references: its offset, the
/// record index of the first referencing scope, and whether a scope with
/// another record index also references it.
#[derive(Clone, Copy)]
struct ReferencedHeader {
    offset: u64,
    first_scope: u32,
    other_scope: bool,
}

impl ReferencedHeader {
    /// Whether a scope other than `owning_scope` references the header.
    fn is_foreign(self, owning_scope: Option<u32>) -> bool {
        self.other_scope || owning_scope.is_none_or(|owning| self.first_scope != owning)
    }

    /// Whether `self` starts a new run after `previous`: either header has
    /// more than one referencing scope, or their first scopes differ.
    fn differs_from(self, previous: Self) -> bool {
        self.other_scope || previous.other_scope || self.first_scope != previous.first_scope
    }
}

/// The boundaries that end a parameter companion's owned interval in one
/// stream: the next owner, parameter, or scope, and the next record header
/// that a scope other than the companion's own references.
struct CompanionBoundaries {
    /// Scope record index of each owner record index; the first owner wins.
    owner_scopes: HashMap<u32, u32>,
    /// Byte offsets of the stream's owners, parameters, and scopes, ascending.
    offsets: Vec<u64>,
    /// The stream's scope-referenced record headers, ascending by offset.
    referenced_headers: Vec<ReferencedHeader>,
    /// Positions in `referenced_headers` where a run of headers that one and
    /// the same scope references begins, ascending. A header that more than
    /// one scope references is a run by itself.
    run_starts: Vec<usize>,
}

impl CompanionBoundaries {
    fn build<'a>(
        ctx: &DecodeContext<'_>,
        stream: &str,
        parameters: impl IntoIterator<Item = &'a DesignParameter>,
        owners: &[DesignParameterOwner],
        scopes: &[DesignParameterScope],
        headers: &[DesignRecordHeader],
    ) -> Result<Self, CodecError> {
        let mut owner_scopes = HashMap::new();
        let mut offsets = Vec::new();
        for owner in ctx.admit_iter(owners, "find F3D dimension companion owner")? {
            if !in_stream(ctx, owner.id(), stream)? {
                continue;
            }
            if let Entry::Vacant(slot) = ctx.entry_hash_map(
                &mut owner_scopes,
                owner.record_index(),
                "f3d companion owner scopes",
            )? {
                slot.insert(owner.scope_record_index());
            }
            ctx.push_vec(
                &mut offsets,
                owner.byte_offset(),
                "f3d companion boundaries",
            )?;
        }
        for parameter in parameters {
            if in_stream(ctx, &parameter.id, stream)? {
                ctx.push_vec(
                    &mut offsets,
                    parameter.byte_offset(),
                    "f3d companion boundaries",
                )?;
            }
        }
        // The record index of the first scope that references each member,
        // and whether a scope with another record index also references it.
        let mut referencing_scopes = HashMap::new();
        for scope in ctx.admit_iter(scopes, "scan F3D companion scopes")? {
            if !in_stream(ctx, &scope.id, stream)? {
                continue;
            }
            ctx.push_vec(
                &mut offsets,
                scope.byte_offset(),
                "f3d companion boundaries",
            )?;
            for &member in admit_reference_values(
                ctx,
                scope.reference_members(),
                "scan F3D companion scope references",
            )? {
                match ctx.get_mut_hash_map(
                    &mut referencing_scopes,
                    &member,
                    "f3d companion foreign scope members",
                )? {
                    Some((first, other)) => *other |= *first != scope.record_index,
                    None => {
                        ctx.insert_hash_map(
                            &mut referencing_scopes,
                            member,
                            (scope.record_index, false),
                            "f3d companion foreign scope members",
                        )?;
                    }
                }
            }
        }
        let mut referenced_headers = Vec::new();
        for header in ctx.admit_iter(headers, "scan F3D companion record headers")? {
            if !in_stream(ctx, &header.id, stream)? {
                continue;
            }
            let Some(&(first_scope, other_scope)) = ctx.get_hash_map(
                &referencing_scopes,
                &header.record_index,
                "find F3D companion foreign scope member",
            )?
            else {
                continue;
            };
            ctx.push_vec(
                &mut referenced_headers,
                ReferencedHeader {
                    offset: header.byte_offset,
                    first_scope,
                    other_scope,
                },
                "f3d companion record headers",
            )?;
        }
        ctx.stable_sort_by_key(
            &mut offsets[..],
            |offset| *offset,
            Ord::cmp,
            "order F3D companion boundaries",
        )?;
        ctx.stable_sort_by_key(
            &mut referenced_headers[..],
            |header| header.offset,
            Ord::cmp,
            "order F3D companion record headers",
        )?;
        let mut run_starts = Vec::new();
        let mut previous = None;
        for (position, header) in ctx
            .admit_iter(&referenced_headers, "group F3D companion record headers")?
            .enumerate()
        {
            if previous.is_none_or(|previous| header.differs_from(previous)) {
                ctx.push_vec(&mut run_starts, position, "f3d companion header runs")?;
            }
            previous = Some(*header);
        }
        Ok(Self {
            owner_scopes,
            offsets,
            referenced_headers,
            run_starts,
        })
    }

    /// The owned interval of `companion`: from 58 bytes after its offset to
    /// the first boundary after it, or to the stream end. Three bisections
    /// locate the boundary.
    fn interval(
        &self,
        ctx: &DecodeContext<'_>,
        companion: &DesignParameterCompanion,
        stream_length: usize,
    ) -> Result<Option<(usize, usize)>, CodecError> {
        let companion_at = companion.byte_offset();
        let owning_scope = ctx
            .get_hash_map(
                &self.owner_scopes,
                &companion.owner_record_index(),
                "find F3D dimension companion owner",
            )?
            .copied();
        let Some(start) = usize::try_from(companion_at)
            .ok()
            .and_then(|offset| offset.checked_add(58))
        else {
            return Ok(None);
        };
        let next = ctx.partition_point(
            &self.offsets,
            |offset| Ok(*offset <= companion_at),
            "find F3D companion boundary",
        )?;
        let boundary = self.offsets.get(next).copied();
        // Headers in one run share their referencing scope, so the first
        // foreign header is the first header after the companion, or the
        // start of the run that follows it.
        let first = ctx.partition_point(
            &self.referenced_headers,
            |header| Ok(header.offset <= companion_at),
            "find F3D companion header boundary",
        )?;
        let foreign_header = match self.referenced_headers.get(first) {
            Some(header) if header.is_foreign(owning_scope) => Some(header.offset),
            Some(_) => {
                let run = ctx.partition_point(
                    &self.run_starts,
                    |start| Ok(*start <= first),
                    "find F3D companion header boundary",
                )?;
                self.run_starts
                    .get(run)
                    .and_then(|start| self.referenced_headers.get(*start))
                    .map(|header| header.offset)
            }
            None => None,
        };
        let end = boundary
            .into_iter()
            .chain(foreign_header)
            .filter_map(|offset| usize::try_from(offset).ok())
            .min()
            .unwrap_or(stream_length);
        Ok((start <= end && end <= stream_length).then_some((start, end)))
    }
}

/// Companion boundaries of the records a pass reads, built per stream on
/// first use and held until the pass ends.
pub(super) struct CompanionIntervals<'a, 'ctx> {
    parameters: &'a [DesignParameter],
    owners: &'a [DesignParameterOwner],
    scopes: &'a [DesignParameterScope],
    headers: &'a [DesignRecordHeader],
    boundaries: HashMap<&'a str, CompanionBoundaries>,
    storage: ScopedReservation<'ctx>,
}

impl<'a, 'ctx> CompanionIntervals<'a, 'ctx> {
    pub(super) fn new(
        ctx: &'ctx DecodeContext<'_>,
        parameters: &'a [DesignParameter],
        owners: &'a [DesignParameterOwner],
        scopes: &'a [DesignParameterScope],
        headers: &'a [DesignRecordHeader],
    ) -> Result<Self, CodecError> {
        Ok(Self {
            parameters,
            owners,
            scopes,
            headers,
            boundaries: HashMap::new(),
            storage: ctx.reserve_scoped(0, "f3d companion boundaries")?,
        })
    }

    /// The owned interval of `companion`, of stream scope `stream`, in a
    /// stream of `stream_length` bytes, as [`companion_owned_interval`]
    /// states it.
    pub(super) fn interval(
        &mut self,
        ctx: &DecodeContext<'_>,
        stream: &'a str,
        companion: &DesignParameterCompanion,
        stream_length: usize,
    ) -> Result<Option<(usize, usize)>, CodecError> {
        let (parameters, owners, scopes, headers) =
            (self.parameters, self.owners, self.scopes, self.headers);
        let boundaries = &mut self.boundaries;
        let boundaries = self.storage.with_storage(move || {
            match ctx.entry_hash_map(boundaries, stream, "f3d companion boundaries")? {
                Entry::Occupied(entry) => Ok::<_, CodecError>(entry.into_mut()),
                Entry::Vacant(entry) => Ok(entry.insert(CompanionBoundaries::build(
                    ctx,
                    stream,
                    ctx.admit_iter(parameters, "scan F3D companion parameters")?,
                    owners,
                    scopes,
                    headers,
                )?)),
            }
        })?;
        boundaries.interval(ctx, companion, stream_length)
    }

    /// The intervals of the companion boundaries in `inputs`.
    fn of_inputs(
        ctx: &'ctx DecodeContext<'_>,
        inputs: &DimensionDecodeInputs<'a>,
    ) -> Result<Self, CodecError> {
        Self::new(
            ctx,
            inputs.parameters,
            inputs.owners,
            inputs.scopes,
            inputs.headers,
        )
    }
}

/// The owned interval of `companion` in a stream of `stream_length` bytes:
/// from 58 bytes after the companion to the first owner, parameter, or scope
/// after it, or the first record header after it that a scope other than the
/// companion's own references. Each call builds the stream's boundaries; a
/// pass over many companions holds a [`CompanionIntervals`].
#[cfg(test)]
pub(super) fn companion_owned_interval<'a>(
    ctx: &DecodeContext<'_>,
    companion: &DesignParameterCompanion,
    parameters: impl IntoIterator<Item = &'a DesignParameter>,
    owners: &[DesignParameterOwner],
    scopes: &[DesignParameterScope],
    headers: &[DesignRecordHeader],
    stream_length: usize,
) -> Result<Option<(usize, usize)>, CodecError> {
    let Some(stream) = record_stream(ctx, companion.id())? else {
        return Ok(None);
    };
    let (boundaries, _storage) = ctx.with_scoped_storage("f3d companion boundaries", || {
        CompanionBoundaries::build(ctx, stream, parameters, owners, scopes, headers)
    })?;
    boundaries.interval(ctx, companion, stream_length)
}

/// Dimension owners by stream scope and byte offset, and the parameter each
/// names, for finding the companion that governs a dimension frame. Decode
/// and native validation both apply this rule.
pub(crate) struct GoverningCompanions<'a, 'ctx> {
    owners_at: HashMap<(&'a str, u64), Vec<&'a DesignParameterOwner>>,
    /// The first parameter of each stream scope and record index, and whether
    /// it is the only one.
    parameters: HashMap<(&'a str, u32), (&'a DesignParameter, bool)>,
    _storage: ScopedReservation<'ctx>,
}

impl<'a, 'ctx> GoverningCompanions<'a, 'ctx> {
    /// Index `owners` and `parameters` under a scoped reservation.
    pub(crate) fn build(
        ctx: &'ctx DecodeContext<'_>,
        owners: &'a [DesignParameterOwner],
        parameters: &'a [DesignParameter],
    ) -> Result<Self, CodecError> {
        let ((owners_at, parameters), storage) =
            ctx.with_scoped_storage("f3d governing dimension companions", || {
                let mut owners_at = HashMap::new();
                for owner in ctx.admit_iter(owners, "index F3D governing dimension owners")? {
                    let Some(stream) = record_stream(ctx, owner.id())? else {
                        continue;
                    };
                    ctx.push_hash_group(
                        &mut owners_at,
                        (stream, owner.byte_offset()),
                        owner,
                        "f3d governing dimension owner offsets",
                        "f3d governing dimension owners",
                    )?;
                }
                let mut by_index = HashMap::new();
                for parameter in
                    ctx.admit_iter(parameters, "index F3D governing dimension parameters")?
                {
                    let Some(stream) = record_stream(ctx, &parameter.id)? else {
                        continue;
                    };
                    let key = (stream, parameter.record_index);
                    match ctx.get_mut_hash_map(
                        &mut by_index,
                        &key,
                        "f3d governing dimension parameters",
                    )? {
                        Some((_, unique)) => *unique = false,
                        None => {
                            ctx.insert_hash_map(
                                &mut by_index,
                                key,
                                (parameter, true),
                                "f3d governing dimension parameters",
                            )?;
                        }
                    }
                }
                Ok::<_, CodecError>((owners_at, by_index))
            })?;
        Ok(Self {
            owners_at,
            parameters,
            _storage: storage,
        })
    }

    /// The companion of the only owner 59 bytes after `paired_byte_offset`,
    /// in the stream of the frame `native_id`, whose parameter is the stream's
    /// only parameter of that record index and a dimension.
    pub(crate) fn governing(
        &self,
        ctx: &DecodeContext<'_>,
        native_id: &str,
        paired_byte_offset: u64,
    ) -> Result<Option<u32>, CodecError> {
        let Some(stream) = record_stream(ctx, native_id)? else {
            return Ok(None);
        };
        self.governing_in(ctx, stream, paired_byte_offset)
    }

    /// The governing companion of a frame of stream scope `stream`.
    fn governing_in(
        &self,
        ctx: &DecodeContext<'_>,
        stream: &str,
        paired_byte_offset: u64,
    ) -> Result<Option<u32>, CodecError> {
        let Some(following_offset) = paired_byte_offset.checked_add(59) else {
            return Ok(None);
        };
        let Some(owners) = ctx.get_hash_map(
            &self.owners_at,
            &(stream, following_offset),
            "find F3D governing dimension owner",
        )?
        else {
            return Ok(None);
        };
        let mut governing = None;
        for owner in owners {
            ctx.charge_work(1, "scan F3D governing dimension owners")?;
            let dimension = ctx
                .get_hash_map(
                    &self.parameters,
                    &(stream, owner.parameter_record_index()),
                    "find F3D governing dimension parameter",
                )?
                .is_some_and(|(parameter, unique)| {
                    *unique && parameter.kind() == DesignParameterKind::Dimension
                });
            if !dimension {
                continue;
            }
            if governing.is_some() {
                return Ok(None);
            }
            governing = Some(owner.companion_record_index());
        }
        Ok(governing)
    }
}

/// Parse with the parse's retained storage held under a scoped reservation.
/// A caller that keeps the parsed value commits the reservation; dropping it
/// releases the storage of a value the caller discards.
fn parse_scoped<'ctx, T>(
    ctx: &'ctx DecodeContext<'_>,
    parse: impl FnOnce() -> Result<Option<T>, CodecError>,
) -> Result<Option<(T, ScopedReservation<'ctx>)>, CodecError> {
    let (parsed, storage) = ctx.with_scoped_storage("f3d dimension frame candidate", parse)?;
    Ok(parsed.map(|parsed| (parsed, storage)))
}

/// Decode the indexed record that directly contains each construction recipe
/// owned by a dimensional parameter companion.
pub(crate) fn decode_dimension_recipe_records(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    parameters: &[DesignParameter],
    owners: &[DesignParameterOwner],
    companions: &[DesignParameterCompanion],
    recipes: &[ConstructionRecipe],
) -> Result<Vec<DesignDimensionRecipeRecord>, CodecError> {
    let (parameter_index, _parameter_storage) =
        dimension_parameter_index(ctx, parameters, "f3d dimension recipe parameter index")?;
    let ((dimension_owners, recipe_index), _index_storage) =
        ctx.with_scoped_storage("f3d dimension recipe indexes", || {
            let mut dimension_owners: StreamRecordKeys<'_> = Vec::new();
            for owner in ctx.admit_iter(owners, "scan F3D dimension recipe owners")? {
                let Some(stream) = record_stream(ctx, owner.id())? else {
                    continue;
                };
                if is_dimension_parameter(
                    ctx,
                    &parameter_index,
                    stream,
                    owner.parameter_record_index(),
                )? {
                    ctx.push_vec(
                        &mut dimension_owners,
                        (stream, owner.record_index()),
                        "f3d dimension recipe owners",
                    )?;
                }
            }
            sort_record_keys(ctx, &mut dimension_owners, "f3d dimension recipe owners")?;
            let mut recipe_index = HashMap::new();
            for recipe in ctx.admit_iter(recipes, "index F3D dimension recipes")? {
                ctx.insert_hash_map(
                    &mut recipe_index,
                    recipe.id.as_str(),
                    recipe,
                    "f3d dimension recipe index",
                )?;
            }
            Ok::<_, CodecError>((dimension_owners, recipe_index))
        })?;
    let mut out = Vec::new();
    for companion in ctx.admit_iter(companions, "scan F3D dimension recipe companions")? {
        let Some(stream) = record_stream(ctx, companion.id())? else {
            continue;
        };
        if !has_record_key(
            ctx,
            &dimension_owners,
            (stream, companion.owner_record_index()),
            "find F3D dimension recipe owner",
        )? {
            continue;
        }
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(payload) = companion.payload() else {
            continue;
        };
        let Some(start) = usize::try_from(payload.byte_offset()).ok() else {
            continue;
        };
        let Some(end) = usize::try_from(payload.byte_length())
            .ok()
            .and_then(|length| start.checked_add(length))
            .filter(|end| *end <= bytes.len())
        else {
            continue;
        };
        // The payload's record headers, read when its first recipe resolves.
        let mut payload_headers = None;
        for (recipe_ordinal, recipe_id) in ctx
            .admit_iter(
                payload.owned_recipe_ids(),
                "scan F3D owned dimension recipes",
            )?
            .enumerate()
        {
            let Some(recipe) = ctx
                .get_hash_map(
                    &recipe_index,
                    recipe_id.as_str(),
                    "find F3D dimension recipe",
                )?
                .copied()
            else {
                continue;
            };
            let Some(recipe_offset) = usize::try_from(recipe.byte_offset).ok() else {
                continue;
            };
            let (headers, _) = match &mut payload_headers {
                Some(headers) => headers,
                slot @ None => slot.insert(companion_record_headers(ctx, bytes, start, end)?),
            };
            let Some((header, record_end)) =
                record_containing(ctx, headers, start, end, recipe_offset)?
            else {
                continue;
            };
            let at = header.offset;
            let family_name_len = construction_recipe_family_name_len(recipe.kind);
            let Some(program_offset) = recipe_offset
                .checked_add(family_name_len)
                .filter(|offset| *offset < record_end)
            else {
                continue;
            };
            let Some((prefix_offset, prefix_bytes)) =
                recipe_record_prefix(bytes, at, recipe_offset, family_name_len)
            else {
                continue;
            };
            let Ok(prefix_offset) = u64::try_from(prefix_offset) else {
                continue;
            };
            let Some(program) = contiguous_i32_program(ctx, bytes, program_offset, record_end)
            else {
                continue;
            };
            let program = program?;
            let references = decode_recipe_references_charged(ctx, prefix_bytes, prefix_offset)?;
            let prefix_bytes = ctx.copy_retained(prefix_bytes, "f3d dimension recipe prefix")?;
            let class_tag = header.retain_class_tag(ctx, "f3d dimension recipe class tag")?;
            let (Ok(recipe_ordinal), Ok(byte_offset), Ok(frame_length), Ok(program_offset)) = (
                u32::try_from(recipe_ordinal),
                u64::try_from(at),
                u64::try_from(record_end - at),
                u64::try_from(program_offset),
            ) else {
                continue;
            };
            let id = design_record_id_charged(
                ctx,
                &entry.name,
                ":design-dimension-recipe-record#",
                recipe.byte_offset,
                "f3d dimension recipe record ID",
            )?;
            let recipe_id = ctx.copy_retained_text(&recipe.id, "f3d dimension recipe ID")?;

            ctx.push_vec(
                &mut out,
                DesignDimensionRecipeRecord {
                    id,
                    companion_record_index: companion.record_index(),
                    recipe_ordinal,
                    recipe_id,
                    recipe_kind: recipe.kind,
                    byte_offset,
                    class_tag,
                    record_index: header.record_index,
                    frame_length,
                    prefix_offset,
                    prefix_bytes,
                    references,
                    program_offset,
                    program,
                    matching_edge_operand_ids: Vec::new(),
                },
                "f3d dimension recipe records",
            )?;
        }
    }
    ctx.stable_sort_by(
        &mut out[..],
        |value| &value.id,
        Ord::cmp,
        "sort f3d design dimension_frames 1",
    )?;
    Ok(out)
}

pub(crate) fn decode_recipe_references_charged(
    ctx: &DecodeContext<'_>,
    prefix: &[u8],
    prefix_offset: u64,
) -> Result<Vec<crate::records::dimensions::DesignRecipeReference>, CodecError> {
    if !recipe_reference_header(prefix) {
        return Ok(Vec::new());
    }
    // A run that fails part-way drops the references it built, so they are
    // held under a scoped reservation until the whole run decodes.
    let (references, storage) = ctx.with_scoped_storage("f3d recipe references", || {
        match View::u32_le_at(prefix, 14) {
            Some(2) => decode_paired_recipe_references(ctx, prefix, prefix_offset),
            Some(3) => decode_standard_recipe_references(ctx, prefix, prefix_offset),
            Some(group_count) if group_count >= 4 => decode_grouped_recipe_references(
                ctx,
                prefix,
                prefix_offset,
                index_from_u32(group_count),
            ),
            _ => Ok(None),
        }
    })?;
    let Some(references) = references else {
        return Ok(Vec::new());
    };
    storage.commit()?;
    Ok(references)
}

fn decode_standard_recipe_references(
    ctx: &DecodeContext<'_>,
    prefix: &[u8],
    prefix_offset: u64,
) -> Result<Option<Vec<crate::records::dimensions::DesignRecipeReference>>, CodecError> {
    if View::u32_le_at(prefix, 22).is_none_or(|value| value == 0) {
        return Ok(None);
    }
    let mut references = Vec::new();
    let mut at = 22usize;
    // Each operand advances `at` by at least seventeen bytes, and each one
    // charges its own scan and copies.
    while prefix
        .len()
        .checked_sub(at)
        .is_some_and(|remaining| remaining > 4)
    {
        if recipe_reference_suffix(ctx, &prefix[at..])? {
            return Ok(Some(references));
        }
        let Some(next) = decode_recipe_reference_operand(
            ctx,
            prefix,
            prefix_offset,
            at,
            RecipeReferenceTokenFrame::Either,
            &mut references,
        )?
        else {
            return Ok(None);
        };
        at = next;
    }
    Ok((prefix.get(at..) == Some(&[0, 0, 0, 0])).then_some(references))
}

fn decode_paired_recipe_references(
    ctx: &DecodeContext<'_>,
    prefix: &[u8],
    prefix_offset: u64,
) -> Result<Option<Vec<crate::records::dimensions::DesignRecipeReference>>, CodecError> {
    const MINIMUM_PAIR_SIZE: usize = 42;

    let Some(pair_count) = View::u32_le_at(prefix, 18).map(index_from_u32) else {
        return Ok(None);
    };
    if pair_count == 0
        || prefix
            .len()
            .checked_sub(22)
            .is_none_or(|remaining| pair_count > remaining / MINIMUM_PAIR_SIZE)
    {
        return Ok(None);
    }
    let mut at = 22usize;
    let mut references = Vec::new();
    for _ in 0..pair_count {
        ctx.charge_work(1, "scan F3D paired recipe operands")?;
        let packed_start = references.len();
        let Some(packed_next) = decode_recipe_reference_operand(
            ctx,
            prefix,
            prefix_offset,
            at,
            RecipeReferenceTokenFrame::Packed,
            &mut references,
        )?
        else {
            return Ok(None);
        };
        let length_prefixed_start = references.len();
        let Some(next) = decode_recipe_reference_operand(
            ctx,
            prefix,
            prefix_offset,
            packed_next,
            RecipeReferenceTokenFrame::LengthPrefixed,
            &mut references,
        )?
        else {
            return Ok(None);
        };
        // Both forms of a pair name the same selector and design references.
        let (packed, length_prefixed) =
            references[packed_start..].split_at(length_prefixed_start - packed_start);
        let selector = |operand: &[crate::records::dimensions::DesignRecipeReference]| {
            operand.first().map(|reference| reference.selector)
        };
        if selector(packed) != selector(length_prefixed)
            || packed.len() != length_prefixed.len()
            || ctx
                .position_by(
                    packed,
                    {
                        let mut paired = length_prefixed.iter();
                        move |reference| {
                            Ok(paired.next().map(|paired| paired.design_reference)
                                != Some(reference.design_reference))
                        }
                    },
                    "compare F3D paired recipe references",
                )?
                .is_some()
        {
            return Ok(None);
        }
        at = next;
    }
    Ok((at == prefix.len()).then_some(references))
}

fn decode_grouped_recipe_references(
    ctx: &DecodeContext<'_>,
    prefix: &[u8],
    prefix_offset: u64,
    group_count: usize,
) -> Result<Option<Vec<crate::records::dimensions::DesignRecipeReference>>, CodecError> {
    const MINIMUM_PACKED_OPERAND_SIZE: usize = 17;
    const GROUP_COUNT_WORD_SIZE: usize = 4;

    let mut references = Vec::new();
    let mut at = grouped_recipe::LEN;
    // `group_count` is parsed, so both refusals below are reachable: a prefix
    // shorter than the grouped-recipe header states no group at all, and a
    // group count whose packed groups do not fit the prefix states no group
    // this decoder can read. The multiplication states the same bound the
    // division stated, without a divisor whose zero case no input reaches.
    let Some(available) = prefix.len().checked_sub(at) else {
        return Ok(None);
    };
    let Some(required) =
        group_count.checked_mul(GROUP_COUNT_WORD_SIZE + MINIMUM_PACKED_OPERAND_SIZE)
    else {
        return Ok(None);
    };
    if required > available {
        return Ok(None);
    }
    for _ in 0..group_count {
        ctx.charge_work(1, "scan F3D grouped recipe groups")?;
        let Some(operand_count) = View::u32_le_at(prefix, at).map(index_from_u32) else {
            return Ok(None);
        };
        at += 4;
        if operand_count == 0
            || prefix
                .len()
                .checked_sub(at)
                .is_none_or(|remaining| operand_count > remaining / MINIMUM_PACKED_OPERAND_SIZE)
        {
            return Ok(None);
        }
        for _ in 0..operand_count {
            ctx.charge_work(1, "scan F3D grouped recipe operands")?;
            let Some(next) = decode_recipe_reference_operand(
                ctx,
                prefix,
                prefix_offset,
                at,
                RecipeReferenceTokenFrame::Packed,
                &mut references,
            )?
            else {
                return Ok(None);
            };
            at = next;
        }
    }
    Ok((prefix.get(at..) == Some(&[0, 0, 0, 0])).then_some(references))
}

pub(in crate::design) fn is_paired_recipe_reference_frame(
    ctx: &DecodeContext<'_>,
    prefix: &[u8],
) -> Result<bool, CodecError> {
    if !recipe_reference_header(prefix)
        || View::u32_le_at(prefix, grouped_recipe::GROUP_COUNT) != Some(2)
    {
        return Ok(false);
    }
    let Some(pair_count) = View::u32_le_at(prefix, 18).map(index_from_u32) else {
        return Ok(false);
    };
    if pair_count == 0
        || prefix
            .len()
            .checked_sub(22)
            .is_none_or(|remaining| pair_count > remaining / 42)
    {
        return Ok(false);
    }
    let mut at = 22usize;
    for _ in 0..pair_count {
        ctx.charge_work(1, "scan F3D paired recipe frame")?;
        let Some(packed) =
            scan_recipe_reference_operand(ctx, prefix, at, RecipeReferenceTokenFrame::Packed)?
        else {
            return Ok(false);
        };
        let Some(length_prefixed) = scan_recipe_reference_operand(
            ctx,
            prefix,
            packed.next,
            RecipeReferenceTokenFrame::LengthPrefixed,
        )?
        else {
            return Ok(false);
        };
        // Equal design-reference words are equal reference bytes.
        if packed.selector != length_prefixed.selector
            || packed.reference_count != length_prefixed.reference_count
            || !ctx.equal_bytes(
                packed.reference_bytes(prefix),
                length_prefixed.reference_bytes(prefix),
                "compare F3D paired recipe references",
            )?
        {
            return Ok(false);
        }
        at = length_prefixed.next;
    }
    Ok(at == prefix.len())
}

pub(crate) fn is_grouped_recipe_reference_frame(
    ctx: &DecodeContext<'_>,
    prefix: &[u8],
) -> Result<bool, CodecError> {
    if !recipe_reference_header(prefix) {
        return Ok(false);
    }
    let Some(group_count) = View::u32_le_at(prefix, grouped_recipe::GROUP_COUNT)
        .filter(|count| *count >= 4)
        .map(index_from_u32)
    else {
        return Ok(false);
    };
    let Some(available) = prefix.len().checked_sub(grouped_recipe::LEN) else {
        return Ok(false);
    };
    let Some(required) = group_count.checked_mul(21) else {
        return Ok(false);
    };
    if required > available {
        return Ok(false);
    }
    let mut at = grouped_recipe::LEN;
    for _ in 0..group_count {
        ctx.charge_work(1, "scan F3D grouped recipe frame groups")?;
        let Some(operand_count) = View::u32_le_at(prefix, at).map(index_from_u32) else {
            return Ok(false);
        };
        at += 4;
        if operand_count == 0
            || prefix
                .len()
                .checked_sub(at)
                .is_none_or(|remaining| operand_count > remaining / 17)
        {
            return Ok(false);
        }
        for _ in 0..operand_count {
            ctx.charge_work(1, "scan F3D grouped recipe frame operands")?;
            let Some(operand) =
                scan_recipe_reference_operand(ctx, prefix, at, RecipeReferenceTokenFrame::Packed)?
            else {
                return Ok(false);
            };
            at = operand.next;
        }
    }
    Ok(prefix.get(at..) == Some(&[0, 0, 0, 0]))
}

/// The recipe-reference header: ten zero bytes, the u32 `1`, a group-count
/// word, and a nonzero u32 at offset 18.
fn recipe_reference_header(prefix: &[u8]) -> bool {
    zeros_at::<10>(prefix, 0)
        && View::u32_le_at(prefix, 10) == Some(1)
        && View::u32_le_at(prefix, 18).is_some_and(|value| value != 0)
}

#[derive(Clone, Copy)]
enum RecipeReferenceTokenFrame {
    Either,
    Packed,
    LengthPrefixed,
}

struct ScannedRecipeReferenceOperand<'a> {
    selector: u32,
    token: &'a str,
    token_at: usize,
    references_at: usize,
    reference_count: usize,
    next: usize,
}

impl ScannedRecipeReferenceOperand<'_> {
    /// The operand's design-reference words.
    fn reference_bytes<'p>(&self, prefix: &'p [u8]) -> &'p [u8] {
        prefix
            .get(self.references_at..self.references_at + self.reference_count * 4)
            .unwrap_or(&[])
    }
}

fn scan_recipe_reference_operand<'prefix>(
    ctx: &DecodeContext<'_>,
    prefix: &'prefix [u8],
    at: usize,
    token_frame: RecipeReferenceTokenFrame,
) -> Result<Option<ScannedRecipeReferenceOperand<'prefix>>, CodecError> {
    let Some(selector) = View::u32_le_at(prefix, at).filter(|value| *value != 0) else {
        return Ok(None);
    };
    let Some(token_encoding_at) = at.checked_add(4) else {
        return Ok(None);
    };
    let length_prefixed = if matches!(token_frame, RecipeReferenceTokenFrame::Packed) {
        None
    } else if let Some((token, marker_at)) =
        lp_ascii_filtered_view(prefix, token_encoding_at, 0..=2000, u8::is_ascii_graphic)
    {
        if is_decimal_integer_token(ctx, token.as_bytes())?
            && View::u32_le_at(prefix, marker_at) == Some(0)
        {
            Some((token, token_encoding_at + 4, marker_at + 4))
        } else {
            None
        }
    } else {
        None
    };
    let mut packed = None;
    if !matches!(token_frame, RecipeReferenceTokenFrame::LengthPrefixed) {
        for length in 1usize..=8 {
            let Some(token_end) = token_encoding_at.checked_add(length) else {
                continue;
            };
            let Some(token) = prefix.get(token_encoding_at..token_end) else {
                continue;
            };
            // A packed token is at most eight bytes.
            if !decimal_integer_digits(token)
                .is_some_and(|digits| digits.iter().all(u8::is_ascii_digit))
                || !zeros_at::<4>(prefix, token_end)
            {
                continue;
            }
            // A decimal token is ASCII.
            if let Ok(token) = std::str::from_utf8(token) {
                packed = Some((token, token_encoding_at, token_end + 4));
                break;
            }
        }
    }
    let Some((token, token_at, marker_at)) = length_prefixed.or(packed) else {
        return Ok(None);
    };
    let Some(reference_count) = View::u32_le_at(prefix, marker_at)
        .filter(|value| *value != 0)
        .and_then(|value| usize::try_from(value).ok())
    else {
        return Ok(None);
    };
    let Some(reference_bytes) = reference_count.checked_mul(4) else {
        return Ok(None);
    };
    let Some(references_at) = marker_at.checked_add(4) else {
        return Ok(None);
    };
    let Some(references_end) = references_at.checked_add(reference_bytes) else {
        return Ok(None);
    };
    let Some(words) = prefix.get(references_at..references_end) else {
        return Ok(None);
    };
    let next = match token_frame {
        RecipeReferenceTokenFrame::Packed => references_end,
        RecipeReferenceTokenFrame::Either | RecipeReferenceTokenFrame::LengthPrefixed => {
            if View::u32_le_at(prefix, references_end) != Some(0) {
                return Ok(None);
            }
            references_end + 4
        }
    };
    if !ctx.all_by(
        words.as_chunks::<4>().0,
        |word| Ok(*word != [0; 4]),
        "validate F3D recipe design references",
    )? {
        return Ok(None);
    }
    Ok(Some(ScannedRecipeReferenceOperand {
        selector,
        token,
        token_at,
        references_at,
        reference_count,
        next,
    }))
}

/// Append the references of the operand of `token_frame` at `at` to
/// `references` and return the offset after the operand, or `None` when no
/// such operand is there. A `None` can leave references of the operand
/// appended; the caller then drops the whole run.
fn decode_recipe_reference_operand(
    ctx: &DecodeContext<'_>,
    prefix: &[u8],
    prefix_offset: u64,
    at: usize,
    token_frame: RecipeReferenceTokenFrame,
    references: &mut Vec<crate::records::dimensions::DesignRecipeReference>,
) -> Result<Option<usize>, CodecError> {
    let Some(scanned) = scan_recipe_reference_operand(ctx, prefix, at, token_frame)? else {
        return Ok(None);
    };
    let offset = |at: usize| prefix_offset.checked_add(u64::try_from(at).ok()?);
    let (Some(selector_offset), Some(token_offset)) = (offset(at), offset(scanned.token_at)) else {
        return Ok(None);
    };
    for (ordinal, word) in ctx
        .admit_iter(
            scanned.reference_bytes(prefix).as_chunks::<4>().0,
            "scan F3D recipe operand references",
        )?
        .enumerate()
    {
        let design_reference_at = scanned.references_at + ordinal * 4;
        let (Some(design_reference), Some(design_reference_offset)) =
            (View::u32_le_at(word, 0), offset(design_reference_at))
        else {
            return Ok(None);
        };
        let reference = crate::records::dimensions::DesignRecipeReference {
            selector: i64::from(scanned.selector),
            selector_offset,
            token: ctx.copy_retained_text(scanned.token, "f3d recipe reference token")?,
            token_offset,
            design_reference: i64::from(design_reference),
            design_reference_offset,
            candidate_faces: Vec::new(),
            candidate_edges: Vec::new(),
            alternate_selector_faces: Vec::new(),
            alternate_selector_edges: Vec::new(),
        };
        ctx.push_vec(references, reference, "f3d recipe operand references")?;
    }
    Ok(Some(scanned.next))
}

/// The digits of a decimal integer token: the token after an optional `-`,
/// when that is not empty.
fn decimal_integer_digits(token: &[u8]) -> Option<&[u8]> {
    let digits = match token {
        [b'-', digits @ ..] => digits,
        digits => digits,
    };
    (!digits.is_empty()).then_some(digits)
}

/// Whether `token` is a decimal integer, charging each digit it tests.
fn is_decimal_integer_token(ctx: &DecodeContext<'_>, token: &[u8]) -> Result<bool, CodecError> {
    let Some(digits) = decimal_integer_digits(token) else {
        return Ok(false);
    };
    ctx.all_by(
        digits,
        |byte| Ok(byte.is_ascii_digit()),
        "validate F3D recipe reference token",
    )
}

/// Whether `bytes` holds the recipe-reference terminator: four zero bytes, or
/// a counted run of nonzero references closed by four or six zero bytes that
/// ends the prefix.
fn recipe_reference_suffix(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<bool, CodecError> {
    if bytes == [0; 4] {
        return Ok(true);
    }
    if View::u32_le_at(bytes, 0) != Some(1)
        || View::u32_le_at(bytes, 4) != Some(1)
        || View::u32_le_at(bytes, 8) != Some(0)
        || View::u32_le_at(bytes, 12) != Some(0)
    {
        return Ok(false);
    }
    let Some(reference_count) = View::u32_le_at(bytes, 16).filter(|count| *count != 0) else {
        return Ok(false);
    };
    let Some(terminator_at) = index_from_u32(reference_count)
        .checked_mul(4)
        .and_then(|reference_bytes| reference_bytes.checked_add(20))
    else {
        return Ok(false);
    };
    let Some(terminator) = bytes
        .get(terminator_at..)
        .filter(|terminator| matches!(terminator.len(), 4 | 6))
    else {
        return Ok(false);
    };
    // The terminator is at most six bytes.
    if terminator.iter().any(|byte| *byte != 0) {
        return Ok(false);
    }
    ctx.all_by(
        bytes[20..terminator_at].as_chunks::<4>().0,
        |reference| Ok(*reference != [0; 4]),
        "validate F3D recipe suffix references",
    )
}

/// Join dimension-recipe selector/reference pairs to active solved subentities.
pub(crate) fn bind_dimension_recipe_reference_candidates(
    ctx: &DecodeContext<'_>,
    records: &mut [DesignDimensionRecipeRecord],
    tags: &[PersistentSubentityTag],
) -> Result<(), CodecError> {
    let record_count = records.len();
    for (record, _) in records
        .iter_mut()
        .zip(ctx.admit_iter(&(0..record_count), "scan F3D dimension recipe records")?)
    {
        let reference_count = record.references.len();
        for (reference, _) in record.references.iter_mut().zip(ctx.admit_iter(
            &(0..reference_count),
            "scan F3D dimension recipe references",
        )?) {
            bind_recipe_reference_candidates_charged(ctx, reference, tags, Some(&record.id))?;
        }
    }
    Ok(())
}

pub(crate) fn bind_recipe_reference_candidates_charged(
    ctx: &DecodeContext<'_>,
    reference: &mut crate::records::dimensions::DesignRecipeReference,
    tags: &[PersistentSubentityTag],
    owner_id: Option<&str>,
) -> Result<(), CodecError> {
    use cadmpeg_ir::attributes::AttributeTarget;

    // Releasing the earlier candidates is free.
    reference.candidate_faces.clear();
    reference.candidate_edges.clear();
    reference.alternate_selector_faces.clear();
    reference.alternate_selector_edges.clear();
    for tag in ctx.admit_iter(tags, "scan F3D recipe-reference tags")? {
        if !(ctx.equal_bytes(
            tag.token.as_str().as_bytes(),
            reference.token.as_bytes(),
            "match F3D recipe reference token",
        )? && ctx.contains(
            &tag.design_references,
            &reference.design_reference,
            "find F3D dimension recipe design reference",
        )? && owner_id
            .is_none_or(|owner_id| crate::ids::same_native_occurrence(&tag.id, owner_id)))
        {
            continue;
        }
        let matching_selector = tag.selector == reference.selector;
        match (&tag.target, matching_selector) {
            (AttributeTarget::Face(face), true) => ctx.push_vec(
                &mut reference.candidate_faces,
                face.try_clone_for_decode(ctx, "f3d recipe reference candidate ID")?,
                "f3d recipe reference candidate",
            )?,
            (AttributeTarget::Edge(edge), true) => ctx.push_vec(
                &mut reference.candidate_edges,
                edge.try_clone_for_decode(ctx, "f3d recipe reference candidate ID")?,
                "f3d recipe reference candidate",
            )?,
            (AttributeTarget::Face(face), false) => ctx.push_vec(
                &mut reference.alternate_selector_faces,
                face.try_clone_for_decode(ctx, "f3d recipe reference candidate ID")?,
                "f3d recipe reference candidate",
            )?,
            (AttributeTarget::Edge(edge), false) => ctx.push_vec(
                &mut reference.alternate_selector_edges,
                edge.try_clone_for_decode(ctx, "f3d recipe reference candidate ID")?,
                "f3d recipe reference candidate",
            )?,
            _ => {}
        }
    }
    ctx.stable_sort_by(
        &mut reference.candidate_faces[..],
        |value| value.as_str(),
        Ord::cmp,
        "sort f3d design dimension_frames 2",
    )?;
    ctx.dedup_vec(
        &mut reference.candidate_faces,
        "dedupe F3D recipe reference candidates",
    )?;
    ctx.stable_sort_by(
        &mut reference.candidate_edges[..],
        |value| value.as_str(),
        Ord::cmp,
        "sort f3d design dimension_frames 3",
    )?;
    ctx.dedup_vec(
        &mut reference.candidate_edges,
        "dedupe F3D recipe reference candidates",
    )?;
    ctx.stable_sort_by(
        &mut reference.alternate_selector_faces[..],
        |value| value.as_str(),
        Ord::cmp,
        "sort f3d design dimension_frames 4",
    )?;
    ctx.dedup_vec(
        &mut reference.alternate_selector_faces,
        "dedupe F3D recipe reference candidates",
    )?;
    ctx.stable_sort_by(
        &mut reference.alternate_selector_edges[..],
        |value| value.as_str(),
        Ord::cmp,
        "sort f3d design dimension_frames 5",
    )?;
    ctx.dedup_vec(
        &mut reference.alternate_selector_edges,
        "dedupe F3D recipe reference candidates",
    )?;
    Ok(())
}

/// Join dimension programs to byte-identical edge-recipe program tails.
pub(crate) fn bind_dimension_recipe_edge_operands(
    ctx: &DecodeContext<'_>,
    records: &mut [DesignDimensionRecipeRecord],
    operands: &[DesignEdgeOperand],
) -> Result<(), CodecError> {
    let record_count = records.len();
    for (record, _) in records
        .iter_mut()
        .zip(ctx.admit_iter(&(0..record_count), "scan F3D dimension recipe records")?)
    {
        record.matching_edge_operand_ids =
            dimension_recipe_matching_edge_operand_ids(ctx, record, operands)?;
    }
    Ok(())
}

pub(crate) fn dimension_recipe_matching_edge_operand_ids(
    ctx: &DecodeContext<'_>,
    record: &DesignDimensionRecipeRecord,
    operands: &[DesignEdgeOperand],
) -> Result<Vec<String>, CodecError> {
    let mut ids = Vec::new();
    let stream = record_stream(ctx, &record.id)?;
    for operand in ctx.admit_iter(operands, "scan F3D dimension recipe edge operands")? {
        if !dimension_recipe_edge_matches(ctx, record, stream, operand)? {
            continue;
        }
        ctx.push_formatted_retained(
            &mut ids,
            format_args!("{}", operand.id),
            "f3d dimension recipe edge IDs",
            "f3d dimension recipe edge ID text",
        )?;
    }
    ctx.stable_sort_by(
        &mut ids[..],
        |value| value,
        Ord::cmp,
        "sort f3d design dimension_frames 7",
    )?;
    ctx.dedup_vec(&mut ids, "dedupe F3D dimension recipe edge IDs")?;
    Ok(ids)
}

/// Whether `operand`, in the stream scope `stream` of `record`, has a recipe
/// program whose tail after its first seven words occurs in the record's
/// program. Identifiers without a stream scope share one.
fn dimension_recipe_edge_matches(
    ctx: &DecodeContext<'_>,
    record: &DesignDimensionRecipeRecord,
    stream: Option<&str>,
    operand: &DesignEdgeOperand,
) -> Result<bool, CodecError> {
    let same_stream = match (record_stream(ctx, &operand.id)?, stream) {
        (Some(operand_stream), Some(stream)) => ctx.equal_bytes(
            operand_stream.as_bytes(),
            stream.as_bytes(),
            "match F3D record stream scope",
        )?,
        (None, None) => true,
        (Some(_), None) | (None, Some(_)) => false,
    };
    if !same_stream {
        return Ok(false);
    }
    let Some(tail) = operand
        .recipe_program
        .get(7..)
        .filter(|tail| !tail.is_empty())
    else {
        return Ok(false);
    };
    let Some(window_count) = (record.program.len() + 1).checked_sub(tail.len()) else {
        return Ok(false);
    };
    // Each visited start is charged with its window comparison.
    let mut window_start = 0;
    ctx.any_by(
        &record.program[..window_count],
        |_| {
            let window = &record.program[window_start..window_start + tail.len()];
            window_start += 1;
            ctx.equal(window, tail, "match F3D dimension recipe program tail")
        },
        "scan F3D dimension recipe program",
    )
}

pub(super) fn recipe_record_prefix(
    bytes: &[u8],
    record_offset: usize,
    family_name_offset: usize,
    family_name_len: usize,
) -> Option<(usize, &[u8])> {
    let prefix_offset = record_offset.checked_add(11)?;
    let prefix_end = family_name_offset.checked_sub(4)?;
    if View::u32_le_at(bytes, prefix_end)? != u32::try_from(family_name_len).ok()? {
        return None;
    }
    let prefix = bytes.get(prefix_offset..prefix_end)?;
    Some((prefix_offset, prefix))
}

/// The record headers that open in `start..end`: the first at or after
/// `start`, then each next one at least a header length after its
/// predecessor. The forward search visits each byte of the interval, and the
/// ten bytes after it that a header opening before `end` reaches, at most
/// once. Header storage is held by the returned reservation.
fn companion_record_headers<'bytes, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &'bytes [u8],
    start: usize,
    end: usize,
) -> Result<(Vec<IndexedRecordHeader<'bytes>>, ScopedReservation<'ctx>), CodecError> {
    let mut storage = ctx.reserve_scoped(0, "f3d dimension recipe headers")?;
    let mut headers = Vec::new();
    let mut cursor = start;
    let window = header_search_window(bytes, end);
    while let Some(header) = next_indexed_record_header(ctx, window, cursor, |_| true)? {
        ctx.push_scoped_vec(
            &mut storage,
            &mut headers,
            header,
            "f3d dimension recipe headers",
        )?;
        cursor = header.offset + 11;
    }
    Ok((headers, storage))
}

/// The header of `headers`, the record headers of `start..end`, whose record
/// contains `member_offset`, and the end of that record: the next header, or
/// `end`.
fn record_containing<'bytes>(
    ctx: &DecodeContext<'_>,
    headers: &[IndexedRecordHeader<'bytes>],
    start: usize,
    end: usize,
    member_offset: usize,
) -> Result<Option<(IndexedRecordHeader<'bytes>, usize)>, CodecError> {
    if start > member_offset || member_offset >= end {
        return Ok(None);
    }
    let following = ctx.partition_point(
        headers,
        |header| Ok(header.offset <= member_offset),
        "find F3D dimension recipe record",
    )?;
    let Some(containing) = following
        .checked_sub(1)
        .and_then(|index| headers.get(index))
        .copied()
    else {
        return Ok(None);
    };
    let record_end = headers.get(following).map_or(end, |header| header.offset);
    Ok(Some((containing, record_end)))
}

pub(super) fn contiguous_i32_program(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    end: usize,
) -> Option<Result<Vec<i32>, CodecError>> {
    let view = View::over_retained(bytes).child(start, end)?;
    if view.remaining() == 0 || !view.remaining().is_multiple_of(4) {
        return None;
    }
    let (words, _) = view.unread().as_chunks::<4>();
    let mut program = Vec::new();
    let collected = (|| {
        ctx.reserve_capacity(&mut program, words.len(), "f3d recipe program words")?;
        for word in ctx.admit_iter(words, "scan F3D recipe program words")? {
            let value = View::i32_le_at(word, 0)
                .ok_or_else(|| CodecError::malformed("F3D recipe program word is short"))?;
            ctx.push_vec(&mut program, value, "f3d recipe program words")?;
        }
        Ok(())
    })();
    Some(collected.map(|()| program))
}

/// Decode paired typed sketch loci nested immediately after dimensional
/// parameter-companion prefixes.
pub(crate) fn decode_dimension_locus_pairs(
    ctx: &DecodeContext<'_>,
    inputs: &DimensionDecodeInputs<'_>,
) -> Result<Vec<DesignDimensionLocusPair>, CodecError> {
    let &DimensionDecodeInputs {
        scan,
        parameters,
        owners,
        companions,
        points,
        curves,
        ..
    } = inputs;
    let (parameter_index, _parameter_storage) =
        dimension_parameter_index(ctx, parameters, "f3d dimension locus parameter index")?;
    let (dimension_companions, _companion_storage) = dimension_companion_keys(
        ctx,
        owners,
        &parameter_index,
        "f3d dimension locus companions",
    )?;
    let governing = GoverningCompanions::build(ctx, owners, parameters)?;
    let mut intervals = CompanionIntervals::of_inputs(ctx, inputs)?;
    let mut geometry = StreamIndexSets::new(ctx)?;
    let mut records = RecordOffsetCache::new(ctx)?;
    let mut out = Vec::new();
    for companion in ctx.admit_iter(companions, "scan F3D dimension locus companions")? {
        let Some(scope) = record_stream(ctx, companion.id())? else {
            continue;
        };
        if !has_record_key(
            ctx,
            &dimension_companions,
            (scope, companion.record_index()),
            "find F3D dimension locus companion",
        )? {
            continue;
        }
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, scope)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some((start, end)) = intervals.interval(ctx, scope, companion, bytes.len())? else {
            continue;
        };
        let geometry_indices = geometry.get(ctx, scope, || {
            dimension_geometry_indices(ctx, scope, points, curves)
        })?;
        let stream_records = records.get(ctx, scope, bytes)?;
        let Some((mut pair, pair_storage)) = find_dimension_locus_pair(
            ctx,
            bytes,
            start,
            end,
            companion.record_index(),
            geometry_indices,
            stream_records,
        )?
        else {
            continue;
        };
        let Some(governing_companion_record_index) =
            governing.governing_in(ctx, scope, pair.paired_byte_offset())?
        else {
            continue;
        };
        pair_storage.commit()?;
        pair.id = design_record_id_charged(
            ctx,
            &entry.name,
            ":design-dimension-locus-pair#",
            pair.byte_offset(),
            "f3d dimension locus pair ID",
        )?;
        pair.governing_companion_record_index = governing_companion_record_index;

        ctx.push_vec(&mut out, pair, "f3d dimension locus pairs")?;
    }
    ctx.stable_sort_by(
        &mut out[..],
        |value| &value.id,
        Ord::cmp,
        "sort f3d design dimension_frames 8",
    )?;
    Ok(out)
}

/// The only frame in `start..end` that `parse` reads and whose paired header
/// precedes `end`. Candidates are `start` and each record header after it in
/// the interval; a second frame ends the search with none. A candidate's
/// retained storage stays scoped until the search keeps it, and the caller
/// commits the returned reservation when it keeps the frame.
fn only_frame_in<'ctx, T>(
    ctx: &'ctx DecodeContext<'_>,
    records: &IndexedRecordOffsets,
    start: usize,
    end: usize,
    mut parse: impl FnMut(usize) -> Result<Option<T>, CodecError>,
) -> Result<Option<(T, ScopedReservation<'ctx>)>, CodecError> {
    let mut candidate = parse_scoped(ctx, || parse(start))?;
    let later = match start.checked_add(1) {
        Some(after_start) => records.headers_in(ctx, after_start, end)?,
        None => &[],
    };
    for &at in later {
        ctx.charge_work(1, "scan F3D dimension frame candidates")?;
        if let Some(frame) = parse_scoped(ctx, || parse(at))? {
            if candidate.is_some() {
                return Ok(None);
            }
            candidate = Some(frame);
        }
    }
    Ok(candidate)
}

fn find_dimension_locus_pair<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    end: usize,
    companion_record_index: u32,
    geometry_indices: &[u32],
    records: &IndexedRecordOffsets,
) -> Result<Option<(DesignDimensionLocusPair, ScopedReservation<'ctx>)>, CodecError> {
    only_frame_in(ctx, records, start, end, |at| {
        Ok(parse_dimension_locus_pair(
            ctx,
            bytes,
            at,
            companion_record_index,
            geometry_indices,
            records,
        )?
        .filter(|pair| pair.paired_byte_offset() < u64_from_index(end)))
    })
}

/// The header that closes the frame of `record_index` opened at `start`: the
/// first header of that index at or after `position`.
fn paired_header<'bytes>(
    ctx: &DecodeContext<'_>,
    bytes: &'bytes [u8],
    records: &IndexedRecordOffsets,
    position: usize,
    record_index: u32,
) -> Result<Option<IndexedRecordHeader<'bytes>>, CodecError> {
    Ok(records
        .first_at_or_after(ctx, position, record_index)?
        .and_then(|at| indexed_record_header_at(bytes, at)))
}

fn parse_dimension_locus_pair(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    companion_record_index: u32,
    geometry_indices: &[u32],
    records: &IndexedRecordOffsets,
) -> Result<Option<DesignDimensionLocusPair>, CodecError> {
    let Some(header) = indexed_record_header_at(bytes, start) else {
        return Ok(None);
    };
    if !zeros_at::<8>(bytes, start + 11)
        || bytes.get(start + 19) != Some(&1)
        || View::u32_le_at(bytes, start + 20) != Some(3)
        || bytes.get(start + 24) != Some(&1)
        || View::u32_le_at(bytes, start + 25) != Some(0)
        || !zeros_at::<6>(bytes, start + 29)
        || bytes.get(start + 39) != Some(&1)
        || !zeros_at::<6>(bytes, start + 44)
        || bytes.get(start + 54) != Some(&1)
        || !zeros_at::<6>(bytes, start + 59)
    {
        return Ok(None);
    }
    let (
        Some(first_geometry_record_index),
        Some(second_geometry_record_index),
        Some(opaque_index),
        Some(first_role),
        Some(second_role),
    ) = (
        View::u32_le_at(bytes, start + 40),
        View::u32_le_at(bytes, start + 55),
        View::u32_le_at(bytes, start + 35),
        View::u32_le_at(bytes, start + 50),
        View::u32_le_at(bytes, start + 65),
    )
    else {
        return Ok(None);
    };
    if !set_contains(ctx, geometry_indices, first_geometry_record_index)?
        || !set_contains(ctx, geometry_indices, second_geometry_record_index)?
    {
        return Ok(None);
    }
    let Some(paired) = paired_header(ctx, bytes, records, start + 69, header.record_index)? else {
        return Ok(None);
    };
    let (Some(first), Some(second)) = (
        NonZeroU32::new(first_geometry_record_index),
        NonZeroU32::new(second_geometry_record_index),
    ) else {
        return Ok(None);
    };
    let pair = DesignDimensionLocusPair::try_new(
        crate::records::dimensions::DesignDimensionLocusPairDraft {
            id: String::new(),
            companion_record_index,
            governing_companion_record_index: companion_record_index,
            byte_offset: u64_from_index(start),
            class_tag: header.retain_class_tag(ctx, "copy F3D dimension locus class tag")?,
            record_index: header.record_index,
            frame_length: u64_from_index(paired.offset - start),
            opaque_index: Some(crate::records::identity::Located {
                value: opaque_index,
                offset: u64_from_index(start + 35),
            }),
            loci: [
                crate::records::dimensions::DesignDimensionAnnotationOperand {
                    geometry_record_index: Some(first),
                    geometry_reference_offset: u64_from_index(start + 40),
                    role: first_role,
                    role_offset: u64_from_index(start + 50),
                },
                crate::records::dimensions::DesignDimensionAnnotationOperand {
                    geometry_record_index: Some(second),
                    geometry_reference_offset: u64_from_index(start + 55),
                    role: second_role,
                    role_offset: u64_from_index(start + 65),
                },
            ],
            paired_class_tag: paired
                .retain_class_tag(ctx, "copy F3D dimension locus paired class tag")?,
            paired_byte_offset: u64_from_index(paired.offset),
        },
    );
    Ok(pair.ok())
}

/// Decode dimension frames whose ordered operand run contains a null record
/// reference followed by one typed sketch-geometry reference.
pub(crate) fn decode_dimension_null_locus_pairs(
    ctx: &DecodeContext<'_>,
    inputs: &DimensionDecodeInputs<'_>,
    pairs: &[DesignDimensionLocusPair],
    groups: &[DesignDimensionLocusGroup],
) -> Result<Vec<DesignDimensionLocusPair>, CodecError> {
    let &DimensionDecodeInputs {
        scan,
        parameters,
        owners,
        companions,
        points,
        curves,
        ..
    } = inputs;
    let (parameter_index, _parameter_storage) =
        dimension_parameter_index(ctx, parameters, "f3d dimension locus parameter index")?;
    let (dimension_companions, _companion_storage) = dimension_companion_keys(
        ctx,
        owners,
        &parameter_index,
        "f3d dimension locus companions",
    )?;
    let (typed_companions, _typed_storage) =
        ctx.with_scoped_storage("f3d typed dimension companions", || {
            let mut typed_companions: StreamRecordKeys<'_> = Vec::new();
            for pair in ctx.admit_iter(pairs, "scan F3D typed dimension locus pairs")? {
                if let Some(scope) = record_stream(ctx, &pair.id)? {
                    ctx.push_vec(
                        &mut typed_companions,
                        (scope, pair.companion_record_index),
                        "f3d typed dimension companions",
                    )?;
                }
            }
            for group in ctx.admit_iter(groups, "scan F3D typed dimension locus groups")? {
                if let Some(scope) = record_stream(ctx, &group.id)? {
                    ctx.push_vec(
                        &mut typed_companions,
                        (scope, group.companion_record_index),
                        "f3d typed dimension companions",
                    )?;
                }
            }
            sort_record_keys(ctx, &mut typed_companions, "f3d typed dimension companions")?;
            Ok::<_, CodecError>(typed_companions)
        })?;
    let governing = GoverningCompanions::build(ctx, owners, parameters)?;
    let mut intervals = CompanionIntervals::of_inputs(ctx, inputs)?;
    let mut geometry = StreamIndexSets::new(ctx)?;
    let mut records = RecordOffsetCache::new(ctx)?;
    let mut out = Vec::new();
    for companion in ctx.admit_iter(companions, "scan F3D null-locus companions")? {
        let Some(scope) = record_stream(ctx, companion.id())? else {
            continue;
        };
        let key = (scope, companion.record_index());
        if !has_record_key(
            ctx,
            &dimension_companions,
            key,
            "find F3D null-locus companion",
        )? || has_record_key(ctx, &typed_companions, key, "find F3D typed companion")?
        {
            continue;
        }
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, scope)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some((start, end)) = intervals.interval(ctx, scope, companion, bytes.len())? else {
            continue;
        };
        let geometry_indices = geometry.get(ctx, scope, || {
            dimension_geometry_indices(ctx, scope, points, curves)
        })?;
        let stream_records = records.get(ctx, scope, bytes)?;
        let Some((mut pair, pair_storage)) = find_dimension_null_locus_pair(
            ctx,
            bytes,
            start,
            end,
            companion.record_index(),
            geometry_indices,
            stream_records,
        )?
        else {
            continue;
        };
        let Some(governing_companion_record_index) =
            governing.governing_in(ctx, scope, pair.paired_byte_offset())?
        else {
            continue;
        };
        pair_storage.commit()?;
        pair.id = design_record_id_charged(
            ctx,
            &entry.name,
            ":design-dimension-null-locus-pair#",
            pair.byte_offset(),
            "f3d dimension null locus pair ID",
        )?;
        pair.governing_companion_record_index = governing_companion_record_index;

        ctx.push_vec(&mut out, pair, "f3d dimension null locus pairs")?;
    }
    ctx.stable_sort_by(
        &mut out[..],
        |value| &value.id,
        Ord::cmp,
        "sort f3d design dimension_frames 9",
    )?;
    Ok(out)
}

fn find_dimension_null_locus_pair<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    end: usize,
    companion_record_index: u32,
    geometry_indices: &[u32],
    records: &IndexedRecordOffsets,
) -> Result<Option<(DesignDimensionLocusPair, ScopedReservation<'ctx>)>, CodecError> {
    only_frame_in(ctx, records, start, end, |at| {
        Ok(parse_dimension_null_locus_pair(
            ctx,
            bytes,
            at,
            companion_record_index,
            geometry_indices,
            records,
        )?
        .filter(|pair| pair.paired_byte_offset() < u64_from_index(end)))
    })
}

fn parse_dimension_null_locus_pair(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    companion_record_index: u32,
    geometry_indices: &[u32],
    records: &IndexedRecordOffsets,
) -> Result<Option<DesignDimensionLocusPair>, CodecError> {
    let Some(header) = indexed_record_header_at(bytes, start) else {
        return Ok(None);
    };
    if !zeros_at::<8>(bytes, start + 11)
        || bytes.get(start + 19) != Some(&1)
        || View::u32_le_at(bytes, start + 20) != Some(2)
        || bytes.get(start + 24) != Some(&1)
        || View::u32_le_at(bytes, start + 25) != Some(0)
        || !zeros_at::<6>(bytes, start + 29)
        || bytes.get(start + 39) != Some(&1)
        || !zeros_at::<6>(bytes, start + 44)
    {
        return Ok(None);
    }
    let (Some(geometry_record_index), Some(null_role), Some(role)) = (
        View::u32_le_at(bytes, start + 40),
        View::u32_le_at(bytes, start + 35),
        View::u32_le_at(bytes, start + 50),
    ) else {
        return Ok(None);
    };
    if !set_contains(ctx, geometry_indices, geometry_record_index)? {
        return Ok(None);
    }
    let Some(paired) = paired_header(ctx, bytes, records, start + 54, header.record_index)? else {
        return Ok(None);
    };
    let Some(geometry) = NonZeroU32::new(geometry_record_index) else {
        return Ok(None);
    };
    let pair = DesignDimensionLocusPair::try_new(
        crate::records::dimensions::DesignDimensionLocusPairDraft {
            id: String::new(),
            companion_record_index,
            governing_companion_record_index: companion_record_index,
            byte_offset: u64_from_index(start),
            class_tag: header.retain_class_tag(ctx, "copy F3D dimension locus class tag")?,
            record_index: header.record_index,
            frame_length: u64_from_index(paired.offset - start),
            opaque_index: None,
            loci: [
                crate::records::dimensions::DesignDimensionAnnotationOperand {
                    geometry_record_index: None,
                    geometry_reference_offset: u64_from_index(start + 25),
                    role: null_role,
                    role_offset: u64_from_index(start + 35),
                },
                crate::records::dimensions::DesignDimensionAnnotationOperand {
                    geometry_record_index: Some(geometry),
                    geometry_reference_offset: u64_from_index(start + 40),
                    role,
                    role_offset: u64_from_index(start + 50),
                },
            ],
            paired_class_tag: paired
                .retain_class_tag(ctx, "copy F3D dimension locus paired class tag")?,
            paired_byte_offset: u64_from_index(paired.offset),
        },
    );
    Ok(pair.ok())
}

/// One interval of a stream that may hold annotation frames, and the
/// companion whose owned interval it is.
type AnnotationInterval = (usize, usize, Option<u32>);

/// Decode paired `EntityGenesis` dimensional frames carrying annotation data
/// and a direct backlink to the governed parameter owner.
pub(crate) fn decode_dimension_annotation_frames(
    ctx: &DecodeContext<'_>,
    inputs: &DimensionDecodeInputs<'_>,
    entities: &[DesignEntityHeader],
) -> Result<Vec<DesignDimensionAnnotationFrame>, CodecError> {
    let &DimensionDecodeInputs {
        scan,
        parameters,
        owners,
        companions,
        points,
        curves,
        ..
    } = inputs;
    let (parameter_index, _parameter_storage) =
        dimension_parameter_index(ctx, parameters, "f3d dimension annotation parameter index")?;
    let (dimension_companions, _companion_storage) = dimension_companion_keys(
        ctx,
        owners,
        &parameter_index,
        "f3d dimension annotation companions",
    )?;
    let mut intervals_by_stream = CompanionIntervals::of_inputs(ctx, inputs)?;
    let mut out = Vec::new();
    // Each stream is decoded once, in the order of its first companion.
    let (streams, _streams_storage) = companion_streams(ctx, companions)?;
    for &(stream, _) in ctx.admit_iter(&streams, "scan F3D dimension annotation streams")? {
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let (intervals, _interval_storage) =
            annotation_intervals(ctx, inputs, stream, bytes.len(), &mut intervals_by_stream)?;
        if intervals.is_empty() {
            continue;
        }
        let ((geometry_indices, sketch_entities, governed_owners), _stream_storage) = ctx
            .with_scoped_storage("f3d dimension annotation stream tables", || {
                let geometry_indices = dimension_geometry_indices(ctx, stream, points, curves)?;
                let sketch_entities = dimension_sketch_entities(ctx, stream, entities)?;
                let mut governed_owners = HashMap::new();
                for owner in ctx.admit_iter(owners, "scan F3D dimension annotation owners")? {
                    if in_stream(ctx, owner.id(), stream)?
                        && has_record_key(
                            ctx,
                            &dimension_companions,
                            (stream, owner.companion_record_index()),
                            "find F3D dimension annotation companion",
                        )?
                    {
                        ctx.insert_hash_map(
                            &mut governed_owners,
                            owner.record_index(),
                            owner.companion_record_index(),
                            "f3d dimension annotation governed owners",
                        )?;
                    }
                }
                Ok::<_, CodecError>((geometry_indices, sketch_entities, governed_owners))
            })?;
        let (stream_records, _records_storage) = IndexedRecordOffsets::build_scoped(ctx, bytes)?;
        let frame_inputs = AnnotationFrameInputs {
            governed_owners: &governed_owners,
            geometry_indices: &geometry_indices,
            sketch_entities: &sketch_entities,
            records: &stream_records,
        };
        // The paired header offset of each frame decoded in this stream, by
        // frame offset. A frame's parse depends only on the stream, so a later
        // interval that reaches the same frame reuses its extent.
        let mut decoded = HashMap::new();
        let mut decoded_storage =
            ctx.reserve_scoped(0, "f3d dimension annotation decoded offsets")?;
        for &(start, end, containing_companion_record_index) in
            ctx.admit_iter(&intervals, "scan F3D dimension annotation intervals")?
        {
            let mut candidates = stream_records.headers_in(ctx, start, end)?;
            while let Some((&at, rest)) = candidates.split_first() {
                ctx.charge_work(1, "scan F3D dimension annotation candidates")?;
                candidates = rest;
                let paired_at = if let Some(&paired_at) = ctx.get_hash_map(
                    &decoded,
                    &at,
                    "find F3D dimension annotation decoded offset",
                )? {
                    if paired_at >= end {
                        continue;
                    }
                    paired_at
                } else {
                    let Some((mut frame, frame_storage)) = parse_scoped(ctx, || {
                        parse_dimension_annotation_frame(
                            ctx,
                            bytes,
                            at,
                            end,
                            containing_companion_record_index,
                            &frame_inputs,
                        )
                    })?
                    else {
                        continue;
                    };
                    frame_storage.commit()?;
                    let Ok(paired_at) = usize::try_from(frame.paired_byte_offset()) else {
                        return Err(CodecError::Malformed(
                            "F3D annotation paired offset is not representable".into(),
                        ));
                    };
                    frame.id = design_record_id_charged(
                        ctx,
                        &entry.name,
                        ":design-dimension-annotation-frame#",
                        frame.byte_offset(),
                        "f3d dimension annotation frame ID",
                    )?;
                    decoded_storage.with_storage(|| {
                        ctx.insert_hash_map(
                            &mut decoded,
                            at,
                            paired_at,
                            "f3d dimension annotation decoded offsets",
                        )
                    })?;
                    ctx.push_vec(&mut out, frame, "f3d dimension annotation frames")?;
                    paired_at
                };
                candidates = stream_records.headers_in(ctx, paired_at + 1, end)?;
            }
        }
    }
    ctx.stable_sort_by(
        &mut out[..],
        |value| &value.id,
        Ord::cmp,
        "sort f3d design dimension_frames 10",
    )?;
    Ok(out)
}

/// Stream scopes, each with the position of its first companion, and the
/// reservation that holds them.
type CompanionStreams<'a, 'ctx> = (Vec<(&'a str, usize)>, ScopedReservation<'ctx>);

/// The distinct stream scopes of `companions`, each with the position of its
/// first companion, in that order.
fn companion_streams<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    companions: &'a [DesignParameterCompanion],
) -> Result<CompanionStreams<'a, 'ctx>, CodecError> {
    let operation = "f3d dimension annotation streams";
    ctx.with_scoped_storage(operation, || {
        let mut streams = Vec::new();
        for (position, companion) in ctx
            .admit_iter(companions, "scan F3D dimension annotation companions")?
            .enumerate()
        {
            if let Some(stream) = record_stream(ctx, companion.id())? {
                ctx.push_vec(&mut streams, (stream, position), operation)?;
            }
        }
        // The stable sort keeps each stream's first companion ahead of its
        // others, and the deduplication keeps it.
        ctx.stable_sort_by(&mut streams[..], |(stream, _)| *stream, Ord::cmp, operation)?;
        ctx.dedup_by(
            &mut streams,
            |(left, _), (right, _)| ctx.equal_bytes(left.as_bytes(), right.as_bytes(), operation),
            operation,
        )?;
        ctx.stable_sort_by_key(
            &mut streams[..],
            |(_, position)| *position,
            Ord::cmp,
            operation,
        )?;
        Ok(streams)
    })
}

/// The intervals of `stream` that may hold annotation frames: the owned
/// interval of each of its companions, in companion order, then for each of
/// its scopes, the span from the scope to the nearest companion that an owner
/// of that scope names, in scope order. The intervals are held under the
/// returned reservation.
fn annotation_intervals<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    inputs: &'a DimensionDecodeInputs<'a>,
    stream: &'a str,
    stream_length: usize,
    intervals_by_stream: &mut CompanionIntervals<'a, '_>,
) -> Result<(Vec<AnnotationInterval>, ScopedReservation<'ctx>), CodecError> {
    ctx.with_scoped_storage("f3d dimension annotation intervals", || {
        let mut intervals = Vec::new();
        // The first companion of each record index in the stream.
        let mut companion_offsets = HashMap::new();
        for companion in
            ctx.admit_iter(inputs.companions, "scan F3D annotation owner companions")?
        {
            if !in_stream(ctx, companion.id(), stream)? {
                continue;
            }
            if let Entry::Vacant(slot) = ctx.entry_hash_map(
                &mut companion_offsets,
                companion.record_index(),
                "f3d dimension annotation companions",
            )? {
                slot.insert(companion.byte_offset());
            }
            let Some((start, end)) =
                intervals_by_stream.interval(ctx, stream, companion, stream_length)?
            else {
                continue;
            };
            ctx.push_vec(
                &mut intervals,
                (start, end, Some(companion.record_index())),
                "f3d dimension annotation intervals",
            )?;
        }
        // The nearest companion that an owner of each scope record index names.
        let mut scope_ends = HashMap::new();
        for owner in ctx.admit_iter(
            inputs.owners,
            "scan F3D dimension annotation interval owners",
        )? {
            if !in_stream(ctx, owner.id(), stream)? {
                continue;
            }
            let Some(offset) = ctx
                .get_hash_map(
                    &companion_offsets,
                    &owner.companion_record_index(),
                    "find F3D dimension annotation interval companion",
                )?
                .and_then(|offset| usize::try_from(*offset).ok())
            else {
                continue;
            };
            match ctx.get_mut_hash_map(
                &mut scope_ends,
                &owner.scope_record_index(),
                "f3d dimension annotation scope ends",
            )? {
                Some(end) => *end = offset.min(*end),
                None => {
                    ctx.insert_hash_map(
                        &mut scope_ends,
                        owner.scope_record_index(),
                        offset,
                        "f3d dimension annotation scope ends",
                    )?;
                }
            }
        }
        for scope in ctx.admit_iter(
            inputs.scopes,
            "scan F3D dimension annotation interval scopes",
        )? {
            if !in_stream(ctx, &scope.id, stream)? {
                continue;
            }
            let Some(&end) = ctx.get_hash_map(
                &scope_ends,
                &scope.record_index,
                "find F3D dimension annotation scope end",
            )?
            else {
                continue;
            };
            let Ok(start) = usize::try_from(scope.byte_offset()) else {
                continue;
            };
            if start < end {
                ctx.push_vec(
                    &mut intervals,
                    (start, end, None),
                    "f3d dimension annotation intervals",
                )?;
            }
        }
        Ok::<_, CodecError>(intervals)
    })
}

/// Per-stream tables an annotation frame is checked against.
struct AnnotationFrameInputs<'a> {
    /// The companion of each owner that a dimension companion names.
    governed_owners: &'a HashMap<u32, u32>,
    geometry_indices: &'a [u32],
    sketch_entities: &'a [u32],
    records: &'a IndexedRecordOffsets,
}

/// The record indices of at most 64 operand or return members, in order, and
/// their count.
fn member_indices(members: impl Iterator<Item = u32>) -> ([u32; 64], usize) {
    let mut indices = [0u32; 64];
    let mut count = 0usize;
    for (slot, member) in indices.iter_mut().zip(members) {
        *slot = member;
        count += 1;
    }
    (indices, count)
}

/// Whether the first `left_count` indices of `left` and the first
/// `right_count` of `right` hold the same indices, each as often. Each index
/// of `left` claims an unclaimed equal index of `right`. Both runs hold at most
/// 64 indices, so the comparison is bounded by a constant.
fn same_members(
    (left, left_count): ([u32; 64], usize),
    (right, right_count): ([u32; 64], usize),
) -> bool {
    if left_count != right_count {
        return false;
    }
    let mut claimed = [false; 64];
    left.iter().take(left_count).all(|index| {
        let unclaimed = right
            .iter()
            .zip(claimed.iter_mut())
            .take(right_count)
            .find(|(candidate, claimed)| !**claimed && *candidate == index);
        let Some((_, claimed)) = unclaimed else {
            return false;
        };
        *claimed = true;
        true
    })
}

/// The annotation frame of the record header at `start`, if its paired header
/// precedes `end`.
fn parse_dimension_annotation_frame(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    end: usize,
    companion_record_index: Option<u32>,
    inputs: &AnnotationFrameInputs<'_>,
) -> Result<Option<DesignDimensionAnnotationFrame>, CodecError> {
    let Some(header) = indexed_record_header_at(bytes, start) else {
        return Ok(None);
    };
    if !zeros_at::<8>(bytes, start + 11) || bytes.get(start + 19) != Some(&1) {
        return Ok(None);
    }
    let Some(count) = View::u32_le_at(bytes, start + 20)
        .map(index_from_u32)
        .filter(|count| (1..=64).contains(count))
    else {
        return Ok(None);
    };
    let mut position = start + 24;

    let mut operands = Vec::new();
    ctx.reserve_capacity(&mut operands, count, "f3d dimension annotation operands")?;
    for _ in 0..count {
        if bytes.get(position) != Some(&1) || !zeros_at::<6>(bytes, position + 5) {
            return Ok(None);
        }
        let (Some(geometry_record_index), Some(role)) = (
            View::u32_le_at(bytes, position + 1),
            View::u32_le_at(bytes, position + 11),
        ) else {
            return Ok(None);
        };
        let geometry_record_index = NonZeroU32::new(geometry_record_index);
        if let Some(index) = geometry_record_index {
            if !set_contains(ctx, inputs.geometry_indices, index.get())? {
                return Ok(None);
            }
        }
        ctx.push_vec(
            &mut operands,
            DesignDimensionAnnotationOperand {
                geometry_record_index,
                geometry_reference_offset: u64_from_index(position + 1),
                role,
                role_offset: u64_from_index(position + 11),
            },
            "f3d dimension annotation operands",
        )?;
        position += 15;
    }
    if bytes.get(position) != Some(&1) || View::u32_le_at(bytes, position + 1) != Some(1) {
        return Ok(None);
    }
    let Some(after_key) = lp_ascii_literal_end(bytes, position + 5, b"EntityGenesis") else {
        return Ok(None);
    };
    let Some(after_type) = lp_ascii_literal_end(bytes, after_key, b"IntrinsicMetaTypeuint64")
    else {
        return Ok(None);
    };
    let Some(entity_genesis) = View::u64_le_at(bytes, after_type) else {
        return Ok(None);
    };
    let annotation_byte_offset = after_type + 8;
    let Some(paired) = paired_header(
        ctx,
        bytes,
        inputs.records,
        annotation_byte_offset,
        header.record_index,
    )?
    else {
        return Ok(None);
    };
    let paired_byte_offset = paired.offset;
    if paired_byte_offset >= end
        || !zeros_at::<8>(bytes, paired_byte_offset + 11)
        || bytes.get(paired_byte_offset + 19) != Some(&1)
        || !zeros_at::<6>(bytes, paired_byte_offset + 24)
    {
        return Ok(None);
    }
    let Some(owner_reference) = View::u32_le_at(bytes, paired_byte_offset + 20) else {
        return Ok(None);
    };
    if !set_contains(ctx, inputs.sketch_entities, owner_reference)? {
        return Ok(None);
    }
    let Some(tail) = annotation_tail(
        ctx,
        bytes,
        annotation_byte_offset,
        paired_byte_offset,
        &operands,
        inputs,
    )?
    else {
        return Ok(None);
    };
    let Some(annotation) = bytes.get(annotation_byte_offset..tail.at) else {
        return Ok(None);
    };
    let annotation_bytes = ctx.copy_retained(annotation, "f3d dimension annotation bytes")?;
    let draft = crate::records::dimensions::DesignDimensionAnnotationFrameDraft {
        id: String::new(),
        companion_record_index,
        governing_companion_record_index: tail.governing_companion_record_index,
        byte_offset: u64_from_index(start),
        class_tag: header.retain_class_tag(ctx, "copy F3D dimension annotation class tag")?,
        record_index: header.record_index,
        frame_length: u64_from_index(paired_byte_offset - start),
        operands,
        entity_genesis,
        annotation_bytes,
        annotation_byte_offset: u64_from_index(annotation_byte_offset),
        governing_owner_record_index: tail.governing_owner_record_index,
        governing_owner_reference_offset: u64_from_index(tail.at + 1),
        return_members: tail.return_members,
        paired_class_tag: paired
            .retain_class_tag(ctx, "copy F3D dimension annotation paired class tag")?,
        paired_byte_offset: u64_from_index(paired_byte_offset),
        owner_reference,
        owner_reference_offset: u64_from_index(paired_byte_offset + 20),
    };
    match DesignDimensionAnnotationFrame::try_new_charged(ctx, draft) {
        Ok(frame) => Ok(Some(frame)),
        Err(error @ CodecError::ResourceLimit(_)) => Err(error),
        Err(_) => Ok(None),
    }
}

/// The offset after a u32-counted ASCII field at `at` that holds `expected`.
fn lp_ascii_literal_end<const N: usize>(
    bytes: &[u8],
    at: usize,
    expected: &[u8; N],
) -> Option<usize> {
    if index_from_u32(View::u32_le_at(bytes, at)?) != N {
        return None;
    }
    let start = at.checked_add(4)?;
    (bytes_at::<N>(bytes, start)? == expected).then_some(start + N)
}

/// The annotation tail: the governing-owner reference and the return-member
/// run that closes the annotation bytes.
struct AnnotationTail {
    at: usize,
    governing_owner_record_index: u32,
    governing_companion_record_index: u32,
    return_members: Vec<crate::records::identity::Located<NonZeroU32>>,
}

/// The only position in the annotation bytes `annotation_at..paired_at` that
/// opens a tail: a marked reference to a governed owner, a counted run of
/// return members naming exactly the frame's operand geometry, and zero
/// padding to the paired header.
fn annotation_tail(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    annotation_at: usize,
    paired_at: usize,
    operands: &[DesignDimensionAnnotationOperand],
    inputs: &AnnotationFrameInputs<'_>,
) -> Result<Option<AnnotationTail>, CodecError> {
    let Some(last) = paired_at
        .checked_sub(15)
        .filter(|last| annotation_at <= *last)
    else {
        return Ok(None);
    };
    let Some(annotation) = bytes.get(annotation_at..paired_at) else {
        return Ok(None);
    };
    // Padding before the paired header is zero exactly when every nonzero
    // byte precedes it. The backward walk visits the padding and the last
    // nonzero byte.
    let mut padding_from = annotation_at;
    for (offset, byte) in annotation.iter().enumerate().rev() {
        ctx.charge_work(1, "validate F3D dimension annotation padding")?;
        if *byte != 0 {
            padding_from = annotation_at + offset + 1;
            break;
        }
    }
    let operand_members = member_indices(
        operands
            .iter()
            .filter_map(|operand| operand.geometry_record_index)
            .map(NonZeroU32::get),
    );
    let mut matched = None;
    for tail in annotation_at..last {
        ctx.charge_work(1, "f3d dimension annotation tail scan")?;
        if bytes.get(tail) != Some(&1) || !zeros_at::<6>(bytes, tail + 5) {
            continue;
        }
        let Some(governing_owner_record_index) = View::u32_le_at(bytes, tail + 1) else {
            continue;
        };
        let Some(&governing_companion_record_index) = ctx.get_hash_map(
            inputs.governed_owners,
            &governing_owner_record_index,
            "find F3D dimension annotation governed owner",
        )?
        else {
            continue;
        };
        let Some(return_count) = View::u32_le_at(bytes, tail + 11)
            .map(index_from_u32)
            .filter(|count| *count <= 64)
        else {
            continue;
        };
        let Some(returned) =
            annotation_return_members(ctx, bytes, tail + 15, return_count, inputs)?
        else {
            continue;
        };
        let after_members = tail + 15 + return_count * 11;
        // A run that reaches past the paired header frames no annotation.
        if after_members > paired_at {
            return Ok(None);
        }
        if padding_from > after_members {
            continue;
        }
        if !same_members(
            operand_members,
            (returned.map(NonZeroU32::get), return_count),
        ) {
            continue;
        }
        if matched.is_some() {
            return Ok(None);
        }
        matched = Some((
            tail,
            governing_owner_record_index,
            governing_companion_record_index,
            returned,
            return_count,
        ));
    }
    let Some((at, governing_owner_record_index, governing_companion_record_index, returned, count)) =
        matched
    else {
        return Ok(None);
    };
    let mut return_members = Vec::new();
    ctx.reserve_capacity(
        &mut return_members,
        count,
        "f3d dimension annotation return members",
    )?;
    for (ordinal, value) in returned.into_iter().take(count).enumerate() {
        ctx.push_vec(
            &mut return_members,
            crate::records::identity::Located {
                value,
                offset: u64_from_index(at + 15 + ordinal * 11 + 1),
            },
            "f3d dimension annotation return members",
        )?;
    }
    Ok(Some(AnnotationTail {
        at,
        governing_owner_record_index,
        governing_companion_record_index,
        return_members,
    }))
}

/// The record indices of the `count` marked return members at `at`, in run
/// order, each naming registered sketch geometry.
fn annotation_return_members(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    at: usize,
    count: usize,
    inputs: &AnnotationFrameInputs<'_>,
) -> Result<Option<[NonZeroU32; 64]>, CodecError> {
    let mut members = [NonZeroU32::MIN; 64];
    for (ordinal, member) in members.iter_mut().take(count).enumerate() {
        let cursor = at + ordinal * 11;
        if bytes.get(cursor) != Some(&1) || !zeros_at::<6>(bytes, cursor + 5) {
            return Ok(None);
        }
        let Some(reference) = View::u32_le_at(bytes, cursor + 1).and_then(NonZeroU32::new) else {
            return Ok(None);
        };
        if !set_contains(ctx, inputs.geometry_indices, reference.get())? {
            return Ok(None);
        }
        *member = reference;
    }
    Ok(Some(members))
}

/// Stable Fusion type whose indexed records carry the older direct dimension
/// presentation geometry.
const DIMENSION_PRESENTATION_TYPE_GUID: &str = "6CCF41D5-40BE-48ED-A834-18F3EAED6C57";
/// Stable Fusion type whose indexed records carry the current direct
/// dimension presentation geometry.
const DIMENSION_PRESENTATION_V3_TYPE_GUID: &str = "8C780195-72C0-4a56-A911-E43AB14357F2";
/// Stable `EntityTracking` type used by a dimension presentation's paired
/// header.
const DIMENSION_PRESENTATION_PAIR_TYPE_GUID: &str = "90055C05-546C-4EE7-B3C9-3DD922AD0C9C";

/// Whether `type_guid` names a dimension presentation type. Each literal is
/// 36 bytes, so the comparison is bounded by a constant.
fn is_dimension_presentation_type(type_guid: &str) -> bool {
    type_guid.eq_ignore_ascii_case(DIMENSION_PRESENTATION_TYPE_GUID)
        || type_guid.eq_ignore_ascii_case(DIMENSION_PRESENTATION_V3_TYPE_GUID)
}

/// Decode direct presentation frames that precede a dimension parameter's
/// owner. The decoded Design type tables `types` select the primary and
/// paired classes; no numeric class tag is treated as a cross-stream type
/// identity.
pub(crate) fn decode_dimension_presentation_frames(
    ctx: &DecodeContext<'_>,
    inputs: &DimensionDecodeInputs<'_>,
    types: &[SegmentType],
    entities: &[DesignEntityHeader],
) -> Result<Vec<DesignDimensionPresentationFrame>, CodecError> {
    let &DimensionDecodeInputs {
        scan,
        placements,
        parameters,
        ..
    } = inputs;
    let (parameter_index, _parameter_storage) = dimension_parameter_index(
        ctx,
        parameters,
        "f3d dimension presentation parameter index",
    )?;
    let (sketch_scope_by_entity, _sketch_scope_storage) =
        ctx.with_scoped_storage("f3d dimension presentation sketch scopes", || {
            let mut sketch_scope_by_entity = HashMap::new();
            for placement in ctx.admit_iter(placements, "index F3D dimension sketch placements")? {
                let Some(scope_record_index) = placement.scope_record_index else {
                    continue;
                };
                let Some(stream) = record_stream(ctx, &placement.id)? else {
                    continue;
                };
                ctx.insert_hash_map(
                    &mut sketch_scope_by_entity,
                    (stream, placement.entity_id.suffix()),
                    scope_record_index,
                    "f3d dimension presentation sketch scopes",
                )?;
            }
            Ok::<_, CodecError>(sketch_scope_by_entity)
        })?;
    let (presentation_streams, _presentation_streams_storage) =
        presentation_stream_prefixes(ctx, types)?;
    let mut out = Vec::new();
    for entry in ctx
        .admit_iter(&scan.entries, "scan F3D dimension presentation streams")?
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let (_stream_reservation, stream) =
            crate::design::decode::sketch::native_scope_scoped(ctx, &entry.name)?;
        let Some(prefix) = stream.strip_suffix(BULK_STREAM_FILE) else {
            continue;
        };
        if ctx
            .binary_search(
                &presentation_streams,
                &prefix,
                "find F3D dimension presentation stream",
            )?
            .is_err()
        {
            continue;
        }
        let (stream_types, _stream_types_storage) = ctx
            .with_scoped_storage("f3d dimension presentation stream types", || {
                stream_types_by_entity(ctx, types, &entry.name)
            })?;
        let type_guid_of = |class_code: u32| -> Result<Option<&str>, CodecError> {
            Ok(ctx
                .get_hash_map(
                    &stream_types,
                    &u64::from(class_code),
                    "find F3D dimension presentation type",
                )?
                .map(|(type_guid, _)| *type_guid))
        };
        let is_paired_class = |class_code: u32| -> Result<bool, CodecError> {
            Ok(type_guid_of(class_code)?.is_some_and(|type_guid| {
                type_guid.eq_ignore_ascii_case(DIMENSION_PRESENTATION_PAIR_TYPE_GUID)
            }))
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut stream_tables = None;
        for header in indexed_record_offsets(ctx, bytes)? {
            if !type_guid_of(header.class_code)?.is_some_and(is_dimension_presentation_type) {
                continue;
            }
            if stream_tables.is_none() {
                stream_tables = Some(ctx.with_scoped_storage(
                    "f3d dimension presentation stream tables",
                    || {
                        PresentationStreamTables::build(
                            ctx,
                            &stream,
                            bytes,
                            inputs,
                            &parameter_index,
                            entities,
                        )
                    },
                )?);
            }
            let Some((tables, _)) = stream_tables.as_ref() else {
                continue;
            };
            let Some((mut frame, frame_storage)) = parse_scoped(ctx, || {
                parse_dimension_presentation_frame(
                    ctx,
                    bytes,
                    header.offset,
                    tables,
                    is_paired_class,
                )
            })?
            else {
                continue;
            };
            let Some(&scope_record_index) = ctx.get_hash_map(
                &sketch_scope_by_entity,
                &(stream.as_str(), u64::from(frame.owner_reference)),
                "find F3D dimension presentation sketch scope",
            )?
            else {
                continue;
            };
            let Some(owner) =
                tables.governing_owner(ctx, scope_record_index, frame.paired_byte_offset)?
            else {
                continue;
            };
            frame_storage.commit()?;
            frame.id = design_record_id_charged(
                ctx,
                &entry.name,
                ":design-dimension-presentation-frame#",
                frame.byte_offset,
                "f3d dimension presentation frame ID",
            )?;
            frame.governing_owner_record_index = owner.record_index();
            frame.governing_parameter_record_index = owner.parameter_record_index();
            frame.governing_companion_record_index = owner.companion_record_index();

            ctx.push_vec(&mut out, frame, "f3d dimension presentation frames")?;
        }
    }
    ctx.stable_sort_by(
        &mut out[..],
        |value| &value.id,
        Ord::cmp,
        "sort f3d design dimension_frames 11",
    )?;
    Ok(out)
}

/// The segment prefixes, ascending and distinct, whose type tables register
/// both a dimension presentation type and the paired presentation type: the
/// native scope of each such segment's type-table stream without its file
/// name. Only the record stream of such a segment can hold a presentation
/// frame.
fn presentation_stream_prefixes<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    types: &'a [SegmentType],
) -> Result<(Vec<&'a str>, ScopedReservation<'ctx>), CodecError> {
    let operation = "f3d dimension presentation streams";
    ctx.with_scoped_storage(operation, || {
        let mut presentation = Vec::new();
        let mut paired = Vec::new();
        for design_type in ctx.admit_iter(types, "scan F3D dimension presentation types")? {
            let type_guid = design_type.type_guid.as_str();
            let prefixes = if is_dimension_presentation_type(type_guid) {
                &mut presentation
            } else if type_guid.eq_ignore_ascii_case(DIMENSION_PRESENTATION_PAIR_TYPE_GUID) {
                &mut paired
            } else {
                continue;
            };
            let Some(prefix) = record_stream(ctx, design_type.id())?
                .and_then(|stream| stream.strip_suffix(META_STREAM_FILE))
            else {
                continue;
            };
            ctx.push_vec(prefixes, prefix, operation)?;
        }
        for prefixes in [&mut presentation, &mut paired] {
            ctx.stable_sort_by(&mut prefixes[..], |prefix| *prefix, Ord::cmp, operation)?;
            ctx.dedup_vec(prefixes, operation)?;
        }
        ctx.retain_vec(
            &mut presentation,
            |prefix| Ok(ctx.binary_search(&paired, prefix, operation)?.is_ok()),
            operation,
        )?;
        Ok(presentation)
    })
}

/// Per-stream tables for presentation frames, built when a stream's first
/// presentation-typed record appears.
struct PresentationStreamTables<'a> {
    geometry_indices: Vec<u32>,
    sketch_entities: Vec<u32>,
    records: IndexedRecordOffsets,
    /// The stream's owners of dimensional parameters by scope record index,
    /// each group ascending by offset.
    owners_by_scope: HashMap<u32, Vec<&'a DesignParameterOwner>>,
}

impl<'a> PresentationStreamTables<'a> {
    fn build(
        ctx: &DecodeContext<'_>,
        stream: &str,
        bytes: &[u8],
        inputs: &DimensionDecodeInputs<'a>,
        parameters: &ParameterIndex<'_>,
        entities: &[DesignEntityHeader],
    ) -> Result<Self, CodecError> {
        let mut dimension_owners = Vec::new();
        for owner in ctx.admit_iter(inputs.owners, "scan F3D dimension presentation owners")? {
            if in_stream(ctx, owner.id(), stream)?
                && is_dimension_parameter(ctx, parameters, stream, owner.parameter_record_index())?
            {
                ctx.push_vec(
                    &mut dimension_owners,
                    owner,
                    "f3d dimension presentation owners",
                )?;
            }
        }
        ctx.stable_sort_by_key(
            &mut dimension_owners[..],
            |owner| owner.byte_offset(),
            Ord::cmp,
            "order F3D dimension presentation owners",
        )?;
        let mut owners_by_scope = HashMap::new();
        for &owner in
            ctx.admit_iter(&dimension_owners, "group F3D dimension presentation owners")?
        {
            ctx.push_hash_group(
                &mut owners_by_scope,
                owner.scope_record_index(),
                owner,
                "f3d dimension presentation owner scopes",
                "f3d dimension presentation owners",
            )?;
        }
        Ok(Self {
            geometry_indices: dimension_geometry_indices(
                ctx,
                stream,
                inputs.points,
                inputs.curves,
            )?,
            sketch_entities: dimension_sketch_entities(ctx, stream, entities)?,
            records: IndexedRecordOffsets::build(ctx, bytes)?,
            owners_by_scope,
        })
    }

    /// The first owner of scope `scope_record_index` after
    /// `paired_byte_offset`, earliest in the input among owners at one offset.
    fn governing_owner(
        &self,
        ctx: &DecodeContext<'_>,
        scope_record_index: u32,
        paired_byte_offset: u64,
    ) -> Result<Option<&'a DesignParameterOwner>, CodecError> {
        let Some(owners) = ctx.get_hash_map(
            &self.owners_by_scope,
            &scope_record_index,
            "find F3D dimension presentation owner",
        )?
        else {
            return Ok(None);
        };
        let first = ctx.partition_point(
            owners,
            |owner| Ok(owner.byte_offset() <= paired_byte_offset),
            "find F3D dimension presentation owner",
        )?;
        Ok(owners.get(first).copied())
    }
}

fn parse_dimension_presentation_frame(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    tables: &PresentationStreamTables<'_>,
    is_paired_class: impl Fn(u32) -> Result<bool, CodecError>,
) -> Result<Option<DesignDimensionPresentationFrame>, CodecError> {
    let Some(header) = indexed_record_header_at(bytes, start) else {
        return Ok(None);
    };
    if !zeros_at::<8>(bytes, start + 11) || bytes.get(start + 19) != Some(&1) {
        return Ok(None);
    }
    let Some(count) = View::u32_le_at(bytes, start + 20)
        .map(index_from_u32)
        .filter(|count| (1..=64).contains(count))
    else {
        return Ok(None);
    };
    let mut position = start + 24;

    let mut operands = Vec::new();
    ctx.reserve_capacity(&mut operands, count, "f3d dimension presentation operands")?;
    for _ in 0..count {
        if bytes.get(position) != Some(&1) || !zeros_at::<6>(bytes, position + 5) {
            return Ok(None);
        }
        let (Some(geometry_record_index), Some(role)) = (
            View::u32_le_at(bytes, position + 1).and_then(NonZeroU32::new),
            View::u32_le_at(bytes, position + 11),
        ) else {
            return Ok(None);
        };
        if !set_contains(ctx, &tables.geometry_indices, geometry_record_index.get())? {
            return Ok(None);
        }
        ctx.push_vec(
            &mut operands,
            DesignDimensionPresentationOperand {
                geometry_record_index,
                geometry_reference_offset: u64_from_index(position + 1),
                role,
                role_offset: u64_from_index(position + 11),
            },
            "f3d dimension presentation operands",
        )?;
        position += 15;
    }
    let presentation_byte_offset = position;
    // The paired header is the first header of the frame's record index that
    // carries a nonzero-led paired class tag; it must open with the paired
    // record prologue.
    let offsets = tables.records.offsets(header.record_index);
    let first = ctx.partition_point(
        offsets,
        |offset| Ok(*offset < position),
        "find F3D dimension presentation paired header",
    )?;
    let Some(paired) = ctx.find_map(
        offsets.get(first..).unwrap_or(&[]),
        |offset| {
            let Some(candidate) = indexed_record_header_at(bytes, *offset) else {
                return Ok(None);
            };
            Ok(
                (candidate.class_tag[0] != b'0' && is_paired_class(candidate.class_code)?)
                    .then_some(candidate),
            )
        },
        "find F3D dimension presentation paired header",
    )?
    else {
        return Ok(None);
    };
    let paired_byte_offset = paired.offset;
    if !zeros_at::<8>(bytes, paired_byte_offset + 11)
        || bytes.get(paired_byte_offset + 19) != Some(&1)
    {
        return Ok(None);
    }
    let Some(owner_reference) = View::u32_le_at(bytes, paired_byte_offset + 20) else {
        return Ok(None);
    };
    if !set_contains(ctx, &tables.sketch_entities, owner_reference)? {
        return Ok(None);
    }
    let Some(presentation) = bytes.get(presentation_byte_offset..paired_byte_offset) else {
        return Ok(None);
    };
    let presentation_bytes = ctx.copy_retained(presentation, "f3d dimension presentation bytes")?;
    Ok(Some(DesignDimensionPresentationFrame {
        id: String::new(),
        byte_offset: u64_from_index(start),
        class_tag: header.retain_class_tag(ctx, "copy F3D dimension presentation class tag")?,
        record_index: header.record_index,
        frame_length: u64_from_index(paired_byte_offset - start),
        operands,
        presentation_bytes,
        presentation_byte_offset: u64_from_index(presentation_byte_offset),
        paired_class_tag: paired
            .retain_class_tag(ctx, "copy F3D dimension presentation paired class tag")?,
        paired_byte_offset: u64_from_index(paired_byte_offset),
        owner_reference,
        owner_reference_offset: u64_from_index(paired_byte_offset + 20),
        governing_owner_record_index: 0,
        governing_parameter_record_index: 0,
        governing_companion_record_index: 0,
    }))
}

/// Decode counted typed sketch loci nested immediately after dimensional
/// parameter-companion prefixes.
pub(crate) fn decode_dimension_locus_groups(
    ctx: &DecodeContext<'_>,
    inputs: &DimensionDecodeInputs<'_>,
    entities: &[DesignEntityHeader],
) -> Result<Vec<DesignDimensionLocusGroup>, CodecError> {
    let &DimensionDecodeInputs {
        scan,
        parameters,
        owners,
        companions,
        points,
        curves,
        ..
    } = inputs;
    let (parameter_index, _parameter_storage) =
        dimension_parameter_index(ctx, parameters, "f3d dimension locus parameter index")?;
    let (dimension_companions, _companion_storage) = dimension_companion_keys(
        ctx,
        owners,
        &parameter_index,
        "f3d dimension locus companions",
    )?;
    let mut intervals = CompanionIntervals::of_inputs(ctx, inputs)?;
    let mut geometry = StreamIndexSets::new(ctx)?;
    let mut sketch_entities = StreamIndexSets::new(ctx)?;
    let mut out = Vec::new();
    for companion in ctx.admit_iter(companions, "scan F3D dimension locus-group companions")? {
        let Some(scope) = record_stream(ctx, companion.id())? else {
            continue;
        };
        if !has_record_key(
            ctx,
            &dimension_companions,
            (scope, companion.record_index()),
            "find F3D dimension locus-group companion",
        )? {
            continue;
        }
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, scope)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some((start, end)) = intervals.interval(ctx, scope, companion, bytes.len())? else {
            continue;
        };
        let geometry_indices = geometry.get(ctx, scope, || {
            dimension_geometry_indices(ctx, scope, points, curves)
        })?;
        let stream_sketch_entities = sketch_entities.get(ctx, scope, || {
            dimension_sketch_entities(ctx, scope, entities)
        })?;
        find_dimension_locus_groups(
            ctx,
            LocusGroupStream {
                name: &entry.name,
                bytes,
                geometry_indices,
                sketch_entities: stream_sketch_entities,
            },
            (start, end),
            companion.record_index(),
            &mut out,
        )?;
    }
    ctx.stable_sort_by(
        &mut out[..],
        |value| &value.id,
        Ord::cmp,
        "sort f3d design dimension_frames 12",
    )?;
    Ok(out)
}

/// A record stream that locus groups are read from, with the stream's sketch
/// geometry and sketch entity record indices.
struct LocusGroupStream<'a> {
    name: &'a str,
    bytes: &'a [u8],
    geometry_indices: &'a [u32],
    sketch_entities: &'a [u32],
}

/// Append to `out` the locus groups at `start` and at each record header after
/// it that opens before `end`, whose run ends by `end`, in byte order. A
/// candidate's storage stays scoped until its group is kept.
fn find_dimension_locus_groups(
    ctx: &DecodeContext<'_>,
    stream: LocusGroupStream<'_>,
    (start, end): (usize, usize),
    companion_record_index: u32,
    out: &mut Vec<DesignDimensionLocusGroup>,
) -> Result<(), CodecError> {
    let mut candidate_at = Some(start);
    while let Some(at) = candidate_at {
        ctx.charge_work(1, "scan F3D dimension locus group candidates")?;
        if let Some((mut group, group_storage)) = parse_scoped(ctx, || {
            Ok(parse_dimension_locus_group(
                ctx,
                stream.bytes,
                at,
                companion_record_index,
                stream.geometry_indices,
                stream.sketch_entities,
            )?
            .filter(|group| group.next_byte_offset <= u64_from_index(end)))
        })? {
            group_storage.commit()?;
            group.id = design_record_id_charged(
                ctx,
                stream.name,
                ":design-dimension-locus-group#",
                group.byte_offset,
                "f3d dimension locus group ID",
            )?;
            ctx.push_vec(out, group, "f3d dimension locus groups")?;
        }
        candidate_at = match at.checked_add(1) {
            Some(after) => next_header_before(ctx, stream.bytes, after, end)?,
            None => None,
        };
    }
    Ok(())
}

/// The first record header at or after `position` that opens before `end`.
fn next_header_before(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    position: usize,
    end: usize,
) -> Result<Option<usize>, CodecError> {
    next_indexed_record_offset(ctx, header_search_window(bytes, end), position)
}

/// The bytes a search for record headers that open before `end` reads. An
/// eleven-byte header that opens before `end` ends by `end + 10`, so the
/// window stops there, or at the stream end when that is nearer.
fn header_search_window(bytes: &[u8], end: usize) -> &[u8] {
    end.checked_add(10)
        .and_then(|limit| bytes.get(..limit))
        .unwrap_or(bytes)
}

fn parse_dimension_locus_group(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    companion_record_index: u32,
    geometry_indices: &[u32],
    sketch_entities: &[u32],
) -> Result<Option<DesignDimensionLocusGroup>, CodecError> {
    let Some(header) = indexed_record_header_at(bytes, start) else {
        return Ok(None);
    };
    if !zeros_at::<8>(bytes, start + 11) || bytes.get(start + 19) != Some(&1) {
        return Ok(None);
    }
    let Some(count) = View::u32_le_at(bytes, start + 20)
        .map(index_from_u32)
        .filter(|count| (1..=64).contains(count))
    else {
        return Ok(None);
    };
    let mut position = start + 24;

    let mut geometry = Vec::new();
    ctx.reserve_capacity(&mut geometry, count, "f3d dimension locus geometry")?;
    for _ in 0..count {
        if bytes.get(position) != Some(&1) || !zeros_at::<6>(bytes, position + 5) {
            return Ok(None);
        }
        let (Some(geometry_record_index), Some(role)) = (
            View::u32_le_at(bytes, position + 1),
            View::u32_le_at(bytes, position + 11),
        ) else {
            return Ok(None);
        };
        if !set_contains(ctx, geometry_indices, geometry_record_index)? {
            return Ok(None);
        }
        ctx.push_vec(
            &mut geometry,
            (
                geometry_record_index,
                u64_from_index(position + 1),
                role,
                u64_from_index(position + 11),
            ),
            "f3d dimension locus geometry",
        )?;
        position += 15;
    }
    if bytes.get(position) != Some(&0)
        || bytes.get(position + 1) != Some(&1)
        || !zeros_at::<6>(bytes, position + 6)
    {
        return Ok(None);
    }
    let (Some(owner_reference), Some(owner_role), Some(state), Some(return_count)) = (
        View::u32_le_at(bytes, position + 2),
        View::u32_le_at(bytes, position + 12),
        View::u32_le_at(bytes, position + 16),
        View::u32_le_at(bytes, position + 20).map(index_from_u32),
    ) else {
        return Ok(None);
    };
    if !set_contains(ctx, sketch_entities, owner_reference)? {
        return Ok(None);
    }
    let owner_reference_offset = u64_from_index(position + 2);
    let owner_role_offset = u64_from_index(position + 12);
    let state_offset = u64_from_index(position + 16);
    if return_count != count {
        return Ok(None);
    }
    position += 24;

    let mut loci = Vec::new();
    ctx.reserve_capacity(
        &mut loci,
        return_count,
        "f3d dimension locus return members",
    )?;
    for &(geometry_record_index, geometry_reference_offset, role, role_offset) in &geometry {
        if bytes.get(position) != Some(&1) || !zeros_at::<6>(bytes, position + 5) {
            return Ok(None);
        }
        let Some(record_index) = View::u32_le_at(bytes, position + 1) else {
            return Ok(None);
        };
        if !set_contains(ctx, geometry_indices, record_index)? {
            return Ok(None);
        }
        ctx.push_vec(
            &mut loci,
            DesignDimensionLocus {
                geometry_record_index,
                geometry_reference_offset,
                role,
                role_offset,
                returned: crate::records::identity::Located {
                    value: record_index,
                    offset: u64_from_index(position + 1),
                },
            },
            "f3d dimension locus return members",
        )?;
        position += 11;
    }
    if bytes.get(position) != Some(&0) {
        return Ok(None);
    }
    let next_byte_offset = position + 1;
    let Some(next) = indexed_record_header_at(bytes, next_byte_offset) else {
        return Ok(None);
    };
    Ok(Some(DesignDimensionLocusGroup {
        id: String::new(),
        companion_record_index,
        byte_offset: u64_from_index(start),
        class_tag: header.retain_class_tag(ctx, "copy F3D dimension locus group class tag")?,
        record_index: header.record_index,
        frame_length: u64_from_index(next_byte_offset - start),
        loci,
        owner_reference,
        owner_reference_offset,
        owner_role,
        owner_role_offset,
        state,
        state_offset,
        next_class_tag: next.retain_class_tag(ctx, "copy F3D dimension locus group next tag")?,
        next_record_index: next.record_index,
        next_byte_offset: u64_from_index(next_byte_offset),
    }))
}

#[cfg(test)]
mod tests;
