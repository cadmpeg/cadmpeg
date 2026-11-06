// SPDX-License-Identifier: Apache-2.0
//! Preserve available F3D record identities in a generated BREP table.

use cadmpeg_asm::kernel_header::RefWidth;
use cadmpeg_asm::sab::{Record, Token};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::Model;

use super::native_bytes::{
    native_enum, native_f64, native_i64, native_ident, native_length_prefixed_string, native_point,
    native_ref, native_subident, native_vector,
};

pub(super) struct Starts {
    pub(super) region: i64,
    pub(super) shell: i64,
    pub(super) face: i64,
    pub(super) loop_: i64,
    pub(super) surface: i64,
    pub(super) curve: i64,
    pub(super) pcurve: i64,
    pub(super) coedge: i64,
    pub(super) edge: i64,
    pub(super) vertex: i64,
    pub(super) point: i64,
}

pub(super) fn preserve_indices(
    records: &mut Vec<u8>,
    model: &Model,
    topology: &super::index::NativeGenerationIndex<'_>,
    limits: cadmpeg_core::decode::ResourceLimits,
    starts: &Starts,
) -> Result<(), CodecError> {
    if topology.edge_ownerships.is_empty()
        && topology.face_native_keys.is_empty()
        && !model
            .edges
            .iter()
            .any(|edge| entity_index(edge.id.as_str()).is_some())
        && !model
            .faces
            .iter()
            .any(|face| entity_index(face.id.as_str()).is_some())
    {
        return Ok(());
    }
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy {
        limits,
        ..Default::default()
    };
    let ctx = cadmpeg_core::decode::DecodeContext::for_encode(&arena, &policy);
    let framed = cadmpeg_asm::sab::frame(&ctx, records, 0, records.len(), RefWidth::Eight, None)
        .map_err(|error| error.into_codec_error(&ctx, CodecError::malformed))?;
    let count = framed.len();
    let mut destinations = ctx.alloc_filled(count, None, "plan F3D BREP record destinations")?;
    let mut occupied = ctx.alloc_filled(count, false, "reserve F3D BREP record indices")?;
    for index in 0..=model.bodies.len() {
        if index >= count {
            return Ok(());
        }
        destinations[index] = Some(index);
        occupied[index] = true;
    }
    macro_rules! reserve {
        ($items:expr, $start:expr, $index:expr) => {
            for (ordinal, item) in $items.iter().enumerate() {
                let Some(desired) = ($index)(item) else {
                    continue;
                };
                let Some(source) = usize::try_from($start)
                    .ok()
                    .and_then(|start| start.checked_add(ordinal))
                else {
                    return Ok(());
                };
                if source >= count
                    || desired >= count
                    || occupied[desired]
                    || destinations[source].is_some()
                {
                    return Ok(());
                }
                destinations[source] = Some(desired);
                occupied[desired] = true;
            }
        };
    }
    reserve!(
        model.regions,
        starts.region,
        |v: &cadmpeg_ir::topology::Region| entity_index(v.id.as_str())
    );
    reserve!(
        model.shells,
        starts.shell,
        |v: &cadmpeg_ir::topology::Shell| entity_index(v.id.as_str())
    );
    reserve!(
        model.faces,
        starts.face,
        |v: &cadmpeg_ir::topology::Face| {
            topology
                .face_native_keys
                .get(v.id.as_str())
                .and_then(|row| usize::try_from(row.record_index).ok())
                .or_else(|| entity_index(v.id.as_str()))
        }
    );
    reserve!(
        model.loops,
        starts.loop_,
        |v: &cadmpeg_ir::topology::Loop| entity_index(v.id.as_str())
    );
    reserve!(
        model.surfaces,
        starts.surface,
        |v: &cadmpeg_ir::geometry::Surface| entity_index(v.id.as_str())
    );
    reserve!(
        model.curves,
        starts.curve,
        |v: &cadmpeg_ir::geometry::Curve| entity_index(v.id.as_str())
    );
    reserve!(
        model.pcurves,
        starts.pcurve,
        |v: &cadmpeg_ir::geometry::pcurve::Pcurve| entity_index(v.id.as_str())
    );
    reserve!(
        model.coedges,
        starts.coedge,
        |v: &cadmpeg_ir::topology::Coedge| entity_index(v.id.as_str())
    );
    reserve!(
        model.edges,
        starts.edge,
        |v: &cadmpeg_ir::topology::Edge| {
            topology
                .edge_ownerships
                .iter()
                .find(|row| v.native_ref.as_deref() == Some(row.id().as_str()))
                .and_then(|row| usize::try_from(row.record_index).ok())
                .or_else(|| entity_index(v.id.as_str()))
        }
    );
    reserve!(
        model.vertices,
        starts.vertex,
        |v: &cadmpeg_ir::topology::Vertex| {
            topology
                .vertex_ownerships
                .get(v.id.as_str())
                .and_then(|row| usize::try_from(row.record_index).ok())
                .or_else(|| entity_index(v.id.as_str()))
        }
    );
    reserve!(
        model.points,
        starts.point,
        |v: &cadmpeg_ir::topology::Point| entity_index(v.id.as_str())
    );
    let mut available = occupied
        .iter()
        .enumerate()
        .filter_map(|(i, used)| (!used).then_some(i));
    for destination in &mut destinations {
        if destination.is_none() {
            *destination = available.next();
        }
    }
    let mapping = destinations
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| CodecError::Malformed("F3D BREP record remap is incomplete".into()))?;
    if mapping
        .iter()
        .enumerate()
        .all(|(index, destination)| index == *destination)
    {
        return Ok(());
    }
    let mut ordered = ctx.alloc_filled(count, None, "order F3D BREP records")?;
    for record in &framed {
        ordered[mapping[record.index]] = Some(record);
    }
    let mut rewritten = Vec::with_capacity(records.len());
    for record in ordered.into_iter().flatten() {
        encode_record(&mut rewritten, record, &mapping)?;
    }
    *records = rewritten;
    Ok(())
}

