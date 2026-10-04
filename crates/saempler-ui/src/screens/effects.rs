use nih_plug_egui::egui::{self, pos2, vec2, Align2, FontId, Id, Rect, Sense, Ui};
use saempler_model::{
    CellEffects, Division, DriveShape, FilterShape, PerformanceCell, MAX_CUTOFF_HZ, MAX_DRIVE,
    MAX_RESONANCE, MIN_CUTOFF_HZ, MIN_RESONANCE,
};

use crate::screens::main::{hint_light, section_with, ViewState, THEME};
use crate::widgets::{
    dropdown, icon_button, inset, lamp, toggle, value_knob, value_slider, Icon, KnobSpec,
    SliderSpec, Taper, Unit,
};

/// Diameter of a knob on an effect card.
const KNOB: f32 = 34.0;
/// Width of a send amount bar.
const SEND_WIDTH: f32 = 84.0;
/// Width of a selector on a card.
const SELECTOR: f32 = 78.0;

/// Memory key for whether the send window is open.
fn sends_open_id() -> Id {
    Id::new("sends-open")
}

/// The cell's own chain and its send amounts.
///
/// Filter and drive belong to the chop and run per voice. The four sends are
/// amounts only: the effects themselves are shared, because sixteen voices
/// would otherwise mean sixteen reverbs.
pub fn effects_section(ui: &mut Ui, cell: &mut PerformanceCell, sounding: bool) -> bool {
    let mut changed = false;
    let mut open_sends = false;

    let lit = (sounding && cell.effects.is_active()).then_some(THEME.active);
    section_with(
        ui,
        "EFFECTS",
        lit,
        |ui| {
            if icon_button(ui, &THEME, Icon::Edit, "Send-Effekte einstellen") {
                open_sends = true;
            }
        },
        |ui| {
            ui.horizontal(|ui| {
                changed |= filter_card(ui, &mut cell.effects);
                changed |= drive_card(ui, &mut cell.effects);
                changed |= send_card(ui, &mut cell.effects);
            });
        },
    );

    if open_sends {
        ui.memory_mut(|memory| {
            let open: bool = memory.data.get_temp(sends_open_id()).unwrap_or(false);
            memory.data.insert_temp(sends_open_id(), !open);
        });
    }

    changed
}

/// The recessed plate a card sits on, with its name and its lamp.
///
/// Returns the area left for the controls.
fn card(ui: &mut Ui, title: &str, width: f32, lit: bool) -> Rect {
    let height = 86.0;
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    inset(ui.painter(), &THEME, rect, THEME.control_pressed_bg);

    lamp(
        ui.painter(),
        &THEME,
        pos2(rect.min.x + 12.0, rect.min.y + 13.0),
        lit.then_some(THEME.accent),
    );
    ui.painter().text(
        pos2(rect.min.x + 24.0, rect.min.y + 13.0),
        Align2::LEFT_CENTER,
        title,
        FontId::proportional(THEME.font_sm),
        if lit { THEME.accent } else { THEME.text_dim },
    );

    Rect::from_min_max(
        pos2(rect.min.x + 6.0, rect.min.y + 22.0),
        pos2(rect.max.x - 6.0, rect.max.y - 2.0),
    )
}

/// Lay controls out inside a card.
fn inside(ui: &mut Ui, area: Rect, contents: impl FnOnce(&mut Ui)) {
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(area)
            .layout(egui::Layout::left_to_right(egui::Align::Min)),
        |ui| {
            ui.spacing_mut().item_spacing = vec2(THEME.spacing_sm, 0.0);
            contents(ui);
        },
    );
}

