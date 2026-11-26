// SPDX-License-Identifier: Apache-2.0
//! The records of the sketch-driven `Revolve` a design writes in place of
//! its `Base Feature` (f3d.md §3.1, "Records of a sketch-driven Revolve"):
//! the scope frame, the axis group and its direct selection, the axis
//! tracking carrier and its links, and the `AlongAngle` owner.
//! [`super::swept`] writes the item tail, the start record, the profile
//! group and selection, and the tracked results of the side faces and the
//! body.

use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;

use super::super::base_feature::{
    timestamp_table, Dependency, COMPONENT_EDGE, DURATION, EDGE, EPOCH, SKETCH_EDGE,
};
use super::super::frames::DesignRecords;
use super::super::revolve::RevolvePlan;
use super::super::sketch::CurveGeometry;
use super::super::types;
use super::owned::Owned;
use super::sketches::{item_companion, matrix, point_identity, NULL_GUID};
use super::swept::{
    allocator, GroupRecords, Place, ResultRecords, ScopeRecords, SelectionRecords, StartRecords,
    Swept, AXIS_ROLE, PROFILE_ROLE,
};
use super::Context;
use crate::writer::generate::presentation::TypeRow;

/// The feature-family name of the scope.
pub(super) const FAMILY: &str = "Revolve";
/// The counter the timeline context keeps for the family.
pub(super) const COUNTER: &str = "DcRevolveFeatureMetaType";
/// The result operation `new body`.
const NEW_BODY: u32 = 4;
/// The extent kind `angle`.
const ANGLE_EXTENT: u32 = 2;
/// The direction kind of the scope frame.
const DIRECTION_KIND: u8 = 0;
/// The Project Axis flag the writer stores.
const PROJECT_AXIS: u8 = 1;
/// The geometry kind `solid`.
const SOLID: u8 = 1;
/// The number of ordered references of a one-sided scope.
const ORDERED_REFERENCES: u32 = 6;
/// The source kind of the angle parameter.
const ALONG_ANGLE: &str = "AlongAngle";

/// The axis selection `A`, its record-timestamp table `A + 1`, its
/// item-tail companion `A + 2` and its axis curve identity `A + 3`.
#[derive(Clone, Debug)]
struct AxisRecords {
    record: u32,
    stamps: u32,
    companion: u32,
    identity: u32,
}

/// The axis tracking carrier `C` and its records `C + 1` to `C + 6`.
#[derive(Clone, Debug)]
struct CarrierRecords {
    record: u32,
    /// `C + 1`, `C + 3` and `C + 5`: the links to `C + 2`, `C + 4` and
    /// `C + 6`.
    links: [u32; 3],
    /// `C + 2`: the axis curve identity.
    curve: u32,
    /// `C + 4` and `C + 6`: the identities of the line's start point and
    /// end point.
    points: [u32; 2],
}

/// The record indices of the `Revolve` item.
#[derive(Clone, Debug)]
pub(in crate::writer::generate::design) struct RevolveRecords {
    pub(in crate::writer::generate::design) scope: ScopeRecords,
    profile_group: GroupRecords,
    selection: SelectionRecords,
    axis_group: GroupRecords,
    axis: AxisRecords,
    carrier: CarrierRecords,
    /// The `AlongAngle` owner, its parameter and its companion.
    angle: [u32; 3],
    start: StartRecords,
    /// The side face of each loop entry off the axis, and the body.
    results: ResultRecords,
}