fn native_index(id: Option<&str>, prefix: &str) -> Option<usize> {
    id?.strip_prefix(prefix)?.parse().ok()
}

fn entity_index(id: &str) -> Option<usize> {
    native_index(Some(id), "f3d:brep:entity#")
}

fn encode_record(out: &mut Vec<u8>, record: &Record, mapping: &[usize]) -> Result<(), CodecError> {
    let mut names = record.name.split('-').peekable();
    while let Some(name) = names.next() {
        if names.peek().is_some() {
            native_subident(out, name)?;
        } else {
            native_ident(out, name)?;
        }
    }
    for token in record.tokens.iter() {
        match token {
            Token::Char(value) => out.extend_from_slice(&[0x02, *value]),
            Token::Short(value) => {
                out.push(0x03);
                out.extend_from_slice(&value.to_le_bytes());
            }
            Token::Long(value) => native_i64(out, *value),
            Token::Float(value) => {
                out.push(0x05);
                out.extend_from_slice(&value.to_le_bytes());
            }
            Token::Double(value) => native_f64(out, *value),
            Token::Str(value) => native_length_prefixed_string(out, value),
            Token::True => out.push(0x0a),
            Token::False => out.push(0x0b),
            Token::Ref(value) => {
                let translated = usize::try_from(*value)
                    .ok()
                    .and_then(|index| mapping.get(index))
                    .and_then(|index| i64::try_from(*index).ok())
                    .unwrap_or(*value);
                native_ref(out, translated);
            }
            Token::SubtypeOpen => out.push(0x0f),
            Token::SubtypeClose => out.push(0x10),
            Token::Enum(value) => native_enum(out, *value),
            Token::Position(value) => native_point(out, *value),
            Token::Vector3(value) => native_vector(out, *value),
            Token::Vector2(value) => {
                out.push(0x16);
                for component in value {
                    out.extend_from_slice(&component.to_le_bytes());
                }
            }
            Token::Int64(value) => {
                out.push(0x17);
                out.extend_from_slice(&value.to_le_bytes());
            }
            Token::Ident(value) => native_ident(out, value)?,
            Token::SubIdent(value) => native_subident(out, value)?,
        }
    }
    out.push(0x11);
    Ok(())
}
