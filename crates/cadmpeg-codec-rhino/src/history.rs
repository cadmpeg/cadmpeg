// SPDX-License-Identifier: Apache-2.0
//! Built-in history-record decoding.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;

use crate::chunks::{checked_count_bytes, chunk_at, ArchiveVersion, BoundedReader, FramingError};
use crate::container::Record;
use crate::objects::{parse_class_wrapper, parse_class_wrapper_with_userdata, UserdataDescriptor};
use crate::settings::{point, utf16, vector, xform, Point3, Vector3, Xform};
use crate::wire::Uuid;

const HISTORY_RECORD: u32 = 0x2000_807b;
const ANONYMOUS: u32 = 0x4000_8000;
const HISTORY_CLASS: Uuid = Uuid::from_canonical([
    0xec, 0xd0, 0xfd, 0x2f, 0x20, 0x88, 0x49, 0xdc, 0x96, 0x41, 0x9c, 0xf7, 0xa2, 0x8f, 0xfa, 0x6b,
]);
const VALUE_CAP: usize = 1 << 20;

/// Semantic role of a history record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecordType {
    HistoryParameters,
    FeatureParameters,
}

/// One bounded history parameter value.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HistoryValue {
    pub(crate) id: i32,
    pub(crate) value: Value,
}

/// Built-in history parameter families.
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
    PolyEdges(Vec<PolyEdge>),
    SubdEdgeChains(Vec<SubdEdgeChain>),
    Opaque { type_code: i32, range: Range<usize> },
}

/// One polymorphic geometry object embedded in a history parameter.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EmbeddedGeometry {
    pub(crate) class_id: Uuid,
    pub(crate) class_data_range: Range<usize>,
    pub(crate) userdata: Vec<UserdataDescriptor>,
}

/// Persistent construction data for one polyedge.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PolyEdge {
    pub(crate) segments: Vec<CurveProxy>,
    pub(crate) parameters: Vec<f64>,
    pub(crate) evaluation_mode: i32,
}

/// Persistent construction data for one curve-proxy segment.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CurveProxy {
    pub(crate) curve: ObjectReference,
    pub(crate) reversed: bool,
    pub(crate) full_domain: [f64; 2],
    pub(crate) sub_domain: [f64; 2],
    pub(crate) proxy_domain: [f64; 2],
    pub(crate) edge_domain: Option<[f64; 2]>,
    pub(crate) trim_domain: Option<[f64; 2]>,
}

/// Persistent edge sequence on a `SubD` object.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SubdEdgeChain {
    pub(crate) subd_id: Uuid,
    pub(crate) edge_ids: Vec<u32>,
    pub(crate) orientations: Vec<u8>,
}

/// Persistent object selection stored in a history value.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ObjectReference {
    pub(crate) object_id: Uuid,
    pub(crate) component: [i32; 2],
    pub(crate) geometry_type: i32,
    pub(crate) point: Point3,
    pub(crate) evaluation: EvaluationParameter,
    pub(crate) instance_path: Vec<InstanceReference>,
    pub(crate) osnap_mode: i32,
}

/// Evaluation location attached to a persistent object selection.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EvaluationParameter {
    pub(crate) parameter_type: i32,
    pub(crate) component: [i32; 2],
    pub(crate) parameters: [f64; 4],
    pub(crate) intervals: [[f64; 2]; 3],
}

/// One nested instance-definition step in an object selection.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct InstanceReference {
    pub(crate) reference_id: Uuid,
    pub(crate) transform: Xform,
    pub(crate) definition_id: Uuid,
    pub(crate) geometry_index: i32,
    pub(crate) component: Option<[i32; 2]>,
    pub(crate) evaluation: Option<EvaluationParameter>,
}

/// A complete built-in history record.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HistoryRecord {
    pub(crate) source_range: Range<usize>,
    pub(crate) id: Uuid,
    pub(crate) version: i32,
    pub(crate) command_id: Uuid,
    pub(crate) descendants: Vec<Uuid>,
    pub(crate) antecedents: Vec<Uuid>,
    pub(crate) values: Vec<HistoryValue>,
    pub(crate) record_type: RecordType,
    pub(crate) copy_on_replace: bool,
}

fn structural(offset: usize, message: impl Into<String>) -> FramingError {
    FramingError::Structural {
        offset,
        message: message.into(),
    }
}

fn uuid(reader: &mut BoundedReader<'_>) -> Result<Uuid, FramingError> {
    Ok(Uuid::from_wire(reader.array()?))
}

fn anonymous(
    bytes: &[u8],
    offset: usize,
    end: usize,
    archive: ArchiveVersion,
) -> Result<(BoundedReader<'_>, usize, i32), FramingError> {
    let chunk = chunk_at(bytes, offset, end, archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short {
        return Err(structural(offset, "expected long anonymous chunk"));
    }
    let mut reader = BoundedReader::new(bytes, chunk.body.start, chunk.body.end)?;
    let major = reader.i32()?;
    let minor = reader.i32()?;
    if major != 1 {
        return Err(structural(
            chunk.body.start,
            "unsupported anonymous major version",
        ));
    }
    Ok((reader, chunk.next_offset, minor))
}

fn count(reader: &mut BoundedReader<'_>, element_size: usize) -> Result<usize, FramingError> {
    let offset = reader.position();
    let value = reader.i32()?;
    checked_count_bytes(value, element_size, reader.remaining(), VALUE_CAP, offset)?;
    usize::try_from(value).map_err(|_| FramingError::Overflow { offset })
}

fn uuid_list(
    bytes: &[u8],
    offset: usize,
    end: usize,
    archive: ArchiveVersion,
) -> Result<(Vec<Uuid>, usize), FramingError> {
    let (mut reader, next, _) = anonymous(bytes, offset, end, archive)?;
    let count = count(&mut reader, 16)?;
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        values.push(uuid(&mut reader)?);
    }
    if reader.remaining() != 0 {
        return Err(structural(
            reader.position(),
            "UUID list has trailing bytes",
        ));
    }
    Ok((values, next))
}

