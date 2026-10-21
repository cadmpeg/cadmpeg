// SPDX-License-Identifier: Apache-2.0
//! Interpret typed native text without replacing its source-stated bytes.

use cadmpeg_ir::native::bytes::NativeBytes;

pub(crate) fn decoded<T: AsRef<[u8]>>(value: &NativeBytes<T>) -> Option<&str> {
    std::str::from_utf8(value.as_ref()).ok()
}

pub(crate) fn field(value: &serde_json::Value) -> Result<Vec<u8>, String> {
    serde_json::from_value::<NativeBytes>(value.clone())
        .map(|bytes| bytes.as_ref().to_vec())
        .map_err(|error| format!("native text has no exact byte encoding: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_utf8_text_retains_its_exact_bytes() {
        let bytes = NativeBytes::from("Rib α".as_bytes());
        assert_eq!(decoded(&bytes), Some("Rib α"));
        assert_eq!(
            serde_json::to_value(bytes).unwrap(),
            serde_json::json!("52696220ceb1")
        );
    }

    #[test]
    fn non_utf8_text_retains_its_exact_bytes_without_digest_substitution() {
        let source = [0xff, 0xfe, 0x80];
        let bytes = NativeBytes::from(source);
        assert_eq!(decoded(&bytes), None);
        let value = serde_json::to_value(bytes).unwrap();
        assert_eq!(value, serde_json::json!("fffe80"));
        assert_eq!(field(&value).unwrap(), source);
    }
}
