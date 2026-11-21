// SPDX-License-Identifier: Apache-2.0
//! The records of the `WorkPlane` items a design writes for the sketches
//! that lie off the origin planes (f3d.md §3.1, "Records of a
//! construction-plane item"): each item's records in Fusion's record order,
//! and the construction collection that lists their plane entities.

use cadmpeg_core::CodecError;

use super::super::base_feature::timestamp_table;
use super::super::frames::DesignRecords;
use super::super::sketch::PlannedPlane;
use super::super::types;
use super::owned::{self, Owned};
use super::sketches::{
    genesis, item_companion, item_tail, placement_base, placement_path, placement_transform,
    tracked_results, transform, Classes, ItemTail, PathRecords, PlacementBase, COMPONENT,
    NULL_GUID, ORIGIN_PLANES, TRACKING_FLAGS,
};
use super::{count, Context, AGGREGATE};
use crate::writer::generate::presentation::TypeRow;

/// The construction collection of the component (type
/// `A8635B5F-1E2C-45FD-8BA9-0173B0FEB75E`).
pub(super) const CONSTRUCTION_COLLECTION: u32 = 11;
/// The record of type `47C245A2-2DBE-4B1F-9C56-762F373BB0C8` the
/// construction collection names.
const RECORD_47C245A2: u32 = 21;
/// The family name of a `WorkPlane` scope.
const FAMILY: &str = "WorkPlane";
/// The construction word of an offset plane (`refType` `2`).
const OFFSET_CONSTRUCTION: u32 = 0;
/// The placement `refType` of an offset plane.
const OFFSET_REF_TYPE: u32 = 2;
/// The plane kind of an offset plane entity.
const PLANE_KIND: u32 = 1;
/// The `EntityGenesis` value of a plane entity.
const PLANE_GENESIS: u64 = 0;
/// The display corners of a plane entity, in plane coordinates in cm: a
/// stored pair `(a, a)`, `(29a, 29a)` of offset planes (f3d-open-items.md
/// DR-1004).
const LOWER_CORNER: [f64; 2] = [0.075, 0.075];
const UPPER_CORNER: [f64; 2] = [2.175, 2.175];
/// The u32 at offset 219 of a `WorkPlane` item's placement path.
const PATH_WORD: u32 = 0;
/// The source kinds of the plane's two parameters.
const ALONG_DISTANCE: &str = "AlongDistance";
const EXTRA_OFFSET: &str = "ExtraOffset";

/// The record indices of one `WorkPlane` item.
#[derive(Clone, Debug)]
pub(in crate::writer::generate::design) struct PlaneRecords {
    pub(in crate::writer::generate::design) entity: u32,
    pub(in crate::writer::generate::design) scope: u32,
    /// The `AlongDistance` parameter, owner and companion.
    pub(in crate::writer::generate::design) along: [u32; 3],
    pub(in crate::writer::generate::design) path: u32,
    path_stamps: u32,
    path_companion: u32,
    first_identity: u32,
    pub(in crate::writer::generate::design) placement: u32,
    placement_stamps: u32,
    placement_companion: u32,
    scope_stamps: u32,
    scope_companion: u32,
    tracked: u32,
    /// The `ExtraOffset` parameter, owner and companion.
    pub(in crate::writer::generate::design) extra: [u32; 3],
    transform: u32,
    target: u32,
    second_identity: u32,
    tracking_flag: u32,
    /// The position of the first `Sketch` item on the plane, which the item
    /// precedes in the timeline.
    pub(in crate::writer::generate::design) before: usize,
}

impl PlaneRecords {
    /// The records of a `WorkPlane` item before `Sketch` item `before`, from
    /// record `next` in Fusion's record order, with their type rows pushed
    /// to `rows`. Returns them and the next free index.
    pub(super) fn allocate(
        before: usize,
        mut next: u32,
        rows: &mut Vec<(u32, TypeRow)>,
    ) -> Result<(Self, u32), CodecError> {
        let mut take = |row: TypeRow| {
            let index = next;
            next = next.checked_add(1).ok_or_else(|| {
                CodecError::NotImplemented("WorkPlane record index overflow".into())
            })?;
            rows.push((index, row));
            Ok::<_, CodecError>(index)
        };
        let records = Self {
            entity: take(types::ORIGIN_PLANE)?,
            scope: take(types::WORKPLANE_SCOPE)?,
            along: [
                take(types::PARAMETER)?,
                take(types::PARAMETER_OWNER)?,
                take(types::RECORD_TIMESTAMPS)?,
            ],
            path: take(types::PLACEMENT_PATH)?,
            path_stamps: take(types::RECORD_TIMESTAMPS)?,
            path_companion: take(types::ITEM_TAIL_COMPANION)?,
            first_identity: take(types::TRACKED_IDENTITY)?,
            placement: take(types::SKETCH_PLACEMENT)?,
            placement_stamps: take(types::RECORD_TIMESTAMPS)?,
            placement_companion: take(types::ITEM_TAIL_COMPANION)?,
            scope_stamps: take(types::RECORD_TIMESTAMPS)?,
            scope_companion: take(types::ITEM_TAIL_COMPANION)?,
            tracked: take(types::TRACKED_RESULTS)?,
            extra: [
                take(types::PARAMETER)?,
                take(types::PARAMETER_OWNER)?,
                take(types::RECORD_TIMESTAMPS)?,
            ],
            transform: take(types::PLACEMENT_TRANSFORM)?,
            target: take(types::TRANSFORM_TARGET)?,
            second_identity: take(types::TRACKED_IDENTITY)?,
            tracking_flag: take(types::PLACEMENT_TRACKING_FLAG)?,
            before,
        };
        Ok((records, next))
    }

