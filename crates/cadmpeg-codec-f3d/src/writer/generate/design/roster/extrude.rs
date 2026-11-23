// SPDX-License-Identifier: Apache-2.0
//! The records of the sketch-driven `Extrude` a design writes in place of
//! its `Base Feature` (f3d.md §3.1, "Records of a sketch-driven Extrude" and
//! "Records of a sketch-and-extrude design"): the scope frame and the two
//! owned parameters. [`super::swept`] writes the item tail, the start record,
//! the profile group and selection, and the tracked results of the side
//! faces and the body.

use cadmpeg_core::CodecError;

use super::super::base_feature::{DURATION, EPOCH};
use super::super::extrude::ExtrudePlan;
use super::super::frames::DesignRecords;
use super::super::sketch::CurveGeometry;
use super::super::types;
use super::owned::Owned;
use super::swept::{
    GroupRecords, ResultRecords, ScopeRecords, SelectionRecords, StartRecords, Swept,
    FIRST_SELECTION_IDENTITY, PROFILE_ROLE,
};
use super::{Context, PARAMETER_DISCRIMINATOR};
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
/// The source kinds of the two owned parameters.
const ALONG_DISTANCE: &str = "AlongDistance";
const TAPER_ANGLE: &str = "TaperAngle";

/// The record indices of the `Extrude` item.
#[derive(Clone, Debug)]
pub(in crate::writer::generate::design) struct ExtrudeRecords {
    pub(in crate::writer::generate::design) scope: ScopeRecords,
    /// The profile group.
    pub(in crate::writer::generate::design) group: GroupRecords,
    pub(in crate::writer::generate::design) start: StartRecords,
    /// The profile selection.
    pub(in crate::writer::generate::design) selection: SelectionRecords,
    /// The `AlongDistance` owner, its parameter and its companion.
    pub(in crate::writer::generate::design) along: [u32; 3],
    /// The `TaperAngle` owner, its parameter and its companion.
    pub(in crate::writer::generate::design) taper: [u32; 3],
    /// The side face of each loop entry and the body.
    results: ResultRecords,
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
        let scope = ScopeRecords::allocate(&mut take, types::EXTRUDE_SCOPE)?;
        let group = GroupRecords::allocate(&mut take)?;
        let start = StartRecords::allocate(&mut take, types::START_RECORD)?;
        let selection = SelectionRecords::allocate(&mut take)?;
        let mut owned = || {
            Ok::<_, CodecError>([
                take(types::PARAMETER_OWNER)?,
                take(types::PARAMETER)?,
                take(types::RECORD_TIMESTAMPS)?,
            ])
        };
        let along = owned()?;
        let taper = owned()?;
        let results = ResultRecords::allocate(&mut take, plan.entries().count())?;
        Ok((
            Self {
                scope,
                group,
                start,
                selection,
                along,
                taper,
                results,
            },
            next,
        ))
    }
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
    let swept = Swept::new(ctx, FAMILY, &records.scope, plan.sketch, plan.preceding)?;
    scope(ctx, &swept, records, out)?;
    swept.group(out, &records.group, records.selection.record, PROFILE_ROLE)?;
    swept.start(
        out,
        &records.start,
        &[records.group.record, records.along[0], records.taper[0]],
    )?;
    swept.selection(out, &records.selection, &plan.profile)?;
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
                scope: records.scope.record,
                local,
                kind,
                value,
                driven: false,
            },
        )?;
    }
    swept.tracked(out, &records.results, plan.entries())?;
    Ok(())
}

/// The scope frame, ended by the item tail [`Swept::tail`] writes.
fn scope(
    ctx: &Context<'_>,
    swept: &Swept<'_>,
    records: &ExtrudeRecords,
    out: &mut DesignRecords,
) -> Result<(), CodecError> {
    let class = ctx.class_of(records.scope.record)?;
    let [_, _, normal] = swept
        .sketch
        .origin_plane()
        .ok_or_else(|| CodecError::Malformed("an Extrude sketch on a construction plane".into()))?
        .axes();
    out.record(&class, records.scope.record)
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
        .reference(records.start.record)
        .byte(0)
        .reference(records.taper[0])
        .byte(0)
        .u32(DISTANCE_EXTENT)
        .zeros(5)
        .u32(0)
        .u32(0)
        .zeros(23)
        .u32(1)
        .reference(records.group.record)
        .u32(0)
        .u32(1)
        .reference(swept.node)
        .u32(0)
        .u32(0)
        .lp_utf16(NULL_GUID)?
        .zeros(3)
        .u32(5);
    for reference in [
        records.group.record,
        records.selection.record,
        records.along[0],
        records.taper[0],
        records.start.record,
    ] {
        out.reference(reference);
    }
    swept.tail(out, records.start.record)
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
            records.scope.record
        ),
        format!(
            "records {along} and {taper} (Extrude parameters: family discriminator {}, DR-911; the ten-byte expression trailer with every flag 0; the parameter tail naming the parameter collection)",
            PARAMETER_DISCRIMINATOR.code()
        ),
        format!(
            "records {} and {} (persistent selection identities: local identities {FIRST_SELECTION_IDENTITY} and {})",
            records.start.identity,
            records.selection.identity,
            FIRST_SELECTION_IDENTITY + 1
        ),
        format!(
            "records {}, {} and {} (record-timestamp tables of the start record, profile group and profile selection: the prefix 01 01)",
            records.start.stamps, records.group.stamps, records.selection.stamps
        ),
        format!(
            "record {} (region identity record: the class of its paired header)",
            records.selection.regions
        ),
        "generic_tag_attrib_def body group of the Extrude's body (Design reference 1)".to_owned(),
        "record 187 (body-presentation owner: the ten bytes before its name)".to_owned(),
        "records 193, 215, 219 to 222 (the second Body owner and the parameter-side records of the import roster, kept where f3d-open-items.md DR-916 states the presentation owner as the result body's only presentation record)".to_owned(),
    ];
    if let Some(plan) = ctx.extrude {
        if plan.profile.iter().filter(|hole| !hole.exterior).count() > 1 {
            entries.push(format!(
                "record {} and the side tokens of its hole loops (the holes stored in the order of their curves' primary identities, a hole of several curves by its smallest, their side tokens in stored order, f3d-open-items.md DR-883)",
                records.selection.regions
            ));
        }
        let arcs = plan.profile.iter().any(|profile_loop| {
            profile_loop.entries.iter().any(|entry| {
                ctx.sketches
                    .get(plan.sketch)
                    .and_then(|sketch| sketch.curves.get(entry.curve))
                    .is_some_and(|curve| matches!(curve.geometry, CurveGeometry::Arc { .. }))
            })
        });
        if arcs {
            entries.push(format!(
                "record {} and the side tokens of each loop that holds an arc (the walk of a loop of lines, with C the loop curve of the largest record index, gives the entry order and the curve of the loop's lowest token, f3d-open-items.md DR-883)",
                records.selection.regions
            ));
        }
        if plan.reversed && (arcs || plan.profile.len() > 1) {
            entries.push(
                "the side tokens of the Extrude's hole loops and loops with arcs under its negative AlongDistance (each loop's tokens run against its ring from its first token, as f3d.md gives for one loop of lines, f3d-open-items.md DR-883)".to_owned(),
            );
        }
    }
    let single = ctx.sketches.len() == 1;
    if !single {
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
