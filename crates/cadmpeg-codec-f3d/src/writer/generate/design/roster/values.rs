// SPDX-License-Identifier: Apache-2.0
//! The values a Base Feature design stores that Fusion's files vary: record
//! timestamps, fresh GUIDs, the unit system, the component name and colour,
//! and the body boxes (f3d.md §3.1, "Records of a Base Feature design").
//! Each is a function of the written document, so two writes of one
//! document are equal.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::units::UnitSymbol;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::ProductDefinitionId;
use cadmpeg_measure::{MeasureInput, MeasureOptions, Measurer, Selection};
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

/// The values of one written design.
pub(in crate::writer::generate) struct Values {
    pub(in crate::writer::generate) stamps: Timestamps,
    /// The seed of every fresh GUID of the archive.
    seed: String,
    /// The unit-system record `AssetSettings` names.
    pub(super) unit_system: u32,
    pub(super) name: String,
    /// The box of each result body, in body order; `None` where the body
    /// has no measured box.
    pub(super) boxes: Vec<Option<BodyBox>>,
}

impl Values {
    pub(in crate::writer::generate) fn new(
        target: &CadIr,
        definition: &ProductDefinitionId,
        model: &[u8],
    ) -> Result<Self, CodecError> {
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
        Ok(Self {
            stamps: Timestamps::FIXED,
            seed: Sha256::digest(model)
                .iter()
                .flat_map(|byte| [byte >> 4, byte & 0x0f])
                .filter_map(|nibble| char::from_digit(u32::from(nibble), 16))
                .collect(),
            unit_system: unit_system(length),
            name,
            boxes: body_boxes(target)?,
        })
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

/// The unit-system record of the document's length unit: `MmMKS` where the
/// document states none or a unit no group models.
fn unit_system(length: Option<UnitSymbol>) -> u32 {
    match length {
        Some(UnitSymbol::Cm) => 47,
        Some(UnitSymbol::M) => 87,
        Some(UnitSymbol::In) => 107,
        Some(UnitSymbol::Ft) => 124,
        _ => 67,
    }
}

/// The measured box of each body, in centimetres.
fn body_boxes(target: &CadIr) -> Result<Vec<Option<BodyBox>>, CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())?;
    let input = MeasureInput {
        ir: target,
        fidelity: None,
        geometry_transferred: true,
    };
    let report = Measurer::new(input, MeasureOptions::default())
        .measure(&ctx, &Selection::default())
        .map_err(|error| CodecError::malformed(format_args!("body boxes: {error}")))?;
    Ok(target
        .geometry
        .bodies
        .iter()
        .map(|body| {
            let row = report
                .bodies()
                .iter()
                .find(|row| row.body() == body.id.as_str())?;
            let measured = row
                .bbox()
                .measured()
                .or_else(|| row.vertex_box().as_ref()?.measured())?;
            let (min, max) = (measured.value().min.get(), measured.value().max.get());
            Some(BodyBox {
                min: [min.x, min.y, min.z].map(|value| value / MM_PER_CM),
                max: [max.x, max.y, max.z].map(|value| value / MM_PER_CM),
            })
        })
        .collect())
}
