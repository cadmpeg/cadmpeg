// SPDX-License-Identifier: Apache-2.0
//! The records a sketch-driven swept item writes in the same form whatever
//! its family (f3d.md §3.1, "Records of a sketch-driven Extrude"): the end
//! of the scope with its item tail, an operand group, the start record, the
//! persistent selection identities, the profile selection, and the tracked
//! results of the side faces and the body.

use cadmpeg_core::CodecError;

use super::super::base_feature::{timestamp_table, DURATION, EPOCH, FIRST_IDENTITY};
use super::super::extrude::profile::ProfileLoop;
use super::super::frames::DesignRecords;
use super::super::history::{HEAD_STATE, ROOT_STATE};
use super::super::sketch::{CurveGeometry, PlannedSketch};
use super::super::types;
use super::sketches::{
    curve_identity, item_companion, SketchRecords, COMPONENT, CURVE_SECONDARY_IDENTITY,
    OWNER_COLLECTION,
};
use super::{count, Context};
use crate::records::references::DesignClassTag;
use crate::writer::generate::presentation::TypeRow;

/// The operand role of a profile.
pub(super) const PROFILE_ROLE: u64 = 0x0000_0041_0000_0000;
/// The operand role of a revolve axis.
pub(super) const AXIS_ROLE: u64 = 0x0000_0021_0000_0000;
/// The kind word of a profile-loop entry that names a sketch curve.
const CURVE_ENTRY: u32 = 3;
/// The sense word of the curve-record entry of an exterior loop entry.
const FORWARD_SENSE: u32 = 1;
/// The sense word of the curve-record entry of a hole loop entry (f3d.md
/// §3.1, "Tracked results of a profile with circular holes" and the curve
/// record of a side face).
const HOLE_SENSE: u32 = u32::MAX;
/// The local identity of the start record's selection identity; the
/// profile selection's is the next (f3d.md §3.1, "Persistent selection
/// identities": consecutive).
pub(super) const FIRST_SELECTION_IDENTITY: u64 = 4;
/// The key of the first side-face entry.
const FIRST_SIDE_KEY: u64 = 3;

/// The scope, its record-timestamp table, its item-tail companion and its
/// tracked-results table.
#[derive(Clone, Debug)]
pub(in crate::writer::generate::design) struct ScopeRecords {
    pub(in crate::writer::generate::design) record: u32,
    stamps: u32,
    companion: u32,
    pub(super) tracked: u32,
}

impl ScopeRecords {
    /// The records from `take`, the scope of type `row`.
    pub(super) fn allocate(
        take: &mut impl FnMut(TypeRow) -> Result<u32, CodecError>,
        row: TypeRow,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            record: take(row)?,
            stamps: take(types::RECORD_TIMESTAMPS)?,
            companion: take(types::ITEM_TAIL_COMPANION)?,
            tracked: take(types::TRACKED_RESULTS)?,
        })
    }
}

/// An operand group, its identity wrapper, its item-tail companion and its
/// record-timestamp table.
#[derive(Clone, Debug)]
pub(in crate::writer::generate::design) struct GroupRecords {
    pub(in crate::writer::generate::design) record: u32,
    wrapper: u32,
    companion: u32,
    pub(super) stamps: u32,
}

impl GroupRecords {
    /// The records from `take`.
    pub(super) fn allocate(
        take: &mut impl FnMut(TypeRow) -> Result<u32, CodecError>,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            record: take(types::OPERAND_GROUP)?,
            wrapper: take(types::PLACEMENT_TRACKING_FLAG)?,
            companion: take(types::ITEM_TAIL_COMPANION)?,
            stamps: take(types::RECORD_TIMESTAMPS)?,
        })
    }

    /// The records from `take` in the order the decoder reads a closed
    /// construction-operand group: its record-timestamp table at the group's
    /// index plus one, its item-tail companion at plus two, then its
    /// identity wrapper.
    pub(super) fn allocate_closed(
        take: &mut impl FnMut(TypeRow) -> Result<u32, CodecError>,
    ) -> Result<Self, CodecError> {
        let record = take(types::OPERAND_GROUP)?;
        let stamps = take(types::RECORD_TIMESTAMPS)?;
        let companion = take(types::ITEM_TAIL_COMPANION)?;
        Ok(Self {
            record,
            wrapper: take(types::PLACEMENT_TRACKING_FLAG)?,
            companion,
            stamps,
        })
    }
}

