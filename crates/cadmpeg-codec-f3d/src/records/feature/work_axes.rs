use crate::bytes::reference_fields::{ReadField, ReferenceFields};
use cadmpeg_ir::scalar::FiniteReal;
use serde::{Deserialize, Serialize};

pub(crate) type AxisReference = ReferenceFields<String, String>;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignWorkAxisConstruction {
    pub(crate) scope_references: Vec<AxisReference>,
    pub(crate) candidates: Vec<DesignWorkAxisCandidate>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AxisCarrierPrefix {
    pub(crate) header_word: ReadField<u32>,
    pub(crate) name: ReadField<String>,
    pub(crate) leading_block: ReadField<u8>,
    pub(crate) property_presence: ReadField<u8>,
    pub(crate) property_count: ReadField<u32>,
    pub(crate) properties: Vec<AxisCarrierProperty>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AxisCarrierProperty {
    pub(crate) key: ReadField<String>,
    pub(crate) intrinsic_type: ReadField<String>,
    pub(crate) value: ReadField<AxisCarrierPropertyValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) enum AxisCarrierPropertyValue {
    Unsigned(u64),
    Real(FiniteReal),
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(try_from = "AxisReferenceRunWire")]
pub(crate) struct AxisReferenceRun {
    pub(crate) declared: ReadField<u32>,
    pub(crate) references: Vec<AxisReference>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AxisReferenceRunWire {
    declared: ReadField<u32>,
    references: Vec<AxisReference>,
}
impl TryFrom<AxisReferenceRunWire> for AxisReferenceRun {
    type Error = &'static str;
    fn try_from(value: AxisReferenceRunWire) -> Result<Self, Self::Error> {
        let count = value.references.len();
        let declared = value
            .declared
            .value()
            .and_then(|v| usize::try_from(*v).ok());
        if declared.is_none() && count != 0
            || declared.is_some_and(|n| count > n)
            || value.references.iter().rev().skip(1).any(|v| !v.complete)
        {
            return Err(
                "WorkAxis reference prefix contradicts its declared count or occurrence order",
            );
        }
        Ok(Self {
            declared: value.declared,
            references: value.references,
        })
    }
}

impl AxisReferenceRun {
    pub(crate) fn complete(&self) -> bool {
        self.declared.value().is_some_and(|declared| {
            *declared > 0 && usize::try_from(*declared).ok() == Some(self.references.len())
        }) && self.references.iter().all(|reference| reference.complete)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignWorkAxisCandidate {
    pub(crate) record_index: Option<u32>,
    pub(crate) version: ReadField<u32>,
    pub(crate) prefix: AxisCarrierPrefix,
    pub(crate) origin: [ReadField<FiniteReal>; 3],
    pub(crate) displacement: [ReadField<FiniteReal>; 3],
    pub(crate) source: Option<DesignWorkAxisSource>,
    pub(crate) endpoints: Vec<AxisEndpoint>,
    pub(crate) complete: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) enum DesignWorkAxisSource {
    TwoPoint {
        value_count: ReadField<u32>,
        reserved: [ReadField<FiniteReal>; 2],
        inputs: AxisReferenceRun,
    },
    DirectCarrier {
        value_count: ReadField<u32>,
        reserved: [ReadField<FiniteReal>; 2],
        reference_count: ReadField<u32>,
        reference_preamble: ReadField<u32>,
        support_record_index: u32,
    },
    PointDirection {
        bounding_box_index: ReadField<u32>,
        point_parameters: [ReadField<FiniteReal>; 2],
        reference_type: ReadField<u32>,
        tangent_prefix: ReadField<u8>,
        tangent: [ReadField<FiniteReal>; 3],
        inputs: AxisReferenceRun,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AxisEndpoint {
    pub(crate) unique_frame: bool,
    pub(crate) record_index: u32,
    pub(crate) readings: Vec<AxisEndpointReading>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AxisEndpointReading {
    pub(crate) version: ReadField<u32>,
    pub(crate) prefix: AxisCarrierPrefix,
    pub(crate) bounding_box_index: ReadField<u32>,
    pub(crate) parameters: [ReadField<FiniteReal>; 2],
    pub(crate) pick_point_parameter: ReadField<AxisReference>,
    pub(crate) position: [ReadField<FiniteReal>; 3],
    pub(crate) reference_type: ReadField<u32>,
    pub(crate) tangent: [ReadField<FiniteReal>; 3],
    pub(crate) inputs: AxisReferenceRun,
    pub(crate) complete: bool,
}

rewrite_native_scalar!(DesignWorkAxisConstruction);
