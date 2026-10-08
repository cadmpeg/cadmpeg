//! Consolidated record framing and edge-resolution vocabulary.
//!
//! Inventories length-closed A/B-family records, groups consolidated edge runs
//! and their native incidence graph, and resolves edge-block side carriers
//! against typed analytic and NURBS charts.

use crate::math::distance;
use cadmpeg_core::decode::{DecodeContext, ScopedReservation, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::eval::nurbs_surface_partials;
use cadmpeg_ir::features::FinitePoint3;
#[cfg(test)]
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::FiniteReal;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::num::NonZeroUsize;
use std::ops::Range;

use crate::families::a5a8::records::{a5_surfaces_from_records, FreeformSurface};
use crate::families::b2::records::{
    b2_adjacent_face_counted_owners_from_records, b2_circle_from_record,
    b2_class25_descriptors_from_records, b2_closed_owner_boundary_edges, b2_cone_point,
    b2_cones_from_records, b2_cylinder_point, b2_cylinders_from_records,
    b2_edge_nodes_from_records, b2_edge_parameters_from_records,
    b2_embedded_cylinders_from_records, b2_face_nodes_5f_from_records,
    b2_owner_identity_targets_from_records, b2_owner_packets_from_records,
    b2_plane_carriers_from_records, b2_plane_geometry, b2_sphere_geometry, b2_spheres_from_records,
    b2_tori_from_records, b2_torus_geometry, b2_use_metadata_from_records, B2Circle,
    B2Class25Descriptor, B2Cone, B2Cylinder, B2EdgeNode, B2EdgeParameters, B2EmbeddedCylinder,
    B2FaceNode5f, B2PlaneCarrier, B2Sphere, B2Torus, B2UseMetadata,
};
use crate::wire::bytes::{
    allocation_ref, compact_int, finite_f64_lane, persistent_ref, read_f64_array,
    AllocationReferenceEncoding,
};
#[cfg(test)]
use crate::wire::records::consolidated_records;
use crate::wire::records::{
    family_pcurves_from_records, records_are_contiguous, scan_vertex_record_ranges,
    ConsolidatedFamily, ConsolidatedPcurve, ConsolidatedRawFrame, ConsolidatedRecord,
};

const EPS_TRANSVERSE_RESIDUAL: f64 = 1.0e-6;
const EPS_SAMPLE_AGREEMENT: f64 = 1.0e-6;
const EPS_ENDPOINT_RANGE: f64 = 1.0e-6;
const EPS_CIRCLE_ENDPOINT: f64 = 1.0e-9;

/// Serialized consolidated edge block formed by two pcurves and one range packet.
#[derive(Debug, Clone)]
pub(crate) struct ConsolidatedEdgeBlock {
    /// The two face-side UV definitions in serialization order.
    pub(crate) pcurves: [ConsolidatedPcurve; 2],
    /// Shared parameter range and tolerance packet.
    pub(crate) parameters: B2EdgeParameters,
}

/// Complete consolidated edge run serialized as two side pcurves, their shared
/// parameter packet, two oriented uses, and one native edge node.
#[derive(Debug, Clone)]
pub(crate) struct ConsolidatedTopologyEdgeRun {
    /// Co-parametric side definitions and shared range packet.
    pub(crate) edge: ConsolidatedEdgeBlock,
    /// Native edge node carrying curve, endpoint, and endpoint-parameter identities.
    pub(crate) node: B2EdgeNode,
}

/// Complete analytic-circle edge run serialized as a class-`0x18` descriptor,
/// circle carrier, scalar definition, two oriented uses, and one edge node.
#[derive(Debug, Clone)]
pub(crate) struct ConsolidatedAnalyticCircleEdgeRun {
    /// Class-`0x18` descriptor immediately preceding the circle carrier.
    pub(crate) descriptor: ConsolidatedRawFrame,
    /// Arc-length circle carrier.
    pub(crate) circle: B2Circle,
    /// Eight-scalar class-`0x23` edge definition.
    #[cfg(test)]
    pub(super) definition: ConsolidatedEdgeDefinition,
    /// Native edge node carrying curve, endpoint, and endpoint-parameter identities.
    pub(crate) node: B2EdgeNode,
}

/// Complete class-`0x25` edge run with its adjacent class-`0x18` descriptor.
#[derive(Debug, Clone)]
pub(crate) struct ConsolidatedClass25EdgeRun {
    /// Typed class-`0x18` descriptor.
    pub(crate) descriptor: B2Class25Descriptor,
    /// Native edge node carrying curve, endpoint, and endpoint-parameter identities.
    pub(crate) node: B2EdgeNode,
}

/// Two adjacent oriented uses and their terminal native edge node.
#[derive(Debug, Clone)]
pub(crate) struct ConsolidatedEdgeUseRun {
    /// Immediately preceding edge-definition frame in classes `0x23..=0x25`.
    pub(crate) definition: Option<ConsolidatedEdgeDefinition>,
    /// The two serialized edge uses, in side order.
    pub(crate) uses: [B2UseMetadata; 2],
    /// Native edge node carrying curve, endpoint, and endpoint-parameter identities.
    pub(crate) node: B2EdgeNode,
}

/// Compact edge node selected by its zero-based ordinal in one face-owner allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ConsolidatedOwnedEdgeNode {
    /// Byte offset of the owning class-`0x62` packet.
    pub(crate) owner_pos: usize,
    /// Zero-based frame ordinal after the owner packet.
    pub(crate) allocation_ordinal: u32,
    /// Selected compact edge node.
    pub(crate) node: B2EdgeNode,
}

/// Compact edge endpoints resolved through structural allocation references.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ConsolidatedCompactEdgeEndpoints {
    /// Edge node whose endpoint references closed under the local allocation grammar.
    pub(crate) node: B2EdgeNode,
    /// Byte offsets of the resolved endpoint records, in edge order.
    pub(crate) endpoint_records: [usize; 2],
}

/// One fixed-nine owner boundary closed by four resolved class-`0x5e` edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ConsolidatedOwnerBoundaryCycle {
    /// Bounded record source containing the owner and its local targets.
    pub(crate) source_index: usize,
    /// Class-`0x62` owner-record offset.
    pub(crate) owner_pos: usize,
    /// Source-scoped class-`0x5f` face node associated with this boundary
    /// allocation, when the cycle prelude closes its checked identity.
    pub(crate) face_node: Option<B2FaceNode5f>,
    /// Four edge targets in fixed-nine slot order.
    pub(crate) edges: [crate::families::b2::records::B2OwnerBoundaryEdge; 4],
}

/// Class of a consolidated edge-definition frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub(crate) enum ConsolidatedEdgeDefinitionClass {
    Class23,
    Class24,
    Class25,
}

impl From<ConsolidatedEdgeDefinitionClass> for u8 {
    fn from(class: ConsolidatedEdgeDefinitionClass) -> Self {
        match class {
            ConsolidatedEdgeDefinitionClass::Class23 => 0x23,
            ConsolidatedEdgeDefinitionClass::Class24 => 0x24,
            ConsolidatedEdgeDefinitionClass::Class25 => 0x25,
        }
    }
}

impl TryFrom<u8> for ConsolidatedEdgeDefinitionClass {
    type Error = String;
    fn try_from(class: u8) -> Result<Self, Self::Error> {
        match class {
            0x23 => Ok(Self::Class23),
            0x24 => Ok(Self::Class24),
            0x25 => Ok(Self::Class25),
            _ => Err(format!(
                "edge-definition class {class:#x} is not 0x23, 0x24, or 0x25"
            )),
        }
    }
}

/// Framed edge definition structurally owned by an adjacent oriented-use run.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ConsolidatedEdgeDefinition {
    /// Framed record.
    pub(crate) frame: ConsolidatedRawFrame,
    /// Edge-definition class in `0x23..=0x25`.
    pub(crate) class: ConsolidatedEdgeDefinitionClass,
}

#[cfg(test)]
impl ConsolidatedEdgeDefinition {
    fn data(&self) -> Option<ConsolidatedEdgeDefinitionData> {
        consolidated_edge_definition_data(self.class.into(), &self.frame.payload)
    }
}

/// Persistent operand encoding in a class-25 definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Option<u8>", into = "Option<u8>")]
pub(crate) enum Class25PersistentLead {
    Compact,
    Lead0a,
    Lead0b,
}
impl TryFrom<Option<u8>> for Class25PersistentLead {
    type Error = String;
    fn try_from(value: Option<u8>) -> Result<Self, Self::Error> {
        match value {
            None => Ok(Self::Compact),
            Some(0x0a) => Ok(Self::Lead0a),
            Some(0x0b) => Ok(Self::Lead0b),
            _ => Err("persistent_lead must be null, 10, or 11".into()),
        }
    }
}
impl From<Class25PersistentLead> for Option<u8> {
    fn from(value: Class25PersistentLead) -> Self {
        match value {
            Class25PersistentLead::Compact => None,
            Class25PersistentLead::Lead0a => Some(0x0a),
            Class25PersistentLead::Lead0b => Some(0x0b),
        }
    }
}
/// Boundary marker in a class-25 scalar lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
enum Class25ScalarMarker {
    M82,
    M83,
    M89,
    M8b,
}
impl TryFrom<u8> for Class25ScalarMarker {
    type Error = String;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x82 => Ok(Self::M82),
            0x83 => Ok(Self::M83),
            0x89 => Ok(Self::M89),
            0x8b => Ok(Self::M8b),
            _ => Err(format!("marker {value:#x} is not a class-25 scalar marker")),
        }
    }
}
impl From<Class25ScalarMarker> for u8 {
    fn from(value: Class25ScalarMarker) -> Self {
        match value {
            Class25ScalarMarker::M82 => 0x82,
            Class25ScalarMarker::M83 => 0x83,
            Class25ScalarMarker::M89 => 0x89,
            Class25ScalarMarker::M8b => 0x8b,
        }
    }
}
/// Scalar tail with the arity selected by its marker.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "Class25ScalarSegmentWire")]
pub(crate) enum Class25ScalarSegment {
    M82Five(Box<[FiniteReal; 5]>),
    M82Six(Box<[FiniteReal; 6]>),
    M82Seven(Box<[FiniteReal; 7]>),
    M83Eight(Box<[FiniteReal; 8]>),
    M83Nine(Box<[FiniteReal; 9]>),
    M89(Box<[FiniteReal; 20]>),
    M8b(Box<[FiniteReal; 24]>),
}
#[derive(Serialize, Deserialize)]
struct Class25ScalarSegmentWire {
    marker: Class25ScalarMarker,
    trailing: Vec<FiniteReal>,
}
#[derive(Serialize)]
struct Class25ScalarSegmentWireRef<'a> {
    marker: Class25ScalarMarker,
    trailing: &'a [FiniteReal],
}

impl Serialize for Class25ScalarSegment {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let (marker, trailing): (_, &[FiniteReal]) = match self {
            Self::M82Five(lane) => (Class25ScalarMarker::M82, &lane[..]),
            Self::M82Six(lane) => (Class25ScalarMarker::M82, &lane[..]),
            Self::M82Seven(lane) => (Class25ScalarMarker::M82, &lane[..]),
            Self::M83Eight(lane) => (Class25ScalarMarker::M83, &lane[..]),
            Self::M83Nine(lane) => (Class25ScalarMarker::M83, &lane[..]),
            Self::M89(lane) => (Class25ScalarMarker::M89, &lane[..]),
            Self::M8b(lane) => (Class25ScalarMarker::M8b, &lane[..]),
        };
        Class25ScalarSegmentWireRef { marker, trailing }.serialize(serializer)
    }
}
impl TryFrom<Class25ScalarSegmentWire> for Class25ScalarSegment {
    type Error = String;
    fn try_from(wire: Class25ScalarSegmentWire) -> Result<Self, Self::Error> {
        fn with_arity<const N: usize>(
            lane: Vec<FiniteReal>,
            constructor: fn(Box<[FiniteReal; N]>) -> Class25ScalarSegment,
        ) -> Option<Class25ScalarSegment> {
            Some(constructor(lane.into_boxed_slice().try_into().ok()?))
        }

        match (wire.marker, wire.trailing.len()) {
            (Class25ScalarMarker::M82, 5) => with_arity(wire.trailing, Self::M82Five),
            (Class25ScalarMarker::M82, 6) => with_arity(wire.trailing, Self::M82Six),
            (Class25ScalarMarker::M82, 7) => with_arity(wire.trailing, Self::M82Seven),
            (Class25ScalarMarker::M83, 8) => with_arity(wire.trailing, Self::M83Eight),
            (Class25ScalarMarker::M83, 9) => with_arity(wire.trailing, Self::M83Nine),
            (Class25ScalarMarker::M89, 20) => with_arity(wire.trailing, Self::M89),
            (Class25ScalarMarker::M8b, 24) => with_arity(wire.trailing, Self::M8b),
            _ => None,
        }
        .ok_or_else(|| "trailing arity does not match marker".to_owned())
    }
}
#[cfg(test)]
impl From<Class25ScalarSegment> for Class25ScalarSegmentWire {
    fn from(value: Class25ScalarSegment) -> Self {
        fn into_vec<const N: usize>(lane: Box<[FiniteReal; N]>) -> Vec<FiniteReal> {
            let lane: Box<[FiniteReal]> = lane;
            lane.into_vec()
        }
        let (marker, trailing) = match value {
            Class25ScalarSegment::M82Five(lane) => (Class25ScalarMarker::M82, into_vec(lane)),
            Class25ScalarSegment::M82Six(lane) => (Class25ScalarMarker::M82, into_vec(lane)),
            Class25ScalarSegment::M82Seven(lane) => (Class25ScalarMarker::M82, into_vec(lane)),
            Class25ScalarSegment::M83Eight(lane) => (Class25ScalarMarker::M83, into_vec(lane)),
            Class25ScalarSegment::M83Nine(lane) => (Class25ScalarMarker::M83, into_vec(lane)),
            Class25ScalarSegment::M89(lane) => (Class25ScalarMarker::M89, into_vec(lane)),
            Class25ScalarSegment::M8b(lane) => (Class25ScalarMarker::M8b, into_vec(lane)),
        };
        Self { marker, trailing }
    }
}
/// A finite scalar lane with an admitted serialized arity.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(transparent)]
pub(crate) struct ScalarLane<const LOWER: usize, const UPPER: usize>(Vec<FiniteReal>);