/// The start record, its selection identity, its item-tail companion and
/// its record-timestamp table.
#[derive(Clone, Debug)]
pub(in crate::writer::generate::design) struct StartRecords {
    pub(in crate::writer::generate::design) record: u32,
    pub(super) identity: u32,
    companion: u32,
    pub(super) stamps: u32,
}

impl StartRecords {
    /// The records from `take`, the start record of type `row`.
    pub(super) fn allocate(
        take: &mut impl FnMut(TypeRow) -> Result<u32, CodecError>,
        row: TypeRow,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            record: take(row)?,
            identity: take(types::SELECTION_IDENTITY)?,
            companion: take(types::ITEM_TAIL_COMPANION)?,
            stamps: take(types::RECORD_TIMESTAMPS)?,
        })
    }
}

/// The profile selection `N`, its record-timestamp table `N + 1`, item-tail
/// companion `N + 2` and region identity record `N + 3`, and its selection
/// identity.
#[derive(Clone, Debug)]
pub(in crate::writer::generate::design) struct SelectionRecords {
    pub(in crate::writer::generate::design) record: u32,
    pub(super) stamps: u32,
    companion: u32,
    pub(super) regions: u32,
    pub(super) identity: u32,
}

impl SelectionRecords {
    /// The records from `take`.
    pub(super) fn allocate(
        take: &mut impl FnMut(TypeRow) -> Result<u32, CodecError>,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            record: take(types::PROFILE_SELECTION)?,
            stamps: take(types::RECORD_TIMESTAMPS)?,
            companion: take(types::ITEM_TAIL_COMPANION)?,
            regions: take(types::PROFILE_REGIONS)?,
            identity: take(types::SELECTION_IDENTITY)?,
        })
    }
}

/// The side-face entry and curve record of each side face, and the body
/// entry.
#[derive(Clone, Debug)]
pub(super) struct ResultRecords {
    sides: Vec<[u32; 2]>,
    pub(super) body: u32,
}

impl ResultRecords {
    /// The records of `sides` side faces from `take`.
    pub(super) fn allocate(
        take: &mut impl FnMut(TypeRow) -> Result<u32, CodecError>,
        sides: usize,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            sides: (0..sides)
                .map(|_| Ok([take(types::SIDE_ENTRY)?, take(types::CURVE_IDENTITIES)?]))
                .collect::<Result<Vec<_>, CodecError>>()?,
            body: take(types::BODY_ENTRY)?,
        })
    }
}

/// A record-index source for an item's records: each call takes the index
/// after the last from `next` and pushes the record's type row to `rows`.
pub(super) fn allocator<'r>(
    next: &'r mut u32,
    rows: &'r mut Vec<(u32, TypeRow)>,
) -> impl FnMut(TypeRow) -> Result<u32, CodecError> + 'r {
    move |row| {
        let index = *next;
        *next = next
            .checked_add(1)
            .ok_or_else(|| CodecError::NotImplemented("item record index overflow".into()))?;
        rows.push((index, row));
        Ok(index)
    }
}

/// The persistent identity of a swept scope after `preceding` timeline
/// items; the scope owns it and the next.
pub(in crate::writer::generate::design) fn persistent_id(
    preceding: usize,
) -> Result<u64, CodecError> {
    u64::try_from(preceding)
        .ok()
        .and_then(|items| FIRST_IDENTITY.checked_add(items))
        .ok_or_else(|| CodecError::NotImplemented("persistent identity overflow".into()))
}

/// The class tags of the records [`Swept`] writes.
struct Classes {
    object: DesignClassTag,
    tracking_entry: DesignClassTag,
    stamps: DesignClassTag,
    item_companion: DesignClassTag,
    tracked: DesignClassTag,
    group: DesignClassTag,
    wrapper: DesignClassTag,
    identity: DesignClassTag,
    selection: DesignClassTag,
    regions: DesignClassTag,
    side: DesignClassTag,
    curve: DesignClassTag,
    body: DesignClassTag,
}

impl Classes {
    fn of(ctx: &Context<'_>) -> Result<Self, CodecError> {
        let class = |row: TypeRow| ctx.class(row.guid);
        Ok(Self {
            object: class(types::DATA_MODEL_OBJECT)?,
            tracking_entry: class(types::TRACKING_ENTRY)?,
            stamps: class(types::RECORD_TIMESTAMPS)?,
            item_companion: class(types::ITEM_TAIL_COMPANION)?,
            tracked: class(types::TRACKED_RESULTS)?,
            group: class(types::OPERAND_GROUP)?,
            wrapper: class(types::PLACEMENT_TRACKING_FLAG)?,
            identity: class(types::SELECTION_IDENTITY)?,
            selection: class(types::PROFILE_SELECTION)?,
            regions: class(types::PROFILE_REGIONS)?,
            side: class(types::SIDE_ENTRY)?,
            curve: class(types::CURVE_IDENTITIES)?,
            body: class(types::BODY_ENTRY)?,
        })
    }
}

