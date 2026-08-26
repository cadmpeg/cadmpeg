use super::*;

pub(crate) fn out_of_table_annotation_font_values_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 212,
            form: 0,
            label: "BAD212".into(),
            status: "00000100",
            parameters: "212,1,1,1,4,1.5707963267948966,0,0,0,0,0,0,1HA;".into(),
        },
        OwnedTestEntity {
            entity_type: 213,
            form: 0,
            label: "BAD213".into(),
            status: "00000100",
            parameters: "213,40,20,2,0,20,0,0,0,18,0,-5,1,0,2,3,-0.5,0,4,0,4HTUNL,4,12,3,1,1.5707963267948966,0,0,0,2,18,0,4HTOL!;".into(),
        },
    ])
}

pub(crate) fn leader_forms_file() -> Vec<u8> {
    let entities = (1..=12)
        .map(|form| {
            let (height, width) = match form {
                4 => (0, 0),
                5 | 6 | 12 => (2, 2),
                _ => (2, 1),
            };
            OwnedTestEntity {
                entity_type: 214,
                form,
                label: format!("LEAD{form}"),
                status: "00000100",
                parameters: format!("214,2,{height},{width},3,0,0,5,0,5,4;"),
            }
        })
        .collect::<Vec<_>>();
    owned_test_file(&entities)
}
