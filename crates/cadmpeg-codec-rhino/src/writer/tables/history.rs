// SPDX-License-Identifier: Apache-2.0
//! History-record tables synthesized from neutral features.
//!
//! A neutral feature with source tag `HistoryRecord` carries the record
//! header (command UUID, version, type, copy flag, antecedent and descendant
//! object UUIDs) and every value the decoder admits in structured form.
//! Object references, poly-edges and SubD edge chains round-trip exactly
//! from those properties. Scalar values survive neutrally only as display
//! text with no type tag, and embedded geometries and opaque values retain
//! source byte ranges the synthesis-only writer cannot frame, so a record
//! holding any of those stays omitted with a per-record loss.

use super::{
    anonymous, class_wrapper, crc_chunk_with_direct, neutral_object_uuid, records as held, CadIr,
    CodecError, Value,
};
use crate::loss::RhinoLossCode;
use crate::writer::WriteError;
use cadmpeg_ir::features::{Feature, FeatureOperation};
use cadmpeg_ir::report::loss::LossNote;
use std::collections::{BTreeMap, BTreeSet};

const HISTORY_RECORD: u32 = 0x2000_807b;

pub(in crate::writer) struct HistoryExport {
    pub(in crate::writer) records: Vec<Vec<u8>>,
    pub(in crate::writer) written: BTreeSet<String>,
    pub(in crate::writer) losses: Vec<LossNote>,
}

type Props<'a> = BTreeMap<&'a str, &'a str>;

fn props(feature: &Feature) -> Props<'_> {
    feature
        .source_properties
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect()
}

fn field<'a>(props: &'a Props<'a>, record: &str, name: &str) -> Result<&'a str, WriteError> {
    props
        .get(name)
        .copied()
        .ok_or_else(|| WriteError::Omit(format!("history {record} property {name} is absent")))
}

fn flag(props: &Props<'_>, record: &str, name: &str) -> Result<bool, WriteError> {
    match field(props, record, name)? {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(WriteError::Omit(format!(
            "history {record} property {name} is not a boolean"
        ))),
    }
}

fn integer(props: &Props<'_>, record: &str, name: &str) -> Result<i32, WriteError> {
    field(props, record, name)?
        .parse::<i32>()
        .map_err(|_| WriteError::Omit(format!("history {record} property {name} is not an i32")))
}

fn uuid_bytes(text: &str, record: &str, name: &str) -> Result<[u8; 16], WriteError> {
    if text.is_empty() {
        return Ok([0; 16]);
    }
    crate::presentation::parse_uuid_text(text)
        .map(crate::wire::Uuid::to_wire)
        .ok_or_else(|| WriteError::Omit(format!("history {record} property {name} is not a UUID")))
}

fn uuid_list(text: &str, record: &str, name: &str) -> Result<Vec<[u8; 16]>, WriteError> {
    if text.is_empty() {
        return Ok(Vec::new());
    }
    text.split(',')
        .map(|item| uuid_bytes(item, record, name))
        .collect()
}

fn floats<const N: usize>(text: &str, record: &str, name: &str) -> Result<[f64; N], WriteError> {
    let parts: Vec<&str> = text.split(',').collect();
    if parts.len() != N {
        return Err(WriteError::Omit(format!(
            "history {record} property {name} needs {N} numbers"
        )));
    }
    let mut out = [0.0; N];
    for (target, part) in out.iter_mut().zip(parts) {
        *target = part
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
            .ok_or_else(|| {
                WriteError::Omit(format!("history {record} property {name} must be finite"))
            })?;
    }
    Ok(out)
}

fn float_list(text: &str, record: &str, name: &str) -> Result<Vec<f64>, WriteError> {
    if text.is_empty() {
        return Ok(Vec::new());
    }
    text.split(',')
        .map(|part| {
            part.parse::<f64>()
                .ok()
                .filter(|value| value.is_finite())
                .ok_or_else(|| {
                    WriteError::Omit(format!("history {record} property {name} must be finite"))
                })
        })
        .collect()
}

