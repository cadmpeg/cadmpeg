// SPDX-License-Identifier: Apache-2.0
//! Parasolid source-record extractors and their record types.

#[allow(clippy::wildcard_imports)]
use super::*;

/// Complete typed source record for one Parasolid offset surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParasolidOffsetSurfaceRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Cross-reference index of the offset surface.
    pub xmt: u32,
    /// Serialized `V`, `I`, or `U` discriminator.
    pub discriminator: char,
    /// Serialized true-offset flag.
    pub true_offset: bool,
    /// Cross-reference index of the support surface.
    pub support_xmt: u32,
    /// Signed offset distance in millimetres.
    pub distance: f64,
    /// Record tag offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Decode complete typed source records for Parasolid offset surfaces.
pub(crate) fn parasolid_offset_surface_records(
    parsed: &ParsedStreams,
) -> Vec<ParasolidOffsetSurfaceRecord> {
    let mut records = Vec::new();
    for (stream_ordinal, stream) in parsed.iter() {
        for offset in stream.view_for_records().offset_surfaces.iter().copied() {
            records.push(ParasolidOffsetSurfaceRecord {
                id: format!("nx:s{stream_ordinal}:offset-surface-record#{}", offset.xmt),
                stream_ordinal: stream_ordinal as u32,
                xmt: offset.xmt,
                discriminator: offset.discriminator,
                true_offset: offset.true_offset,
                support_xmt: offset.support,
                distance: offset.distance,
                inflated_offset: offset.pos as u64,
            });
        }
    }
    records.sort_by(|left, right| left.id.cmp(&right.id));
    records
}

/// Complete typed source record for one Parasolid trimmed curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParasolidTrimmedCurveRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Cross-reference index of the trimmed curve.
    pub xmt: u32,
    /// Cross-reference index of the basis curve.
    pub basis_xmt: u32,
    /// Stored start and end points in millimetres.
    pub points: [[f64; 3]; 2],
    /// Stored start and end parameters in basis-curve units.
    pub parameters: [f64; 2],
    /// Record tag offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Decode complete typed source records for Parasolid trimmed curves.
pub(crate) fn parasolid_trimmed_curve_records(
    parsed: &ParsedStreams,
) -> Vec<ParasolidTrimmedCurveRecord> {
    let mut records = Vec::new();
    for (stream_ordinal, stream) in parsed.iter() {
        for trim in stream.view_for_records().trimmed_curves.iter().copied() {
            records.push(ParasolidTrimmedCurveRecord {
                id: format!("nx:s{stream_ordinal}:trimmed-curve-record#{}", trim.xmt),
                stream_ordinal: stream_ordinal as u32,
                xmt: trim.xmt,
                basis_xmt: trim.basis,
                points: trim.points,
                parameters: trim.parameters,
                inflated_offset: trim.pos as u64,
            });
        }
    }
    records.sort_by(|left, right| left.id.cmp(&right.id));
    records
}

/// Complete typed source record for one Parasolid surface curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParasolidSurfaceCurveRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Cross-reference index of the surface curve.
    pub xmt: u32,
    /// Cross-reference index of the support surface.
    pub surface_xmt: u32,
    /// Cross-reference index of the parameter-space B-curve.
    pub pcurve_xmt: u32,
    /// Nullable cross-reference index of the original model-space curve.
    pub original_curve_xmt: u32,
    /// Serialized tolerance to the original curve in Parasolid metres.
    pub tolerance_to_original: f64,
    /// Record tag offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Decode complete typed source records for Parasolid surface curves.
pub(crate) fn parasolid_surface_curve_records(
    parsed: &ParsedStreams,
) -> Vec<ParasolidSurfaceCurveRecord> {
    let mut records = Vec::new();
    for (stream_ordinal, stream) in parsed.iter() {
        for curve in stream.view_for_records().surface_curves.iter().copied() {
            records.push(ParasolidSurfaceCurveRecord {
                id: format!("nx:s{stream_ordinal}:surface-curve-record#{}", curve.xmt),
                stream_ordinal: stream_ordinal as u32,
                xmt: curve.xmt,
                surface_xmt: curve.surface,
                pcurve_xmt: curve.pcurve,
                original_curve_xmt: curve.original,
                tolerance_to_original: curve.tolerance,
                inflated_offset: curve.pos as u64,
            });
        }
    }
    records.sort_by(|left, right| left.id.cmp(&right.id));
    records
}

