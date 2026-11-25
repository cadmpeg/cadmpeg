// SPDX-License-Identifier: Apache-2.0
//! Parasolid stream framing.

use cadmpeg_core::decode::{ParseError, View};
use cadmpeg_core::CodecError;

#[derive(Debug)]
pub(crate) struct PartitionError(CodecError);

impl From<ParseError> for PartitionError {
    fn from(error: ParseError) -> Self {
        Self(error.into())
    }
}

impl From<PartitionError> for CodecError {
    fn from(error: PartitionError) -> Self {
        error.0
    }
}

pub(crate) fn payload(mut stream: View<'_>) -> Result<View<'_>, PartitionError> {
    if stream.window().starts_with(b"PS") {
        return Ok(stream);
    }
    let marker = b"**END_OF_HEADER";
    let bytes = stream.window();
    let start = bytes
        .windows(marker.len())
        .position(|bytes| bytes == marker)
        .ok_or_else(|| {
            PartitionError(CodecError::Malformed(
                "Parasolid text header is incomplete".into(),
            ))
        })?;
    let end = bytes
        .get(start..)
        .and_then(|bytes| bytes.iter().position(|byte| *byte == b'\n'))
        .and_then(|length| start.checked_add(length + 1))
        .ok_or_else(|| {
            PartitionError(CodecError::Malformed(
                "Parasolid header line is incomplete".into(),
            ))
        })?;
    stream.req_take(end)?;
    let payload = stream.req_take_child(stream.remaining())?;
    if !payload.window().starts_with(b"PS") {
        return Err(PartitionError(CodecError::Malformed(
            "Parasolid binary signature is absent".into(),
        )));
    }
    Ok(payload)
}

pub(crate) fn schema(
    mut payload: View<'_>,
) -> Result<cadmpeg_parasolid::OwnedSchemaToken, PartitionError> {
    payload.req_take(2)?;
    let description_len = payload.req_u32_be()?;
    let count = payload
        .counted(u64::from(description_len), 1)
        .ok_or_else(|| {
            PartitionError(CodecError::Malformed(
                "invalid Parasolid description length".into(),
            ))
        })?;
    payload.req_take(count.get())?;
    let schema_len = payload.req_u32_be()?;
    let count = payload.counted(u64::from(schema_len), 1).ok_or_else(|| {
        PartitionError(CodecError::Malformed(
            "invalid Parasolid schema length".into(),
        ))
    })?;
    let bytes = payload.req_take(count.get())?;
    cadmpeg_parasolid::find_schema_token(bytes)
        .filter(|token| token.value().len() == bytes.len())
        .map(Into::into)
        .ok_or_else(|| {
            PartitionError(CodecError::Malformed(
                "invalid Parasolid schema declaration".into(),
            ))
        })
}

pub(crate) fn is_primary(path: &str) -> bool {
    let Some((storage, name)) = path.split_once('/') else {
        return false;
    };
    storage.eq_ignore_ascii_case("PARASOLID")
        && name
            .to_ascii_uppercase()
            .strip_prefix("STREAM")
            .and_then(|name| name.strip_suffix(".P_B"))
            .is_some_and(|index| {
                !index.is_empty() && index.bytes().all(|byte| byte.is_ascii_digit())
            })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_length_follows_its_terminal_line() {
        for prefix in [b"**PART1;\n".as_slice(), b"**PART1;\nKEY=STREAM12;\n"] {
            let mut bytes = prefix.to_vec();
            bytes.extend_from_slice(b"**END_OF_HEADER****\nPS\0\0");
            let root = View::over_retained(&bytes);
            let inner = payload(root).expect("complete transmit header");
            assert_eq!(inner.window(), b"PS\0\0");
            assert_eq!(inner.location().space, root.location().space);
        }
        assert!(payload(View::over_retained(b"**END_OF_HEADER")).is_err());
        assert!(payload(View::over_retained(b"**END_OF_HEADER\nZZ")).is_err());
    }

    #[test]
    fn schema_uses_the_declared_header_field() {
        let description = b": TRANSMIT FILE (partition) SCH_DESCRIPTION";
        let declared = b"SCH_999_13006";
        let mut bytes = b"PS".to_vec();
        bytes.extend_from_slice(
            &u32::try_from(description.len())
                .expect("description length")
                .to_be_bytes(),
        );
        bytes.extend_from_slice(description);
        bytes.extend_from_slice(
            &u32::try_from(declared.len())
                .expect("schema length")
                .to_be_bytes(),
        );
        bytes.extend_from_slice(declared);
        assert_eq!(
            schema(View::over_retained(&bytes))
                .expect("schema field")
                .value(),
            "SCH_999_13006"
        );
    }

    #[test]
    fn primary_paths_have_a_numeric_stream_index() {
        assert!(is_primary("PARASOLID/STREAM0.P_B"));
        assert!(is_primary("parasolid/stream12.p_b"));
        for path in [
            "STREAM0.P_B",
            "PARASOLID/STREAM.P_B",
            "PARASOLID/STREAM0.D_B",
            "PARASOLID/STREAM0A.P_B",
            "PARASOLID/STREAM0.P_B.bak",
            "PARASOLID/sub/STREAM0.P_B",
            "other/STREAM0.P_B",
        ] {
            assert!(!is_primary(path));
        }
    }
}
