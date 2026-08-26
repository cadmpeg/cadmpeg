use super::*;

pub(crate) fn asymmetric_parameter_domain_surface_file() -> Vec<u8> {
    owned_test_file(&[OwnedTestEntity {
        entity_type: 128,
        form: 0,
        label: "SURFACE".into(),
        status: "00010000",
        parameters:
            "128,1,1,1,1,0,0,1,0,0,0,0,1,1,-2,-2,2,2,1,1,1,1,0,0,0,0,1,0,0,1,0,1,0,1,0,1,-2,2;"
                .into(),
    }])
}

pub(crate) fn alternate_asymmetric_parameter_domain_surface_file() -> Vec<u8> {
    owned_test_file(&[OwnedTestEntity {
        entity_type: 128,
        form: 0,
        label: "SURFACE".into(),
        status: "00010000",
        parameters:
            "128,1,1,1,1,0,0,1,0,0,0,0,1,1,-2,-2,2,2,1,1,1,1,0,0,0,0,1,0,0,1,0,1,0,1,0,-2,1,2;"
                .into(),
    }])
}

pub(crate) fn subrange_nurbs_surface_file() -> Vec<u8> {
    owned_test_file(&[OwnedTestEntity {
        entity_type: 128,
        form: 0,
        label: "SURFACE".into(),
        status: "00010000",
        parameters:
            "128,1,1,1,1,0,0,1,0,0,0,0,1,1,-2,-2,2,2,1,1,1,1,0,0,0,0,1,0,0,1,0,1,0,1,0.2,0.8,-1,1;"
                .into(),
    }])
}

pub(crate) fn nested_transformed_point_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,0.5,10,2HCM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    for (sequence, parameter_start, transform, entity_type, form, label) in [
        (1, 1, 0, "124", 0, "PARENT"),
        (3, 2, 1, "124", 1, "LOCAL"),
        (5, 3, 3, "116", 0, "POINT"),
    ] {
        bytes.extend(directory_card(
            [
                entity_type,
                &parameter_start.to_string(),
                "0",
                "0",
                "0",
                "0",
                &transform.to_string(),
                "0",
                "00000000",
            ],
            sequence,
        ));
        bytes.extend(directory_card(
            [
                entity_type,
                "0",
                "0",
                "1",
                &form.to_string(),
                "",
                "",
                label,
                "0",
            ],
            sequence + 1,
        ));
    }
    bytes.extend(parameter_card(b"124,1,0,0,0,0,1,0,2,0,0,1,0;", 1, 1));
    bytes.extend(parameter_card(b"124,-1,0,0,1,0,1,0,0,0,0,1,0;", 3, 2));
    bytes.extend(parameter_card(b"116,1,2,3;", 5, 3));
    let global_cards = global_card_count(global);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000006P0000003").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

pub(crate) fn transform_chain_overflow_file(transform_count: u32) -> Vec<u8> {
    assert!(transform_count > 0);
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    let identity = b"124,1,0,0,0,0,1,0,0,0,0,1,0;";
    for index in 0..transform_count {
        let sequence = 1 + index * 2;
        let transform = if index + 1 < transform_count {
            sequence + 2
        } else {
            0
        };
        bytes.extend(directory_card(
            [
                "124",
                &(index + 1).to_string(),
                "0",
                "0",
                "0",
                "0",
                &transform.to_string(),
                "0",
                "00000000",
            ],
            sequence,
        ));
        bytes.extend(directory_card(
            ["124", "0", "0", "1", "0", "", "", "TRANS", "0"],
            sequence + 1,
        ));
    }
    let point_sequence = 1 + transform_count * 2;
    bytes.extend(directory_card(
        [
            "116",
            &(transform_count + 1).to_string(),
            "0",
            "0",
            "0",
            "0",
            "1",
            "0",
            "00000000",
        ],
        point_sequence,
    ));
    bytes.extend(directory_card(
        ["116", "0", "0", "1", "0", "", "", "POINT", "0"],
        point_sequence + 1,
    ));
    for index in 0..transform_count {
        let sequence = 1 + index * 2;
        bytes.extend(parameter_card(identity, sequence, index + 1));
    }
    bytes.extend(parameter_card(
        b"116,1,2,3;",
        point_sequence,
        transform_count + 1,
    ));
    let global_cards = global_card_count(global);
    bytes.extend(card(
        format!(
            "S0000001G{global_cards:07}D{:07}P{:07}",
            (transform_count + 1) * 2,
            transform_count + 1
        )
        .as_bytes(),
        b'T',
        1,
    ));
    bytes
}
