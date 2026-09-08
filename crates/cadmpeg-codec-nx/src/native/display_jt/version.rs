// SPDX-License-Identifier: Apache-2.0
//! Exact JT version text with a bounded decimal version token.

use crate::layout::jt_document_header;

/// An exact admitted JT version field; decoded numbers are not stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JtVersionField(String);

impl JtVersionField {
    pub(crate) fn new(field: String) -> Result<Self, &'static str> {
        if field.len() != jt_document_header::BYTE_ORDER
            || !field
                .bytes()
                .all(|byte| byte.is_ascii_graphic() || byte.is_ascii_whitespace())
        {
            return Err(
                "JtVersionField.version_field must contain 80 ASCII graphic or whitespace bytes",
            );
        }
        let token = field
            .strip_prefix("Version ")
            .and_then(|suffix| suffix.split_ascii_whitespace().next())
            .ok_or("JtVersionField.version_field lacks its Version token")?;
        let (major, minor) = token
            .split_once('.')
            .ok_or("JtVersionField.version_field lacks major.minor")?;
        major
            .parse::<u16>()
            .map_err(|_| "JtVersionField.version_field major is not u16")?;
        minor
            .parse::<u16>()
            .map_err(|_| "JtVersionField.version_field minor is not u16")?;
        Ok(Self(field))
    }

    pub(crate) fn major(&self) -> u16 {
        decimal(
            self.0
                .bytes()
                .skip(b"Version ".len())
                .skip_while(u8::is_ascii_whitespace)
                .take_while(|byte| *byte != b'.'),
        )
    }

    pub(crate) fn minor(&self) -> u16 {
        decimal(
            self.0
                .bytes()
                .skip_while(|byte| *byte != b'.')
                .skip(1)
                .take_while(|byte| !byte.is_ascii_whitespace()),
        )
    }

    pub(crate) fn into_string(self) -> String {
        self.0
    }
}

/// Decode a constructor-checked u16 token, including its optional plus sign.
fn decimal(bytes: impl Iterator<Item = u8>) -> u16 {
    bytes
        .filter(|byte| *byte != b'+')
        .fold(0, |value, digit| value * 10 + u16::from(digit - b'0'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_version_text_preserves_numeric_spelling_and_padding() {
        for (text, expected) in [
            ("Version 9.5", (9, 5)),
            ("Version \t+0009.+0010 metadata", (9, 10)),
            ("Version 65535.65535", (65535, 65535)),
        ] {
            let text = format!("{text:<80}");
            let version = JtVersionField::new(text.clone()).unwrap();
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
            assert!(JtVersionField::new(format!("{text:<80}")).is_err());
        }
        assert!(JtVersionField::new("Version 9.5".into()).is_err());
    }
}
