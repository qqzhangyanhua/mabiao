use crate::tray_popup::{place_popup, popup_logical_size, RectF};

#[test]
fn popup_sits_below_a_top_tray_icon_and_stays_on_screen() {
    let tray = RectF {
        x: 1400.0,
        y: 0.0,
        w: 28.0,
        h: 22.0,
    };
    let work = RectF {
        x: 0.0,
        y: 0.0,
        w: 1512.0,
        h: 982.0,
    };
    let (x, y) = place_popup(tray, 372.0, 300.0, work, 8.0);
    assert_eq!(y, 30.0);
    assert!(x >= 8.0);
    assert!(x + 372.0 <= 1512.0 - 8.0);
}

#[test]
fn popup_flips_above_when_the_tray_is_at_the_bottom() {
    let tray = RectF {
        x: 1400.0,
        y: 960.0,
        w: 28.0,
        h: 22.0,
    };
    let work = RectF {
        x: 0.0,
        y: 0.0,
        w: 1512.0,
        h: 982.0,
    };
    let (x, y) = place_popup(tray, 372.0, 300.0, work, 8.0);
    assert_eq!(y, 652.0);
    assert!(x + 372.0 <= work.w - 8.0);
}

#[test]
fn popup_height_clamps_and_empty_state_still_has_room() {
    let empty = popup_logical_size(0, 0);
    assert_eq!(empty.0, 372.0);
    assert!(empty.1 >= 120.0);
    assert!(empty.1 < 200.0);

    let crowded = popup_logical_size(12, 40);
    assert_eq!(crowded.1, 640.0);
}
