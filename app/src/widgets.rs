//! Small widget helpers shared by the panes.
//!
//! egui's sliders and combo boxes ignore the mouse wheel, so these helpers add
//! wheel support on top: hover a control and scroll to nudge its value or cycle
//! its selection.

use eframe::egui::{Event, MouseWheelUnit, Response, Ui};

/// Trackpad travel (in points) that counts as one step for a dropdown.
const COMBO_POINTS_PER_STEP: f32 = 15.0;
/// Trackpad travel (in points) that counts as one step for a slider.
const SLIDER_POINTS_PER_STEP: f32 = 6.0;

/// A numeric value that can be nudged by the mouse wheel.
pub trait ScrollValue: Copy + PartialOrd {
    /// Move `steps` wheel notches (positive scrolls up) of size `step`.
    fn nudge(self, steps: i32, step: Self) -> Self;
    /// Clamp into `[min, max]`.
    fn clamped(self, min: Self, max: Self) -> Self;
}

impl ScrollValue for i32 {
    fn nudge(self, steps: i32, step: i32) -> i32 {
        self + steps * step
    }

    fn clamped(self, min: i32, max: i32) -> i32 {
        self.clamp(min, max)
    }
}

impl ScrollValue for f32 {
    fn nudge(self, steps: i32, step: f32) -> f32 {
        self + steps as f32 * step
    }

    fn clamped(self, min: f32, max: f32) -> f32 {
        self.clamp(min, max)
    }
}

/// Consume this frame's vertical wheel movement while the pointer is over
/// `response`, returning how many discrete steps that was.
///
/// Raw wheel events are used rather than egui's smoothed delta: a mouse wheel
/// delivers one `Line` event per notch, which maps to exactly one step, so the
/// selection stays predictable instead of only moving on a scroll burst.
/// The movement is consumed so an enclosing scroll area does not also act on it.
fn wheel_steps(ui: &Ui, response: &Response, points_per_step: f32) -> i32 {
    if !response.hovered() {
        return 0;
    }
    // Ctrl/Cmd + wheel is zoom; leave that to egui.
    if ui.input(|i| i.modifiers.command || i.modifiers.ctrl) {
        return 0;
    }
    let mut notches = 0.0_f32;
    let mut wheeled = false;
    ui.input(|i| {
        for event in &i.raw.events {
            let Event::MouseWheel { unit, delta, .. } = event else {
                continue;
            };
            wheeled = true;
            let amount = match unit {
                // A mouse wheel delivers one discrete event per notch, but the
                // magnitude varies wildly between devices (some report 0.1,
                // others 1.0), so count the event rather than summing it.
                MouseWheelUnit::Line | MouseWheelUnit::Page => delta.y.signum(),
                // Trackpads report smooth point deltas.
                MouseWheelUnit::Point => delta.y / points_per_step,
            };
            notches += amount;
            #[cfg(not(target_arch = "wasm32"))]
            tracing::debug!(unit = ?unit, delta_y = delta.y, amount, "mouse wheel over widget");
        }
    });
    // Consume the smoothed scroll every frame while hovered. It is emitted for
    // several frames after a wheel event (egui smooths it), so only clearing it
    // on the event frame would let the decay leak into an enclosing scroll area.
    ui.input_mut(|i| i.smooth_scroll_delta.y = 0.0);
    if !wheeled {
        return 0;
    }
    let id = response.id.with("wheel-accum");
    let accumulated = ui.data(|d| d.get_temp::<f32>(id)).unwrap_or(0.0) + notches;
    let steps = accumulated.trunc();
    ui.data_mut(|d| d.insert_temp(id, accumulated - steps));
    steps as i32
}

/// Nudge `value` with the wheel while the pointer is over `response`. Positive
/// scroll (up) increases the value. Returns `true` when it changed.
pub fn scroll_value<T: ScrollValue>(
    ui: &Ui,
    response: &Response,
    value: &mut T,
    step: T,
    min: T,
    max: T,
) -> bool {
    let steps = wheel_steps(ui, response, SLIDER_POINTS_PER_STEP);
    if steps == 0 {
        return false;
    }
    let next = value.nudge(steps, step).clamped(min, max);
    let changed = next != *value;
    *value = next;
    if changed {
        ui.ctx().request_repaint();
    }
    changed
}