impl RevolveRecords {
    /// The records of `plan` from record `next` in creation order, with their
    /// type rows pushed to `rows`. Returns the records and the next free
    /// index.
    pub(super) fn allocate(
        plan: &RevolvePlan<'_>,
        mut next: u32,
        rows: &mut Vec<(u32, TypeRow)>,
    ) -> Result<(Self, u32), CodecError> {
        let records = {
            let mut take = allocator(&mut next, rows);
            let scope = take(types::REVOLVE_SCOPE)?;
            let profile_group = group(&mut take)?;
            let [selection, selection_stamps, selection_companion, regions] = [
                take(types::PROFILE_SELECTION)?,
                take(types::RECORD_TIMESTAMPS)?,
                take(types::ITEM_TAIL_COMPANION)?,
                take(types::PROFILE_REGIONS)?,
            ];
            let axis_group = group(&mut take)?;
            let axis = AxisRecords {
                record: take(types::DIRECT_SELECTION)?,
                stamps: take(types::RECORD_TIMESTAMPS)?,
                companion: take(types::ITEM_TAIL_COMPANION)?,
                identity: take(types::AXIS_CURVE_IDENTITY)?,
            };
            let angle = [
                take(types::PARAMETER_OWNER)?,
                take(types::PARAMETER)?,
                take(types::RECORD_TIMESTAMPS)?,
            ];
            let [start, start_stamps, start_companion] = [
                take(types::REVOLVE_START)?,
                take(types::RECORD_TIMESTAMPS)?,
                take(types::ITEM_TAIL_COMPANION)?,
            ];
            let [scope_stamps, scope_companion, tracked] = [
                take(types::RECORD_TIMESTAMPS)?,
                take(types::ITEM_TAIL_COMPANION)?,
                take(types::TRACKED_RESULTS)?,
            ];
            let results = ResultRecords::allocate(&mut take, plan.entries().count())?;
            let [profile_wrapper, axis_wrapper, start_identity, selection_identity] = [
                take(types::PLACEMENT_TRACKING_FLAG)?,
                take(types::PLACEMENT_TRACKING_FLAG)?,
                take(types::SELECTION_IDENTITY)?,
                take(types::SELECTION_IDENTITY)?,
            ];
            let record = take(types::AXIS_CARRIER)?;
            let curve_link = take(types::CARRIER_LINK)?;
            let curve = take(types::AXIS_CURVE_IDENTITY)?;
            let start_point_link = take(types::CARRIER_LINK)?;
            let start_point = take(types::AXIS_POINT_IDENTITY)?;
            let end_point_link = take(types::CARRIER_LINK)?;
            let end_point = take(types::AXIS_POINT_IDENTITY)?;
            let operand_group = |(record, stamps, companion), wrapper| GroupRecords {
                record,
                wrapper,
                companion,
                stamps,
            };
            Self {
                scope: ScopeRecords {
                    record: scope,
                    stamps: scope_stamps,
                    companion: scope_companion,
                    tracked,
                },
                profile_group: operand_group(profile_group, profile_wrapper),
                selection: SelectionRecords {
                    record: selection,
                    stamps: selection_stamps,
                    companion: selection_companion,
                    regions,
                    identity: selection_identity,
                },
                axis_group: operand_group(axis_group, axis_wrapper),
                axis,
                carrier: CarrierRecords {
                    record,
                    links: [curve_link, start_point_link, end_point_link],
                    curve,
                    points: [start_point, end_point],
                },
                angle,
                start: StartRecords {
                    record: start,
                    identity: start_identity,
                    companion: start_companion,
                    stamps: start_stamps,
                },
                results,
            }
        };
        Ok((records, next))
    }

    /// The six ordered references of the scope, in ascending record index
    /// (f3d.md §3.1, "Revolve scope frame").
    fn ordered(&self) -> [u32; 6] {
        let mut ordered = [
            self.profile_group.record,
            self.selection.record,
            self.axis_group.record,
            self.axis.record,
            self.angle[0],
            self.start.record,
        ];
        ordered.sort_unstable();
        ordered
    }

    /// The members the owner collection lists for the `Revolve`: its ordered
    /// references, then its scope.
    pub(super) fn owners(&self) -> Vec<u32> {
        let mut owners = self.ordered().to_vec();
        owners.push(self.scope.record);
        owners
    }

    /// The members the `B65B5682` record lists for the `Revolve`: each
    /// selection before its group, the angle owner, the start record and the
    /// scope.
    pub(super) fn context_members(&self) -> Vec<u32> {
        vec![
            self.selection.record,
            self.profile_group.record,
            self.axis.record,
            self.axis_group.record,
            self.angle[0],
            self.start.record,
            self.scope.record,
        ]
    }

    /// The angle parameter record.
    pub(super) fn parameters(&self) -> Vec<u32> {
        vec![self.angle[1]]
    }

