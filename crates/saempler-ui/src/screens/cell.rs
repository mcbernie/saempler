use nih_plug_egui::egui::Ui;
use saempler_model::{
    note_name, Division, EnvelopeDefinition, LfoDefinition, LfoShape, ModDestination, ModSource,
    ModulationRoute, PerformanceCell, PlaybackMode, MAX_COLLAPSE, MAX_PITCH_SEMITONES, MAX_ROUTES,
    MAX_SPEED, MIN_COLLAPSE, MIN_SPEED,
};

use crate::screens::main::{hint, placeholder, section, ViewState, THEME};
use crate::screens::performance::sync_cells;
use crate::widgets::{
    button, dropdown, envelope_display, lfo_display, segmented, toggle, value_knob, value_slider,
    KnobSpec, SliderSpec, Taper, Unit,
};

/// Diameter of the playback and envelope knobs.
const KNOB_DIAMETER: f32 = 42.0;
/// Width of the amount bar in a matrix row.
const ROUTE_AMOUNT: f32 = 146.0;
/// Width of the source and destination selectors in a matrix row.
const ROUTE_SELECTOR: f32 = 100.0;
/// Height of a drawn envelope or LFO curve. The width follows the column.
const CURVE_HEIGHT: f32 = 48.0;
/// Longest stage any envelope control reaches, in milliseconds.
const MAX_STAGE_MS: f32 = 4_000.0;
/// Range of the LFO rate control, in hertz.
const LFO_RATE_RANGE: (f32, f32) = (0.01, 50.0);

/// The whole modulation page for the selected cell.
pub fn cell_section(ui: &mut Ui, state: &ViewState<'_>) {
    let Ok(mut project) = state.project.lock() else {
        return;
    };
    let Some(mut cell) = project.project.selected_cell().cloned() else {
        section(ui, "CELL", |ui| {
            placeholder(ui, "Kein Pad gewählt");
            hint(
                ui,
                "Auf der Seite PERFORM ein Pad anklicken, um es hier zu bearbeiten",
            );
        });
        return;
    };

    let mut changed = false;

    changed |= playback_section(ui, &mut cell);
    // Two columns rather than four stacked panels: the matrix is what joins
    // the modules to the sound, and it has to be on the page with them.
    ui.columns(2, |columns| {
        changed |= envelopes_section(&mut columns[0], &mut cell);
        changed |= matrix_section(&mut columns[0], &mut cell);
        changed |= lfos_section(&mut columns[1], &mut cell);
    });

    if changed {
        let id = cell.id;
        let edited = cell.sanitized();
        project.project.with_cell_mut(id, |slot| *slot = edited);
        sync_cells(state, &project);
    }
}

/// Reverse, speed, pitch and level.
fn playback_section(ui: &mut Ui, cell: &mut PerformanceCell) -> bool {
    let mut changed = false;

    section(ui, "PLAYBACK", |ui| {
        ui.horizontal(|ui| {
            if toggle(ui, &THEME, "Reverse", cell.playback.reverse) {
                cell.playback.reverse = !cell.playback.reverse;
                changed = true;
            }

            ui.add_space(THEME.spacing_md);
            changed |= value_knob(
                ui,
                &THEME,
                KnobSpec {
                    label: "Speed",
                    range: (MIN_SPEED, MAX_SPEED),
                    default: 1.0,
                    taper: Taper::Logarithmic,
                    unit: Unit::Multiplier,
                    diameter: KNOB_DIAMETER,
                },
                &mut cell.playback.speed,
            );
            changed |= value_knob(
                ui,
                &THEME,
                KnobSpec {
                    label: "Pitch",
                    range: (-MAX_PITCH_SEMITONES, MAX_PITCH_SEMITONES),
                    default: 0.0,
                    taper: Taper::Linear,
                    unit: Unit::Semitones,
                    diameter: KNOB_DIAMETER,
                },
                &mut cell.playback.pitch_semitones,
            );
            changed |= value_knob(
                ui,
                &THEME,
                KnobSpec {
                    label: "Gain",
                    range: (0.0, 2.0),
                    default: 1.0,
                    taper: Taper::Linear,
                    unit: Unit::Multiplier,
                    diameter: KNOB_DIAMETER,
                },
                &mut cell.playback.gain,
            );

            ui.add_space(THEME.spacing_md);
            ui.vertical(|ui| {
                hint(ui, &format!("Note {}", note_name(cell.midi_note)));
                hint(
                    ui,
                    "Speed und Pitch wirken beide auf die Lesegeschwindigkeit",
                );
                hint(ui, "die Länge ändert sich also mit");
            });
        });

        changed |= mode_controls(ui, cell);
    });

    changed
}

