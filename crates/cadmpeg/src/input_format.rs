// SPDX-License-Identifier: Apache-2.0
//! Registry-backed argument values with fallible registry loading.

use std::ffi::OsStr;

use cadmpeg_registry::{ForcedInput, NativeDescriptor, RegistryLoadError};
use clap::builder::{PossibleValue, TypedValueParser};

#[derive(Clone)]
struct InputFormatParser<T> {
    rows: Result<Vec<(&'static str, T)>, RegistryLoadError>,
}

impl<T> InputFormatParser<T> {
    fn new(select: impl Fn(ForcedInput) -> Option<T>) -> Self {
        let rows = (|| {
            let mut rows = Vec::new();
            for name in cadmpeg_registry::input_names()? {
                if let Some(value) = cadmpeg_registry::forced_input(name)?.and_then(&select) {
                    rows.push((name, value));
                }
            }
            Ok(rows)
        })();
        Self { rows }
    }
}

impl<T: Clone + Send + Sync + 'static> TypedValueParser for InputFormatParser<T> {
    type Value = T;

    fn parse_ref(
        &self,
        command: &clap::Command,
        argument: Option<&clap::Arg>,
        value: &OsStr,
    ) -> Result<T, clap::Error> {
        let rows = self.rows.as_ref().map_err(|error| {
            clap::Error::raw(clap::error::ErrorKind::ValueValidation, error.to_string())
                .with_cmd(command)
        })?;
        let name = value.to_str().ok_or_else(|| {
            clap::Error::new(clap::error::ErrorKind::InvalidUtf8).with_cmd(command)
        })?;
        let ignore_case = argument.is_some_and(clap::Arg::is_ignore_case_set);
        rows.iter()
            .find(|(candidate, _)| PossibleValue::new(*candidate).matches(name, ignore_case))
            .map(|(_, value)| value.clone())
            .ok_or_else(|| {
                use clap::error::{ContextKind, ContextValue, ErrorKind};
                let mut error = clap::Error::new(ErrorKind::InvalidValue).with_cmd(command);
                error.insert(
                    ContextKind::InvalidValue,
                    ContextValue::String(name.to_owned()),
                );
                error.insert(
                    ContextKind::InvalidArg,
                    ContextValue::String(
                        argument.map_or_else(|| "...".to_owned(), ToString::to_string),
                    ),
                );
                error.insert(
                    ContextKind::ValidValue,
                    ContextValue::Strings(
                        rows.iter().map(|(name, _)| (*name).to_owned()).collect(),
                    ),
                );
                error
            })
    }

    fn possible_values(&self) -> Option<Box<dyn Iterator<Item = PossibleValue> + '_>> {
        // Clap's help metadata has no error channel. Parsing reports the stored failure.
        match &self.rows {
            Ok(rows) => Some(Box::new(
                rows.iter().map(|(name, _)| PossibleValue::new(*name)),
            )),
            Err(_) => None,
        }
    }
}

pub(crate) fn input_format_parser() -> impl TypedValueParser<Value = ForcedInput> {
    InputFormatParser::new(Some)
}

pub(crate) fn native_input_parser() -> impl TypedValueParser<Value = &'static NativeDescriptor> {
    InputFormatParser::new(|input| match input {
        ForcedInput::Codec(native) => Some(native),
        ForcedInput::Cadir => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_failure_is_not_an_unknown_value() {
        let parser = InputFormatParser::<ForcedInput> {
            rows: Err(RegistryLoadError::IdentityWithoutSupport(
                cadmpeg_core::dialect_id!("step:ap203"),
            )),
        };
        let error = parser
            .parse_ref(&clap::Command::new("test"), None, OsStr::new("step"))
            .expect_err("registry load failure must reach the argument error");
        assert_eq!(error.kind(), clap::error::ErrorKind::ValueValidation);
        assert!(error.to_string().contains("has no support row"));
        assert!(parser.possible_values().is_none());
    }

    #[test]
    fn argument_case_policy_selects_the_typed_value() {
        let argument = clap::Arg::new("from").ignore_case(true);
        assert_eq!(
            input_format_parser()
                .parse_ref(
                    &clap::Command::new("test"),
                    Some(&argument),
                    OsStr::new("JSON")
                )
                .unwrap(),
            ForcedInput::Cadir
        );
        let error = input_format_parser()
            .parse_ref(&clap::Command::new("test"), None, OsStr::new("JSON"))
            .unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::InvalidValue);
        assert!(error.to_string().contains("possible values"));
    }

    #[test]
    fn cadir_alias_is_accepted_only_by_general_input_parser() {
        let command = clap::Command::new("test");
        assert_eq!(
            input_format_parser()
                .parse_ref(&command, None, OsStr::new("json"))
                .unwrap(),
            ForcedInput::Cadir
        );
        assert!(native_input_parser()
            .parse_ref(&command, None, OsStr::new("json"))
            .is_err());
        let names = input_format_parser()
            .possible_values()
            .unwrap()
            .map(|value| value.get_name().to_owned())
            .collect::<Vec<_>>();
        assert!(names.iter().any(|name| name == "json"));
        assert!(names.iter().any(|name| name == "cadir"));
    }
}
