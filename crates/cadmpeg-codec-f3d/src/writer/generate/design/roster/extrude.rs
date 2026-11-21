// SPDX-License-Identifier: Apache-2.0
//! The records of the sketch-driven `Extrude` a design writes in place of
//! its `Base Feature` (f3d.md §3.1, "Records of a sketch-driven Extrude" and
//! "Records of a sketch-and-extrude design"): the scope with its item tail,
//! the start record, the profile group and selection, the two owned
//! parameters, and the tracked results of the side faces and the body.

use cadmpeg_core::CodecError;

use super::super::base_feature::{timestamp_table, DURATION, EPOCH};
use super::super::extrude::profile::ProfileLoop;
use super::super::extrude::ExtrudePlan;
use super::super::frames::DesignRecords;
use super::super::history::{HEAD_STATE, ROOT_STATE};
use super::super::sketch::{CurveGeometry, PlannedSketch};
use super::super::types;
use super::owned::{Owned, Trailer, DISCRIMINATOR};
use super::sketches::{
    curve_identity, item_companion, SketchRecords, COMPONENT, CURVE_SECONDARY_IDENTITY,
    OWNER_COLLECTION,
};
use super::{count, Context};
use crate::records::references::DesignClassTag;
use crate::writer::generate::presentation::TypeRow;

/// The feature-family name of the scope.
const FAMILY: &str = "Extrude";
/// The GUID the scope frame stores before its scope-reference count.
const NULL_GUID: &str = "00000000-0000-0000-0000-000000000000";
/// The result operation `new body`.
const NEW_BODY: u32 = 4;
/// The travel direction `one side`.
const ONE_SIDE: u32 = 1;
/// The face-extend option of a one-sided blind new body.
const FACE_EXTEND: u32 = 2;
/// The geometry kind `solid`.
const SOLID: u8 = 1;
/// The first-side extent `distance`.
const DISTANCE_EXTENT: u32 = 1;
/// The operand role of a profile.
const PROFILE_ROLE: u64 = 0x0000_0041_0000_0000;
/// The kind word of a profile-loop entry that names a sketch curve.
const CURVE_ENTRY: u32 = 3;
/// The sense word of the curve-record entry of an exterior loop entry.
const FORWARD_SENSE: u32 = 1;
/// The sense word of the curve-record entry of a hole loop entry (f3d.md
/// §3.1, "Tracked results of a profile with circular holes" and the curve
/// record of a side face).
const HOLE_SENSE: u32 = u32::MAX;
/// The source kinds of the two owned parameters.
const ALONG_DISTANCE: &str = "AlongDistance";
const TAPER_ANGLE: &str = "TaperAngle";
/// The local identity of the start record's selection identity; the
/// profile selection's is the next (f3d.md §3.1, "Persistent selection
/// identities": consecutive).
const FIRST_SELECTION_IDENTITY: u64 = 4;
/// The key of the first side-face entry.
const FIRST_SIDE_KEY: u64 = 3;

/// The record indices of the `Extrude` item.
#[derive(Clone, Debug)]
pub(in crate::writer::generate::design) struct ExtrudeRecords {
    pub(in crate::writer::generate::design) scope: u32,
    scope_stamps: u32,
    scope_companion: u32,
    tracked: u32,
    pub(in crate::writer::generate::design) group: u32,
    wrapper: u32,
    group_companion: u32,
    group_stamps: u32,
    pub(in crate::writer::generate::design) start: u32,
    start_identity: u32,
    start_companion: u32,
    start_stamps: u32,
    /// The selection `N`; `N + 1` to `N + 3` follow it.
    pub(in crate::writer::generate::design) selection: u32,
    selection_identity: u32,
    /// The `AlongDistance` owner, its parameter and its companion.
    pub(in crate::writer::generate::design) along: [u32; 3],
    /// The `TaperAngle` owner, its parameter and its companion.
    pub(in crate::writer::generate::design) taper: [u32; 3],
    /// The side-face entry and curve record of each loop entry.
    sides: Vec<[u32; 2]>,
    body_entry: u32,
}

