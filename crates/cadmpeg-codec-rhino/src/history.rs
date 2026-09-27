// SPDX-License-Identifier: Apache-2.0
//! Built-in history-record decoding.

use crate::loss::Diagnostics;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::ops::Range;

use crate::chunks::{
    admitted_vec, checked_count_bytes, chunk_at, reserve_admitted_vec, ArchiveVersion,
    BoundedReader, FramingError,
};
use crate::container::{OpaqueRecord, Record};
use crate::objects::{parse_class_wrapper, parse_class_wrapper_with_userdata, UserdataDescriptor};
use crate::polyedge::{EdgeDomains, HistoryPolyEdge, HistoryReference, PolyEdge, Segment};
use crate::settings::{point, utf16, vector, xform, MillimeterScale, Point3, Vector3, Xform};
use crate::wire::{uuid, Uuid};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

const HISTORY_RECORD: u32 = 0x2000_807b;
const ANONYMOUS: u32 = 0x4000_8000;
const HISTORY_CLASS: Uuid = Uuid::from_canonical([
    0xec, 0xd0, 0xfd, 0x2f, 0x20, 0x88, 0x49, 0xdc, 0x96, 0x41, 0x9c, 0xf7, 0xa2, 0x8f, 0xfa, 0x6b,
]);
const VALUE_CAP: usize = 1 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecordType {
    HistoryParameters,
    FeatureParameters,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HistoryValue {
    id: i32,
    pub(crate) value: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Value {
    None,
    Booleans(Vec<bool>),
    Integers(Vec<i32>),
    Doubles(Vec<f64>),
    Colors(Vec<[u8; 4]>),
    Points(Vec<Point3>),
    Vectors(Vec<Vector3>),
    Transforms(Vec<Xform>),
    Strings(Vec<String>),
    ObjectReferences(Vec<ObjectReference>),
    Geometries(Vec<EmbeddedGeometry>),
    Uuids(Vec<Uuid>),
    PolyEdges(Vec<HistoryPolyEdge>),
    SubdEdgeChains(Vec<SubdEdgeChain>),
    Opaque { type_code: i32, range: Range<usize> },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EmbeddedGeometry {
    class_id: Uuid,
    class_data_range: Range<usize>,
    userdata: Vec<UserdataDescriptor>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SubdEdgeChain {
    subd_id: Uuid,
    edges: Vec<SubdEdge>,
}

#[derive(Debug, Clone, PartialEq)]
struct SubdEdge {
    id: u32,
    reversed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ObjectReference {
    object_id: Uuid,
    component: [i32; 2],
    geometry_type: i32,
    point: Point3,
    evaluation: EvaluationParameter,
    instance_path: Vec<InstanceReference>,
    osnap_mode: i32,
}

#[derive(Debug, Clone, PartialEq)]
struct EvaluationParameter {
    parameter_type: i32,
    component: [i32; 2],
    parameters: [f64; 4],
    intervals: [Option<[f64; 2]>; 3],
}

#[derive(Debug, Clone, PartialEq)]
struct InstanceReference {
    reference_id: Uuid,
    transform: Xform,
    definition_id: Uuid,
    geometry_index: i32,
    evaluation: Option<InstanceEvaluation>,
}

#[derive(Debug, Clone, PartialEq)]
struct InstanceEvaluation {
    component: [i32; 2],
    parameter: EvaluationParameter,
}

struct UuidText<'a>(&'a Uuid);

impl serde::Serialize for UuidText<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self.0)
    }
}

struct UuidList<'a>(&'a [Uuid]);

impl serde::Serialize for UuidList<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for id in self.0 {
            sequence.serialize_element(&UuidText(id))?;
        }
        sequence.end()
    }
}

#[derive(serde::Serialize)]
struct NativeHistoryRecord<'a> {
    id: &'a str,
    source_offset: u64,
    source_uuid: Option<UuidText<'a>>,
    command_uuid: UuidText<'a>,
    record_version: i32,
    record_type: &'static str,
    copy_on_replace: bool,
    antecedent_object_uuids: UuidList<'a>,
    descendant_object_uuids: UuidList<'a>,
    value_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HistoryRecord {
    pub(crate) source_range: Range<usize>,
    id: Uuid,
    version: i32,
    command_id: Uuid,
    descendants: Vec<Uuid>,
    antecedents: Vec<Uuid>,
    pub(crate) values: Vec<HistoryValue>,
    record_type: RecordType,
    copy_on_replace: bool,
}

/// Result of scanning the history-record table.
#[derive(Debug, Clone, Default)]
pub(crate) struct HistoryScan {
    /// Valid history records in source order.
    pub(crate) records: Vec<HistoryRecord>,
    /// Complete records whose registered class payload was not admitted.
    pub(crate) opaque_records: Vec<OpaqueRecord>,
}

