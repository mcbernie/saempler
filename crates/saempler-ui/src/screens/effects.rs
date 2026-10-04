use nih_plug_egui::egui::{self, pos2, vec2, Align2, FontId, Id, Rect, Ui};
use saempler_model::{
    CellEffects, Division, DriveShape, FilterShape, ModDestination, PerformanceCell, SendEffects,
    MAX_CUTOFF_HZ, MAX_DRIVE, MAX_RESONANCE, MAX_SEND_DRIVE, MAX_SEND_LEVEL, MIN_CUTOFF_HZ,
    MIN_RESONANCE,
};

use saempler_audio::{CUTOFF_RANGE_OCTAVES, DRIVE_RANGE_OCTAVES, RESONANCE_RANGE};

use crate::screens::cell::Live;
use crate::screens::main::{hint_light, section_with, ViewState, THEME};
use crate::widgets::{
    dropdown, icon_button, inset, lamp, toggle, value_knob, value_slider, Icon, KnobSpec,
    SliderSpec, Taper, Unit,
};

/// Diameter of a knob on an effect card.
const KNOB: f32 = 34.0;
/// Width of a send amount bar.
const SEND_WIDTH: f32 = 136.0;
/// Width of a selector on a card.
const SELECTOR: f32 = 84.0;
/// Height of a card carrying knobs: the legend, the dial and its two label
/// lines.
const KNOB_CARD: f32 = 86.0;
/// Height of a card carrying only bars.
const BAR_CARD: f32 = 50.0;
/// Room the legend takes at the top of a card.
const CARD_LEGEND: f32 = 22.0;

/// Memory key for whether the send window is open.
fn sends_open_id() -> Id {
    Id::new("sends-open")
}

/// The cell's own chain and its send amounts.
///
/// Filter and drive belong to the chop and run per voice. The four sends are
/// amounts only: the effects themselves are shared, because sixteen voices
/// would otherwise mean sixteen reverbs.
pub(crate) fn effects_section(ui: &mut Ui, cell: &mut PerformanceCell, live: Live) -> bool {
    let sounding = live.sounding;
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
            ui.spacing_mut().item_spacing.y = THEME.spacing_sm;
            // Two rows rather than three cards in one: the sends need the
            // full width to line their four bars up, and a row of unequal
            // cards reads as a leftover rather than as a layout.
            ui.horizontal(|ui| {
                changed |= filter_card(ui, &mut cell.effects, live);
                changed |= drive_card(ui, &mut cell.effects, live);
            });
            changed |= send_card(ui, &mut cell.effects);
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
/// Returns the area left for the controls, laid out the same way on every
/// card so the three of them read as one row of equipment.
fn card(ui: &mut Ui, title: &str, size: (f32, f32), lit: bool, contents: impl FnOnce(&mut Ui)) {
    // The card's own rectangle, taken before anything is drawn in it. The
    // cursor is advanced past it at the end rather than by allocating here:
    // a child laid out in a given rectangle reports only the room it used,
    // which would pull the cursor back inside the card and overlap the next.
    let rect = Rect::from_min_size(ui.cursor().min, vec2(size.0, size.1));
    inset(ui.painter(), &THEME, rect, THEME.control_pressed_bg);

    // The legend runs down the left edge of the card, which leaves the whole
    // width for the controls and keeps every card the same shape.
    lamp(
        ui.painter(),
        &THEME,
        pos2(rect.min.x + 13.0, rect.min.y + 14.0),
        lit.then_some(THEME.accent),
    );
    ui.painter().text(
        pos2(rect.min.x + 25.0, rect.min.y + 14.0),
        Align2::LEFT_CENTER,
        title,
        FontId::proportional(THEME.font_sm),
        if lit { THEME.accent } else { THEME.text_dim },
    );

    let area = Rect::from_min_max(
        pos2(rect.min.x + 8.0, rect.min.y + CARD_LEGEND),
        pos2(rect.max.x - 8.0, rect.max.y - 2.0),
    );
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(area)
            .layout(egui::Layout::left_to_right(egui::Align::Min)),
        |ui| {
            ui.spacing_mut().item_spacing = vec2(THEME.spacing_sm, 0.0);
            contents(ui);
        },
    );
    ui.advance_cursor_after_rect(rect);
}

fn filter_card(ui: &mut Ui, effects: &mut CellEffects, live: Live) -> bool {
    let mut changed = false;

    card(ui, "FILTER", (330.0, KNOB_CARD), effects.filter_on, |ui| {
        if toggle(ui, &THEME, "An", effects.filter_on) {
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
                // The route works in octaves, so the mark lands where the
                // filter actually is rather than where a linear reading of
                // the amount would put it.
                modulated: live
                    .reaching(ModDestination::FilterCutoff)
                    .map(|amount| effects.cutoff_hz * 2.0f32.powf(amount * CUTOFF_RANGE_OCTAVES)),
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
                modulated: live
                    .reaching(ModDestination::Resonance)
                    .map(|amount| effects.resonance + amount * RESONANCE_RANGE),
            },
            &mut effects.resonance,
        );
    });

    changed
}

