// SPDX-License-Identifier: Apache-2.0

use super::*;

fn path(radius: f64, pitch: f64, hand: Hand) -> HelicalPath {
    HelicalPath {
        radius,
        pitch,
        hand,
    }
}

fn modeled_internal() -> DesignThreadConstruction {
    let mut construction = super::super::tests::construction();
    construction.extended = None;
    construction
}

#[test]
fn matching_helices_that_agree_state_the_hand() {
    let construction = modeled_internal();
    let paths = [
        path(4.2, 1.5, Hand::Left),
        path(5.4, 1.5, Hand::Left),
        path(4.2, 2.0, Hand::Right),
        path(9.0, 1.5, Hand::Right),
    ];
    assert_eq!(hand(&paths, &construction), Some(Hand::Left));
}

#[test]
fn matching_helices_that_disagree_state_no_hand() {
    let construction = modeled_internal();
    let paths = [path(4.2, 1.5, Hand::Left), path(5.4, 1.5, Hand::Right)];
    assert_eq!(hand(&paths, &construction), None);
}

#[test]
fn no_matching_helix_states_no_hand() {
    let construction = modeled_internal();
    assert_eq!(hand(&[path(4.2, 2.0, Hand::Right)], &construction), None);
    assert_eq!(hand(&[], &construction), None);
}

#[test]
fn a_cosmetic_thread_states_no_hand() {
    let mut construction = modeled_internal();
    construction.modeled = false;
    assert_eq!(hand(&[path(4.2, 1.5, Hand::Right)], &construction), None);
}
