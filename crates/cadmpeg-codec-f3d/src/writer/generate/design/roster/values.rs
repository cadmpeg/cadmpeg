// SPDX-License-Identifier: Apache-2.0
//! The values a Base Feature design stores that Fusion's files vary: record
//! timestamps, fresh GUIDs, the unit system, the component name and colour,
//! and the body boxes (f3d.md §3.1, "Records of a Base Feature design").
//! Each is a function of the written document, so two writes of one
//! document are equal.

use std::collections::HashMap;

use cadmpeg_ir::design::units::UnitSymbol;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::ProductDefinitionId;
use sha2::{Digest, Sha256};

use crate::writer::generate::presentation::deterministic_guid;

/// 2026-01-01T00:00:00 UTC in microseconds: the timestamp of the component
/// record where the document states no save time.
const COMPONENT_TIME: u64 = 1_767_225_600_000_000;
/// Microseconds from the component record's timestamp to the scope's.
const SCOPE_DELAY: u64 = 1000;
/// Microseconds from the scope's timestamp to the result owner's.
const RESULT_DELAY: u64 = 7;
/// Millimetres per centimetre: Design records store lengths in centimetres.
const MM_PER_CM: f64 = 10.0;

/// The timestamps of the three record-timestamp tables, `t_c ≤ t_s ≤ t_r`
/// (f3d.md §3.1, "Record timestamps and evaluation values").
#[derive(Clone, Copy)]
pub(in crate::writer::generate) struct Timestamps {
    pub(in crate::writer::generate) component: u64,
    pub(in crate::writer::generate) scope: u64,
    pub(in crate::writer::generate) result: u64,
}

impl Timestamps {
    pub(in crate::writer::generate) const FIXED: Self = Self {
        component: COMPONENT_TIME,
        scope: COMPONENT_TIME + SCOPE_DELAY,
        result: COMPONENT_TIME + SCOPE_DELAY + RESULT_DELAY,
    };
}

/// An axis-aligned box in centimetres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct BodyBox {
    pub(super) min: [f64; 3],
    pub(super) max: [f64; 3],
}

impl BodyBox {
    /// The six values a Scene record stores: `(xmax, ymax, zmax, xmin,
    /// ymin, zmin)`.
    pub(super) fn scene(&self) -> [f64; 6] {
        let ([x0, y0, z0], [x1, y1, z1]) = (self.min, self.max);
        [x1, y1, z1, x0, y0, z0]
    }

    pub(super) fn centre(&self) -> [f64; 3] {
        [0, 1, 2].map(|axis| f64::midpoint(self.min[axis], self.max[axis]))
    }

    pub(super) fn union(&self, other: &Self) -> Self {
        Self {
            min: [0, 1, 2].map(|axis| self.min[axis].min(other.min[axis])),
            max: [0, 1, 2].map(|axis| self.max[axis].max(other.max[axis])),
        }
    }
}

/// Why a design's unit system fell back to `MmMKS`.
#[derive(Clone, Copy)]
pub(super) enum UnitFallback {
    /// The document declares no length unit.
    Undeclared,
    /// The document's length unit is outside Fusion's five custom unit
    /// systems.
    Unmodeled(UnitSymbol),
}

/// The values of one written design.
pub(in crate::writer::generate) struct Values {
    pub(in crate::writer::generate) stamps: Timestamps,
    /// The seed of every fresh GUID of the archive.
    seed: String,
    /// The unit-system record `AssetSettings` names.
    pub(super) unit_system: u32,
    /// Why the unit system fell back to `MmMKS` (record 67), where it did.
    pub(super) unit_system_fallback: Option<UnitFallback>,
    pub(super) name: String,
    /// The vertex box of each result body, in body order; `None` where the
    /// body's vertices span no extent.
    pub(super) boxes: Vec<Option<BodyBox>>,
}