/// Complete typed source record for one Parasolid blend-bound bridge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidBlendBoundRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Cross-reference index of the bridge.
    pub xmt: u32,
    /// Five ordered common-header references.
    pub header_references: [u32; 5],
    /// Serialized orientation sense.
    pub sense: bool,
    /// Zero- or one-valued blend boundary index.
    pub boundary_index: u32,
    /// Cross-reference index of the blend surface.
    pub blend_surface_xmt: u32,
    /// Whether the record tag uses the `0xff` envelope escape.
    pub escaped: bool,
    /// Record tag offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Decode complete typed source records for Parasolid blend-bound bridges.
pub fn parasolid_blend_bound_records(streams: &[Stream]) -> Vec<ParasolidBlendBoundRecord> {
    let mut records = Vec::new();
    for (stream_ordinal, stream) in streams.iter().enumerate() {
        if !stream.kind.is_parasolid() {
            continue;
        }
        for bound in crate::intersection::blend_bounds(&stream.inflated) {
            records.push(ParasolidBlendBoundRecord {
                id: format!("nx:s{stream_ordinal}:blend-bound-record#{}", bound.xmt),
                stream_ordinal: stream_ordinal as u32,
                xmt: bound.xmt,
                header_references: bound.header_references,
                sense: bound.sense,
                boundary_index: bound.boundary_index,
                blend_surface_xmt: bound.blend_surface,
                escaped: bound.escaped,
                inflated_offset: bound.pos as u64,
            });
        }
    }
    records.sort_by(|left, right| left.id.cmp(&right.id));
    records
}

/// Complete typed source record for one Parasolid `term_use` endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParasolidTermUseRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Cross-reference index of the endpoint.
    pub xmt: u32,
    /// Serialized leading count.
    pub count: u32,
    /// Two-byte endpoint-form discriminator as printable ASCII.
    pub form: String,
    /// Endpoint position in millimetres.
    pub point: [f64; 3],
    /// Serialized record framing.
    pub framing: crate::intersection::TermUseFraming,
    /// Tag or inline-payload offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Decode complete typed source records for Parasolid `term_use` endpoints.
pub fn parasolid_term_use_records(streams: &[Stream]) -> Vec<ParasolidTermUseRecord> {
    let mut records = Vec::new();
    for (stream_ordinal, stream) in streams.iter().enumerate() {
        if !stream.kind.is_parasolid() {
            continue;
        }
        for term in crate::intersection::term_use_records(&stream.inflated) {
            records.push(ParasolidTermUseRecord {
                id: format!("nx:s{stream_ordinal}:term-use-record#{}", term.xmt),
                stream_ordinal: stream_ordinal as u32,
                xmt: term.xmt,
                count: term.count,
                form: String::from_utf8_lossy(&term.form).into_owned(),
                point: [term.point.x, term.point.y, term.point.z],
                framing: term.framing,
                inflated_offset: term.pos as u64,
            });
        }
    }
    records.sort_by(|left, right| left.id.cmp(&right.id));
    records
}

/// Complete typed source record for one Parasolid support-UV values array.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParasolidSupportUvRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Cross-reference index of the values array.
    pub xmt: u32,
    /// Serialized scalar count.
    pub count: u32,
    /// Tuple-packing marker (`2`, `3`, or `4`).
    pub marker: u8,
    /// Ordered serialized scalar values.
    pub values: Vec<f64>,
    /// Serialized record framing.
    pub framing: crate::intersection::SupportUvFraming,
    /// Tag or inline-payload offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Decode complete typed source records for Parasolid support-UV arrays.
pub fn parasolid_support_uv_records(streams: &[Stream]) -> Vec<ParasolidSupportUvRecord> {
    let mut records = Vec::new();
    for (stream_ordinal, stream) in streams.iter().enumerate() {
        if !stream.kind.is_parasolid() {
            continue;
        }
        for record in crate::intersection::support_uv_records(&stream.inflated) {
            records.push(ParasolidSupportUvRecord {
                id: format!("nx:s{stream_ordinal}:support-uv-record#{}", record.xmt),
                stream_ordinal: stream_ordinal as u32,
                xmt: record.xmt,
                count: record.count,
                marker: record.marker,
                values: record.values,
                framing: record.framing,
                inflated_offset: record.pos as u64,
            });
        }
    }
    records.sort_by(|left, right| left.id.cmp(&right.id));
    records
}