fn ints<const N: usize>(text: &str, record: &str, name: &str) -> Result<[i32; N], WriteError> {
    let parts: Vec<&str> = text.split(',').collect();
    if parts.len() != N {
        return Err(WriteError::Omit(format!(
            "history {record} property {name} needs {N} integers"
        )));
    }
    let mut out = [0; N];
    for (target, part) in out.iter_mut().zip(parts) {
        *target = part
            .parse::<i32>()
            .map_err(|_| WriteError::Omit(format!("history {record} property {name} needs i32")))?;
    }
    Ok(out)
}

fn uint_list(text: &str, record: &str, name: &str) -> Result<Vec<u32>, WriteError> {
    if text.is_empty() {
        return Ok(Vec::new());
    }
    text.split(',')
        .map(|part| {
            part.parse::<u32>().map_err(|_| {
                WriteError::Omit(format!("history {record} property {name} needs u32"))
            })
        })
        .collect()
}

fn bit_list(text: &str, record: &str, name: &str) -> Result<Vec<u8>, WriteError> {
    if text.is_empty() {
        return Ok(Vec::new());
    }
    text.split(',')
        .map(|part| match part {
            "0" => Ok(0),
            "1" => Ok(1),
            _ => Err(WriteError::Omit(format!(
                "history {record} property {name} needs bits"
            ))),
        })
        .collect()
}

fn uuid_anon(uuids: &[[u8; 16]]) -> Result<Vec<u8>, WriteError> {
    let mut body = i32::try_from(uuids.len())
        .map_err(|_| {
            WriteError::Fail(CodecError::malformed("Rhino history UUID list exceeds i32"))
        })?
        .to_le_bytes()
        .to_vec();
    for uuid in uuids {
        body.extend(uuid);
    }
    anonymous(1, 0, &body).map_err(WriteError::Fail)
}

struct ParsedEvaluation {
    parameter_type: i32,
    component: [i32; 2],
    parameters: [f64; 4],
    intervals: [Option<[f64; 2]>; 3],
}

fn parse_evaluation(
    props: &Props<'_>,
    prefix: &str,
    record: &str,
    nested: bool,
) -> Result<ParsedEvaluation, WriteError> {
    let key = |name: &str| format!("{prefix}.{name}");
    let mut intervals = [None, None, None];
    for (index, slot) in intervals.iter_mut().enumerate() {
        let name = key(&format!("interval_{index}"));
        if let Some(text) = props.get(name.as_str()) {
            *slot = Some(floats(text, record, &name)?);
        } else if nested {
            return Err(WriteError::Omit(format!(
                "history {record} nested evaluation {name} is absent"
            )));
        }
    }
    Ok(ParsedEvaluation {
        parameter_type: integer(props, record, &key("type"))?,
        component: ints(
            field(props, record, &key("component"))?,
            record,
            &key("component"),
        )?,
        parameters: floats(
            field(props, record, &key("parameters"))?,
            record,
            &key("parameters"),
        )?,
        intervals,
    })
}

fn encode_evaluation(body: &mut Vec<u8>, evaluation: &ParsedEvaluation, count: usize) {
    body.extend(evaluation.parameter_type.to_le_bytes());
    for value in evaluation.component {
        body.extend(value.to_le_bytes());
    }
    for value in evaluation.parameters {
        body.extend(value.to_le_bytes());
    }
    for interval in evaluation.intervals.iter().take(count).flatten() {
        for value in interval {
            body.extend(value.to_le_bytes());
        }
    }
}

struct ParsedInstance {
    reference_id: [u8; 16],
    transform: [f64; 16],
    definition_id: [u8; 16],
    geometry_index: i32,
    evaluation: Option<(ParsedEvaluation, [i32; 2])>,
}

