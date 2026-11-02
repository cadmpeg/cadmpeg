// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;

use super::WholeInput;
use crate::decode::View;

#[test]
fn a_view_over_every_byte_is_whole() {
    let bytes = b"0123456789";
    let view = View::over_retained(bytes);
    let mut cursor = Cursor::new(bytes.as_slice());
    cursor.set_position(4);
    let whole = WholeInput::of_source(view, &mut cursor)
        .expect("the source length is readable")
        .expect("the view covers every byte");
    assert_eq!(whole.view().window(), bytes.as_slice());
    assert_eq!(cursor.position(), 4);
}

#[test]
fn a_shorter_view_is_not_whole() {
    let bytes = b"0123456789";
    let view = View::over_retained(&bytes[..5]);
    let mut cursor = Cursor::new(bytes.as_slice());
    cursor.set_position(4);
    let whole = WholeInput::of_source(view, &mut cursor).expect("the source length is readable");
    assert!(whole.is_none());
    assert_eq!(cursor.position(), 4);
}

#[test]
fn a_view_that_does_not_start_at_zero_is_not_whole() {
    let bytes = b"0123456789";
    let view = View::over_retained(bytes)
        .child(5, 10)
        .expect("the child range sits inside the parent");
    assert_ne!(view.start(), 0);
    assert_eq!(view.window().len(), 5);
    let mut cursor = Cursor::new(view.window());
    cursor.set_position(2);
    let whole = WholeInput::of_source(view, &mut cursor).expect("the source length is readable");
    assert!(whole.is_none());
    assert_eq!(cursor.position(), 2);
}
