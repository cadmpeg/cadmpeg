// SPDX-License-Identifier: Apache-2.0
use super::*;
use cadmpeg_core::scan::{ScanClock, ScanLimits};
use std::cell::Cell;
use std::time::Duration;

struct Clock(Cell<u32>);
impl ScanClock for Clock {
    fn elapsed(&self) -> Duration {
        Duration::ZERO
    }
    fn is_cancelled(&self) -> bool {
        let steps = self.0.get();
        self.0.set(steps + 1);
        steps >= 8
    }
}

#[test]
fn scan_utf16_admits_units_and_worst_case_utf8_before_conversion() {
    let mut source = vec![0xff, 0xfe];
    for unit in "<Keywords><Feature Type=\""
        .encode_utf16()
        .chain(std::iter::repeat_n(0x0800, 1024 * 1024))
        .chain("\"/></Keywords>".encode_utf16())
    {
        source.extend_from_slice(&unit.to_le_bytes());
    }
    let clock = Clock(Cell::new(0));
    let budget = ScanBudget::new(
        ScanLimits {
            retained_bytes: 7 * 1024 * 1024,
            ..ScanLimits::default()
        },
        &clock,
    );
    budget
        .retain(u64_from_index(source.len()))
        .expect("acquired stream");
    let mut scan = DesignScan::new(Some("sldprt"));
    assert_eq!(
        census(View::over_retained(&source), &budget, &mut scan),
        Err(StopReason::RetainedBytes)
    );
    assert!(scan.census.is_none());
}

struct Idle;
impl ScanClock for Idle {
    fn elapsed(&self) -> Duration {
        Duration::ZERO
    }
    fn is_cancelled(&self) -> bool {
        false
    }
}

#[test]
fn scan_excludes_application_elements_and_descendants() {
    let text = "<Keywords Name=\"Part\"><Application Name=\"Simulation\">\
<Keyword Name=\"Analysis type\">Static</Keyword>\
<Keyword Name=\"Mesh type\">Solid Mesh</Keyword></Application>\
<Feature id=\"1\" Name=\"Sketch1\" Type=\"ProfileFeature\"/>\
<Feature id=\"2\" Name=\"Boss\" Type=\"Extrusion\"></Feature></Keywords>";
    let budget = ScanBudget::new(ScanLimits::default(), &Idle);
    let mut scan = DesignScan::new(Some("sldprt"));
    assert_eq!(
        census(View::over_retained(text.as_bytes()), &budget, &mut scan),
        Ok(true)
    );
    scan.complete();
    let census = scan.census.expect("census");
    assert_eq!(census.total, Some(2));
    let kinds = census
        .counts_by_kind
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    assert_eq!(kinds, ["Extrusion", "ProfileFeature"]);
}
