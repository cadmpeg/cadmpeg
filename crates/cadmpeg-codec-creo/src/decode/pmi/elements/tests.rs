// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::View;

use super::elements;

fn fixture() -> Vec<u8> {
    let prototype = b"\xe3\xe0\x00AE_Header\0\xe2\xe0\x00AE_Item\0\xe2\xe0\x01AE_Id\0\x01\xe0\x01AE_Type\0\x02\xe0\x01AE_Attr\0\xf1\xf8\x02\x00\xe4\xe0\x00AE_Annot\0\xe2\xe0\x01Annot_Id\0\xf6\xe0\x01Annot_Type\0\xf6\xe0\x00AE_Appl\0\xe2\xe0\x01Appl_Id\0\xf6\xe0\x01Appl_Type\0\xf6\xe0\x00AE_Owner\0\xe2\xe0\x01Owner_Type\0\x03\xe0\x01Owner_Id\0\x01\xe0\x01Owner_Sub_Id\0\x01\xe0\x00AE_GenInfo\0\xe2\xe0\x00AE_trf\0\xe1\xf2\xf7\x05\xe3";
    let row = b"\xe2\xe2\x2a\x03\x00\xe4\xe2\x11\x20\xe2\xf6\xf6\xe2\x03\x01\x01\xe2\xe1";
    let mut data = b"\xe0\x00annot_elems\0\xf2\xf8\x02\xf7\x05\xfb".to_vec();
    data.extend_from_slice(prototype);
    data.extend_from_slice(row);
    data
}

#[test]
fn resolves_annotation_identity_after_a_null_prototype() {
    let data = fixture();
    crate::decode::with_service_context(|ctx| {
        let rows = elements(ctx, View::over_retained(&data))
            .expect("allocation")
            .expect("complete table");
        let first = rows[0].expect("prototype");
        assert_eq!(first.annotation, None);
        let second = rows[1].expect("second row");
        assert_eq!((second.owner, second.owner_type), (42, 3));
        assert_eq!(
            (second.annotation, second.annotation_type),
            (Some(17), Some(32))
        );
    });
}

#[test]
fn truncated_annotation_rows_do_not_form_a_complete_table() {
    let mut data = fixture();
    data.pop();
    crate::decode::with_service_context(|ctx| {
        assert!(elements(ctx, View::over_retained(&data))
            .expect("allocation")
            .is_none());
    });
}