fn parse_instance(
    props: &Props<'_>,
    prefix: String,
    record: &str,
) -> Result<ParsedInstance, WriteError> {
    let key = |name: &str| format!("{prefix}.{name}");
    let evaluation = match props.get(key("evaluation.type").as_str()) {
        Some(_) => {
            let evaluation = parse_evaluation(props, &key("evaluation"), record, true)?;
            let component = ints(
                field(props, record, &key("component"))?,
                record,
                &key("component"),
            )?;
            Some((evaluation, component))
        }
        None => None,
    };
    Ok(ParsedInstance {
        reference_id: uuid_bytes(
            field(props, record, &key("reference_id"))?,
            record,
            &key("reference_id"),
        )?,
        transform: floats(
            field(props, record, &key("transform"))?,
            record,
            &key("transform"),
        )?,
        definition_id: uuid_bytes(
            field(props, record, &key("definition_id"))?,
            record,
            &key("definition_id"),
        )?,
        geometry_index: integer(props, record, &key("geometry_index"))?,
        evaluation,
    })
}

fn encode_instance(instance: &ParsedInstance) -> Result<(Vec<u8>, i32), WriteError> {
    let mut body = Vec::new();
    body.extend(instance.reference_id);
    for value in instance.transform {
        body.extend(value.to_le_bytes());
    }
    body.extend(instance.definition_id);
    body.extend(instance.geometry_index.to_le_bytes());
    let mut minor = 0;
    if let Some((evaluation, component)) = &instance.evaluation {
        minor = 1;
        for value in component {
            body.extend(value.to_le_bytes());
        }
        let mut nested = Vec::new();
        encode_evaluation(&mut nested, evaluation, 3);
        body.extend(anonymous(1, 0, &nested).map_err(WriteError::Fail)?);
    }
    Ok((anonymous(1, minor, &body).map_err(WriteError::Fail)?, minor))
}

struct ParsedReference {
    object_id: [u8; 16],
    component: [i32; 2],
    geometry_type: i32,
    point: [f64; 3],
    osnap_mode: i32,
    evaluation: ParsedEvaluation,
    instances: Vec<ParsedInstance>,
}

fn parse_reference(
    props: &Props<'_>,
    prefix: &str,
    record: &str,
) -> Result<ParsedReference, WriteError> {
    let key = |name: &str| format!("{prefix}.{name}");
    let mut instances = Vec::new();
    let mut index = 0;
    loop {
        let candidate = format!("{prefix}.instance_{index}");
        if props
            .get(format!("{candidate}.reference_id").as_str())
            .is_none()
        {
            break;
        }
        instances.push(parse_instance(props, candidate, record)?);
        index += 1;
    }
    if let Some(count) = props.get(key("instance_count").as_str()) {
        let count: usize = count.parse().map_err(|_| {
            WriteError::Omit(format!("history {record} instance count is not a number"))
        })?;
        if count != instances.len() {
            return Err(WriteError::Omit(format!(
                "history {record} instance count {count} names {} instances",
                instances.len()
            )));
        }
    }
    Ok(ParsedReference {
        object_id: uuid_bytes(
            field(props, record, &key("object_id"))?,
            record,
            &key("object_id"),
        )?,
        component: ints(
            field(props, record, &key("component"))?,
            record,
            &key("component"),
        )?,
        geometry_type: integer(props, record, &key("geometry_type"))?,
        point: floats(field(props, record, &key("point"))?, record, &key("point"))?,
        osnap_mode: integer(props, record, &key("osnap_mode"))?,
        evaluation: parse_evaluation(props, &key("evaluation"), record, false)?,
        instances,
    })
}

