// SPDX-License-Identifier: Apache-2.0
//! Built-in history-record decoding.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;

use crate::chunks::{checked_count_bytes, chunk_at, ArchiveVersion, BoundedReader, FramingError};
use crate::container::Record;
use crate::objects::parse_class_wrapper;
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
    Uuids(Vec<Uuid>),
    Opaque { type_code: i32, range: Range<usize> },
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

fn parse_value(
    bytes: &[u8],
    offset: usize,
    end: usize,
    archive: ArchiveVersion,
) -> Result<(HistoryValue, usize), FramingError> {
    let (mut reader, next, _) = anonymous(bytes, offset, end, archive)?;
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
        11 => Value::Uuids(array(&mut reader, 16, uuid)?),
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
    let mut values = Vec::with_capacity(value_count);
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
        Value::Uuids(values) => list(values),
        Value::Opaque { .. } => return None,
    })
}

/// Projects source history into ordered neutral native operations.
pub(crate) fn project(records: &[HistoryRecord], ir: &mut cadmpeg_ir::document::CadIr) {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId};

    let mut ids = Vec::with_capacity(records.len());
    let mut seen_record_ids = HashSet::new();
    for record in records {
        let unique = !record.id.is_nil() && seen_record_ids.insert(record.id);
        ids.push(FeatureId(if unique {
            format!("rhino:history:feature#{}", record.id)
        } else {
            format!("rhino:history:feature#offset-{}", record.source_range.start)
        }));
    }
    let mut producers = HashMap::<Uuid, Option<(usize, FeatureId)>>::new();
    for (index, record) in records.iter().enumerate() {
        for descendant in &record.descendants {
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
        for value in &record.values {
            let key = format!("value_{}", value.id);
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
            suppressed: false,
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
            native_ref: Some(format!("rhino:history:record#{}", record.id)),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        project(&records, &mut ir);

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
}