    /// The dependency-table inputs and dependents of the `Revolve` whose
    /// profile and axis lie on the `Sketch` scope `profile`.
    pub(super) fn dependencies(
        &self,
        component: u32,
        profile: u32,
    ) -> (Vec<Dependency>, Vec<Dependency>) {
        let (scope, profile_group, selection, axis_group, axis, angle, start) = (
            self.scope.record,
            self.profile_group.record,
            self.selection.record,
            self.axis_group.record,
            self.axis.record,
            self.angle[0],
            self.start.record,
        );
        let inputs = vec![
            (scope, vec![(component, COMPONENT_EDGE), (start, EDGE)]),
            (profile_group, vec![(selection, EDGE)]),
            (selection, vec![(profile, SKETCH_EDGE)]),
            (axis_group, vec![(axis, EDGE)]),
            (axis, vec![(profile, SKETCH_EDGE)]),
            (
                start,
                vec![(profile_group, EDGE), (axis_group, EDGE), (angle, EDGE)],
            ),
        ];
        let dependents = vec![
            (profile, vec![(selection, SKETCH_EDGE), (axis, SKETCH_EDGE)]),
            (profile_group, vec![(start, EDGE)]),
            (axis_group, vec![(start, EDGE)]),
            (start, vec![(scope, EDGE)]),
            (selection, vec![(profile_group, EDGE)]),
            (axis, vec![(axis_group, EDGE)]),
            (angle, vec![(start, EDGE)]),
        ];
        (inputs, dependents)
    }
}

/// A construction-operand group with its record-timestamp table and item-tail
/// companion after it.
fn group(
    take: &mut impl FnMut(TypeRow) -> Result<u32, CodecError>,
) -> Result<(u32, u32, u32), CodecError> {
    Ok((
        take(types::OPERAND_GROUP)?,
        take(types::RECORD_TIMESTAMPS)?,
        take(types::ITEM_TAIL_COMPANION)?,
    ))
}

/// Every record of the `Revolve` item of `ctx`.
pub(super) fn write(
    ctx: &Context<'_>,
    plan: &RevolvePlan<'_>,
    records: &RevolveRecords,
    place: &Place,
    out: &mut DesignRecords,
) -> Result<(), CodecError> {
    let swept = Swept::new(ctx, FAMILY, &records.scope, plan.sketch, place)?;
    scope(ctx, &swept, records, out)?;
    swept.group(
        out,
        &records.profile_group,
        records.selection.record,
        PROFILE_ROLE,
    )?;
    swept.selection(out, &records.selection, &plan.profile)?;
    swept.group(out, &records.axis_group, records.axis.record, AXIS_ROLE)?;
    axis(ctx, &swept, records, plan, out)?;
    carrier(ctx, &swept, records, plan, out)?;
    let [owner, parameter, companion] = records.angle;
    super::owned::write(
        ctx,
        out,
        &Owned {
            owner,
            parameter,
            companion,
            scope: records.scope.record,
            local: 0,
            kind: ALONG_ANGLE,
            value: &plan.angle,
            driven: false,
        },
    )?;
    swept.start(
        out,
        &records.start,
        &[
            records.profile_group.record,
            records.axis_group.record,
            owner,
        ],
    )?;
    swept.tracked(out, &records.results, plan.entries())
}

/// The one-sided fixed-angle version-2 scope frame
/// (`sketch_revolve_scope_frame`), ended by the item tail [`Swept::tail`]
/// writes.
fn scope(
    ctx: &Context<'_>,
    swept: &Swept<'_>,
    records: &RevolveRecords,
    out: &mut DesignRecords,
) -> Result<(), CodecError> {
    out.record(&ctx.class_of(records.scope.record)?, records.scope.record)
        .zeros(9)
        .byte(1)
        .u32(0)
        .u32(NEW_BODY)
        .u32(ANGLE_EXTENT)
        .byte(DIRECTION_KIND)
        .byte(PROJECT_AXIS)
        .byte(SOLID)
        .byte(0)
        .byte(0)
        .byte(0)
        .reference(records.angle[0])
        .reference(records.axis_group.record)
        .byte(0)
        .reference(records.start.record)
        .byte(0)
        .u32(1)
        .reference(records.profile_group.record)
        .u32(0)
        .u32(1)
        .reference(swept.node)
        .u32(0)
        .u32(0)
        .lp_utf16(NULL_GUID)?
        .zeros(3)
        .u32(ORDERED_REFERENCES);
    for reference in records.ordered() {
        out.reference(reference);
    }
    swept.tail(out, records.start.record)
}

