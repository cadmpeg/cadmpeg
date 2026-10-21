// SPDX-License-Identifier: Apache-2.0
//! Family-qualified complete native cores; no authored construction is inferred.

mod copy_face;
mod curve_mesh;
mod delete_face;
mod frames;
mod thread;

use super::move_face::{CoreView as MoveFaceView, MoveFaceCore};
use super::operation_record::OperationPayload;
use cadmpeg_core::CodecError;
use cadmpeg_ir::native::bytes::NativeBytes;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "bytes", rename_all = "snake_case")]
pub(crate) enum OperationCore {
    DeleteFace(#[serde(deserialize_with = "deserialize_delete_face")] NativeBytes<Vec<u8>>),
    Threads(#[serde(deserialize_with = "deserialize_threads")] NativeBytes<Vec<u8>>),
    DetailedThread(#[serde(deserialize_with = "deserialize_detailed_thread")] NativeBytes<Vec<u8>>),
    MoveFace(MoveFaceCore),
    CurveMesh(#[serde(deserialize_with = "deserialize_curve_mesh")] NativeBytes<Vec<u8>>),
    CopyFace(#[serde(deserialize_with = "deserialize_copy_face")] NativeBytes<Vec<u8>>),
    Cylinder(#[serde(deserialize_with = "deserialize_cylinder")] NativeBytes<Vec<u8>>),
    Cone(#[serde(deserialize_with = "deserialize_cone")] NativeBytes<Vec<u8>>),
    Sphere(#[serde(deserialize_with = "deserialize_sphere")] NativeBytes<Vec<u8>>),
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
fn deserialize_cone<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<NativeBytes<Vec<u8>>, D::Error> {
    deserialize_frame(d, frames::Family::Cone)
}
fn deserialize_sphere<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<NativeBytes<Vec<u8>>, D::Error> {
    deserialize_frame(d, frames::Family::Sphere)
}

fn deserialize_delete_face<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<NativeBytes<Vec<u8>>, D::Error> {
    let bytes = NativeBytes::<Vec<u8>>::deserialize(d)?;
    delete_face::read(&bytes)
        .ok_or_else(|| serde::de::Error::custom("core: invalid complete delete_face frame"))?;
    Ok(bytes)
}

fn deserialize_threads<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<NativeBytes<Vec<u8>>, D::Error> {
    let bytes = NativeBytes::<Vec<u8>>::deserialize(d)?;
    thread::read(&bytes, thread::Family::Threads)
        .ok_or_else(|| serde::de::Error::custom("core: invalid complete threads frame"))?;
    Ok(bytes)
}

fn deserialize_detailed_thread<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<NativeBytes<Vec<u8>>, D::Error> {
    let bytes = NativeBytes::<Vec<u8>>::deserialize(d)?;
    thread::read(&bytes, thread::Family::DetailedThread)
        .ok_or_else(|| serde::de::Error::custom("core: invalid complete detailed_thread frame"))?;
    Ok(bytes)
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
            Self::Threads(bytes)
            | Self::DetailedThread(bytes)
            | Self::DeleteFace(bytes)
            | Self::Cylinder(bytes)
            | Self::Cone(bytes)
            | Self::Sphere(bytes)
            | Self::MoveObject(bytes)
            | Self::CopyFace(bytes)
            | Self::CurveMesh(bytes) => bytes,
        }
    }
    pub(crate) fn boolean_property(&self) -> Option<(&'static str, &'static str)> {
        let (bytes, family, key) = match self {
            Self::Cylinder(bytes) => (bytes, frames::Family::Cylinder, "cylinder.boolean_mode"),
            Self::Cone(bytes) => (bytes, frames::Family::Cone, "cone.boolean_mode"),
            Self::Sphere(bytes) => (bytes, frames::Family::Sphere, "sphere.boolean_mode"),
            _ => return None,
        };
        Some((key, frames::read(bytes, family)?.mode?))
    }
}

pub(crate) enum CoreView<'a> {
    MoveFace(MoveFaceView<'a>),
    DeleteFace(&'a [u8]),
    Threads(&'a [u8]),
    DetailedThread(&'a [u8]),
    CopyFace(&'a [u8]),
    CurveMesh(&'a [u8]),
    Cylinder(&'a [u8]),
    Cone(&'a [u8]),
    Sphere(&'a [u8]),
    MoveObject(&'a [u8]),
}
impl CoreView<'_> {
    pub(crate) fn byte_len(&self) -> usize {
        match self {
            Self::MoveFace(core) => core.byte_len,
            Self::Threads(bytes)
            | Self::DetailedThread(bytes)
            | Self::DeleteFace(bytes)
            | Self::Cylinder(bytes)
            | Self::Cone(bytes)
            | Self::Sphere(bytes)
            | Self::MoveObject(bytes)
            | Self::CopyFace(bytes)
            | Self::CurveMesh(bytes) => bytes.len(),
        }
    }
    pub(crate) fn retain(&self) -> Result<OperationCore, CodecError> {
        match self {
            Self::MoveFace(core) => Ok(OperationCore::MoveFace(core.retain()?)),
            Self::DeleteFace(bytes) => Ok(OperationCore::DeleteFace((bytes).to_vec().into())),
            Self::Threads(bytes) => Ok(OperationCore::Threads((bytes).to_vec().into())),
            Self::DetailedThread(bytes) => {
                Ok(OperationCore::DetailedThread((bytes).to_vec().into()))
            }
            Self::CurveMesh(bytes) => Ok(OperationCore::CurveMesh((bytes).to_vec().into())),
            Self::CopyFace(bytes) => Ok(OperationCore::CopyFace((bytes).to_vec().into())),
            Self::Cylinder(bytes) => Ok(OperationCore::Cylinder((bytes).to_vec().into())),
            Self::Cone(bytes) => Ok(OperationCore::Cone((bytes).to_vec().into())),
            Self::Sphere(bytes) => Ok(OperationCore::Sphere((bytes).to_vec().into())),
            Self::MoveObject(bytes) => Ok(OperationCore::MoveObject((bytes).to_vec().into())),
        }
    }
}

pub(crate) fn core_for_decode<'a>(
    record: OperationPayload<'a>,
    classes: &[super::TypeDefinition<'_>],
) -> Option<CoreView<'a>> {
    let selects = |selector: u8, expected: &str| {
        selector
            .checked_sub(1)
            .and_then(|i| classes.get(usize::from(i)))
            .is_some_and(|class| class.name == expected)
    };
    if record.name() == "MOVE_FACE" {
        return super::move_face::core_for_decode(record, classes).map(CoreView::MoveFace);
    }
    if matches!(record.name(), "COPY_FACE" | "THRU_CURVE_MESH") {
        return if record.name() == "COPY_FACE" {
            copy_face::read(record.payload())
                .filter(|s| {
                    s.iter()
                        .zip(copy_face::CLASSES)
                        .all(|(s, name)| selects(*s, name))
                })
                .map(|_| CoreView::CopyFace(record.payload()))
        } else {
            curve_mesh::read(record.payload())
                .filter(|s| {
                    s.iter()
                        .zip(curve_mesh::CLASSES)
                        .all(|(s, name)| selects(*s, name))
                })
                .map(|_| CoreView::CurveMesh(record.payload()))
        };
    }
    if matches!(record.name(), "DELETE FACE" | "THREADS" | "DETAILED_THREAD") {
        if record.name() == "DELETE FACE" {
            return delete_face::read(record.payload())
                .filter(|s| {
                    s.iter()
                        .zip(delete_face::CLASSES)
                        .all(|(s, name)| selects(*s, name))
                })
                .map(|_| CoreView::DeleteFace(record.payload()));
        }
        let family = if record.name() == "THREADS" {
            thread::Family::Threads
        } else {
            thread::Family::DetailedThread
        };
        return thread::read(record.payload(), family)
            .filter(|s| {
                selects(s[0], thread::OWNER) && selects(s[1], super::body_write::BODY_IMAGE_CLASS)
            })
            .map(|_| {
                if record.name() == "THREADS" {
                    CoreView::Threads(record.payload())
                } else {
                    CoreView::DetailedThread(record.payload())
                }
            });
    }
    let (family, owner) = match record.name() {
        "CYLINDER" => (frames::Family::Cylinder, "UGS::CYLINDER_PARMS"),
        "CONE" => (frames::Family::Cone, "UGS::CONE_PARMS"),
        "SPHERE" => (frames::Family::Sphere, "UGS::SPHERE_PARMS"),
        "MOVE_OBJECT" => (
            frames::Family::MoveObject,
            "UGS::ModlFeature::MoveObjectParms",
        ),
        _ => return None,
    };
    let frame = frames::read(record.payload(), family)?;
    if !selects(frame.owner, owner)
        || !selects(frame.endpoint, super::body_write::BODY_IMAGE_CLASS)
        || frame
            .motion
            .is_some_and(|selector| !selects(selector, "UGS::Modl::MotionOptions"))
    {
        return None;
    }
    if frame.byte_len != record.payload().len()
        && (!frame.null_terminal
            || !matches!(family, frames::Family::MoveObject)
            || !record
                .payload()
                .get(frame.byte_len)
                .is_some_and(|selector| selects(*selector, "UGS::OM::MethodicObject")))
    {
        return None;
    }
    let bytes = &record.payload()[..frame.byte_len];
    Some(match family {
        frames::Family::Cylinder => CoreView::Cylinder(bytes),
        frames::Family::Cone => CoreView::Cone(bytes),
        frames::Family::Sphere => CoreView::Sphere(bytes),
        frames::Family::MoveObject => CoreView::MoveObject(bytes),
    })
}

#[cfg(test)]
mod tests;
