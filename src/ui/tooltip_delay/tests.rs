// SPDX-License-Identifier: MIT

use super::*;
use crate::test_support::gtk_test;

const MS: Duration = Duration::from_millis(1);

#[test]
fn tooltip_waits_until_the_pointer_has_rested() {
    let start = Instant::now();
    let mut gate = RestGate::default();
    assert_eq!(gate.query(1, 10, 5.0, 5.0, start), Decision::WaitFor(REST));
    assert_eq!(
        gate.query(1, 10, 5.0, 5.0, start + 60 * MS),
        Decision::WaitFor(REST - 60 * MS)
    );
    assert_eq!(gate.query(1, 10, 5.0, 5.0, start + REST), Decision::Allow);
}

#[test]
fn moving_restarts_the_rest_on_another_widget() {
    let start = Instant::now();
    let mut gate = RestGate::default();
    gate.query(1, 10, 5.0, 5.0, start);
    gate.query(1, 10, 5.0, 5.0, start + REST);
    let later = start + REST + 100 * MS;
    assert_eq!(gate.query(1, 20, 5.0, 30.0, later), Decision::WaitFor(REST));
    assert_eq!(gate.query(1, 20, 5.0, 30.0, later + REST), Decision::Allow);
}

#[test]
fn shown_tooltip_follows_the_pointer_within_its_widget() {
    let start = Instant::now();
    let mut gate = RestGate::default();
    gate.query(1, 10, 5.0, 5.0, start);
    gate.query(1, 10, 5.0, 5.0, start + REST);
    assert_eq!(
        gate.query(1, 10, 9.0, 6.0, start + REST + MS),
        Decision::Allow
    );
}

#[test]
fn returning_to_a_previous_widget_needs_a_new_rest() {
    let start = Instant::now();
    let mut gate = RestGate::default();
    gate.query(1, 10, 5.0, 5.0, start);
    gate.query(1, 10, 5.0, 5.0, start + REST);
    gate.query(1, 20, 5.0, 30.0, start + REST + MS);
    assert_eq!(
        gate.query(1, 10, 5.0, 5.0, start + REST + 2 * MS),
        Decision::WaitFor(REST)
    );
}

#[test]
fn climbing_to_a_parent_is_not_pointer_motion() {
    let start = Instant::now();
    let mut gate = RestGate::default();
    gate.query(1, 10, 5.0, 5.0, start);
    assert_eq!(
        gate.query(1, 11, 5.0, 5.0, start + 100 * MS),
        Decision::WaitFor(REST - 100 * MS)
    );
    assert_eq!(
        gate.query(2, 11, 5.0, 5.0, start + REST),
        Decision::WaitFor(REST)
    );
}

#[test]
fn query_tooltip_is_suppressed_until_the_pointer_rests() {
    gtk_test(
        "ui::tooltip_delay::tests::query_tooltip_is_suppressed_until_the_pointer_rests",
        || {
            install();
            let label = gtk::Label::new(Some("folder"));
            label.set_tooltip_text(Some("/home/folder"));
            let window = gtk::Window::builder().child(&label).build();
            window.present();
            let tooltip = glib::Object::new::<gtk::Tooltip>();
            let query = |x: i32| {
                label.emit_by_name::<bool>("query-tooltip", &[&x, &1i32, &false, &tooltip])
            };

            assert!(query(1), "the hook only gates from the next emission");
            assert!(!query(2), "a moving pointer gets no tooltip");
            std::thread::sleep(REST);
            assert!(query(2), "a rested pointer gets its tooltip");
            assert!(
                query(3),
                "the shown tooltip survives motion within its widget"
            );
            window.destroy();
        },
    );
}