/// The playback mode and the settings that belong to it.
///
/// Only the controls the chosen mode uses are shown: a collapse factor next to
/// a gated cell would be a dial that does nothing.
fn mode_controls(ui: &mut Ui, cell: &mut PerformanceCell) -> bool {
    let mut changed = false;

    ui.horizontal(|ui| {
        let labels: Vec<&str> = PlaybackMode::ALL.iter().map(|mode| mode.label()).collect();
        let selected = PlaybackMode::ALL
            .iter()
            .position(|mode| *mode == cell.playback.mode)
            .unwrap_or(0);
        if let Some(index) = segmented(ui, &THEME, &labels, selected) {
            cell.playback.mode = PlaybackMode::ALL[index];
            changed = true;
        }

        ui.add_space(THEME.spacing_md);

        if cell.playback.mode.uses_division() {
            let divisions: Vec<&str> = Division::ALL
                .iter()
                .map(|division| division.label())
                .collect();
            let selected = Division::ALL
                .iter()
                .position(|division| *division == cell.playback.division)
                .unwrap_or(0);
            if let Some(index) = dropdown(
                ui,
                &THEME,
                "mode-division",
                &divisions,
                selected,
                ROUTE_SELECTOR,
            ) {
                cell.playback.division = Division::ALL[index];
                changed = true;
            }
        }

        if cell.playback.mode == PlaybackMode::Collapse {
            changed |= value_slider(
                ui,
                &THEME,
                SliderSpec {
                    label: "Collapse",
                    range: (MIN_COLLAPSE, MAX_COLLAPSE),
                    default: 0.75,
                    unit: Unit::Plain,
                    width: ROUTE_AMOUNT,
                },
                &mut cell.playback.collapse,
            );
        }

        if cell.playback.mode.loops()
            && toggle(ui, &THEME, "Release Trigger", cell.playback.release_trigger)
        {
            cell.playback.release_trigger = !cell.playback.release_trigger;
            changed = true;
        }
    });

    // On its own line: with a collapse selected the row above is already full.
    hint(
        ui,
        mode_hint(cell.playback.mode, cell.playback.release_trigger),
    );

    changed
}

/// One line saying what the chosen mode does.
fn mode_hint(mode: PlaybackMode, release_trigger: bool) -> &'static str {
    match (mode, release_trigger) {
        (PlaybackMode::Gate, _) => "spielt, solange die Taste liegt",
        (PlaybackMode::OneShot, _) => "spielt den Slice zu Ende, Taste egal",
        (PlaybackMode::Loop, false) => "wiederholt den ganzen Slice",
        (PlaybackMode::Repeat, false) => "wiederholt die gewählte Notenlänge",
        (PlaybackMode::Collapse, false) => "Loop wird mit jedem Durchlauf kürzer",
        (_, true) => "Loop startet erst beim Loslassen und läuft im Release aus",
    }
}

/// Both envelopes, one below the other.
fn envelopes_section(ui: &mut Ui, cell: &mut PerformanceCell) -> bool {
    let mut changed = false;

    section(ui, "ENVELOPES", |ui| {
        for (index, name) in ["ENV A", "ENV B"].into_iter().enumerate() {
            let width = ui.available_width();
            envelope_display(
                ui,
                &THEME,
                name,
                cell.envelopes[index],
                (width, CURVE_HEIGHT),
            );
            ui.horizontal(|ui| {
                changed |= envelope_controls(ui, &mut cell.envelopes[index]);
            });
        }
    });

    changed
}

/// The four stage knobs of one envelope.
fn envelope_controls(ui: &mut Ui, envelope: &mut EnvelopeDefinition) -> bool {
    let default = EnvelopeDefinition::default();
    let stage = |label: &'static str, default: f32| KnobSpec {
        label,
        range: (0.0, MAX_STAGE_MS),
        default,
        taper: Taper::Skewed,
        unit: Unit::Milliseconds,
        diameter: KNOB_DIAMETER,
    };
    let mut changed = false;

    changed |= value_knob(
        ui,
        &THEME,
        stage("Attack", default.attack_ms),
        &mut envelope.attack_ms,
    );
    changed |= value_knob(
        ui,
        &THEME,
        stage("Decay", default.decay_ms),
        &mut envelope.decay_ms,
    );
    changed |= value_knob(
        ui,
        &THEME,
        KnobSpec {
            label: "Sustain",
            range: (0.0, 1.0),
            default: default.sustain,
            taper: Taper::Linear,
            unit: Unit::Plain,
            diameter: KNOB_DIAMETER,
        },
        &mut envelope.sustain,
    );
    changed |= value_knob(
        ui,
        &THEME,
        stage("Release", default.release_ms),
        &mut envelope.release_ms,
    );

    changed
}

/// Both LFOs, one below the other.
fn lfos_section(ui: &mut Ui, cell: &mut PerformanceCell) -> bool {
    let mut changed = false;

    section(ui, "LFOS", |ui| {
        for (index, name) in ["LFO 1", "LFO 2"].into_iter().enumerate() {
            let width = ui.available_width();
            lfo_display(
                ui,
                &THEME,
                name,
                cell.lfos[index].shape,
                (width, CURVE_HEIGHT),
            );
            changed |= lfo_controls(ui, index, &mut cell.lfos[index]);
        }
    });

    changed
}

