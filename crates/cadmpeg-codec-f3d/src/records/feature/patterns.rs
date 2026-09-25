// SPDX-License-Identifier: Apache-2.0
//! Circular and rectangular pattern constructions and their instances.

use crate::records::identity::Located;
use crate::records::mesh::DesignRelaxedGuidText;
use crate::records::sketch_placement::SketchPlacementMatrix;
use cadmpeg_ir::features::{FinitePoint3, FiniteVector3};
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::units::UnitVector3;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

cadmpeg_core::named_optional_field!(
    deserialize_component_occurrences,
    DesignComponentPatternOccurrencesWire,
    "component_occurrences"
);
cadmpeg_core::named_optional_field!(
    deserialize_instances,
    DesignRectangularPatternInstances,
    "instances"
);
/// Exact construction carried by a fixed circular-pattern scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignCircularPatternConstruction {
    /// Positive total instance count, including the seed.
    pub(crate) count: u32,
    /// Referenced compact count-parameter owner.
    pub(crate) count_record_index: u32,
    /// Byte offset of the evaluated count scalar.
    pub(crate) count_offset: u64,
    /// Angular span.
    pub(crate) angle: cadmpeg_ir::scalar::PositiveAngle,
    /// Referenced total-angle scalar.
    pub(crate) angle_record_index: u32,
    /// Byte offset of the total-angle scalar.
    pub(crate) angle_offset: u64,
    /// Serialized axis construction and its resolved placement.
    pub(crate) axis: DesignCircularPatternAxis,
    /// Referenced axis record.
    pub(crate) axis_record_index: u32,
    /// Referenced persistent selection operand.
    pub(crate) selection_record_index: u32,
}

/// Proven origin and unit direction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DesignAxis {
    pub(crate) origin: FinitePoint3,
    pub(crate) direction: UnitVector3,
}

impl DesignAxis {
    pub(crate) fn from_parts(origin: FinitePoint3, direction: FiniteVector3) -> Option<Self> {
        Some(Self {
            origin,
            direction: UnitVector3::normalized(direction.get())?,
        })
    }
}

/// Proven origin and unit normal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DesignPlane {
    pub(crate) origin: FinitePoint3,
    pub(crate) normal: FiniteVector3,
}

/// Axis construction carried by a fixed circular-pattern scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignCircularPatternAxisWire",
    into = "DesignCircularPatternAxisWire"
)]
pub(crate) enum DesignCircularPatternAxis {
    /// Axis coordinates stored directly in the Design record.
    Inline {
        /// Axis origin in source centimetres.
        origin: [FiniteReal; 3],
        /// Byte offset of the first origin coordinate.
        origin_offset: u64,
        /// Axis direction; the decoder stores the serialized displacement at unit
        /// length.
        direction: UnitVector3,
        /// Byte offset of the first direction coordinate.
        direction_offset: u64,
    },
    /// Axis selected through wrappers of one persistent historical topology identity.
    HistoricalEdge {
        /// Referenced wrappers and the offsets of their shared identity.
        wrappers: Vec<DesignPatternAxisWrapper>,
        /// Persistent ASM identity shared by the wrappers.
        persistent_identity: u64,
        /// Resolved model-space axis, when exact.
        resolved: Option<DesignAxis>,
    },
}

/// One historical axis wrapper and the location of its persistent identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DesignPatternAxisWrapper {
    pub(crate) record_index: u32,
    pub(crate) identity_offset: u64,
}

/// Axis construction carried by a fixed circular-pattern scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum DesignCircularPatternAxisWire {
    /// Axis coordinates stored directly in the Design record.
    Inline {
        /// Axis origin in source centimetres.
        origin: [FiniteReal; 3],
        /// Byte offset of the first origin coordinate.
        origin_offset: u64,
        /// Axis direction; the decoder stores the serialized displacement at unit
        /// length.
        direction: [f64; 3],
        /// Byte offset of the first direction coordinate.
        direction_offset: u64,
    },
    /// Axis selected through wrappers of one persistent historical topology identity.
    HistoricalEdge {
        /// Referenced Design wrapper records, in serialized order.
        wrapper_record_indices: Vec<u32>,
        /// Persistent ASM identities carried by the wrappers.
        persistent_identities: Vec<u64>,
        /// Identity byte offsets parallel to `wrapper_record_indices`.
        identity_offsets: Vec<u64>,
        /// Resolved model-space axis, when exact.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resolved_origin: Option<FinitePoint3>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resolved_direction: Option<FiniteVector3>,
    },
}

