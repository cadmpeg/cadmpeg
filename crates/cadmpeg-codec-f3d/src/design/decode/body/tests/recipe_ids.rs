// SPDX-License-Identifier: Apache-2.0

use super::super::{ascii_id_at, recipe_design_id};

#[test]
fn recipe_design_ids_read_fixed_fields() {
    assert_eq!(recipe_design_id(b"123", 23, b""), Some(("123", 0)));
    assert_eq!(recipe_design_id(b"1a3", 23, b""), None);

    let mut recipe_id = Vec::new();
    recipe_id.extend_from_slice(&3u32.to_le_bytes());
    recipe_id.extend_from_slice(b"abc");
    assert_eq!(ascii_id_at(&recipe_id, 0), Some(("abc", 4)));
    recipe_id[5] = b'-';
    assert_eq!(ascii_id_at(&recipe_id, 0), None);

    let mut long_id = Vec::new();
    long_id.extend_from_slice(&9u32.to_le_bytes());
    long_id.extend_from_slice(b"123456789");
    assert_eq!(ascii_id_at(&long_id, 0), None);
}

#[test]
fn construction_recipe_counter_keys_borrow_short_design_ids() {
    for length in 1..=8 {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&u32::try_from(length).unwrap().to_le_bytes());
        bytes.extend_from_slice(&b"12345678"[..length]);
        let (id, offset) = ascii_id_at(&bytes, 0).unwrap();
        assert_eq!(offset, 4);
        assert_eq!(id.as_ptr(), bytes[4..].as_ptr());
        assert_eq!(id, std::str::from_utf8(&b"12345678"[..length]).unwrap());
    }
}
