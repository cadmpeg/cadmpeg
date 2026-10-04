// SPDX-License-Identifier: Apache-2.0
//! Exact JT version text with a bounded decimal version token.

use crate::layout::jt_document_header;

/// Exact admitted JT version text and its validated decimal numbers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct JtVersionField {
    text: String,
    major: u16,
    minor: u16,
}

impl JtVersionField {
    pub(super) fn new<S: AsRef<str>, E>(
        source: S,
        retain: impl FnOnce(S) -> Result<String, E>,
        mut parse: impl FnMut(&str, &'static str) -> Result<Result<u16, std::num::ParseIntError>, E>,
    ) -> Result<Result<Self, &'static str>, E> {
        let field = source.as_ref();
        if field.len() != jt_document_header::BYTE_ORDER
            || !field
                .bytes()
                .all(|byte| byte.is_ascii_graphic() || byte.is_ascii_whitespace())
        {
            return Ok(Err(
                "JtVersionField.version_field must contain 80 ASCII graphic or whitespace bytes",
            ));
        }
        let Some(token) = field
            .strip_prefix("Version ")
            .and_then(|suffix| suffix.split_ascii_whitespace().next())
        else {
            return Ok(Err("JtVersionField.version_field lacks its Version token"));
        };
        let Some((major, minor)) = token.split_once('.') else {
            return Ok(Err("JtVersionField.version_field lacks major.minor"));
        };
        let Ok(major) = parse(major, "NX JT version major")? else {
            return Ok(Err("JtVersionField.version_field major is not u16"));
        };
        let Ok(minor) = parse(minor, "NX JT version minor")? else {
            return Ok(Err("JtVersionField.version_field minor is not u16"));
        };
        let text = retain(source)?;
        Ok(Ok(Self { text, major, minor }))
    }

    pub(super) fn major(&self) -> u16 {
        self.major
    }

    pub(super) fn minor(&self) -> u16 {
        self.minor
    }

    #[cfg(test)]
    pub(super) fn into_string(self) -> String {
        self.text
    }

    pub(super) fn as_str(&self) -> &str {
        &self.text
    }
}

#[cfg(test)]
mod tests {
    use super::JtVersionField;
    use std::convert::Infallible;

    fn version_from_text(text: String) -> Result<JtVersionField, &'static str> {
        match JtVersionField::new(
            text,
            |text| Ok::<_, Infallible>(text),
            |text, _| Ok(text.parse()),
        ) {
            Ok(value) => value,
            Err(error) => match error {},
        }
    }

    #[test]
    fn exact_version_text_preserves_numeric_spelling_and_padding() {
        for (text, expected) in [
            ("Version 9.5", (9, 5)),
            ("Version \t+0009.+0010 metadata", (9, 10)),
            ("Version 65535.65535", (65535, 65535)),
        ] {
            let text = format!("{text:<80}");
            let version = version_from_text(text.clone()).unwrap();
            assert_eq!((version.major(), version.minor()), expected);
            assert_eq!(version.into_string(), text);
        }
        for text in [
            "Version 65536.0",
            "Version 9.-1",
            "Version 9.x",
            "Version 9.5.1",
            "Version .5",
            "9.5",
        ] {
            assert!(version_from_text(format!("{text:<80}")).is_err());
        }
        assert!(version_from_text("Version 9.5".into()).is_err());
    }


    #[test]
    fn version_major_and_minor_parse_refusals_propagate() {
        use cadmpeg_core::decode::ResourceDimension;
        for operation in ["NX JT version major", "NX JT version minor"] {
            let text = format!("{:<80}", "Version +0009.+0010 metadata");
            let error = crate::test_support::resource_refusal_at(
                &[], ResourceDimension::WorkUnits, operation,
                |ctx| JtVersionField::new(
                    text.as_str(),
                    |text| ctx.copy_retained_text(text, "retain DisplayJT version text"),
                    |text, operation| ctx.parse_text(text, operation),
                ).map(|_| ()),
            );
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == operation && limit.dimension == ResourceDimension::WorkUnits));
        }
    }
}
