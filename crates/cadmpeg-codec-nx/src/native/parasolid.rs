// SPDX-License-Identifier: Apache-2.0
//! Parasolid source-record extractors and their record types.

#[allow(clippy::wildcard_imports)]
use super::*;

use super::substrate::StreamView;

/// Shared skeleton for Parasolid record families read from the cached per-stream
/// record view. It owns the stream loop, the `nx:s{ordinal}:{ID_STEM}#{xmt}`
/// identity, and the sort by identity; each family supplies only its cached row
/// slice and its record constructor.
pub(crate) trait ParasolidStreamRecords {
    /// Cached row type read from the stream's record [`StreamView`].
    type Row: Copy;
    /// Emitted native record type.
    type Record;
    /// Identity stem between the `nx:s{ordinal}:` prefix and the `#{xmt}` suffix.
    const ID_STEM: &'static str;
    /// The cached rows of one stream's record view.
    fn rows(view: &StreamView) -> &[Self::Row];
    /// Cross-reference index carried into the record identity.
    fn xmt(row: &Self::Row) -> u32;
    /// Build one record from its identity, stream ordinal, and cached row.
    fn record(id: String, stream_ordinal: u32, row: &Self::Row) -> Self::Record;
    /// The identity of a built record, used as the sort key.
    fn id(record: &Self::Record) -> &str;
}

/// Run the cached-view record skeleton for one family: map every cached row of
/// every stream to a record, then sort by identity. Non-Parasolid streams hold
/// empty views, so no per-stream guard is needed.
pub(crate) fn per_parasolid_stream<P: ParasolidStreamRecords>(
    parsed: &ParsedStreams,
) -> Vec<P::Record> {
    let mut records = Vec::new();
    for (stream_ordinal, stream) in parsed.iter() {
        for row in P::rows(stream.view_for_records()) {
            let id = format!("nx:s{stream_ordinal}:{}#{}", P::ID_STEM, P::xmt(row));
            records.push(P::record(id, stream_ordinal as u32, row));
        }
    }
    records.sort_by(|left, right| P::id(left).cmp(P::id(right)));
    records
}

/// Shared skeleton for Parasolid record families scanned fresh from each
/// Parasolid stream's inflated bytes. It owns the `is_parasolid()` guard, the
/// stream loop, the `nx:s{ordinal}:{ID_STEM}#{xmt}` identity, and the sort; each
/// family supplies only its scanner and its record constructor.
pub(crate) trait ParasolidScanRecords {
    /// Scanned row type produced from the inflated stream bytes.
    type Row;
    /// Emitted native record type.
    type Record;
    /// Identity stem between the `nx:s{ordinal}:` prefix and the `#{xmt}` suffix.
    const ID_STEM: &'static str;
    /// Scan one inflated Parasolid stream into its rows.
    fn scan(bytes: &[u8]) -> Vec<Self::Row>;
    /// Cross-reference index carried into the record identity.
    fn xmt(row: &Self::Row) -> u32;
    /// Build one record from its identity, stream ordinal, and scanned row.
    fn record(id: String, stream_ordinal: u32, row: Self::Row) -> Self::Record;
    /// The identity of a built record, used as the sort key.
    fn id(record: &Self::Record) -> &str;
}

/// Run the fresh-scan record skeleton for one family: scan every Parasolid
/// stream, map each scanned row to a record, then sort by identity.
pub(crate) fn per_parasolid_scan<P: ParasolidScanRecords>(streams: &[Stream]) -> Vec<P::Record> {
    let mut records = Vec::new();
    for (stream_ordinal, stream) in streams.iter().enumerate() {
        if !stream.kind.is_parasolid() {
            continue;
        }
        for row in P::scan(&stream.inflated) {
            let id = format!("nx:s{stream_ordinal}:{}#{}", P::ID_STEM, P::xmt(&row));
            records.push(P::record(id, stream_ordinal as u32, row));
        }
    }
    records.sort_by(|left, right| P::id(left).cmp(P::id(right)));
    records
}

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
    per_parasolid_stream::<ParasolidOffsetSurfaceRecord>(parsed)
}