/// Writes the records of a swept item of `ctx` that share their form.
pub(super) struct Swept<'a> {
    ctx: &'a Context<'a>,
    /// The feature-family name of the item.
    family: &'a str,
    classes: Classes,
    scope: &'a ScopeRecords,
    /// The position of the profile's sketch in [`Context::sketches`].
    sketch_index: usize,
    pub(super) sketch: &'a PlannedSketch<'a>,
    pub(super) sketch_records: &'a SketchRecords,
    /// The browser node of the result body and its GUID.
    pub(super) node: u32,
    node_guid: &'a str,
    /// The scope's first persistent identity `E`.
    identity: u64,
    /// The timestamp of every record-timestamp table entry.
    pub(super) stamp: u64,
    /// The asset GUID of the profile selection and its identities.
    pub(super) asset: &'a str,
    /// The context GUID of the selection identities: the third GUID of the
    /// component aggregate.
    pub(super) context: String,
}

impl<'a> Swept<'a> {
    /// The writer of the `family` item with scope `scope` after `preceding`
    /// timeline items, whose profile lies on sketch `sketch`.
    pub(super) fn new(
        ctx: &'a Context<'a>,
        family: &'a str,
        scope: &'a ScopeRecords,
        sketch: usize,
        preceding: usize,
    ) -> Result<Self, CodecError> {
        let missing_sketch = || CodecError::malformed(format_args!("{family} sketch {sketch}"));
        let planned = ctx.sketches.get(sketch).ok_or_else(missing_sketch)?;
        let sketch_records = ctx
            .roster
            .sketches
            .items
            .get(sketch)
            .ok_or_else(missing_sketch)?;
        let (body, roster_body) = ctx
            .roster
            .bodies
            .first()
            .zip(ctx.bodies.first())
            .ok_or_else(|| {
                CodecError::malformed(format_args!("the {family} design has no body"))
            })?;
        Ok(Self {
            ctx,
            family,
            classes: Classes::of(ctx)?,
            scope,
            sketch_index: sketch,
            sketch: planned,
            sketch_records,
            node: body.node,
            node_guid: &roster_body.node_guid,
            identity: persistent_id(preceding)?,
            stamp: ctx.values.stamps.scope,
            asset: crate::manifest::GENERATED_ASSET_FOLDER_GUID,
            context: ctx.values.lower_guid("component-a"),
        })
    }

    /// The end of the scope frame: the current history state, the family
    /// name, the 78-byte item tail and the paired record; then the scope's
    /// record-timestamp table, which lists the start record `start`, and its
    /// item-tail companion.
    pub(super) fn tail(&self, out: &mut DesignRecords, start: u32) -> Result<(), CodecError> {
        let scope = self.scope;
        // The 78-byte item tail: family ordinal 1, no label, state bytes
        // `00 00 00`, the preceding state, the evaluation values, `s0 = s1 =
        // 0`.
        out.u32(HEAD_STATE)
            .lp_utf16(self.family)?
            .u32(1)
            .u32(0)
            .u32(0)
            .u32(0)
            .zeros(3)
            .reference(scope.tracked)
            .byte(0)
            .u32(ROOT_STATE)
            .u32(EPOCH)
            .f64(DURATION)
            .u32(EPOCH)
            .reference(scope.companion)
            .raw(&[1, 0, 0])
            .reference(scope.stamps)
            .zeros(2);
        self.paired(out, scope.record);
        timestamp_table(
            out,
            &self.classes.stamps,
            scope.stamps,
            true,
            &[
                (COMPONENT, self.ctx.values.stamps.component),
                (scope.record, self.stamp),
                (start, self.stamp),
            ],
            (scope.record, self.stamp),
        )?;
        item_companion(out, &self.classes.item_companion, scope.companion);
        Ok(())
    }

    /// The paired header, eight zero bytes and the owner collection.
    pub(super) fn paired(&self, out: &mut DesignRecords, record: u32) {
        out.header(&self.classes.object, record)
            .zeros(8)
            .reference(OWNER_COLLECTION);
    }

