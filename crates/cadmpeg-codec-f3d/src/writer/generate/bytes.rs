// SPDX-License-Identifier: Apache-2.0
//! Low-level native record byte writers for source-less generation.

use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::topology::Body;
use cadmpeg_ir::transform::Transform;

use crate::writer::primitives::{f3d_native, history_change_kind, native_bool};

pub(crate) fn native_ident(bytes: &mut Vec<u8>, value: &str) -> Result<(), CodecError> {
    native_text(bytes, 0x0d, value)
}

pub(crate) fn native_subident(bytes: &mut Vec<u8>, value: &str) -> Result<(), CodecError> {
    native_text(bytes, 0x0e, value)
}

pub(crate) fn native_curve_base(bytes: &mut Vec<u8>, kind: &str) -> Result<(), CodecError> {
    native_subident(bytes, kind)?;
    native_ident(bytes, "curve")?;
    native_ref(bytes, -1);
    native_i64(bytes, -1);
    native_ref(bytes, -1);
    if kind == "intcurve" {
        bytes.push(native_bool(false));
    }
    Ok(())
}

pub(crate) fn native_surface_base(bytes: &mut Vec<u8>, kind: &str) -> Result<(), CodecError> {
    native_subident(bytes, kind)?;
    native_ident(bytes, "surface")?;
    native_ref(bytes, -1);
    native_i64(bytes, -1);
    native_ref(bytes, -1);
    if kind == "spline" {
        bytes.push(native_bool(false));
    }
    Ok(())
}

pub(crate) fn native_string(bytes: &mut Vec<u8>, value: &str) -> Result<(), CodecError> {
    native_text(bytes, 0x07, value)
}

pub(crate) fn native_u16_string(bytes: &mut Vec<u8>, value: &str) -> Result<(), CodecError> {
    let length = u16::try_from(value.len())
        .map_err(|_| CodecError::NotImplemented("F3D native text exceeds u16".into()))?;
    bytes.push(0x08);
    bytes.extend_from_slice(&length.to_le_bytes());
    bytes.extend_from_slice(value.as_bytes());
    Ok(())
}

fn native_text(bytes: &mut Vec<u8>, tag: u8, value: &str) -> Result<(), CodecError> {
    let length = u8::try_from(value.len())
        .map_err(|_| CodecError::NotImplemented("F3D native text exceeds 255 bytes".into()))?;
    bytes.extend_from_slice(&[tag, length]);
    bytes.extend_from_slice(value.as_bytes());
    Ok(())
}