fn filter_card(ui: &mut Ui, effects: &mut CellEffects) -> bool {
    let mut changed = false;
    let area = card(ui, "FILTER", 216.0, effects.filter_on);

    inside(ui, area, |ui| {
        if toggle(ui, &THEME, "On", effects.filter_on) {
            effects.filter_on = !effects.filter_on;
            changed = true;
        }

        let shapes: Vec<&str> = FilterShape::ALL.iter().map(|shape| shape.label()).collect();
        let selected = FilterShape::ALL
            .iter()
            .position(|shape| *shape == effects.filter_shape)
            .unwrap_or(0);
        if let Some(index) = dropdown(ui, &THEME, "filter-shape", &shapes, selected, SELECTOR) {
            effects.filter_shape = FilterShape::ALL[index];
            changed = true;
        }

        changed |= value_knob(
            ui,
            &THEME,
            KnobSpec {
                label: "Cutoff",
                range: (MIN_CUTOFF_HZ, MAX_CUTOFF_HZ),
                default: 8_000.0,
                taper: Taper::Logarithmic,
                unit: Unit::Hertz,
                diameter: KNOB,
                modulated: None,
            },
            &mut effects.cutoff_hz,
        );
        changed |= value_knob(
            ui,
            &THEME,
            KnobSpec {
                label: "Reso",
                range: (MIN_RESONANCE, MAX_RESONANCE),
                default: 0.707,
                taper: Taper::Logarithmic,
                unit: Unit::Plain,
                diameter: KNOB,
                modulated: None,
            },
            &mut effects.resonance,
        );
    });

    changed
}

fn drive_card(ui: &mut Ui, effects: &mut CellEffects) -> bool {
    let mut changed = false;
    let area = card(ui, "DRIVE", 158.0, effects.drive_on);

    inside(ui, area, |ui| {
        if toggle(ui, &THEME, "On", effects.drive_on) {
            effects.drive_on = !effects.drive_on;
            changed = true;
        }

        let shapes: Vec<&str> = DriveShape::ALL.iter().map(|shape| shape.label()).collect();
        let selected = DriveShape::ALL
            .iter()
            .position(|shape| *shape == effects.drive_shape)
            .unwrap_or(0);
        if let Some(index) = dropdown(ui, &THEME, "drive-shape", &shapes, selected, 62.0) {
            effects.drive_shape = DriveShape::ALL[index];
            changed = true;
        }

        changed |= value_knob(
            ui,
            &THEME,
            KnobSpec {
                label: "Drive",
                range: (1.0, MAX_DRIVE),
                default: 1.0,
                taper: Taper::Logarithmic,
                unit: Unit::Multiplier,
                diameter: KNOB,
                modulated: None,
            },
            &mut effects.drive,
        );
    });

    changed
}

fn send_card(ui: &mut Ui, effects: &mut CellEffects) -> bool {
    let mut changed = false;
    let any = effects.delay_send > 0.0
        || effects.reverb_send > 0.0
        || effects.phaser_send > 0.0
        || effects.flanger_send > 0.0;
    let area = card(ui, "SENDS", 198.0, any);

    inside(ui, area, |ui| {
        ui.spacing_mut().item_spacing = vec2(THEME.spacing_sm, 2.0);
        ui.vertical(|ui| {
            for (label, amount) in [
                ("Delay", &mut effects.delay_send),
                ("Reverb", &mut effects.reverb_send),
            ] {
                changed |= value_slider(
                    ui,
                    &THEME,
                    SliderSpec {
                        label,
                        range: (0.0, 1.0),
                        default: 0.0,
                        unit: Unit::Plain,
                        width: SEND_WIDTH,
                    },
                    amount,
                );
            }
        });
        ui.vertical(|ui| {
            for (label, amount) in [
                ("Phaser", &mut effects.phaser_send),
                ("Flanger", &mut effects.flanger_send),
            ] {
                changed |= value_slider(
                    ui,
                    &THEME,
                    SliderSpec {
                        label,
                        range: (0.0, 1.0),
                        default: 0.0,
                        unit: Unit::Plain,
                        width: SEND_WIDTH,
                    },
                    amount,
                );
            }
        });
    });

    changed
}