fn anonymous(
    bytes: &[u8],
    offset: usize,
    end: usize,
    archive: ArchiveVersion,
) -> Result<(BoundedReader<'_>, usize, u32), FramingError> {
    let chunk = chunk_at(bytes, offset, end, archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short() {
        return Err(FramingError::structural(
            offset,
            "expected long anonymous chunk",
        ));
    }
    let mut reader = BoundedReader::new(bytes, chunk.body().start, chunk.body().end)?;
    let major = reader.i32()?;
    let minor = reader.u32()?;
    if major != 1 || minor > i32::MAX.unsigned_abs() {
        return Err(FramingError::structural(
            chunk.body().start,
            "unsupported anonymous major version",
        ));
    }
    Ok((reader, chunk.next_offset(), minor))
}

fn count(reader: &mut BoundedReader<'_>, element_size: usize) -> Result<usize, FramingError> {
    let offset = reader.position();
    let value = reader.i32()?;
    checked_count_bytes(value, element_size, reader.remaining(), VALUE_CAP, offset)?;
    usize::try_from(value).map_err(|_| FramingError::Overflow { offset })
}

fn uuid_list(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    offset: usize,
    end: usize,
    archive: ArchiveVersion,
) -> Result<(Vec<Uuid>, usize), FramingError> {
    let (mut reader, next, _) = anonymous(bytes, offset, end, archive)?;
    let count = count(&mut reader, 16)?;
    let mut values = admitted_vec(ctx, count, "Rhino history UUID list")?;
    for _ in 0..count {
        values.push(uuid(&mut reader)?);
    }
    reader.skip_remaining()?;
    Ok((values, next))
}

fn array<'a, T>(
    ctx: &DecodeContext<'_>,
    reader: &mut BoundedReader<'a>,
    element_size: usize,
    mut read: impl FnMut(&mut BoundedReader<'a>) -> Result<T, FramingError>,
) -> Result<Vec<T>, FramingError> {
    let count = count(reader, element_size)?;
    let mut values = admitted_vec(ctx, count, "Rhino history value array")?;
    for _ in 0..count {
        values.push(read(reader)?);
    }
    Ok(values)
}

fn component(reader: &mut BoundedReader<'_>) -> Result<[i32; 2], FramingError> {
    Ok([reader.i32()?, reader.i32()?])
}

fn interval(reader: &mut BoundedReader<'_>) -> Result<[f64; 2], FramingError> {
    Ok([reader.f64()?, reader.f64()?])
}

fn evaluation(
    reader: &mut BoundedReader<'_>,
    interval_count: usize,
) -> Result<EvaluationParameter, FramingError> {
    let parameter_type = reader.i32()?;
    let component = component(reader)?;
    let parameters = [reader.f64()?, reader.f64()?, reader.f64()?, reader.f64()?];
    let mut intervals = [None; 3];
    for value in intervals.iter_mut().take(interval_count) {
        *value = Some(interval(reader)?);
    }
    Ok(EvaluationParameter {
        parameter_type,
        component,
        parameters,
        intervals,
    })
}

fn instance_reference(
    bytes: &[u8],
    offset: usize,
    end: usize,
    archive: ArchiveVersion,
) -> Result<(InstanceReference, usize), FramingError> {
    let (mut reader, next, minor) = anonymous(bytes, offset, end, archive)?;
    let reference_id = uuid(&mut reader)?;
    let transform = xform(&mut reader)?;
    let definition_id = uuid(&mut reader)?;
    let geometry_index = reader.i32()?;
    let evaluation = if minor >= 1 {
        let component = component(&mut reader)?;
        let (mut nested, nested_next, _) =
            anonymous(bytes, reader.position(), reader.end(), archive)?;
        let evaluation = evaluation(&mut nested, 3)?;
        nested.skip_remaining()?;
        reader.skip(nested_next - reader.position())?;
        Some(InstanceEvaluation {
            component,
            parameter: evaluation,
        })
    } else {
        None
    };
    reader.skip_remaining()?;
    Ok((
        InstanceReference {
            reference_id,
            transform,
            definition_id,
            geometry_index,
            evaluation,
        },
        next,
    ))
}

fn object_reference(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    offset: usize,
    end: usize,
    archive: ArchiveVersion,
) -> Result<(ObjectReference, usize), FramingError> {
    let (mut reader, next, minor) = anonymous(bytes, offset, end, archive)?;
    let object_id = uuid(&mut reader)?;
    let component = component(&mut reader)?;
    let geometry_type = reader.i32()?;
    let point = point(&mut reader)?;
    let mut evaluation = evaluation(&mut reader, 0)?;
    let path_count = count(&mut reader, 1)?;
    let mut instance_path = admitted_vec(ctx, path_count, "Rhino history instance path")?;
    for _ in 0..path_count {
        let (value, value_next) =
            instance_reference(bytes, reader.position(), reader.end(), archive)?;
        reader.skip(value_next - reader.position())?;
        instance_path.push(value);
    }
    if minor >= 1 {
        evaluation.intervals[0] = Some(interval(&mut reader)?);
        evaluation.intervals[1] = Some(interval(&mut reader)?);
    }
    if minor >= 2 {
        evaluation.intervals[2] = Some(interval(&mut reader)?);
    }
    let osnap_mode = if minor >= 3 { reader.i32()? } else { 0 };
    reader.skip_remaining()?;
    Ok((
        ObjectReference {
            object_id,
            component,
            geometry_type,
            point,
            evaluation,
            instance_path,
            osnap_mode,
        },
        next,
    ))
}

fn object_references(
    ctx: &DecodeContext<'_>,
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<Vec<ObjectReference>, FramingError> {
    let count = count(reader, 1)?;
    let mut values = admitted_vec(ctx, count, "Rhino history object references")?;
    for _ in 0..count {
        let (value, next) = object_reference(
            ctx,
            reader.backing_bytes(),
            reader.position(),
            reader.end(),
            archive,
        )?;
        reader.skip(next - reader.position())?;
        values.push(value);
    }
    Ok(values)
}

fn geometries(
    ctx: &DecodeContext<'_>,
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<Vec<EmbeddedGeometry>, FramingError> {
    let (mut nested, next, _) = anonymous(
        reader.backing_bytes(),
        reader.position(),
        reader.end(),
        archive,
    )?;
    let count = count(&mut nested, 1)?;
    let mut values = admitted_vec(ctx, count, "Rhino history embedded geometries")?;
    for _ in 0..count {
        let start = nested.position();
        let wrapper = chunk_at(nested.backing_bytes(), start, nested.end(), archive, false)?;
        let mut warnings = Diagnostics::new();
        let (class, userdata) = parse_class_wrapper_with_userdata(
            ctx,
            nested.backing_bytes(),
            start..wrapper.next_offset(),
            archive,
            &mut warnings,
        )?;
        nested.skip(wrapper.next_offset() - start)?;
        values.push(EmbeddedGeometry {
            class_id: class.class_uuid,
            class_data_range: class.class_data_range,
            userdata,
        });
    }
    nested.skip_remaining()?;
    reader.skip(next - reader.position())?;
    Ok(values)
}

fn curve_proxy(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    offset: usize,
    end: usize,
    archive: ArchiveVersion,
) -> Result<(Segment<HistoryReference>, usize), FramingError> {
    let (mut reader, next, minor) = anonymous(bytes, offset, end, archive)?;
    let (curve, curve_next) =
        object_reference(ctx, bytes, reader.position(), reader.end(), archive)?;
    reader.skip(curve_next - reader.position())?;
    let reversed = reader.bool()?;
    let full_domain = interval(&mut reader)?;
    let sub_domain = interval(&mut reader)?;
    let proxy_domain = interval(&mut reader)?;
    let domains = if minor >= 1 {
        Some(EdgeDomains {
            edge: interval(&mut reader)?,
            trim: interval(&mut reader)?,
        })
    } else {
        None
    };
    reader.skip_remaining()?;
    Ok((
        Segment {
            reference: HistoryReference {
                curve,
                sub_domain,
                domains,
            },
            reversed,
            domain: full_domain,
            proxy_domain,
        },
        next,
    ))
}

fn poly_edge(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    offset: usize,
    end: usize,
    archive: ArchiveVersion,
) -> Result<(HistoryPolyEdge, usize), FramingError> {
    let (mut reader, next, _) = anonymous(bytes, offset, end, archive)?;
    let segment_count = count(&mut reader, 1)?;
    let mut segments = admitted_vec(ctx, segment_count, "Rhino history polyedge segments")?;
    for _ in 0..segment_count {
        let (segment, segment_next) =
            curve_proxy(ctx, bytes, reader.position(), reader.end(), archive)?;
        reader.skip(segment_next - reader.position())?;
        segments.push(segment);
    }
    let parameters = array(ctx, &mut reader, 8, BoundedReader::f64)?;
    let evaluation_mode = reader.i32()?;
    reader.skip_remaining()?;
    Ok((
        HistoryPolyEdge {
            polyedge: PolyEdge {
                parameters,
                segments,
            },
            evaluation_mode,
        },
        next,
    ))
}

fn poly_edges(
    ctx: &DecodeContext<'_>,
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<Vec<HistoryPolyEdge>, FramingError> {
    let (mut nested, next, _) = anonymous(
        reader.backing_bytes(),
        reader.position(),
        reader.end(),
        archive,
    )?;
    let count = count(&mut nested, 1)?;
    let mut values = admitted_vec(ctx, count, "Rhino history polyedges")?;
    for _ in 0..count {
        let (value, value_next) = poly_edge(
            ctx,
            nested.backing_bytes(),
            nested.position(),
            nested.end(),
            archive,
        )?;
        nested.skip(value_next - nested.position())?;
        values.push(value);
    }
    nested.skip_remaining()?;
    reader.skip(next - reader.position())?;
    Ok(values)
}

fn subd_edge_chain(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    offset: usize,
    end: usize,
    archive: ArchiveVersion,
    warnings: &mut Diagnostics,
) -> Result<(SubdEdgeChain, usize), FramingError> {
    let (mut reader, next, minor) = anonymous(bytes, offset, end, archive)?;
    if minor < 1 {
        return Err(FramingError::structural(
            reader.position(),
            "unsupported SubD edge-chain version",
        ));
    }
    let subd_id = uuid(&mut reader)?;
    let count = count(&mut reader, 1)?;
    let edge_ids = array(ctx, &mut reader, 4, BoundedReader::u32)?;
    let orientations = array(ctx, &mut reader, 1, BoundedReader::u8)?;
    let orientation_start = reader.position() - orientations.len();
    for (index, orientation) in orientations.iter().enumerate() {
        if *orientation > 1 {
            return Err(FramingError::structural(
                orientation_start + index,
                "invalid history SubD edge orientation",
            ));
        }
    }
    let edges = if edge_ids.len() != count || orientations.len() != count {
        warnings.push_coded_admitted(
            ctx,
            crate::loss::RhinoLossCode::RedundantFieldRepaired,
            format_args!("redundant history SubD edge-chain count mismatch; both arrays dropped"),
        )?;
        Vec::new()
    } else {
        let mut edges = admitted_vec(ctx, count, "Rhino history SubD edges")?;
        edges.extend(
            edge_ids
                .into_iter()
                .zip(orientations)
                .map(|(id, orientation)| SubdEdge {
                    id,
                    reversed: orientation == 1,
                }),
        );
        edges
    };
    reader.skip_remaining()?;
    Ok((SubdEdgeChain { subd_id, edges }, next))
}

fn subd_edge_chains(
    ctx: &DecodeContext<'_>,
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    warnings: &mut Diagnostics,
) -> Result<Vec<SubdEdgeChain>, FramingError> {
    let (mut nested, next, minor) = anonymous(
        reader.backing_bytes(),
        reader.position(),
        reader.end(),
        archive,
    )?;
    if minor < 1 {
        return Err(FramingError::structural(
            nested.position(),
            "unsupported SubD edge-chain list version",
        ));
    }
    let count = count(&mut nested, 1)?;
    let mut values = admitted_vec(ctx, count, "Rhino history SubD edge chains")?;
    for _ in 0..count {
        let (value, value_next) = subd_edge_chain(
            ctx,
            nested.backing_bytes(),
            nested.position(),
            nested.end(),
            archive,
            warnings,
        )?;
        nested.skip(value_next - nested.position())?;
        values.push(value);
    }
    nested.skip_remaining()?;
    reader.skip(next - reader.position())?;
    Ok(values)
}

#[cfg(test)]
fn parse_value(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    offset: usize,
    end: usize,
    archive: ArchiveVersion,
) -> Result<(HistoryValue, usize), FramingError> {
    let mut warnings = Diagnostics::new();
    parse_value_with_warnings(ctx, bytes, offset, end, archive, &mut warnings)
}

fn parse_value_with_warnings(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    offset: usize,
    end: usize,
    archive: ArchiveVersion,
    warnings: &mut Diagnostics,
) -> Result<(HistoryValue, usize), FramingError> {
    let (mut reader, next, _) = anonymous(bytes, offset, end, archive)?;
    let type_code = reader.i32()?;
    let id = reader.i32()?;
    let payload = reader.position()..reader.end();
    let value = match type_code {
        0 => Value::None,
        1 => Value::Booleans(array(ctx, &mut reader, 1, BoundedReader::bool)?),
        2 => Value::Integers(array(ctx, &mut reader, 4, BoundedReader::i32)?),
        3 => Value::Doubles(array(ctx, &mut reader, 8, BoundedReader::f64)?),
        4 => Value::Colors(array(ctx, &mut reader, 4, BoundedReader::array)?),
        5 => Value::Points(array(ctx, &mut reader, 24, point)?),
        6 => Value::Vectors(array(ctx, &mut reader, 24, vector)?),
        7 => Value::Transforms(array(ctx, &mut reader, 128, xform)?),
        8 => Value::Strings(array(ctx, &mut reader, 4, utf16)?),
        9 => Value::ObjectReferences(object_references(ctx, &mut reader, archive)?),
        10 => Value::Geometries(geometries(ctx, &mut reader, archive)?),
        11 => Value::Uuids(array(ctx, &mut reader, 16, uuid)?),
        13 => Value::PolyEdges(poly_edges(ctx, &mut reader, archive)?),
        14 => Value::SubdEdgeChains(subd_edge_chains(ctx, &mut reader, archive, warnings)?),
        _ => {
            reader.skip(reader.remaining())?;
            Value::Opaque {
                type_code,
                range: payload,
            }
        }
    };
    reader.skip_remaining()?;
    Ok((HistoryValue { id, value }, next))
}

fn parse_record(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    record: &Record,
    archive: ArchiveVersion,
    warnings: &mut Diagnostics,
) -> Result<HistoryRecord, FramingError> {
    if record.typecode != HISTORY_RECORD || record.is_short() {
        return Err(FramingError::structural(
            record.range.start,
            "invalid history table record",
        ));
    }
    let class = parse_class_wrapper(ctx, bytes, record.body(), archive, warnings)?;
    if class.class_uuid != HISTORY_CLASS {
        return Err(FramingError::structural(
            record.body().start,
            format!("history record has class {}", class.class_uuid),
        ));
    }
    let (mut reader, _next, minor) = anonymous(
        bytes,
        class.class_data_range.start,
        class.class_data_range.end,
        archive,
    )?;
    let id = uuid(&mut reader)?;
    let version = reader.i32()?;
    let command_id = uuid(&mut reader)?;
    let (descendants, next) = uuid_list(ctx, bytes, reader.position(), reader.end(), archive)?;
    reader.skip(next - reader.position())?;
    let (antecedents, next) = uuid_list(ctx, bytes, reader.position(), reader.end(), archive)?;
    reader.skip(next - reader.position())?;
    let (mut values_reader, next, _) = anonymous(bytes, reader.position(), reader.end(), archive)?;
    let value_count = count(&mut values_reader, 1)?;
    let mut values = admitted_vec(ctx, value_count, "Rhino history record values")?;
    for _ in 0..value_count {
        let (value, value_next) = parse_value_with_warnings(
            ctx,
            bytes,
            values_reader.position(),
            values_reader.end(),
            archive,
            warnings,
        )?;
        values_reader.skip(value_next - values_reader.position())?;
        values.push(value);
    }
    values_reader.skip_remaining()?;
    reader.skip(next - reader.position())?;
    let record_type = if minor >= 1 {
        let offset = reader.position();
        match reader.i32()? {
            0 => RecordType::HistoryParameters,
            1 => RecordType::FeatureParameters,
            _ => {
                return Err(FramingError::structural(
                    offset,
                    "invalid history record type",
                ))
            }
        }
    } else {
        RecordType::HistoryParameters
    };
    let copy_on_replace = minor >= 2 && reader.bool()?;
    reader.skip_remaining()?;
    Ok(HistoryRecord {
        source_range: record.range.clone(),
        id,
        version,
        command_id,
        descendants,
        antecedents,
        values,
        record_type,
        copy_on_replace,
    })
}

/// Decodes valid built-in records and isolates malformed records at table boundaries.
pub(crate) fn parse_records(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &[Record],
    archive: ArchiveVersion,
    warnings: &mut Diagnostics,
    table_typecode: u32,
) -> Result<HistoryScan, CodecError> {
    let mut result = HistoryScan::default();
    for record in records {
        match parse_record(ctx, bytes, record, archive, warnings) {
            Ok(value) => {
                reserve_admitted_vec(ctx, &mut result.records, 1, "Rhino history records")
                    .map_err(history_resource_error)?;
                result.records.push(value);
            }
            Err(FramingError::Resource(limit)) => return Err(CodecError::ResourceLimit(limit)),
            Err(error) => {
                warnings.push_admitted(
                    ctx,
                    format_args!("history record at {} degraded: {error}", record.range.start),
                )?;
                reserve_admitted_vec(
                    ctx,
                    &mut result.opaque_records,
                    1,
                    "Rhino opaque history records",
                )
                .map_err(history_resource_error)?;
                result.opaque_records.push(OpaqueRecord {
                    table_typecode,
                    record: record.clone(),
                });
            }
        }
    }
    Ok(result)
}

fn history_resource_error(error: FramingError) -> CodecError {
    match error {
        FramingError::Resource(limit) => CodecError::ResourceLimit(limit),
        other => CodecError::Malformed(other.to_string()),
    }
}

struct Joined<I>(I, &'static str);

impl<I> fmt::Display for Joined<I>
where
    I: Clone + Iterator,
    I::Item: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, value) in self.0.clone().enumerate() {
            if index != 0 {
                f.write_str(self.1)?;
            }
            write!(f, "{value}")?;
        }
        Ok(())
    }
}

struct ReferenceList<'a>(&'a [ObjectReference]);

impl fmt::Display for ReferenceList<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, value) in self.0.iter().enumerate() {
            if index != 0 {
                f.write_str(",")?;
            }
            write!(
                f,
                "{}@{}:{}",
                value.object_id, value.component[0], value.component[1]
            )?;
        }
        Ok(())
    }
}