fn encode_reference(reference: &ParsedReference) -> Result<Vec<u8>, WriteError> {
    let mut minor = 0;
    if reference.evaluation.intervals[0].is_some() && reference.evaluation.intervals[1].is_some() {
        minor = 1;
    }
    if reference.evaluation.intervals[2].is_some() {
        if minor < 1 {
            return Err(WriteError::Fail(CodecError::malformed(
                "Rhino history interval 2 needs intervals 0 and 1",
            )));
        }
        minor = 2;
    }
    if reference.osnap_mode != 0 {
        if minor < 1 {
            return Err(WriteError::Fail(CodecError::malformed(
                "Rhino history nonzero osnap needs evaluation intervals",
            )));
        }
        minor = 3;
    }
    let mut body = Vec::new();
    body.extend(reference.object_id);
    for value in reference.component {
        body.extend(value.to_le_bytes());
    }
    body.extend(reference.geometry_type.to_le_bytes());
    for value in reference.point {
        body.extend(value.to_le_bytes());
    }
    encode_evaluation(&mut body, &reference.evaluation, 0);
    body.extend(
        i32::try_from(reference.instances.len())
            .map_err(|_| {
                WriteError::Fail(CodecError::malformed(
                    "Rhino history instance path exceeds i32",
                ))
            })?
            .to_le_bytes(),
    );
    for instance in &reference.instances {
        let (framed, _) = encode_instance(instance)?;
        body.extend(framed);
    }
    if minor >= 1 {
        for interval in reference.evaluation.intervals.iter().take(2).flatten() {
            for value in interval {
                body.extend(value.to_le_bytes());
            }
        }
    }
    if minor >= 2 {
        for interval in reference
            .evaluation
            .intervals
            .iter()
            .skip(2)
            .take(1)
            .flatten()
        {
            for value in interval {
                body.extend(value.to_le_bytes());
            }
        }
    }
    if minor >= 3 {
        body.extend(reference.osnap_mode.to_le_bytes());
    }
    anonymous(1, minor, &body).map_err(WriteError::Fail)
}

/// Contiguous sub-record indices below a value group, or empty when the
/// indices skip.
fn indexed(props: &Props<'_>, group: &str) -> Vec<usize> {
    let mut indices = BTreeSet::new();
    for key in props.keys() {
        let Some(rest) = key.strip_prefix(group) else {
            continue;
        };
        let Some(rest) = rest.strip_prefix('.') else {
            continue;
        };
        let head = rest.split('.').next().unwrap_or("");
        if let Ok(index) = head.parse::<usize>() {
            indices.insert(index);
        }
    }
    let contiguous: Vec<usize> = indices.into_iter().collect();
    contiguous
        .iter()
        .enumerate()
        .all(|(position, index)| position == *index)
        .then_some(contiguous)
        .unwrap_or_default()
}

fn check_count(
    props: &Props<'_>,
    group: &str,
    record: &str,
    found: usize,
    what: &str,
) -> Result<(), WriteError> {
    if let Some(count) = props.get(format!("{group}.count").as_str()) {
        let count: usize = count.parse().map_err(|_| {
            WriteError::Omit(format!(
                "history {record} value {group} count is not a number"
            ))
        })?;
        if count != found {
            return Err(WriteError::Omit(format!(
                "history {record} value {group} count {count} names {found} {what}"
            )));
        }
    }
    Ok(())
}

fn encode_references(props: &Props<'_>, group: &str, record: &str) -> Result<Vec<u8>, WriteError> {
    let indices = indexed(props, group);
    if indices.is_empty() {
        return Err(WriteError::Omit(format!(
            "history {record} value {group} holds no object references"
        )));
    }
    check_count(props, group, record, indices.len(), "references")?;
    let mut body = Vec::new();
    body.extend(
        i32::try_from(indices.len())
            .map_err(|_| {
                WriteError::Fail(CodecError::malformed("Rhino history count exceeds i32"))
            })?
            .to_le_bytes(),
    );
    for index in indices {
        let reference = parse_reference(props, &format!("{group}.{index}"), record)?;
        body.extend(encode_reference(&reference)?);
    }
    Ok(body)
}

