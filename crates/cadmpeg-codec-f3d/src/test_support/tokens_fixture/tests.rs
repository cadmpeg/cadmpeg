use super::{push_u8_string, t_ident, t_subident};

#[test]
fn one_byte_token_builders_refuse_oversize_strings_before_writing() {
    for write in [push_u8_string, t_ident, t_subident] {
        let mut bytes = vec![0xaa];
        write(&mut bytes, &"a".repeat(255));
        assert_eq!(bytes[2], 255);

        let before = bytes.clone();
        let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            write(&mut bytes, &"a".repeat(256));
        }));
        assert!(refused.is_err());
        assert_eq!(bytes, before);
    }
}