fn insert_property(
    ctx: &DecodeContext<'_>,
    properties: &mut BTreeMap<String, String>,
    key: impl fmt::Display,
    value: impl fmt::Display,
) -> Result<(), CodecError> {
    let key =
        crate::wire::admitted_format(ctx, format_args!("{key}"), "Rhino history property key")?;
    let value =
        crate::wire::admitted_format(ctx, format_args!("{value}"), "Rhino history property value")?;
    ctx.charge_collection_items(1, "Rhino history property entries")?;
    properties.insert(key, value);
    Ok(())
}

fn admitted_named_properties(
    ctx: &DecodeContext<'_>,
    record: &str,
    entries: BTreeMap<String, String>,
    warnings: &mut Diagnostics,
) -> Result<BTreeMap<cadmpeg_core::text::NonBlankString, String>, CodecError> {
    use std::collections::btree_map::Entry;

    let mut kept = BTreeMap::new();
    for (name, value) in entries {
        match cadmpeg_core::text::NonBlankString::new(name) {
            Some(key) => match kept.entry(key) {
                Entry::Vacant(slot) => {
                    ctx.charge_collection_items(1, "Rhino history named property entries")?;
                    slot.insert(value);
                }
                Entry::Occupied(slot) => warnings.push_coded_admitted(
                    ctx,
                    crate::loss::RhinoLossCode::ObjectAttributesDegraded,
                    format_args!("{record} states the property {} a second time; the property is not transferred", slot.key()),
                )?,
            },
            None => warnings.push_coded_admitted(
                ctx,
                crate::loss::RhinoLossCode::ObjectAttributesDegraded,
                format_args!("{record} states a property with a blank key; the property is not transferred"),
            )?,
        }
    }
    Ok(kept)
}

fn value_text(ctx: &DecodeContext<'_>, value: &Value) -> Result<Option<String>, CodecError> {
    let text = match value {
        Value::None => {
            crate::wire::admitted_format(ctx, format_args!(""), "Rhino history value text")?
        }
        Value::Booleans(values) => crate::wire::admitted_format(
            ctx,
            format_args!("{}", Joined(values.iter(), ",")),
            "Rhino history value text",
        )?,
        Value::Integers(values) => crate::wire::admitted_format(
            ctx,
            format_args!("{}", Joined(values.iter(), ",")),
            "Rhino history value text",
        )?,
        Value::Doubles(values) => crate::wire::admitted_format(
            ctx,
            format_args!("{}", Joined(values.iter(), ",")),
            "Rhino history value text",
        )?,
        Value::Colors(values) => crate::wire::admitted_format(
            ctx,
            format_args!(
                "{}",
                Joined(values.iter().map(|value| Joined(value.iter(), ",")), ";")
            ),
            "Rhino history value text",
        )?,
        Value::Points(values) => crate::wire::admitted_format(
            ctx,
            format_args!(
                "{}",
                Joined(values.iter().map(|value| Joined(value.0.iter(), ",")), ";")
            ),
            "Rhino history value text",
        )?,
        Value::Vectors(values) => crate::wire::admitted_format(
            ctx,
            format_args!(
                "{}",
                Joined(values.iter().map(|value| Joined(value.0.iter(), ",")), ";")
            ),
            "Rhino history value text",
        )?,
        Value::Transforms(values) => crate::wire::admitted_format(
            ctx,
            format_args!(
                "{}",
                Joined(values.iter().map(|value| Joined(value.0.iter(), ",")), ";")
            ),
            "Rhino history value text",
        )?,
        Value::Strings(values) => crate::wire::admitted_format(
            ctx,
            format_args!("{}", Joined(values.iter(), "\u{1f}")),
            "Rhino history value text",
        )?,
        Value::ObjectReferences(values) => crate::wire::admitted_format(
            ctx,
            format_args!("{}", ReferenceList(values)),
            "Rhino history value text",
        )?,
        Value::Geometries(values) => crate::wire::admitted_format(
            ctx,
            format_args!("{}", Joined(values.iter().map(|value| value.class_id), ",")),
            "Rhino history value text",
        )?,
        Value::Uuids(values) => crate::wire::admitted_format(
            ctx,
            format_args!("{}", Joined(values.iter(), ",")),
            "Rhino history value text",
        )?,
        Value::PolyEdges(_) | Value::SubdEdgeChains(_) | Value::Opaque { .. } => return Ok(None),
    };
    Ok(Some(text))
}

fn evaluation_properties(
    ctx: &DecodeContext<'_>,
    prefix: &str,
    value: &EvaluationParameter,
    properties: &mut BTreeMap<String, String>,
) -> Result<(), CodecError> {
    insert_property(
        ctx,
        properties,
        format_args!("{prefix}.type"),
        value.parameter_type,
    )?;
    insert_property(
        ctx,
        properties,
        format_args!("{prefix}.component"),
        Joined(value.component.iter(), ","),
    )?;
    insert_property(
        ctx,
        properties,
        format_args!("{prefix}.parameters"),
        Joined(value.parameters.iter(), ","),
    )?;
    for (index, interval) in value.intervals.iter().enumerate() {
        if let Some(interval) = interval {
            insert_property(
                ctx,
                properties,
                format_args!("{prefix}.interval_{index}"),
                Joined(interval.iter(), ","),
            )?;
        }
    }
    Ok(())
}