fn encode_polyedges(props: &Props<'_>, group: &str, record: &str) -> Result<Vec<u8>, WriteError> {
    let indices = indexed(props, group);
    if indices.is_empty() {
        return Err(WriteError::Omit(format!(
            "history {record} value {group} holds no poly-edges"
        )));
    }
    check_count(props, group, record, indices.len(), "poly-edges")?;
    let mut body = Vec::new();
    body.extend(
        i32::try_from(indices.len())
            .map_err(|_| {
                WriteError::Fail(CodecError::malformed("Rhino history count exceeds i32"))
            })?
            .to_le_bytes(),
    );
    for index in indices {
        let edge = format!("{group}.{index}");
        let mut segments = Vec::new();
        let mut segment_index = 0;
        loop {
            let segment = format!("{edge}.segment_{segment_index}");
            let curve = format!("{segment}.curve");
            if props.get(format!("{curve}.object_id").as_str()).is_none() {
                break;
            }
            let reference = parse_reference(props, &curve, record)?;
            let framed = encode_reference(&reference)?;
            let mut proxy = framed;
            proxy.push(u8::from(flag(
                props,
                record,
                &format!("{segment}.reversed"),
            )?));
            for name in ["full_domain", "sub_domain", "proxy_domain"] {
                let domain: [f64; 2] = floats(
                    field(props, record, &format!("{segment}.{name}"))?,
                    record,
                    &format!("{segment}.{name}"),
                )?;
                for value in domain {
                    proxy.extend(value.to_le_bytes());
                }
            }
            let domains = ["edge_domain", "trim_domain"]
                .iter()
                .map(|name| {
                    props
                        .get(format!("{segment}.{name}").as_str())
                        .map(|text| floats::<2>(text, record, &format!("{segment}.{name}")))
                })
                .collect::<Option<Result<Vec<[f64; 2]>, WriteError>>>();
            // Each curve proxy carries its own minor: domains present or not.
            let mut minor = 0;
            match domains {
                Some(Ok(domains)) => {
                    minor = 1;
                    for domain in &domains {
                        for value in domain {
                            proxy.extend(value.to_le_bytes());
                        }
                    }
                }
                Some(Err(error)) => return Err(error),
                None => {}
            }
            segments.push(anonymous(1, minor, &proxy).map_err(WriteError::Fail)?);
            segment_index += 1;
        }
        if segments.is_empty() {
            return Err(WriteError::Omit(format!(
                "history {record} value {group} poly-edge holds no segments"
            )));
        }
        let mut edge_body = Vec::new();
        edge_body.extend(
            i32::try_from(segments.len())
                .map_err(|_| {
                    WriteError::Fail(CodecError::malformed(
                        "Rhino history segment count exceeds i32",
                    ))
                })?
                .to_le_bytes(),
        );
        for segment in &segments {
            edge_body.extend(segment);
        }
        // The parameter array is count-prefixed on the wire.
        let parameters = float_list(
            field(props, record, &format!("{edge}.parameters"))?,
            record,
            &format!("{edge}.parameters"),
        )?;
        edge_body.extend(
            i32::try_from(parameters.len())
                .map_err(|_| {
                    WriteError::Fail(CodecError::malformed("Rhino history parameters exceed i32"))
                })?
                .to_le_bytes(),
        );
        for value in &parameters {
            edge_body.extend(value.to_le_bytes());
        }
        edge_body.extend(integer(props, record, &format!("{edge}.evaluation_mode"))?.to_le_bytes());
        body.extend(anonymous(1, 0, &edge_body).map_err(WriteError::Fail)?);
    }
    Ok(body)
}

fn encode_chains(props: &Props<'_>, group: &str, record: &str) -> Result<Vec<u8>, WriteError> {
    let indices = indexed(props, group);
    if indices.is_empty() {
        return Err(WriteError::Omit(format!(
            "history {record} value {group} holds no SubD edge chains"
        )));
    }
    check_count(props, group, record, indices.len(), "SubD edge chains")?;
    let mut body = Vec::new();
    body.extend(
        i32::try_from(indices.len())
            .map_err(|_| {
                WriteError::Fail(CodecError::malformed("Rhino history count exceeds i32"))
            })?
            .to_le_bytes(),
    );
    for index in indices {
        let chain = format!("{group}.{index}");
        let key = |name: &str| format!("{chain}.{name}");
        let edge_ids = uint_list(
            field(props, record, &key("edge_ids"))?,
            record,
            &key("edge_ids"),
        )?;
        let orientations = bit_list(
            field(props, record, &key("orientations"))?,
            record,
            &key("orientations"),
        )?;
        if edge_ids.len() != orientations.len() {
            return Err(WriteError::Omit(format!(
                "history {record} value {group} edge ids and orientations disagree"
            )));
        }
        let mut chain_body = Vec::new();
        chain_body.extend(uuid_bytes(
            field(props, record, &key("subd_id"))?,
            record,
            &key("subd_id"),
        )?);
        chain_body.extend(
            i32::try_from(edge_ids.len())
                .map_err(|_| {
                    WriteError::Fail(CodecError::malformed(
                        "Rhino history edge count exceeds i32",
                    ))
                })?
                .to_le_bytes(),
        );
        for id in &edge_ids {
            chain_body.extend(id.to_le_bytes());
        }
        chain_body.extend(orientations);
        body.extend(anonymous(1, 1, &chain_body).map_err(WriteError::Fail)?);
    }
    anonymous(1, 1, &body).map_err(WriteError::Fail)
}