    fn path_records(&self) -> PathRecords {
        PathRecords {
            word: PATH_WORD,
            scope: self.scope,
            path: self.path,
            stamps: self.path_stamps,
            companion: self.path_companion,
            first_identity: self.first_identity,
            transform: self.transform,
            target: self.target,
            second_identity: self.second_identity,
        }
    }
}

/// Every record of the `WorkPlane` items of `ctx` and the construction
/// collection that lists their plane entities.
pub(super) fn write(ctx: &Context<'_>, out: &mut DesignRecords) -> Result<(), CodecError> {
    if ctx.roster.planes.is_empty() {
        return Ok(());
    }
    #[cfg(test)]
    if let Some(error) = super::super::requested_plane_failure() {
        return Err(error);
    }
    let classes = Classes::of(ctx)?;
    for (ordinal, (records, planned)) in (1u32..).zip(ctx.roster.planes.iter().zip(ctx.planes)) {
        Item {
            ctx,
            classes: &classes,
            records,
            planned,
            ordinal,
        }
        .write(out)?;
    }
    collection(ctx, out)
}

/// The construction collection with the plane entities of the `WorkPlane`
/// items in timeline order and the `WorkPlane` counter.
fn collection(ctx: &Context<'_>, out: &mut DesignRecords) -> Result<(), CodecError> {
    let entities = ctx
        .roster
        .planes
        .iter()
        .map(|records| records.entity)
        .collect::<Vec<_>>();
    let n = count(entities.len())?;
    out.record(
        &ctx.class_of(CONSTRUCTION_COLLECTION)?,
        CONSTRUCTION_COLLECTION,
    )
    .zeros(9)
    .byte(0)
    .u32(n);
    for entity in &entities {
        out.reference(*entity);
    }
    out.u32(0)
        .u32(n)
        .u32(0)
        .u32(0)
        .byte(1)
        .byte(1)
        .reference(RECORD_47C245A2)
        .byte(0)
        .u32(n);
    for entity in &entities {
        out.reference(*entity);
    }
    out.reference(AGGREGATE);
    Ok(())
}

/// Writes the records of one `WorkPlane` item.
struct Item<'a> {
    ctx: &'a Context<'a>,
    classes: &'a Classes,
    records: &'a PlaneRecords,
    planned: &'a PlannedPlane<'a>,
    /// The item's ordinal in the `WorkPlane` family, from `1`.
    ordinal: u32,
}

