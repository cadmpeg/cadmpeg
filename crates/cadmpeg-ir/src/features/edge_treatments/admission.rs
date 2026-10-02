// SPDX-License-Identifier: Apache-2.0
//! Explicit storage and work admission for variable radius construction.
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

pub(super) enum ConstructionError {
    Resource(CodecError),
    Admission(&'static str),
}
impl From<CodecError> for ConstructionError {
    fn from(error: CodecError) -> Self { Self::Resource(error) }
}

pub(super) fn finish<T>(result: Result<T, ConstructionError>) -> Result<Result<T, &'static str>, CodecError> {
    match result {
        Ok(value) => Ok(Ok(value)),
        Err(ConstructionError::Admission(message)) => Ok(Err(message)),
        Err(ConstructionError::Resource(error)) => Err(error),
    }
}

pub(super) trait RadiusAdmission {
    type Error;
    fn work(&self, operation: &'static str) -> Result<(), Self::Error>;
    fn invalid(&self) -> Self::Error;
    fn collect<I, T>(&self, values: Vec<I>, convert: impl FnMut(I) -> Result<T, Self::Error>) -> Result<Vec<T>, Self::Error>;
}

pub(super) struct StandardAdmission;
impl RadiusAdmission for StandardAdmission {
    type Error = &'static str;
    fn work(&self, _operation: &'static str) -> Result<(), Self::Error> { Ok(()) }
    fn invalid(&self) -> Self::Error { super::INVALID_VARIABLE_RADII }
    fn collect<I, T>(&self, values: Vec<I>, convert: impl FnMut(I) -> Result<T, Self::Error>) -> Result<Vec<T>, Self::Error> {
        values.into_iter().map(convert).collect()
    }
}
impl RadiusAdmission for DecodeContext<'_> {
    type Error = ConstructionError;
    fn work(&self, operation: &'static str) -> Result<(), Self::Error> { self.charge_work(1, operation).map_err(Into::into) }
    fn invalid(&self) -> Self::Error { ConstructionError::Admission(super::INVALID_VARIABLE_RADII) }
    fn collect<I, T>(&self, values: Vec<I>, convert: impl FnMut(I) -> Result<T, Self::Error>) -> Result<Vec<T>, Self::Error> {
        self.try_collect_retained_with(values, "IR variable radius admitted samples", convert)
    }
}