/// Value groups of one history feature: `(id, occurrence, kind)` where kind
/// is the reconstructible wire type, or `None` for a value the neutral form
/// cannot carry back.
fn value_groups(props: &Props<'_>) -> Vec<(i32, usize, Option<i32>)> {
    let mut groups = BTreeMap::new();
    for key in props.keys() {
        let Some(rest) = key.strip_prefix("value_") else {
            continue;
        };
        let head = rest.split('.').next().unwrap_or("");
        let (id_text, occ_text) = match head.rsplit_once('_') {
            Some((id, occ)) if !occ.contains('.') => (id, Some(occ)),
            _ => (head, None),
        };
        let (Ok(id), occ) = (
            id_text.parse::<i32>(),
            occ_text.map_or(Ok(0), str::parse::<usize>),
        ) else {
            continue;
        };
        let Ok(occ) = occ else { continue };
        groups.entry((id, occ)).or_insert_with(Vec::new).push(*key);
    }
    let mut out = Vec::new();
    for ((id, occ), keys) in groups {
        let group = if occ == 0 {
            format!("value_{id}")
        } else {
            format!("value_{id}_{occ}")
        };
        // A direct reference marker is exactly `{group}.{i}.object_id`;
        // poly-edge segment curves nest deeper (`.segment_N.curve.object_id`).
        let direct = keys.iter().any(|key| {
            let Some(rest) = key.strip_prefix(&format!("{group}.")) else {
                return false;
            };
            let mut parts = rest.split('.');
            matches!(
                (parts.next(), parts.next(), parts.next()),
                (Some(index), Some("object_id"), None) if index.parse::<usize>().is_ok()
            )
        });
        let segments = keys.iter().any(|key| {
            key.starts_with(&format!("{group}."))
                && key.contains(".segment_")
                && key.ends_with(".curve.object_id")
        });
        let chains = keys.iter().any(|key| {
            key.starts_with(&format!("{group}."))
                && (key.ends_with(".subd_id") || key.ends_with(".edge_ids"))
        });
        let kind = if direct {
            Some(9)
        } else if segments {
            Some(13)
        } else if chains {
            Some(14)
        } else {
            None
        };
        out.push((id, occ, kind));
    }
    out.sort();
    out
}

fn encode_value(
    props: &Props<'_>,
    group: &str,
    id: i32,
    kind: i32,
    record: &str,
) -> Result<Vec<u8>, WriteError> {
    let payload = match kind {
        9 => encode_references(props, group, record)?,
        13 => encode_polyedges(props, group, record)?,
        14 => encode_chains(props, group, record)?,
        _ => {
            return Err(WriteError::Omit(format!(
                "history {record} value {group} has an unhandled type"
            )));
        }
    };
    let mut body = kind.to_le_bytes().to_vec();
    body.extend(id.to_le_bytes());
    body.extend(payload);
    anonymous(1, 0, &body).map_err(WriteError::Fail)
}

fn command_uuid(feature: &Feature, record: &str) -> Result<[u8; 16], WriteError> {
    let definition = feature.evaluation.definition();
    let cadmpeg_ir::features::FeatureDefinition::Operation(operation) = definition else {
        return Err(WriteError::Omit(format!(
            "history {record} feature is not a native operation"
        )));
    };
    let FeatureOperation::Native { kind, .. } = operation else {
        return Err(WriteError::Omit(format!(
            "history {record} feature is not a native operation"
        )));
    };
    uuid_bytes(kind.as_str(), record, "command")
}