impl ExtrudeRecords {
    /// The records of `plan` from record `next` in creation order, with their
    /// type rows pushed to `rows`. Returns the records and the next free
    /// index.
    pub(super) fn allocate(
        plan: &ExtrudePlan<'_>,
        mut next: u32,
        rows: &mut Vec<(u32, TypeRow)>,
    ) -> Result<(Self, u32), CodecError> {
        let mut take = |row: TypeRow| {
            let index = next;
            next = next.checked_add(1).ok_or_else(|| {
                CodecError::NotImplemented("Extrude record index overflow".into())
            })?;
            rows.push((index, row));
            Ok::<_, CodecError>(index)
        };
        let scope = take(types::EXTRUDE_SCOPE)?;
        let scope_stamps = take(types::RECORD_TIMESTAMPS)?;
        let scope_companion = take(types::ITEM_TAIL_COMPANION)?;
        let tracked = take(types::TRACKED_RESULTS)?;
        let group = take(types::PROFILE_GROUP)?;
        let wrapper = take(types::PLACEMENT_TRACKING_FLAG)?;
        let group_companion = take(types::ITEM_TAIL_COMPANION)?;
        let group_stamps = take(types::RECORD_TIMESTAMPS)?;
        let start = take(types::START_RECORD)?;
        let start_identity = take(types::SELECTION_IDENTITY)?;
        let start_companion = take(types::ITEM_TAIL_COMPANION)?;
        let start_stamps = take(types::RECORD_TIMESTAMPS)?;
        let selection = take(types::PROFILE_SELECTION)?;
        take(types::RECORD_TIMESTAMPS)?;
        take(types::ITEM_TAIL_COMPANION)?;
        take(types::PROFILE_REGIONS)?;
        let selection_identity = take(types::SELECTION_IDENTITY)?;
        let mut owned = || {
            Ok::<_, CodecError>([
                take(types::PARAMETER_OWNER)?,
                take(types::PARAMETER)?,
                take(types::RECORD_TIMESTAMPS)?,
            ])
        };
        let along = owned()?;
        let taper = owned()?;
        let sides = plan
            .entries()
            .map(|_| Ok([take(types::SIDE_ENTRY)?, take(types::CURVE_IDENTITIES)?]))
            .collect::<Result<Vec<_>, CodecError>>()?;
        let body_entry = take(types::BODY_ENTRY)?;
        Ok((
            Self {
                scope,
                scope_stamps,
                scope_companion,
                tracked,
                group,
                wrapper,
                group_companion,
                group_stamps,
                start,
                start_identity,
                start_companion,
                start_stamps,
                selection,
                selection_identity,
                along,
                taper,
                sides,
                body_entry,
            },
            next,
        ))
    }

    /// The profile selection's record-timestamp table `N + 1`, item-tail
    /// companion `N + 2` and region identity record `N + 3`.
    fn selection_records(&self) -> Result<[u32; 3], CodecError> {
        let at = |offset: u32| {
            self.selection
                .checked_add(offset)
                .ok_or_else(|| CodecError::NotImplemented("Extrude record index overflow".into()))
        };
        Ok([at(1)?, at(2)?, at(3)?])
    }
}

/// The class tags of the records of the `Extrude` item.
struct Classes {
    object: DesignClassTag,
    tracking_entry: DesignClassTag,
    scope: DesignClassTag,
    stamps: DesignClassTag,
    item_companion: DesignClassTag,
    tracked: DesignClassTag,
    group: DesignClassTag,
    wrapper: DesignClassTag,
    start: DesignClassTag,
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
            scope: class(types::EXTRUDE_SCOPE)?,
            stamps: class(types::RECORD_TIMESTAMPS)?,
            item_companion: class(types::ITEM_TAIL_COMPANION)?,
            tracked: class(types::TRACKED_RESULTS)?,
            group: class(types::PROFILE_GROUP)?,
            wrapper: class(types::PLACEMENT_TRACKING_FLAG)?,
            start: class(types::START_RECORD)?,
            identity: class(types::SELECTION_IDENTITY)?,
            selection: class(types::PROFILE_SELECTION)?,
            regions: class(types::PROFILE_REGIONS)?,
            side: class(types::SIDE_ENTRY)?,
            curve: class(types::CURVE_IDENTITIES)?,
            body: class(types::BODY_ENTRY)?,
        })
    }
}

/// The persistent identity of the `Extrude` scope after `sketches` `Sketch`
/// items; the scope owns it and the next.
pub(in crate::writer::generate::design) fn persistent_id(
    sketches: usize,
) -> Result<u64, CodecError> {
    u64::try_from(sketches)
        .ok()
        .and_then(|sketches| super::super::base_feature::FIRST_IDENTITY.checked_add(sketches))
        .ok_or_else(|| CodecError::NotImplemented("persistent identity overflow".into()))
}

