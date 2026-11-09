// SPDX-License-Identifier: Apache-2.0
//! Encode failures keep their kind when synthesis ends the write.

use super::super::content_error;
use crate::writer::write_error::WriteError;
use cadmpeg_ir::codec::write::EncodeError;

#[test]
fn destination_io_error_keeps_its_kind() {
    let error: WriteError = std::io::Error::from(std::io::ErrorKind::BrokenPipe).into();
    assert!(
        matches!(content_error(&cadmpeg_ir::CadIr::empty(), error), Err(EncodeError::Io(error))
        if error.kind() == std::io::ErrorKind::BrokenPipe)
    );
}

#[test]
fn codec_io_error_keeps_its_kind() {
    let source = cadmpeg_core::CodecError::Io(std::io::Error::from(std::io::ErrorKind::BrokenPipe));
    assert!(
        matches!(content_error(&cadmpeg_ir::CadIr::empty(), WriteError::from(source)), Err(EncodeError::Io(error))
        if error.kind() == std::io::ErrorKind::BrokenPipe)
    );
}
