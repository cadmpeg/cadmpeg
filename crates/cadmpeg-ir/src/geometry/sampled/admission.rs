// SPDX-License-Identifier: Apache-2.0
//! Explicit admission for sampled carrier construction and reconstruction.
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use super::GeometryLayoutError;

pub(super) enum ConstructionError {
    Resource(CodecError),
    Layout(GeometryLayoutError),
}
impl From<CodecError> for ConstructionError {
    fn from(error: CodecError) -> Self { Self::Resource(error) }
}

pub(super) fn finish<T>(result: Result<T, ConstructionError>) -> Result<Result<T, GeometryLayoutError>, CodecError> {
    match result {
        Ok(value) => Ok(Ok(value)),
        Err(ConstructionError::Layout(error)) => Ok(Err(error)),
        Err(ConstructionError::Resource(error)) => Err(error),
    }
}

pub(super) trait SampledAdmission {
    type Error;
    fn work(&self, count: u64, operation: &'static str) -> Result<(), Self::Error>;
    fn layout(&self, message: &'static str) -> Result<Self::Error, Self::Error>;
    fn collect<I, T>(&self, values: Vec<I>, operation: &'static str, convert: impl FnMut(I) -> Result<T, Self::Error>) -> Result<Vec<T>, Self::Error>;
}

pub(super) struct StandardAdmission;
impl SampledAdmission for StandardAdmission {
    type Error = GeometryLayoutError;
    fn work(&self, _count: u64, _operation: &'static str) -> Result<(), Self::Error> { Ok(()) }
    fn layout(&self, message: &'static str) -> Result<Self::Error, Self::Error> { Ok(GeometryLayoutError::Layout(message.into())) }
    fn collect<I, T>(&self, values: Vec<I>, _operation: &'static str, convert: impl FnMut(I) -> Result<T, Self::Error>) -> Result<Vec<T>, Self::Error> {
        values.into_iter().map(convert).collect()
    }
}
impl SampledAdmission for DecodeContext<'_> {
    type Error = ConstructionError;
    fn work(&self, count: u64, operation: &'static str) -> Result<(), Self::Error> { self.charge_work(count, operation).map_err(Into::into) }
    fn layout(&self, message: &'static str) -> Result<Self::Error, Self::Error> {
        Ok(ConstructionError::Layout(GeometryLayoutError::Layout(self.copy_retained_text(message, "IR sampled construction refusal")?)))
    }
    fn collect<I, T>(&self, values: Vec<I>, operation: &'static str, convert: impl FnMut(I) -> Result<T, Self::Error>) -> Result<Vec<T>, Self::Error> {
        self.try_collect_retained_with(values, operation, convert)
    }
}