pub(crate) fn native_ref(bytes: &mut Vec<u8>, value: i64) {
    bytes.push(0x0c);
    bytes.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn native_record_index(base: i64, ordinal: usize) -> Result<i64, CodecError> {
    let ordinal = i64::try_from(ordinal)
        .map_err(|_| CodecError::NotImplemented("F3D record ordinal exceeds i64".into()))?;
    base.checked_add(ordinal)
        .ok_or_else(|| CodecError::NotImplemented("F3D record index exceeds i64".into()))
}

pub(crate) fn native_i64(bytes: &mut Vec<u8>, value: i64) {
    bytes.push(0x04);
    bytes.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn native_enum(bytes: &mut Vec<u8>, value: i64) {
    bytes.push(0x15);
    bytes.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn native_f64(bytes: &mut Vec<u8>, value: f64) {
    bytes.push(0x06);
    bytes.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn native_point(bytes: &mut Vec<u8>, point: [f64; 3]) {
    bytes.push(0x13);
    for value in point {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
}

pub(crate) fn native_vector(bytes: &mut Vec<u8>, vector: [f64; 3]) {
    bytes.push(0x14);
    for value in vector {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
}

pub(crate) fn native_transform(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    body: &Body,
    transform: Transform,
) -> Result<(), CodecError> {
    native_ident(bytes, "transform")?;
    for vector in [
        [
            transform.rows[0][0],
            transform.rows[1][0],
            transform.rows[2][0],
        ],
        [
            transform.rows[0][1],
            transform.rows[1][1],
            transform.rows[2][1],
        ],
        [
            transform.rows[0][2],
            transform.rows[1][2],
            transform.rows[2][2],
        ],
        [
            transform.rows[0][3] / 600.0,
            transform.rows[1][3] / 600.0,
            transform.rows[2][3] / 600.0,
        ],
    ] {
        native_vector(bytes, vector);
    }
    native_f64(bytes, transform.rows[3][3]);
    let hints = f3d_native(target)?
        .and_then(|native| {
            native
                .transform_hints
                .into_iter()
                .find(|hints| hints.body == body.id)
        })
        .map_or_else(
            || derived_transform_hints(transform),
            |hints| [hints.rotation, hints.reflection, hints.shear],
        );
    for hint in hints {
        bytes.push(native_bool(hint));
    }
    Ok(())
}

fn derived_transform_hints(transform: Transform) -> [bool; 3] {
    let linear = [
        [
            transform.rows[0][0],
            transform.rows[0][1],
            transform.rows[0][2],
        ],
        [
            transform.rows[1][0],
            transform.rows[1][1],
            transform.rows[1][2],
        ],
        [
            transform.rows[2][0],
            transform.rows[2][1],
            transform.rows[2][2],
        ],
    ];
    let determinant = linear[0][0] * (linear[1][1] * linear[2][2] - linear[1][2] * linear[2][1])
        - linear[0][1] * (linear[1][0] * linear[2][2] - linear[1][2] * linear[2][0])
        + linear[0][2] * (linear[1][0] * linear[2][1] - linear[1][1] * linear[2][0]);
    let reflection = determinant.is_sign_negative();
    let columns = [
        [linear[0][0], linear[1][0], linear[2][0]],
        [linear[0][1], linear[1][1], linear[2][1]],
        [linear[0][2], linear[1][2], linear[2][2]],
    ];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let scale = columns
        .iter()
        .map(|column| dot(*column, *column))
        .fold(1.0f64, f64::max);
    let shear = dot(columns[0], columns[1]).abs() > f64::EPSILON * scale
        || dot(columns[0], columns[2]).abs() > f64::EPSILON * scale
        || dot(columns[1], columns[2]).abs() > f64::EPSILON * scale;
    let rotation = linear != [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    [rotation, reflection, shear]
}

pub(crate) fn native_history_tail(bytes: &mut Vec<u8>, target: &CadIr) -> Result<(), CodecError> {
    let native = f3d_native(target)?;
    let histories = native
        .as_ref()
        .map_or(&[][..], |native| native.asm_histories.as_slice());
    if histories.is_empty() {
        native_ident(bytes, "delta_state")?;
        return Ok(());
    }
    if histories.len() != 1 {
        return Err(CodecError::NotImplemented(
            "source-less F3D generation supports one ASM history stream".into(),
        ));
    }
    let history = &histories[0];
    match (history.stream_size, history.history_entry_count) {
        (Some(stream_size), Some(history_entry_count)) => {
            if history
                .states
                .first()
                .is_none_or(|state| state.state_id != stream_size)
                || history_entry_count < 0
            {
                return Err(CodecError::Malformed(format!(
                    "F3D history {} requires head state_id == stream_size and nonnegative history_entry_count",
                    history.id
                )));
            }
            for name in ["Begin", "of", "ASM", "History"] {
                native_subident(bytes, name)?;
            }
            native_ident(bytes, "Data")?;
            native_ident(bytes, "history_stream")?;
            native_i64(bytes, stream_size);
            native_i64(bytes, stream_size);
            native_i64(bytes, 0);
            native_i64(bytes, history_entry_count);
            for reference in [-1, 0, 1, -1] {
                native_ref(bytes, reference);
            }
            bytes.push(0x11);
        }
        (None, None) => {}
        _ => {
            return Err(CodecError::Malformed(format!(
                "F3D history {} has an incomplete history-stream preamble",
                history.id
            )));
        }
    }
    for state in &history.states {
        native_ident(bytes, "delta_state")?;
        native_i64(bytes, state.state_id);
        native_i64(bytes, state.version_flag);
        native_i64(bytes, state.state_flag);
        native_ref(bytes, state.previous_ref.unwrap_or(-1));
        native_ref(bytes, state.next_ref.unwrap_or(-1));
        native_ref(bytes, state.node_index);
        native_ref(bytes, state.partner_ref.unwrap_or(-1));
        native_ref(bytes, state.owner_ref);
        bytes.push(0x0b);
        for board in &state.bulletin_boards {
            native_i64(bytes, 1);
            native_ref(bytes, board.owner_ref);
            native_i64(bytes, board.number);
            for change in &board.changes {
                if change.kind != history_change_kind(change.old_ref, change.new_ref)? {
                    return Err(CodecError::Malformed(format!(
                        "F3D entity change {} has a kind inconsistent with its references",
                        change.id
                    )));
                }
                native_i64(bytes, 1);
                native_ref(bytes, change.old_ref.unwrap_or(-1));
                native_ref(bytes, change.new_ref.unwrap_or(-1));
            }
            native_i64(bytes, 0);
        }
        native_i64(bytes, 0);
        bytes.push(0x11);
        for record in &state.records {
            bytes.extend_from_slice(&record.raw_bytes);
        }
    }
    Ok(())
}