/// Complete typed source record for one physical Parasolid `CHART_s` record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParasolidChartRecord {
    /// Globally unique physical-record identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Cross-reference index of the chart.
    pub xmt: u32,
    /// Serialized leading point count.
    pub count: u32,
    /// Base chart parameter.
    pub base_parameter: f64,
    /// Chord-to-parameter scale.
    pub base_scale: f64,
    /// Redundant serialized chart count.
    pub chart_count: u32,
    /// Chordal error in Parasolid metres.
    pub chordal_error: f64,
    /// Angular error in radians.
    pub angular_error: f64,
    /// Two serialized missing-parameter sentinels.
    pub parameter_errors: [f64; 2],
    /// Model-space chart points in millimetres.
    pub points: Vec<[f64; 3]>,
    /// Native ext11 parameters, when present.
    pub native_parameters: Option<Vec<f64>>,
    /// Two ordered ext11 support-UV lanes.
    pub ext_support_uv: [Option<Vec<[f64; 2]>>; 2],
    /// Hvec point layout.
    pub point_layout: crate::intersection::ChartPointLayout,
    /// Serialized record framing.
    pub framing: crate::intersection::ChartFraming,
    /// Type-tag offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Decode every complete physical Parasolid chart source record.
pub fn parasolid_chart_records(streams: &[Stream]) -> Vec<ParasolidChartRecord> {
    let mut records = Vec::new();
    for (stream_ordinal, stream) in streams.iter().enumerate() {
        if !stream.kind.is_parasolid() {
            continue;
        }
        for chart in crate::intersection::chart_source_records(&stream.inflated) {
            records.push(ParasolidChartRecord {
                id: format!(
                    "nx:s{stream_ordinal}:chart-record#{}-{}",
                    chart.xmt, chart.pos
                ),
                stream_ordinal: stream_ordinal as u32,
                xmt: chart.xmt,
                count: chart.count,
                base_parameter: chart.base_parameter,
                base_scale: chart.base_scale,
                chart_count: chart.chart_count,
                chordal_error: chart.chordal_error,
                angular_error: chart.angular_error,
                parameter_errors: chart.parameter_errors,
                points: chart
                    .points
                    .into_iter()
                    .map(|point| [point.x, point.y, point.z])
                    .collect(),
                native_parameters: chart.native_parameters,
                ext_support_uv: chart.ext_support_uv,
                point_layout: chart.point_layout,
                framing: chart.framing,
                inflated_offset: chart.pos as u64,
            });
        }
    }
    records.sort_by(|left, right| left.id.cmp(&right.id));
    records
}

/// Complete typed source record for one Parasolid surface-intersection curve.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidIntersectionRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Cross-reference index of the construction.
    pub xmt: u32,
    /// Five ordered common-header references.
    pub header_references: [u32; 5],
    /// Serialized orientation sense.
    pub sense: bool,
    /// Six ordered support and witness references.
    pub construction_references: [u32; 6],
    /// Whether the record uses the single-byte delta-twin tag.
    pub delta_twin: bool,
    /// Record tag offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Decode complete typed source records for retained intersection constructions.
pub(crate) fn parasolid_intersection_records(
    parsed: &ParsedStreams,
) -> Vec<ParasolidIntersectionRecord> {
    let mut records = Vec::new();
    for (stream_ordinal, stream) in parsed.iter() {
        for construction in stream
            .view_for_records()
            .intersections
            .constructions
            .iter()
            .copied()
        {
            records.push(ParasolidIntersectionRecord {
                id: format!(
                    "nx:s{stream_ordinal}:intersection-record#{}",
                    construction.xmt
                ),
                stream_ordinal: stream_ordinal as u32,
                xmt: construction.xmt,
                header_references: construction.header_references,
                sense: construction.sense,
                construction_references: construction.references,
                delta_twin: construction.delta_twin,
                inflated_offset: construction.pos as u64,
            });
        }
    }
    records.sort_by(|left, right| left.id.cmp(&right.id));
    records
}