fn object_reference_properties(
    ctx: &DecodeContext<'_>,
    prefix: &str,
    value: &ObjectReference,
    properties: &mut BTreeMap<String, String>,
) -> Result<(), CodecError> {
    insert_property(
        ctx,
        properties,
        format_args!("{prefix}.object_id"),
        value.object_id,
    )?;
    insert_property(
        ctx,
        properties,
        format_args!("{prefix}.component"),
        Joined(value.component.iter(), ","),
    )?;
    insert_property(
        ctx,
        properties,
        format_args!("{prefix}.geometry_type"),
        value.geometry_type,
    )?;
    insert_property(
        ctx,
        properties,
        format_args!("{prefix}.point"),
        Joined(value.point.0.iter(), ","),
    )?;
    insert_property(
        ctx,
        properties,
        format_args!("{prefix}.osnap_mode"),
        value.osnap_mode,
    )?;
    evaluation_properties(
        ctx,
        &crate::wire::admitted_format(
            ctx,
            format_args!("{prefix}.evaluation"),
            "Rhino history evaluation prefix",
        )?,
        &value.evaluation,
        properties,
    )?;
    insert_property(
        ctx,
        properties,
        format_args!("{prefix}.instance_count"),
        value.instance_path.len(),
    )?;
    for (index, instance) in value.instance_path.iter().enumerate() {
        let path = crate::wire::admitted_format(
            ctx,
            format_args!("{prefix}.instance_{index}"),
            "Rhino history instance prefix",
        )?;
        insert_property(
            ctx,
            properties,
            format_args!("{path}.reference_id"),
            instance.reference_id,
        )?;
        insert_property(
            ctx,
            properties,
            format_args!("{path}.transform"),
            Joined(instance.transform.0.iter(), ","),
        )?;
        insert_property(
            ctx,
            properties,
            format_args!("{path}.definition_id"),
            instance.definition_id,
        )?;
        insert_property(
            ctx,
            properties,
            format_args!("{path}.geometry_index"),
            instance.geometry_index,
        )?;
        if let Some(evaluation) = &instance.evaluation {
            insert_property(
                ctx,
                properties,
                format_args!("{path}.component"),
                Joined(evaluation.component.iter(), ","),
            )?;
            evaluation_properties(
                ctx,
                &crate::wire::admitted_format(
                    ctx,
                    format_args!("{path}.evaluation"),
                    "Rhino history evaluation prefix",
                )?,
                &evaluation.parameter,
                properties,
            )?;
        }
    }
    Ok(())
}

struct CageJson<'a>(&'a crate::cage::Cage);

struct MeshVertices<'a>(
    &'a cadmpeg_ir::tessellation::TessellationMesh<
        cadmpeg_ir::features::FinitePoint3,
        cadmpeg_ir::features::FiniteVector3,
    >,
);

impl serde::Serialize for MeshVertices<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use cadmpeg_ir::tessellation::TessellationMesh;
        use serde::ser::SerializeSeq;
        match self.0 {
            TessellationMesh::List { vertices, .. }
            | TessellationMesh::CornerShadedList { vertices, .. } => vertices.serialize(serializer),
            TessellationMesh::ShadedList { vertices, .. } => {
                let mut sequence = serializer.serialize_seq(Some(vertices.len()))?;
                for vertex in vertices {
                    sequence.serialize_element(&vertex.position)?;
                }
                sequence.end()
            }
            TessellationMesh::Strips { strips } => {
                let mut sequence = serializer.serialize_seq(Some(self.0.vertex_count()))?;
                for strip in strips.as_slice() {
                    for vertex in strip.vertices() {
                        sequence.serialize_element(vertex)?;
                    }
                }
                sequence.end()
            }
            TessellationMesh::ShadedStrips { strips } => {
                let mut sequence = serializer.serialize_seq(Some(self.0.vertex_count()))?;
                for strip in strips.as_slice() {
                    for vertex in strip.vertices() {
                        sequence.serialize_element(&vertex.position)?;
                    }
                }
                sequence.end()
            }
        }
    }
}

struct MeshTriangles<'a>(
    &'a cadmpeg_ir::tessellation::TessellationMesh<
        cadmpeg_ir::features::FinitePoint3,
        cadmpeg_ir::features::FiniteVector3,
    >,
);

fn serialize_strip_triangles<S: serde::Serializer, V>(
    strips: &cadmpeg_ir::tessellation::Strips<V>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::ser::{Error as _, SerializeSeq};
    let count = strips
        .as_slice()
        .iter()
        .map(cadmpeg_ir::tessellation::Strip::triangle_count)
        .sum();
    let mut sequence = serializer.serialize_seq(Some(count))?;
    let mut base = 0_u32;
    for strip in strips.as_slice() {
        for index in 0..strip.triangle_count() {
            let index = u32::try_from(index).map_err(S::Error::custom)?;
            let a = base
                .checked_add(index)
                .ok_or_else(|| S::Error::custom("mesh index overflow"))?;
            let b = a
                .checked_add(1)
                .ok_or_else(|| S::Error::custom("mesh index overflow"))?;
            let c = a
                .checked_add(2)
                .ok_or_else(|| S::Error::custom("mesh index overflow"))?;
            sequence.serialize_element(&if index.is_multiple_of(2) {
                [a, b, c]
            } else {
                [a, c, b]
            })?;
        }
        let length = u32::try_from(strip.vertices().len()).map_err(S::Error::custom)?;
        base = base
            .checked_add(length)
            .ok_or_else(|| S::Error::custom("mesh strip base overflow"))?;
    }
    sequence.end()
}

fn serialize_strip_lengths<S: serde::Serializer, V>(
    strips: &cadmpeg_ir::tessellation::Strips<V>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::ser::{Error as _, SerializeSeq};
    let mut sequence = serializer.serialize_seq(Some(strips.as_slice().len()))?;
    for strip in strips.as_slice() {
        sequence
            .serialize_element(&u32::try_from(strip.vertices().len()).map_err(S::Error::custom)?)?;
    }
    sequence.end()
}

impl serde::Serialize for MeshTriangles<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use cadmpeg_ir::tessellation::TessellationMesh;
        use serde::ser::SerializeSeq;
        match self.0 {
            TessellationMesh::List { triangles, .. }
            | TessellationMesh::ShadedList { triangles, .. } => triangles.serialize(serializer),
            TessellationMesh::CornerShadedList { triangles, .. } => {
                let mut sequence = serializer.serialize_seq(Some(triangles.len()))?;
                for triangle in triangles {
                    sequence.serialize_element(&triangle.corners)?;
                }
                sequence.end()
            }
            TessellationMesh::Strips { strips } => serialize_strip_triangles(strips, serializer),
            TessellationMesh::ShadedStrips { strips } => {
                serialize_strip_triangles(strips, serializer)
            }
        }
    }
}

struct MeshStripLengths<'a>(
    &'a cadmpeg_ir::tessellation::TessellationMesh<
        cadmpeg_ir::features::FinitePoint3,
        cadmpeg_ir::features::FiniteVector3,
    >,
);

impl serde::Serialize for MeshStripLengths<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use cadmpeg_ir::tessellation::TessellationMesh;
        match self.0 {
            TessellationMesh::List { .. }
            | TessellationMesh::ShadedList { .. }
            | TessellationMesh::CornerShadedList { .. } => ([] as [u32; 0]).serialize(serializer),
            TessellationMesh::Strips { strips } => serialize_strip_lengths(strips, serializer),
            TessellationMesh::ShadedStrips { strips } => {
                serialize_strip_lengths(strips, serializer)
            }
        }
    }
}

struct MeshNormals<'a>(
    &'a cadmpeg_ir::tessellation::TessellationMesh<
        cadmpeg_ir::features::FinitePoint3,
        cadmpeg_ir::features::FiniteVector3,
    >,
);

impl serde::Serialize for MeshNormals<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use cadmpeg_ir::tessellation::TessellationMesh;
        use serde::ser::SerializeSeq;
        match self.0 {
            TessellationMesh::List { .. }
            | TessellationMesh::CornerShadedList { .. }
            | TessellationMesh::Strips { .. } => ([] as [u32; 0]).serialize(serializer),
            TessellationMesh::ShadedList { vertices, .. } => {
                let mut sequence = serializer.serialize_seq(Some(vertices.len()))?;
                for vertex in vertices {
                    sequence.serialize_element(&vertex.normal)?;
                }
                sequence.end()
            }
            TessellationMesh::ShadedStrips { strips } => {
                let mut sequence = serializer.serialize_seq(Some(self.0.vertex_count()))?;
                for strip in strips.as_slice() {
                    for vertex in strip.vertices() {
                        sequence.serialize_element(&vertex.normal)?;
                    }
                }
                sequence.end()
            }
        }
    }
}

struct MeshJson<'a>(&'a crate::mesh::DecodedMesh);

impl serde::Serialize for MeshJson<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let tessellation = &self.0.tessellation;
        let mut map = serializer.serialize_map(Some(6))?;
        map.serialize_entry("channels", tessellation.channels())?;
        map.serialize_entry("kind", "mesh")?;
        map.serialize_entry("normals", &MeshNormals(tessellation.mesh()))?;
        map.serialize_entry("strip_lengths", &MeshStripLengths(tessellation.mesh()))?;
        map.serialize_entry("triangles", &MeshTriangles(tessellation.mesh()))?;
        map.serialize_entry("vertices", &MeshVertices(tessellation.mesh()))?;
        map.end()
    }
}

struct CapPcurveJson<'a>(&'a crate::extrusion::CapPcurve);

