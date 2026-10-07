// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

#[test]
fn measure_index_revisits_shorter_paths_and_stops_cycles() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM(#2,#3);#2=ITEM(#4);#3=ITEM(#5,#1);#4=ITEM(#5);#5=ITEM(#6);#6=MEASURE_WITH_UNIT(LENGTH_MEASURE(1.),#99);#99=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::test_support::with_service_context(source, crate::parse::parse_inner).expect("exchange");
    crate::test_support::with_service_context(source, |_, ctx| {
        let mut visited = BTreeMap::new();
        let mut measures = BTreeSet::new();
        super::super::collect_measure_ids(&crate::parse::Value::Reference(1), &exchange, &mut visited, 0, 4, &mut measures, ctx).expect("measure traversal");
        assert_eq!(measures, BTreeSet::from([6]));
        assert_eq!(visited.get(&5), Some(&2));
        assert_eq!(visited.get(&6), Some(&3));
    });
}