impl<const LOWER: usize, const UPPER: usize> TryFrom<Vec<FiniteReal>> for ScalarLane<LOWER, UPPER> {
    type Error = &'static str;
    fn try_from(values: Vec<FiniteReal>) -> Result<Self, Self::Error> {
        if !(LOWER..=UPPER).contains(&values.len()) {
            return Err("edge-definition scalar arity is outside its grammar");
        }
        Ok(Self(values))
    }
}

impl<const LOWER: usize, const UPPER: usize> std::ops::Deref for ScalarLane<LOWER, UPPER> {
    type Target = [FiniteReal];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'de, const LOWER: usize, const UPPER: usize> Deserialize<'de> for ScalarLane<LOWER, UPPER> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(Vec::<FiniteReal>::deserialize(deserializer)?)
            .map_err(serde::de::Error::custom)
    }
}

/// Closed payload grammar of a consolidated edge-definition frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub(crate) enum ConsolidatedEdgeDefinitionData {
    /// Compact class-`0x24` payload `81 <operand> 0f 87`.
    Compact24 {
        /// Width-coded operand.
        operand: u32,
    },
    /// Three operand references followed by eight or nine scalar lanes.
    Scalar {
        /// Two compact operands followed by one persistent operand.
        operands: [u32; 3],
        /// Complete finite scalar lane.
        values: ScalarLane<8, 9>,
    },
    /// Class-`0x25` three-operand form with one uninterrupted scalar lane.
    Scalar25 {
        /// Two mixed-width allocation operands followed by one persistent operand.
        operands: [u32; 3],
        /// Explicit third-operand lead (`0x0a` or `0x0b`), or `None` for compact encoding.
        persistent_lead: Class25PersistentLead,
        /// Complete finite scalar lane.
        values: ScalarLane<7, 10>,
    },
    /// Class-`0x25` three-operand form with a tagged scalar-lane boundary.
    SegmentedScalar25 {
        /// Two mixed-width allocation operands followed by one persistent operand.
        operands: [u32; 3],
        /// Explicit third-operand lead (`0x0a` or `0x0b`), or `None` for compact encoding.
        persistent_lead: Class25PersistentLead,
        /// Five finite scalars preceding the segment marker.
        leading: [FiniteReal; 5],
        /// Marker and its scalar tail.
        #[serde(flatten)]
        segment: Class25ScalarSegment,
    },
}

/// Decode a complete class-specific edge-definition payload without inferring
/// geometric meanings for its operand or scalar lanes.
#[must_use]
pub(crate) fn consolidated_edge_definition_data(
    class: u8,
    payload: &[u8],
) -> Option<ConsolidatedEdgeDefinitionData> {
    match edge_definition_data_with(class, payload, |bytes| {
        Ok::<_, std::convert::Infallible>(finite_f64_lane(bytes))
    }) {
        Ok(data) => data,
        Err(never) => match never {},
    }
}

pub(crate) fn consolidated_edge_definition_data_charged(
    ctx: &DecodeContext<'_>,
    class: u8,
    payload: &[u8],
) -> Result<Option<ConsolidatedEdgeDefinitionData>, CodecError> {
    edge_definition_data_with(class, payload, |bytes| {
        crate::wire::bytes::finite_f64_lane_charged(
            ctx,
            bytes,
            "catia_native_edge_definition_scalars",
        )
    })
}

fn edge_definition_data_with<E>(
    class: u8,
    payload: &[u8],
    mut lane: impl FnMut(&[u8]) -> Result<Option<Vec<FiniteReal>>, E>,
) -> Result<Option<ConsolidatedEdgeDefinitionData>, E> {
    (|| {
        if class == 0x24 && payload.first() == Some(&0x81) {
            let mut at = 1;
            let operand = compact_int(payload, &mut at)?;
            return (payload.get(at..) == Some(&[0x0f, 0x87][..]))
                .then_some(Ok(ConsolidatedEdgeDefinitionData::Compact24 { operand }));
        }
        if class == 0x25 && payload.first() == Some(&0x82) {
            let mut at = 1;
            let first = allocation_ref(payload, &mut at)?;
            let second = allocation_ref(payload, &mut at)?;
            let (third, persistent_lead) = class25_persistent_ref(payload, &mut at)?;
            let operands = [first, second, third];
            let scalar_bytes = payload.get(at..)?;
            if matches!(scalar_bytes.len(), 56 | 64 | 72 | 80) {
                let values = match lane(scalar_bytes) {
                    Ok(Some(values)) => values,
                    Ok(None) => return None,
                    Err(error) => return Some(Err(error)),
                };
                return Some(Ok(ConsolidatedEdgeDefinitionData::Scalar25 {
                    operands,
                    persistent_lead,
                    values: values.try_into().ok()?,
                }));
            }
            let leading = read_f64_array::<5>(scalar_bytes, 0)?;
            let marker = *scalar_bytes.get(40)?;
            let tail = scalar_bytes.get(41..)?;
            let marker = Class25ScalarMarker::try_from(marker).ok()?;
            if !matches!(
                (marker, tail.len()),
                (Class25ScalarMarker::M82, 40 | 48 | 56)
                    | (Class25ScalarMarker::M83, 64 | 72)
                    | (Class25ScalarMarker::M89, 160)
                    | (Class25ScalarMarker::M8b, 192)
            ) {
                return None;
            }
            let trailing = match lane(tail) {
                Ok(Some(values)) => values,
                Ok(None) => return None,
                Err(error) => return Some(Err(error)),
            };
            let segment =
                Class25ScalarSegment::try_from(Class25ScalarSegmentWire { marker, trailing })
                    .ok()?;
            return Some(Ok(ConsolidatedEdgeDefinitionData::SegmentedScalar25 {
                operands,
                persistent_lead,
                leading,
                segment,
            }));
        }
        if !matches!(class, 0x23 | 0x24) || payload.first() != Some(&0x82) {
            return None;
        }
        let mut at = 1;
        let operands = [
            compact_int(payload, &mut at)?,
            compact_int(payload, &mut at)?,
            persistent_ref(payload, &mut at)?,
        ];
        let scalar_bytes = payload.get(at..)?;
        if !matches!((class, scalar_bytes.len()), (0x23, 64 | 72) | (0x24, 64)) {
            return None;
        }
        let values = match lane(scalar_bytes) {
            Ok(Some(values)) => values,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        if values[2] != *values.last()? {
            return None;
        }
        if values.len() == 9
            && !(values[0] == values[3]
                && values[0] == values[6]
                && values[1] == values[4]
                && values[1] == values[7]
                && values[2] == values[5]
                && values[5].get() == 1.0)
        {
            return None;
        }
        Some(Ok(ConsolidatedEdgeDefinitionData::Scalar {
            operands,
            values: values.try_into().ok()?,
        }))
    })()
    .transpose()
}

fn edge_definition_is_scalar_eight(definition: &ConsolidatedEdgeDefinition) -> bool {
    if !matches!(
        definition.class,
        ConsolidatedEdgeDefinitionClass::Class23 | ConsolidatedEdgeDefinitionClass::Class24
    ) || definition.frame.payload.first() != Some(&0x82)
    {
        return false;
    }
    let payload = &definition.frame.payload;
    let mut at = 1;
    let parsed = (|| {
        compact_int(payload, &mut at)?;
        compact_int(payload, &mut at)?;
        persistent_ref(payload, &mut at)?;
        let scalar_bytes = payload.get(at..)?;
        if scalar_bytes.len() != 64 {
            return None;
        }
        let values = read_f64_array::<8>(scalar_bytes, 0)?;
        Some(values[2] == values[7])
    })();
    parsed == Some(true)
}

fn edge_definition_is_typed_class25(definition: &ConsolidatedEdgeDefinition) -> bool {
    fn finite_lane(bytes: &[u8]) -> bool {
        if !bytes.len().is_multiple_of(8) {
            return false;
        }
        let mut view = View::over_retained(bytes);
        while !view.is_empty() {
            if view.f64_le().and_then(FiniteReal::new).is_none() {
                return false;
            }
        }
        true
    }
    if definition.class != ConsolidatedEdgeDefinitionClass::Class25
        || definition.frame.payload.first() != Some(&0x82)
    {
        return false;
    }
    let payload = &definition.frame.payload;
    let mut at = 1;
    let parsed = (|| {
        allocation_ref(payload, &mut at)?;
        allocation_ref(payload, &mut at)?;
        class25_persistent_ref(payload, &mut at)?;
        let scalar_bytes = payload.get(at..)?;
        if matches!(scalar_bytes.len(), 56 | 64 | 72 | 80) {
            return Some(finite_lane(scalar_bytes));
        }
        read_f64_array::<5>(scalar_bytes, 0)?;
        let marker = *scalar_bytes.get(40)?;
        let trailing = scalar_bytes.get(41..)?;
        let count = trailing.len() / 8;
        let valid_arity = match marker {
            0x82 => matches!(count, 5..=7),
            0x83 => matches!(count, 8 | 9),
            0x89 => count == 20,
            0x8b => count == 24,
            _ => false,
        };
        Some(valid_arity && finite_lane(trailing))
    })();
    parsed == Some(true)
}

fn class25_persistent_ref(bytes: &[u8], at: &mut usize) -> Option<(u32, Class25PersistentLead)> {
    match *bytes.get(*at)? {
        lead @ (0x0a | 0x0b) => {
            let value = u32::from(View::u16_le_at(bytes, *at + 1)?);
            *at += 3;
            Some((value, Class25PersistentLead::try_from(Some(lead)).ok()?))
        }
        _ => Some((compact_int(bytes, at)?, Class25PersistentLead::Compact)),
    }
}

/// Native endpoint-incidence graph of complete consolidated edge runs.
#[derive(Debug, Clone)]
#[cfg(test)]
struct ConsolidatedNativeEdgeGraph {
    /// Persistent native vertex identities in first-incidence order.
    pub(super) vertex_identities: Vec<u32>,
    /// Edge runs in serialization order, with endpoints indexing
    /// `vertex_identities`.
    pub(super) edges: Vec<ConsolidatedNativeGraphEdge>,
    /// Connected edge components, expressed as edge ordinals.
    pub(super) components: Vec<Vec<usize>>,
}

/// One edge in a consolidated native endpoint-incidence graph.
#[derive(Debug, Clone)]
#[cfg(test)]
struct ConsolidatedNativeGraphEdge {
    /// Compact endpoint indices into [`ConsolidatedNativeEdgeGraph::vertex_identities`].
    pub(super) vertices: [usize; 2],
}

/// Uniquely resolved carrier for one side of a consolidated edge block.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ConsolidatedSupportBinding {
    /// Standalone `b2 03 28` cylinder record.
    Cylinder {
        /// Carrier record byte offset.
        pos: usize,
    },
    /// Cylinder frame embedded in a `b2 03 60` wrapper.
    EmbeddedCylinder {
        /// Embedded frame byte offset.
        pos: usize,
        /// Enclosing wrapper byte offset.
        wrapper_pos: usize,
    },
    /// `b2 03 19` circle selected by constant-V and exact arc range.
    Circle {
        /// Carrier record byte offset.
        pos: usize,
    },
    /// `b2 03 29` cone selected by endpoint lifts.
    Cone {
        /// Carrier record byte offset.
        pos: usize,
    },
    /// `b2 03 2a` sphere selected by endpoint lifts.
    Sphere {
        /// Carrier record byte offset.
        pos: usize,
    },
    /// `b2 03 2b` torus selected by endpoint lifts through its scaled chart.
    Torus {
        /// Carrier record byte offset.
        pos: usize,
    },
    /// Direction-bearing `b2/b3/b4 03 27` plane carrier selected by endpoint lifts.
    Plane {
        /// Carrier record byte offset.
        pos: usize,
    },
    /// Consolidated `a5 03 34` NURBS carrier, optionally at a constant normal offset.
    NurbsCarrier {
        /// Carrier record byte offset.
        pos: usize,
        /// Signed normal offset from the stored carrier to the shared 3D edge.
        offset: FiniteReal,
    },
}

