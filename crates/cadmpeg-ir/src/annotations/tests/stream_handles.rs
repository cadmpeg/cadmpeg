// SPDX-License-Identifier: Apache-2.0

use super::StreamHandle;

#[test]
fn stream_handle_moves_an_owned_name() {
    let text = String::from("owned stream");
    let pointer = text.as_ptr();
    let name = crate::StreamName::try_from(text).unwrap();
    let handle = StreamHandle::new(
        &cadmpeg_test_support::service_decode_context(),
        name,
        "source handle",
    )
    .unwrap();
    assert_eq!(handle.0.as_str(), "owned stream");
    assert_eq!(handle.0.as_str().as_ptr(), pointer);
}
