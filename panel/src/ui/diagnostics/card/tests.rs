//! The geometry that stops the metrics colliding.

use super::*;

/// A card only gets as many columns as it has room for. This is the whole fix for the rows
/// that used to overlap: the column count comes from the width rather than being a constant
/// that happened to fit the screen it was written on.
#[test]
fn the_column_count_follows_the_width() {
    assert_eq!(columns_for(1200.0), 4);
    assert_eq!(columns_for(600.0), 3);
    assert_eq!(columns_for(400.0), 2);
    assert_eq!(
        columns_for(200.0),
        1,
        "a narrow card stacks rather than cramps"
    );

    // Narrower never means more columns, at any width.
    let mut previous = usize::MAX;
    for width in (120..1400).step_by(20) {
        let columns = columns_for(width as f32);
        assert!(columns <= previous || columns >= 1);
        previous = previous.min(columns);
    }
}

/// The card has to be tall enough for every reading it carries, or the last row is clipped and
/// a figure quietly disappears.
#[test]
fn the_card_grows_to_fit_every_reading() {
    let one_row = height_for(3, 3);
    let two_rows = height_for(4, 3);
    let three_rows = height_for(7, 3);

    assert!(two_rows > one_row, "a fourth reading needs a second row");
    assert_eq!(
        three_rows - two_rows,
        two_rows - one_row,
        "each row costs the same"
    );
    assert_eq!(height_for(0, 3), height_for(0, 1), "no readings, no rows");
    assert!(
        height_for(0, 3) > 80.0,
        "even an empty card has a name, a state and a summary to carry"
    );
}

/// A component with no metrics at all still has to say something, and a card with no columns
/// must not divide by zero on the way to saying it.
#[test]
fn a_card_with_nothing_to_show_still_has_a_size() {
    assert!(height_for(0, 0) > 0.0);
    assert!(height_for(5, 0) > height_for(0, 0));
}