fn record_uuid(ir: &CadIr, feature: &Feature) -> [u8; 16] {
    let linked = feature.native_ref.as_deref().and_then(|native| {
        let source = held(ir, "history_records")
            .iter()
            .find(|held| held.id() == native)
            .and_then(|held| held.field("source_uuid"))
            .filter(|value| !value.is_null());
        source
            .as_ref()
            .and_then(Value::as_str)
            .and_then(crate::presentation::parse_uuid_text)
            .filter(|uuid| !uuid.is_nil())
            .map(crate::wire::Uuid::to_wire)
    });
    linked.unwrap_or_else(|| neutral_object_uuid(feature.id.as_str()))
}

fn history_record(ir: &CadIr, feature: &Feature) -> Result<Vec<u8>, WriteError> {
    let record = feature.native_ref.as_deref().unwrap_or(feature.id.as_str());
    let properties = props(feature);
    let version = integer(&properties, record, "record_version")?;
    let record_type: i32 = match field(&properties, record, "record_type")? {
        "history_parameters" => 0,
        "feature_parameters" => 1,
        _ => {
            return Err(WriteError::Omit(format!(
                "history {record} record type is not admitted"
            )));
        }
    };
    let copy = flag(&properties, record, "copy_on_replace")?;
    let minor = if copy {
        2
    } else if record_type == 1 {
        1
    } else {
        0
    };
    let mut body = Vec::new();
    body.extend(record_uuid(ir, feature));
    body.extend(version.to_le_bytes());
    body.extend(command_uuid(feature, record)?);
    let descendants = uuid_list(
        field(&properties, record, "descendant_objects").unwrap_or(""),
        record,
        "descendant_objects",
    )?;
    body.extend(uuid_anon(&descendants)?);
    let antecedents = uuid_list(
        field(&properties, record, "antecedent_objects").unwrap_or(""),
        record,
        "antecedent_objects",
    )?;
    body.extend(uuid_anon(&antecedents)?);
    let mut values = Vec::new();
    for (id, occ, kind) in value_groups(&properties) {
        let group = if occ == 0 {
            format!("value_{id}")
        } else {
            format!("value_{id}_{occ}")
        };
        let Some(kind) = kind else {
            return Err(WriteError::Omit(format!(
                "history {record} value {group} has no reconstructible content"
            )));
        };
        values.push(encode_value(&properties, &group, id, kind, record)?);
    }
    let mut values_body = i32::try_from(values.len())
        .map_err(|_| {
            WriteError::Fail(CodecError::malformed(
                "Rhino history value count exceeds i32",
            ))
        })?
        .to_le_bytes()
        .to_vec();
    for value in &values {
        values_body.extend(value);
    }
    body.extend(anonymous(1, 0, &values_body).map_err(WriteError::Fail)?);
    if minor >= 1 {
        body.extend(record_type.to_le_bytes());
    }
    if minor >= 2 {
        body.push(u8::from(copy));
    }
    let classed = class_wrapper(
        crate::wire::Uuid::to_wire(crate::history::HISTORY_CLASS),
        &anonymous(1, minor, &body).map_err(WriteError::Fail)?,
    )
    .map_err(WriteError::Fail)?;
    crc_chunk_with_direct(HISTORY_RECORD, &classed, &[]).map_err(WriteError::Fail)
}

pub(in crate::writer) fn records(ir: &CadIr) -> Result<HistoryExport, WriteError> {
    let mut output = HistoryExport {
        records: Vec::new(),
        written: BTreeSet::new(),
        losses: Vec::new(),
    };
    let mut features: Vec<&Feature> = ir
        .model
        .features
        .iter()
        .filter(|feature| feature.source_tag.as_deref() == Some("HistoryRecord"))
        .collect();
    features.sort_by_key(|feature| feature.ordinal);
    for feature in features {
        let record = feature.native_ref.as_deref().unwrap_or(feature.id.as_str());
        match history_record(ir, feature) {
            Ok(bytes) => {
                output.records.push(bytes);
                if let Some(native) = feature.native_ref.as_deref() {
                    output.written.insert(native.to_owned());
                }
            }
            Err(WriteError::Omit(reason)) => output
                .losses
                .push(RhinoLossCode::WriterAttributesOmitted.note(format!("{record}: {reason}"))),
            Err(error) => return Err(error),
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests;
