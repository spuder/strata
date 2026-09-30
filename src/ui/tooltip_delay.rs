// SPDX-License-Identifier: MIT

//! GTK shows the first tooltip after a 500 ms rest, then enters a browse mode that pops
//! every later tooltip after only 60 ms, so sweeping across rows keeps covering them.
//! GTK 4 exposes no setting for this, so tooltips are gated on the pointer resting.

use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    time::{Duration, Instant},
};

use gtk::{
    glib::{self, translate::IntoGlib},
    prelude::*,
};

/// Below GTK's own hover delay so the first tooltip keeps its usual timing.
const REST: Duration = Duration::from_millis(400);

#[derive(Debug, PartialEq)]
enum Decision {
    Allow,
    WaitFor(Duration),
}

#[derive(Default)]
struct RestGate {
    point: Option<(usize, f64, f64)>,
    rested_since: Option<Instant>,
    allowed: Option<usize>,
}

impl RestGate {
    /// `surface` and `widget` are identities; `x`/`y` are native-relative so a query
    /// that climbs to a parent widget does not count as pointer motion.
    fn query(&mut self, surface: usize, widget: usize, x: f64, y: f64, now: Instant) -> Decision {
        if self.point != Some((surface, x, y)) {
            self.point = Some((surface, x, y));
            self.rested_since = Some(now);
        }
        if self.allowed == Some(widget) {
            return Decision::Allow;
        }
        self.allowed = None;
        let rested = now.saturating_duration_since(self.rested_since.unwrap_or(now));
        if rested >= REST {
            self.allowed = Some(widget);
            Decision::Allow
        } else {
            Decision::WaitFor(REST - rested)
        }
    }
}

thread_local! {
    static GATE: RefCell<RestGate> = RefCell::default();
    static RETRY: RefCell<Option<glib::SourceId>> = const { RefCell::new(None) };
    static GATED: RefCell<HashSet<usize>> = RefCell::default();
    static INSTALLED: Cell<bool> = const { Cell::new(false) };
}

pub(super) fn install() {
    if INSTALLED.replace(true) {
        return;
    }
    // Signals are registered by class_init, which may not have run before any widget exists.
    let _class = glib::Class::<gtk::Widget>::from_type(gtk::Widget::static_type());
    let Some(signal) =
        glib::subclass::signal::SignalId::lookup("query-tooltip", gtk::Widget::static_type())
    else {
        return;
    };
    add_query_tooltip_hook(signal.into_glib());
}

#[expect(
    unsafe_code,
    reason = "glib-rs does not bind g_signal_add_emission_hook, the only app-wide signal hook"
)]
fn add_query_tooltip_hook(signal: u32) {
    unsafe extern "C" fn hook(
        _hint: *mut glib::gobject_ffi::GSignalInvocationHint,
        n_params: u32,
        params: *const glib::gobject_ffi::GValue,
        _data: glib::ffi::gpointer,
    ) -> glib::ffi::gboolean {
        if n_params > 0 && !params.is_null() {
            // SAFETY: GLib passes `n_params` initialized values, the first being the
            // emitting instance, alive for this call. `glib::Value` is a transparent
            // wrapper around `GValue`, and it is only borrowed here.
            let instance = unsafe { &*params.cast::<glib::Value>() };
            if let Ok(widget) = instance.get::<gtk::Widget>() {
                gate(&widget);
            }
        }
        glib::ffi::GTRUE
    }

    // SAFETY: `signal` is the registered id of GtkWidget::query-tooltip, `hook` matches
    // GSignalEmissionHook, and it uses no user data, so none needs to be destroyed.
    unsafe {
        glib::gobject_ffi::g_signal_add_emission_hook(
            signal,
            0,
            Some(hook),
            std::ptr::null_mut(),
            None,
        );
    }
}

/// Handlers added from an emission hook run from the next emission on, which is
/// always a motion query that precedes GTK's show query.
fn gate(widget: &gtk::Widget) {
    let key = widget.as_ptr() as usize;
    if !GATED.with_borrow_mut(|gated| gated.insert(key)) {
        return;
    }
    widget.add_weak_ref_notify_local(move || {
        GATED.with_borrow_mut(|gated| gated.remove(&key));
    });
    widget.connect_query_tooltip(|widget, x, y, keyboard, _| {
        if keyboard {
            return false;
        }
        let Some(native) = widget.native() else {
            return false;
        };
        let point = widget
            .compute_point(&native, &gtk::graphene::Point::new(x as f32, y as f32))
            .unwrap_or_else(|| gtk::graphene::Point::new(x as f32, y as f32));
        let decision = GATE.with_borrow_mut(|gate| {
            gate.query(
                native.as_ptr() as usize,
                widget.as_ptr() as usize,
                f64::from(point.x()),
                f64::from(point.y()),
                Instant::now(),
            )
        });
        if let Decision::WaitFor(remaining) = decision {
            widget.stop_signal_emission_by_name("query-tooltip");
            retry_after(widget, remaining);
        }
        false
    });
}

/// GTK only re-queries on pointer events, so a resting pointer needs a synthetic one.
fn retry_after(widget: &gtk::Widget, remaining: Duration) {
    let widget = widget.downgrade();
    let source = glib::timeout_add_local_once(remaining, move || {
        RETRY.with_borrow_mut(|retry| retry.take());
        if let Some(widget) = widget.upgrade() {
            widget.trigger_tooltip_query();
        }
    });
    if let Some(previous) = RETRY.with_borrow_mut(|retry| retry.replace(source)) {
        previous.remove();
    }
}

#[cfg(test)]
mod tests;