/// Complete typed type-56 rolling-ball blend-surface record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParasolidBlendSurfaceRecord {
    /// Globally unique native-record identity.
    pub id: String,
    /// Zero-based embedded Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Stream-local `BLEND_SURF` identity.
    pub xmt: u32,
    /// Ordered support-surface identities.
    pub support_xmts: [u32; 2],
    /// Ball-centre spine identity; `1` is the null reference.
    pub spine_xmt: u32,
    /// Signed support offsets in model millimetres.
    pub offsets: [f64; 2],
    /// Dimensionless support thumb weights.
    pub thumb_weights: [f64; 2],
    /// Offset of the type tag in the inflated stream.
    pub inflated_offset: u64,
}

/// Named Parasolid attribute class declared in one inflated body stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidAttributeDefinition {
    /// Globally unique native-record identity.
    pub id: String,
    /// Zero-based embedded stream ordinal.
    pub stream_ordinal: u32,
    /// Stream-local definition record identity.
    pub xmt: u16,
    /// Exact printable attribute class name.
    pub name: String,
    /// Declared number of fields.
    pub field_count: u32,
    /// Stream-local identity of the following field record.
    pub field_record_xmt: u16,
    /// Ordered catalog references in the field-record header.
    pub field_record_references: [u16; 2],
    /// Two field-record header words following the catalog references.
    pub field_record_header_words: [u16; 2],
    /// Exact 26-byte descriptor prefix following the field-record header.
    pub field_descriptor_prefix: [u8; 26],
    /// Typed primary storage declared by the descriptor's `03` atom.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field_storage: Option<ParasolidAttributeFieldStorage>,
    /// One serialized code for every declared field.
    pub field_codes: Vec<u8>,
    /// Offset of the declaration in the inflated stream.
    pub inflated_offset: u64,
}

/// Primary storage alphabet declared by a Parasolid attribute field descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParasolidAttributeFieldStorage {
    /// Void or flag storage.
    Void,
    /// Component/reference or string storage.
    Component,
    /// Binary64 floating-point storage.
    Double,
}

pub(crate) fn parasolid_attribute_field_storage(
    descriptor: &[u8; 26],
) -> Option<ParasolidAttributeFieldStorage> {
    (descriptor[4] == 0x03).then_some(())?;
    match descriptor[5] {
        0x00 => Some(ParasolidAttributeFieldStorage::Void),
        0x05 => Some(ParasolidAttributeFieldStorage::Component),
        0x06 => Some(ParasolidAttributeFieldStorage::Double),
        _ => None,
    }
}

/// Explicit topology-record ownership of one Parasolid attribute list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidTopologyAttributeListReference {
    /// Globally unique reference identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Parasolid topology record type.
    pub topology_type: u8,
    /// Stream-local topology-record identity.
    pub topology_xmt: u32,
    /// Stream-local attribute-list identity.
    pub attribute_list_xmt: u32,
    /// Uniquely resolved type-81 attribute-list record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attribute_list_record: Option<String>,
    /// Offset of the attribute-list field in the inflated stream.
    pub inflated_offset: u64,
}

/// Framed Parasolid type-81 entity/attribute-list record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidEntity51Record {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Stream-local record identity.
    pub xmt: u32,
    /// Exact record flags.
    pub flags: u32,
    /// Serialized sequence value.
    pub sequence: u32,
    /// Layout discriminator.
    pub discriminator: u16,
    /// Ordered stream-local references.
    pub references: Vec<u32>,
    /// Exact framed record length.
    pub byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub inflated_offset: u64,
}

/// Self-framed printable Parasolid type-84 string record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidEntity54StringRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Stream-local record identity.
    pub xmt: u32,
    /// Exact nonempty printable value.
    pub value: String,
    /// Exact framed record length.
    pub byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub inflated_offset: u64,
}

/// Counted Parasolid type-82 unsigned-integer record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidEntity52IntegerRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Stream-local record identity.
    pub xmt: u32,
    /// Ordered big-endian unsigned values.
    pub values: Vec<u32>,
    /// Exact framed record length.
    pub byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub inflated_offset: u64,
}

