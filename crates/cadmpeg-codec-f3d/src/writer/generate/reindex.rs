// SPDX-License-Identifier: Apache-2.0
//! Preserve available F3D record identities in a generated BREP table.

use cadmpeg_asm::kernel_header::RefWidth;
use cadmpeg_asm::sab::{Record, Token};
use cadmpeg_ir::document::Model;

use super::native_bytes::{
    native_enum, native_f64, native_i64, native_point, native_ref, native_string, native_vector,
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
    limits: &cadmpeg_core::decode::ResourceLimits,
    starts: &Starts,
) -> Result<(), crate::writer::WriteError> {
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
        limits: *limits,
        ..Default::default()
    };
    let ctx = cadmpeg_core::decode::DecodeContext::for_encode(&arena, &policy);
    let framed =
        match cadmpeg_asm::sab::frame(&ctx, records, 0, records.len(), RefWidth::Eight, None) {
            Ok(framed) => framed,
            Err(cadmpeg_asm::stream_error::StreamFailure::Resource(limit)) => {
                return Err(limit.into())
            }
            Err(error) => {
                debug_assert!(false, "generated F3D BREP records must frame: {error:?}");
                return Ok(());
            }
        };
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
    let Some(mapping) = destinations.into_iter().collect::<Option<Vec<_>>>() else {
        debug_assert!(false, "each F3D BREP record has a unique remap destination");
        return Ok(());
    };
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
        if !encode_record(&mut rewritten, record, &mapping) {
            return Ok(());
        }
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

fn generated_identifier(out: &mut Vec<u8>, tag: u8, value: &str) -> bool {
    let Ok(length) = u8::try_from(value.len()) else {
        debug_assert!(false, "a framed F3D identifier fits its one-byte length");
        return false;
    };
    out.extend_from_slice(&[tag, length]);
    out.extend_from_slice(value.as_bytes());
    true
}

fn encode_record(out: &mut Vec<u8>, record: &Record, mapping: &[usize]) -> bool {
    let mut names = record.name.split('-').peekable();
    while let Some(name) = names.next() {
        let tag = if names.peek().is_some() { 0x0e } else { 0x0d };
        if !generated_identifier(out, tag, name) {
            return false;
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
            Token::Str(value) => native_string(out, value),
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
            Token::Ident(value) => {
                if !generated_identifier(out, 0x0d, value) {
                    return false;
                }
            }
            Token::SubIdent(value) => {
                if !generated_identifier(out, 0x0e, value) {
                    return false;
                }
            }
        }
    }
    out.push(0x11);
    true
}

#[cfg(test)]
mod tests {
    #[test]
    fn reframed_record_keeps_identifiers_and_reindexes_references() {
        use cadmpeg_asm::{kernel_header::RefWidth, sab::Token, test_support::sab::frame};

        let mut source = vec![0x0e, 2, b'a', b'b', 0x0d, 2, b'c', b'd'];
        source.extend_from_slice(&[0x0d, 2, b'x', b'y', 0x0c]);
        source.extend_from_slice(&1_i64.to_le_bytes());
        source.push(0x11);
        let records = frame(&source, 0, source.len(), RefWidth::Eight).unwrap();
        let mut output = Vec::new();
        assert!(super::encode_record(&mut output, &records[0], &[1, 0]));
        let rewritten = frame(&output, 0, output.len(), RefWidth::Eight).unwrap();
        assert_eq!(rewritten[0].name, records[0].name);
        assert_eq!(rewritten[0].tokens[0], Token::Ident("xy".into()));
        assert_eq!(rewritten[0].chunk(0), Some(&Token::Ref(0)));
    }
}