    /// The evaluation epochs and duration, the companion, `01 00 00`, the
    /// record-timestamp table, a zero byte and the scope.
    pub(super) fn evaluation(&self, out: &mut DesignRecords, companion: u32, stamps: u32) {
        out.u32(EPOCH)
            .f64(DURATION)
            .u32(EPOCH)
            .reference(companion)
            .raw(&[1, 0, 0])
            .reference(stamps)
            .byte(0)
            .reference(self.scope.record);
    }

    /// The operand group of role `role` whose member is `member`, with its
    /// identity wrapper, item-tail companion and record-timestamp table.
    pub(super) fn group(
        &self,
        out: &mut DesignRecords,
        records: &GroupRecords,
        member: u32,
        role: u64,
    ) -> Result<(), CodecError> {
        let classes = &self.classes;
        out.record(&classes.group, records.record)
            .zeros(10)
            .u32(1)
            .reference(member)
            .zeros(2)
            .u32(1)
            .reference(records.wrapper)
            .u64(role)
            .zeros(10);
        self.evaluation(out, records.companion, records.stamps);
        self.paired(out, records.record);
        out.record(&classes.wrapper, records.wrapper)
            .zeros(10)
            .raw(&[1, 1, 0]);
        item_companion(out, &classes.item_companion, records.companion);
        timestamp_table(
            out,
            &classes.stamps,
            records.stamps,
            true,
            &[(member, self.stamp)],
            (records.record, self.stamp),
        )
    }

    /// The start record with its selection identity, item-tail companion and
    /// record-timestamp table, which lists `inputs`.
    pub(super) fn start(
        &self,
        out: &mut DesignRecords,
        records: &StartRecords,
        inputs: &[u32],
    ) -> Result<(), CodecError> {
        let classes = &self.classes;
        out.record(&self.ctx.class_of(records.record)?, records.record)
            .zeros(10)
            .reference(self.scope.record)
            .u32(1)
            .reference(records.identity);
        self.evaluation(out, records.companion, records.stamps);
        self.paired(out, records.record);
        self.identity(out, records.identity, FIRST_SELECTION_IDENTITY)?;
        item_companion(out, &classes.item_companion, records.companion);
        let entries = inputs
            .iter()
            .map(|input| (*input, self.stamp))
            .collect::<Vec<_>>();
        timestamp_table(
            out,
            &classes.stamps,
            records.stamps,
            true,
            &entries,
            (records.record, self.stamp),
        )
    }

    /// A persistent selection identity: ten zero bytes, its local identity,
    /// the asset and context GUIDs, `u32 2` and an absent optional slot.
    fn identity(&self, out: &mut DesignRecords, record: u32, local: u64) -> Result<(), CodecError> {
        out.record(&self.classes.identity, record)
            .zeros(10)
            .u64(local)
            .lp_utf16(self.asset)?
            .lp_utf16(&self.context)?
            .u32(2)
            .byte(0)
            .u32(0);
        Ok(())
    }

    /// The profile selection of the loops `profile` with its records.
    pub(super) fn selection(
        &self,
        out: &mut DesignRecords,
        records: &SelectionRecords,
        profile: &[ProfileLoop],
    ) -> Result<(), CodecError> {
        let classes = &self.classes;
        out.record(&classes.selection, records.record)
            .zeros(10)
            .reference(records.regions)
            .u32(1)
            .lp_utf16(self.asset)?
            .lp_utf16(&self.sketch_records.container.to_string())?
            .u32(1)
            .u32(0)
            .u32(1)
            .reference(records.identity)
            .u64(PROFILE_ROLE)
            .zeros(10);
        self.evaluation(out, records.companion, records.stamps);
        self.paired(out, records.record);
        timestamp_table(
            out,
            &classes.stamps,
            records.stamps,
            true,
            &[(self.sketch_records.scope, self.stamp)],
            (records.record, self.stamp),
        )?;
        item_companion(out, &classes.item_companion, records.companion);
        out.record(&classes.regions, records.regions)
            .zeros(10)
            .reference(records.record)
            .u32(1)
            .u32(count(profile.len())?);
        for profile_loop in profile {
            out.u32(count(profile_loop.entries.len())?);
            for entry in &profile_loop.entries {
                self.curve_entry(out, entry.curve, entry.traversal_flag())?;
            }
            out.byte(u8::from(profile_loop.exterior));
        }
        out.u32(0);
        out.header(&classes.object, records.regions).zeros(8);
        self.identity(out, records.identity, FIRST_SELECTION_IDENTITY + 1)
    }