/// Step an index into `len` items with the wheel while the pointer is over
/// `response`. Wraps around. Returns the new index when it changed.
pub fn scroll_index(ui: &Ui, response: &Response, current: usize, len: usize) -> Option<usize> {
    let steps = wheel_steps(ui, response, COMBO_POINTS_PER_STEP);
    if steps == 0 || len == 0 {
        return None;
    }
    let next = (current as i64 - steps as i64).rem_euclid(len as i64) as usize;
    ui.ctx().request_repaint();
    (next != current).then_some(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{self, Event, Modifiers, MouseWheelUnit, Pos2, RawInput, Rect, TouchPhase};

    fn run<F, R>(ctx: &egui::Context, pos: Pos2, events: Vec<Event>, run_ui: F) -> R
    where
        F: FnOnce(&mut Ui) -> R,
    {
        let mut events = events;
        events.insert(0, Event::PointerMoved(pos));
        let input = RawInput {
            events,
            ..Default::default()
        };
        let mut out = None;
        let mut run_ui = Some(run_ui);
        let mut output = ctx.run_ui(input, |ui| {
            out = Some(run_ui.take().expect("run_ui called once")(ui));
        });
        output.textures_delta.clear();
        out.unwrap()
    }

    #[test]
    fn wheel_over_widget_returns_index() {
        let ctx = egui::Context::default();
        let mut rect = Rect::NOTHING;
        run(&ctx, Pos2::new(10.0, 10.0), vec![], |ui| {
            rect = ui
                .allocate_response(egui::vec2(100.0, 20.0), egui::Sense::hover())
                .rect;
        });

        let center = rect.center();
        let wheel_up = Event::MouseWheel {
            unit: MouseWheelUnit::Line,
            delta: egui::vec2(0.0, 1.0),
            phase: TouchPhase::Move,
            modifiers: Modifiers::NONE,
        };
        let result = run(&ctx, center, vec![wheel_up], |ui| {
            let response = ui.allocate_response(egui::vec2(100.0, 20.0), egui::Sense::hover());
            super::scroll_index(ui, &response, 5, 10)
        });
        assert_eq!(result, Some(4), "wheel up should select the previous item");
    }

    fn combo_ui(ui: &mut Ui) -> (Rect, Option<usize>) {
        let response = egui::ComboBox::from_id_salt("test-combo")
            .selected_text("five")
            .show_ui(ui, |ui| {
                for n in 0..10 {
                    let _ = ui.selectable_label(false, n.to_string());
                }
            })
            .response;
        let rect = response.rect;
        (rect, super::scroll_index(ui, &response, 5, 10))
    }

    #[test]
    fn wheel_over_combobox_returns_index() {
        let ctx = egui::Context::default();
        let rect = run(&ctx, Pos2::new(10.0, 10.0), vec![], |ui| combo_ui(ui).0);

        let wheel_up = Event::MouseWheel {
            unit: MouseWheelUnit::Line,
            delta: egui::vec2(0.0, 1.0),
            phase: TouchPhase::Move,
            modifiers: Modifiers::NONE,
        };
        let result = run(&ctx, rect.center(), vec![wheel_up], |ui| combo_ui(ui).1);
        assert_eq!(result, Some(4), "wheel up over a combo should step back");
    }

    #[test]
    fn trackpad_delta_steps_dropdown() {
        let ctx = egui::Context::default();
        let mut rect = Rect::NOTHING;
        run(&ctx, Pos2::new(10.0, 10.0), vec![], |ui| {
            rect = ui
                .allocate_response(egui::vec2(100.0, 20.0), egui::Sense::hover())
                .rect;
        });

        let wheel_down = Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -COMBO_POINTS_PER_STEP),
            phase: TouchPhase::Move,
            modifiers: Modifiers::NONE,
        };
        let result = run(&ctx, rect.center(), vec![wheel_down], |ui| {
            let response = ui.allocate_response(egui::vec2(100.0, 20.0), egui::Sense::hover());
            super::scroll_index(ui, &response, 5, 10)
        });
        assert_eq!(result, Some(6), "wheel down should select the next item");
    }

    #[test]
    fn trackpad_delta_accumulates_across_frames() {
        let ctx = egui::Context::default();
        let mut rect = Rect::NOTHING;
        run(&ctx, Pos2::new(10.0, 10.0), vec![], |ui| {
            rect = ui
                .allocate_response(egui::vec2(100.0, 20.0), egui::Sense::hover())
                .rect;
        });

        let half = Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -COMBO_POINTS_PER_STEP / 2.0),
            phase: TouchPhase::Move,
            modifiers: Modifiers::NONE,
        };
        let first = run(&ctx, rect.center(), vec![half.clone()], |ui| {
            let response = ui.allocate_response(egui::vec2(100.0, 20.0), egui::Sense::hover());
            super::scroll_index(ui, &response, 5, 10)
        });
        assert_eq!(first, None, "half a step should not move yet");

        let second = run(&ctx, rect.center(), vec![half], |ui| {
            let response = ui.allocate_response(egui::vec2(100.0, 20.0), egui::Sense::hover());
            super::scroll_index(ui, &response, 5, 10)
        });
        assert_eq!(second, Some(6), "two halves should accumulate to one step");
    }

    fn panel_ui(ui: &mut Ui) -> (Rect, Option<usize>, f32) {
        let out = egui::ScrollArea::vertical()
            .id_salt("panel")
            .max_height(80.0)
            .show(ui, |ui| {
                let response = ui.allocate_response(egui::vec2(200.0, 20.0), egui::Sense::hover());
                let rect = response.rect;
                let stepped = super::scroll_index(ui, &response, 5, 10);
                ui.add_space(1000.0);
                (rect, stepped)
            });
        let (rect, stepped) = out.inner;
        (rect, stepped, out.state.offset.y)
    }

    #[test]
    fn wheel_over_widget_does_not_scroll_panel() {
        let ctx = egui::Context::default();
        let rect = run(&ctx, Pos2::new(10.0, 10.0), vec![], |ui| panel_ui(ui).0);

        let wheel_down = Event::MouseWheel {
            unit: MouseWheelUnit::Line,
            delta: egui::vec2(0.0, -1.0),
            phase: TouchPhase::Move,
            modifiers: Modifiers::NONE,
        };
        let (_, stepped, offset) = run(&ctx, rect.center(), vec![wheel_down], panel_ui);
        assert!(stepped.is_some(), "the widget should still get the step");
        assert_eq!(offset, 0.0, "the enclosing panel must not scroll");

        // egui keeps emitting smoothed scroll for a few frames afterwards; the
        // panel must stay put while the pointer is still over the widget.
        let (_, _, offset) = run(&ctx, rect.center(), vec![], panel_ui);
        assert_eq!(offset, 0.0, "smoothing must not leak into the panel");
    }

    #[test]
    fn wheel_ignored_when_not_hovered() {
        let ctx = egui::Context::default();
        let mut rect = Rect::NOTHING;
        run(&ctx, Pos2::new(10.0, 10.0), vec![], |ui| {
            rect = ui
                .allocate_response(egui::vec2(100.0, 20.0), egui::Sense::hover())
                .rect;
        });

        let outside = rect.max + egui::vec2(25.0, 25.0);
        let wheel_up = Event::MouseWheel {
            unit: MouseWheelUnit::Line,
            delta: egui::vec2(0.0, 1.0),
            phase: TouchPhase::Move,
            modifiers: Modifiers::NONE,
        };
        let result = run(&ctx, outside, vec![wheel_up], |ui| {
            let response = ui.allocate_response(egui::vec2(100.0, 20.0), egui::Sense::hover());
            super::scroll_index(ui, &response, 5, 10)
        });
        assert_eq!(result, None);
    }
}