/// Consolidated edge block with uniquely resolved side carriers.
#[derive(Debug, Clone)]
pub(crate) struct ResolvedConsolidatedEdgeBlock {
    /// Parsed pcurve pair and shared edge packet.
    pub(crate) block: ConsolidatedEdgeBlock,
    /// Carrier binding for each pcurve side.
    pub(crate) supports: [Option<ConsolidatedSupportBinding>; 2],
    /// Shared lifted 3D definition sites when every liftable side agrees
    /// pointwise in the common edge parameterization.
    pub(crate) shared_loci: Option<Vec<Point3>>,
    /// Unordered 3D endpoint loci when at least one uniquely bound side can be
    /// lifted and every liftable side agrees.
    pub(crate) endpoint_loci: Option<[Point3; 2]>,
}

struct ConsolidatedCarriers<'a> {
    cylinders: &'a [B2Cylinder],
    embedded_cylinders: &'a [B2EmbeddedCylinder],
    circles: &'a [B2Circle],
    cones: &'a [B2Cone],
    spheres: &'a [B2Sphere],
    tori: &'a [B2Torus],
    planes: &'a [B2PlaneCarrier],
    nurbs_surfaces: &'a [FreeformSurface],
}

/// Group ordered pairs of same-family class-`0x20` pcurves followed by one
/// B-family class-`0x23` range packet.
#[must_use]
#[cfg(test)]
fn consolidated_edge_blocks(data: &[u8]) -> Vec<ConsolidatedEdgeBlock> {
    let records = consolidated_records(data);
    crate::test_support::with_service_context(|ctx| {
        consolidated_edge_blocks_from_records(ctx, data, &records).expect("service decode")
    })
}