impl serde::Serialize for CapPcurveJson<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(5))?;
        map.serialize_entry("control_points", &self.0.control_points)?;
        map.serialize_entry("degree", &self.0.degree)?;
        map.serialize_entry("knots", &self.0.knots)?;
        map.serialize_entry("periodic", &self.0.periodic)?;
        map.serialize_entry("weights", &self.0.weights)?;
        map.end()
    }
}

struct BoundaryJson<'a>(&'a crate::extrusion::ExtrusionBoundary);

impl serde::Serialize for BoundaryJson<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(5))?;
        map.serialize_entry("end_nurbs", &self.0.end_nurbs)?;
        map.serialize_entry("end_pcurve", &CapPcurveJson(&self.0.end_pcurve))?;
        map.serialize_entry("start_curve", self.0.start_curve.reported_geometry())?;
        map.serialize_entry("start_nurbs", &self.0.start_nurbs)?;
        map.serialize_entry("start_pcurve", &CapPcurveJson(&self.0.start_pcurve))?;
        map.end()
    }
}

struct BoundariesJson<'a>(&'a [crate::extrusion::ExtrusionBoundary]);

impl serde::Serialize for BoundariesJson<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for boundary in self.0 {
            sequence.serialize_element(&BoundaryJson(boundary))?;
        }
        sequence.end()
    }
}

struct LateralsJson<'a>(&'a [crate::extrusion::ExtrusionBoundary]);

impl serde::Serialize for LateralsJson<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for boundary in self.0 {
            sequence.serialize_element(&boundary.lateral)?;
        }
        sequence.end()
    }
}

struct ExtrusionJson<'a>(&'a crate::extrusion::DecodedExtrusion);

impl serde::Serialize for ExtrusionJson<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(8))?;
        map.serialize_entry("boundaries", &BoundariesJson(&self.0.boundaries))?;
        map.serialize_entry("cap_normals", &self.0.cap_normals)?;
        map.serialize_entry("cap_origins", &self.0.cap_origins)?;
        map.serialize_entry("cap_u_axes", &self.0.cap_u_axes)?;
        map.serialize_entry("caps", &self.0.caps)?;
        map.serialize_entry("direction", &self.0.direction)?;
        map.serialize_entry("kind", "extrusion")?;
        map.serialize_entry("laterals", &LateralsJson(&self.0.boundaries))?;
        map.end()
    }
}

struct MorphControlJson<'a>(&'a crate::morph::Control);

impl serde::Serialize for MorphControlJson<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(3))?;
        match self.0 {
            crate::morph::Control::Curve { start, end } => {
                map.serialize_entry("end", end)?;
                map.serialize_entry("kind", "curve")?;
                map.serialize_entry("start", start)?;
            }
            crate::morph::Control::Surface { start, end } => {
                map.serialize_entry("end", end)?;
                map.serialize_entry("kind", "surface")?;
                map.serialize_entry("start", start)?;
            }
            crate::morph::Control::Cage {
                start_transform,
                end,
            } => {
                map.serialize_entry("end", &CageJson(end))?;
                map.serialize_entry("kind", "cage")?;
                map.serialize_entry("start_transform", start_transform)?;
            }
        }
        map.end()
    }
}

struct LocalizerJson<'a>(&'a crate::morph::Localizer);

impl serde::Serialize for LocalizerJson<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(6))?;
        map.serialize_entry("curve", &self.0.curve)?;
        map.serialize_entry("interval", &self.0.interval)?;
        map.serialize_entry("kind", &self.0.kind)?;
        map.serialize_entry("point", &self.0.point)?;
        map.serialize_entry("surface", &self.0.surface)?;
        map.serialize_entry("vector", &self.0.vector)?;
        map.end()
    }
}

struct LocalizersJson<'a>(&'a [crate::morph::Localizer]);

impl serde::Serialize for LocalizersJson<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for localizer in self.0 {
            sequence.serialize_element(&LocalizerJson(localizer))?;
        }
        sequence.end()
    }
}

struct MorphJson<'a>(&'a crate::morph::Morph);

impl serde::Serialize for MorphJson<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(7))?;
        map.serialize_entry("captive_ids", &UuidList(&self.0.captive_ids))?;
        map.serialize_entry("control", &MorphControlJson(&self.0.control))?;
        map.serialize_entry("kind", "morph_control")?;
        map.serialize_entry("localizers", &LocalizersJson(&self.0.localizers))?;
        map.serialize_entry("preserve_structure", &self.0.preserve_structure)?;
        map.serialize_entry("quick_preview", &self.0.quick_preview)?;
        map.serialize_entry("tolerance", &self.0.tolerance)?;
        map.end()
    }
}

struct DetailJson<'a>(&'a crate::detail::Detail);

impl serde::Serialize for DetailJson<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(3))?;
        map.serialize_entry("boundary", self.0.boundary.reported_geometry())?;
        map.serialize_entry("kind", "detail_view")?;
        map.serialize_entry("page_per_model_ratio", &self.0.page_per_model_ratio)?;
        map.end()
    }
}

struct HatchLoopJson<'a>(&'a crate::hatch::HatchLoop);

impl serde::Serialize for HatchLoopJson<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("curve", self.0.curve.reported_geometry())?;
        map.serialize_entry(
            "kind",
            match self.0.kind {
                crate::hatch::LoopKind::Outer => "outer",
                crate::hatch::LoopKind::Inner => "inner",
            },
        )?;
        map.end()
    }
}

struct HatchLoopsJson<'a>(&'a [crate::hatch::HatchLoop]);

impl serde::Serialize for HatchLoopsJson<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for hatch_loop in self.0 {
            sequence.serialize_element(&HatchLoopJson(hatch_loop))?;
        }
        sequence.end()
    }
}

struct HatchPlaneJson<'a> {
    plane: &'a crate::settings::Plane,
    origin: [cadmpeg_ir::scalar::FiniteReal; 3],
    equation_constant: cadmpeg_ir::scalar::FiniteReal,
}

impl serde::Serialize for HatchPlaneJson<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(5))?;
        map.serialize_entry(
            "equation",
            &[
                self.plane.equation[0],
                self.plane.equation[1],
                self.plane.equation[2],
                self.equation_constant.get(),
            ],
        )?;
        map.serialize_entry("origin", &self.origin)?;
        map.serialize_entry("xaxis", &self.plane.xaxis.get())?;
        map.serialize_entry("yaxis", &self.plane.yaxis.get())?;
        map.serialize_entry("zaxis", &self.plane.zaxis.get())?;
        map.end()
    }
}

struct HatchJson<'a> {
    hatch: &'a crate::hatch::Hatch,
    plane: HatchPlaneJson<'a>,
}

impl serde::Serialize for HatchJson<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map =
            serializer.serialize_map(Some(7 + usize::from(self.hatch.gradient.is_some())))?;
        map.serialize_entry("basepoint", &self.hatch.basepoint)?;
        if let Some(gradient) = &self.hatch.gradient {
            map.serialize_entry("gradient", &crate::hatch::gradient_semantic(gradient))?;
        }
        map.serialize_entry("kind", "hatch")?;
        map.serialize_entry("loops", &HatchLoopsJson(&self.hatch.loops))?;
        map.serialize_entry("pattern_index", &self.hatch.pattern_index)?;
        map.serialize_entry("pattern_rotation", &self.hatch.pattern_rotation)?;
        map.serialize_entry("pattern_scale", &self.hatch.pattern_scale)?;
        map.serialize_entry("plane", &self.plane)?;
        map.end()
    }
}

struct SubdDiagnostics<'a>(&'a [crate::subd::SubdEnumDiagnostic]);

impl serde::Serialize for SubdDiagnostics<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        struct Text(crate::subd::SubdEnumDiagnostic);
        impl serde::Serialize for Text {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.collect_str(&self.0)
            }
        }
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for diagnostic in self.0 {
            sequence.serialize_element(&Text(*diagnostic))?;
        }
        sequence.end()
    }
}

#[derive(serde::Serialize)]
struct SubdJson<'a, S: serde::Serialize, M: serde::Serialize> {
    enum_diagnostics: SubdDiagnostics<'a>,
    kind: &'static str,
    neutral_metadata: &'a M,
    surface: &'a S,
}

#[derive(serde::Serialize)]
struct EmptySubdJson {
    empty: bool,
    kind: &'static str,
}

impl serde::Serialize for CageJson<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let cage = self.0;
        let mut map = serializer.serialize_map(Some(8))?;
        map.serialize_entry("control_points", &cage.control_points)?;
        map.serialize_entry("counts", &cage.counts)?;
        map.serialize_entry("dimension", &cage.dimension)?;
        map.serialize_entry("kind", "nurbs_cage")?;
        map.serialize_entry("knots", &cage.knots)?;
        map.serialize_entry("orders", &cage.orders)?;
        map.serialize_entry("rational", &cage.rational())?;
        map.serialize_entry("weights", &cage.weights)?;
        map.end()
    }
}

fn embedded_json(
    ctx: &DecodeContext<'_>,
    value: &impl serde::Serialize,
    refusal: &mut Option<CodecError>,
) -> Option<String> {
    match crate::wire::admitted_canonical_json(ctx, value, "Rhino embedded history geometry JSON") {
        Ok(text) => Some(text),
        Err(error @ CodecError::ResourceLimit(_)) => {
            *refusal = Some(error);
            None
        }
        Err(_) => None,
    }
}