impl ParasolidStreamRecords for ParasolidOffsetSurfaceRecord {
    type Row = crate::topology::OffsetSurface;
    type Record = ParasolidOffsetSurfaceRecord;
    const ID_STEM: &'static str = "offset-surface-record";
    fn rows(view: &StreamView) -> &[Self::Row] {
        &view.offset_surfaces
    }
    fn xmt(row: &Self::Row) -> u32 {
        row.xmt
    }
    fn record(id: String, stream_ordinal: u32, row: &Self::Row) -> Self::Record {
        ParasolidOffsetSurfaceRecord {
            id,
            stream_ordinal,
            xmt: row.xmt,
            discriminator: row.discriminator,
            true_offset: row.true_offset,
            support_xmt: row.support,
            distance: row.distance,
            inflated_offset: row.pos as u64,
        }
    }
    fn id(record: &Self::Record) -> &str {
        &record.id
    }
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
    per_parasolid_stream::<ParasolidTrimmedCurveRecord>(parsed)
}

impl ParasolidStreamRecords for ParasolidTrimmedCurveRecord {
    type Row = crate::topology::TrimmedCurve;
    type Record = ParasolidTrimmedCurveRecord;
    const ID_STEM: &'static str = "trimmed-curve-record";
    fn rows(view: &StreamView) -> &[Self::Row] {
        &view.trimmed_curves
    }
    fn xmt(row: &Self::Row) -> u32 {
        row.xmt
    }
    fn record(id: String, stream_ordinal: u32, row: &Self::Row) -> Self::Record {
        ParasolidTrimmedCurveRecord {
            id,
            stream_ordinal,
            xmt: row.xmt,
            basis_xmt: row.basis,
            points: row.points,
            parameters: row.parameters,
            inflated_offset: row.pos as u64,
        }
    }
    fn id(record: &Self::Record) -> &str {
        &record.id
    }
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
    per_parasolid_stream::<ParasolidSurfaceCurveRecord>(parsed)
}

impl ParasolidStreamRecords for ParasolidSurfaceCurveRecord {
    type Row = crate::topology::SurfaceCurve;
    type Record = ParasolidSurfaceCurveRecord;
    const ID_STEM: &'static str = "surface-curve-record";
    fn rows(view: &StreamView) -> &[Self::Row] {
        &view.surface_curves
    }
    fn xmt(row: &Self::Row) -> u32 {
        row.xmt
    }
    fn record(id: String, stream_ordinal: u32, row: &Self::Row) -> Self::Record {
        ParasolidSurfaceCurveRecord {
            id,
            stream_ordinal,
            xmt: row.xmt,
            surface_xmt: row.surface,
            pcurve_xmt: row.pcurve,
            original_curve_xmt: row.original,
            tolerance_to_original: row.tolerance,
            inflated_offset: row.pos as u64,
        }
    }
    fn id(record: &Self::Record) -> &str {
        &record.id
    }
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
    per_parasolid_scan::<ParasolidBlendBoundRecord>(streams)
}

impl ParasolidScanRecords for ParasolidBlendBoundRecord {
    type Row = crate::intersection::BlendBound;
    type Record = ParasolidBlendBoundRecord;
    const ID_STEM: &'static str = "blend-bound-record";
    fn scan(bytes: &[u8]) -> Vec<Self::Row> {
        crate::intersection::blend_bounds(bytes)
    }
    fn xmt(row: &Self::Row) -> u32 {
        row.xmt
    }
    fn record(id: String, stream_ordinal: u32, row: Self::Row) -> Self::Record {
        ParasolidBlendBoundRecord {
            id,
            stream_ordinal,
            xmt: row.xmt,
            header_references: row.header_references,
            sense: row.sense,
            boundary_index: row.boundary_index,
            blend_surface_xmt: row.blend_surface,
            escaped: row.escaped,
            inflated_offset: row.pos as u64,
        }
    }
    fn id(record: &Self::Record) -> &str {
        &record.id
    }
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
    per_parasolid_scan::<ParasolidTermUseRecord>(streams)
}