/// Every record of the `Extrude` item of `ctx`.
pub(super) fn write(
    ctx: &Context<'_>,
    plan: &ExtrudePlan<'_>,
    out: &mut DesignRecords,
) -> Result<(), CodecError> {
    let Some(records) = &ctx.roster.extrude else {
        return Ok(());
    };
    #[cfg(test)]
    if let Some(error) = super::super::requested_extrude_failure() {
        return Err(error);
    }
    let sketch = ctx
        .sketches
        .get(plan.sketch)
        .ok_or_else(|| CodecError::malformed(format_args!("Extrude sketch {}", plan.sketch)))?;
    let sketch_records = ctx
        .roster
        .sketches
        .items
        .get(plan.sketch)
        .ok_or_else(|| CodecError::malformed(format_args!("Extrude sketch {}", plan.sketch)))?;
    let body = ctx
        .roster
        .bodies
        .first()
        .zip(ctx.bodies.first())
        .ok_or_else(|| CodecError::Malformed("an Extrude design has no body".into()))?;
    let item = Item {
        ctx,
        classes: Classes::of(ctx)?,
        plan,
        records,
        sketch,
        sketch_records,
        node: body.0.node,
        node_guid: &body.1.node_guid,
        identity: persistent_id(plan.after)?,
        stamp: ctx.values.stamps.scope,
        asset: crate::manifest::GENERATED_ASSET_FOLDER_GUID,
        context: ctx.values.lower_guid("component-a"),
    };
    item.scope(out)?;
    item.group(out)?;
    item.start(out)?;
    item.selection(out)?;
    for (local, ([owner, parameter, companion], kind, value)) in (0..).zip([
        (records.along, ALONG_DISTANCE, &plan.along),
        (records.taper, TAPER_ANGLE, &plan.taper),
    ]) {
        super::owned::write(
            ctx,
            out,
            &Owned {
                owner,
                parameter,
                companion,
                scope: records.scope,
                local,
                kind,
                trailer: Trailer::Flagged,
                value,
            },
        )?;
    }
    item.tracked(out)?;
    Ok(())
}

/// Writes the records of the `Extrude` item.
struct Item<'a> {
    ctx: &'a Context<'a>,
    classes: Classes,
    plan: &'a ExtrudePlan<'a>,
    records: &'a ExtrudeRecords,
    sketch: &'a PlannedSketch<'a>,
    sketch_records: &'a SketchRecords,
    /// The browser node of the result body and its GUID.
    node: u32,
    node_guid: &'a str,
    /// The scope's first persistent identity `E`.
    identity: u64,
    stamp: u64,
    /// The asset GUID of the profile selection and its identities.
    asset: &'a str,
    /// The context GUID of the selection identities: the third GUID of the
    /// component aggregate.
    context: String,
}

