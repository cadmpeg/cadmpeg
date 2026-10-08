// SPDX-License-Identifier: Apache-2.0
//! Exact JT version text with a bounded decimal version token.

use crate::layout::jt_document_header;

/// Exact admitted JT version text and its validated decimal numbers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct JtVersionField {
    text: [u8; jt_document_header::BYTE_ORDER],
    major: u16,
    minor: u16,
}

impl JtVersionField {
    pub(super) fn new(field: &str) -> Result<Self, &'static str> {
        if field.len() != jt_document_header::BYTE_ORDER
            || !field
                .bytes()
                .all(|byte| byte.is_ascii_graphic() || byte.is_ascii_whitespace())
        {
            return Err(
                "JtVersionField.version_field must contain 80 ASCII graphic or whitespace bytes",
            );
        }
        let Some(token) = field
            .strip_prefix("Version ")
            .and_then(|suffix| suffix.split_ascii_whitespace().next())
        else {
            return Err("JtVersionField.version_field lacks its Version token");
        };
        let Some((major, minor)) = token.split_once('.') else {
            return Err("JtVersionField.version_field lacks major.minor");
        };
        let Ok(major) = major.parse::<u16>() else {
            return Err("JtVersionField.version_field major is not u16");
        };
        let Ok(minor) = minor.parse::<u16>() else {
            return Err("JtVersionField.version_field minor is not u16");
        };
        let mut text = [0; jt_document_header::BYTE_ORDER];
        text.copy_from_slice(field.as_bytes());
        Ok(Self { text, major, minor })
    }

    pub(super) fn major(&self) -> u16 {
        self.major
    }

    pub(super) fn minor(&self) -> u16 {
        self.minor
    }

    #[cfg(test)]
    pub(super) fn into_string(self) -> String {
        self.as_str().to_owned()
    }

    pub(super) fn as_str(&self) -> &str {
        // SAFETY: new admits exactly 80 ASCII bytes before copying them into
        // this private array. Clone preserves those bytes; no method mutates it.
        unsafe { std::str::from_utf8_unchecked(&self.text) }
    }
}

#[cfg(test)]
mod tests {
    use super::JtVersionField;

    fn version_from_text(text: String) -> Result<JtVersionField, &'static str> {
        JtVersionField::new(&text)
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
    fn fixed_version_field_needs_no_work_or_heap_admission() {
        let text = format!("{:<80}", "Version +0009.+0010 metadata");
        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_work_units = 0;
                policy.limits.max_materialized_bytes = 0;
                policy.limits.max_retained_bytes = 0;
            },
            |ctx| {
                let version = JtVersionField::new(&text).unwrap();
                assert_eq!((version.major(), version.minor()), (9, 10));
                assert_eq!(version.as_str(), text);
                assert!(ctx.resource_refusal().is_none());
            },
        );
    }
}