/// The axis selection `A` of the axis line, with its record-timestamp
/// table, which lists the `Sketch` scope and the selection, its item-tail
/// companion and its axis curve identity.
fn axis(
    ctx: &Context<'_>,
    swept: &Swept<'_>,
    records: &RevolveRecords,
    plan: &RevolvePlan<'_>,
    out: &mut DesignRecords,
) -> Result<(), CodecError> {
    let axis = &records.axis;
    out.record(&ctx.class_of(axis.record)?, axis.record)
        .zeros(10)
        .reference(axis.identity)
        .u32(1)
        .lp_utf16(swept.asset)?
        .lp_utf16(&swept.context)?
        .u32(2)
        .u32(0)
        .u32(1)
        .reference(records.carrier.record)
        .u64(AXIS_ROLE)
        .zeros(10);
    swept.evaluation(out, axis.companion, axis.stamps);
    swept.paired(out, axis.record);
    timestamp_table(
        out,
        &ctx.class_of(axis.stamps)?,
        axis.stamps,
        true,
        &[
            (swept.sketch_records.scope, swept.stamp),
            (axis.record, swept.stamp),
        ],
        (axis.record, swept.stamp),
    )?;
    item_companion(out, &ctx.class_of(axis.companion)?, axis.companion);
    curve_identity(ctx, swept, axis.identity, plan.axis, out)
}

/// A 45-byte axis curve identity naming the sketch curve `curve`.
fn curve_identity(
    ctx: &Context<'_>,
    swept: &Swept<'_>,
    record: u32,
    curve: usize,
    out: &mut DesignRecords,
) -> Result<(), CodecError> {
    out.record(&ctx.class_of(record)?, record)
        .zeros(18)
        .u64(u64::from(swept.sketch_records.container))
        .u64(swept.curve_identity(curve)?);
    Ok(())
}

/// The axis tracking carrier `C`: the axis line's start point and its end
/// point minus its start point through the Sketch frame record's matrix, in
/// cm, and its links to the curve identity `C + 2` and the point identities
/// `C + 4` and `C + 6`.
fn carrier(
    ctx: &Context<'_>,
    swept: &Swept<'_>,
    records: &RevolveRecords,
    plan: &RevolvePlan<'_>,
    out: &mut DesignRecords,
) -> Result<(), CodecError> {
    let carrier = &records.carrier;
    let sketch = swept.sketch;
    let CurveGeometry::Line { start, end } = sketch
        .curves
        .get(plan.axis)
        .map(|curve| &curve.geometry)
        .ok_or_else(|| CodecError::malformed(format_args!("Revolve axis curve {}", plan.axis)))?
    else {
        return Err(CodecError::Malformed(
            "a Revolve axis that is no line".into(),
        ));
    };
    let at = |point: usize| {
        sketch
            .points
            .get(point)
            .copied()
            .ok_or_else(|| CodecError::malformed(format_args!("sketch point {point}")))
    };
    let (from, to) = (at(*start)?, at(*end)?);
    let placement = sketch
        .origin_plane()
        .map(matrix)
        .ok_or_else(|| CodecError::Malformed("a Revolve sketch on a construction plane".into()))?;
    let identity = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];
    let m = placement.unwrap_or(identity);
    let row =
        |i: usize, [x, y]: [f64; 2], w: f64| m[4 * i] * x + m[4 * i + 1] * y + m[4 * i + 3] * w;
    let run = [to[0] - from[0], to[1] - from[1]];
    let [link_curve, link_start, link_end] = carrier.links;
    out.record(&ctx.class_of(carrier.record)?, carrier.record)
        .zeros(10);
    for i in 0..3 {
        out.f64(row(i, from, 1.0));
    }
    for i in 0..3 {
        out.f64(row(i, run, 0.0));
    }
    out.reference(link_end).byte(0).reference(link_start);
    match placement {
        None => {
            out.byte(1);
        }
        Some(matrix) => {
            out.byte(0);
            for value in matrix {
                out.f64(value);
            }
        }
    }
    out.reference(link_curve);
    for (link, next) in [
        (link_curve, carrier.curve),
        (link_start, carrier.points[0]),
        (link_end, carrier.points[1]),
    ] {
        out.record(&ctx.class_of(link)?, link)
            .zeros(10)
            .reference(next)
            .byte(0);
    }
    curve_identity(ctx, swept, carrier.curve, plan.axis, out)?;
    let container = u64::from(swept.sketch_records.container);
    for (record, point) in carrier.points.into_iter().zip([*start, *end]) {
        out.record(&ctx.class_of(record)?, record)
            .zeros(10)
            .u64(container)
            .u64(point_identity(ctx.sketches, plan.sketch, point)?);
    }
    Ok(())
}