impl ParasolidScanRecords for ParasolidTermUseRecord {
    type Row = crate::intersection::TermUse;
    type Record = ParasolidTermUseRecord;
    const ID_STEM: &'static str = "term-use-record";
    fn scan(bytes: &[u8]) -> Vec<Self::Row> {
        crate::intersection::term_use_records(bytes)
    }
    fn xmt(row: &Self::Row) -> u32 {
        row.xmt
    }
    fn record(id: String, stream_ordinal: u32, row: Self::Row) -> Self::Record {
        ParasolidTermUseRecord {
            id,
            stream_ordinal,
            xmt: row.xmt,
            count: row.count,
            form: String::from_utf8_lossy(&row.form).into_owned(),
            point: [row.point.x, row.point.y, row.point.z],
            framing: row.framing,
            inflated_offset: row.pos as u64,
        }
    }
    fn id(record: &Self::Record) -> &str {
        &record.id
    }
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
    per_parasolid_scan::<ParasolidSupportUvRecord>(streams)
}

impl ParasolidScanRecords for ParasolidSupportUvRecord {
    type Row = crate::intersection::SupportUvRecord;
    type Record = ParasolidSupportUvRecord;
    const ID_STEM: &'static str = "support-uv-record";
    fn scan(bytes: &[u8]) -> Vec<Self::Row> {
        crate::intersection::support_uv_records(bytes)
    }
    fn xmt(row: &Self::Row) -> u32 {
        row.xmt
    }
    fn record(id: String, stream_ordinal: u32, row: Self::Row) -> Self::Record {
        ParasolidSupportUvRecord {
            id,
            stream_ordinal,
            xmt: row.xmt,
            count: row.count,
            marker: row.marker,
            values: row.values,
            framing: row.framing,
            inflated_offset: row.pos as u64,
        }
    }
    fn id(record: &Self::Record) -> &str {
        &record.id
    }
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
    per_parasolid_stream::<ParasolidIntersectionRecord>(parsed)
}

impl ParasolidStreamRecords for ParasolidIntersectionRecord {
    type Row = crate::topology::CompositeCurve;
    type Record = ParasolidIntersectionRecord;
    const ID_STEM: &'static str = "intersection-record";
    fn rows(view: &StreamView) -> &[Self::Row] {
        &view.intersections.constructions
    }
    fn xmt(row: &Self::Row) -> u32 {
        row.xmt
    }
    fn record(id: String, stream_ordinal: u32, row: &Self::Row) -> Self::Record {
        ParasolidIntersectionRecord {
            id,
            stream_ordinal,
            xmt: row.xmt,
            header_references: row.header_references,
            sense: row.sense,
            construction_references: row.references,
            delta_twin: row.delta_twin,
            inflated_offset: row.pos as u64,
        }
    }
    fn id(record: &Self::Record) -> &str {
        &record.id
    }
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
    per_parasolid_stream::<ParasolidBlendSurfaceRecord>(parsed)
}

impl ParasolidStreamRecords for ParasolidBlendSurfaceRecord {
    type Row = crate::topology::BlendSurface;
    type Record = ParasolidBlendSurfaceRecord;
    const ID_STEM: &'static str = "blend-surface-record";
    fn rows(view: &StreamView) -> &[Self::Row] {
        &view.blend_surfaces
    }
    fn xmt(row: &Self::Row) -> u32 {
        row.xmt
    }
    fn record(id: String, stream_ordinal: u32, row: &Self::Row) -> Self::Record {
        ParasolidBlendSurfaceRecord {
            id,
            stream_ordinal,
            xmt: row.xmt,
            support_xmts: row.supports,
            spine_xmt: row.spine,
            offsets: row.offsets,
            thumb_weights: row.thumb_weights,
            inflated_offset: row.pos as u64,
        }
    }
    fn id(record: &Self::Record) -> &str {
        &record.id
    }
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

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use std::io::{Cursor, Write};

    use flate2::write::ZlibEncoder;
    use flate2::Compression;