fn extended_geometry_json(
    expand: crate::mesh::MeshExpand<'_>,
    value: &EmbeddedGeometry,
    archive: ArchiveVersion,
    writer_version: Option<i64>,
    scale: MillimeterScale,
    warnings: &mut Diagnostics,
    refusal: &mut Option<cadmpeg_core::CodecError>,
) -> Option<String> {
    let data = expand.data();
    if crate::mesh::supported_class(value.class_id) {
        let mut budget = crate::mesh::MeshBudget::new();
        let mesh = optional_geometry(
            crate::mesh::decode(
                expand,
                data,
                value.class_data_range.clone(),
                archive,
                crate::mesh::MeshDecodeOptions {
                    writer_version,
                    association: None,
                    id: "rhino:history:embedded-mesh".to_string().into(),
                    scale,
                    userdata: &value.userdata,
                },
                &mut budget,
            ),
            refusal,
        )?;
        embedded_json(expand.ctx(), &MeshJson(&mesh), refusal)
    } else if crate::subd::supported_class(value.class_id) {
        let subd = match crate::subd::decode(
            expand.ctx(),
            data,
            value.class_data_range.clone(),
            archive,
            scale,
            cadmpeg_ir::ids::SubdId::compose(
                &cadmpeg_ir::identity_namespace!("rhino", "history", "subd"),
                cadmpeg_ir::identity_key!("embedded"),
            ),
        ) {
            Ok(subd) => subd,
            Err(crate::subd::SubdError::Resource(limit)) => {
                *refusal = Some(cadmpeg_core::CodecError::ResourceLimit(limit));
                return None;
            }
            Err(_) => return None,
        };
        match subd {
            None => embedded_json(
                expand.ctx(),
                &EmptySubdJson {
                    empty: true,
                    kind: "subd",
                },
                refusal,
            ),
            Some(crate::subd::DecodedSubd {
                surface,
                neutral_metadata,
                enum_diagnostics,
                ..
            }) => embedded_json(
                expand.ctx(),
                &SubdJson {
                    enum_diagnostics: SubdDiagnostics(&enum_diagnostics),
                    kind: "subd",
                    neutral_metadata: &neutral_metadata,
                    surface: &surface,
                },
                refusal,
            ),
        }
    } else if crate::extrusion::supported_class(value.class_id) {
        let mut budget = crate::mesh::MeshBudget::new();
        let extrusion = optional_geometry(
            crate::extrusion::decode(
                expand,
                data,
                value.class_data_range.clone(),
                archive,
                writer_version,
                scale,
                &value.userdata,
                &mut budget,
            ),
            refusal,
        )?;
        embedded_json(expand.ctx(), &ExtrusionJson(&extrusion), refusal)
    } else if value.class_id == crate::cage::CLASS {
        let cage = optional_geometry(
            crate::cage::decode(expand, value.class_data_range.clone(), scale, archive),
            refusal,
        )?;
        embedded_json(expand.ctx(), &CageJson(&cage), refusal)
    } else if value.class_id == crate::morph::CLASS {
        let morph = optional_geometry(
            crate::morph::decode(expand, value.class_data_range.clone(), scale, archive),
            refusal,
        )?;
        embedded_json(expand.ctx(), &MorphJson(&morph), refusal)
    } else if crate::brep::supported_class(value.class_id) {
        crate::decode::embedded_brep_json(
            expand,
            data,
            value.class_data_range.clone(),
            archive,
            writer_version,
            scale,
            refusal,
        )
    } else if value.class_id == crate::hatch::CLASS {
        let mut hatch = optional_geometry(
            crate::hatch::decode(expand, value.class_data_range.clone(), scale, archive),
            refusal,
        )?;
        if let Err(errors) =
            crate::hatch::apply_userdata(data, &value.userdata, scale, archive, &mut hatch)
        {
            for error in errors {
                optional_warning(
                    expand.ctx(),
                    warnings,
                    refusal,
                    format_args!(
                        "embedded history hatch userdata at offset {}: {error}",
                        value.class_data_range.start
                    ),
                )?;
            }
        }
        let plane = hatch.plane;
        let millimetres = |coordinate: f64, field: &str| {
            crate::wire::scaled_coordinate(coordinate, scale).ok_or_else(|| {
                cadmpeg_core::CodecError::NotImplemented(format!(
                    "history hatch {field} {coordinate} at {} millimetres per unit has no \
                     finite millimetre value",
                    scale.value()
                ))
            })
        };
        let scaled = (|| {
            Ok::<_, cadmpeg_core::CodecError>((
                [
                    millimetres(plane.origin[0], "plane.origin[0]")?,
                    millimetres(plane.origin[1], "plane.origin[1]")?,
                    millimetres(plane.origin[2], "plane.origin[2]")?,
                ],
                millimetres(plane.equation[3], "plane.equation[3]")?,
            ))
        })();
        let (origin, equation_constant) = match scaled {
            Ok(scaled) => scaled,
            Err(error) => {
                optional_warning(
                    expand.ctx(),
                    warnings,
                    refusal,
                    format_args!(
                        "embedded history hatch at offset {}: {error}",
                        value.class_data_range.start
                    ),
                )?;
                return None;
            }
        };
        embedded_json(
            expand.ctx(),
            &HatchJson {
                hatch: &hatch,
                plane: HatchPlaneJson {
                    plane: &plane,
                    origin,
                    equation_constant,
                },
            },
            refusal,
        )
    } else if value.class_id == crate::detail::CLASS {
        let detail = optional_geometry(
            crate::detail::decode(expand.ctx(), data, value.class_data_range.clone(), archive),
            refusal,
        )?;
        embedded_json(expand.ctx(), &DetailJson(&detail), refusal)
    } else if crate::dimensions::supported_class(value.class_id) {
        let dimension = match crate::dimensions::decode(
            expand.ctx(),
            data,
            value.class_id,
            value.class_data_range.clone(),
            scale,
            archive,
        ) {
            Ok(dimension) => dimension,
            Err(crate::chunks::FramingError::Resource(limit)) => {
                *refusal = Some(cadmpeg_core::CodecError::ResourceLimit(limit));
                return None;
            }
            Err(error) => {
                optional_warning(
                    expand.ctx(),
                    warnings,
                    refusal,
                    format_args!(
                        "embedded history dimension at offset {}: {error}",
                        value.class_data_range.start
                    ),
                )?;
                return None;
            }
        };
        let mut dimension = dimension;
        if let Err(error) =
            crate::dimensions::apply_userdata(data, &value.userdata, archive, scale, &mut dimension)
        {
            optional_warning(
                expand.ctx(),
                warnings,
                refusal,
                format_args!(
                    "embedded history dimension userdata at offset {}: {error}",
                    value.class_data_range.start
                ),
            )?;
            return None;
        }
        match crate::dimensions::semantic_json(expand.ctx(), &dimension) {
            Ok(semantic) => Some(semantic),
            Err(error @ CodecError::ResourceLimit(_)) => {
                *refusal = Some(error);
                None
            }
            Err(error) => {
                optional_warning(
                    expand.ctx(),
                    warnings,
                    refusal,
                    format_args!(
                        "embedded history dimension at offset {}: {error}",
                        value.class_data_range.start
                    ),
                )?;
                None
            }
        }
    } else if value.class_id == crate::polyedge::CURVE_CLASS {
        let polyedge =
            match crate::polyedge::decode(expand, value.class_data_range.clone(), archive) {
                Ok(polyedge) => polyedge,
                Err(FramingError::Resource(limit)) => {
                    *refusal = Some(CodecError::ResourceLimit(limit));
                    return None;
                }
                Err(error) => {
                    optional_warning(
                        expand.ctx(),
                        warnings,
                        refusal,
                        format_args!(
                            "embedded history polyedge at offset {}: {error}",
                            value.class_data_range.start
                        ),
                    )?;
                    return None;
                }
            };
        match crate::polyedge::semantic_json(expand.ctx(), &polyedge) {
            Ok(semantic) => semantic,
            Err(error) => {
                *refusal = Some(error);
                None
            }
        }
    } else {
        None
    }
}

/// Accounting for geometry decoded out of a history record's values.
///
/// History curves/surfaces are stringified into native properties; putting them
/// in `model.curves`/`surfaces` fails `Check::CarrierReachability`.
struct GeometrySink<'a> {
    warnings: &'a mut Diagnostics,
    untyped: usize,
    failed: usize,
    redundant_repairs: usize,
    refusal: Option<cadmpeg_core::CodecError>,
}

#[derive(Debug)]
pub(crate) enum ProjectionError {
    Admission(String),
    Codec(cadmpeg_core::CodecError),
}

impl From<String> for ProjectionError {
    fn from(message: String) -> Self {
        Self::Admission(message)
    }
}

impl From<CodecError> for ProjectionError {
    fn from(error: CodecError) -> Self {
        Self::Codec(error)
    }
}

fn optional_geometry<T>(
    result: Result<T, crate::curves::GeometryError>,
    refusal: &mut Option<cadmpeg_core::CodecError>,
) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(crate::curves::GeometryError::Codec(error)) => {
            *refusal = Some(error);
            None
        }
        Err(_) => None,
    }
}

fn optional_warning(
    ctx: &DecodeContext<'_>,
    warnings: &mut Diagnostics,
    refusal: &mut Option<CodecError>,
    message: fmt::Arguments<'_>,
) -> Option<()> {
    match warnings.push_admitted(ctx, message) {
        Ok(()) => Some(()),
        Err(error) => {
            *refusal = Some(error);
            None
        }
    }
}