fn array<T>(
    reader: &mut BoundedReader<'_>,
    element_size: usize,
    mut read: impl FnMut(&mut BoundedReader<'_>) -> Result<T, FramingError>,
) -> Result<Vec<T>, FramingError> {
    let count = count(reader, element_size)?;
    let mut values = Vec::with_capacity(count);
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
    let mut intervals = [[0.0; 2]; 3];
    for value in intervals.iter_mut().take(interval_count) {
        *value = interval(reader)?;
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
    if !(0..=1).contains(&minor) {
        return Err(structural(
            reader.position(),
            "unsupported instance-reference path version",
        ));
    }
    let reference_id = uuid(&mut reader)?;
    let transform = xform(&mut reader)?;
    let definition_id = uuid(&mut reader)?;
    let geometry_index = reader.i32()?;
    let (component, evaluation) = if minor >= 1 {
        let component = component(&mut reader)?;
        let (mut nested, nested_next, nested_minor) =
            anonymous(bytes, reader.position(), reader.end(), archive)?;
        if nested_minor != 0 {
            return Err(structural(
                nested.position(),
                "unsupported object-evaluation version",
            ));
        }
        let evaluation = evaluation(&mut nested, 3)?;
        if nested.remaining() != 0 {
            return Err(structural(
                nested.position(),
                "object evaluation has trailing bytes",
            ));
        }
        reader.skip(nested_next - reader.position())?;
        (Some(component), Some(evaluation))
    } else {
        (None, None)
    };
    if reader.remaining() != 0 {
        return Err(structural(
            reader.position(),
            "instance-reference path has trailing bytes",
        ));
    }
    Ok((
        InstanceReference {
            reference_id,
            transform,
            definition_id,
            geometry_index,
            component,
            evaluation,
        },
        next,
    ))
}

fn object_reference(
    bytes: &[u8],
    offset: usize,
    end: usize,
    archive: ArchiveVersion,
) -> Result<(ObjectReference, usize), FramingError> {
    let (mut reader, next, minor) = anonymous(bytes, offset, end, archive)?;
    if !(0..=3).contains(&minor) {
        return Err(structural(
            reader.position(),
            "unsupported object-reference version",
        ));
    }
    let object_id = uuid(&mut reader)?;
    let component = component(&mut reader)?;
    let geometry_type = reader.i32()?;
    let point = point(&mut reader)?;
    let mut evaluation = evaluation(&mut reader, 0)?;
    let path_count = count(&mut reader, 1)?;
    let mut instance_path = Vec::new();
    for _ in 0..path_count {
        let (value, value_next) =
            instance_reference(bytes, reader.position(), reader.end(), archive)?;
        reader.skip(value_next - reader.position())?;
        instance_path.push(value);
    }
    if minor >= 1 {
        evaluation.intervals[0] = interval(&mut reader)?;
        evaluation.intervals[1] = interval(&mut reader)?;
    }
    if minor >= 2 {
        evaluation.intervals[2] = interval(&mut reader)?;
    }
    let osnap_mode = if minor >= 3 { reader.i32()? } else { 0 };
    if reader.remaining() != 0 {
        return Err(structural(
            reader.position(),
            "object reference has trailing bytes",
        ));
    }
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
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<Vec<ObjectReference>, FramingError> {
    let count = count(reader, 1)?;
    let mut values = Vec::new();
    for _ in 0..count {
        let (value, next) = object_reference(
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
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<Vec<EmbeddedGeometry>, FramingError> {
    let (mut nested, next, minor) = anonymous(
        reader.backing_bytes(),
        reader.position(),
        reader.end(),
        archive,
    )?;
    if minor != 0 {
        return Err(structural(
            nested.position(),
            "unsupported geometry-value version",
        ));
    }
    let count = count(&mut nested, 1)?;
    let mut values = Vec::new();
    for _ in 0..count {
        let start = nested.position();
        let wrapper = chunk_at(nested.backing_bytes(), start, nested.end(), archive, false)?;
        let mut warnings = Vec::new();
        let (class, userdata) = parse_class_wrapper_with_userdata(
            nested.backing_bytes(),
            start..wrapper.next_offset,
            archive,
            &mut warnings,
        )?;
        nested.skip(wrapper.next_offset - start)?;
        values.push(EmbeddedGeometry {
            class_id: class.class_uuid,
            class_data_range: class.class_data_range,
            userdata,
        });
    }
    if nested.remaining() != 0 {
        return Err(structural(
            nested.position(),
            "geometry value has trailing bytes",
        ));
    }
    reader.skip(next - reader.position())?;
    Ok(values)
}

fn curve_proxy(
    bytes: &[u8],
    offset: usize,
    end: usize,
    archive: ArchiveVersion,
) -> Result<(CurveProxy, usize), FramingError> {
    let (mut reader, next, minor) = anonymous(bytes, offset, end, archive)?;
    if !(0..=1).contains(&minor) {
        return Err(structural(
            reader.position(),
            "unsupported curve-proxy version",
        ));
    }
    let (curve, curve_next) = object_reference(bytes, reader.position(), reader.end(), archive)?;
    reader.skip(curve_next - reader.position())?;
    let reversed = reader.bool()?;
    let full_domain = interval(&mut reader)?;
    let sub_domain = interval(&mut reader)?;
    let proxy_domain = interval(&mut reader)?;
    let (edge_domain, trim_domain) = if minor >= 1 {
        (Some(interval(&mut reader)?), Some(interval(&mut reader)?))
    } else {
        (None, None)
    };
    if reader.remaining() != 0 {
        return Err(structural(
            reader.position(),
            "curve proxy has trailing bytes",
        ));
    }
    Ok((
        CurveProxy {
            curve,
            reversed,
            full_domain,
            sub_domain,
            proxy_domain,
            edge_domain,
            trim_domain,
        },
        next,
    ))
}

fn poly_edge(
    bytes: &[u8],
    offset: usize,
    end: usize,
    archive: ArchiveVersion,
) -> Result<(PolyEdge, usize), FramingError> {
    let (mut reader, next, minor) = anonymous(bytes, offset, end, archive)?;
    if minor != 0 {
        return Err(structural(
            reader.position(),
            "unsupported polyedge version",
        ));
    }
    let segment_count = count(&mut reader, 1)?;
    let mut segments = Vec::new();
    for _ in 0..segment_count {
        let (segment, segment_next) = curve_proxy(bytes, reader.position(), reader.end(), archive)?;
        reader.skip(segment_next - reader.position())?;
        segments.push(segment);
    }
    let parameters = array(&mut reader, 8, read_f64)?;
    let evaluation_mode = reader.i32()?;
    if reader.remaining() != 0 {
        return Err(structural(reader.position(), "polyedge has trailing bytes"));
    }
    Ok((
        PolyEdge {
            segments,
            parameters,
            evaluation_mode,
        },
        next,
    ))
}

fn poly_edges(
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<Vec<PolyEdge>, FramingError> {
    let (mut nested, next, minor) = anonymous(
        reader.backing_bytes(),
        reader.position(),
        reader.end(),
        archive,
    )?;
    if minor != 0 {
        return Err(structural(
            nested.position(),
            "unsupported polyedge-value version",
        ));
    }
    let count = count(&mut nested, 1)?;
    let mut values = Vec::new();
    for _ in 0..count {
        let (value, value_next) = poly_edge(
            nested.backing_bytes(),
            nested.position(),
            nested.end(),
            archive,
        )?;
        nested.skip(value_next - nested.position())?;
        values.push(value);
    }
    if nested.remaining() != 0 {
        return Err(structural(
            nested.position(),
            "polyedge value has trailing bytes",
        ));
    }
    reader.skip(next - reader.position())?;
    Ok(values)
}

fn subd_edge_chain(
    bytes: &[u8],
    offset: usize,
    end: usize,
    archive: ArchiveVersion,
) -> Result<(SubdEdgeChain, usize), FramingError> {
    let (mut reader, next, minor) = anonymous(bytes, offset, end, archive)?;
    if minor != 0 {
        return Err(structural(
            reader.position(),
            "unsupported SubD edge-chain version",
        ));
    }
    let subd_id = uuid(&mut reader)?;
    let count = count(&mut reader, 1)?;
    let edge_ids = array(&mut reader, 4, read_u32)?;
    let orientations = array(&mut reader, 1, read_u8)?;
    if edge_ids.len() != count || orientations.len() != count {
        return Err(structural(
            reader.position(),
            "SubD edge-chain array counts disagree",
        ));
    }
    if orientations.iter().any(|orientation| *orientation > 1) {
        return Err(structural(
            reader.position(),
            "invalid SubD edge orientation",
        ));
    }
    if reader.remaining() != 0 {
        return Err(structural(
            reader.position(),
            "SubD edge chain has trailing bytes",
        ));
    }
    Ok((
        SubdEdgeChain {
            subd_id,
            edge_ids,
            orientations,
        },
        next,
    ))
}

fn subd_edge_chains(
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<Vec<SubdEdgeChain>, FramingError> {
    let (mut nested, next, minor) = anonymous(
        reader.backing_bytes(),
        reader.position(),
        reader.end(),
        archive,
    )?;
    if minor != 0 {
        return Err(structural(
            nested.position(),
            "unsupported SubD edge-chain list version",
        ));
    }
    let count = count(&mut nested, 1)?;
    let mut values = Vec::new();
    for _ in 0..count {
        let (value, value_next) = subd_edge_chain(
            nested.backing_bytes(),
            nested.position(),
            nested.end(),
            archive,
        )?;
        nested.skip(value_next - nested.position())?;
        values.push(value);
    }
    if nested.remaining() != 0 {
        return Err(structural(
            nested.position(),
            "SubD edge-chain value has trailing bytes",
        ));
    }
    reader.skip(next - reader.position())?;
    Ok(values)
}

fn parse_value(
    bytes: &[u8],
    offset: usize,
    end: usize,
    archive: ArchiveVersion,
) -> Result<(HistoryValue, usize), FramingError> {
    let (mut reader, next, minor) = anonymous(bytes, offset, end, archive)?;
    if minor != 0 {
        return Err(structural(
            reader.position(),
            "unsupported history-value version",
        ));
    }
    let type_code = reader.i32()?;
    let id = reader.i32()?;
    let payload = reader.position()..reader.end();
    let value = match type_code {
        0 => Value::None,
        1 => Value::Booleans(array(&mut reader, 1, read_bool)?),
        2 => Value::Integers(array(&mut reader, 4, read_i32)?),
        3 => Value::Doubles(array(&mut reader, 8, read_f64)?),
        4 => Value::Colors(array(&mut reader, 4, read_color)?),
        5 => Value::Points(array(&mut reader, 24, point)?),
        6 => Value::Vectors(array(&mut reader, 24, vector)?),
        7 => Value::Transforms(array(&mut reader, 128, xform)?),
        8 => Value::Strings(array(&mut reader, 4, utf16)?),
        9 => Value::ObjectReferences(object_references(&mut reader, archive)?),
        10 => Value::Geometries(geometries(&mut reader, archive)?),
        11 => Value::Uuids(array(&mut reader, 16, uuid)?),
        13 => Value::PolyEdges(poly_edges(&mut reader, archive)?),
        14 => Value::SubdEdgeChains(subd_edge_chains(&mut reader, archive)?),
        _ => {
            reader.skip(reader.remaining())?;
            Value::Opaque {
                type_code,
                range: payload,
            }
        }
    };
    if reader.remaining() != 0 {
        return Err(structural(
            reader.position(),
            "history value has trailing bytes",
        ));
    }
    Ok((HistoryValue { id, value }, next))
}

fn read_bool(reader: &mut BoundedReader<'_>) -> Result<bool, FramingError> {
    reader.bool()
}

fn read_i32(reader: &mut BoundedReader<'_>) -> Result<i32, FramingError> {
    reader.i32()
}

fn read_u32(reader: &mut BoundedReader<'_>) -> Result<u32, FramingError> {
    reader.u32()
}

fn read_u8(reader: &mut BoundedReader<'_>) -> Result<u8, FramingError> {
    reader.u8()
}

fn read_f64(reader: &mut BoundedReader<'_>) -> Result<f64, FramingError> {
    reader.f64()
}

fn read_color(reader: &mut BoundedReader<'_>) -> Result<[u8; 4], FramingError> {
    reader.array()
}

fn parse_record(
    bytes: &[u8],
    record: &Record,
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Result<HistoryRecord, FramingError> {
    if record.typecode != HISTORY_RECORD || record.short {
        return Err(structural(
            record.range.start,
            "invalid history table record",
        ));
    }
    let class = parse_class_wrapper(bytes, record.body.clone(), archive, warnings)?;
    if class.class_uuid != HISTORY_CLASS {
        return Err(structural(
            record.body.start,
            format!("history record has class {}", class.class_uuid),
        ));
    }
    let (mut reader, next, minor) = anonymous(
        bytes,
        class.class_data_range.start,
        class.class_data_range.end,
        archive,
    )?;
    if next != class.class_data_range.end || !(0..=2).contains(&minor) {
        return Err(structural(
            reader.position(),
            "unsupported history-record version",
        ));
    }
    let id = uuid(&mut reader)?;
    let version = reader.i32()?;
    let command_id = uuid(&mut reader)?;
    let (descendants, next) = uuid_list(bytes, reader.position(), reader.end(), archive)?;
    reader.skip(next - reader.position())?;
    let (antecedents, next) = uuid_list(bytes, reader.position(), reader.end(), archive)?;
    reader.skip(next - reader.position())?;
    let (mut values_reader, next, values_minor) =
        anonymous(bytes, reader.position(), reader.end(), archive)?;
    if values_minor != 0 {
        return Err(structural(
            values_reader.position(),
            "unsupported history-values version",
        ));
    }
    let value_count = count(&mut values_reader, 1)?;
    let mut values = Vec::new();
    for _ in 0..value_count {
        let (value, value_next) = parse_value(
            bytes,
            values_reader.position(),
            values_reader.end(),
            archive,
        )?;
        values_reader.skip(value_next - values_reader.position())?;
        values.push(value);
    }
    if values_reader.remaining() != 0 {
        return Err(structural(
            values_reader.position(),
            "history-values chunk has trailing bytes",
        ));
    }
    reader.skip(next - reader.position())?;
    let record_type = if minor >= 1 {
        match reader.i32()? {
            0 => RecordType::HistoryParameters,
            1 => RecordType::FeatureParameters,
            value => {
                return Err(structural(
                    reader.position() - 4,
                    format!("invalid history record type {value}"),
                ))
            }
        }
    } else {
        RecordType::HistoryParameters
    };
    let copy_on_replace = minor >= 2 && reader.bool()?;
    if reader.remaining() != 0 {
        return Err(structural(
            reader.position(),
            "history record has trailing bytes",
        ));
    }
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
    bytes: &[u8],
    records: &[Record],
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Vec<HistoryRecord> {
    records
        .iter()
        .filter_map(
            |record| match parse_record(bytes, record, archive, warnings) {
                Ok(value) => Some(value),
                Err(error) => {
                    warnings.push(format!(
                        "history record at {} degraded: {error}",
                        record.range.start
                    ));
                    None
                }
            },
        )
        .collect()
}

fn list<T: ToString>(values: &[T]) -> String {
    values
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

fn value_text(value: &Value) -> Option<String> {
    Some(match value {
        Value::None => String::new(),
        Value::Booleans(values) => list(values),
        Value::Integers(values) => list(values),
        Value::Doubles(values) => list(values),
        Value::Colors(values) => values
            .iter()
            .map(|value| {
                value
                    .iter()
                    .map(u8::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .collect::<Vec<_>>()
            .join(";"),
        Value::Points(values) => values
            .iter()
            .map(|value| list(&value.0))
            .collect::<Vec<_>>()
            .join(";"),
        Value::Vectors(values) => values
            .iter()
            .map(|value| list(&value.0))
            .collect::<Vec<_>>()
            .join(";"),
        Value::Transforms(values) => values
            .iter()
            .map(|value| list(&value.0))
            .collect::<Vec<_>>()
            .join(";"),
        Value::Strings(values) => values.join("\u{1f}"),
        Value::ObjectReferences(values) => values
            .iter()
            .map(|value| {
                format!(
                    "{}@{}:{}",
                    value.object_id, value.component[0], value.component[1]
                )
            })
            .collect::<Vec<_>>()
            .join(","),
        Value::Geometries(values) => values
            .iter()
            .map(|value| value.class_id.to_string())
            .collect::<Vec<_>>()
            .join(","),
        Value::Uuids(values) => list(values),
        Value::PolyEdges(_) | Value::SubdEdgeChains(_) => return None,
        Value::Opaque { .. } => return None,
    })
}

fn evaluation_properties(
    prefix: &str,
    value: &EvaluationParameter,
    properties: &mut BTreeMap<String, String>,
) {
    properties.insert(format!("{prefix}.type"), value.parameter_type.to_string());
    properties.insert(format!("{prefix}.component"), list(&value.component));
    properties.insert(format!("{prefix}.parameters"), list(&value.parameters));
    for (index, interval) in value.intervals.iter().enumerate() {
        properties.insert(format!("{prefix}.interval_{index}"), list(interval));
    }
}

fn object_reference_properties(
    prefix: &str,
    value: &ObjectReference,
    properties: &mut BTreeMap<String, String>,
) {
    properties.insert(format!("{prefix}.object_id"), value.object_id.to_string());
    properties.insert(format!("{prefix}.component"), list(&value.component));
    properties.insert(
        format!("{prefix}.geometry_type"),
        value.geometry_type.to_string(),
    );
    properties.insert(format!("{prefix}.point"), list(&value.point.0));
    properties.insert(format!("{prefix}.osnap_mode"), value.osnap_mode.to_string());
    evaluation_properties(
        &format!("{prefix}.evaluation"),
        &value.evaluation,
        properties,
    );
    properties.insert(
        format!("{prefix}.instance_count"),
        value.instance_path.len().to_string(),
    );
    for (index, instance) in value.instance_path.iter().enumerate() {
        let path = format!("{prefix}.instance_{index}");
        properties.insert(
            format!("{path}.reference_id"),
            instance.reference_id.to_string(),
        );
        properties.insert(format!("{path}.transform"), list(&instance.transform.0));
        properties.insert(
            format!("{path}.definition_id"),
            instance.definition_id.to_string(),
        );
        properties.insert(
            format!("{path}.geometry_index"),
            instance.geometry_index.to_string(),
        );
        if let Some(component) = instance.component {
            properties.insert(format!("{path}.component"), list(&component));
        }
        if let Some(evaluation) = &instance.evaluation {
            evaluation_properties(&format!("{path}.evaluation"), evaluation, properties);
        }
    }
}

fn cage_json(cage: &crate::cage::Cage) -> serde_json::Value {
    serde_json::json!({
        "kind": "nurbs_cage",
        "dimension": cage.dimension,
        "rational": cage.rational,
        "orders": cage.orders,
        "counts": cage.counts,
        "knots": cage.knots,
        "control_points": cage.control_points,
        "weights": cage.weights,
    })
}

fn extended_geometry_json(
    data: &[u8],
    value: &EmbeddedGeometry,
    archive: ArchiveVersion,
    writer_version: Option<i64>,
    scale: f64,
) -> Option<String> {
    let semantic = if crate::mesh::supported_class(value.class_id) {
        let mut budget = crate::mesh::MeshBudget::new();
        let mesh = crate::mesh::decode(
            data,
            value.class_data_range.clone(),
            archive,
            crate::mesh::MeshDecodeOptions {
                writer_version,
                association: None,
                id: "rhino:history:embedded-mesh".to_string(),
                scale,
            },
            &mut budget,
        )
        .ok()?;
        serde_json::json!({
            "kind": "mesh",
            "vertices": mesh.tessellation.vertices,
            "triangles": mesh.tessellation.triangles,
            "strip_lengths": mesh.tessellation.strip_lengths,
            "normals": mesh.tessellation.normals,
            "channels": mesh.tessellation.channels,
        })
    } else if crate::subd::supported_class(value.class_id) {
        let subd = crate::subd::decode(
            data,
            value.class_data_range.clone(),
            archive,
            scale,
            cadmpeg_ir::ids::SubdId("rhino:history:embedded-subd".to_string()),
        )
        .ok()?;
        match subd {
            crate::subd::DecodedSubd::Empty => serde_json::json!({
                "kind": "subd",
                "empty": true,
            }),
            crate::subd::DecodedSubd::Surface {
                surface,
                neutral_metadata,
                ..
            } => serde_json::json!({
                "kind": "subd",
                "surface": surface,
                "neutral_metadata": neutral_metadata,
            }),
        }
    } else if crate::extrusion::supported_class(value.class_id) {
        let mut budget = crate::mesh::MeshBudget::new();
        let extrusion = crate::extrusion::decode(
            data,
            value.class_data_range.clone(),
            archive,
            writer_version,
            scale,
            &mut budget,
        )
        .ok()?;
        let boundaries = extrusion
            .boundaries
            .iter()
            .map(|boundary| {
                serde_json::json!({
                    "start_curve": boundary.start_curve.geometry,
                    "start_nurbs": boundary.start_nurbs,
                    "end_nurbs": boundary.end_nurbs,
                    "start_pcurve": {
                        "degree": boundary.start_pcurve.degree,
                        "knots": boundary.start_pcurve.knots,
                        "control_points": boundary.start_pcurve.control_points,
                        "weights": boundary.start_pcurve.weights,
                        "periodic": boundary.start_pcurve.periodic,
                    },
                    "end_pcurve": {
                        "degree": boundary.end_pcurve.degree,
                        "knots": boundary.end_pcurve.knots,
                        "control_points": boundary.end_pcurve.control_points,
                        "weights": boundary.end_pcurve.weights,
                        "periodic": boundary.end_pcurve.periodic,
                    },
                })
            })
            .collect::<Vec<_>>();
        serde_json::json!({
            "kind": "extrusion",
            "boundaries": boundaries,
            "laterals": extrusion.laterals,
            "direction": extrusion.direction,
            "cap_origins": extrusion.cap_origins,
            "cap_normals": extrusion.cap_normals,
            "cap_u_axes": extrusion.cap_u_axes,
            "caps": extrusion.caps,
        })
    } else if value.class_id == crate::cage::CLASS {
        cage_json(&crate::cage::decode(data, value.class_data_range.clone(), scale, archive).ok()?)
    } else if value.class_id == crate::morph::CLASS {
        let morph =
            crate::morph::decode(data, value.class_data_range.clone(), scale, archive).ok()?;
        let control = match &morph.control {
            crate::morph::Control::Curve { start, end } => serde_json::json!({
                "kind": "curve",
                "start": start,
                "end": end,
            }),
            crate::morph::Control::Surface { start, end } => serde_json::json!({
                "kind": "surface",
                "start": start,
                "end": end,
            }),
            crate::morph::Control::Cage {
                start_transform,
                end,
            } => serde_json::json!({
                "kind": "cage",
                "start_transform": start_transform,
                "end": cage_json(end),
            }),
        };
        let localizers = morph
            .localizers
            .iter()
            .map(|localizer| {
                serde_json::json!({
                    "kind": localizer.kind,
                    "point": localizer.point,
                    "vector": localizer.vector,
                    "interval": localizer.interval,
                    "curve": localizer.curve,
                    "surface": localizer.surface,
                })
            })
            .collect::<Vec<_>>();
        serde_json::json!({
            "kind": "morph_control",
            "control": control,
            "captive_ids": morph.captive_ids.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "localizers": localizers,
            "tolerance": morph.tolerance,
            "quick_preview": morph.quick_preview,
            "preserve_structure": morph.preserve_structure,
        })
    } else if crate::brep::supported_class(value.class_id) {
        return crate::decode::embedded_brep_json(
            data,
            value.class_data_range.clone(),
            archive,
            writer_version,
            scale,
        );
    } else if value.class_id == crate::hatch::CLASS {
        let hatch =
            crate::hatch::decode(data, value.class_data_range.clone(), scale, archive).ok()?;
        let mut plane = hatch.plane;
        for coordinate in &mut plane.origin.0 {
            *coordinate *= scale;
        }
        plane.equation[3] *= scale;
        let loops = hatch
            .loops
            .iter()
            .map(|hatch_loop| {
                serde_json::json!({
                    "kind": match hatch_loop.kind {
                        crate::hatch::LoopKind::Outer => "outer",
                        crate::hatch::LoopKind::Inner => "inner",
                    },
                    "curve": hatch_loop.curve.geometry,
                })
            })
            .collect::<Vec<_>>();
        serde_json::json!({
            "kind": "hatch",
            "plane": {
                "origin": plane.origin.0,
                "xaxis": plane.xaxis.0,
                "yaxis": plane.yaxis.0,
                "zaxis": plane.zaxis.0,
                "equation": plane.equation,
            },
            "pattern_scale": hatch.pattern_scale,
            "pattern_rotation": hatch.pattern_rotation,
            "pattern_index": hatch.pattern_index,
            "loops": loops,
            "basepoint": hatch.basepoint,
        })
    } else if value.class_id == crate::detail::CLASS {
        let detail =
            crate::detail::decode(data, value.class_data_range.clone(), scale, archive).ok()?;
        serde_json::json!({
            "kind": "detail_view",
            "boundary": detail.boundary.geometry,
            "page_per_model_ratio": detail.page_per_model_ratio,
        })
    } else if crate::dimensions::supported_class(value.class_id) {
        let dimension = crate::dimensions::decode(
            data,
            value.class_id,
            value.class_data_range.clone(),
            scale,
            archive,
        )
        .ok()?;
        let mut dimension = dimension;
        crate::dimensions::apply_userdata(data, &value.userdata, archive, &mut dimension).ok()?;
        return crate::dimensions::semantic_json(&dimension);
    } else if value.class_id == crate::polyedge::CURVE_CLASS {
        let polyedge =
            crate::polyedge::decode(data, value.class_data_range.clone(), archive).ok()?;
        return crate::polyedge::semantic_json(&polyedge);
    } else {
        return None;
    };
    serde_json::to_string(&semantic).ok()
}

fn structured_value_properties(
    key: &str,
    value: &Value,
    geometry_context: Option<(&[u8], ArchiveVersion, Option<i64>, f64)>,
    properties: &mut BTreeMap<String, String>,
) {
    match value {
        Value::ObjectReferences(values) => {
            properties.insert(format!("{key}.count"), values.len().to_string());
            for (index, value) in values.iter().enumerate() {
                object_reference_properties(&format!("{key}.{index}"), value, properties);
            }
        }
        Value::Geometries(values) => {
            properties.insert(format!("{key}.count"), values.len().to_string());
            for (index, value) in values.iter().enumerate() {
                properties.insert(
                    format!("{key}.{index}.class_id"),
                    value.class_id.to_string(),
                );
                if let Some((data, archive, writer_version, scale)) = geometry_context {
                    if let Ok(decoded) = crate::curves::decode(
                        data,
                        value.class_id,
                        value.class_data_range.clone(),
                        scale,
                        archive,
                    ) {
                        let semantic = match decoded {
                            crate::curves::DecodedGeometry::Point { position, .. } => {
                                serde_json::to_string(&position)
                            }
                            crate::curves::DecodedGeometry::PointCloud(cloud) => {
                                serde_json::to_string(&cloud.points)
                            }
                            crate::curves::DecodedGeometry::Curve { curve } => {
                                serde_json::to_string(&curve.geometry)
                            }
                            crate::curves::DecodedGeometry::Surface { surface } => match surface {
                                crate::surfaces::DecodedSurface::Typed { geometry, .. } => {
                                    serde_json::to_string(&geometry)
                                }
                                crate::surfaces::DecodedSurface::Procedural {
                                    geometry, ..
                                } => serde_json::to_string(&geometry),
                            },
                        };
                        if let Ok(semantic) = semantic {
                            properties.insert(format!("{key}.{index}.geometry"), semantic);
                        }
                    } else if let Some(semantic) =
                        extended_geometry_json(data, value, archive, writer_version, scale)
                    {
                        properties.insert(format!("{key}.{index}.geometry"), semantic);
                    }
                }
            }
        }
        Value::PolyEdges(values) => {
            properties.insert(format!("{key}.count"), values.len().to_string());
            for (edge_index, edge) in values.iter().enumerate() {
                let edge_key = format!("{key}.{edge_index}");
                properties.insert(format!("{edge_key}.parameters"), list(&edge.parameters));
                properties.insert(
                    format!("{edge_key}.evaluation_mode"),
                    edge.evaluation_mode.to_string(),
                );
                properties.insert(
                    format!("{edge_key}.segment_count"),
                    edge.segments.len().to_string(),
                );
                for (segment_index, segment) in edge.segments.iter().enumerate() {
                    let segment_key = format!("{edge_key}.segment_{segment_index}");
                    object_reference_properties(
                        &format!("{segment_key}.curve"),
                        &segment.curve,
                        properties,
                    );
                    properties.insert(
                        format!("{segment_key}.reversed"),
                        segment.reversed.to_string(),
                    );
                    properties.insert(
                        format!("{segment_key}.full_domain"),
                        list(&segment.full_domain),
                    );
                    properties.insert(
                        format!("{segment_key}.sub_domain"),
                        list(&segment.sub_domain),
                    );
                    properties.insert(
                        format!("{segment_key}.proxy_domain"),
                        list(&segment.proxy_domain),
                    );
                    if let Some(domain) = segment.edge_domain {
                        properties.insert(format!("{segment_key}.edge_domain"), list(&domain));
                    }
                    if let Some(domain) = segment.trim_domain {
                        properties.insert(format!("{segment_key}.trim_domain"), list(&domain));
                    }
                }
            }
        }
        Value::SubdEdgeChains(values) => {
            properties.insert(format!("{key}.count"), values.len().to_string());
            for (index, chain) in values.iter().enumerate() {
                let chain_key = format!("{key}.{index}");
                properties.insert(format!("{chain_key}.subd_id"), chain.subd_id.to_string());
                properties.insert(format!("{chain_key}.edge_ids"), list(&chain.edge_ids));
                properties.insert(
                    format!("{chain_key}.orientations"),
                    list(&chain.orientations),
                );
            }
        }
        _ => {}
    }
}

/// Projects source history into ordered neutral native operations.
pub(crate) fn project(
    records: &[HistoryRecord],
    geometry_context: Option<(&[u8], ArchiveVersion, Option<i64>, f64)>,
    ir: &mut cadmpeg_ir::document::CadIr,
) {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId};

    #[derive(serde::Serialize)]
    struct NativeHistoryRecord {
        id: String,
        source_offset: u64,
        source_uuid: Option<String>,
        command_uuid: String,
        record_version: i32,
        record_type: &'static str,
        copy_on_replace: bool,
        antecedent_object_uuids: Vec<String>,
        descendant_object_uuids: Vec<String>,
        value_count: usize,
    }

    let mut ids = Vec::with_capacity(records.len());
    let mut native_ids = Vec::with_capacity(records.len());
    let mut seen_record_ids = HashSet::new();
    for record in records {
        let unique = !record.id.is_nil() && seen_record_ids.insert(record.id);
        let key = if unique {
            record.id.to_string()
        } else {
            format!("offset-{}", record.source_range.start)
        };
        ids.push(FeatureId(format!("rhino:history:feature#{key}")));
        native_ids.push(format!("rhino:history:record#{key}"));
    }
    let mut producers = HashMap::<Uuid, Option<(usize, FeatureId)>>::new();
    for (index, record) in records.iter().enumerate() {
        let mut record_descendants = HashSet::new();
        for descendant in record
            .descendants
            .iter()
            .filter(|descendant| record_descendants.insert(**descendant))
        {
            if descendant.is_nil() {
                continue;
            }
            producers
                .entry(*descendant)
                .and_modify(|producer| *producer = None)
                .or_insert_with(|| Some((index, ids[index].clone())));
        }
    }
    for (index, record) in records.iter().enumerate() {
        let mut dependency_seen = HashSet::new();
        let dependencies = record
            .antecedents
            .iter()
            .filter_map(|antecedent| producers.get(antecedent).and_then(Option::as_ref))
            .filter(|(producer_index, _)| *producer_index < index)
            .filter(|(_, id)| dependency_seen.insert((*id).clone()))
            .map(|(_, id)| id.clone())
            .collect();
        let mut parameters = BTreeMap::new();
        let mut properties = BTreeMap::new();
        let mut value_occurrences = HashMap::<i32, usize>::new();
        for value in &record.values {
            let occurrence = value_occurrences.entry(value.id).or_default();
            let key = if *occurrence == 0 {
                format!("value_{}", value.id)
            } else {
                format!("value_{}_{}", value.id, occurrence)
            };
            *occurrence += 1;
            structured_value_properties(&key, &value.value, geometry_context, &mut properties);
            if let Some(text) = value_text(&value.value) {
                parameters.insert(key, text);
            } else if let Value::Opaque { type_code, range } = &value.value {
                properties.insert(format!("{key}.type"), type_code.to_string());
                properties.insert(
                    format!("{key}.source_range"),
                    format!("{}..{}", range.start, range.end),
                );
            }
        }
        properties.insert("record_version".to_string(), record.version.to_string());
        properties.insert(
            "record_type".to_string(),
            match record.record_type {
                RecordType::HistoryParameters => "history_parameters",
                RecordType::FeatureParameters => "feature_parameters",
            }
            .to_string(),
        );
        properties.insert(
            "copy_on_replace".to_string(),
            record.copy_on_replace.to_string(),
        );
        properties.insert("antecedent_objects".to_string(), list(&record.antecedents));
        properties.insert("descendant_objects".to_string(), list(&record.descendants));
        ir.model.features.push(Feature {
            id: ids[index].clone(),
            ordinal: u64::try_from(index).expect("history source order fits u64"),
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies,
            source_properties: BTreeMap::new(),
            source_tag: Some("HistoryRecord".to_string()),
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Native {
                kind: record.command_id.to_string(),
                parameters,
                properties,
            },
            native_ref: Some(native_ids[index].clone()),
        });
    }
    let native = records
        .iter()
        .enumerate()
        .map(|(index, record)| NativeHistoryRecord {
            id: native_ids[index].clone(),
            source_offset: record.source_range.start as u64,
            source_uuid: (!record.id.is_nil()).then(|| record.id.to_string()),
            command_uuid: record.command_id.to_string(),
            record_version: record.version,
            record_type: match record.record_type {
                RecordType::HistoryParameters => "history_parameters",
                RecordType::FeatureParameters => "feature_parameters",
            },
            copy_on_replace: record.copy_on_replace,
            antecedent_object_uuids: record.antecedents.iter().map(ToString::to_string).collect(),
            descendant_object_uuids: record.descendants.iter().map(ToString::to_string).collect(),
            value_count: record.values.len(),
        })
        .collect::<Vec<_>>();
    ir.native
        .namespace_mut("rhino")
        .set_arena("history_records", &native)
        .expect("Rhino history records serialize");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anonymous_value(minor: i32, body: &[u8]) -> Vec<u8> {
        let mut payload = 1_i32.to_le_bytes().to_vec();
        payload.extend(minor.to_le_bytes());
        payload.extend(body);
        crate::archive_test_support::crc_chunk(ANONYMOUS, &payload)
    }

    fn value(type_code: i32, payload: &[u8]) -> Vec<u8> {
        let mut body = type_code.to_le_bytes().to_vec();
        body.extend(7_i32.to_le_bytes());
        body.extend(payload);
        anonymous_value(0, &body)
    }

    fn id(value: u8) -> Uuid {
        let mut bytes = [0; 16];
        bytes[15] = value;
        Uuid::from_canonical(bytes)
    }

    fn record(record_id: u8, command: u8, antecedents: &[u8], descendants: &[u8]) -> HistoryRecord {
        HistoryRecord {
            source_range: usize::from(record_id)..usize::from(record_id) + 1,
            id: id(record_id),
            version: 1,
            command_id: id(command),
            descendants: descendants.iter().copied().map(id).collect(),
            antecedents: antecedents.iter().copied().map(id).collect(),
            values: vec![HistoryValue {
                id: 7,
                value: Value::Doubles(vec![2.5]),
            }],
            record_type: RecordType::FeatureParameters,
            copy_on_replace: false,
        }
    }

    #[test]
    fn projection_links_unique_prior_producers_and_preserves_native_parameters() {
        let records = [record(1, 11, &[], &[40]), record(2, 12, &[40], &[41])];
        let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
        project(&records, None, &mut ir);

        assert_eq!(ir.model.features.len(), 2);
        assert_eq!(
            ir.model.features[1].dependencies,
            vec![ir.model.features[0].id.clone()]
        );
        let cadmpeg_ir::features::FeatureDefinition::Native {
            kind,
            parameters,
            properties,
        } = &ir.model.features[1].definition
        else {
            panic!("native history operation");
        };
        assert_eq!(kind, "00000000-0000-0000-0000-00000000000c");
        assert_eq!(parameters["value_7"], "2.5");
        assert_eq!(properties["antecedent_objects"], id(40).to_string());
        assert_eq!(
            ir.model.features[1].native_ref.as_deref(),
            Some("rhino:history:record#00000000-0000-0000-0000-000000000002")
        );
    }

    #[test]
    fn projection_preserves_duplicate_values_and_same_record_descendants() {
        let mut producer = record(1, 11, &[], &[40, 40]);
        producer.values.push(HistoryValue {
            id: 7,
            value: Value::Doubles(vec![3.5]),
        });
        let records = [producer, record(2, 12, &[40], &[41])];
        let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
        project(&records, None, &mut ir);

        assert_eq!(
            ir.model.features[1].dependencies,
            vec![ir.model.features[0].id.clone()]
        );
        let cadmpeg_ir::features::FeatureDefinition::Native { parameters, .. } =
            &ir.model.features[0].definition
        else {
            panic!("native history operation");
        };
        assert_eq!(parameters["value_7"], "2.5");
        assert_eq!(parameters["value_7_1"], "3.5");
    }

    #[test]
    fn embedded_geometry_polyedge_and_subd_chain_values_are_typed() {
        let geometry = crate::archive_test_support::class_wrapper(
            crate::archive_test_support::POINT_CLASS,
            &crate::archive_test_support::point_payload([1.0, 2.0, 3.0]),
        );
        let mut geometry_payload = 1_i32.to_le_bytes().to_vec();
        geometry_payload.extend(geometry);
        let geometry_value = value(10, &anonymous_value(0, &geometry_payload));
        let (parsed, next) =
            parse_value(&geometry_value, 0, geometry_value.len(), ArchiveVersion::V8)
                .expect("embedded geometry");
        assert_eq!(next, geometry_value.len());
        assert!(matches!(&parsed.value, Value::Geometries(values)
            if values.len() == 1
                && values[0].class_id == Uuid::from_wire(crate::archive_test_support::POINT_CLASS)));
        let mut properties = BTreeMap::new();
        structured_value_properties(
            "value_7",
            &parsed.value,
            Some((&geometry_value, ArchiveVersion::V8, None, 2.0)),
            &mut properties,
        );
        assert_eq!(
            properties["value_7.0.geometry"],
            r#"{"x":2.0,"y":4.0,"z":6.0}"#
        );

        let mut polyedge = 0_i32.to_le_bytes().to_vec();
        polyedge.extend(2_i32.to_le_bytes());
        polyedge.extend(0.25_f64.to_le_bytes());
        polyedge.extend(0.75_f64.to_le_bytes());
        polyedge.extend(3_i32.to_le_bytes());
        let mut polyedges = 1_i32.to_le_bytes().to_vec();
        polyedges.extend(anonymous_value(0, &polyedge));
        let polyedge_value = value(13, &anonymous_value(0, &polyedges));
        let (parsed, _) = parse_value(&polyedge_value, 0, polyedge_value.len(), ArchiveVersion::V8)
            .expect("polyedge");
        assert!(matches!(parsed.value, Value::PolyEdges(values)
            if values.len() == 1
                && values[0].segments.is_empty()
                && values[0].parameters == [0.25, 0.75]
                && values[0].evaluation_mode == 3));

        let subd_id = id(42);
        let mut chain = [0_u8; 16].to_vec();
        chain[15] = 42;
        chain.extend(2_i32.to_le_bytes());
        chain.extend(2_i32.to_le_bytes());
        chain.extend(11_u32.to_le_bytes());
        chain.extend(12_u32.to_le_bytes());
        chain.extend(2_i32.to_le_bytes());
        chain.extend([0, 1]);
        let mut chains = 1_i32.to_le_bytes().to_vec();
        chains.extend(anonymous_value(0, &chain));
        let chain_value = value(14, &anonymous_value(0, &chains));
        let (parsed, _) = parse_value(&chain_value, 0, chain_value.len(), ArchiveVersion::V8)
            .expect("SubD edge chain");
        assert!(matches!(parsed.value, Value::SubdEdgeChains(values)
            if values.len() == 1
                && values[0].subd_id == subd_id
                && values[0].edge_ids == [11, 12]
                && values[0].orientations == [0, 1]));
    }

    #[test]
    fn embedded_cage_projects_exact_construction_semantics() {
        let mut body = 1_i32.to_le_bytes().to_vec();
        body.extend(0_i32.to_le_bytes());
        body.extend(3_i32.to_le_bytes());
        body.extend(0_i32.to_le_bytes());
        for _ in 0..6 {
            body.extend(2_i32.to_le_bytes());
        }
        for axis in 0..3 {
            body.extend(0.0_f64.to_le_bytes());
            body.extend((axis as f64 + 1.0).to_le_bytes());
        }
        for index in 0..8 {
            for coordinate in [index as f64, 0.0, 0.0] {
                body.extend(coordinate.to_le_bytes());
            }
        }
        let bytes = crate::archive_test_support::crc_chunk(ANONYMOUS, &body);
        let geometry = EmbeddedGeometry {
            class_id: crate::cage::CLASS,
            class_data_range: 0..bytes.len(),
            userdata: Vec::new(),
        };
        let semantic = extended_geometry_json(&bytes, &geometry, ArchiveVersion::V8, None, 10.0)
            .expect("cage semantics");
        let semantic: serde_json::Value = serde_json::from_str(&semantic).unwrap();
        assert_eq!(semantic["kind"], "nurbs_cage");
        assert_eq!(semantic["orders"], serde_json::json!([2, 2, 2]));
        assert_eq!(
            semantic["control_points"][7],
            serde_json::json!([70.0, 0.0, 0.0])
        );

        let empty_subd = [0_u8];
        let geometry = EmbeddedGeometry {
            class_id: crate::subd::ON_SUBD,
            class_data_range: 0..1,
            userdata: Vec::new(),
        };
        let semantic =
            extended_geometry_json(&empty_subd, &geometry, ArchiveVersion::V8, None, 1.0)
                .expect("empty SubD semantics");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&semantic).unwrap(),
            serde_json::json!({"kind": "subd", "empty": true})
        );

        let brep = crate::archive_test_support::brep_payload(false);
        let geometry = EmbeddedGeometry {
            class_id: crate::brep::ON_BREP,
            class_data_range: 0..brep.len(),
            userdata: Vec::new(),
        };
        let semantic = extended_geometry_json(&brep, &geometry, ArchiveVersion::V8, None, 10.0)
            .expect("Brep topology semantics");
        let semantic: serde_json::Value = serde_json::from_str(&semantic).unwrap();
        assert_eq!(semantic["kind"], "brep");
        assert_eq!(semantic["bodies"].as_array().unwrap().len(), 1);
        assert_eq!(semantic["faces"].as_array().unwrap().len(), 1);
        assert_eq!(semantic["vertices"].as_array().unwrap().len(), 3);
    }
}