fn consolidated_edge_blocks_from_records(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Result<Vec<ConsolidatedEdgeBlock>, CodecError> {
    let mut pcurves = BTreeMap::new();
    for family in [ConsolidatedFamily::A, ConsolidatedFamily::B] {
        for value in family_pcurves_from_records(ctx, data, records, family)? {
            ctx.insert_btree_map(
                &mut pcurves,
                value.pos,
                value,
                "catia_consolidated_edge_pcurves",
            )?;
        }
    }
    let mut parameters = BTreeMap::new();
    for value in b2_edge_parameters_from_records(ctx, data, records)? {
        ctx.insert_btree_map(
            &mut parameters,
            value.pos,
            value,
            "catia_consolidated_edge_parameters",
        )?;
    }
    let mut blocks = Vec::new();
    for window in ctx
        .admit_iter(records, "catia_consolidated_record_windows")?
        .windows(NonZeroUsize::new(3).ok_or_else(|| {
            ctx.refuse_codec_limit("catia_consolidated_record_windows", u64::MAX, u64::MAX)
        })?)
    {
        let [first_record, second_record, parameter_record] = window else {
            continue;
        };
        if records_are_contiguous(&[first_record, second_record, parameter_record])
            && first_record.class() == 0x20
            && second_record.class() == 0x20
            && first_record.family() == second_record.family()
            && parameter_record.family() == ConsolidatedFamily::B
            && parameter_record.class() == 0x23
        {
            if let (Some(first), Some(second), Some(parameter)) = (
                pcurves.get(&first_record.byte_offset()),
                pcurves.get(&second_record.byte_offset()),
                parameters.get(&parameter_record.byte_offset()),
            ) {
                let co_parametric = first.sites.len() == second.sites.len()
                    && first.range == second.range
                    && first.range == parameter.range;
                if co_parametric {
                    if let (Some(first), Some(second), Some(parameter)) = (
                        pcurves.remove(&first_record.byte_offset()),
                        pcurves.remove(&second_record.byte_offset()),
                        parameters.remove(&parameter_record.byte_offset()),
                    ) {
                        ctx.push_vec(
                            &mut blocks,
                            ConsolidatedEdgeBlock {
                                pcurves: [first, second],
                                parameters: parameter,
                            },
                            "catia_consolidated_edge_blocks",
                        )?;
                    }
                }
            }
        }
    }
    Ok(blocks)
}

/// Decode complete six-record consolidated edge runs. Records separated by any
/// other framed record do not form a run.
#[must_use]
#[cfg(test)]
fn consolidated_topology_edge_runs(data: &[u8]) -> Vec<ConsolidatedTopologyEdgeRun> {
    let records = consolidated_records(data);
    crate::test_support::with_service_context(|ctx| {
        consolidated_topology_edge_runs_from_records(ctx, data, &records).expect("service decode")
    })
}

pub(crate) fn consolidated_topology_edge_runs_from_records(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Result<Vec<ConsolidatedTopologyEdgeRun>, CodecError> {
    let (edge_index, _edge_index_storage) = ctx.collect_scoped_btree_map(
        consolidated_edge_blocks_from_records(ctx, data, records)?
            .into_iter()
            .map(|edge| (edge.pcurves[0].pos, edge)),
        "catia_consolidated_topology_edges",
    )?;
    let mut edges = edge_index;
    let (use_index, _use_index_storage) = ctx.collect_scoped_btree_map(
        consolidated_edge_use_runs_from_records(ctx, data, records)?
            .into_iter()
            .map(|value| (value.uses[0].pos, value)),
        "catia_consolidated_topology_uses",
    )?;
    let mut use_runs = use_index;
    let mut runs = Vec::new();
    for window in ctx
        .admit_iter(records, "catia_consolidated_record_windows")?
        .windows(NonZeroUsize::new(6).ok_or_else(|| {
            ctx.refuse_codec_limit("catia_consolidated_record_windows", u64::MAX, u64::MAX)
        })?)
    {
        let [pcurve0, pcurve1, parameters, use0, use1, node] = window else {
            continue;
        };
        if records_are_contiguous(&[pcurve0, pcurve1, parameters, use0, use1, node])
            && pcurve0.class() == 0x20
            && pcurve1.class() == 0x20
            && pcurve0.family() == pcurve1.family()
            && parameters.family() == ConsolidatedFamily::B
            && parameters.class() == 0x23
            && use0.family() == ConsolidatedFamily::B
            && use0.class() == 0x06
            && use1.family() == ConsolidatedFamily::B
            && use1.class() == 0x06
            && node.family() == ConsolidatedFamily::B
            && node.class() == 0x5e
        {
            if let (Some(edge), Some(use_run)) = (
                edges.remove(&pcurve0.byte_offset()),
                use_runs.remove(&use0.byte_offset()),
            ) {
                ctx.push_vec(
                    &mut runs,
                    ConsolidatedTopologyEdgeRun {
                        edge,
                        node: use_run.node,
                    },
                    "catia_consolidated_topology_runs",
                )?;
            }
        }
    }
    Ok(runs)
}

/// Decode adjacent `18,19,23,06,06,5e` analytic-circle edge runs. The
/// class-`0x23` definition must close under the eight-scalar grammar.
#[must_use]
#[cfg(test)]
fn consolidated_analytic_circle_edge_runs(data: &[u8]) -> Vec<ConsolidatedAnalyticCircleEdgeRun> {
    let records = consolidated_records(data);
    crate::test_support::with_service_context(|ctx| {
        consolidated_analytic_circle_edge_runs_from_records(ctx, data, &records)
    })
    .expect("service decode")
}

pub(crate) fn consolidated_analytic_circle_edge_runs_from_records(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Result<Vec<ConsolidatedAnalyticCircleEdgeRun>, CodecError> {
    let mut temporary = ctx.reserve_scoped(0, "catia edge resolution workspace")?;
    let mut circles = BTreeMap::new();
    for value in ctx
        .admit_iter(records, "catia_b2_family_record_scan")?
        .filter_map(|record| b2_circle_from_record(data, record))
    {
        temporary.with_storage(|| {
            ctx.insert_btree_map(
                &mut circles,
                value.pos,
                value,
                "catia_analytic_circle_carriers",
            )
        })?;
    }
    let (use_index, _use_index_storage) = ctx.collect_scoped_btree_map(
        temporary
            .with_storage(|| consolidated_edge_use_runs_from_records(ctx, data, records))?
            .into_iter()
            .map(|value| (value.uses[0].pos, value)),
        "catia_analytic_circle_use_runs",
    )?;
    let use_runs = use_index;
    let mut runs = Vec::new();
    for window in ctx
        .admit_iter(records, "catia_consolidated_record_windows")?
        .windows(NonZeroUsize::new(6).ok_or_else(|| {
            ctx.refuse_codec_limit("catia_consolidated_record_windows", u64::MAX, u64::MAX)
        })?)
    {
        let candidate = (|| {
            let [parameter, circle, definition, use0, use1, node] = window else {
                return None;
            };
            if !records_are_contiguous(&[parameter, circle, definition, use0, use1, node]) {
                return None;
            }
            if parameter.family() != ConsolidatedFamily::B
                || parameter.class() != 0x18
                || circle.family() != ConsolidatedFamily::B
                || circle.class() != 0x19
                || definition.family() != ConsolidatedFamily::B
                || definition.class() != 0x23
                || use0.family() != ConsolidatedFamily::B
                || use0.class() != 0x06
                || use1.family() != ConsolidatedFamily::B
                || use1.class() != 0x06
                || node.family() != ConsolidatedFamily::B
                || node.class() != 0x5e
            {
                return None;
            }
            let use_run = use_runs.get(&use0.byte_offset())?;
            let definition = use_run.definition.as_ref()?;
            edge_definition_is_scalar_eight(definition).then_some((
                parameter,
                circles.get(&circle.byte_offset())?,
                use_run,
            ))
        })();
        let Some((parameter, circle, use_run)) = candidate else {
            continue;
        };
        let Some(payload) = parameter.payload() else {
            continue;
        };
        #[cfg(test)]
        let Some(definition) = use_run.definition.as_ref() else {
            continue;
        };
        #[cfg(test)]
        let definition = ConsolidatedEdgeDefinition {
            frame: ConsolidatedRawFrame::new(
                definition.frame.pos,
                definition.frame.width(),
                definition.frame.flag,
                definition.frame.header_token(),
                ctx.copy_slice(
                    &definition.frame.payload,
                    "catia_analytic_circle_test_definition_payload",
                )?,
            )
            .map_err(CodecError::malformed)?,
            class: definition.class,
        };
        let descriptor = ConsolidatedRawFrame::from_record(
            parameter,
            ctx.copy_slice(&data[payload], "catia_analytic_circle_descriptor_payload")?,
        )?;
        ctx.push_vec(
            &mut runs,
            ConsolidatedAnalyticCircleEdgeRun {
                descriptor,
                circle: circle.clone(),
                #[cfg(test)]
                definition,
                node: use_run.node,
            },
            "catia_analytic_circle_edge_runs",
        )?;
    }
    Ok(runs)
}

/// Decode adjacent `18,25,06,06,5e` edge runs whose descriptor and definition
/// both close under their typed grammars.
#[must_use]
#[cfg(test)]
fn consolidated_class25_edge_runs(data: &[u8]) -> Vec<ConsolidatedClass25EdgeRun> {
    let records = consolidated_records(data);
    crate::test_support::with_service_context(|ctx| {
        consolidated_class25_edge_runs_from_records(ctx, data, &records).expect("service decode")
    })
}

pub(crate) fn consolidated_class25_edge_runs_from_records(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Result<Vec<ConsolidatedClass25EdgeRun>, CodecError> {
    let mut temporary = ctx.reserve_scoped(0, "catia edge resolution workspace")?;
    let (descriptor_index, _descriptor_index_storage) = ctx.collect_scoped_btree_map(
        temporary
            .with_storage(|| b2_class25_descriptors_from_records(ctx, data, records))?
            .into_iter()
            .map(|value| (value.pos, value)),
        "catia_class25_edge_descriptors",
    )?;
    let descriptors = descriptor_index;
    let (use_index, _use_index_storage) = ctx.collect_scoped_btree_map(
        temporary
            .with_storage(|| consolidated_edge_use_runs_from_records(ctx, data, records))?
            .into_iter()
            .map(|value| (value.uses[0].pos, value)),
        "catia_class25_edge_use_runs",
    )?;
    let use_runs = use_index;
    let mut runs = Vec::new();
    for window in ctx
        .admit_iter(records, "catia_consolidated_record_windows")?
        .windows(NonZeroUsize::new(5).ok_or_else(|| {
            ctx.refuse_codec_limit("catia_consolidated_record_windows", u64::MAX, u64::MAX)
        })?)
    {
        let candidate = (|| {
            let [descriptor, definition, use0, use1, node] = window else {
                return None;
            };
            if !records_are_contiguous(&[descriptor, definition, use0, use1, node]) {
                return None;
            }
            if descriptor.family() != ConsolidatedFamily::B
                || descriptor.class() != 0x18
                || definition.family() != ConsolidatedFamily::B
                || definition.class() != 0x25
                || use0.family() != ConsolidatedFamily::B
                || use0.class() != 0x06
                || use1.family() != ConsolidatedFamily::B
                || use1.class() != 0x06
                || node.family() != ConsolidatedFamily::B
                || node.class() != 0x5e
            {
                return None;
            }
            let use_run = use_runs.get(&use0.byte_offset())?;
            let definition = use_run.definition.as_ref()?;
            if !edge_definition_is_typed_class25(definition) {
                return None;
            }
            Some((descriptors.get(&descriptor.byte_offset())?, use_run.node))
        })();
        let Some((descriptor, node)) = candidate else {
            continue;
        };
        ctx.push_vec(
            &mut runs,
            ConsolidatedClass25EdgeRun {
                descriptor: B2Class25Descriptor {
                    pos: descriptor.pos,
                    record_id: descriptor.record_id,
                    control: descriptor.control,
                    values: ctx
                        .copy_slice(&descriptor.values, "catia_class25_edge_descriptor_values")?,
                },
                node,
            },
            "catia_class25_edge_runs",
        )?;
    }
    Ok(runs)
}

/// Decode every adjacent `06,06,5e` edge-use run independently of pcurve
/// availability. Records separated by another framed record do not form a run.
#[must_use]
#[cfg(test)]
fn consolidated_edge_use_runs(data: &[u8]) -> Vec<ConsolidatedEdgeUseRun> {
    let records = consolidated_records(data);
    crate::test_support::with_service_context(|ctx| {
        consolidated_edge_use_runs_from_records(ctx, data, &records).expect("service decode")
    })
}

pub(crate) fn consolidated_edge_use_runs_from_records(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Result<Vec<ConsolidatedEdgeUseRun>, CodecError> {
    let mut temporary = ctx.reserve_scoped(0, "catia edge resolution workspace")?;
    let (use_index, _use_index_storage) = ctx.collect_scoped_btree_map(
        temporary
            .with_storage(|| b2_use_metadata_from_records(ctx, data, records))?
            .into_iter()
            .map(|value| (value.pos, value)),
        "catia_edge_use_metadata_index",
    )?;
    let uses = use_index;
    let mut nodes = BTreeMap::new();
    for value in b2_edge_nodes_from_records(ctx, data, records)? {
        temporary.with_storage(|| {
            ctx.insert_btree_map(&mut nodes, value.pos, value, "catia_edge_use_node_index")
        })?;
    }
    let mut runs = Vec::new();
    for (index, window) in ctx
        .admit_iter(records, "catia_consolidated_record_windows")?
        .windows(NonZeroUsize::new(3).ok_or_else(|| {
            ctx.refuse_codec_limit("catia_consolidated_record_windows", u64::MAX, u64::MAX)
        })?)
        .enumerate()
    {
        let candidate = (|| {
            let [use0, use1, node] = window else {
                return None;
            };
            if !records_are_contiguous(&[use0, use1, node]) {
                return None;
            }
            if use0.family() != ConsolidatedFamily::B
                || use0.class() != 0x06
                || use1.family() != ConsolidatedFamily::B
                || use1.class() != 0x06
                || node.family() != ConsolidatedFamily::B
                || node.class() != 0x5e
            {
                return None;
            }
            let node = *nodes.get(&node.byte_offset())?;
            let uses = [
                uses.get(&use0.byte_offset())?,
                uses.get(&use1.byte_offset())?,
            ];
            let identity_chain_consistent = node
                .curve_ref
                .checked_sub(2)
                .zip(node.curve_ref.checked_sub(1))
                .is_some_and(|(first, second)| {
                    uses[0].references() == Some(&[first, second][..])
                        && uses[1].references() == Some(&[second, node.curve_ref][..])
                })
                && [node.start_parameter_ref, node.end_parameter_ref] == [2, 1];
            let definition = index
                .checked_sub(1)
                .and_then(|preceding| records.get(preceding))
                .filter(|record| {
                    record.source_index() == use0.source_index()
                        && record.source_range().end == use0.source_range().start
                        && record.family() == ConsolidatedFamily::B
                        && matches!(record.class(), 0x23..=0x25)
                });
            identity_chain_consistent.then_some((definition, uses, node))
        })();
        let Some((definition_record, uses, node)) = candidate else {
            continue;
        };
        let definition = match definition_record.and_then(|record| {
            Some((
                record,
                record.payload()?,
                ConsolidatedEdgeDefinitionClass::try_from(record.class()).ok()?,
            ))
        }) {
            Some((record, payload, class)) => Some(ConsolidatedEdgeDefinition {
                frame: ConsolidatedRawFrame::from_record(
                    record,
                    ctx.copy_slice(
                        &data[payload],
                        "catia_edge_use_preceding_definition_payload",
                    )?,
                )?,
                class,
            }),
            None => None,
        };
        let uses = [uses[0].clone_charged(ctx)?, uses[1].clone_charged(ctx)?];
        ctx.push_vec(
            &mut runs,
            ConsolidatedEdgeUseRun {
                definition,
                uses,
                node,
            },
            "catia_edge_use_runs",
        )?;
    }
    for window in ctx
        .admit_iter(records, "catia_consolidated_record_windows")?
        .windows(NonZeroUsize::new(4).ok_or_else(|| {
            ctx.refuse_codec_limit("catia_consolidated_record_windows", u64::MAX, u64::MAX)
        })?)
    {
        let candidate = (|| {
            let [node_record, definition_record, use0, use1] = window else {
                return None;
            };
            if !records_are_contiguous(&[node_record, definition_record, use0, use1])
                || node_record.family() != ConsolidatedFamily::B
                || node_record.class() != 0x5e
                || definition_record.family() != ConsolidatedFamily::B
                || definition_record.class() != 0x24
                || use0.family() != ConsolidatedFamily::B
                || use0.class() != 0x06
                || use1.family() != ConsolidatedFamily::B
                || use1.class() != 0x06
            {
                return None;
            }
            let node = *nodes.get(&node_record.byte_offset())?;
            let uses = [
                uses.get(&use0.byte_offset())?,
                uses.get(&use1.byte_offset())?,
            ];
            let payload = &data[definition_record.payload()?];
            if payload.first() != Some(&0x81) {
                return None;
            }
            let mut at = 1;
            let operand = compact_int(payload, &mut at)?;
            if payload.get(at..) != Some(&[0x0f, 0x87][..]) {
                return None;
            }
            let identity_chain_consistent = operand
                .checked_add(1)
                .zip(operand.checked_add(2))
                .is_some_and(|(first, second)| {
                    uses[0].references() == Some(&[node.start_parameter_ref, first][..])
                        && uses[1].references() == Some(&[node.end_parameter_ref, second][..])
                })
                && [node.start_parameter_ref, node.end_parameter_ref] == [1, 2];
            if !identity_chain_consistent {
                return None;
            }
            Some((definition_record, uses, node))
        })();
        let Some((definition_record, uses, node)) = candidate else {
            continue;
        };
        let Some(payload) = definition_record.payload() else {
            continue;
        };
        let Ok(class) = ConsolidatedEdgeDefinitionClass::try_from(definition_record.class()) else {
            continue;
        };
        let definition = Some(ConsolidatedEdgeDefinition {
            frame: ConsolidatedRawFrame::from_record(
                definition_record,
                ctx.copy_slice(
                    &data[payload],
                    "catia_edge_use_succeeding_definition_payload",
                )?,
            )?,
            class,
        });
        let uses = [uses[0].clone_charged(ctx)?, uses[1].clone_charged(ctx)?];
        ctx.push_vec(
            &mut runs,
            ConsolidatedEdgeUseRun {
                definition,
                uses,
                node,
            },
            "catia_edge_use_runs",
        )?;
    }
    Ok(runs)
}

/// Resolve compact owner references that land exactly on class-`0x5e` frames.
pub(crate) fn consolidated_owned_edge_nodes_from_records(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Result<Vec<ConsolidatedOwnedEdgeNode>, CodecError> {
    let mut temporary = ctx.reserve_scoped(0, "catia edge resolution workspace")?;
    let mut indices = BTreeMap::new();
    for (index, record) in ctx
        .admit_iter(records, "catia_consolidated_record_visits")?
        .enumerate()
    {
        temporary.with_storage(|| {
            ctx.insert_btree_map(
                &mut indices,
                record.byte_offset(),
                index,
                "catia_owned_edge_record_indices",
            )
        })?;
    }
    let mut nodes = BTreeMap::new();
    for node in b2_edge_nodes_from_records(ctx, data, records)? {
        temporary.with_storage(|| {
            ctx.insert_btree_map(&mut nodes, node.pos, node, "catia_owned_edge_nodes")
        })?;
    }
    let mut owned = Vec::new();
    let relations = temporary
        .with_storage(|| b2_adjacent_face_counted_owners_from_records(ctx, data, records))?;
    for relation in ctx.admit_iter(&relations, "catia_consolidated_owner_relations")? {
        let Some(&owner_index) = indices.get(&relation.owner.pos) else {
            continue;
        };
        for (&allocation_ordinal, &encoding) in ctx
            .admit_iter(
                &relation.owner.references,
                "catia_consolidated_owner_references",
            )?
            .zip(ctx.admit_iter(
                &relation.owner.reference_encodings,
                "catia_consolidated_owner_reference_encodings",
            )?)
        {
            if encoding != AllocationReferenceEncoding::OwnedChild {
                continue;
            }
            let Some(target_index) = usize::try_from(allocation_ordinal)
                .ok()
                .and_then(|ordinal| owner_index.checked_add(1 + ordinal))
                .filter(|target| *target < records.len())
            else {
                continue;
            };
            if !crate::wire::records::record_run_is_contiguous(
                ctx,
                &records[owner_index..=target_index],
            )? {
                continue;
            }
            let target = &records[target_index];
            if target.family() != ConsolidatedFamily::B || target.class() != 0x5e {
                continue;
            }
            let Some(&node) = nodes.get(&target.byte_offset()) else {
                continue;
            };
            if owned
                .last()
                .is_some_and(|previous: &ConsolidatedOwnedEdgeNode| {
                    previous.owner_pos == relation.owner.pos && previous.node.pos == node.pos
                })
            {
                continue;
            }
            ctx.push_vec(
                &mut owned,
                ConsolidatedOwnedEdgeNode {
                    owner_pos: relation.owner.pos,
                    allocation_ordinal,
                    node,
                },
                "catia_owned_edge_results",
            )?;
        }
    }
    Ok(owned)
}

/// Resolve compact edge endpoint references through the framed allocation walk.
pub(crate) fn consolidated_compact_edge_endpoints_from_records(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Result<Vec<ConsolidatedCompactEdgeEndpoints>, CodecError> {
    struct EndpointResolver<'a, 'b> {
        ctx: &'a DecodeContext<'b>,
        records: &'a [ConsolidatedRecord],
        nodes: &'a BTreeMap<usize, B2EdgeNode>,
        allocation_locations: &'a HashMap<usize, (usize, usize)>,
        allocation_scopes: &'a [Vec<usize>],
        active: HashSet<(usize, usize)>,
        memo: HashMap<(usize, usize), Option<usize>>,
        storage: ScopedReservation<'a>,
    }

    impl EndpointResolver<'_, '_> {
        fn resolve(
            &mut self,
            record_index: usize,
            endpoint: usize,
        ) -> Result<Option<usize>, CodecError> {
            let key = (record_index, endpoint);
            if let Some(cached) = self.memo.get(&key) {
                return Ok(*cached);
            }
            let _depth = self
                .ctx
                .enter_nested("catia_compact_endpoint_resolution_depth")?;
            self.ctx
                .charge_work(1, "catia_compact_endpoint_resolution_work")?;
            if !self.storage.with_storage(|| {
                self.ctx
                    .insert_hash_set(&mut self.active, key, "catia_compact_endpoint_active")
            })? {
                return Ok(None);
            }
            let result = (|| {
                let node = self.nodes.get(&record_index)?;
                let reference = [node.start_vertex_ref, node.end_vertex_ref][endpoint];
                let encoding = node.reference_encodings[endpoint + 1];
                let &(scope, allocation_ordinal) = self.allocation_locations.get(&record_index)?;
                let target = match encoding {
                    AllocationReferenceEncoding::OwnedChild => allocation_ordinal
                        .checked_add(1)?
                        .checked_add(usize::try_from(reference).ok()?)
                        .and_then(|target| self.allocation_scopes.get(scope)?.get(target))
                        .copied()?,
                    AllocationReferenceEncoding::BackwardDistance => {
                        let target =
                            allocation_ordinal.checked_sub(usize::try_from(reference).ok()?)?;
                        *self.allocation_scopes.get(scope)?.get(target)?
                    }
                    AllocationReferenceEncoding::WidthCoded => {
                        let target = record_index.checked_add(usize::try_from(reference).ok()?)?;
                        let target_record = self.records.get(target)?;
                        if target_record.source_index()
                            != self.records.get(record_index)?.source_index()
                            || target_record.family() != ConsolidatedFamily::B
                            || target_record.class() != 0x18
                        {
                            return None;
                        }
                        return Some(Ok(target));
                    }
                    AllocationReferenceEncoding::Selector2
                    | AllocationReferenceEncoding::TaggedU8
                    | AllocationReferenceEncoding::TaggedU16 => return None,
                };
                let target_record = self.records.get(target)?;
                if target_record.family() != ConsolidatedFamily::B {
                    return None;
                }
                match target_record.class() {
                    0x5d => Some(Ok(target)),
                    0x5e => self.resolve(target, 1).transpose(),
                    _ => None,
                }
            })();
            self.active.remove(&key);
            let result = result.transpose()?;
            self.storage.with_storage(|| {
                self.ctx
                    .insert_hash_map(&mut self.memo, key, result, "catia_compact_endpoint_memo")
            })?;
            Ok(result)
        }
    }

    let mut temporary = ctx.reserve_scoped(0, "catia compact endpoint workspace")?;
    let mut by_pos = HashMap::new();
    for node in b2_edge_nodes_from_records(ctx, data, records)? {
        temporary.with_storage(|| {
            ctx.insert_hash_map(
                &mut by_pos,
                node.pos,
                node,
                "catia_compact_endpoint_nodes_by_pos",
            )
        })?;
    }
    let mut nodes = BTreeMap::new();
    for (index, record) in ctx
        .admit_iter(records, "catia_consolidated_record_visits")?
        .enumerate()
    {
        if let Some(node) = ctx
            .get_hash_map(
                &by_pos,
                &record.byte_offset(),
                "catia_compact_endpoint_nodes_by_pos",
            )?
            .copied()
        {
            temporary.with_storage(|| {
                ctx.insert_btree_map(
                    &mut nodes,
                    index,
                    node,
                    "catia_compact_endpoint_nodes_by_index",
                )
            })?;
        }
    }
    let mut allocation_scopes = Vec::new();
    temporary.with_storage(|| {
        ctx.push_vec(
            &mut allocation_scopes,
            Vec::new(),
            "catia_compact_endpoint_scopes",
        )
    })?;
    let mut allocation_locations = HashMap::new();
    for (index, record) in ctx
        .admit_iter(records, "catia_consolidated_record_visits")?
        .enumerate()
    {
        if index > 0
            && (records[index - 1].source_index() != record.source_index()
                || records[index - 1].source_range().end != record.source_range().start)
        {
            temporary.with_storage(|| {
                ctx.push_vec(
                    &mut allocation_scopes,
                    Vec::new(),
                    "catia_compact_endpoint_scopes",
                )
            })?;
        }
        if record.family() == ConsolidatedFamily::B && matches!(record.class(), 0x5d | 0x5e) {
            let scope = allocation_scopes.len() - 1;
            let ordinal = allocation_scopes[scope].len();
            temporary.with_storage(|| {
                ctx.push_vec(
                    &mut allocation_scopes[scope],
                    index,
                    "catia_compact_endpoint_scope_entries",
                )
            })?;
            temporary.with_storage(|| {
                ctx.insert_hash_map(
                    &mut allocation_locations,
                    index,
                    (scope, ordinal),
                    "catia_compact_endpoint_locations",
                )
            })?;
        }
    }
    let mut resolver = EndpointResolver {
        ctx,
        storage: ctx.reserve_scoped(0, "catia compact endpoint memo workspace")?,
        records,
        nodes: &nodes,
        allocation_locations: &allocation_locations,
        allocation_scopes: &allocation_scopes,
        active: HashSet::new(),
        memo: HashMap::new(),
    };
    let mut endpoints = Vec::new();
    for (&record_index, &node) in ctx.admit_iter(&nodes, "catia_consolidated_endpoint_nodes")? {
        let vertices = [
            resolver.resolve(record_index, 0)?,
            resolver.resolve(record_index, 1)?,
        ];
        let [Some(start), Some(end)] = vertices else {
            continue;
        };
        ctx.push_vec(
            &mut endpoints,
            ConsolidatedCompactEdgeEndpoints {
                node,
                endpoint_records: [records[start].byte_offset(), records[end].byte_offset()],
            },
            "catia_compact_endpoint_bindings",
        )?;
    }
    ctx.stable_sort_by(
        &mut endpoints,
        |value| &value.node.pos,
        Ord::cmp,
        "catia_compact_endpoint_bindings_sort",
    )?;
    Ok(endpoints)
}

/// Derive owner-local four-edge boundary cycles from fixed-nine references.
/// Every returned endpoint is closed by the same bounded record source as its
/// owner. Other fixed-nine roles remain unclassified.
pub(crate) fn consolidated_owner_boundary_cycles_from_records(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Result<Vec<ConsolidatedOwnerBoundaryCycle>, CodecError> {
    let mut endpoint_records = HashMap::new();
    let bindings = consolidated_compact_edge_endpoints_from_records(ctx, data, records)?;
    for binding in ctx.admit_iter(&bindings, "catia_consolidated_owner_endpoint_bindings")? {
        ctx.insert_hash_map(
            &mut endpoint_records,
            binding.node.pos,
            binding.endpoint_records,
            "catia_owner_boundary_endpoints",
        )?;
    }
    let mut face_nodes = BTreeMap::new();
    for node in b2_face_nodes_5f_from_records(ctx, data, records)? {
        ctx.insert_btree_map(
            &mut face_nodes,
            node.pos,
            node,
            "catia_owner_boundary_face_nodes",
        )?;
    }
    let mut record_indices = BTreeMap::new();
    let mut record_sources = HashMap::new();
    for (index, record) in ctx
        .admit_iter(records, "catia_consolidated_record_visits")?
        .enumerate()
    {
        ctx.insert_btree_map(
            &mut record_indices,
            record.byte_offset(),
            index,
            "catia_owner_boundary_record_indices",
        )?;
        ctx.insert_hash_map(
            &mut record_sources,
            record.byte_offset(),
            record.source_index(),
            "catia_owner_boundary_record_sources",
        )?;
    }
    let mut targets_by_owner = BTreeMap::<(usize, usize), Vec<_>>::new();
    let identity_targets = b2_owner_identity_targets_from_records(ctx, data, records)?;
    for &target in ctx.admit_iter(
        &identity_targets,
        "catia_consolidated_owner_identity_targets",
    )? {
        let key = (target.source_index, target.owner_pos);
        if let Some(targets) = targets_by_owner.get_mut(&key) {
            ctx.push_vec(targets, target, "catia_owner_boundary_target_entries")?;
        } else {
            let mut targets = Vec::new();
            ctx.push_vec(&mut targets, target, "catia_owner_boundary_target_entries")?;
            ctx.insert_btree_map(
                &mut targets_by_owner,
                key,
                targets,
                "catia_owner_boundary_target_groups",
            )?;
        }
    }
    let mut cycles = Vec::new();
    for packet in b2_owner_packets_from_records(ctx, data, records)? {
        let Some(targets) = targets_by_owner.get(&(packet.source_index, packet.pos)) else {
            continue;
        };
        let Some(edges) = b2_closed_owner_boundary_edges(ctx, targets, &endpoint_records)? else {
            continue;
        };
        let cycle = {
            let face_node = (|| -> Result<Option<B2FaceNode5f>, CodecError> {
                macro_rules! boundary_value {
                    ($value:expr) => {
                        match $value {
                            Some(value) => value,
                            None => return Ok(None),
                        }
                    };
                }
                let first_edge_pos =
                    boundary_value!(edges.iter().map(|edge| edge.target_pos).min());
                let &first_edge_index = boundary_value!(record_indices.get(&first_edge_pos));
                let node_index = boundary_value!(first_edge_index.checked_sub(1));
                let node_record = boundary_value!(records.get(node_index));
                let face_node = boundary_value!(face_nodes.get(&node_record.byte_offset()));
                if !matches!(face_node.terminal, [0x27, 0x03 | 0x05]) {
                    return Ok(None);
                }
                let &owner_index = boundary_value!(record_indices.get(&packet.pos));
                if owner_index <= first_edge_index
                    || !crate::wire::records::record_run_is_contiguous(
                        ctx,
                        &records[node_index..=owner_index],
                    )?
                {
                    return Ok(None);
                }
                let span = &records[first_edge_index..owner_index];
                if ctx
                    .admit_iter(span, "catia_consolidated_owner_span_classes")?
                    .any(|record| {
                        record.family() != ConsolidatedFamily::B
                            || !matches!(record.class(), 0x5d | 0x5e)
                    })
                    || ctx
                        .admit_iter(span, "catia_consolidated_owner_span_nodes")?
                        .filter(|record| record.class() == 0x5e)
                        .count()
                        != 4
                {
                    return Ok(None);
                }
                for edge in &edges {
                    if !ctx
                        .admit_iter(span, "catia_consolidated_owner_span_membership")?
                        .any(|record| record.byte_offset() == edge.target_pos)
                    {
                        return Ok(None);
                    }
                }
                Ok(
                    (packet.references[8].checked_add(10) == Some(face_node.target))
                        .then_some(*face_node),
                )
            })()?;
            edges
                .iter()
                .all(|edge| {
                    record_sources.get(&edge.target_pos) == Some(&packet.source_index)
                        && edge.endpoint_records.iter().all(|endpoint| {
                            record_sources.get(endpoint) == Some(&packet.source_index)
                        })
                })
                .then_some(ConsolidatedOwnerBoundaryCycle {
                    source_index: packet.source_index,
                    owner_pos: packet.pos,
                    face_node,
                    edges,
                })
        };
        if let Some(cycle) = cycle {
            ctx.push_vec(&mut cycles, cycle, "catia_owner_boundary_cycles")?;
        }
    }
    Ok(cycles)
}

/// Build the native endpoint-incidence graph for all complete consolidated
/// edge runs. A broken use/edge allocation chain invalidates the graph.
#[must_use]
#[cfg(test)]
fn consolidated_native_edge_graph(data: &[u8]) -> Option<ConsolidatedNativeEdgeGraph> {
    let runs = consolidated_topology_edge_runs(data);
    if runs.is_empty() {
        return None;
    }
    let mut vertex_indices = HashMap::new();
    let mut vertex_identities = Vec::new();
    let mut edges = Vec::with_capacity(runs.len());
    for run in runs {
        let vertices = [run.node.start_vertex_ref, run.node.end_vertex_ref].map(|identity| {
            *vertex_indices.entry(identity).or_insert_with(|| {
                let index = vertex_identities.len();
                vertex_identities.push(identity);
                index
            })
        });
        edges.push(ConsolidatedNativeGraphEdge { vertices });
    }
    let mut vertex_edges = vec![Vec::new(); vertex_identities.len()];
    for (edge, value) in edges.iter().enumerate() {
        for vertex in value.vertices {
            vertex_edges[vertex].push(edge);
        }
    }
    let mut unseen = (0..edges.len()).collect::<std::collections::BTreeSet<_>>();
    let mut components = Vec::new();
    while let Some(&first) = unseen.first() {
        let mut component = Vec::new();
        let mut stack = vec![first];
        unseen.remove(&first);
        while let Some(edge) = stack.pop() {
            component.push(edge);
            for vertex in edges[edge].vertices {
                for &neighbor in &vertex_edges[vertex] {
                    if unseen.remove(&neighbor) {
                        stack.push(neighbor);
                    }
                }
            }
        }
        component.sort_unstable();
        components.push(component);
    }
    Some(ConsolidatedNativeEdgeGraph {
        vertex_identities,
        edges,
        components,
    })
}

/// Resolve consolidated edge sides against typed cylinder, circle, cone, and
/// NURBS carriers.
///
/// A carrier binds only when record identity or chart geometry determines one
/// solution. Ambiguous candidates, including matches from different analytic
/// families, remain unresolved.
#[cfg(test)]
fn resolve_consolidated_edge_blocks(
    data: &[u8],
) -> Result<Vec<ResolvedConsolidatedEdgeBlock>, CodecError> {
    let records = consolidated_records(data);
    crate::test_support::with_service_context(|ctx| {
        resolve_consolidated_edge_blocks_from_records(
            ctx,
            data,
            &records,
            &mut crate::nurbs::LaneRefusals::new(),
        )
    })
}

pub(crate) fn resolve_consolidated_edge_blocks_from_records(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    records: &[ConsolidatedRecord],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Vec<ResolvedConsolidatedEdgeBlock>, cadmpeg_core::CodecError> {
    let surfaces = a5_surfaces_from_records(ctx, data, records, refusal)?;
    let points = object_stream_vertices_from_records(ctx, data, records)?;
    let embedded = ctx.try_collect_vec(
        b2_embedded_cylinders_from_records(ctx, data, records)?,
        "catia_resolved_embedded_cylinders",
    )?;
    let standalone = ctx.try_collect_vec(
        b2_cylinders_from_records(ctx, data, records)?,
        "catia_resolved_standalone_cylinders",
    )?;
    let circles = ctx.collect_vec(
        ctx.admit_iter(records, "catia_resolved_circle_record_scan")?
            .filter_map(|record| b2_circle_from_record(data, record)),
        "catia_resolved_circles",
    )?;
    let cones = ctx.collect_vec(
        b2_cones_from_records(ctx, data, records)?,
        "catia_resolved_cones",
    )?;
    let spheres = ctx.collect_vec(
        b2_spheres_from_records(ctx, data, records)?,
        "catia_resolved_spheres",
    )?;
    let tori = ctx.collect_vec(
        b2_tori_from_records(ctx, data, records)?,
        "catia_resolved_tori",
    )?;
    let planes = b2_plane_carriers_from_records(ctx, data, records)?;
    let carriers = ConsolidatedCarriers {
        cylinders: &standalone,
        embedded_cylinders: &embedded,
        circles: &circles,
        cones: &cones,
        spheres: &spheres,
        tori: &tori,
        planes: &planes,
        nurbs_surfaces: &surfaces,
    };
    ctx.try_collect_vec(
        consolidated_edge_blocks_from_records(ctx, data, records)?
            .into_iter()
            .map(|block| -> Result<_, CodecError> {
                let mut supports = [None, None];
                for side in [0, 1] {
                    supports[side] =
                        resolve_side_support(ctx, &block.pcurves[side], &points, &carriers)?;
                }
                for anchor_side in [0, 1] {
                    let partner = 1 - anchor_side;
                    if supports[partner].is_some() {
                        continue;
                    }
                    let Some(binding) = supports[anchor_side].as_ref() else {
                        continue;
                    };
                    let Some(anchor_points) =
                        support_points(ctx, binding, &block.pcurves[anchor_side], &carriers)?
                    else {
                        continue;
                    };
                    let partner_points = ctx.collect_vec(
                        ctx.admit_iter(
                            &block.pcurves[partner].sites[..],
                            "catia_consolidated_partner_site_visits",
                        )?
                        .map(|site| site.point.get()),
                        "catia_resolved_partner_parameters",
                    )?;
                    let mut winners = Vec::new();
                    for surface in ctx.admit_iter(&surfaces, "catia_consolidated_surface_visits")? {
                        if let Some(offset) = nurbs_carrier_offset_surface(
                            ctx,
                            &surface.geometry,
                            &partner_points,
                            &anchor_points,
                        )? {
                            ctx.push_vec(
                                &mut winners,
                                ConsolidatedSupportBinding::NurbsCarrier {
                                    pos: surface.pos,
                                    offset,
                                },
                                "catia_resolved_partner_winners",
                            )?;
                        }
                    }
                    if let [winner] = winners.as_slice() {
                        supports[partner] = Some(*winner);
                    }
                }
                if supports.iter().all(Option::is_none) {
                    let mut candidates = [Vec::new(), Vec::new()];
                    for side in [0, 1] {
                        for surface in
                            ctx.admit_iter(&surfaces, "catia_consolidated_surface_visits")?
                        {
                            let binding = ConsolidatedSupportBinding::NurbsCarrier {
                                pos: surface.pos,
                                offset: FiniteReal::ZERO,
                            };
                            if let Some(points) =
                                support_points(ctx, &binding, &block.pcurves[side], &carriers)?
                            {
                                ctx.push_vec(
                                    &mut candidates[side],
                                    (binding, points),
                                    "catia_resolved_surface_candidates",
                                )?;
                            }
                        }
                    }
                    let mut winner = None;
                    'pairs: for (first_binding, first_points) in ctx.admit_iter(
                        &candidates[0],
                        "catia_consolidated_first_surface_candidates",
                    )? {
                        for (second_binding, second_points) in ctx.admit_iter(
                            &candidates[1],
                            "catia_consolidated_second_surface_candidates",
                        )? {
                            if !point_sequences_agree(ctx, first_points, second_points)? {
                                continue;
                            }
                            if winner.is_some() {
                                winner = None;
                                break 'pairs;
                            }
                            winner = Some([*first_binding, *second_binding]);
                        }
                    }
                    if let Some(winner) = winner {
                        supports = winner.map(Some);
                    }
                }
                let shared_loci = resolved_support_loci(ctx, &block, &supports, &carriers)?;
                let endpoint_loci = shared_loci
                    .as_ref()
                    .and_then(|points| Some([*points.first()?, *points.last()?]));
                Ok(ResolvedConsolidatedEdgeBlock {
                    block,
                    supports,
                    shared_loci,
                    endpoint_loci,
                })
            }),
        "catia_resolved_edge_blocks",
    )
}

fn resolve_side_support(
    ctx: &DecodeContext<'_>,
    pcurve: &ConsolidatedPcurve,
    points: &[FinitePoint3],
    carriers: &ConsolidatedCarriers<'_>,
) -> Result<Option<ConsolidatedSupportBinding>, CodecError> {
    let circle_count = ctx
        .admit_iter(carriers.circles, "catia_consolidated_circle_identity_count")?
        .filter(|circle| circle.record_id == pcurve.support_id)
        .count();
    let embedded_count = ctx
        .admit_iter(
            carriers.embedded_cylinders,
            "catia_consolidated_embedded_identity_count",
        )?
        .filter(|value| value.object_id == pcurve.support_id)
        .count();
    let identity_count = circle_count.checked_add(embedded_count).ok_or_else(|| {
        ctx.refuse_codec_limit(
            "catia_consolidated_carrier_identity_count",
            u64::MAX,
            u64::MAX,
        )
    })?;
    if identity_count > 1 {
        return Ok(None);
    }
    if let Some(circle) = ctx
        .admit_iter(
            carriers.circles,
            "catia_consolidated_circle_identity_lookup",
        )?
        .find(|circle| circle.record_id == pcurve.support_id)
    {
        return Ok(pcurve_matches_circle(pcurve, circle)
            .then_some(ConsolidatedSupportBinding::Circle { pos: circle.pos }));
    }
    if let Some(value) = ctx
        .admit_iter(
            carriers.embedded_cylinders,
            "catia_consolidated_embedded_identity_lookup",
        )?
        .find(|value| value.object_id == pcurve.support_id)
    {
        return Ok(pcurve_endpoints_match(ctx, pcurve, points, |uv| {
            Ok(b2_cylinder_point(&value.cylinder, uv))
        })?
        .then_some(ConsolidatedSupportBinding::EmbeddedCylinder {
            pos: value.pos,
            wrapper_pos: value.wrapper_pos,
        }));
    }
    let mut winners = Vec::new();
    for cylinder in ctx.admit_iter(carriers.cylinders, "catia_consolidated_cylinders_visits")? {
        if pcurve_endpoints_match(ctx, pcurve, points, |uv| {
            Ok(b2_cylinder_point(cylinder, uv))
        })? {
            ctx.push_vec(
                &mut winners,
                ConsolidatedSupportBinding::Cylinder { pos: cylinder.pos },
                "catia_resolved_side_winners",
            )?;
        }
    }
    for value in ctx.admit_iter(
        carriers.embedded_cylinders,
        "catia_consolidated_embedded_cylinders_visits",
    )? {
        if pcurve_endpoints_match(ctx, pcurve, points, |uv| {
            Ok(b2_cylinder_point(&value.cylinder, uv))
        })? {
            ctx.push_vec(
                &mut winners,
                ConsolidatedSupportBinding::EmbeddedCylinder {
                    pos: value.pos,
                    wrapper_pos: value.wrapper_pos,
                },
                "catia_resolved_side_winners",
            )?;
        }
    }
    for circle in ctx.admit_iter(carriers.circles, "catia_consolidated_circles_visits")? {
        if pcurve_matches_circle(pcurve, circle) {
            ctx.push_vec(
                &mut winners,
                ConsolidatedSupportBinding::Circle { pos: circle.pos },
                "catia_resolved_side_winners",
            )?;
        }
    }
    for cone in ctx.admit_iter(carriers.cones, "catia_consolidated_cones_visits")? {
        if pcurve_endpoints_match(ctx, pcurve, points, |uv| Ok(b2_cone_point(cone, uv)))? {
            ctx.push_vec(
                &mut winners,
                ConsolidatedSupportBinding::Cone { pos: cone.pos },
                "catia_resolved_side_winners",
            )?;
        }
    }
    for sphere in ctx.admit_iter(carriers.spheres, "catia_consolidated_spheres_visits")? {
        let geometry = b2_sphere_geometry(sphere);
        if pcurve_endpoints_match(ctx, pcurve, points, |[u, v]| {
            match cadmpeg_ir::eval::decode::outer_refusal(cadmpeg_ir::eval::decode::surface_point(
                ctx, &geometry, u, v,
            ))? {
                Ok(point) => Ok(Some(point.get())),
                Err(failure) => failure.non_finite(),
            }
        })? {
            ctx.push_vec(
                &mut winners,
                ConsolidatedSupportBinding::Sphere { pos: sphere.pos },
                "catia_resolved_side_winners",
            )?;
        }
    }
    for torus in ctx.admit_iter(carriers.tori, "catia_consolidated_tori_visits")? {
        if pcurve_endpoints_match(ctx, pcurve, points, |uv| b2_torus_point(ctx, torus, uv))? {
            ctx.push_vec(
                &mut winners,
                ConsolidatedSupportBinding::Torus { pos: torus.pos },
                "catia_resolved_side_winners",
            )?;
        }
    }
    for plane in ctx.admit_iter(carriers.planes, "catia_consolidated_planes_visits")? {
        if let Some(geometry) = b2_plane_geometry(plane) {
            if pcurve_endpoints_match(ctx, pcurve, points, |[u, v]| {
                match cadmpeg_ir::eval::decode::outer_refusal(
                    cadmpeg_ir::eval::decode::surface_point(ctx, &geometry, u, v),
                )? {
                    Ok(point) => Ok(Some(point.get())),
                    Err(failure) => failure.non_finite(),
                }
            })? {
                ctx.push_vec(
                    &mut winners,
                    ConsolidatedSupportBinding::Plane { pos: plane.pos },
                    "catia_resolved_side_winners",
                )?;
            }
        }
    }
    Ok(match winners.as_slice() {
        [winner] => Some(*winner),
        _ => None,
    })
}

fn point_sequences_agree(
    ctx: &DecodeContext<'_>,
    first: &[Point3],
    second: &[Point3],
) -> Result<bool, CodecError> {
    Ok(!first.is_empty()
        && first.len() == second.len()
        && ctx
            .admit_iter(first, "catia_consolidated_first_point_sequence")?
            .zip(ctx.admit_iter(second, "catia_consolidated_second_point_sequence")?)
            .all(|(&left, &right)| distance(left, right) <= 2e-3))
}

fn resolved_support_loci(
    ctx: &DecodeContext<'_>,
    block: &ConsolidatedEdgeBlock,
    supports: &[Option<ConsolidatedSupportBinding>; 2],
    carriers: &ConsolidatedCarriers<'_>,
) -> Result<Option<Vec<Point3>>, CodecError> {
    let mut first = None::<Vec<Point3>>;
    for (binding, pcurve) in supports.iter().zip(&block.pcurves) {
        let Some(binding) = binding.as_ref() else {
            continue;
        };
        let Some(points) = support_points(ctx, binding, pcurve, carriers)? else {
            continue;
        };
        if points.is_empty() {
            continue;
        }
        if let Some(previous) = first.as_ref() {
            if !point_sequences_agree(ctx, previous, &points)? {
                return Ok(None);
            }
        } else {
            first = Some(points);
        }
    }
    Ok(first)
}

fn support_points(
    ctx: &DecodeContext<'_>,
    binding: &ConsolidatedSupportBinding,
    pcurve: &ConsolidatedPcurve,
    carriers: &ConsolidatedCarriers<'_>,
) -> Result<Option<Vec<Point3>>, CodecError> {
    let points = match binding {
        ConsolidatedSupportBinding::Cylinder { pos } => {
            let Some(carrier) = ctx
                .admit_iter(carriers.cylinders, "catia_consolidated_carrier_visits")?
                .find(|value| value.pos == *pos)
            else {
                return Ok(None);
            };
            ctx.collect_options(
                ctx.admit_iter(&pcurve.sites[..], "catia_consolidated_pcurve_site_visits")?
                    .map(|site| b2_cylinder_point(carrier, site.point.get())),
                "catia_resolved_support_points",
            )?
        }
        ConsolidatedSupportBinding::EmbeddedCylinder { pos, .. } => {
            let Some(carrier) = ctx
                .admit_iter(
                    carriers.embedded_cylinders,
                    "catia_consolidated_carrier_visits",
                )?
                .find(|value| value.pos == *pos)
                .map(|value| &value.cylinder)
            else {
                return Ok(None);
            };
            ctx.collect_options(
                ctx.admit_iter(&pcurve.sites[..], "catia_consolidated_pcurve_site_visits")?
                    .map(|site| b2_cylinder_point(carrier, site.point.get())),
                "catia_resolved_support_points",
            )?
        }
        ConsolidatedSupportBinding::Cone { pos } => {
            let Some(carrier) = ctx
                .admit_iter(carriers.cones, "catia_consolidated_carrier_visits")?
                .find(|value| value.pos == *pos)
            else {
                return Ok(None);
            };
            ctx.collect_options(
                ctx.admit_iter(&pcurve.sites[..], "catia_consolidated_pcurve_site_visits")?
                    .map(|site| b2_cone_point(carrier, site.point.get())),
                "catia_resolved_support_points",
            )?
        }
        ConsolidatedSupportBinding::Sphere { pos } => {
            let Some(carrier) = ctx
                .admit_iter(carriers.spheres, "catia_consolidated_carrier_visits")?
                .find(|value| value.pos == *pos)
            else {
                return Ok(None);
            };
            ctx.collect_fallible_options(
                ctx.admit_iter(&pcurve.sites[..], "catia_consolidated_pcurve_site_visits")?
                    .map(|site| {
                        let [u, v] = site.point.get();
                        match cadmpeg_ir::eval::decode::outer_refusal(
                            cadmpeg_ir::eval::decode::surface_point(
                                ctx,
                                &b2_sphere_geometry(carrier),
                                u,
                                v,
                            ),
                        )? {
                            Ok(point) => Ok(Some(point.get())),
                            Err(failure) => failure.non_finite(),
                        }
                    }),
                "catia_resolved_support_points",
            )?
        }
        ConsolidatedSupportBinding::Torus { pos } => {
            let Some(carrier) = ctx
                .admit_iter(carriers.tori, "catia_consolidated_carrier_visits")?
                .find(|value| value.pos == *pos)
            else {
                return Ok(None);
            };
            ctx.collect_fallible_options(
                ctx.admit_iter(&pcurve.sites[..], "catia_consolidated_pcurve_site_visits")?
                    .map(|site| b2_torus_point(ctx, carrier, site.point.get())),
                "catia_resolved_support_points",
            )?
        }
        ConsolidatedSupportBinding::Plane { pos } => {
            let Some(carrier) = ctx
                .admit_iter(carriers.planes, "catia_consolidated_carrier_visits")?
                .find(|value| value.pos == *pos)
            else {
                return Ok(None);
            };
            let Some(geometry) = b2_plane_geometry(carrier) else {
                return Ok(None);
            };
            ctx.collect_fallible_options(
                ctx.admit_iter(&pcurve.sites[..], "catia_consolidated_pcurve_site_visits")?
                    .map(|site| {
                        let [u, v] = site.point.get();
                        match cadmpeg_ir::eval::decode::outer_refusal(
                            cadmpeg_ir::eval::decode::surface_point(ctx, &geometry, u, v),
                        )? {
                            Ok(point) => Ok(Some(point.get())),
                            Err(failure) => failure.non_finite(),
                        }
                    }),
                "catia_resolved_support_points",
            )?
        }
        ConsolidatedSupportBinding::NurbsCarrier { pos, offset } => {
            let Some(surface) = ctx
                .admit_iter(carriers.nurbs_surfaces, "catia_consolidated_carrier_visits")?
                .find(|surface| surface.pos == *pos)
                .map(|surface| &surface.geometry)
            else {
                return Ok(None);
            };
            ctx.collect_fallible_options(
                ctx.admit_iter(&pcurve.sites[..], "catia_consolidated_pcurve_site_visits")?
                    .map(|site| {
                        let [u, v] = site.point.get();
                        let partials = match nurbs_surface_partials(ctx, surface, u, v) {
                            Ok(partials) => partials,
                            Err(failure) => return failure.non_finite(),
                        };
                        let Some(normal) = partials.du.cross(partials.dv.get()).unit() else {
                            return Ok(None);
                        };
                        Ok(Some(Point3::new(
                            partials.point.x + offset.get() * normal.x,
                            partials.point.y + offset.get() * normal.y,
                            partials.point.z + offset.get() * normal.z,
                        )))
                    }),
                "catia_resolved_support_points",
            )?
        }
        ConsolidatedSupportBinding::Circle { .. } => None,
    };
    Ok(points)
}

/// The torus point at stored site `[u, v]`. A non-finite point is returned
/// as the evaluation reached it; the loci comparisons read it as a
/// disagreement.
fn b2_torus_point(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    torus: &B2Torus,
    [u, v]: [f64; 2],
) -> Result<Option<Point3>, cadmpeg_core::decode::ResourceLimit> {
    match cadmpeg_ir::eval::decode::outer_refusal(cadmpeg_ir::eval::decode::surface_point(
        ctx,
        &b2_torus_geometry(torus),
        u / torus.major_scale.get(),
        v / torus.minor_scale.get(),
    ))? {
        Ok(point) => Ok(Some(point.get())),
        Err(failure) => failure.non_finite(),
    }
}

#[cfg(test)]
fn nurbs_carrier_offset(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    geometry: &SurfaceGeometry,
    parameters: &[[f64; 2]],
    anchors: &[Point3],
) -> Result<Option<FiniteReal>, cadmpeg_core::decode::ResourceLimit> {
    ctx.charge_work_limit(0, "catia surface offset boundary")?;
    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)) = geometry else {
        return Ok(None);
    };
    nurbs_carrier_offset_surface(ctx, surface, parameters, anchors)
}

fn nurbs_carrier_offset_surface(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    parameters: &[[f64; 2]],
    anchors: &[Point3],
) -> Result<Option<FiniteReal>, cadmpeg_core::decode::ResourceLimit> {
    ctx.charge_work_limit(0, "catia surface offset boundary")?;
    if parameters.len() != anchors.len() || parameters.is_empty() {
        return Ok(None);
    }
    let mut first = None::<FiniteReal>;
    for (&[u, v], &anchor) in ctx
        .admit_iter(parameters, "catia_consolidated_offset_parameters")?
        .zip(ctx.admit_iter(anchors, "catia_consolidated_offset_anchors")?)
    {
        ctx.charge_work_limit(1, "catia surface offset sample")?;
        let partials = match nurbs_surface_partials(ctx, surface, u, v) {
            Ok(partials) => partials,
            Err(cadmpeg_ir::eval::EvaluationFailure::ResourceLimit(limit)) => return Err(limit),
            Err(_) => return Ok(None),
        };
        let point = partials.point;
        let residual = Vector3::new(anchor.x - point.x, anchor.y - point.y, anchor.z - point.z);
        if residual == Vector3::new(0.0, 0.0, 0.0) {
            if first
                .is_some_and(|value| value.get().abs() > EPS_SAMPLE_AGREEMENT * value.get().abs())
            {
                return Ok(None);
            }
            first.get_or_insert(FiniteReal::ZERO);
            continue;
        }
        let residual_length = residual.x.hypot(residual.y).hypot(residual.z);
        let Some(normal) = partials.du.cross(partials.dv.get()).unit() else {
            return Ok(None);
        };
        let distance = residual.x * normal.x + residual.y * normal.y + residual.z * normal.z;
        let transverse = Vector3::new(
            residual.x - normal.x * distance,
            residual.y - normal.y * distance,
            residual.z - normal.z * distance,
        );
        let transverse_length = transverse.x.hypot(transverse.y).hypot(transverse.z);
        let Some(distance) = FiniteReal::new(distance) else {
            return Ok(None);
        };
        if !residual_length.is_finite()
            || !transverse_length.is_finite()
            || transverse_length > EPS_TRANSVERSE_RESIDUAL * residual_length
        {
            return Ok(None);
        }
        if let Some(value) = first {
            if (distance.get() - value.get()).abs()
                > EPS_SAMPLE_AGREEMENT * distance.get().abs().max(value.get().abs())
            {
                return Ok(None);
            }
        } else {
            first = Some(distance);
        }
    }
    Ok(first)
}

fn pcurve_matches_circle(pcurve: &ConsolidatedPcurve, circle: &B2Circle) -> bool {
    let (Some(first), Some(last)) = (
        pcurve.sites.first().map(|site| site.point.get()),
        pcurve.sites.last().map(|site| site.point.get()),
    ) else {
        return false;
    };
    let span = circle.range.upper() - circle.range.lower();
    span.is_finite()
        && (first[1] - last[1]).abs() <= EPS_ENDPOINT_RANGE * span
        && (first[0].min(last[0]) - circle.range.lower()).abs() <= EPS_CIRCLE_ENDPOINT * span
        && (first[0].max(last[0]) - circle.range.upper()).abs() <= EPS_CIRCLE_ENDPOINT * span
}

fn pcurve_endpoints_match(
    ctx: &DecodeContext<'_>,
    pcurve: &ConsolidatedPcurve,
    vertices: &[FinitePoint3],
    evaluate: impl Fn([f64; 2]) -> Result<Option<Point3>, cadmpeg_core::decode::ResourceLimit>,
) -> Result<bool, cadmpeg_core::decode::ResourceLimit> {
    let (Some(first), Some(last)) = (
        pcurve.sites.first().map(|site| site.point.get()),
        pcurve.sites.last().map(|site| site.point.get()),
    ) else {
        return Ok(false);
    };
    for uv in [first, last] {
        let matches = match evaluate(uv)? {
            Some(point) => ctx
                .admit_iter(vertices, "catia_consolidated_endpoint_vertex_visits")?
                .any(|vertex| distance(point, vertex.get()) < 2e-3),
            None => false,
        };
        if !matches {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Read `05 08 01` coordinate rows outside every length-closed consolidated
/// A/B or B5/A8 record. Marker-like bytes inside record payloads are not
/// vertices.
pub(in crate::families) fn object_stream_vertices(
    ctx: &DecodeContext<'_>,
    data: &[u8],
) -> Result<Vec<FinitePoint3>, CodecError> {
    let records = crate::wire::records::consolidated_records_in_sources(
        ctx,
        data,
        std::iter::once(std::iter::once(crate::wire::records::SourceExtent::whole(
            data,
        ))),
    )?;
    object_stream_vertices_from_records(ctx, data, &records)
}

fn object_stream_vertices_from_records(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    records: &[crate::wire::records::ConsolidatedRecord],
) -> Result<Vec<FinitePoint3>, CodecError> {
    let mut points = Vec::new();
    let ranges = object_stream_vertex_row_ranges_from_records(ctx, data, records)?;
    for range in ctx.admit_iter(&ranges, "catia_consolidated_vertex_ranges")? {
        let vertices =
            crate::wire::records::scan_vertex_records(ctx, &data[range.start..range.end])?;
        for point in vertices {
            ctx.push_vec(&mut points, point, "catia_object_stream_vertices")?;
        }
    }
    Ok(points)
}

fn object_stream_vertex_row_ranges_from_records(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    records: &[crate::wire::records::ConsolidatedRecord],
) -> Result<Vec<Range<usize>>, CodecError> {
    let mut ranges = ctx.try_collect_vec(
        ctx.admit_iter(records, "catia_consolidated_frame_record_ranges")?
            .filter_map(crate::wire::records::ConsolidatedRecord::range)
            .map(Ok)
            .chain(crate::families::b5::graph::framed_ranges(ctx, data)?),
        "catia_object_stream_frame_ranges",
    )?;
    if ranges.is_empty() {
        return Ok(Vec::new());
    }
    ctx.sort_unstable_by_key(
        &mut ranges,
        |value| (value.start, value.end),
        Ord::cmp,
        "catia_object_stream_frame_ranges_sort",
    )?;
    let mut rows = Vec::new();
    let mut region_start = 0usize;
    for range in ctx.admit_iter(&ranges, "catia_consolidated_frame_ranges")? {
        if range.end <= region_start {
            continue;
        }
        if range.start > region_start {
            let vertex_rows = scan_vertex_record_ranges(ctx, &data[region_start..range.start])?;
            for row in vertex_rows {
                ctx.push_vec(
                    &mut rows,
                    row.start + region_start..row.end + region_start,
                    "catia_object_stream_vertex_rows",
                )?;
            }
        }
        region_start = region_start.max(range.end);
    }
    let vertex_rows = scan_vertex_record_ranges(ctx, &data[region_start..])?;
    for row in vertex_rows {
        ctx.push_vec(
            &mut rows,
            row.start + region_start..row.end + region_start,
            "catia_object_stream_vertex_rows",
        )?;
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use cadmpeg_ir::geometry::{nurbs::NurbsSurface, SolvedSurfaceGeometry, SurfaceGeometry};
    use cadmpeg_ir::math::Point3;

    use crate::families::b2::records::B2Circle;
    use crate::wire::records::ConsolidatedPcurve;

    use super::{nurbs_carrier_offset, pcurve_matches_circle, ConsolidatedEdgeDefinitionData};

    #[test]
    fn consolidated_point_sequence_zip_propagates_each_source_refusal() {
        let points = [Point3::new(0.0, 0.0, 0.0)];
        for (cap, operation) in [
            (0, "catia_consolidated_first_point_sequence"),
            (1, "catia_consolidated_second_point_sequence"),
        ] {
            crate::test_support::with_work_limit(cap, |ctx| {
                let result = super::point_sequences_agree(ctx, &points, &points);
                let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = result else {
                    panic!("point sequence work refusal required")
                };
                assert_eq!(limit.operation, operation);
                assert_eq!(ctx.resource_refusal(), Some(limit));
            });
        }
        crate::test_support::with_service_context(|ctx| {
            assert!(super::point_sequences_agree(ctx, &points, &points)
                .expect("service context admits equal point sequences"));
        });
    }

    #[test]
    fn consolidated_endpoint_vertex_scan_propagates_caller_refusal() {
        let bytes = crate::test_support::test_a5a8::a5_pcurve_stream();
        let records = crate::wire::records::consolidated_records(&bytes);
        let pcurves = crate::test_support::with_service_context(|ctx| {
            crate::wire::records::family_pcurves_from_records(
                ctx,
                &bytes,
                &records,
                crate::wire::records::ConsolidatedFamily::A,
            )
        })
        .expect("service context admits pcurve fixture");
        let point = Point3::new(0.0, 0.0, 0.0);
        let vertices = [cadmpeg_ir::features::FinitePoint3::new(point).expect("finite vertex")];
        crate::test_support::with_work_limit(0, |ctx| {
            let result =
                super::pcurve_endpoints_match(ctx, &pcurves[0], &vertices, |_| Ok(Some(point)));
            let Err(limit) = result else {
                panic!("endpoint vertex work refusal required")
            };
            assert_eq!(limit.operation, "catia_consolidated_endpoint_vertex_visits");
            assert_eq!(ctx.resource_refusal(), Some(limit));
        });
    }

    fn scalar_segment() -> super::Class25ScalarSegment {
        let value = cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite scalar");
        super::Class25ScalarSegment::M82Five(Box::new([value; 5]))
    }

    #[test]
    fn class25_scalar_segment_borrowed_wire_preserves_json_bytes() {
        let value = cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite scalar");
        for segment in [
            scalar_segment(),
            super::Class25ScalarSegment::M82Six(Box::new([value; 6])),
            super::Class25ScalarSegment::M82Seven(Box::new([value; 7])),
            super::Class25ScalarSegment::M83Eight(Box::new([value; 8])),
            super::Class25ScalarSegment::M83Nine(Box::new([value; 9])),
            super::Class25ScalarSegment::M89(Box::new([value; 20])),
            super::Class25ScalarSegment::M8b(Box::new([value; 24])),
        ] {
            let owned: super::Class25ScalarSegmentWire = segment.clone().into();
            assert_eq!(
                serde_json::to_vec(&segment).expect("borrowed segment JSON"),
                serde_json::to_vec(&owned).expect("owned segment JSON")
            );
        }
    }

    #[test]
    fn class25_scalar_segment_retained_limit_refuses_json_record() {
        #[derive(serde::Serialize)]
        struct Record<'a> {
            id: &'static str,
            #[serde(flatten)]
            segment: &'a super::Class25ScalarSegment,
        }
        let segment = scalar_segment();

        let record = Record {
            id: "catia:test:segment#0",
            segment: &segment,
        };
        let arena_name = "scalar_segments";
        let json_len = serde_json::to_vec(&record).expect("segment JSON").len();
        let limit = u64::try_from(json_len + arena_name.len() - 1).expect("small JSON");
        let refused = crate::test_support::with_retained_limit(limit, |ctx| {
            let mut namespace = cadmpeg_ir::NativeNamespace::default();
            namespace.set_arena(ctx, arena_name, std::slice::from_ref(&record))
        });
        let error = refused.expect_err("segment exceeds retained-byte limit");
        assert!(error.to_string().contains("RetainedBytes"), "{error}");
        crate::test_support::with_service_context(|ctx| {
            let mut namespace = cadmpeg_ir::NativeNamespace::default();
            namespace
                .set_arena(ctx, arena_name, std::slice::from_ref(&record))
                .expect("service profile admits segment");
        });
    }

    #[test]
    fn nurbs_carrier_offset_preserves_tiny_nonzero_distance() {
        let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
            NurbsSurface::from_lanes(
                &cadmpeg_test_support::service_decode_context(),
                cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    false,
                ),
                cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    false,
                ),
                cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(
                    vec![
                        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
                        vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
                    ],
                    None,
                ),
                false,
            )
            .expect("fixture constructor admission")
            .expect("valid unit-square surface"),
        ));
        let tiny = 1e-200;
        let offset = nurbs_carrier_offset(
            &cadmpeg_test_support::service_decode_context(),
            &surface,
            &[[0.25, 0.25], [0.75, 0.75]],
            &[Point3::new(0.25, 0.25, tiny), Point3::new(0.75, 0.75, tiny)],
        )
        .expect("evaluator allocation succeeds")
        .expect("constant normal offset");
        assert_eq!(offset.get(), tiny);

        assert_eq!(
            nurbs_carrier_offset(
                &cadmpeg_test_support::service_decode_context(),
                &surface,
                &[[0.25, 0.25], [0.75, 0.75]],
                &[
                    Point3::new(0.25, 0.25, tiny),
                    Point3::new(0.75, 0.75, 2.0 * tiny),
                ],
            )
            .expect("evaluator allocation succeeds"),
            None
        );
        assert_eq!(
            nurbs_carrier_offset(
                &cadmpeg_test_support::service_decode_context(),
                &surface,
                &[[0.0, 0.0]],
                &[Point3::new(tiny, 0.0, tiny)],
            )
            .expect("evaluator allocation succeeds"),
            None
        );
        for invalid in [f64::NAN, f64::INFINITY] {
            assert_eq!(
                nurbs_carrier_offset(
                    &cadmpeg_test_support::service_decode_context(),
                    &surface,
                    &[[0.25, 0.25], [0.75, 0.75]],
                    &[
                        Point3::new(0.25, 0.25, tiny),
                        Point3::new(0.75, 0.75, invalid),
                    ],
                )
                .expect("evaluator allocation succeeds"),
                None
            );
        }
    }

    #[test]
    fn circle_binding_is_relative_to_the_arc_length_span() {
        let span = 1e-200_f64;
        let circle = B2Circle {
            pos: 0,
            layout: crate::native::CatiaCircleLayout::PackedSix,
            record_id: 1,
            frame_token: 0,
            center_pair: crate::test_support::test_b5::finite_vector([0.0; 2]),
            radius: cadmpeg_ir::scalar::PositiveLength::new(span).expect("positive span"),
            range: cadmpeg_ir::topology::IncreasingParameterInterval::new([0.0, span])
                .expect("increasing span"),
            chart_shift: cadmpeg_ir::scalar::FiniteReal::ZERO,
        };
        let pcurve = |points: Vec<[f64; 2]>| ConsolidatedPcurve {
            pos: 0,
            support_id: 1,
            extrapolation_sites: 0,
            sites: points
                .into_iter()
                .enumerate()
                .map(
                    |(index, point)| crate::wire::records::ConsolidatedPcurveSite {
                        knot: crate::test_support::test_b5::finite(if index == 0 {
                            0.0
                        } else {
                            span
                        }),
                        point: crate::test_support::test_b5::finite_vector(point),
                        first_derivatives: crate::test_support::test_b5::finite_vector([0.0, 0.0]),
                        second_derivatives: crate::test_support::test_b5::finite_vector([0.0, 0.0]),
                    },
                )
                .collect::<Vec<_>>()
                .try_into()
                .expect("ordered fixture sites"),
            range: crate::test_support::test_b5::increasing([0.0, span]),
            tail: Vec::new(),
        };

        assert!(pcurve_matches_circle(
            &pcurve(vec![[0.0, span], [span, span]]),
            &circle
        ));
        assert!(!pcurve_matches_circle(
            &pcurve(vec![[0.0, span], [span, 2.0 * span]]),
            &circle
        ));
        assert!(!pcurve_matches_circle(
            &pcurve(vec![[0.0, span], [2.0 * span, span]]),
            &circle
        ));
    }
    #[test]
    fn support_loci_with_an_overflowing_site_do_not_agree() {
        // The plane's origin is the largest finite x coordinate: the site
        // u = MAX lifts to a point without a finite x, and u = -MAX lifts to
        // the model origin.
        let plane = crate::families::b2::records::B2PlaneCarrier {
            pos: 7,
            end: 0,
            width: crate::wire::records::ConsolidatedFrameWidth::One,
            flag: crate::wire::records::ConsolidatedFrameFlag::Flag03,
            header_token: 0,
            payload: crate::families::b2::records::B2PlaneCarrierPayload::PointDirection2 {
                origin: cadmpeg_ir::features::FinitePoint3::new(Point3::new(f64::MAX, 0.0, 0.0))
                    .expect("finite origin"),
                frame: cadmpeg_ir::units::OrthonormalFrame3::new(
                    cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                    cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("orthonormal frame"),
                tail: cadmpeg_ir::units::FiniteVector::new([0.0; 3]).expect("finite tail"),
            },
        };
        let pcurve = |points: [[f64; 2]; 2]| ConsolidatedPcurve {
            pos: 0,
            support_id: 1,
            extrapolation_sites: 0,
            sites: points
                .into_iter()
                .enumerate()
                .map(
                    |(index, point)| crate::wire::records::ConsolidatedPcurveSite {
                        knot: crate::test_support::test_b5::finite(
                            cadmpeg_core::convert::f64_from_index(index)
                                .expect("fixture index is exactly representable"),
                        ),
                        point: crate::test_support::test_b5::finite_vector(point),
                        first_derivatives: crate::test_support::test_b5::finite_vector([0.0, 0.0]),
                        second_derivatives: crate::test_support::test_b5::finite_vector([0.0, 0.0]),
                    },
                )
                .collect::<Vec<_>>()
                .try_into()
                .expect("ordered fixture sites"),
            range: crate::test_support::test_b5::increasing([0.0, 1.0]),
            tail: Vec::new(),
        };
        let block = super::ConsolidatedEdgeBlock {
            pcurves: [
                pcurve([[f64::MAX, 0.0], [-f64::MAX, 0.0]]),
                pcurve([[-f64::MAX, 0.0], [-f64::MAX, 0.0]]),
            ],
            parameters: crate::families::b2::records::B2EdgeParameters {
                pos: 0,
                range: crate::test_support::test_b5::increasing([0.0, 1.0]),
                tolerance: cadmpeg_ir::scalar::FiniteReal::ZERO,
            },
        };
        let planes = [plane];
        let carriers = super::ConsolidatedCarriers {
            cylinders: &[],
            embedded_cylinders: &[],
            circles: &[],
            cones: &[],
            spheres: &[],
            tori: &[],
            planes: &planes,
            nurbs_surfaces: &[],
        };
        let binding = || Some(super::ConsolidatedSupportBinding::Plane { pos: 7 });
        assert_eq!(
            crate::test_support::with_service_context(|ctx| super::resolved_support_loci(
                ctx,
                &block,
                &[None, binding()],
                &carriers
            ))
            .expect("service decode"),
            Some(vec![Point3::new(0.0, 0.0, 0.0); 2])
        );
        assert_eq!(
            crate::test_support::with_service_context(|ctx| super::resolved_support_loci(
                ctx,
                &block,
                &[binding(), binding()],
                &carriers
            ))
            .expect("service decode"),
            None
        );
    }

    #[test]
    fn resolved_cylinder_winner_and_loci_refuse_collection_limits() {
        let pcurve_bytes = crate::test_support::test_a5a8::a5_pcurve_stream();
        let pcurve_records = crate::wire::records::consolidated_records(&pcurve_bytes);
        let pcurves = crate::test_support::with_service_context(|ctx| {
            crate::wire::records::family_pcurves_from_records(
                ctx,
                &pcurve_bytes,
                &pcurve_records,
                crate::wire::records::ConsolidatedFamily::A,
            )
        })
        .expect("service decode");
        let pcurve = &pcurves[0];
        let cylinder_bytes = crate::test_support::test_b2::b2_cylinder_stream();
        let cylinder_records = crate::wire::records::consolidated_records(&cylinder_bytes);
        let cylinders = crate::test_support::with_service_context(|ctx| {
            ctx.try_collect_vec(
                crate::families::b2::records::b2_cylinders_from_records(
                    ctx,
                    &cylinder_bytes,
                    &cylinder_records,
                )?,
                "catia_test_resolved_cylinders",
            )
        })
        .expect("service context admits cylinders");
        let carriers = super::ConsolidatedCarriers {
            cylinders: &cylinders,
            embedded_cylinders: &[],
            circles: &[],
            cones: &[],
            spheres: &[],
            tori: &[],
            planes: &[],
            nurbs_surfaces: &[],
        };
        let cylinder = &cylinders[0];
        let points: Vec<_> = pcurve
            .sites
            .iter()
            .map(|site| {
                let point = super::b2_cylinder_point(cylinder, site.point.get())
                    .expect("finite cylinder site");
                cadmpeg_ir::features::FinitePoint3::new(point).expect("finite point")
            })
            .collect();
        let limited = crate::test_support::with_collection_limit(0, |ctx| {
            super::resolve_side_support(ctx, pcurve, &points, &carriers)
        });
        assert!(
            matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(error))
            if error.operation == "catia_resolved_side_winners")
        );
        let binding = crate::test_support::with_service_context(|ctx| {
            super::resolve_side_support(ctx, pcurve, &points, &carriers)
        })
        .expect("service decode")
        .expect("unique cylinder");
        let limited = crate::test_support::with_collection_limit(1, |ctx| {
            super::support_points(ctx, &binding, pcurve, &carriers)
        });
        assert!(
            matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(error))
            if error.operation == "catia_resolved_support_points")
        );
        let lifted = crate::test_support::with_service_context(|ctx| {
            super::support_points(ctx, &binding, pcurve, &carriers)
        })
        .expect("service decode")
        .expect("lifted cylinder sites");
        assert_eq!(
            lifted,
            points.iter().map(|point| point.get()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn class25_wire_rejects_unknown_marker_and_wrong_arity() {
        for (marker, count) in [(0x99, 0), (0x82, 0), (0x83, 7), (0x89, 19), (0x8b, 25)] {
            let wire = serde_json::json!({
                "kind": "segmented_scalar25", "operands": [1, 2, 3],
                "persistent_lead": null, "leading": [0.0, 0.0, 0.0, 0.0, 0.0],
                "marker": marker, "trailing": vec![0.0; count]
            });
            assert!(serde_json::from_value::<ConsolidatedEdgeDefinitionData>(wire).is_err());
        }
    }
    #[test]
    fn class25_wire_preserves_marker_tail_and_lead() {
        for (marker, count) in [
            (0x82, 5),
            (0x82, 6),
            (0x82, 7),
            (0x83, 8),
            (0x83, 9),
            (0x89, 20),
            (0x8b, 24),
        ] {
            for lead in [None, Some(10), Some(11)] {
                let wire = serde_json::json!({
                    "kind": "segmented_scalar25", "operands": [1, 2, 3],
                    "persistent_lead": lead, "leading": [0.0, 0.0, 0.0, 0.0, 0.0],
                    "marker": marker, "trailing": vec![0.0; count]
                });
                let record: ConsolidatedEdgeDefinitionData = serde_json::from_value(wire.clone())
                    .expect("admitted class-25 marker and tail");
                assert_eq!(
                    serde_json::to_value(record).expect("serialize class-25 record"),
                    wire
                );
            }
        }
        let wire = serde_json::json!({"kind": "scalar25", "operands": [1, 2, 3], "persistent_lead": 12, "values": []});
        assert!(
            serde_json::from_value::<ConsolidatedEdgeDefinitionData>(wire)
                .expect_err("unknown persistent lead must fail admission")
                .to_string()
                .contains("persistent_lead")
        );
    }
}

#[cfg(test)]
mod decoder_tests;