fn drive_card(ui: &mut Ui, effects: &mut CellEffects, live: Live) -> bool {
    let mut changed = false;

    card(ui, "DRIVE", (262.0, KNOB_CARD), effects.drive_on, |ui| {
        if toggle(ui, &THEME, "An", effects.drive_on) {
            effects.drive_on = !effects.drive_on;
            changed = true;
        }

        let shapes: Vec<&str> = DriveShape::ALL.iter().map(|shape| shape.label()).collect();
        let selected = DriveShape::ALL
            .iter()
            .position(|shape| *shape == effects.drive_shape)
            .unwrap_or(0);
        if let Some(index) = dropdown(ui, &THEME, "drive-shape", &shapes, selected, SELECTOR) {
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
                modulated: live
                    .reaching(ModDestination::Drive)
                    .map(|amount| effects.drive * 2.0f32.powf(amount * DRIVE_RANGE_OCTAVES)),
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

    card(ui, "SENDS", (600.0, BAR_CARD), any, |ui| {
        for (label, amount) in [
            ("Delay", &mut effects.delay_send),
            ("Reverb", &mut effects.reverb_send),
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

    changed
}

/// Memory key for which half of the rack the window is editing.
fn driven_page_id() -> Id {
    Id::new("sends-driven-page")
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
    let mut rack = project.project.sends();
    let before = rack;
    let mut page_driven = ui.memory(|memory| {
        memory
            .data
            .get_temp::<bool>(driven_page_id())
            .unwrap_or(false)
    });

    egui::Window::new("Send-Effekte")
        .id(Id::new("sends-window"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_pos(pos2(300.0, 200.0))
        .show(ui.ctx(), |ui| {
            ui.spacing_mut().item_spacing = vec2(THEME.spacing_sm, THEME.spacing_sm);

            // The two halves of the rack are the same four effects set up
            // twice, so they share one page rather than sitting side by side:
            // double the knobs on screen would make neither set readable.
            ui.horizontal(|ui| {
                if toggle(ui, &THEME, "Normal", !page_driven) {
                    page_driven = false;
                }
                if toggle(ui, &THEME, "Getrieben", page_driven) {
                    page_driven = true;
                }
                hint_light(
                    ui,
                    if page_driven {
                        "Mit gehaltener Modifier-Taste"
                    } else {
                        "Im Normalbetrieb"
                    },
                );
            });

            let sends = if page_driven {
                &mut rack.driven
            } else {
                &mut rack.normal
            };
            send_controls(ui, sends);

            if page_driven {
                hint_light(ui, "SÄTTIGUNG");
                ui.horizontal(|ui| {
                    knob(
                        ui,
                        "Drive",
                        &mut rack.drive,
                        (1.0, MAX_SEND_DRIVE),
                        6.0,
                        Unit::Multiplier,
                    );
                    let shapes: Vec<&str> =
                        DriveShape::ALL.iter().map(|shape| shape.label()).collect();
                    let selected = DriveShape::ALL
                        .iter()
                        .position(|shape| *shape == rack.drive_shape)
                        .unwrap_or(0);
                    if let Some(index) =
                        dropdown(ui, &THEME, "send-drive-shape", &shapes, selected, SELECTOR)
                    {
                        rack.drive_shape = DriveShape::ALL[index];
                    }
                    hint_light(ui, "Nur auf dem getriebenen Weg");
                });
            }

            hint_light(ui, "Pegel regelt, wie laut ein Send zurückkommt");
        });

    if rack != before {
        project.project.set_sends(rack);
        state.send(saempler_audio::EngineCommand::SetSends(
            project.project.sends(),
        ));
    }
    ui.memory_mut(|memory| {
        memory.data.insert_temp(sends_open_id(), open);
        memory.data.insert_temp(driven_page_id(), page_driven);
    });
}

/// The four effects of one half of the rack.
///
/// Every group ends in its return level, because that is the control reached
/// for first when an effect is too loud, and it reads as part of the effect
/// rather than as a mixer somewhere else.
fn send_controls(ui: &mut Ui, sends: &mut SendEffects) {
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
            if let Some(index) = dropdown(ui, &THEME, "delay-division", &divisions, selected, 80.0)
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
        level_knob(ui, &mut sends.delay_level, 0.45);
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
        level_knob(ui, &mut sends.reverb_level, 0.35);
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
        level_knob(ui, &mut sends.phaser_level, 0.5);
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
        level_knob(ui, &mut sends.flanger_level, 0.5);
    });
}

/// The return level of one send.
fn level_knob(ui: &mut Ui, value: &mut f32, default: f32) {
    knob(
        ui,
        "Pegel",
        value,
        (0.0, MAX_SEND_LEVEL),
        default,
        Unit::Plain,
    );
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