fn structured_value_properties(
    ctx: &DecodeContext<'_>,
    key: &str,
    value: &Value,
    geometry_context: Option<(
        crate::mesh::MeshExpand<'_>,
        ArchiveVersion,
        Option<i64>,
        MillimeterScale,
    )>,
    properties: &mut BTreeMap<String, String>,
    sink: &mut GeometrySink<'_>,
) -> Result<(), CodecError> {
    match value {
        Value::ObjectReferences(values) => {
            insert_property(ctx, properties, format_args!("{key}.count"), values.len())?;
            for (index, value) in values.iter().enumerate() {
                let prefix = crate::wire::admitted_format(
                    ctx,
                    format_args!("{key}.{index}"),
                    "Rhino history reference prefix",
                )?;
                object_reference_properties(ctx, &prefix, value, properties)?;
            }
        }
        Value::Geometries(values) => {
            insert_property(ctx, properties, format_args!("{key}.count"), values.len())?;
            for (index, value) in values.iter().enumerate() {
                insert_property(
                    ctx,
                    properties,
                    format_args!("{key}.{index}.class_id"),
                    value.class_id,
                )?;
                if let Some((expand, archive, writer_version, scale)) = geometry_context {
                    let data = expand.data();
                    let decoded = optional_geometry(
                        crate::curves::decode(
                            expand.ctx(),
                            data,
                            value.class_id,
                            value.class_data_range.clone(),
                            scale,
                            archive,
                        ),
                        &mut sink.refusal,
                    );
                    if sink.refusal.is_some() {
                        return Ok(());
                    }
                    if let Some(decoded) = decoded {
                        sink.untyped += 1;
                        let semantic = match decoded {
                            crate::curves::DecodedGeometry::Point { position, .. } => {
                                crate::wire::admitted_json(
                                    ctx,
                                    &position,
                                    "Rhino history geometry JSON",
                                )
                            }
                            crate::curves::DecodedGeometry::PointCloud(cloud) => {
                                sink.redundant_repairs += cloud.warnings.len();
                                crate::wire::admitted_json(
                                    ctx,
                                    &cloud.points,
                                    "Rhino history geometry JSON",
                                )
                            }
                            crate::curves::DecodedGeometry::Curve { curve } => {
                                crate::wire::admitted_json(
                                    ctx,
                                    &curve.reported_geometry(),
                                    "Rhino history geometry JSON",
                                )
                            }
                            crate::curves::DecodedGeometry::Surface { surface } => match surface {
                                crate::surfaces::DecodedSurface::Typed { geometry, .. } => {
                                    crate::wire::admitted_json(
                                        ctx,
                                        &geometry.into_geometry(),
                                        "Rhino history geometry JSON",
                                    )
                                }
                                crate::surfaces::DecodedSurface::Procedural {
                                    geometry, ..
                                } => crate::wire::admitted_json(
                                    ctx,
                                    &geometry,
                                    "Rhino history geometry JSON",
                                ),
                            },
                        };
                        match semantic {
                            Ok(semantic) => {
                                ctx.charge_collection_items(1, "Rhino history property entries")?;
                                let property_key = crate::wire::admitted_format(
                                    ctx,
                                    format_args!("{key}.{index}.geometry"),
                                    "Rhino history property key",
                                )?;
                                properties.insert(property_key, semantic);
                            }
                            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                            Err(_) => {}
                        }
                    } else {
                        let semantic = extended_geometry_json(
                            expand,
                            value,
                            archive,
                            writer_version,
                            scale,
                            sink.warnings,
                            &mut sink.refusal,
                        );
                        if sink.refusal.is_some() {
                            return Ok(());
                        }
                        if let Some(semantic) = semantic {
                            sink.untyped += 1;
                            ctx.charge_collection_items(1, "Rhino history property entries")?;
                            let property_key = crate::wire::admitted_format(
                                ctx,
                                format_args!("{key}.{index}.geometry"),
                                "Rhino history property key",
                            )?;
                            properties.insert(property_key, semantic);
                        } else {
                            sink.failed += 1;
                        }
                    }
                } else {
                    sink.untyped += 1;
                    sink.warnings.push_coded_admitted(ctx,
                        crate::loss::RhinoLossCode::HistoryGeometryNotTransferred,
                        format_args!(
                            "history geometry value {key}.{index} at source range {}..{} has no coordinate-unit binding",
                            value.class_data_range.start,
                            value.class_data_range.end
                        ),
                    )?;
                    insert_property(
                        ctx,
                        properties,
                        format_args!("{key}.{index}.geometry_status"),
                        "unavailable_unit_binding",
                    )?;
                    insert_property(
                        ctx,
                        properties,
                        format_args!("{key}.{index}.source_range"),
                        format_args!(
                            "{}..{}",
                            value.class_data_range.start, value.class_data_range.end
                        ),
                    )?;
                }
            }
        }
        Value::PolyEdges(values) => {
            insert_property(ctx, properties, format_args!("{key}.count"), values.len())?;
            for (edge_index, edge) in values.iter().enumerate() {
                let edge_key = crate::wire::admitted_format(
                    ctx,
                    format_args!("{key}.{edge_index}"),
                    "Rhino history edge prefix",
                )?;
                insert_property(
                    ctx,
                    properties,
                    format_args!("{edge_key}.parameters"),
                    Joined(edge.polyedge.parameters.iter(), ","),
                )?;
                insert_property(
                    ctx,
                    properties,
                    format_args!("{edge_key}.evaluation_mode"),
                    edge.evaluation_mode,
                )?;
                insert_property(
                    ctx,
                    properties,
                    format_args!("{edge_key}.segment_count"),
                    edge.polyedge.segments.len(),
                )?;
                for (segment_index, segment) in edge.polyedge.segments.iter().enumerate() {
                    let segment_key = crate::wire::admitted_format(
                        ctx,
                        format_args!("{edge_key}.segment_{segment_index}"),
                        "Rhino history segment prefix",
                    )?;
                    let curve_key = crate::wire::admitted_format(
                        ctx,
                        format_args!("{segment_key}.curve"),
                        "Rhino history curve prefix",
                    )?;
                    object_reference_properties(
                        ctx,
                        &curve_key,
                        &segment.reference.curve,
                        properties,
                    )?;
                    insert_property(
                        ctx,
                        properties,
                        format_args!("{segment_key}.reversed"),
                        segment.reversed,
                    )?;
                    insert_property(
                        ctx,
                        properties,
                        format_args!("{segment_key}.full_domain"),
                        Joined(segment.domain.iter(), ","),
                    )?;
                    insert_property(
                        ctx,
                        properties,
                        format_args!("{segment_key}.sub_domain"),
                        Joined(segment.reference.sub_domain.iter(), ","),
                    )?;
                    insert_property(
                        ctx,
                        properties,
                        format_args!("{segment_key}.proxy_domain"),
                        Joined(segment.proxy_domain.iter(), ","),
                    )?;
                    if let Some(domains) = &segment.reference.domains {
                        insert_property(
                            ctx,
                            properties,
                            format_args!("{segment_key}.edge_domain"),
                            Joined(domains.edge.iter(), ","),
                        )?;
                        insert_property(
                            ctx,
                            properties,
                            format_args!("{segment_key}.trim_domain"),
                            Joined(domains.trim.iter(), ","),
                        )?;
                    }
                }
            }
        }
        Value::SubdEdgeChains(values) => {
            insert_property(ctx, properties, format_args!("{key}.count"), values.len())?;
            for (index, chain) in values.iter().enumerate() {
                let chain_key = crate::wire::admitted_format(
                    ctx,
                    format_args!("{key}.{index}"),
                    "Rhino history chain prefix",
                )?;
                insert_property(
                    ctx,
                    properties,
                    format_args!("{chain_key}.subd_id"),
                    chain.subd_id,
                )?;
                insert_property(
                    ctx,
                    properties,
                    format_args!("{chain_key}.edge_ids"),
                    Joined(chain.edges.iter().map(|edge| edge.id), ","),
                )?;
                insert_property(
                    ctx,
                    properties,
                    format_args!("{chain_key}.orientations"),
                    Joined(chain.edges.iter().map(|edge| u8::from(edge.reversed)), ","),
                )?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Projects source history into ordered neutral native operations.
/// Projects history records into native features and carrier geometry.
///
/// Returns counts for untyped values, failed geometry, later dependencies, and
/// repaired optional geometry channels. The caller reports these counts.
pub(crate) fn project(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    records: &[HistoryRecord],
    geometry_context: Option<(
        crate::mesh::MeshExpand<'_>,
        ArchiveVersion,
        Option<i64>,
        MillimeterScale,
    )>,
    ir: &mut cadmpeg_ir::document::CadIr,
    warnings: &mut Diagnostics,
) -> Result<(usize, usize, usize, usize), ProjectionError> {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};

    let mut sink = GeometrySink {
        warnings,
        untyped: 0,
        failed: 0,
        redundant_repairs: 0,
        refusal: None,
    };
    let mut ids = admitted_vec(ctx, records.len(), "Rhino history feature ids")
        .map_err(history_resource_error)?;
    let mut native_ids = admitted_vec(ctx, records.len(), "Rhino history native ids")
        .map_err(history_resource_error)?;
    let mut seen_record_ids = HashSet::new();
    crate::wire::reserve_hash_set(
        ctx,
        &mut seen_record_ids,
        records.len(),
        "Rhino history record identities",
    )
    .map_err(ProjectionError::Codec)?;
    for record in records {
        let unique = !record.id.is_nil() && seen_record_ids.insert(record.id);
        let key = if unique {
            crate::wire::admitted_format(
                ctx,
                format_args!("{}", record.id),
                "Rhino history identity key",
            )
        } else {
            crate::wire::admitted_format(
                ctx,
                format_args!("offset-{}", record.source_range.start),
                "Rhino history identity key",
            )
        }
        .map_err(ProjectionError::Codec)?;
        let feature_id = crate::wire::admitted_format(
            ctx,
            format_args!("rhino:history:feature#{key}"),
            "Rhino history feature identity",
        )
        .map_err(ProjectionError::Codec)?;
        ids.push(FeatureId::mint(feature_id).map_err(|error| error.to_string())?);
        native_ids.push(
            crate::wire::admitted_format(
                ctx,
                format_args!("rhino:history:record#{key}"),
                "Rhino history native identity",
            )
            .map_err(ProjectionError::Codec)?,
        );
    }
    let mut producers = HashMap::<Uuid, Option<(usize, FeatureId)>>::new();
    for (index, record) in records.iter().enumerate() {
        let mut record_descendants = HashSet::new();
        crate::wire::reserve_hash_set(
            ctx,
            &mut record_descendants,
            record.descendants.len(),
            "Rhino history unique descendants",
        )
        .map_err(ProjectionError::Codec)?;
        for descendant in &record.descendants {
            if !record_descendants.insert(*descendant) {
                continue;
            }
            if descendant.is_nil() {
                continue;
            }
            if let Some(producer) = producers.get_mut(descendant) {
                *producer = None;
            } else {
                crate::wire::reserve_hash_map(ctx, &mut producers, 1, "Rhino history producers")
                    .map_err(ProjectionError::Codec)?;
                ctx.charge_retained(
                    cadmpeg_core::decode::u64_from_index(ids[index].as_str().len()),
                    "Rhino history producer identity",
                )
                .map_err(ProjectionError::Codec)?;
                producers.insert(*descendant, Some((index, ids[index].clone())));
            }
        }
    }
    let mut dropped_dependencies = 0;
    for (index, record) in records.iter().enumerate() {
        for antecedent in &record.antecedents {
            match producers.get(antecedent) {
                Some(None) => dropped_dependencies += 1,
                Some(Some((producer_index, _))) if *producer_index >= index => {
                    dropped_dependencies += 1;
                }
                _ => {}
            }
        }
        let mut dependency_seen = HashSet::new();
        crate::wire::reserve_hash_set(
            ctx,
            &mut dependency_seen,
            record.antecedents.len(),
            "Rhino history seen dependencies",
        )
        .map_err(ProjectionError::Codec)?;
        let mut dependencies =
            admitted_vec(ctx, record.antecedents.len(), "Rhino history dependencies")
                .map_err(history_resource_error)?;
        for antecedent in &record.antecedents {
            let Some((producer_index, id)) = producers.get(antecedent).and_then(Option::as_ref)
            else {
                continue;
            };
            if *producer_index >= index || dependency_seen.contains(id) {
                continue;
            }
            let id_bytes = cadmpeg_core::decode::u64_from_index(id.as_str().len());
            ctx.charge_retained(id_bytes, "Rhino history seen dependency identity")
                .map_err(ProjectionError::Codec)?;
            dependency_seen.insert(id.clone());
            ctx.charge_retained(id_bytes, "Rhino history dependency identity")
                .map_err(ProjectionError::Codec)?;
            dependencies.push(id.clone());
        }
        let mut parameters = BTreeMap::new();
        let mut properties = BTreeMap::new();
        let mut value_occurrences = HashMap::<i32, usize>::new();
        crate::wire::reserve_hash_map(
            ctx,
            &mut value_occurrences,
            record.values.len(),
            "Rhino history value occurrences",
        )
        .map_err(ProjectionError::Codec)?;
        for value in &record.values {
            let occurrence = value_occurrences.entry(value.id).or_default();
            let key = if *occurrence == 0 {
                crate::wire::admitted_format(
                    ctx,
                    format_args!("value_{}", value.id),
                    "Rhino history value key",
                )
            } else {
                crate::wire::admitted_format(
                    ctx,
                    format_args!("value_{}_{}", value.id, occurrence),
                    "Rhino history value key",
                )
            }
            .map_err(ProjectionError::Codec)?;
            *occurrence += 1;
            structured_value_properties(
                ctx,
                &key,
                &value.value,
                geometry_context,
                &mut properties,
                &mut sink,
            )
            .map_err(ProjectionError::Codec)?;
            if let Some(error) = sink.refusal.take() {
                return Err(ProjectionError::Codec(error));
            }
            if geometry_context.is_none()
                && matches!(&value.value, Value::Geometries(values) if !values.is_empty())
            {
                insert_property(
                    ctx,
                    &mut properties,
                    format_args!("{key}.source_fidelity_id"),
                    format_args!("rhino:history:source#{:012}", record.source_range.start),
                )
                .map_err(ProjectionError::Codec)?;
            }
            if let Some(text) = value_text(ctx, &value.value).map_err(ProjectionError::Codec)? {
                ctx.charge_collection_items(1, "Rhino history parameter entries")
                    .map_err(ProjectionError::Codec)?;
                parameters.insert(key, text);
            } else if let Value::Opaque { type_code, range } = &value.value {
                insert_property(ctx, &mut properties, format_args!("{key}.type"), type_code)
                    .map_err(ProjectionError::Codec)?;
                insert_property(
                    ctx,
                    &mut properties,
                    format_args!("{key}.source_range"),
                    format_args!("{}..{}", range.start, range.end),
                )
                .map_err(ProjectionError::Codec)?;
            }
        }
        insert_property(ctx, &mut properties, "record_version", record.version)
            .map_err(ProjectionError::Codec)?;
        insert_property(
            ctx,
            &mut properties,
            "record_type",
            match record.record_type {
                RecordType::HistoryParameters => "history_parameters",
                RecordType::FeatureParameters => "feature_parameters",
            },
        )
        .map_err(ProjectionError::Codec)?;
        insert_property(
            ctx,
            &mut properties,
            "copy_on_replace",
            record.copy_on_replace,
        )
        .map_err(ProjectionError::Codec)?;
        insert_property(
            ctx,
            &mut properties,
            "antecedent_objects",
            Joined(record.antecedents.iter(), ","),
        )
        .map_err(ProjectionError::Codec)?;
        insert_property(
            ctx,
            &mut properties,
            "descendant_objects",
            Joined(record.descendants.iter(), ","),
        )
        .map_err(ProjectionError::Codec)?;
        let properties =
            admitted_named_properties(ctx, &native_ids[index], properties, sink.warnings)
                .map_err(ProjectionError::Codec)?;
        let parameters =
            admitted_named_properties(ctx, &native_ids[index], parameters, sink.warnings)
                .map_err(ProjectionError::Codec)?;
        let dependency_count = cadmpeg_core::decode::u64_from_index(dependencies.len());
        let comparison_work = dependency_count
            .checked_mul(dependency_count)
            .ok_or_else(|| {
                ProjectionError::Codec(ctx.refuse_codec_limit(
                    "Rhino history dependency distinctness",
                    u64::MAX - 1,
                    u64::MAX,
                ))
            })?;
        ctx.charge_work(comparison_work, "Rhino history dependency distinctness")
            .map_err(ProjectionError::Codec)?;
        let dependencies = cadmpeg_ir::features::DistinctMembers::try_from_unique_vec(dependencies)
            .map_err(std::string::ToString::to_string)?;
        crate::wire::reserve_collection(
            ctx,
            &mut ir.model.features,
            1,
            "Rhino history projected features",
        )
        .map_err(ProjectionError::Codec)?;
        let feature_id_text = crate::wire::copy_retained_string(
            ctx,
            ids[index].as_str(),
            "Rhino history projected feature identity",
        )
        .map_err(ProjectionError::Codec)?;
        let feature_id = FeatureId::mint(feature_id_text).map_err(|error| error.to_string())?;
        let source_tag = crate::wire::copy_retained_string(
            ctx,
            "HistoryRecord",
            "Rhino history feature source tag",
        )
        .map_err(ProjectionError::Codec)?;
        let kind = crate::wire::admitted_format(
            ctx,
            format_args!("{}", record.command_id),
            "Rhino history feature kind",
        )
        .map_err(ProjectionError::Codec)?;
        let native_ref = crate::wire::copy_retained_string(
            ctx,
            &native_ids[index],
            "Rhino history feature native reference",
        )
        .map_err(ProjectionError::Codec)?;
        ir.model.features.push(Feature {
            id: feature_id,
            ordinal: u64::try_from(index)
                .map_err(|_| "history source order exceeds u64".to_string())?,
            name: None,
            suppressed: Some(false),
            dependencies,
            source_properties: properties,
            source_tag: Some(source_tag),
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::Native {
                    kind: kind.into(),
                    parameters,
                }),
            ),
            native_ref: Some(native_ref),
        });
    }
    for record in records {
        u64::try_from(record.source_range.start)
            .map_err(|_| "history source offset exceeds u64".to_string())?;
    }
    let native = records
        .iter()
        .enumerate()
        .map(|(index, record)| NativeHistoryRecord {
            id: &native_ids[index],
            source_offset: cadmpeg_core::decode::u64_from_index(record.source_range.start),
            source_uuid: (!record.id.is_nil()).then_some(UuidText(&record.id)),
            command_uuid: UuidText(&record.command_id),
            record_version: record.version,
            record_type: match record.record_type {
                RecordType::HistoryParameters => "history_parameters",
                RecordType::FeatureParameters => "feature_parameters",
            },
            copy_on_replace: record.copy_on_replace,
            antecedent_object_uuids: UuidList(&record.antecedents),
            descendant_object_uuids: UuidList(&record.descendants),
            value_count: record.values.len(),
        });
    ir.native
        .namespace_mut("rhino")
        .set_arena_from(ctx, "history_records", native)
        .map_err(|error| {
            let detail = error.to_string();
            match cadmpeg_core::CodecError::from(error) {
                resource @ cadmpeg_core::CodecError::ResourceLimit(_) => {
                    ProjectionError::Codec(resource)
                }
                _ => ProjectionError::Admission(detail),
            }
        })?;
    Ok((
        sink.untyped,
        sink.failed,
        dropped_dependencies,
        sink.redundant_repairs,
    ))
}

#[cfg(test)]
pub(crate) mod tests;
