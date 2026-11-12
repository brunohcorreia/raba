//! Placing elements on a canvas.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use raba_gen::layout::{Layout, Position};

#[test]
fn art_alone_is_the_whole_canvas() {
    let layout = Layout::of(10, 4);
    assert_eq!((layout.columns, layout.lines), (10, 4));
    assert_eq!(layout.placement(0), Some((0, 0)));
}

#[test]
fn below_adds_room_and_centres_horizontally() {
    let mut layout = Layout::of(10, 4);
    layout.attach(2, 1, Position::Below, 1);
    assert_eq!((layout.columns, layout.lines), (10, 6), "one gap line plus one spinner");
    assert_eq!(layout.placement(0), Some((0, 0)), "the art does not move");
    assert_eq!(layout.placement(1), Some((4, 5)));
}

#[test]
fn above_pushes_the_art_down() {
    // The part that is easy to get wrong: attaching above has to move everything
    // already placed, not just reserve space.
    let mut layout = Layout::of(10, 4);
    layout.attach(2, 1, Position::Above, 1);
    assert_eq!((layout.columns, layout.lines), (10, 6));
    assert_eq!(layout.placement(0), Some((0, 2)), "the art moved down");
    assert_eq!(layout.placement(1), Some((4, 0)));
}

#[test]
fn left_pushes_the_art_right_and_centres_vertically() {
    let mut layout = Layout::of(10, 5);
    layout.attach(3, 1, Position::Left, 2);
    assert_eq!((layout.columns, layout.lines), (15, 5));
    assert_eq!(layout.placement(0), Some((5, 0)));
    assert_eq!(layout.placement(1), Some((0, 2)));
}

#[test]
fn right_leaves_the_art_where_it_was() {
    let mut layout = Layout::of(10, 5);
    layout.attach(3, 1, Position::Right, 2);
    assert_eq!((layout.columns, layout.lines), (15, 5));
    assert_eq!(layout.placement(0), Some((0, 0)));
    assert_eq!(layout.placement(1), Some((12, 2)));
}

#[test]
fn overlay_takes_no_space() {
    let mut layout = Layout::of(10, 5);
    layout.attach(2, 1, Position::Overlay, 7);
    assert_eq!((layout.columns, layout.lines), (10, 5), "and ignores the gap");
    assert_eq!(layout.placement(1), Some((4, 2)));
}

#[test]
fn an_element_wider_than_the_art_widens_the_canvas() {
    let mut layout = Layout::of(4, 2);
    layout.attach(10, 1, Position::Below, 0);
    assert_eq!(layout.columns, 10);
    assert_eq!(layout.placement(1), Some((0, 2)));
}

#[test]
fn two_elements_stack_in_the_order_they_are_attached() {
    let mut layout = Layout::of(10, 4);
    layout.attach(2, 1, Position::Below, 1); // spinner
    layout.attach(6, 1, Position::Below, 0); // caption under it
    assert_eq!(layout.lines, 7);
    assert_eq!(layout.placement(1), Some((4, 5)), "spinner");
    assert_eq!(layout.placement(2), Some((2, 6)), "caption");
}

#[test]
fn padding_moves_everything_and_grows_both_ways() {
    let mut layout = Layout::of(10, 4);
    layout.attach(2, 1, Position::Below, 1);
    layout.pad(2);
    assert_eq!((layout.columns, layout.lines), (14, 10));
    assert_eq!(layout.placement(0), Some((2, 2)));
    assert_eq!(layout.placement(1), Some((6, 7)));
}

#[test]
fn every_documented_position_parses() {
    for name in Position::NAMES {
        let parsed = Position::parse(name).expect(name);
        assert_eq!(parsed.to_string(), *name, "parse and display must agree");
    }
    assert!(Position::parse("beneath").is_none());
}
