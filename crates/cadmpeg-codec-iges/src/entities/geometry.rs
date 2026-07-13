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

const MAX_TRANSFORM_DEPTH: usize = 64;

#[derive(Clone, Copy)]
struct Affine {
    rows: [[f64; 4]; 3],
}

impl Affine {
    const IDENTITY: Self = Self {
        rows: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ],
    };

    fn compose(self, local: Self) -> Self {
        let mut rows = [[0.0; 4]; 3];
        for (row, values) in rows.iter_mut().enumerate() {
            for (column, value) in values.iter_mut().enumerate().take(3) {
                *value = (0..3)
                    .map(|index| self.rows[row][index] * local.rows[index][column])
                    .sum();
            }
            values[3] = self.rows[row][3]
                + (0..3)
                    .map(|index| self.rows[row][index] * local.rows[index][3])
                    .sum::<f64>();
        }
        Self { rows }
    }

    fn point(self, point: Point3) -> Point3 {
        let values = [point.x, point.y, point.z];
        let coordinate = |row: usize| {
            self.rows[row][3]
                + values
                    .iter()
                    .enumerate()
                    .map(|(column, value)| self.rows[row][column] * value)
                    .sum::<f64>()
        };
        Point3::new(coordinate(0), coordinate(1), coordinate(2))
    }
}

fn resolve_transform(
    sequence: i64,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    length_factor: f64,
    path: &mut BTreeSet<u32>,
) -> Result<Affine, String> {
    if sequence == 0 {
        return Ok(Affine::IDENTITY);
    }
    let sequence = u32::try_from(sequence)
        .map_err(|_| "transformation pointer is not a positive sequence".to_string())?;
    if sequence % 2 == 0 {
        return Err("transformation pointer names an even Directory sequence".into());
    }
    if path.len() >= MAX_TRANSFORM_DEPTH {
        return Err(format!(
            "transformation chain exceeds {MAX_TRANSFORM_DEPTH} entities"
        ));
    }
    if !path.insert(sequence) {
        return Err("transformation chain is cyclic".into());
    }
    let result = (|| {
        let entry = entries
            .get(&sequence)
            .copied()
            .ok_or_else(|| format!("transformation D{sequence} is missing"))?;
        if entry.entity_type != 124 || entry.form != 0 {
            return Err(format!(
                "transformation D{sequence} is type {} form {}, expected type 124 form 0",
                entry.entity_type, entry.form
            ));
        }
        let record = records
            .get(&sequence)
            .copied()
            .ok_or_else(|| format!("transformation D{sequence} parameters are missing"))?;
        let mut values = [0.0; 12];
        for (index, value) in values.iter_mut().enumerate() {
            *value = record.number(index + 1).ok_or_else(|| {
                format!(
                    "transformation D{sequence} coefficient {} is not numeric",
                    index + 1
                )
            })?;
            if !value.is_finite() {
                return Err(format!(
                    "transformation D{sequence} has a non-finite coefficient"
                ));
            }
        }
        for index in [3, 7, 11] {
            values[index] *= length_factor;
        }
        let local = Affine {
            rows: [
                [values[0], values[1], values[2], values[3]],
                [values[4], values[5], values[6], values[7]],
                [values[8], values[9], values[10], values[11]],
            ],
        };
        let parent = resolve_transform(entry.transform, entries, records, length_factor, path)?;
        Ok(parent.compose(local))
    })();
    path.remove(&sequence);
    result
}

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
    let entries = directory
        .iter()
        .map(|entry| (entry.sequence, entry))
        .collect::<BTreeMap<_, _>>();
    let mut handled = BTreeSet::new();
    let mut decoded = BTreeSet::new();
    let mut losses = Vec::new();
    handled.extend(
        directory
            .iter()
            .filter(|entry| entry.entity_type == 124 && entry.form == 0)
            .map(|entry| entry.sequence),
    );
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
        let transform = match resolve_transform(
            entry.transform,
            &entries,
            &records,
            factor,
            &mut BTreeSet::new(),
        ) {
            Ok(transform) => transform,
            Err(message) => {
                losses.push(point_loss(entry, message));
                continue;
            }
        };
        let position = transform.point(Point3::new(x * factor, y * factor, z * factor));
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