impl Item<'_> {
    /// The scope frame, its 78-byte item tail and paired record, its
    /// record-timestamp table and its item-tail companion.
    fn scope(&self, out: &mut DesignRecords) -> Result<(), CodecError> {
        let (records, classes) = (self.records, &self.classes);
        let [_, _, normal] = self.sketch.plane.axes();
        out.record(&classes.scope, records.scope)
            .zeros(9)
            .byte(1)
            .zeros(6)
            .byte(0)
            .u32(NEW_BODY)
            .u32(ONE_SIDE)
            .u32(FACE_EXTEND)
            .byte(0)
            .byte(SOLID)
            .byte(0)
            .zeros(3);
        for component in normal {
            out.f64(component);
        }
        out.zeros(2)
            .reference(records.along[0])
            .reference(records.start)
            .byte(0)
            .reference(records.taper[0])
            .byte(0)
            .u32(DISTANCE_EXTENT)
            .zeros(5)
            .u32(0)
            .u32(0)
            .zeros(23)
            .u32(1)
            .reference(records.group)
            .u32(0)
            .u32(1)
            .reference(self.node)
            .u32(0)
            .u32(0)
            .lp_utf16(NULL_GUID)?
            .zeros(3)
            .u32(5);
        for reference in [
            records.group,
            records.selection,
            records.along[0],
            records.taper[0],
            records.start,
        ] {
            out.reference(reference);
        }
        // The 78-byte item tail: family ordinal 1, no label, state bytes
        // `00 00 00`, the preceding state, the evaluation values, `s0 = s1 =
        // 0`.
        out.u32(HEAD_STATE)
            .lp_utf16(FAMILY)?
            .u32(1)
            .u32(0)
            .u32(0)
            .u32(0)
            .zeros(3)
            .reference(records.tracked)
            .byte(0)
            .u32(ROOT_STATE)
            .u32(EPOCH)
            .f64(DURATION)
            .u32(EPOCH)
            .reference(records.scope_companion)
            .raw(&[1, 0, 0])
            .reference(records.scope_stamps)
            .zeros(2);
        self.paired(out, records.scope);
        timestamp_table(
            out,
            &classes.stamps,
            records.scope_stamps,
            true,
            &[
                (COMPONENT, self.ctx.values.stamps.component),
                (records.scope, self.stamp),
                (records.start, self.stamp),
            ],
            (records.scope, self.stamp),
        )?;
        item_companion(out, &classes.item_companion, records.scope_companion);
        Ok(())
    }

    /// The paired header, eight zero bytes and the owner collection.
    fn paired(&self, out: &mut DesignRecords, record: u32) {
        out.header(&self.classes.object, record)
            .zeros(8)
            .reference(OWNER_COLLECTION);
    }

    /// The evaluation epochs and duration, the companion, `01 00 00`, the
    /// record-timestamp table, a zero byte and the scope.
    fn evaluation(&self, out: &mut DesignRecords, companion: u32, stamps: u32) {
        out.u32(EPOCH)
            .f64(DURATION)
            .u32(EPOCH)
            .reference(companion)
            .raw(&[1, 0, 0])
            .reference(stamps)
            .byte(0)
            .reference(self.records.scope);
    }

    /// The profile group with its identity wrapper, item-tail companion and
    /// record-timestamp table.
    fn group(&self, out: &mut DesignRecords) -> Result<(), CodecError> {
        let (records, classes) = (self.records, &self.classes);
        out.record(&classes.group, records.group)
            .zeros(10)
            .u32(1)
            .reference(records.selection)
            .zeros(2)
            .u32(1)
            .reference(records.wrapper)
            .u64(PROFILE_ROLE)
            .zeros(10);
        self.evaluation(out, records.group_companion, records.group_stamps);
        self.paired(out, records.group);
        out.record(&classes.wrapper, records.wrapper)
            .zeros(10)
            .raw(&[1, 1, 0]);
        item_companion(out, &classes.item_companion, records.group_companion);
        timestamp_table(
            out,
            &classes.stamps,
            records.group_stamps,
            true,
            &[(records.selection, self.stamp)],
            (records.group, self.stamp),
        )
    }

    /// The start record with its selection identity, item-tail companion and
    /// record-timestamp table.
    fn start(&self, out: &mut DesignRecords) -> Result<(), CodecError> {
        let (records, classes) = (self.records, &self.classes);
        out.record(&classes.start, records.start)
            .zeros(10)
            .reference(records.scope)
            .u32(1)
            .reference(records.start_identity);
        self.evaluation(out, records.start_companion, records.start_stamps);
        self.paired(out, records.start);
        self.identity(out, records.start_identity, FIRST_SELECTION_IDENTITY)?;
        item_companion(out, &classes.item_companion, records.start_companion);
        timestamp_table(
            out,
            &classes.stamps,
            records.start_stamps,
            true,
            &[
                (records.group, self.stamp),
                (records.along[0], self.stamp),
                (records.taper[0], self.stamp),
            ],
            (records.start, self.stamp),
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

    /// The profile selection `N` with `N + 1`, `N + 2`, the region identity
    /// record `N + 3` and its selection identity.
    fn selection(&self, out: &mut DesignRecords) -> Result<(), CodecError> {
        let (records, classes) = (self.records, &self.classes);
        let [stamps, companion, regions] = records.selection_records()?;
        out.record(&classes.selection, records.selection)
            .zeros(10)
            .reference(regions)
            .u32(1)
            .lp_utf16(self.asset)?
            .lp_utf16(&self.sketch_records.container.to_string())?
            .u32(1)
            .u32(0)
            .u32(1)
            .reference(records.selection_identity)
            .u64(PROFILE_ROLE)
            .zeros(10);
        self.evaluation(out, companion, stamps);
        self.paired(out, records.selection);
        timestamp_table(
            out,
            &classes.stamps,
            stamps,
            true,
            &[(self.sketch_records.scope, self.stamp)],
            (records.selection, self.stamp),
        )?;
        item_companion(out, &classes.item_companion, companion);
        out.record(&classes.regions, regions)
            .zeros(10)
            .reference(records.selection)
            .u32(1)
            .u32(count(self.plan.profile.len())?);
        for profile_loop in &self.plan.profile {
            out.u32(count(profile_loop.entries.len())?);
            for entry in &profile_loop.entries {
                self.curve_entry(out, entry.curve, entry.traversal_flag())?;
            }
            out.byte(u8::from(profile_loop.exterior));
        }
        out.u32(0);
        out.header(&classes.object, regions).zeros(8);
        self.identity(
            out,
            records.selection_identity,
            FIRST_SELECTION_IDENTITY + 1,
        )
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

    fn curve_identity(&self, curve: usize) -> Result<u64, CodecError> {
        curve_identity(self.ctx.sketches, self.plan.sketch, curve)
    }

    /// The tracked-results table, a side-face entry and curve record per
    /// loop entry, and the body entry.
    fn tracked(&self, out: &mut DesignRecords) -> Result<(), CodecError> {
        let (records, classes) = (self.records, &self.classes);
        let sides = count(records.sides.len())?;
        out.record(&classes.tracked, records.tracked)
            .zeros(10)
            .u32(sides + 1);
        for (key, [entry, _]) in (FIRST_SIDE_KEY..).zip(&records.sides) {
            out.reference(*entry).u64(key);
        }
        let body_key = FIRST_SIDE_KEY + u64::from(sides);
        out.reference(records.body_entry)
            .u64(body_key)
            .u64(body_key + 1)
            .header(&classes.object, records.tracked)
            .zeros(9);
        for ((profile_loop, position), [entry, curve]) in self.plan.entries().zip(&records.sides) {
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
        out.record(&classes.body, records.body_entry)
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
            .header(&classes.tracking_entry, records.body_entry)
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

/// The values the writer chooses where f3d.md and f3d-open-items.md leave
/// the rule open, in the `Extrude` records and the roster records it
/// changes.
pub(super) fn losses(
    ctx: &Context<'_>,
    records: &ExtrudeRecords,
) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    let [_, along, _] = records.along;
    let [_, taper, _] = records.taper;
    let mut entries = vec![
        format!(
            "record {} (Extrude scope: the byte at offset 27, 0, and the evaluation epoch {EPOCH} in place of the 252 f3d.md states, because the decoder reads an empty-label tail of epoch 252 as named, F3D-EPOCH; the duration {DURATION} of its tail, start record, profile group and profile selection, DR-913)",
            records.scope
        ),
        format!(
            "records {along} and {taper} (Extrude parameters: family discriminator {DISCRIMINATOR}, DR-911; the flagged expression trailer with every flag 0; the parameter tail naming the parameter collection)"
        ),
        format!(
            "records {} and {} (persistent selection identities: local identities {FIRST_SELECTION_IDENTITY} and {})",
            records.start_identity,
            records.selection_identity,
            FIRST_SELECTION_IDENTITY + 1
        ),
        format!(
            "records {}, {} and {} (record-timestamp tables of the start record, profile group and profile selection: the prefix 01 01)",
            records.start_stamps, records.group_stamps, records.selection + 1
        ),
        format!(
            "record {} (region identity record: the class of its paired header)",
            records.selection + 3
        ),
        "generic_tag_attrib_def body group of the Extrude's body (Design reference 1)".to_owned(),
        "record 187 (body-presentation owner: the ten bytes before its name)".to_owned(),
        "records 193, 215, 219 to 222 (the second Body owner and the parameter-side records of the import roster, kept where f3d-open-items.md DR-916 states the presentation owner as the result body's only presentation record)".to_owned(),
    ];
    if let Some(plan) = ctx.extrude {
        if plan.profile.iter().filter(|hole| !hole.exterior).count() > 1 {
            entries.push(format!(
                "record {} and the side tokens of its hole loops (the holes stored in the order of their curves' primary identities, a hole of several curves by its smallest, their side tokens in stored order, f3d-open-items.md DR-883)",
                records.selection + 3
            ));
        }
        if plan.profile.iter().any(|profile_loop| {
            profile_loop.entries.len() > 1
                && profile_loop.entries.iter().any(|entry| {
                    ctx.sketches
                        .get(plan.sketch)
                        .and_then(|sketch| sketch.curves.get(entry.curve))
                        .is_some_and(|curve| matches!(curve.geometry, CurveGeometry::Arc { .. }))
                })
        }) {
            entries.push(format!(
                "record {} and the side tokens of each loop that holds an arc (the walk of a loop of lines, with C the loop curve of the largest record index, gives the entry order and the curve of the loop's lowest token, f3d-open-items.md DR-883)",
                records.selection + 3
            ));
        }
    }
    let single = ctx.sketches.len() == 1;
    if !single {
        entries.push(
            format!("record 198 (timeline context: context value {}, the largest source ordinal of an owned parameter, where f3d-open-items.md DR-882 states no value for this item set)", ctx.context_value()),
        );
        entries.push(
            "records 199, 200 and 201 (owner collection, B65B5682 and dependency table: f3d-open-items.md DR-891's orders, stated for one Sketch item, applied item by item)".to_owned(),
        );
    }
    cadmpeg_ir::codec::write::record_residual_unavailable(
        "F3D",
        "writes the sketch-driven Extrude with these values where the specification states no rule",
        entries,
    )
    .into_iter()
    .collect()
}
