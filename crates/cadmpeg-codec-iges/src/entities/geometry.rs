// SPDX-License-Identifier: Apache-2.0
//! Point and analytic curve entity projection.

use crate::directory::DirectoryEntry;
use crate::global::Global;
use crate::parameter::ParameterRecord;
use cadmpeg_ir::ids::{BodyId, PointId, RegionId, ShellId, VertexId};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::report::{LossCategory, LossNote, Severity};
use cadmpeg_ir::topology::{Body, BodyKind, Point, Region, Shell, Vertex};
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) struct Projection {
    pub(crate) handled: BTreeSet<u32>,
    pub(crate) decoded: BTreeSet<u32>,
    pub(crate) losses: Vec<LossNote>,
}

fn point_loss(entry: &DirectoryEntry, message: impl Into<String>) -> LossNote {
    LossNote {
        category: LossCategory::Geometry,
        severity: Severity::Warning,
        message: format!(
            "IGES entity type {} form {} was not projected: {}",
            entry.entity_type,
            entry.form,
            message.into()
        ),
        provenance: None,
    }
}

pub(crate) fn project_points(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    parameters: &[ParameterRecord],
    global: &Global,
) -> Projection {
    let records = parameters
        .iter()
        .map(|record| (record.directory_sequence, record))
        .collect::<BTreeMap<_, _>>();
    let mut handled = BTreeSet::new();
    let mut decoded = BTreeSet::new();
    let mut losses = Vec::new();
    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 116 && entry.form == 0)
    {
        handled.insert(entry.sequence);
        let Some(factor) = global.length_factor_mm() else {
            losses.push(point_loss(entry, "units or model scale are unsupported"));
            continue;
        };
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(point_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let coordinates = [record.number(1), record.number(2), record.number(3)];
        let [Some(x), Some(y), Some(z)] = coordinates else {
            losses.push(point_loss(entry, "X, Y, or Z is not numeric"));
            continue;
        };
        let position = Point3::new(x * factor, y * factor, z * factor);
        if !position.x.is_finite() || !position.y.is_finite() || !position.z.is_finite() {
            losses.push(point_loss(entry, "scaled coordinates are not finite"));
            continue;
        }
        let point = PointId(format!("iges:model:point#D{}", entry.sequence));
        let vertex = VertexId(format!("iges:model:vertex#D{}", entry.sequence));
        ir.model.points.push(Point {
            id: point.clone(),
            position,
        });
        ir.model.vertices.push(Vertex {
            id: vertex,
            point,
            tolerance: None,
        });
        decoded.insert(entry.sequence);
    }
    if !decoded.is_empty() {
        let body = BodyId("iges:model:body#points".into());
        let region = RegionId("iges:model:region#points".into());
        let shell = ShellId("iges:model:shell#points".into());
        ir.model.bodies.push(Body {
            id: body.clone(),
            kind: BodyKind::Wire,
            regions: vec![region.clone()],
            transform: None,
            name: Some("IGES points".into()),
            color: None,
            visible: None,
        });
        ir.model.regions.push(Region {
            id: region.clone(),
            body,
            shells: vec![shell.clone()],
        });
        ir.model.shells.push(Shell {
            id: shell,
            region,
            faces: Vec::new(),
            wire_edges: Vec::new(),
            free_vertices: decoded
                .iter()
                .map(|sequence| VertexId(format!("iges:model:vertex#D{sequence}")))
                .collect(),
        });
    }
    Projection {
        handled,
        decoded,
        losses,
    }
}