/// The window the shared sends are set up in.
///
/// A window rather than a panel: these settings are shared by every cell, so
/// they are not part of the one being edited, and they are reached for far
/// less often than the amounts that feed them.
pub fn sends_window(ui: &Ui, state: &ViewState<'_>) {
    let mut open = ui.memory(|memory| memory.data.get_temp(sends_open_id()).unwrap_or(false));
    if !open {
        return;
    }

    let Ok(mut project) = state.project.lock() else {
        return;
    };
    let mut sends = project.project.sends();
    let before = sends;

    egui::Window::new("Send-Effekte")
        .id(Id::new("sends-window"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_pos(pos2(300.0, 260.0))
        .show(ui.ctx(), |ui| {
            ui.spacing_mut().item_spacing = vec2(THEME.spacing_sm, THEME.spacing_sm);

            hint_light(ui, "DELAY");
            ui.horizontal(|ui| {
                if toggle(ui, &THEME, "Sync", sends.delay_sync) {
                    sends.delay_sync = !sends.delay_sync;
                }
                if sends.delay_sync {
                    let divisions: Vec<&str> = Division::ALL
                        .iter()
                        .map(|division| division.label())
                        .collect();
                    let selected = Division::ALL
                        .iter()
                        .position(|division| *division == sends.delay_division)
                        .unwrap_or(0);
                    if let Some(index) =
                        dropdown(ui, &THEME, "delay-division", &divisions, selected, 80.0)
                    {
                        sends.delay_division = Division::ALL[index];
                    }
                } else {
                    knob(
                        ui,
                        "Zeit",
                        &mut sends.delay_seconds,
                        (0.01, 2.0),
                        0.25,
                        Unit::Plain,
                    );
                }
                knob(
                    ui,
                    "Feedback",
                    &mut sends.delay_feedback,
                    (0.0, 0.95),
                    0.35,
                    Unit::Plain,
                );
                knob(
                    ui,
                    "Dämpfung",
                    &mut sends.delay_damping_hz,
                    (200.0, 18_000.0),
                    6_000.0,
                    Unit::Hertz,
                );
            });

            hint_light(ui, "REVERB");
            ui.horizontal(|ui| {
                knob(
                    ui,
                    "Größe",
                    &mut sends.reverb_size,
                    (0.0, 1.0),
                    0.6,
                    Unit::Plain,
                );
                knob(
                    ui,
                    "Dämpfung",
                    &mut sends.reverb_damping,
                    (0.0, 0.95),
                    0.4,
                    Unit::Plain,
                );
            });

            hint_light(ui, "PHASER");
            ui.horizontal(|ui| {
                knob(
                    ui,
                    "Rate",
                    &mut sends.phaser_rate_hz,
                    (0.01, 10.0),
                    0.5,
                    Unit::Hertz,
                );
                knob(
                    ui,
                    "Tiefe",
                    &mut sends.phaser_depth,
                    (0.0, 1.0),
                    0.7,
                    Unit::Plain,
                );
                knob(
                    ui,
                    "Feedback",
                    &mut sends.phaser_feedback,
                    (0.0, 0.9),
                    0.4,
                    Unit::Plain,
                );
            });

            hint_light(ui, "FLANGER");
            ui.horizontal(|ui| {
                knob(
                    ui,
                    "Rate",
                    &mut sends.flanger_rate_hz,
                    (0.01, 10.0),
                    0.3,
                    Unit::Hertz,
                );
                knob(
                    ui,
                    "Tiefe",
                    &mut sends.flanger_depth,
                    (0.0, 1.0),
                    0.8,
                    Unit::Plain,
                );
                knob(
                    ui,
                    "Feedback",
                    &mut sends.flanger_feedback,
                    (-0.95, 0.95),
                    0.5,
                    Unit::Plain,
                );
            });

            hint_light(
                ui,
                "Diese Effekte teilen sich alle Cells; wie viel ankommt, steht pro Cell",
            );
        });

    if sends != before {
        project.project.set_sends(sends);
        state.send(saempler_audio::EngineCommand::SetSends(
            project.project.sends(),
        ));
    }
    ui.memory_mut(|memory| memory.data.insert_temp(sends_open_id(), open));
}

/// A knob on the send window, where every one looks the same.
fn knob(ui: &mut Ui, label: &str, value: &mut f32, range: (f32, f32), default: f32, unit: Unit) {
    value_knob(
        ui,
        &THEME,
        KnobSpec {
            label,
            range,
            default,
            taper: Taper::Linear,
            unit,
            diameter: KNOB,
            modulated: None,
        },
        value,
    );
}

/// Push the send settings to the engine, after a project is loaded.
pub fn sync_sends(state: &ViewState<'_>, project: &saempler_model::ProjectFile) {
    state.send(saempler_audio::EngineCommand::SetSends(
        project.project.sends(),
    ));
}