/// Shape, rate and the two switches of one LFO.
fn lfo_controls(ui: &mut Ui, index: usize, lfo: &mut LfoDefinition) -> bool {
    let mut changed = false;

    let shapes: Vec<&str> = LfoShape::ALL.iter().map(|shape| shape.label()).collect();
    let selected = LfoShape::ALL
        .iter()
        .position(|shape| *shape == lfo.shape)
        .unwrap_or(0);
    if let Some(shape) = segmented(ui, &THEME, &shapes, selected) {
        lfo.shape = LfoShape::ALL[shape];
        changed = true;
    }

    ui.horizontal(|ui| {
        if toggle(ui, &THEME, "Sync", lfo.sync) {
            lfo.sync = !lfo.sync;
            changed = true;
        }
        if toggle(ui, &THEME, "Retrigger", lfo.retrigger) {
            lfo.retrigger = !lfo.retrigger;
            changed = true;
        }

        // The rate is given either in hertz or as a note value, never both,
        // so the two controls share the place next to the switches.
        if lfo.sync {
            let divisions: Vec<&str> = Division::ALL
                .iter()
                .map(|division| division.label())
                .collect();
            let selected = Division::ALL
                .iter()
                .position(|division| *division == lfo.division)
                .unwrap_or(0);
            if let Some(index) = dropdown(
                ui,
                &THEME,
                ("division", index),
                &divisions,
                selected,
                ROUTE_SELECTOR,
            ) {
                lfo.division = Division::ALL[index];
                changed = true;
            }
        } else {
            changed |= value_knob(
                ui,
                &THEME,
                KnobSpec {
                    label: "Rate",
                    range: LFO_RATE_RANGE,
                    default: LfoDefinition::default().rate_hz,
                    taper: Taper::Logarithmic,
                    unit: Unit::Hertz,
                    diameter: KNOB_DIAMETER,
                },
                &mut lfo.rate_hz,
            );
        }
    });

    changed
}

/// The modulation matrix: one row per route.
fn matrix_section(ui: &mut Ui, cell: &mut PerformanceCell) -> bool {
    let mut changed = false;

    section(ui, "MOD MATRIX", |ui| {
        let sources: Vec<&str> = ModSource::ALL.iter().map(|source| source.label()).collect();
        let destinations: Vec<&str> = ModDestination::ALL
            .iter()
            .map(|destination| destination.label())
            .collect();

        let mut remove: Option<usize> = None;

        for index in 0..cell.routes.len() {
            ui.horizontal(|ui| {
                let route = &mut cell.routes[index];

                let selected = ModSource::ALL
                    .iter()
                    .position(|source| *source == route.source)
                    .unwrap_or(0);
                if let Some(next) = dropdown(
                    ui,
                    &THEME,
                    ("source", index),
                    &sources,
                    selected,
                    ROUTE_SELECTOR,
                ) {
                    route.source = ModSource::ALL[next];
                    changed = true;
                }

                let selected = ModDestination::ALL
                    .iter()
                    .position(|destination| *destination == route.destination)
                    .unwrap_or(0);
                if let Some(next) = dropdown(
                    ui,
                    &THEME,
                    ("destination", index),
                    &destinations,
                    selected,
                    ROUTE_SELECTOR,
                ) {
                    route.destination = ModDestination::ALL[next];
                    changed = true;
                }

                changed |= value_slider(
                    ui,
                    &THEME,
                    SliderSpec {
                        label: "Amount",
                        range: (-1.0, 1.0),
                        default: 1.0,
                        unit: Unit::Plain,
                        width: ROUTE_AMOUNT,
                    },
                    &mut route.amount,
                );

                // A row is as narrow as the column allows, so the button
                // that takes it out carries a mark rather than a word.
                if button(ui, &THEME, "×") {
                    remove = Some(index);
                }
            });
        }

        if let Some(index) = remove {
            cell.remove_route(index);
            changed = true;
        }

        ui.horizontal(|ui| {
            if cell.routes.len() < MAX_ROUTES && button(ui, &THEME, "Route hinzufügen") {
                cell.add_route(ModulationRoute::default());
                changed = true;
            }
            hint(
                ui,
                &format!("{} von {MAX_ROUTES} Routen belegt", cell.routes.len()),
            );
        });

        // Volume is a route like any other, so it can be taken out. The cell
        // is then silent, which is worth saying rather than letting the user
        // hunt for a voice that never sounds.
        if !cell.has_amplitude() {
            hint(
                ui,
                "Keine Route auf Volume — diese Zelle bleibt stumm. ENV A → Volume stellt sie wieder her.",
            );
        }
    });

    changed
}