impl Values {
    pub(in crate::writer::generate) fn new(
        target: &CadIr,
        definition: &ProductDefinitionId,
        model: &[u8],
    ) -> Self {
        let root = target
            .model
            .product_definitions
            .iter()
            .find(|candidate| &candidate.id == definition);
        let length = root
            .and_then(|root| root.units.as_ref())
            .map(|units| units.length);
        let name = root
            .and_then(|root| root.label.as_deref().or(root.source_name.as_deref()))
            .filter(|name| !name.trim().is_empty())
            .unwrap_or("Component1")
            .to_owned();
        let (unit_system, unit_system_fallback) = match unit_system(length) {
            Some(record) => (record, None),
            None => (
                67,
                Some(length.map_or(UnitFallback::Undeclared, UnitFallback::Unmodeled)),
            ),
        };
        Self {
            stamps: Timestamps::FIXED,
            seed: Sha256::digest(model)
                .iter()
                .flat_map(|byte| [byte >> 4, byte & 0x0f])
                .filter_map(|nibble| char::from_digit(u32::from(nibble), 16))
                .collect(),
            unit_system,
            unit_system_fallback,
            name,
            boxes: body_boxes(target),
        }
    }

    /// The fresh GUID of `role` in this archive, in uppercase.
    pub(super) fn guid(&self, role: &str) -> String {
        deterministic_guid(role, &self.seed)
    }

    /// The fresh GUID of `role` in this archive, in lowercase.
    pub(super) fn lower_guid(&self, role: &str) -> String {
        self.guid(role).to_ascii_lowercase()
    }
}

/// The unit-system record of the document's length unit, or `None` when the
/// document states none or a unit outside Fusion's five custom unit systems.
fn unit_system(length: Option<UnitSymbol>) -> Option<u32> {
    Some(match length? {
        UnitSymbol::Mm => 67,
        UnitSymbol::Cm => 47,
        UnitSymbol::M => 87,
        UnitSymbol::In => 107,
        UnitSymbol::Ft => 124,
        _ => return None,
    })
}

/// The box of each body's vertex points, in centimetres; `None` for a body
/// whose vertices span no extent.
fn body_boxes(target: &CadIr) -> Vec<Option<BodyBox>> {
    let geometry = &target.geometry;
    let children = geometry.child_lists();
    let mut body_of_face = HashMap::new();
    for (ordinal, body) in geometry.bodies.iter().enumerate() {
        for region in children.stated_body_regions(&body.id) {
            for shell in region.stated_shells() {
                for face in children.stated_shell_faces(shell) {
                    body_of_face.insert(&face.id, ordinal);
                }
            }
        }
    }
    let face_of_loop = geometry
        .loops
        .iter()
        .map(|face_loop| (&face_loop.id, &face_loop.face))
        .collect::<HashMap<_, _>>();
    let edges = geometry
        .edges
        .iter()
        .map(|edge| (&edge.id, edge))
        .collect::<HashMap<_, _>>();
    let vertices = geometry
        .vertices
        .iter()
        .map(|vertex| (&vertex.id, &vertex.point))
        .collect::<HashMap<_, _>>();
    let points = geometry
        .points
        .iter()
        .map(|point| (&point.id, point.position().get()))
        .collect::<HashMap<_, _>>();
    let mut boxes: Vec<Option<BodyBox>> = vec![None; geometry.bodies.len()];
    for coedge in &geometry.coedges {
        let Some(&body) = face_of_loop
            .get(&coedge.owner_loop)
            .and_then(|face| body_of_face.get(*face))
        else {
            continue;
        };
        let Some(ends) = edges.get(&coedge.edge).and_then(|edge| edge.ends.as_ref()) else {
            continue;
        };
        for vertex in [&ends.start, &ends.end] {
            let Some(point) = vertices.get(vertex).and_then(|point| points.get(*point)) else {
                continue;
            };
            let corner = [point.x, point.y, point.z].map(|value| value / MM_PER_CM);
            let single = BodyBox {
                min: corner,
                max: corner,
            };
            boxes[body] = Some(boxes[body].map_or(single, |found| found.union(&single)));
        }
    }
    boxes
        .into_iter()
        .map(|found| found.filter(|found| (0..3).any(|axis| found.max[axis] > found.min[axis])))
        .collect()
}
