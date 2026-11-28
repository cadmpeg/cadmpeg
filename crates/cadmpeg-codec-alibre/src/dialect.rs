// SPDX-License-Identifier: Apache-2.0
//! Alibre dialect identity: which registry row a part is, and how it was
//! admitted.
//!
//! The host row follows the leading bytes of the `SAB_DATA` stream. The magic
//! `ACIS BinaryFile` with a framed header selects `alibre:cfb-acis-binary`,
//! admitted. Any other `SAB_DATA` content is `alibre:unknown`, refused: decode
//! returns [`cadmpeg_core::CodecError::UnsupportedDialect`].
//! The ACIS save format is banded on the separate `acis:` kernel layer owned by
//! `cadmpeg-asm`.

use cadmpeg_asm::acis_header;
use cadmpeg_asm::dialect::KernelHeaderRef;
use cadmpeg_asm::kernel_header::BinaryHeader;
use cadmpeg_core::dialect::{DialectLayers, DialectMatch};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;

use crate::loss::AlibreLossCode;

#[path = "dialect/registry_ids.rs"]
pub(crate) mod registry_ids;
use self::registry_ids::{ALIBRE_CFB_ACIS_BINARY, ALIBRE_UNKNOWN};

/// The `SAB_DATA` stream's kernel header and the offset its record stream starts at.
pub(crate) struct KernelStream {
    pub(crate) header: BinaryHeader,
    pub(crate) records_at: usize,
}

/// Parses the ACIS binary header of a `SAB_DATA` stream.
///
/// `None` when the stream has no ACIS binary magic or no record-stream
/// boundary.
pub(crate) fn kernel_stream(bytes: &[u8]) -> Option<KernelStream> {
    let header = acis_header::parse(bytes)?;
    let records_at = acis_header::record_stream_start_with_header(bytes, &header)?;
    Some(KernelStream { header, records_at })
}

/// The host and kernel matches of one part.
pub(crate) struct Classification {
    pub(crate) host: DialectMatch,
    pub(crate) kernel: Option<DialectMatch>,
}

/// Classifies a part from its `SAB_DATA` bytes. The single construction path
/// for a [`DialectMatch`] in this codec.
pub(crate) fn classify(kernel: Option<&KernelStream>) -> Classification {
    match kernel {
        Some(stream) => Classification {
            host: DialectMatch::admitted(ALIBRE_CFB_ACIS_BINARY),
            kernel: Some(cadmpeg_asm::dialect::classify(KernelHeaderRef::Acis(
                &stream.header,
            ))),
        },
        None => Classification {
            host: DialectMatch::refused(ALIBRE_UNKNOWN),
            kernel: None,
        },
    }
}

impl Classification {
    /// The recovery loss the kernel layer charges, if it recovered.
    pub(crate) fn kernel_loss(&self) -> Option<LossNote> {
        let kernel = self.kernel.as_ref()?;
        cadmpeg_asm::dialect::unverified_message("the SAB_DATA stream", kernel)
            .map(|message| AlibreLossCode::SourceDialectUnverified.note(message))
    }

    pub(crate) fn layers(self) -> Result<DialectLayers, CodecError> {
        let layers = DialectLayers::of(self.host);
        match self.kernel {
            Some(kernel) => layers.with(kernel),
            None => Ok(layers),
        }
        .map_err(CodecError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::registry_ids::{ALIBRE_CFB_ACIS_BINARY, ALIBRE_UNKNOWN};
    use super::{classify, kernel_stream};
    use cadmpeg_core::dialect::Admission;

    #[test]
    fn a_stream_without_the_acis_magic_is_refused() {
        assert!(kernel_stream(b"ISO-10303-21;").is_none());
        let classification = classify(None);
        assert_eq!(classification.host.dialect(), &ALIBRE_UNKNOWN);
        assert_eq!(classification.host.admission(), &Admission::Refused);
        assert!(classification.kernel.is_none());
        assert!(classification.kernel_loss().is_none());
    }

    #[test]
    fn an_acis_stream_is_admitted_with_a_kernel_layer() {
        let mut bytes = b"ACIS BinaryFile".to_vec();
        for value in [3300_u32, 0, 1, 0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in ["Alibre, Inc", "ACIS 33.0.1 NT", "date"] {
            bytes.push(0x07);
            bytes.push(u8::try_from(value.len()).unwrap());
            bytes.extend_from_slice(value.as_bytes());
        }
        for value in [10.0_f64, 1.0e-6, 1.0e-10] {
            bytes.push(0x06);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let stream = kernel_stream(&bytes).unwrap();
        assert_eq!(stream.records_at, bytes.len());
        let classification = classify(Some(&stream));
        assert_eq!(classification.host.dialect(), &ALIBRE_CFB_ACIS_BINARY);
        assert_eq!(classification.host.admission(), &Admission::Admitted);
        let kernel = classification.kernel.as_ref().unwrap();
        assert!(matches!(kernel.admission(), Admission::Unverified { .. }));
        assert!(classification.kernel_loss().is_some());
    }
}
