// SPDX-License-Identifier: Apache-2.0
//! Type 116 construction shared by source parts and the complete graph pass.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::{Point, Vertex};

use crate::directory::{DirectoryEntry, Subordinate};
use crate::global::ProjectedGlobal;
use crate::loss::IgesLossCode;
use crate::parameter::ParameterRecord;

use super::{
    base_geometry_use_flag_valid, point_display_symbol_valid, resolve_transform, source_object,
};
use cadmpeg_ir::source_fidelity::parts::PartOutput;

pub(super) struct ProjectedPoint {
    pub(super) point: Point,
    pub(super) vertex: Option<Vertex>,
}

pub(super) fn project(
    entry: &DirectoryEntry,
    record: &ParameterRecord,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    (global, analytic_surface_location): (&ProjectedGlobal, bool),
    ctx: &DecodeContext<'_>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<Option<ProjectedPoint>, CodecError> {
    if (1..=3).any(|index| record.tokens().get(index).is_some_and(Option::is_none)) {
        return Ok(None);
    }
    let factor = global.length_factor_mm();
    let coordinates = [record.number(1), record.number(2), record.number(3)];
    let [Some(x), Some(y), Some(z)] = coordinates else {
        return Err(CodecError::malformed("X, Y, or Z is not numeric"));
    };
    if !point_display_symbol_valid(record, entries, global.global_table()) {
        super::super::push_attributed_loss(
            losses,
            entry,
            IgesLossCode::DisplayDataNotProjected,
            format_args!(
                "Type 116 display symbol pointer is invalid for the effective specification family"
            ),
        );
    }
    let transform = resolve_transform(
        entry.transform,
        entries,
        records,
        factor,
        global.real_precision(),
        &mut BTreeSet::new(),
        ctx,
    )
    .map_err(|error| {
        error
            .non_resource()
            .map_or_else(|error| error, CodecError::malformed)
    })?;
    let position = transform
        .apply_point(Point3::new(x * factor, y * factor, z * factor))
        .ok_or_else(|| CodecError::malformed("placement produces a non-finite point"))?;
    let point = crate::ids::point(&crate::ids::Stem::directory(entry.sequence));
    let vertex = (!entry.status.is_physically_dependent()
        && (entry.status.subordinate() == Some(Subordinate::Independent)
            || !analytic_surface_location))
        .then(|| {
            Vertex::new(
                crate::ids::vertex(&crate::ids::Stem::directory(entry.sequence)),
                point.clone(),
                None,
            )
        });
    Ok(Some(ProjectedPoint {
        point: Point::new(point, position, Some(source_object(entry)?)),
        vertex,
    }))
}

pub(crate) struct SourcePointProjector<'a> {
    entries: BTreeMap<u32, &'a DirectoryEntry>,
    global: ProjectedGlobal,
}

impl<'a> SourcePointProjector<'a> {
    pub(crate) fn new(
        ctx: &DecodeContext<'_>,
        directory: &'a [DirectoryEntry],
        global: ProjectedGlobal,
    ) -> Result<Self, CodecError> {
        let mut entries = BTreeMap::new();
        for entry in directory {
            ctx.retain_bytes(
                cadmpeg_core::decode::u64_from_index(size_of::<(u32, &DirectoryEntry)>()),
                "IGES source point Directory index",
            )?;
            entries.insert(entry.sequence, entry);
        }
        Ok(Self { entries, global })
    }

    pub(crate) fn emit(
        &self,
        ctx: &DecodeContext<'_>,
        entry: &DirectoryEntry,
        record: &ParameterRecord,
        output: &mut PartOutput,
    ) -> Result<(), CodecError> {
        let table = self.global.global_table();
        if entry.entity_type != 116
            || entry.form != 0
            || entry.transform != 0
            || !crate::profile::envelope_a_admits(entry.entity_type, entry.form, table)
            || !entry.status.use_flag(table).is_some_and(|flag| {
                base_geometry_use_flag_valid(entry.entity_type, entry.form, flag, table)
            })
            || !(entry.status.is_physically_dependent()
                || entry.status.subordinate() == Some(Subordinate::Independent))
        {
            return Ok(());
        }
        let mut losses = Vec::new();
        let Some(point) = project(
            entry,
            record,
            &self.entries,
            &BTreeMap::new(),
            (&self.global, false),
            ctx,
            &mut losses,
        )?
        else {
            return Ok(());
        };
        output.insert_model(ctx, point.point)?;
        if let Some(vertex) = point.vertex {
            output.insert_model(ctx, vertex)?;
        }
        for loss in losses {
            output.push_loss(ctx, loss)?;
        }
        Ok(())
    }
}