    use cadmpeg_ir::codec::{Codec, CodecEntry, Confidence, DecodeOptions};
    use cadmpeg_ir::geometry::{
        BlendCrossSection, BlendRadiusLaw, CurveGeometry, PcurveGeometry,
        ProceduralCurveDefinition, ProceduralSurfaceDefinition, SurfaceGeometry,
    };
    use cadmpeg_ir::math::{Point2, Vector3};
    use cadmpeg_ir::report::LossCategory;
    use cadmpeg_ir::Exactness;

    use crate::container;
    use crate::parasolid::{self, StreamKind};
    use crate::test_support::*;
    use crate::NxCodec;

    use super::*;

    #[test]
    fn topology_retains_entity_attribute_list_references() {
        let mut stream = topology_partition_stream();
        for (kind, attribute) in [(14, 41), (15, 42), (17, 43), (16, 44), (18, 45)] {
            let at = stream
                .windows(2)
                .position(|window| window == [0, kind])
                .expect("topology record");
            put_ref(&mut stream, at + if kind == 17 { 4 } else { 8 }, attribute);
        }
        stream.extend_from_slice(&[0, 0x51]);
        stream.extend_from_slice(&1u32.to_be_bytes());
        stream.extend_from_slice(&41u16.to_be_bytes());
        stream.extend_from_slice(&1u32.to_be_bytes());
        stream.extend_from_slice(&0x21u16.to_be_bytes());
        for reference in [4u16, 1, 1, 1, 1, 42] {
            stream.extend_from_slice(&reference.to_be_bytes());
        }
        stream.extend_from_slice(&[0, 0x54]);
        stream.extend_from_slice(&8u32.to_be_bytes());
        stream.extend_from_slice(&42u16.to_be_bytes());
        stream.extend_from_slice(b"deadbeef\0");

        let graph = crate::topology::Graph::parse(&stream);
        assert_eq!(
            graph.get(14, 4).unwrap().face_fields().unwrap().attributes,
            41
        );
        assert_eq!(
            graph.get(15, 5).unwrap().loop_fields().unwrap().attributes,
            42
        );
        assert_eq!(
            graph.get(17, 7).unwrap().fin_fields().unwrap().attributes,
            43
        );
        assert_eq!(
            graph.get(16, 8).unwrap().edge_fields().unwrap().attributes,
            44
        );
        assert_eq!(
            graph
                .get(18, 10)
                .unwrap()
                .vertex_fields()
                .unwrap()
                .attributes,
            45
        );

        let result = NxCodec
            .decode(
                &mut Cursor::new(prt_with_partition(&stream)),
                &DecodeOptions::default(),
            )
            .unwrap();
        let references = result
            .ir
            .native
            .namespace("nx")
            .unwrap()
            .arena_as::<super::ParasolidTopologyAttributeListReference>(
                "parasolid_topology_attribute_list_references",
            )
            .unwrap();
        assert_eq!(references.len(), 5);
        assert_eq!(references[0].topology_type, 14);
        assert_eq!(references[0].topology_xmt, 4);
        assert_eq!(references[0].attribute_list_xmt, 41);
        assert!(references[0].attribute_list_record.is_some());
        assert_eq!(result.ir.model.attributes.len(), 1);
        assert_eq!(
            result.ir.model.attributes[0].target,
            cadmpeg_ir::attributes::AttributeTarget::Face(cadmpeg_ir::ids::FaceId(
                "nx:s0:face#4".into()
            ))
        );
        assert_eq!(
            result.ir.model.attributes[0].name,
            "parasolid_type_84_reference_5"
        );
        assert_eq!(
            result.ir.model.attributes[0].values,
            [cadmpeg_ir::attributes::AttributeValue::String(
                "deadbeef".into()
            )]
        );
    }

