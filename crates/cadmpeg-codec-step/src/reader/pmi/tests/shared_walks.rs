// SPDX-License-Identifier: Apache-2.0
//! PMI measure walks visit each shared record once.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use cadmpeg_ir::pmi::{PmiQuantity, PmiValue};

use crate::parse::{Exchange, Value};

const HEADER: &str = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;";
const TAIL: &str = "ENDSEC;END-ISO-10303-21;";
const LEVELS: u64 = 40;
const TOP: u64 = LEVELS + 1;

/// `#1` is `bottom`; `#2` to `#{TOP}` each name the one before twice, so a
/// walk that revisits shared records takes `2^LEVELS` steps.
fn doubling_exchange(bottom: &str, root: &str) -> Exchange {
    let mut source = format!("{HEADER}#1={bottom};");
    for id in 2..=TOP {
        write!(source, "#{id}=ITEM(#{},#{});", id - 1, id - 1).expect("write record fixture");
    }
    write!(source, "#100={root};{TAIL}").expect("write root fixture");
    crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
        .expect("valid exchange")
        .0
}

#[test]
fn measure_id_walk_visits_shared_records_once() {
    let exchange = doubling_exchange(
        "MEASURE_REPRESENTATION_ITEM('d',LENGTH_MEASURE(2.),$)",
        &format!("ITEM(#{TOP})"),
    );
    crate::test_support::with_service_context(b"", |_, ctx| {
        let mut measure_ids = BTreeSet::new();
        super::super::collect_measure_ids(
            &Value::Reference(100),
            &exchange,
            &mut BTreeSet::new(),
            &mut measure_ids,
            ctx,
        )
        .expect("the walk fits");
        assert_eq!(measure_ids, BTreeSet::from([1]));
    });
}

#[test]
fn measure_walk_evaluates_shared_records_once() {
    let exchange = doubling_exchange("ITEM()", &format!("ITEM(#{TOP},7)"));
    crate::test_support::with_service_context(b"", |_, ctx| {
        let mut losses = Vec::new();
        let mut measurements = super::super::MeasureContext {
            length_scale: 1.0,
            angle_scale: 1.0,
            losses: &mut losses,
            measured: BTreeMap::new(),
        };
        assert_eq!(
            super::super::measure(&Value::Reference(100), &exchange, &mut measurements, ctx)
                .expect("the walk fits"),
            PmiValue::new(7.0, PmiQuantity::Ratio)
        );
        assert!(losses.is_empty());
    });
}
