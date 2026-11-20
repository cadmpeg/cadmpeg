// SPDX-License-Identifier: Apache-2.0

use super::super::*;
use crate::wire::bytes::AllocationReferenceEncoding;
use crate::wire::records::consolidated_records;

#[test]
fn wide_roster_does_not_override_a_companion_station_or_expand_the_trim() {
    let original =
        include_bytes!("../../../../../../tests/fixtures/endpoint_stations/mapped-trim.bin");
    let mut bytes = original.to_vec();
    assert_eq!(&bytes[..6], &[0xb2, 3, 0x5d, 1, 5, 1]);
    bytes.splice(5..6, [0x0e, 0x56, 0x34, 0x12, 0]);
    bytes[3] = 5;
    let roster = consolidated_records(&bytes).len();
    bytes.extend([0xb2, 3, 0x05, 2, 5, 0x81, 3]);
    let mut incidence = vec![
        0x82,
        3,
        u8::try_from(4 * (roster + 1 - 3) + 1).expect("compact fixture reference"),
        0x80,
    ];
    incidence.extend(0.5_f64.to_le_bytes());
    bytes.extend([
        0xb2,
        3,
        0x06,
        u8::try_from(incidence.len()).expect("short incidence frame"),
        5,
    ]);
    bytes.extend(incidence);
    let records = consolidated_records(&bytes);
    let resolve = |index: usize, reference: AllocationReference| {
        let value = usize::try_from(reference.value).ok()?;
        match reference.encoding {
            AllocationReferenceEncoding::TaggedU24 if value == 0x0012_3456 => Some(roster),
            AllocationReferenceEncoding::OwnedChild => index.checked_add(1)?.checked_add(value),
            AllocationReferenceEncoding::BackwardDistance => index.checked_sub(value),
            _ => None,
        }
    };
    assert_eq!(
        stations::wide_parameter(
            &bytes,
            &records,
            0,
            3,
            stations::Domain {
                range: [0.0, 8.0],
                knots: None,
                line: false
            },
            &resolve,
            &mut LaneRefusals::new(),
        ),
        Some(0.5),
    );
    let source = Source {
        bytes: &bytes,
        records: &records,
        resolve,
        physical: Some,
        tensors: &BTreeMap::new(),
    };
    let ir = CadIr::empty();
    let carriers = BTreeMap::new();
    let mut refusal = LaneRefusals::new();
    let carrier = source
        .carrier(&ir, 10, &carriers, &mut refusal)
        .expect("fixture plane");
    let trim = source
        .trim(0, &[0x0b, 0x17], &mut 0, &carrier, &mut refusal)
        .expect("mapped trim");
    assert_eq!(trim.stations, [Some(5.0), Some(6.0)]);
    assert_eq!(trim.primary_stations, [None, None]);
    assert_eq!(trim.range, [0.0, 8.0]);
    assert_eq!(trim.mapped_range, Some([0.0, 8.0]));
}