/// Counted Parasolid type-83 finite binary64 record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParasolidEntity53DoubleRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Stream-local record identity.
    pub xmt: u32,
    /// Ordered finite big-endian binary64 values.
    pub values: Vec<f64>,
    /// Exact framed record length.
    pub byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub inflated_offset: u64,
}

/// Numeric value-record family referenced by a type-81 record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParasolidEntity51NumericKind {
    /// Type-82 unsigned-integer lane.
    UnsignedIntegers,
    /// Type-83 binary64 lane.
    Doubles,
}

/// Exact type-81 reference to one uniquely resolved numeric value record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidEntity51NumericUse {
    /// Globally unique use identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Owning type-81 record.
    pub entity_51_record: String,
    /// Zero-based position in the type-81 reference lane.
    pub reference_ordinal: u32,
    /// Stream-local referenced xmt.
    pub referenced_xmt: u32,
    /// Numeric record family.
    pub kind: ParasolidEntity51NumericKind,
    /// Uniquely resolved numeric record.
    pub value_record: String,
    /// Offset of the owning type-81 record in the inflated stream.
    pub inflated_offset: u64,
}

/// Exact type-81 reference to a uniquely resolved type-84 string record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidEntity51StringUse {
    /// Globally unique use identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Owning type-81 record.
    pub entity_51_record: String,
    /// Zero-based position in the type-81 reference lane.
    pub reference_ordinal: u32,
    /// Stream-local referenced xmt.
    pub referenced_xmt: u32,
    /// Uniquely resolved type-84 string record.
    pub string_record: String,
    /// Offset of the owning type-81 record in the inflated stream.
    pub inflated_offset: u64,
}

/// Resolved registered class of one Parasolid type-81 attribute instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidAttributeClassUse {
    /// Globally unique relation identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Type-81 attribute-instance record.
    pub entity_51_record: String,
    /// Class discriminator serialized by the type-81 instance.
    pub class_discriminator: u16,
    /// Stream-local XMT of the matched type-79 definition.
    pub definition_xmt: u16,
    /// Uniquely matched attribute definition.
    pub attribute_definition: String,
}

/// Resolved class of one topology-owned Parasolid attribute instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidTopologyAttributeClassUse {
    /// Globally unique relation identity.
    pub id: String,
    /// Owning topology-to-attribute relation.
    pub topology_attribute_reference: String,
    /// Topology-owned type-81 attribute-instance record.
    pub entity_51_record: String,
    /// Class discriminator serialized by the type-81 instance.
    pub class_discriminator: u16,
    /// Stream-local XMT of the matched type-79 definition.
    pub definition_xmt: u16,
    /// Uniquely matched attribute definition.
    pub attribute_definition: String,
}

