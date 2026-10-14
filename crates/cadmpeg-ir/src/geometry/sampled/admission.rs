// SPDX-License-Identifier: Apache-2.0
//! Explicit admission for sampled carrier construction and reconstruction.
use super::GeometryLayoutError;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

pub(super) enum ConstructionError {
    Resource(CodecError),
    Layout(GeometryLayoutError),
}
impl From<CodecError> for ConstructionError {
    fn from(error: CodecError) -> Self {
        Self::Resource(error)
    }
}

pub(super) fn finish<T>(
    result: Result<T, ConstructionError>,
) -> Result<Result<T, GeometryLayoutError>, CodecError> {
    match result {
        Ok(value) => Ok(Ok(value)),
        Err(ConstructionError::Layout(error)) => Ok(Err(error)),
        Err(ConstructionError::Resource(error)) => Err(error),
    }
}

pub(super) trait SampledAdmission {
    type Error;
    fn layout(&self, message: &'static str) -> Self::Error;
}

pub(super) struct StandardAdmission;
impl SampledAdmission for StandardAdmission {
    type Error = GeometryLayoutError;
    fn layout(&self, message: &'static str) -> Self::Error {
        GeometryLayoutError::Layout(message.into())
    }
}
impl SampledAdmission for DecodeContext<'_> {
    type Error = ConstructionError;
    fn layout(&self, message: &'static str) -> Self::Error {
        ConstructionError::Layout(GeometryLayoutError::Layout(message.to_owned()))
    }
}