impl Item<'_> {
    fn write(&self, out: &mut DesignRecords) -> Result<(), CodecError> {
        let [along, extra] = self.planned.parameters.as_ref().ok_or_else(|| {
            CodecError::NotImplemented("a construction plane without its parameters".into())
        })?;
        self.entity(out)?;
        self.scope(out)?;
        for (records, local, kind, value) in [
            (self.records.along, 0, ALONG_DISTANCE, along),
            (self.records.extra, 1, EXTRA_OFFSET, extra),
        ] {
            let [parameter, owner, companion] = records;
            owned::write(
                self.ctx,
                out,
                &Owned {
                    owner,
                    parameter,
                    companion,
                    scope: self.records.scope,
                    local,
                    kind,
                    value,
                },
            )?;
        }
        let parent = u64::from(ORIGIN_PLANES + self.planned.base.ordinal());
        placement_path(
            self.ctx,
            self.classes,
            out,
            &self.records.path_records(),
            parent,
        )?;
        self.placement(out)?;
        placement_transform(
            self.classes,
            out,
            &self.records.path_records(),
            transform(self.planned.base),
            parent,
        );
        Ok(())
    }

    /// The plane entity: the constructed plane, its display corners and
    /// name, and the tail naming the collection and the scope.
    fn entity(&self, out: &mut DesignRecords) -> Result<(), CodecError> {
        let records = self.records;
        out.record(&self.ctx.class(types::ORIGIN_PLANE.guid)?, records.entity)
            .zeros(9)
            .byte(1)
            .u32(1);
        genesis(out, PLANE_GENESIS)?;
        out.u32(PLANE_KIND)
            .f64(LOWER_CORNER[0])
            .f64(LOWER_CORNER[1])
            .byte(0);
        let [u, v, _] = self.planned.axes();
        for value in self.planned.origin().into_iter().chain(u).chain(v) {
            out.f64(value);
        }
        out.f64(UPPER_CORNER[0])
            .f64(UPPER_CORNER[1])
            .lp_utf16(&format!("0_{}", records.entity))?
            .reference(CONSTRUCTION_COLLECTION)
            .u32(self.ordinal)
            .zeros(5)
            .reference(records.scope)
            .byte(0)
            .byte(1)
            .lp_utf16("")?
            .u32(0)
            .byte(0)
            .zeros(12)
            .f32(1.0)
            .f32(1.0)
            .zeros(5)
            .reference(CONSTRUCTION_COLLECTION);
        Ok(())
    }

    /// The `WorkPlane` scope with its 78-byte item tail, the scope's
    /// record-timestamp table, item-tail companion and tracked-results
    /// table.
    fn scope(&self, out: &mut DesignRecords) -> Result<(), CodecError> {
        let (records, classes) = (self.records, self.classes);
        let stamp = self.ctx.values.stamps.scope;
        out.record(&self.ctx.class(types::WORKPLANE_SCOPE.guid)?, records.scope)
            .zeros(9)
            .byte(0)
            .byte(1)
            .u32(OFFSET_CONSTRUCTION)
            .reference(records.extra[1])
            .byte(1)
            .reference(records.entity)
            .reference(records.placement)
            .byte(1)
            .lp_utf16(NULL_GUID)?
            .zeros(3)
            .u32(4)
            .reference(records.along[1])
            .reference(records.path)
            .reference(records.placement)
            .reference(records.extra[1])
            .u32(u32::MAX)
            .lp_utf16(FAMILY)?;
        item_tail(
            out,
            classes,
            &ItemTail {
                scope: records.scope,
                ordinal: self.ordinal,
                tracked: records.tracked,
                companion: records.scope_companion,
                stamps: records.scope_stamps,
            },
        );
        timestamp_table(
            out,
            &classes.stamps,
            records.scope_stamps,
            true,
            &[
                (COMPONENT, self.ctx.values.stamps.component),
                (records.scope, stamp),
                (records.placement, stamp),
            ],
            (records.scope, stamp),
        )?;
        item_companion(out, &classes.item_companion, records.scope_companion);
        tracked_results(out, classes, records.tracked);
        Ok(())
    }

    /// The offset placement, its record-timestamp table, item-tail companion
    /// and tracking flag.
    fn placement(&self, out: &mut DesignRecords) -> Result<(), CodecError> {
        let (records, classes) = (self.records, self.classes);
        let stamp = self.ctx.values.stamps.scope;
        out.record(&classes.placement, records.placement)
            .zeros(10)
            .byte(0)
            .f64(0.0)
            .f64(0.0)
            .f64(0.0)
            .f64(0.0)
            .byte(0)
            .byte(1)
            .byte(0)
            .reference(records.extra[1])
            .zeros(4)
            .zeros(3)
            .byte(0);
        for value in self.planned.matrix() {
            out.f64(value);
        }
        out.f64(0.0)
            .f64(0.0)
            .f64(0.0)
            .u32(0)
            .byte(0)
            .u32(0)
            .byte(0)
            .byte(0)
            .u32(OFFSET_REF_TYPE)
            .byte(0)
            .u32(1)
            .reference(records.along[1]);
        placement_base(
            out,
            classes,
            &PlacementBase {
                placement: records.placement,
                path: records.path,
                tracking_flag: records.tracking_flag,
                companion: records.placement_companion,
                stamps: records.placement_stamps,
                scope: records.scope,
            },
        );
        timestamp_table(
            out,
            &classes.stamps,
            records.placement_stamps,
            true,
            &[
                (records.along[1], stamp),
                (records.path, stamp),
                (records.extra[1], stamp),
            ],
            (records.placement, stamp),
        )?;
        item_companion(out, &classes.item_companion, records.placement_companion);
        out.record(&classes.tracking_flag, records.tracking_flag)
            .zeros(10)
            .raw(&TRACKING_FLAGS);
        Ok(())
    }
}