    #[test]
    fn topology_attribute_class_uses_resolve_instance_discriminators_by_xmt() {
        use super::{
            ParasolidAttributeDefinition, ParasolidEntity51Record,
            ParasolidTopologyAttributeListReference,
        };

        let definition = ParasolidAttributeDefinition {
            id: "definition".into(),
            stream_ordinal: 3,
            xmt: 34,
            name: "UG2/PMARK_ATTRIBUTE".into(),
            field_count: 1,
            field_record_xmt: 19,
            field_record_references: [21, 22],
            field_record_header_words: [0, 9000],
            field_descriptor_prefix: [0; 26],
            field_storage: None,
            field_codes: vec![1],
            inflated_offset: 100,
        };
        let entity = ParasolidEntity51Record {
            id: "entity".into(),
            stream_ordinal: 3,
            xmt: 50,
            flags: 1,
            sequence: 7,
            discriminator: 0x21,
            references: vec![60, 61, 1, 62, 63, 64],
            byte_len: 26,
            inflated_offset: 200,
        };
        let reference = ParasolidTopologyAttributeListReference {
            id: "topology-reference".into(),
            stream_ordinal: 3,
            topology_type: 14,
            topology_xmt: 60,
            attribute_list_xmt: 50,
            attribute_list_record: Some(entity.id.clone()),
            inflated_offset: 300,
        };

        let instance_uses = super::parasolid_attribute_class_uses(
            std::slice::from_ref(&entity),
            std::slice::from_ref(&definition),
        );
        assert_eq!(instance_uses.len(), 1);
        assert_eq!(instance_uses[0].entity_51_record, entity.id);
        assert_eq!(instance_uses[0].class_discriminator, 0x21);
        assert_eq!(instance_uses[0].definition_xmt, 34);
        assert_eq!(instance_uses[0].attribute_definition, definition.id);

        let uses = super::parasolid_topology_attribute_class_uses(
            std::slice::from_ref(&reference),
            &instance_uses,
        );
        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].class_discriminator, 0x21);
        assert_eq!(uses[0].definition_xmt, 34);
        assert_eq!(uses[0].attribute_definition, definition.id);

        let mut invalid = entity;
        invalid.discriminator = 0x20;
        assert!(super::parasolid_attribute_class_uses(
            std::slice::from_ref(&invalid),
            std::slice::from_ref(&definition),
        )
        .is_empty());
        assert!(super::parasolid_topology_attribute_class_uses(
            &[reference],
            &super::parasolid_attribute_class_uses(&[invalid], &[definition]),
        )
        .is_empty());
    }

    #[test]
    fn parasolid_attribute_definition_requires_declared_printable_name_and_field_record() {
        let mut bytes = vec![0xaa, 0x00, 0x4f, 0xff];
        bytes.extend_from_slice(&16u32.to_be_bytes());
        bytes.extend_from_slice(&0x012au16.to_be_bytes());
        bytes.extend_from_slice(b"SDL/TYSA_DENSITY");
        bytes.extend_from_slice(&[0x00, 0x50, 0x00, 0x00, 0x00, 0x01]);
        bytes.extend_from_slice(&0x012bu16.to_be_bytes());
        bytes.extend_from_slice(&0x0030u16.to_be_bytes());
        bytes.extend_from_slice(&0x0031u16.to_be_bytes());
        bytes.extend_from_slice(&[0x00, 0x00, 0x23, 0x28]);
        let descriptor = [
            0x00, 0x00, 0x00, 0x00, 0x03, 0x06, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
            0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00,
        ];
        bytes.extend_from_slice(&descriptor);
        bytes.push(1);
        let definitions = crate::parasolid::attribute_definitions(&bytes);
        assert_eq!(definitions.len(), 1);
        assert_eq!(definitions[0].offset, 1);
        assert_eq!(definitions[0].xmt, 0x12a);
        assert_eq!(definitions[0].name, "SDL/TYSA_DENSITY");
        assert_eq!(definitions[0].field_count, 1);
        assert_eq!(definitions[0].field_record_xmt, 0x12b);
        assert_eq!(definitions[0].field_record_references, [0x30, 0x31]);
        assert_eq!(definitions[0].field_record_header_words, [0, 0x2328]);
        assert_eq!(definitions[0].field_descriptor_prefix, descriptor);
        assert_eq!(
            super::parasolid_attribute_field_storage(&definitions[0].field_descriptor_prefix),
            Some(super::ParasolidAttributeFieldStorage::Double)
        );
        assert_eq!(definitions[0].field_codes, [1]);

        let truncated = &bytes[..bytes.len() - 1];
        assert!(crate::parasolid::attribute_definitions(truncated).is_empty());

        bytes[20] = 0;
        assert!(crate::parasolid::attribute_definitions(&bytes).is_empty());
    }

    #[test]
    fn decode_emits_offset_surface_construction() {
        let stream = offset_surface_topology_partition_stream();
        let mut cur = Cursor::new(prt_with_partition(&stream));
        let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

        let procedural = result
            .ir
            .model
            .procedural_surfaces
            .first()
            .expect("offset surface");
        let ProceduralSurfaceDefinition::Offset {
            support,
            distance,
            u_sense,
            v_sense,
            extension_flags,
        } = &procedural.definition
        else {
            panic!("offset definition");
        };
        assert_eq!(*distance, 2.5);
        assert_eq!(*u_sense, Some(0));
        assert_eq!(*v_sense, Some(0));
        assert!(extension_flags.is_empty());
        assert_ne!(procedural.surface, *support);
        assert_eq!(result.ir.model.faces[0].surface, procedural.surface);
        let records = result
            .ir
            .native
            .namespace("nx")
            .unwrap()
            .arena_as::<super::ParasolidOffsetSurfaceRecord>("parasolid_offset_surface_records")
            .unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].discriminator, 'V');
        assert!(records[0].true_offset);
        assert_eq!(records[0].support_xmt, 6);
        assert_eq!(records[0].distance, 2.5);
        let carrier = result
            .ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == procedural.surface)
            .expect("offset carrier");
        assert_eq!(
            carrier
                .source_object
                .as_ref()
                .map(|source| &source.object_id),
            Some(&records[0].id)
        );
        assert!(matches!(
            &carrier.geometry,
            SurfaceGeometry::Procedural { construction } if construction == &procedural.id
        ));
        assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
    }

    #[test]
    fn decode_resolves_surface_curve_to_its_basis_curve() {
        let stream = surface_curve_topology_partition_stream();
        let mut cur = Cursor::new(prt_with_partition(&stream));
        let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

        assert_eq!(result.ir.model.edges.len(), 1);
        let records = result
            .ir
            .native
            .namespace("nx")
            .unwrap()
            .arena_as::<super::ParasolidSurfaceCurveRecord>("parasolid_surface_curve_records")
            .unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].surface_xmt, 6);
        assert_eq!(records[0].pcurve_xmt, 9);
        assert_eq!(records[0].original_curve_xmt, 9);
        assert_eq!(records[0].tolerance_to_original, 0.000_01);
        assert_eq!(
            result.ir.model.edges[0].curve.as_ref(),
            Some(&result.ir.model.curves[0].id)
        );
        assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
    }

    #[test]
    fn decode_emits_rolling_ball_blend_surface() {
        let stream = blend_surface_topology_partition_stream();
        let mut cur = Cursor::new(prt_with_partition(&stream));
        let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

        let procedural = result
            .ir
            .model
            .procedural_surfaces
            .first()
            .expect("blend surface");
        let ProceduralSurfaceDefinition::Blend {
            supports,
            radius,
            cross_section,
            spine,
            native,
        } = &procedural.definition
        else {
            panic!("blend definition");
        };
        assert_eq!(*cross_section, BlendCrossSection::Circular);
        assert_eq!(
            *radius,
            BlendRadiusLaw::Constant {
                signed_radius: -3.0
            }
        );
        assert_eq!(supports[0].as_ref().map(|side| side.reversed), Some(true));
        assert_eq!(supports[1].as_ref().map(|side| side.reversed), Some(false));
        assert!(spine.is_none());
        assert!(native.is_none());
        assert_eq!(result.ir.model.faces[0].surface, procedural.surface);
        let records = result
            .ir
            .native
            .namespace("nx")
            .unwrap()
            .arena_as::<super::ParasolidBlendSurfaceRecord>("parasolid_blend_surface_records")
            .unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].support_xmts, [6, 6]);
        assert_eq!(records[0].spine_xmt, 1);
        assert_eq!(records[0].offsets, [-3.0, 3.0]);
        assert_eq!(records[0].thumb_weights, [1.0, 1.0]);
        let carrier = result
            .ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == procedural.surface)
            .unwrap();
        assert_eq!(
            carrier
                .source_object
                .as_ref()
                .map(|association| association.object_id.as_str()),
            Some(records[0].id.as_str())
        );
        assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
    }

    #[test]
    fn decode_preserves_intersection_curve_as_connected_carrier() {
        let stream = intersection_curve_topology_partition_stream();
        let mut cur = Cursor::new(prt_with_partition(&stream));
        let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

        let edge_curve = result.ir.model.edges[0].curve.as_ref().expect("edge curve");
        let curve = result
            .ir
            .model
            .curves
            .iter()
            .find(|curve| &curve.id == edge_curve)
            .expect("intersection carrier");
        assert!(matches!(curve.geometry, CurveGeometry::Unknown { .. }));
        let records = result
            .ir
            .native
            .namespace("nx")
            .unwrap()
            .arena_as::<super::ParasolidIntersectionRecord>("parasolid_intersection_records")
            .unwrap();
        assert_eq!(records.len(), 1);
        assert!(!records[0].delta_twin);
        assert_eq!(records[0].header_references[0], 1);
        assert_eq!(records[0].construction_references, [6, 6, 1, 1, 1, 1]);
        assert_eq!(
            curve.source_object.as_ref().map(|source| &source.object_id),
            Some(&records[0].id)
        );
        assert_eq!(result.ir.model.procedural_curves.len(), 1);
        assert_eq!(result.ir.model.procedural_curves[0].curve, curve.id);
        assert!(result.report.losses.iter().any(|loss| {
            loss.category == LossCategory::Geometry
                && loss.message.starts_with("1 surface-intersection record(s)")
        }));
        assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
    }

    #[test]
    fn decode_preserves_deltas_intersection_data_curve() {
        let stream = deltas_intersection_curve_stream();
        let mut cur = Cursor::new(prt_with_partition(&stream));
        let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

        assert_eq!(result.ir.model.procedural_curves.len(), 1);
        let records = result
            .ir
            .native
            .namespace("nx")
            .unwrap()
            .arena_as::<super::ParasolidIntersectionRecord>("parasolid_intersection_records")
            .unwrap();
        assert_eq!(records.len(), 1);
        assert!(records[0].delta_twin);
        assert_eq!(records[0].header_references[0], 1);
        assert_eq!(records[0].construction_references, [6, 6, 1, 1, 1, 1]);
        assert_eq!(
            result.ir.model.edges[0].curve.as_ref(),
            Some(&result.ir.model.procedural_curves[0].curve)
        );
        assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
    }

    #[test]
    fn decode_emits_charted_surface_intersection_construction() {
        let stream = charted_intersection_curve_topology_partition_stream();
        let mut cur = Cursor::new(prt_with_partition(&stream));
        let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

        let terms = result
            .ir
            .native
            .namespace("nx")
            .unwrap()
            .arena_as::<super::ParasolidTermUseRecord>("parasolid_term_use_records")
            .unwrap();
        assert_eq!(terms.len(), 2);
        assert_eq!(terms[0].count, 1);
        assert_eq!(terms[0].form, "L?");
        assert_eq!(terms[0].point, [0.0, 0.0, 0.0]);
        assert_eq!(terms[1].point, [10.0, 0.0, 0.0]);
        assert!(terms
            .iter()
            .all(|term| matches!(term.framing, crate::intersection::TermUseFraming::Direct)));
        let support_uv = result
            .ir
            .native
            .namespace("nx")
            .unwrap()
            .arena_as::<super::ParasolidSupportUvRecord>("parasolid_support_uv_records")
            .unwrap();
        assert_eq!(support_uv.len(), 1);
        assert_eq!(support_uv[0].count, 4);
        assert_eq!(support_uv[0].marker, 2);
        assert_eq!(support_uv[0].values, [0.0, 0.0, 0.01, 0.0]);
        assert!(matches!(
            support_uv[0].framing,
            crate::intersection::SupportUvFraming::Direct
        ));
        let charts = result
            .ir
            .native
            .namespace("nx")
            .unwrap()
            .arena_as::<super::ParasolidChartRecord>("parasolid_chart_records")
            .unwrap();
        assert_eq!(charts.len(), 1);
        assert_eq!(charts[0].count, 2);
        assert_eq!(charts[0].base_parameter, 0.0);
        assert_eq!(charts[0].base_scale, 1.0);
        assert_eq!(charts[0].chart_count, 2);
        assert_eq!(charts[0].chordal_error, 0.000_01);
        assert_eq!(charts[0].angular_error, 0.001);
        assert_eq!(charts[0].points, [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]]);
        assert!(matches!(
            charts[0].point_layout,
            crate::intersection::ChartPointLayout::Xyz3
        ));

        let procedural = result
            .ir
            .model
            .procedural_curves
            .first()
            .expect("intersection construction");
        let curve = result
            .ir
            .model
            .curves
            .iter()
            .find(|curve| curve.id == procedural.curve)
            .expect("solved chart cache");
        let CurveGeometry::Nurbs(nurbs) = &curve.geometry else {
            panic!("charted NURBS cache");
        };
        assert_eq!(nurbs.degree, 1);
        assert_eq!(nurbs.control_points[0].x, 0.0);
        assert_eq!(nurbs.control_points[1].x, 10.0);
        assert_eq!(procedural.cache_fit_tolerance, Some(0.01));
        let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
            &procedural.definition
        else {
            panic!("typed surface intersection");
        };
        assert!(context.sides[0].surface.is_some());
        assert!(context.sides[0].pcurve.is_some());
        assert!(context.sides[1].surface.is_none());
        assert_eq!(context.parameter_range, [0.0, 0.01]);
        assert!(result.ir.model.coedges[0].pcurves.is_empty());
        assert!(!result.report.losses.iter().any(|loss| {
            loss.category == LossCategory::Geometry
                && loss.message.contains("surface-intersection record(s)")
        }));
        let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
        assert!(validation.is_ok(), "findings: {:?}", validation.findings);
    }

    #[test]
    fn decode_resolves_intersection_second_support_through_blend_bound() {
        let stream = blend_bound_charted_intersection_curve_stream();
        let mut cur = Cursor::new(prt_with_partition(&stream));
        let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

        let records = result
            .ir
            .native
            .namespace("nx")
            .unwrap()
            .arena_as::<super::ParasolidBlendBoundRecord>("parasolid_blend_bound_records")
            .unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].header_references, [1; 5]);
        assert!(records[0].sense);
        assert_eq!(records[0].boundary_index, 0);
        assert_eq!(records[0].blend_surface_xmt, 13);
        assert!(!records[0].escaped);

        let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
            &result.ir.model.procedural_curves[0].definition
        else {
            panic!("typed intersection");
        };
        let second = context.sides[1].surface.as_ref().expect("bridged support");
        assert_ne!(context.sides[0].surface.as_ref(), Some(second));
        assert!(context.sides[1].pcurve.is_some());
    }

    #[test]
    fn decode_resolves_trimmed_edge_to_its_basis_curve_and_range() {
        let mut cur = Cursor::new(prt_with_partition(&trimmed_topology_partition_stream()));
        let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
        let edge = result.ir.model.edges.first().expect("edge");
        assert_eq!(edge.curve.as_ref(), Some(&result.ir.model.curves[0].id));
        assert_eq!(edge.param_range, Some([0.25, 0.75]));
        let records = result
            .ir
            .native
            .namespace("nx")
            .unwrap()
            .arena_as::<super::ParasolidTrimmedCurveRecord>("parasolid_trimmed_curve_records")
            .unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].basis_xmt, 9);
        assert_eq!(records[0].points, [[0.0; 3]; 2]);
        assert_eq!(records[0].parameters, [0.000_25, 0.000_75]);
        assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
    }
}