const EPS_CIRCULAR_PATTERN_AXIS_UNIT: f64 = 1.0e-12;

impl DesignCircularPatternAxis {
    pub(crate) fn inline(
        origin: [FiniteReal; 3],
        origin_offset: u64,
        displacement: [f64; 3],
        direction_offset: u64,
    ) -> Option<Self> {
        let raw = Vector3::from(displacement);
        let length = raw.x.hypot(raw.y).hypot(raw.z);
        if !length.is_finite() || length <= f64::EPSILON {
            return None;
        }
        let direction = if (length - 1.0).abs() <= EPS_CIRCULAR_PATTERN_AXIS_UNIT {
            UnitVector3::new(raw)?
        } else {
            UnitVector3::normalized(raw)?
        };
        Some(Self::Inline {
            origin,
            origin_offset,
            direction,
            direction_offset,
        })
    }
}

impl TryFrom<DesignCircularPatternAxisWire> for DesignCircularPatternAxis {
    type Error = String;

    fn try_from(wire: DesignCircularPatternAxisWire) -> Result<Self, Self::Error> {
        match wire {
            DesignCircularPatternAxisWire::Inline {
                origin,
                origin_offset,
                direction,
                direction_offset,
            } => Self::inline(origin, origin_offset, direction, direction_offset)
                .ok_or_else(|| "inline axis direction must be nonzero and finite".into()),
            DesignCircularPatternAxisWire::HistoricalEdge {
                wrapper_record_indices,
                persistent_identities,
                identity_offsets,
                resolved_origin,
                resolved_direction,
            } => {
                if wrapper_record_indices.len() != identity_offsets.len() {
                    return Err(
                        "wrapper_record_indices and identity_offsets must have equal lengths"
                            .into(),
                    );
                }
                let [persistent_identity] = persistent_identities.as_slice() else {
                    return Err("persistent_identities must contain one shared identity".into());
                };
                let resolved = match (resolved_origin, resolved_direction) {
                    (None, None) => None,
                    (Some(origin), Some(direction)) => Some(
                        DesignAxis::from_parts(origin, direction)
                            .ok_or("resolved axis direction must be nonzero")?,
                    ),
                    _ => {
                        return Err(
                            "resolved_origin and resolved_direction must occur together".into()
                        )
                    }
                };
                Ok(Self::HistoricalEdge {
                    wrappers: wrapper_record_indices
                        .into_iter()
                        .zip(identity_offsets)
                        .map(|(record_index, identity_offset)| DesignPatternAxisWrapper {
                            record_index,
                            identity_offset,
                        })
                        .collect(),
                    persistent_identity: *persistent_identity,
                    resolved,
                })
            }
        }
    }
}

impl From<DesignCircularPatternAxis> for DesignCircularPatternAxisWire {
    fn from(axis: DesignCircularPatternAxis) -> Self {
        match axis {
            DesignCircularPatternAxis::Inline {
                origin,
                origin_offset,
                direction,
                direction_offset,
            } => Self::Inline {
                origin,
                origin_offset,
                direction: [
                    direction.as_raw().x,
                    direction.as_raw().y,
                    direction.as_raw().z,
                ],
                direction_offset,
            },
            DesignCircularPatternAxis::HistoricalEdge {
                wrappers,
                persistent_identity,
                resolved,
            } => Self::HistoricalEdge {
                wrapper_record_indices: wrappers
                    .iter()
                    .map(|wrapper| wrapper.record_index)
                    .collect(),
                persistent_identities: vec![persistent_identity],
                identity_offsets: wrappers
                    .iter()
                    .map(|wrapper| wrapper.identity_offset)
                    .collect(),
                resolved_origin: resolved.map(|axis| axis.origin),
                resolved_direction: resolved.map(|axis| FiniteVector3::from(axis.direction)),
            },
        }
    }
}