/// Retain named attribute-class declarations from all Parasolid streams.
pub fn parasolid_attribute_definitions(streams: &[Stream]) -> Vec<ParasolidAttributeDefinition> {
    streams
        .iter()
        .enumerate()
        .filter(|(_, stream)| stream.kind.is_parasolid())
        .flat_map(|(stream_ordinal, stream)| {
            crate::parasolid::attribute_definitions(&stream.inflated)
                .into_iter()
                .map(move |definition| ParasolidAttributeDefinition {
                    id: format!(
                        "nx:s{stream_ordinal}:attribute-definition#{}",
                        definition.xmt
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    xmt: definition.xmt,
                    name: definition.name.to_string(),
                    field_count: definition.field_count,
                    field_record_xmt: definition.field_record_xmt,
                    field_record_references: definition.field_record_references,
                    field_record_header_words: definition.field_record_header_words,
                    field_descriptor_prefix: definition.field_descriptor_prefix,
                    field_storage: parasolid_attribute_field_storage(
                        &definition.field_descriptor_prefix,
                    ),
                    field_codes: definition.field_codes.to_vec(),
                    inflated_offset: definition.offset as u64,
                })
        })
        .collect()
}

/// Retain complete typed rolling-ball blend records from all Parasolid streams.
pub(crate) fn parasolid_blend_surface_records(
    parsed: &ParsedStreams,
) -> Vec<ParasolidBlendSurfaceRecord> {
    let mut records = parsed
        .iter()
        .flat_map(|(stream_ordinal, stream)| {
            stream
                .view_for_records()
                .blend_surfaces
                .iter()
                .copied()
                .map(move |blend| ParasolidBlendSurfaceRecord {
                    id: format!("nx:s{stream_ordinal}:blend-surface-record#{}", blend.xmt),
                    stream_ordinal: stream_ordinal as u32,
                    xmt: blend.xmt,
                    support_xmts: blend.supports,
                    spine_xmt: blend.spine,
                    offsets: blend.offsets,
                    thumb_weights: blend.thumb_weights,
                    inflated_offset: blend.pos as u64,
                })
        })
        .collect::<Vec<_>>();
    records.sort_by(|first, second| first.id.cmp(&second.id));
    records
}

/// Retain every non-null topology-to-attribute-list reference.
pub(crate) fn parasolid_topology_attribute_list_references(
    parsed: &ParsedStreams,
    entity_records: &[ParasolidEntity51Record],
) -> Vec<ParasolidTopologyAttributeListReference> {
    let mut records_by_identity = BTreeMap::<(u32, u32), Vec<&str>>::new();
    for record in entity_records {
        records_by_identity
            .entry((record.stream_ordinal, record.xmt))
            .or_default()
            .push(record.id.as_str());
    }
    let mut references = Vec::new();
    for (stream_ordinal, stream) in parsed.iter() {
        let graph = &stream.view_for_records().graph;
        for topology_type in [13, 14, 15, 16, 17, 18] {
            for node in graph.of_kind(topology_type) {
                let attribute_list_xmt = match topology_type {
                    13 => node.shell_fields().map(|fields| fields.attributes),
                    14 => node.face_fields().map(|fields| fields.attributes),
                    15 => node.loop_fields().map(|fields| fields.attributes),
                    16 => node.edge_fields().map(|fields| fields.attributes),
                    17 => node.fin_fields().map(|fields| fields.attributes),
                    18 => node.vertex_fields().map(|fields| fields.attributes),
                    _ => unreachable!("bounded topology family"),
                };
                let Some(attribute_list_xmt) = attribute_list_xmt.filter(|value| *value > 1) else {
                    continue;
                };
                let Some(inflated_offset) = node.attribute_field_offset() else {
                    continue;
                };
                references.push(ParasolidTopologyAttributeListReference {
                    id: format!(
                        "nx:s{stream_ordinal}:topology-attribute-list-reference#{topology_type}-{}",
                        node.xmt
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    topology_type,
                    topology_xmt: node.xmt,
                    attribute_list_xmt,
                    attribute_list_record: records_by_identity
                        .get(&(stream_ordinal as u32, attribute_list_xmt))
                        .and_then(|records| {
                            let [record] = records.as_slice() else {
                                return None;
                            };
                            Some((*record).to_string())
                        }),
                    inflated_offset: inflated_offset as u64,
                });
            }
        }
    }
    references
}

/// Decode every framed type-81 entity/attribute-list record.
pub fn parasolid_entity_51_records(streams: &[Stream]) -> Vec<ParasolidEntity51Record> {
    let mut records = streams
        .iter()
        .enumerate()
        .filter(|(_, stream)| stream.kind.is_parasolid())
        .flat_map(|(stream_ordinal, stream)| {
            crate::parasolid::entity_51_records(&stream.inflated)
                .into_iter()
                .map(move |record| ParasolidEntity51Record {
                    id: format!(
                        "nx:s{stream_ordinal}:entity-51#{}-{}",
                        record.xmt, record.offset
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    xmt: record.xmt,
                    flags: record.flags,
                    sequence: record.sequence,
                    discriminator: record.discriminator,
                    references: record.references,
                    byte_len: record.byte_len as u64,
                    inflated_offset: record.offset as u64,
                })
        })
        .collect::<Vec<_>>();
    records.sort_by(|first, second| first.id.cmp(&second.id));
    records
}

/// Decode every self-framed printable type-84 string record.
pub fn parasolid_entity_54_string_records(
    streams: &[Stream],
) -> Vec<ParasolidEntity54StringRecord> {
    let mut records = streams
        .iter()
        .enumerate()
        .filter(|(_, stream)| stream.kind.is_parasolid())
        .flat_map(|(stream_ordinal, stream)| {
            crate::parasolid::entity_54_string_records(&stream.inflated)
                .into_iter()
                .map(move |record| ParasolidEntity54StringRecord {
                    id: format!(
                        "nx:s{stream_ordinal}:entity-54-string#{}-{}",
                        record.xmt, record.offset
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    xmt: record.xmt,
                    value: record.value.to_string(),
                    byte_len: record.byte_len as u64,
                    inflated_offset: record.offset as u64,
                })
        })
        .collect::<Vec<_>>();
    records.sort_by(|first, second| first.id.cmp(&second.id));
    records
}

/// Decode every counted type-82 unsigned-integer record.
pub fn parasolid_entity_52_integer_records(
    streams: &[Stream],
) -> Vec<ParasolidEntity52IntegerRecord> {
    let mut records = streams
        .iter()
        .enumerate()
        .filter(|(_, stream)| stream.kind.is_parasolid())
        .flat_map(|(stream_ordinal, stream)| {
            crate::parasolid::entity_52_integer_records(&stream.inflated)
                .into_iter()
                .map(move |record| ParasolidEntity52IntegerRecord {
                    id: format!(
                        "nx:s{stream_ordinal}:entity-52-integers#{}-{}",
                        record.xmt, record.offset
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    xmt: record.xmt,
                    values: record.values,
                    byte_len: record.byte_len as u64,
                    inflated_offset: record.offset as u64,
                })
        })
        .collect::<Vec<_>>();
    records.sort_by(|first, second| first.id.cmp(&second.id));
    records
}

/// Decode every counted type-83 finite binary64 record.
pub fn parasolid_entity_53_double_records(
    streams: &[Stream],
) -> Vec<ParasolidEntity53DoubleRecord> {
    let mut records = streams
        .iter()
        .enumerate()
        .filter(|(_, stream)| stream.kind.is_parasolid())
        .flat_map(|(stream_ordinal, stream)| {
            crate::parasolid::entity_53_double_records(&stream.inflated)
                .into_iter()
                .map(move |record| ParasolidEntity53DoubleRecord {
                    id: format!(
                        "nx:s{stream_ordinal}:entity-53-doubles#{}-{}",
                        record.xmt, record.offset
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    xmt: record.xmt,
                    values: record.values,
                    byte_len: record.byte_len as u64,
                    inflated_offset: record.offset as u64,
                })
        })
        .collect::<Vec<_>>();
    records.sort_by(|first, second| first.id.cmp(&second.id));
    records
}

/// Join type-81 reference slots to unique same-stream numeric value records.
pub fn parasolid_entity_51_numeric_uses(
    entities: &[ParasolidEntity51Record],
    integers: &[ParasolidEntity52IntegerRecord],
    doubles: &[ParasolidEntity53DoubleRecord],
) -> Vec<ParasolidEntity51NumericUse> {
    let mut values = BTreeMap::<(u32, u32), Vec<(ParasolidEntity51NumericKind, &str)>>::new();
    for record in integers {
        values
            .entry((record.stream_ordinal, record.xmt))
            .or_default()
            .push((ParasolidEntity51NumericKind::UnsignedIntegers, &record.id));
    }
    for record in doubles {
        values
            .entry((record.stream_ordinal, record.xmt))
            .or_default()
            .push((ParasolidEntity51NumericKind::Doubles, &record.id));
    }
    let mut uses = Vec::new();
    for entity in entities {
        for (reference_ordinal, referenced_xmt) in entity.references.iter().copied().enumerate() {
            let Some([(kind, value_record)]) = values
                .get(&(entity.stream_ordinal, referenced_xmt))
                .map(Vec::as_slice)
            else {
                continue;
            };
            uses.push(ParasolidEntity51NumericUse {
                id: format!(
                    "nx:s{}:entity-51-numeric-use#{}-{}-{reference_ordinal}",
                    entity.stream_ordinal, entity.xmt, entity.inflated_offset
                ),
                stream_ordinal: entity.stream_ordinal,
                entity_51_record: entity.id.clone(),
                reference_ordinal: reference_ordinal as u32,
                referenced_xmt,
                kind: *kind,
                value_record: (*value_record).to_string(),
                inflated_offset: entity.inflated_offset,
            });
        }
    }
    uses.sort_by(|first, second| first.id.cmp(&second.id));
    uses
}

/// Join type-81 reference slots to unique same-stream type-84 strings.
pub fn parasolid_entity_51_string_uses(
    entities: &[ParasolidEntity51Record],
    strings: &[ParasolidEntity54StringRecord],
) -> Vec<ParasolidEntity51StringUse> {
    let mut strings_by_identity = BTreeMap::<(u32, u32), Vec<&str>>::new();
    for string in strings {
        strings_by_identity
            .entry((string.stream_ordinal, string.xmt))
            .or_default()
            .push(string.id.as_str());
    }
    let mut uses = Vec::new();
    for entity in entities {
        for (reference_ordinal, referenced_xmt) in entity.references.iter().copied().enumerate() {
            let Some([string]) = strings_by_identity
                .get(&(entity.stream_ordinal, referenced_xmt))
                .map(Vec::as_slice)
            else {
                continue;
            };
            uses.push(ParasolidEntity51StringUse {
                id: format!(
                    "nx:s{}:entity-51-string-use#{}-{}-{reference_ordinal}",
                    entity.stream_ordinal, entity.xmt, entity.inflated_offset
                ),
                stream_ordinal: entity.stream_ordinal,
                entity_51_record: entity.id.clone(),
                reference_ordinal: reference_ordinal as u32,
                referenced_xmt,
                string_record: (*string).to_string(),
                inflated_offset: entity.inflated_offset,
            });
        }
    }
    uses.sort_by(|first, second| first.id.cmp(&second.id));
    uses
}

/// Resolve topology-owned attribute instances through their class discriminator.
pub fn parasolid_topology_attribute_class_uses(
    topology_references: &[ParasolidTopologyAttributeListReference],
    class_uses: &[ParasolidAttributeClassUse],
) -> Vec<ParasolidTopologyAttributeClassUse> {
    let class_uses = class_uses
        .iter()
        .map(|class_use| (class_use.entity_51_record.as_str(), class_use))
        .collect::<BTreeMap<_, _>>();
    let mut uses = Vec::new();
    for reference in topology_references {
        let Some(entity_id) = reference.attribute_list_record.as_deref() else {
            continue;
        };
        let Some(class_use) = class_uses.get(entity_id) else {
            continue;
        };
        uses.push(ParasolidTopologyAttributeClassUse {
            id: format!(
                "nx:s{}:topology-attribute-class-use#{}-{}",
                reference.stream_ordinal, reference.topology_type, reference.topology_xmt
            ),
            topology_attribute_reference: reference.id.clone(),
            entity_51_record: class_use.entity_51_record.clone(),
            class_discriminator: class_use.class_discriminator,
            definition_xmt: class_use.definition_xmt,
            attribute_definition: class_use.attribute_definition.clone(),
        });
    }
    uses.sort_by(|first, second| first.id.cmp(&second.id));
    uses
}

/// Resolve every type-81 attribute instance through its class discriminator.
pub fn parasolid_attribute_class_uses(
    entities: &[ParasolidEntity51Record],
    definitions: &[ParasolidAttributeDefinition],
) -> Vec<ParasolidAttributeClassUse> {
    let mut definitions_by_identity =
        BTreeMap::<(u32, u16), Vec<&ParasolidAttributeDefinition>>::new();
    for definition in definitions {
        definitions_by_identity
            .entry((definition.stream_ordinal, definition.xmt))
            .or_default()
            .push(definition);
    }
    let mut uses = entities
        .iter()
        .filter_map(|entity| {
            let definition_xmt = entity.discriminator.checked_add(1)?;
            let [definition] = definitions_by_identity
                .get(&(entity.stream_ordinal, definition_xmt))?
                .as_slice()
            else {
                return None;
            };
            Some(ParasolidAttributeClassUse {
                id: format!(
                    "nx:s{}:attribute-class-use#{}-{}",
                    entity.stream_ordinal, entity.xmt, entity.inflated_offset
                ),
                stream_ordinal: entity.stream_ordinal,
                entity_51_record: entity.id.clone(),
                class_discriminator: entity.discriminator,
                definition_xmt,
                attribute_definition: definition.id.clone(),
            })
        })
        .collect::<Vec<_>>();
    uses.sort_by(|first, second| first.id.cmp(&second.id));
    uses
}
