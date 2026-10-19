// SPDX-License-Identifier: Apache-2.0

#[test]
fn block_construction_admits_fixture() {
    let references = (0..19_u32)
        .map(|ordinal| {
            crate::native::features::block_reference::FeatureBlockConstructionReference {
                id: format!("reference#{ordinal}"),
                operation_label: "operation".into(),
                control: 0x26,
                position: crate::native::features::block_reference::BlockReferencePosition::new(
                    ordinal,
                )
                .expect("block reference position"),
                token: crate::om::reference_index::PayloadIndexToken::from_wire(
                    ordinal + 100,
                    &[
                        0xf0,
                        u8::try_from(ordinal + 100).expect("small object index"),
                    ],
                )
                .expect("payload index token"),
                data_block: Some(format!("block#{ordinal}")),
                source_offset: u64::from(ordinal),
            }
        })
        .collect::<Vec<_>>();
    let decode =
        || crate::native::features::construction_records::feature_block_constructions(&references);
    assert_eq!(decode().len(), 1);
}

fn block_payload_input(
    first_block: &[u8],
) -> (
    crate::container::Container<'static>,
    crate::native::features::FeatureBlockConstruction,
) {
    let mut store = vec![b"A".as_slice(); 20];
    store[0] = first_block;
    let part = crate::test_support::test_om::composed_feature_history_payload(&[], &store);
    let file =
        crate::test_support::test_prt::prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", part)]);
    let container = crate::container::scan_bytes(file).expect("block payload container");
    let construction = crate::native::features::FeatureBlockConstruction {
        id: "block-construction#0".into(),
        operation_label: "operation".into(),
        control: 0x26,
        members: std::array::from_fn(|ordinal| {
            crate::native::features::FeatureConstructionMember {
                reference: format!("reference#{ordinal}"),
                data_block: format!("nx:om-data-blocks-0:block#{}", ordinal + 1),
            }
        }),
        terminal_reference: "reference#18".into(),
        terminal_data_block: "nx:om-data-blocks-0:block#19".into(),
    };
    (container, construction)
}

#[test]
fn block_payload_admits_fixture() {
    let (container, construction) = block_payload_input(b"A");
    let decode = |_: &cadmpeg_core::decode::DecodeContext<'_>| {
        crate::native::features::construction_records::feature_block_construction_payloads(
            &container,
            std::slice::from_ref(&construction),
        )
    };
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| decode(ctx))
            .expect("admitted block payload")
            .len(),
        1
    );
}

#[derive(Clone, Copy)]
enum BlockFieldRoute {
    Scalar,
    Name,
}

fn assert_block_field_admits(route: BlockFieldRoute) {
    let bytes = b"\x03\x08Point1\0\x50\x59\x66\x64\x00\x30\x43\x0c\xcc\xcc\xcc\xcd\x72";
    let (container, construction) = block_payload_input(bytes);
    let payloads =
        crate::native::features::construction_records::feature_block_construction_payloads(
            &container,
            std::slice::from_ref(&construction),
        )
        .expect("block construction payloads");
    let decode = || match route {
        BlockFieldRoute::Scalar => {
            crate::native::features::construction_records::feature_block_payload_scalars(
                &container, &payloads,
            )
            .map(|values| values.len())
        }
        BlockFieldRoute::Name => {
            crate::native::features::construction_records::feature_block_payload_names(
                &container, &payloads,
            )
            .map(|values| values.len())
        }
    };
    assert_eq!(decode().expect("admitted block field"), 1);
}

macro_rules! block_field_admit_test {
    ($name:ident, $route:expr) => {
        #[test]
        fn $name() {
            assert_block_field_admits($route);
        }
    };
}

block_field_admit_test!(block_scalar_admits_fixture, BlockFieldRoute::Scalar);
block_field_admit_test!(block_name_admits_fixture, BlockFieldRoute::Name);

#[test]
fn block_named_record_admits_fixture() {
    let bytes = b"\x03\x08Point1\0\x50\x59\x66\x64\x00\x30\x43\x0c\xcc\xcc\xcc\xcd\x72";
    let (container, construction) = block_payload_input(bytes);
    let (payloads, names, scalars) = crate::test_support::with_decode_context(|_| {
        let payloads =
            crate::native::features::construction_records::feature_block_construction_payloads(
                &container,
                std::slice::from_ref(&construction),
            )?;
        let names = crate::native::features::construction_records::feature_block_payload_names(
            &container, &payloads,
        )?;
        let scalars = crate::native::features::construction_records::feature_block_payload_scalars(
            &container, &payloads,
        )?;
        Ok::<_, cadmpeg_core::CodecError>((payloads, names, scalars))
    })
    .expect("block named record inputs");
    let decode = || {
        crate::native::features::construction_records::feature_block_payload_named_records(
            &payloads, &names, &scalars,
        )
    };
    assert_eq!(decode().len(), 1);
}

fn block_point_input() -> (
    Vec<crate::native::features::FeatureBlockPayloadNamedRecord>,
    Vec<crate::native::features::payload_name::FeaturePayloadName>,
    Vec<crate::native::features::FeaturePayloadScalar>,
) {
    let bytes = b"\x03\x08Point1\0\x50\x59\x66\x64\x00\x30\x43\x0c\xcc\xcc\xcc\xcd\x72";
    let (container, construction) = block_payload_input(bytes);
    crate::test_support::with_decode_context(|_| {
        let payloads =
            crate::native::features::construction_records::feature_block_construction_payloads(
                &container,
                std::slice::from_ref(&construction),
            )?;
        let names = crate::native::features::construction_records::feature_block_payload_names(
            &container, &payloads,
        )?;
        let mut scalars =
            crate::native::features::construction_records::feature_block_payload_scalars(
                &container, &payloads,
            )?;
        let mut records =
            crate::native::features::construction_records::feature_block_payload_named_records(
                &payloads, &names, &scalars,
            );
        assert_eq!((records.len(), scalars.len()), (1, 1));
        let mut second = scalars[0].clone();
        second.id = "second-scalar".into();
        records[0].scalar_fields.push(second.id.clone());
        scalars.push(second);
        Ok::<_, cadmpeg_core::CodecError>((records, names, scalars))
    })
    .expect("block point inputs")
}

#[derive(Clone, Copy)]
enum BlockPointRoute {
    Point,
    Group,
}

fn assert_block_point_admits(route: BlockPointRoute) {
    let (records, names, scalars) = block_point_input();
    let points = {
        crate::native::features::construction_records::feature_block_payload_points(
            &records, &names, &scalars,
        )
    };
    assert_eq!(points.len(), 1);
    let mut second = points[0].clone();
    second.id = "second-point".into();
    let group_points = [points[0].clone(), second];
    let decode = || match route {
        BlockPointRoute::Point => {
            crate::native::features::construction_records::feature_block_payload_points(
                &records, &names, &scalars,
            )
            .len()
        }
        BlockPointRoute::Group => {
            crate::native::features::construction_records::feature_block_payload_point_groups(
                &group_points,
            )
            .len()
        }
    };
    assert_eq!(decode(), 1);
}

macro_rules! block_point_admit_test {
    ($name:ident, $route:expr) => {
        #[test]
        fn $name() {
            assert_block_point_admits($route);
        }
    };
}

block_point_admit_test!(block_point_admits_fixture, BlockPointRoute::Point);
block_point_admit_test!(block_point_group_admits_fixture, BlockPointRoute::Group);