    /// A 40-byte profile-loop entry naming sketch curve `curve`.
    fn curve_entry(
        &self,
        out: &mut DesignRecords,
        curve: usize,
        word: u32,
    ) -> Result<(), CodecError> {
        out.u32(CURVE_ENTRY)
            .u64(self.curve_identity(curve)?)
            .u64(CURVE_SECONDARY_IDENTITY)
            .u32(word)
            .u32(1)
            .u32(1)
            .zeros(8);
        Ok(())
    }

    pub(super) fn curve_identity(&self, curve: usize) -> Result<u64, CodecError> {
        curve_identity(self.ctx.sketches, self.sketch_index, curve)
    }

    /// The tracked-results table, a side-face entry and curve record per
    /// loop entry of `entries`, and the body entry.
    pub(super) fn tracked<'p>(
        &self,
        out: &mut DesignRecords,
        records: &ResultRecords,
        entries: impl Iterator<Item = (&'p ProfileLoop, usize)>,
    ) -> Result<(), CodecError> {
        let entries = entries.collect::<Vec<_>>();
        if entries.len() != records.sides.len() {
            return Err(CodecError::malformed(format_args!(
                "{} tracked results: {} side records for {} loop entries",
                self.family,
                records.sides.len(),
                entries.len()
            )));
        }
        let classes = &self.classes;
        let tracked = self.scope.tracked;
        let sides = count(records.sides.len())?;
        out.record(&classes.tracked, tracked)
            .zeros(10)
            .u32(sides + 1);
        for (key, [entry, _]) in (FIRST_SIDE_KEY..).zip(&records.sides) {
            out.reference(*entry).u64(key);
        }
        let body_key = FIRST_SIDE_KEY + u64::from(sides);
        out.reference(records.body)
            .u64(body_key)
            .u64(body_key + 1)
            .header(&classes.object, tracked)
            .zeros(9);
        for ((profile_loop, position), [entry, curve]) in entries.into_iter().zip(&records.sides) {
            out.record(&classes.side, *entry)
                .zeros(9)
                .byte(1)
                .u32(0)
                .reference(*curve)
                .byte(0);
            out.record(&classes.curve, *curve)
                .zeros(10)
                .u32(1)
                .u32(0)
                .u32(1);
            let sense = if profile_loop.exterior {
                FORWARD_SENSE
            } else {
                HOLE_SENSE
            };
            self.curve_entry(out, profile_loop.entries[position].curve, sense)?;
            for neighbours in self.neighbours(profile_loop, position)? {
                out.u32(count(neighbours.len())?);
                for identity in neighbours {
                    out.u64(identity);
                }
            }
        }
        let tokens = (FIRST_SIDE_KEY..body_key)
            .map(|token| token.to_string())
            .collect::<Vec<_>>();
        let mut sorted = tokens.iter().map(String::as_str).collect::<Vec<_>>();
        sorted.sort_unstable();
        let text = sorted.concat();
        out.record(&classes.body, records.body)
            .zeros(11)
            .lp_utf16(self.node_guid)?
            .u32(0)
            .u32(1)
            .lp_ascii(&text)?
            .u32(count(sorted.len() + 1)?);
        let mut offset = 0usize;
        for token in &sorted {
            out.u32(count(offset)?);
            offset += token.len();
        }
        out.u32(count(offset)?)
            .u64(self.identity)
            .byte(0)
            .u32(1)
            .byte(0)
            .byte(0)
            .header(&classes.tracking_entry, records.body)
            .zeros(9);
        Ok(())
    }

    /// The curve identities that meet the side curve of entry `position` of
    /// `profile_loop` at its first and at its second end: for an exterior
    /// entry the first end is the curve's start point, for a hole entry the
    /// point where the ring starts its run of the curve. A circle meets
    /// none.
    fn neighbours(
        &self,
        profile_loop: &ProfileLoop,
        position: usize,
    ) -> Result<[Vec<u64>; 2], CodecError> {
        let ring = &profile_loop.entries;
        let entry = ring[position];
        let curve =
            self.sketch.curves.get(entry.curve).ok_or_else(|| {
                CodecError::malformed(format_args!("sketch curve {}", entry.curve))
            })?;
        if matches!(curve.geometry, CurveGeometry::Circle { .. }) {
            return Ok([Vec::new(), Vec::new()]);
        }
        let before = ring[(position + ring.len() - 1) % ring.len()].curve;
        let after = ring[(position + 1) % ring.len()].curve;
        let (before, after) = (self.curve_identity(before)?, self.curve_identity(after)?);
        Ok(if profile_loop.exterior && entry.reversed {
            [vec![after], vec![before]]
        } else {
            [vec![before], vec![after]]
        })
    }
}