/// Ordered scalar lanes carried by a rectangular-pattern scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignRectangularPatternConstructionWire",
    into = "DesignRectangularPatternConstructionWire"
)]
pub(crate) struct DesignRectangularPatternConstruction {
    /// Positive U-direction instance count, including the seed.
    u_count: NonZeroU32,
    /// Positive V-direction instance count, including the seed.
    v_count: NonZeroU32,
    /// Signed U-direction seed-to-final-instance span in source centimetres.
    u_extent: f64,
    /// Signed V-direction seed-to-final-instance span in source centimetres.
    v_extent: f64,
    /// Parameter-owner records for U count, V count, U extent, and V extent.
    pub(crate) owner_record_indices: [u32; 4],
    /// Evaluated-value offsets parallel to `owner_record_indices`.
    pub(crate) value_offsets: [u64; 4],
    /// Exact serialized instance sequence when one pattern direction is active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) instances: Option<DesignRectangularPatternInstances>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct DesignRectangularPatternConstructionWire {
    /// Positive U-direction instance count, including the seed.
    pub(crate) u_count: u32,
    /// Positive V-direction instance count, including the seed.
    pub(crate) v_count: u32,
    /// Signed U-direction seed-to-final-instance span in source centimetres.
    pub(crate) u_extent: f64,
    /// Signed V-direction seed-to-final-instance span in source centimetres.
    pub(crate) v_extent: f64,
    /// Parameter-owner records for U count, V count, U extent, and V extent.
    pub(crate) owner_record_indices: [u32; 4],
    /// Evaluated-value offsets parallel to `owner_record_indices`.
    pub(crate) value_offsets: [u64; 4],
    /// Exact serialized instance sequence when one pattern direction is active.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_instances"
    )]
    pub(crate) instances: Option<DesignRectangularPatternInstances>,
}

impl TryFrom<DesignRectangularPatternConstructionWire> for DesignRectangularPatternConstruction {
    type Error = &'static str;
    fn try_from(wire: DesignRectangularPatternConstructionWire) -> Result<Self, Self::Error> {
        let u_count = NonZeroU32::new(wire.u_count).ok_or("u_count must be nonzero")?;
        let v_count = NonZeroU32::new(wire.v_count).ok_or("v_count must be nonzero")?;
        if u_count.get() == 1 && v_count.get() == 1 {
            return Err("u_count and v_count must not both be one");
        }
        if !wire.u_extent.is_finite() || (u_count.get() == 1) != (wire.u_extent == 0.0) {
            return Err("u_extent must be finite and zero exactly when u_count is one");
        }
        if !wire.v_extent.is_finite() || (v_count.get() == 1) != (wire.v_extent == 0.0) {
            return Err("v_extent must be finite and zero exactly when v_count is one");
        }
        Ok(Self {
            u_count,
            v_count,
            u_extent: wire.u_extent,
            v_extent: wire.v_extent,
            owner_record_indices: wire.owner_record_indices,
            value_offsets: wire.value_offsets,
            instances: wire.instances,
        })
    }
}

impl From<DesignRectangularPatternConstruction> for DesignRectangularPatternConstructionWire {
    fn from(value: DesignRectangularPatternConstruction) -> Self {
        Self {
            u_count: value.u_count.get(),
            v_count: value.v_count.get(),
            u_extent: value.u_extent,
            v_extent: value.v_extent,
            owner_record_indices: value.owner_record_indices,
            value_offsets: value.value_offsets,
            instances: value.instances,
        }
    }
}

impl DesignRectangularPatternConstruction {
    pub(crate) fn u_count(&self) -> u32 {
        self.u_count.get()
    }
    pub(crate) fn v_count(&self) -> u32 {
        self.v_count.get()
    }
    pub(crate) fn u_extent(&self) -> f64 {
        self.u_extent
    }
    pub(crate) fn v_extent(&self) -> f64 {
        self.v_extent
    }
}

