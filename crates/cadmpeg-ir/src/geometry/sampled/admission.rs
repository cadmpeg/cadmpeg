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
    fn all_by<T>(&self, values: &[T], operation: &'static str,
        predicate: impl FnMut(&T) -> bool) -> Result<bool, Self::Error>;
    fn layout(&self, message: &'static str) -> Result<Self::Error, Self::Error>;
    fn collect<I: IntoIterator, T>(
        &self,
        values: I,
        operation: &'static str,
        convert: impl FnMut(I::Item) -> Result<T, Self::Error>,
    ) -> Result<Vec<T>, Self::Error>;
}

pub(super) struct StandardAdmission;
impl SampledAdmission for StandardAdmission {
    type Error = GeometryLayoutError;
    fn all_by<T>(&self, values: &[T], _operation: &'static str,
        predicate: impl FnMut(&T) -> bool) -> Result<bool, Self::Error> {
        Ok(values.iter().all(predicate))
    }
    fn layout(&self, message: &'static str) -> Result<Self::Error, Self::Error> {
        Ok(GeometryLayoutError::Layout(message.into()))
    }
    fn collect<I: IntoIterator, T>(
        &self,
        values: I,
        _operation: &'static str,
        convert: impl FnMut(I::Item) -> Result<T, Self::Error>,
    ) -> Result<Vec<T>, Self::Error> {
        values.into_iter().map(convert).collect()
    }
}
impl SampledAdmission for DecodeContext<'_> {
    type Error = ConstructionError;
    fn all_by<T>(&self, values: &[T], operation: &'static str,
        mut predicate: impl FnMut(&T) -> bool) -> Result<bool, Self::Error> {
        self.all_by_limit(values, |value| Ok(predicate(value)), operation)
            .map_err(|limit| ConstructionError::Resource(limit.into()))
    }
    fn layout(&self, message: &'static str) -> Result<Self::Error, Self::Error> {
        Ok(ConstructionError::Layout(GeometryLayoutError::Layout(
            self.copy_retained_text(message, "IR sampled construction refusal")?,
        )))
    }
    fn collect<I: IntoIterator, T>(
        &self,
        values: I,
        operation: &'static str,
        convert: impl FnMut(I::Item) -> Result<T, Self::Error>,
    ) -> Result<Vec<T>, Self::Error> {
        self.try_collect_retained_with(values, operation, convert)
    }
}
