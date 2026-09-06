// SPDX-License-Identifier: Apache-2.0
//! Length-framed NX product text and its source header forms.

use crate::printable_string::PrintableString;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(transparent)]
pub(crate) struct ProductText<S>(PrintableString<S>);

impl<S: AsRef<str>> ProductText<S> {
    pub(crate) fn new(value: S) -> Result<Self, &'static str> {
        let value = PrintableString::new(value)
            .map_err(|_| "product_version/version: requires printable ASCII")?;
        if !value.as_str().starts_with("NX ") || value.as_str().len() > 253 {
            return Err("product_version/version: requires NX-prefixed text of at most 253 bytes");
        }
        Ok(Self(value))
    }

    pub(crate) fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl ProductText<&str> {
    pub(crate) fn into_owned(self) -> ProductText<String> {
        ProductText(self.0.into_owned())
    }
}

impl<'de> serde::Deserialize<'de> for ProductText<String> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = <String as serde::Deserialize>::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ProductRecordForm {
    Modern,
    LegacyFeature,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ProductRecord<'a> {
    form: ProductRecordForm,
    text: ProductText<&'a str>,
}

impl<'a> ProductRecord<'a> {
    pub(crate) fn read(bytes: &'a [u8], form: ProductRecordForm) -> Option<Self> {
        let (length_offset, text_start): (usize, usize) = match form {
            ProductRecordForm::Modern if matches!(bytes.get(..2), Some([0x04 | 0x05, 0x01])) => {
                (2, 3)
            }
            ProductRecordForm::LegacyFeature if bytes.first() == Some(&0x01) => (1, 2),
            _ => return None,
        };
        let text_length = usize::from(*bytes.get(length_offset)?).checked_sub(2)?;
        let text_end = text_start.checked_add(text_length)?;
        let text =
            ProductText::new(std::str::from_utf8(bytes.get(text_start..text_end)?).ok()?).ok()?;
        (bytes.get(text_end) == Some(&0)).then_some(Self { form, text })
    }

    pub(crate) fn text(self) -> ProductText<&'a str> {
        self.text
    }

    pub(crate) fn byte_len(self) -> usize {
        let header_len = match self.form {
            ProductRecordForm::Modern => 3,
            ProductRecordForm::LegacyFeature => 2,
        };
        header_len + self.text.as_str().len() + 1
    }
}

#[cfg(test)]
mod tests {
    use super::{ProductRecord, ProductRecordForm, ProductText};

    #[test]
    fn product_text_preserves_wire_and_length_bound() {
        let text = format!("NX {}", "x".repeat(250));
        let value = ProductText::new(text.as_str()).unwrap().into_owned();
        let wire = serde_json::to_string(&value).unwrap();
        assert_eq!(wire, serde_json::to_string(&text).unwrap());
        assert_eq!(
            serde_json::from_str::<ProductText<String>>(&wire).unwrap(),
            value
        );
        for text in ["NX", "NX μ", "NX \n", &format!("NX {}", "x".repeat(251))] {
            assert!(ProductText::new(text).is_err());
            let error =
                serde_json::from_str::<ProductText<String>>(&serde_json::to_string(text).unwrap())
                    .unwrap_err();
            assert!(error.to_string().contains("product_version/version"));
        }
    }

    #[test]
    fn product_frames_derive_lengths_for_both_modern_markers_and_legacy() {
        for marker in [4, 5] {
            let frame = [marker, 1, 5, b'N', b'X', b' ', 0];
            let product = ProductRecord::read(&frame, ProductRecordForm::Modern).unwrap();
            assert_eq!(product.text().as_str(), "NX ");
            assert_eq!(product.byte_len(), frame.len());
        }
        let frame = [1, 5, b'N', b'X', b' ', 0];
        assert_eq!(
            ProductRecord::read(&frame, ProductRecordForm::LegacyFeature)
                .unwrap()
                .byte_len(),
            frame.len()
        );
        assert!(ProductRecord::read(&frame[..5], ProductRecordForm::LegacyFeature).is_none());
    }
}
