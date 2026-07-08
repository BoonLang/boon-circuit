// Included by `../tests.rs`; kept in the parent test module for private renderer helper access.

#[test]
fn checkbox_circle_snaps_to_pixel_center_for_smoother_edges() {
    let rect = Rect {
        x: 194.0,
        y: 263.6,
        width: 40.0,
        height: 40.0,
    };

    assert_eq!(checkbox_circle_center(rect), (214.5, 283.5));
    assert!(circle_segments_for_radius(17.5) >= 192);
    assert!(circle_segments_for_radius(1.1) <= 24);
}


#[test]
fn checkbox_circle_coverage_feathers_edges() {
    let radius = 17.5;
    let aa = 1.25;

    assert_eq!(circle_coverage(radius, aa, radius - aa - 0.1), 1.0);
    assert_eq!(circle_coverage(radius, aa, radius + aa + 0.1), 0.0);
    let edge = circle_coverage(radius, aa, radius);
    assert!(
        edge > 0.45 && edge < 0.55,
        "edge coverage should be half-ish, got {edge}"
    );
}


#[test]
fn checkbox_check_points_are_centered_inside_circle() {
    let rect = Rect {
        x: 194.0,
        y: 263.6,
        width: 40.0,
        height: 40.0,
    };
    let (start, middle, end) = checkbox_check_points(rect);
    let min_x = start.0.min(middle.0).min(end.0);
    let max_x = start.0.max(middle.0).max(end.0);
    let min_y = start.1.min(middle.1).min(end.1);
    let max_y = start.1.max(middle.1).max(end.1);
    let check_center = ((min_x + max_x) * 0.5, (min_y + max_y) * 0.5);
    let circle_center = checkbox_circle_center(rect);

    assert!(
        (check_center.0 - circle_center.0).abs() <= 0.5,
        "check x center {:?} should stay near circle center {:?}",
        check_center,
        circle_center
    );
    assert!(
        (check_center.1 - circle_center.1).abs() <= 0.5,
        "check y center {:?} should stay near circle center {:?}",
        check_center,
        circle_center
    );
}
