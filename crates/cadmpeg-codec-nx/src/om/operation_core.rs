// SPDX-License-Identifier: Apache-2.0
//! Family-qualified complete native cores; no authored construction is inferred.

mod copy_face;
mod curve_mesh;
mod frames;

use super::move_face::{CoreView as MoveFaceView, MoveFaceCore};
use super::operation_record::OperationPayload;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::native::bytes::NativeBytes;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "bytes", rename_all = "snake_case")]
pub(crate) enum OperationCore {
    MoveFace(MoveFaceCore),
    CurveMesh(#[serde(deserialize_with = "deserialize_curve_mesh")] NativeBytes<Vec<u8>>),
    CopyFace(#[serde(deserialize_with = "deserialize_copy_face")] NativeBytes<Vec<u8>>),
    Cylinder(#[serde(deserialize_with = "deserialize_cylinder")] NativeBytes<Vec<u8>>),
    MoveObject(#[serde(deserialize_with = "deserialize_move_object")] NativeBytes<Vec<u8>>),
}

fn deserialize_frame<'de, D: serde::Deserializer<'de>>(
    d: D,
    family: frames::Family,
) -> Result<NativeBytes<Vec<u8>>, D::Error> {
    let bytes = NativeBytes::<Vec<u8>>::deserialize(d)?;
    frames::read(&bytes, family)
        .filter(|frame| frame.byte_len == bytes.len())
        .ok_or_else(|| serde::de::Error::custom("core: invalid complete family frame"))?;
    Ok(bytes)
}
fn deserialize_cylinder<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<NativeBytes<Vec<u8>>, D::Error> {
    deserialize_frame(d, frames::Family::Cylinder)
}
fn deserialize_move_object<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<NativeBytes<Vec<u8>>, D::Error> {
    deserialize_frame(d, frames::Family::MoveObject)
}

fn deserialize_copy_face<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<NativeBytes<Vec<u8>>, D::Error> {
    let bytes = NativeBytes::<Vec<u8>>::deserialize(d)?;
    copy_face::read(&bytes)
        .ok_or_else(|| serde::de::Error::custom("core: invalid complete face-copy frame"))?;
    Ok(bytes)
}

fn deserialize_curve_mesh<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<NativeBytes<Vec<u8>>, D::Error> {
    let bytes = NativeBytes::<Vec<u8>>::deserialize(d)?;
    curve_mesh::read(&bytes)
        .ok_or_else(|| serde::de::Error::custom("core: invalid complete curve-mesh frame"))?;
    Ok(bytes)
}

impl OperationCore {
    pub(crate) fn as_bytes(&self) -> &[u8] {
        match self {
            Self::MoveFace(core) => core.as_bytes(),
            Self::Cylinder(bytes)
            | Self::MoveObject(bytes)
            | Self::CopyFace(bytes)
            | Self::CurveMesh(bytes) => bytes,
        }
    }
    pub(crate) fn cylinder_boolean_mode(&self) -> Option<&'static str> {
        match self {
            Self::Cylinder(bytes) => frames::read(bytes, frames::Family::Cylinder)?.mode,
            _ => None,
        }
    }
}

pub(crate) enum CoreView<'a> {
    MoveFace(MoveFaceView<'a>),
    CopyFace(&'a [u8]),
    CurveMesh(&'a [u8]),
    Cylinder(&'a [u8]),
    MoveObject(&'a [u8]),
}
impl CoreView<'_> {
    pub(crate) fn byte_len(&self) -> usize {
        match self {
            Self::MoveFace(core) => core.byte_len,
            Self::Cylinder(bytes)
            | Self::MoveObject(bytes)
            | Self::CopyFace(bytes)
            | Self::CurveMesh(bytes) => bytes.len(),
        }
    }
    pub(crate) fn retain(&self, ctx: &DecodeContext<'_>) -> Result<OperationCore, CodecError> {
        match self {
            Self::MoveFace(core) => Ok(OperationCore::MoveFace(core.retain(ctx)?)),
            Self::CurveMesh(bytes) => Ok(OperationCore::CurveMesh(
                ctx.copy_slice(bytes, "NX bounded curve-mesh core")?.into(),
            )),
            Self::CopyFace(bytes) => Ok(OperationCore::CopyFace(
                ctx.copy_slice(bytes, "NX bounded face-copy core")?.into(),
            )),
            Self::Cylinder(bytes) => Ok(OperationCore::Cylinder(
                ctx.copy_slice(bytes, "NX bounded cylinder core")?.into(),
            )),
            Self::MoveObject(bytes) => Ok(OperationCore::MoveObject(
                ctx.copy_slice(bytes, "NX bounded object-motion core")?
                    .into(),
            )),
        }
    }
}

pub(crate) fn core_for_decode<'a>(
    ctx: &DecodeContext<'_>,
    record: OperationPayload<'a>,
    classes: &[super::TypeDefinition<'_>],
) -> Result<Option<CoreView<'a>>, CodecError> {
    if record.name() == "MOVE_FACE" {
        return super::move_face::core_for_decode(ctx, record, classes)
            .map(|core| core.map(CoreView::MoveFace));
    }
    if matches!(record.name(), "COPY_FACE" | "THRU_CURVE_MESH") {
        ctx.charge_work(
            u64_from_index(record.payload().len()),
            "NX complete profile or face-copy core",
        )?;
        let selects = |selectors: &[u8], expected: &[&str]| {
            selectors.iter().zip(expected).all(|(selector, expected)| {
                classes
                    .get(usize::from(*selector) - 1)
                    .is_some_and(|class| class.name == *expected)
            })
        };
        return Ok(if record.name() == "COPY_FACE" {
            copy_face::read(record.payload())
                .filter(|s| selects(s, &copy_face::CLASSES))
                .map(|_| CoreView::CopyFace(record.payload()))
        } else {
            curve_mesh::read(record.payload())
                .filter(|s| selects(s, &curve_mesh::CLASSES))
                .map(|_| CoreView::CurveMesh(record.payload()))
        });
    }
    let (family, owner) = match record.name() {
        "CYLINDER" => (frames::Family::Cylinder, "UGS::CYLINDER_PARMS"),
        "MOVE_OBJECT" => (
            frames::Family::MoveObject,
            "UGS::ModlFeature::MoveObjectParms",
        ),
        _ => return Ok(None),
    };
    ctx.charge_work(
        u64_from_index(record.payload().len()),
        "NX complete operation core",
    )?;
    let Some(frame) = frames::read(record.payload(), family) else {
        return Ok(None);
    };
    let selects = |selector: u8, expected: &str| {
        selector
            .checked_sub(1)
            .and_then(|i| classes.get(usize::from(i)))
            .is_some_and(|class| class.name == expected)
    };
    if !selects(frame.owner, owner)
        || !selects(frame.endpoint, super::body_write::BODY_IMAGE_CLASS)
        || frame
            .motion
            .is_some_and(|selector| !selects(selector, "UGS::Modl::MotionOptions"))
    {
        return Ok(None);
    }
    if frame.byte_len != record.payload().len()
        && (!frame.null_terminal
            || !matches!(family, frames::Family::MoveObject)
            || !record
                .payload()
                .get(frame.byte_len)
                .is_some_and(|selector| selects(*selector, "UGS::OM::MethodicObject")))
    {
        return Ok(None);
    }
    let bytes = &record.payload()[..frame.byte_len];
    Ok(Some(match family {
        frames::Family::Cylinder => CoreView::Cylinder(bytes),
        frames::Family::MoveObject => CoreView::MoveObject(bytes),
    }))
}

#[cfg(test)]
mod tests;