/// The values the writer chooses where f3d.md and f3d-open-items.md leave
/// the rule open, in the `Revolve` records and the roster records it
/// changes.
pub(super) fn losses(
    ctx: &Context<'_>,
    plan: &RevolvePlan<'_>,
    records: &RevolveRecords,
    stated: bool,
) -> Vec<LossNote> {
    let mut entries = vec![
        format!(
            "record {} (Revolve scope: the one-sided version-2 frame of a plain-reference stream with its zero u32 at offset 89 and the 78-byte item tail, f3d-open-items.md DR-1043; Project Axis flag {PROJECT_AXIS}; the evaluation epoch {EPOCH} and the duration {DURATION} of its tail, start record, groups and selections, DR-913; its record-timestamp table listing the scope and the start record after the component)",
            records.scope.record
        ),
        format!(
            "record {} (AlongAngle parameter: family discriminator {}, DR-911; the ten-byte expression trailer with every flag 0; the parameter tail naming the parameter collection)",
            records.angle[1],
            super::PARAMETER_DISCRIMINATOR.code()
        ),
        format!(
            "records {} and {} (axis curve identities: the 45-byte form, f3d-open-items.md DR-1041)",
            records.axis.identity, records.carrier.curve
        ),
        format!(
            "records {}, {} and {} (carrier links: the 33-byte form, f3d-open-items.md DR-1041)",
            records.carrier.links[0], records.carrier.links[1], records.carrier.links[2]
        ),
        format!(
            "record {} and its side entries (tracked results: one side entry in the 37-byte form for each profile loop entry whose curve does not lie on the axis, keyed from 3 in the side-face walk of an Extrude's loop, then one body entry, f3d-open-items.md DR-1040 and DR-1041)",
            records.scope.tracked
        ),
        format!(
            "record {} (record-timestamp table of the profile selection: the prefix 01 01)",
            records.selection.stamps
        ),
        format!(
            "records {} and {} (persistent selection identities: local identities {} and {})",
            records.start.identity,
            records.selection.identity,
            super::swept::FIRST_SELECTION_IDENTITY,
            super::swept::FIRST_SELECTION_IDENTITY + 1
        ),
        format!(
            "record {} (region identity record: the class of its paired header)",
            records.selection.regions
        ),
        format!(
            "records 198 to 203 (timeline context counter {COUNTER} = 1, owner collection, B65B5682, dependency table and persistent-id table: the Revolve's members, edges and two persistent identities in the orders f3d-open-items.md DR-882 and DR-891 state for an Extrude)"
        ),
        format!(
            "records {} to {} (the allocation order of the Revolve's records: the scope, the groups and selections each followed by its own records, the AlongAngle owner, the start record with its own records, the scope's record-timestamp table, companion and tracked results, the side and body entries, then the identity wrappers, the two selection identities and the axis tracking carrier)",
            records.scope.record,
            records.carrier.points[1]
        ),
    ];
    if stated {
        entries.extend([
            "the Revolve's result body (browser-node member, body-presentation owner and Body owner in the forms of an Extrude's result body, f3d-open-items.md DR-1044)".to_owned(),
            "generic_tag_attrib_def body group of the Revolve's body (Design reference 1)".to_owned(),
            "records 193, 215, 219 to 222 (the second Body owner and the parameter-side records of the import roster, kept where f3d-open-items.md DR-916 states the presentation owner as the result body's only presentation record)".to_owned(),
        ]);
    }
    let arcs = plan.profile.iter().any(|profile_loop| {
        profile_loop.entries.iter().any(|entry| {
            ctx.sketches
                .get(plan.sketch)
                .and_then(|sketch| sketch.curves.get(entry.curve))
                .is_some_and(|curve| matches!(curve.geometry, CurveGeometry::Arc { .. }))
        })
    });
    if arcs || plan.profile.len() > 1 {
        entries.push(format!(
            "record {} and the side tokens of its loops with arcs or holes (the walk of a loop of lines and the hole order of an Extrude, f3d-open-items.md DR-883 and DR-1040)",
            records.selection.regions
        ));
    }
    if ctx.sketches.len() > 1 {
        entries.push(
            "records 199, 200 and 201 (owner collection, B65B5682 and dependency table: f3d-open-items.md DR-891's orders, stated for one Sketch item, applied item by item)".to_owned(),
        );
    }
    cadmpeg_ir::codec::write::record_residual_unavailable(
        "F3D",
        "writes the sketch-driven Revolve with these values where the specification states no rule",
        entries,
    )
    .into_iter()
    .collect()
}