/// Serialized placements of one linearized rectangular-pattern instance run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignRectangularPatternInstancesWire",
    into = "DesignRectangularPatternInstancesWire"
)]
pub(crate) enum DesignRectangularPatternInstances {
    Bodies(Vec<DesignPatternInstance>),
    Components {
        component_guid: DesignRelaxedGuidText,
        seed: DesignPatternComponentInstance,
        generated: Vec<DesignPatternComponentInstance>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DesignPatternInstance {
    pub(crate) record_index: u32,
    pub(crate) transform: Located<SketchPlacementMatrix>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignPatternComponentInstance {
    pub(crate) instance: DesignPatternInstance,
    pub(crate) occurrence_guid: DesignRelaxedGuidText,
}

impl DesignRectangularPatternInstances {
    pub(crate) fn instance_count(&self) -> usize {
        match self {
            Self::Bodies(instances) => instances.len(),
            Self::Components { generated, .. } => generated.len() + 1,
        }
    }

    pub(crate) fn frames(&self) -> impl DoubleEndedIterator<Item = &DesignPatternInstance> {
        let (bodies, seed, generated): (
            &[DesignPatternInstance],
            Option<&DesignPatternInstance>,
            &[DesignPatternComponentInstance],
        ) = match self {
            Self::Bodies(instances) => (instances, None, &[]),
            Self::Components {
                seed, generated, ..
            } => (&[], Some(&seed.instance), generated),
        };
        bodies
            .iter()
            .chain(seed)
            .chain(generated.iter().map(|row| &row.instance))
    }
}

/// Serialized placements of one linearized rectangular-pattern instance run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct DesignRectangularPatternInstancesWire {
    /// Seed record followed by the generated-instance records in pattern order.
    record_indices: Vec<u32>,
    /// Row-major local-to-model placements parallel to `record_indices`.
    transforms: Vec<SketchPlacementMatrix>,
    /// Byte offsets of the first transform scalar parallel to `record_indices`.
    transform_offsets: Vec<u64>,
    /// Component occurrences carried by this run when the pattern repeats a component.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_component_occurrences"
    )]
    component_occurrences: Option<DesignComponentPatternOccurrencesWire>,
}

/// Component seed and generated occurrences carried by a rectangular pattern.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignComponentPatternOccurrencesWire {
    /// Reusable local component definition shared by every occurrence.
    component_guid: DesignRelaxedGuidText,
    /// Existing seed occurrence.
    seed_occurrence_guid: DesignRelaxedGuidText,
    /// Newly generated occurrences in pattern order after the seed.
    generated_occurrence_guids: Vec<DesignRelaxedGuidText>,
}

impl TryFrom<DesignRectangularPatternInstancesWire> for DesignRectangularPatternInstances {
    type Error = String;
    fn try_from(wire: DesignRectangularPatternInstancesWire) -> Result<Self, Self::Error> {
        if wire.record_indices.len() != wire.transforms.len()
            || wire.record_indices.len() != wire.transform_offsets.len()
        {
            return Err(
                "record_indices, transforms, and transform_offsets must have equal lengths".into(),
            );
        }
        let frames = wire
            .record_indices
            .into_iter()
            .zip(wire.transforms)
            .zip(wire.transform_offsets)
            .map(|((record_index, value), offset)| DesignPatternInstance {
                record_index,
                transform: Located { value, offset },
            });
        let Some(component) = wire.component_occurrences else {
            return Ok(Self::Bodies(frames.collect()));
        };
        let mut frames = frames;
        let seed = frames
            .next()
            .ok_or("component_occurrences requires a seed frame")?;
        if frames.len() != component.generated_occurrence_guids.len() {
            return Err(
                "generated_occurrence_guids must match the generated instance frames".into(),
            );
        }
        Ok(Self::Components {
            component_guid: component.component_guid,
            seed: DesignPatternComponentInstance {
                instance: seed,
                occurrence_guid: component.seed_occurrence_guid,
            },
            generated: frames
                .zip(component.generated_occurrence_guids)
                .map(
                    |(instance, occurrence_guid)| DesignPatternComponentInstance {
                        instance,
                        occurrence_guid,
                    },
                )
                .collect(),
        })
    }
}

impl From<DesignRectangularPatternInstances> for DesignRectangularPatternInstancesWire {
    fn from(instances: DesignRectangularPatternInstances) -> Self {
        let record_indices = instances.frames().map(|row| row.record_index).collect();
        let transforms = instances.frames().map(|row| row.transform.value).collect();
        let transform_offsets = instances.frames().map(|row| row.transform.offset).collect();
        let component_occurrences = match instances {
            DesignRectangularPatternInstances::Bodies(_) => None,
            DesignRectangularPatternInstances::Components {
                component_guid,
                seed,
                generated,
            } => Some(DesignComponentPatternOccurrencesWire {
                component_guid,
                seed_occurrence_guid: seed.occurrence_guid,
                generated_occurrence_guids: generated
                    .into_iter()
                    .map(|row| row.occurrence_guid)
                    .collect(),
            }),
        };
        Self {
            record_indices,
            transforms,
            transform_offsets,
            component_occurrences,
        }
    }
}
