// SPDX-License-Identifier: Apache-2.0
//! I/O failures during synthesized F3D output.

use std::io::{self, Write};

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeError, EncodeInput, Encoder};

struct FailingWriter;

impl Write for FailingWriter {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("injected output failure"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn synthesized_archive_propagates_the_output_error() {
    let document = cadmpeg_ir::examples::unit_cube().expect("cube");
    let plan = crate::F3dCodec
        .plan(EncodeInput::new(&document, None), TargetRequest::Inherit)
        .expect("plan synthesized F3D");
    let error = plan.write_to(&mut FailingWriter).expect_err("write fails");
    let EncodeError::Io(error) = error else {
        panic!("expected an output I/O error");
    };
    assert_eq!(error.kind(), io::ErrorKind::Other);
    assert_eq!(error.to_string(), "injected output failure");
}
