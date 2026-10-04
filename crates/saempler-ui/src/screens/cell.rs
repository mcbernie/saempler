use nih_plug_egui::egui::{self, epaint::PathShape, pos2, vec2, Align2, FontId, Sense, Stroke, Ui};
use saempler_model::{
    note_name, Division, EnvelopeDefinition, LfoDefinition, LfoShape, ModDestination, ModSource,
    ModulationRoute, PerformanceCell, PlaybackMode, DESTINATION_COUNT, MAX_COLLAPSE,
    MAX_PITCH_SEMITONES, MAX_ROUTES, MAX_SPEED, MIN_COLLAPSE, MIN_SPEED,
};

use crate::screens::main::{
    band, hint, hint_light, placeholder, region, section, ViewState, THEME,
};
use crate::screens::performance::sync_cells;
use crate::widgets::{
    dropdown, envelope_display, icon_button, lfo_display, toggle, value_knob, value_slider, Icon,
    KnobSpec, SliderSpec, Taper, Unit,
};

/// Diameter of the playback and envelope knobs.
const KNOB_DIAMETER: f32 = 36.0;
/// Width of the amount bar in a matrix row.
const ROUTE_AMOUNT: f32 = 150.0;
/// Width of the source and destination selectors in a matrix row.
const ROUTE_SELECTOR: f32 = 112.0;
/// Size of a drawn envelope or LFO curve, which sits beside its controls.
const CURVE_SIZE: (f32, f32) = (150.0, 58.0);
/// Width of the arrow column between a route's source and destination.
const ARROW_WIDTH: f32 = 20.0;
/// Height of the playback panel. The modulation panel takes what is left.
const PLAYBACK_HEIGHT: f32 = 112.0;
/// Height of the effects panel at the foot of the editor column.
const EFFECTS_HEIGHT: f32 = 186.0;
/// Keys a cell may be put on.
///
/// Six octaves around where chops are usually mapped. The whole keyboard
/// would be a list of 128 entries to scroll past.
const CELL_NOTES: std::ops::Range<u8> = 36..108;
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
        section(ui, "SLICE / SOUND", None, |ui| {
            placeholder(ui, "Kein Pad gewählt");
            hint(
                ui,
                "Auf der Seite PERFORM ein Pad anklicken, um es hier zu bearbeiten",
            );
        });
        return;
    };

    let mut changed = false;
    let live = Live::read(state);

    // The column is split the same way the window is: a panel fills the
    // rectangle it was handed, so two stacked panels each need one of their
    // own rather than both claiming the whole column.
    let full = ui.available_rect_before_wrap();
    let playback = band(full, full.min.y, PLAYBACK_HEIGHT);
    let effects = band(full, full.max.y - EFFECTS_HEIGHT, EFFECTS_HEIGHT);
    let modulation = nih_plug_egui::egui::Rect::from_min_max(
        pos2(full.min.x, playback.max.y + THEME.spacing_md),
        pos2(full.max.x, effects.min.y - THEME.spacing_md),
    );

    region(ui, playback, |ui| {
        changed |= playback_section(ui, &mut project, &mut cell, live);
    });
    region(ui, modulation, |ui| {
        changed |= modulation_section(ui, &mut cell, live);
    });
    region(ui, effects, |ui| {
        changed |= crate::screens::effects::effects_section(ui, &mut cell, live);
    });
    changed |= matrix_window(ui, &mut cell);

    if changed {
        let id = cell.id;
        let edited = cell.sanitized();
        project.project.with_cell_mut(id, |slot| *slot = edited);
        sync_cells(state, &project);
    }
}

/// Everything the header strip shows, read before the panel is drawn.
///
/// Gathered up front because the header and the contents of a panel are both
/// alive inside one call, and the header cannot hold the project while the
/// contents are editing the cell.
#[derive(Debug, Clone, Copy)]
struct PagerView {
    position: usize,
    count: usize,
    note: u8,
    slice_index: usize,
    start: f32,
    end: f32,
}

impl PagerView {
    fn read(project: &saempler_model::ProjectFile, cell: &PerformanceCell) -> Self {
        let rate = project
            .project
            .sample
            .as_ref()
            .map(|sample| sample.sample_rate)
            .unwrap_or(0)
            .max(1) as f32;
        let slice = project.project.slice(cell.slice).copied();

        Self {
            position: project
                .project
                .cells()
                .iter()
                .position(|candidate| candidate.id == cell.id)
                .unwrap_or(0),
            count: project.project.cells().len(),
            note: cell.midi_note,
            slice_index: slice
                .and_then(|slice| {
                    project
                        .project
                        .slices()
                        .iter()
                        .position(|candidate| candidate.id == slice.id)
                })
                .unwrap_or(0),
            start: slice
                .map(|slice| slice.start_frame as f32 / rate)
                .unwrap_or(0.0),
            end: slice
                .map(|slice| slice.end_frame as f32 / rate)
                .unwrap_or(0.0),
        }
    }
}

/// The strip above the editor: which chop is up, and arrows to the others.
///
/// The mockup's `< 4/12 >` row. Stepping goes by key order, which is the order
/// the pads sit in, so the arrows walk the grid.
fn pager(ui: &mut Ui, view: PagerView) -> isize {
    // This row sits on the dark bezel between panels, not on a panel, so its
    // text is the light one.
    let light = |ui: &mut Ui, text: &str| {
        let width = text.chars().count() as f32 * THEME.font_sm * 0.62 + 4.0;
        let (rect, _) = ui.allocate_exact_size(vec2(width, THEME.font_sm * 1.7), Sense::hover());
        ui.painter().text(
            rect.left_center(),
            Align2::LEFT_CENTER,
            text,
            FontId::proportional(THEME.font_sm),
            THEME.text_dim,
        );
    };

    let mut step = 0;
    ui.horizontal(|ui| {
        if icon_button(ui, &THEME, Icon::Previous, "Vorheriges Pad") {
            step = -1;
        }
        light(ui, &format!("{} / {}", view.position + 1, view.count));
        if icon_button(ui, &THEME, Icon::Next, "Nächstes Pad") {
            step = 1;
        }

        ui.add_space(THEME.spacing_md);

        // The chop's chip, in its colour, and where it sits in the sample.
        let color = crate::widgets::slice_color(&THEME, view.slice_index);
        let name = note_name(view.note);
        let width = name.chars().count() as f32 * THEME.font_sm * 0.68 + THEME.spacing_sm * 2.5;
        let (chip, _) = ui.allocate_exact_size(vec2(width, THEME.font_sm + 6.0), Sense::hover());
        ui.painter().rect_filled(chip, THEME.radius_sm, color);
        ui.painter().text(
            chip.center(),
            Align2::CENTER_CENTER,
            name,
            FontId::proportional(THEME.font_sm),
            THEME.window_bg,
        );

        light(
            ui,
            &format!(
                "S{}  ·  START {:.2} s  ·  ENDE {:.2} s  ·  LÄNGE {:.2} s",
                view.slice_index + 1,
                view.start,
                view.end,
                view.end - view.start
            ),
        );
    });

    step
}

/// What the engine is doing right now, read once per frame.
///
/// The interface asks the meters rather than recomputing anything: the voice
/// has already worked these values out, and a second copy of the modulation
/// here would be a second place for it to be wrong.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Live {
    pub sounding: bool,
    pub envelopes: [f32; 2],
    pub lfos: [f32; 2],
    /// Indexed by [`ModDestination::index`], so it has to hold every one of
    /// them rather than the handful the interface happens to draw.
    pub destinations: [f32; DESTINATION_COUNT],
}

impl Live {
    fn read(state: &ViewState<'_>) -> Self {
        let sounding = state.meters.any_playhead();
        Self {
            sounding,
            envelopes: [state.meters.envelope(0), state.meters.envelope(1)],
            lfos: [state.meters.lfo(0), state.meters.lfo(1)],
            destinations: std::array::from_fn(|index| {
                ModDestination::ALL
                    .iter()
                    .find(|destination| destination.index() == index)
                    .map_or(0.0, |destination| state.meters.destination(*destination))
            }),
        }
    }

    /// The lamp colour for a module that is in use.
    fn lamp(self, active: bool) -> Option<nih_plug_egui::egui::Color32> {
        (self.sounding && active).then_some(THEME.active)
    }

    /// How much is reaching a destination, or nothing while silent.
    pub(crate) fn reaching(self, destination: ModDestination) -> Option<f32> {
        let amount = self.destinations[destination.index()];
        (self.sounding && amount.abs() > 0.001).then_some(amount)
    }
}

/// Reverse, speed, pitch, level and the playback mode.
///
/// The pager lives in the panel's own header row: which chop is being edited
/// is this panel's headline, not a row of its own.
fn playback_section(
    ui: &mut Ui,
    project: &mut saempler_model::ProjectFile,
    cell: &mut PerformanceCell,
    live: Live,
) -> bool {
    let mut changed = false;
    let mut moved: Option<u8> = None;
    let mut stepped: isize = 0;
    let view = PagerView::read(project, cell);

    crate::screens::main::section_with(
        ui,
        "PLAYBACK",
        live.lamp(true),
        |ui| stepped = pager(ui, view),
        |ui| {
            ui.horizontal(|ui| {
                // Which key plays this cell. Dragging a pad does the same
                // thing and is quicker, but it is invisible until tried.
                let names: Vec<String> = CELL_NOTES.map(note_name).collect();
                let labels: Vec<&str> = names.iter().map(String::as_str).collect();
                let selected = usize::from(cell.midi_note.saturating_sub(CELL_NOTES.start));
                if let Some(index) = dropdown(ui, &THEME, "cell-note", &labels, selected, 64.0) {
                    let target = CELL_NOTES.start + index as u8;
                    if target != cell.midi_note {
                        moved = Some(target);
                    }
                }

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
                        // The rate destination is a multiplier on the read speed,
                        // exactly as this knob is.
                        modulated: live
                            .reaching(ModDestination::PlaybackRate)
                            .map(|amount| cell.playback.speed * (1.0 + amount).max(0.01)),
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
                        // A full-amount route reaches the end of the knob's own
                        // range, so the two scales are the same.
                        modulated: live.reaching(ModDestination::Pitch).map(|amount| {
                            cell.playback.pitch_semitones + amount * MAX_PITCH_SEMITONES
                        }),
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
                        modulated: None,
                    },
                    &mut cell.playback.gain,
                );

                ui.add_space(THEME.spacing_md);
                let labels: Vec<&str> = PlaybackMode::ALL.iter().map(|mode| mode.label()).collect();
                let selected = PlaybackMode::ALL
                    .iter()
                    .position(|mode| *mode == cell.playback.mode)
                    .unwrap_or(0);
                // A list rather than a segment row: five segments were the
                // one thing wider than the column.
                if let Some(index) = dropdown(ui, &THEME, "playback-mode", &labels, selected, 104.0)
                {
                    cell.playback.mode = PlaybackMode::ALL[index];
                    changed = true;
                }
            });

            changed |= mode_controls(ui, cell);
        },
    );

    // Moving a cell and stepping to another are the project's business, not
    // the copy being edited, so they are applied here rather than written
    // into the copy.
    if let Some(note) = moved {
        if project.project.move_cell_to_note(cell.id, note) {
            cell.midi_note = note;
            changed = true;
        }
    }
    if stepped != 0 && view.count > 0 {
        let next = (view.position as isize + stepped).rem_euclid(view.count as isize) as usize;
        let (id, slice) = {
            let target = &project.project.cells()[next];
            (target.id, target.slice)
        };
        project.project.select_cell(Some(id));
        project.project.select(Some(slice));
    }

    changed
}

/// The playback mode and the settings that belong to it.
///
/// Only the controls the chosen mode uses are shown: a collapse factor next to
/// a gated cell would be a dial that does nothing.
fn mode_controls(ui: &mut Ui, cell: &mut PerformanceCell) -> bool {
    let mut changed = false;

    // Nothing to say and nothing to set while the cell simply gates.
    if cell.playback.mode == PlaybackMode::Gate {
        return false;
    }

    ui.horizontal(|ui| {
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

        hint(
            ui,
            mode_hint(cell.playback.mode, cell.playback.release_trigger),
        );
    });

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

/// Both envelopes and both LFOs on one panel, a slim row each.
///
/// One panel rather than two, because two headers and two sets of margins
/// cost exactly the height that forced the page to scroll.
fn modulation_section(ui: &mut Ui, cell: &mut PerformanceCell, live: Live) -> bool {
    let mut changed = false;

    let running = live.envelopes.iter().any(|level| *level > 0.001)
        || live.lfos.iter().any(|value| value.abs() > 0.001);
    let mut open_matrix = false;
    crate::screens::main::section_with(
        ui,
        "MODULATION",
        live.lamp(running),
        |ui| {
            // The matrix belongs to this panel but not on it: eight rows of
            // three controls would be taller than everything above them.
            if icon_button(ui, &THEME, Icon::Edit, "Mod-Matrix bearbeiten") {
                open_matrix = true;
            }
        },
        |ui| {
            ui.spacing_mut().item_spacing.y = THEME.spacing_sm;
            for (index, name) in ["ENV A", "ENV B"].into_iter().enumerate() {
                ui.horizontal(|ui| {
                    let level = live.sounding.then_some(live.envelopes[index]);
                    envelope_display(ui, &THEME, name, cell.envelopes[index], level, CURVE_SIZE);
                    ui.add_space(THEME.spacing_sm);
                    changed |= envelope_controls(ui, &mut cell.envelopes[index]);
                });
            }
            for (index, name) in ["LFO 1", "LFO 2"].into_iter().enumerate() {
                ui.horizontal(|ui| {
                    let value = live.sounding.then_some(live.lfos[index]);
                    lfo_display(ui, &THEME, name, cell.lfos[index].shape, value, CURVE_SIZE);
                    ui.add_space(THEME.spacing_sm);
                    changed |= lfo_controls(ui, index, &mut cell.lfos[index]);
                });
            }

            // One line saying where the modulation actually goes, so the panel
            // does not look like four modules wired to nothing.
            hint(ui, &matrix_summary(cell));
        },
    );

    if open_matrix {
        ui.memory_mut(|memory| {
            let open: bool = memory.data.get_temp(matrix_open_id()).unwrap_or(false);
            memory.data.insert_temp(matrix_open_id(), !open);
        });
    }

    changed
}

/// Memory key for whether the matrix window is open.
fn matrix_open_id() -> nih_plug_egui::egui::Id {
    nih_plug_egui::egui::Id::new("mod-matrix-open")
}

/// What the matrix does, in one line.
fn matrix_summary(cell: &PerformanceCell) -> String {
    match cell.routes.len() {
        0 => "Keine Route — diese Zelle bleibt stumm".to_owned(),
        count => {
            let first = cell.routes[0];
            let rest = match count - 1 {
                0 => String::new(),
                more => format!("  +{more}"),
            };
            // Spelled out rather than an arrow: the interface font has no
            // arrow glyph, and a missing one renders as an empty box.
            format!(
                "{} auf {}{rest}",
                first.source.label(),
                first.destination.label()
            )
        }
    }
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
        modulated: None,
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
            modulated: None,
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

/// Shape, rate and the two switches of one LFO, all on one line.
fn lfo_controls(ui: &mut Ui, index: usize, lfo: &mut LfoDefinition) -> bool {
    let mut changed = false;

    let shapes: Vec<&str> = LfoShape::ALL.iter().map(|shape| shape.label()).collect();
    let selected = LfoShape::ALL
        .iter()
        .position(|shape| *shape == lfo.shape)
        .unwrap_or(0);
    if let Some(shape) = dropdown(ui, &THEME, ("shape", index), &shapes, selected, 86.0) {
        lfo.shape = LfoShape::ALL[shape];
        changed = true;
    }

    if toggle(ui, &THEME, "Sync", lfo.sync) {
        lfo.sync = !lfo.sync;
        changed = true;
    }
    if toggle(ui, &THEME, "Retrig", lfo.retrigger) {
        lfo.retrigger = !lfo.retrigger;
        changed = true;
    }

    // The rate is given either in hertz or as a note value, never both, so
    // the two controls share the last place in the line.
    if lfo.sync {
        let divisions: Vec<&str> = Division::ALL
            .iter()
            .map(|division| division.label())
            .collect();
        let selected = Division::ALL
            .iter()
            .position(|division| *division == lfo.division)
            .unwrap_or(0);
        if let Some(chosen) = dropdown(ui, &THEME, ("division", index), &divisions, selected, 80.0)
        {
            lfo.division = Division::ALL[chosen];
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
                modulated: None,
            },
            &mut lfo.rate_hz,
        );
    }

    changed
}

/// The modulation matrix, as a table of sentences in a window of its own.
///
/// A row is hard to read as three unlabelled controls, so the table carries
/// column headings and an arrow between source and destination: the row says
/// "this source moves that destination by this much".
fn matrix_window(ui: &mut Ui, cell: &mut PerformanceCell) -> bool {
    let mut open = ui.memory(|memory| memory.data.get_temp(matrix_open_id()).unwrap_or(false));
    if !open {
        return false;
    }

    let mut changed = false;
    let sources: Vec<&str> = ModSource::ALL.iter().map(|source| source.label()).collect();
    let destinations: Vec<&str> = ModDestination::ALL
        .iter()
        .map(|destination| destination.label())
        .collect();

    egui::Window::new("Mod-Matrix")
        .id(egui::Id::new("mod-matrix"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_pos(pos2(420.0, 300.0))
        .show(ui.ctx(), |ui| {
            ui.spacing_mut().item_spacing = vec2(THEME.spacing_sm, THEME.spacing_sm);
            matrix_headings(ui);

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

                    arrow(ui);

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

                    // The bar is labelled by the heading above it.
                    changed |= value_slider(
                        ui,
                        &THEME,
                        SliderSpec {
                            label: "",
                            range: (-1.0, 1.0),
                            default: 1.0,
                            unit: Unit::Plain,
                            width: ROUTE_AMOUNT,
                        },
                        &mut route.amount,
                    );

                    if icon_button(ui, &THEME, Icon::Cross, "Diese Route entfernen") {
                        remove = Some(index);
                    }

                    hint_light(ui, route_summary(*route));
                });
            }

            if let Some(index) = remove {
                cell.remove_route(index);
                changed = true;
            }

            ui.add_space(THEME.spacing_sm);
            ui.horizontal(|ui| {
                if cell.routes.len() < MAX_ROUTES
                    && icon_button(ui, &THEME, Icon::Plus, "Route hinzufügen")
                {
                    cell.add_route(ModulationRoute::default());
                    changed = true;
                }
                hint_light(
                    ui,
                    &format!("{} von {MAX_ROUTES} Routen belegt", cell.routes.len()),
                );
            });

            // Volume is a route like any other, so it can be taken out. The
            // cell is then silent, which is worth saying rather than letting
            // the user hunt for a voice that never sounds.
            if !cell.routes.is_empty() && !cell.has_amplitude() {
                hint_light(
                    ui,
                    "Keine Route auf Volume — diese Zelle bleibt stumm. ENV A auf Volume stellt sie wieder her.",
                );
            }
        });

    ui.memory_mut(|memory| memory.data.insert_temp(matrix_open_id(), open));
    changed
}

/// The column headings above the matrix rows.
fn matrix_headings(ui: &mut Ui) {
    ui.horizontal(|ui| {
        for (width, caption) in [
            (ROUTE_SELECTOR, "QUELLE"),
            (ARROW_WIDTH, ""),
            (ROUTE_SELECTOR, "ZIEL"),
            (ROUTE_AMOUNT, "BETRAG"),
        ] {
            let (rect, _) =
                ui.allocate_exact_size(vec2(width, THEME.font_sm * 1.4), Sense::hover());
            if caption.is_empty() {
                continue;
            }
            ui.painter().text(
                rect.left_center(),
                Align2::LEFT_CENTER,
                caption,
                FontId::proportional(THEME.font_sm),
                THEME.text_dim,
            );
        }
    });
}

/// The arrow between a route's source and its destination.
fn arrow(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(
        vec2(ARROW_WIDTH, THEME.font_md + THEME.spacing_md * 2.0),
        Sense::hover(),
    );
    // Drawn rather than written: the interface font has no arrow glyph, and a
    // missing character renders as an empty box.
    let painter = ui.painter();
    let centre = rect.center();
    let half = ARROW_WIDTH * 0.3;
    painter.line_segment(
        [
            pos2(centre.x - half, centre.y),
            pos2(centre.x + half, centre.y),
        ],
        Stroke::new(THEME.stroke_thin, THEME.text_dim),
    );
    painter.add(PathShape::convex_polygon(
        vec![
            pos2(centre.x + half - 4.0, centre.y - 3.0),
            pos2(centre.x + half, centre.y),
            pos2(centre.x + half - 4.0, centre.y + 3.0),
        ],
        THEME.text_dim,
        Stroke::NONE,
    ));
}

/// What a route does, in words.
///
/// The amount alone does not say whether a source pushes one way or both, and
/// that is the part of the matrix people get wrong.
fn route_summary(route: ModulationRoute) -> &'static str {
    if route.amount == 0.0 {
        return "ohne Wirkung";
    }
    match (route.source.is_bipolar(), route.amount > 0.0) {
        (true, _) => "schwingt um den eingestellten Wert",
        (false, true) => "hebt den Wert an",
        (false, false) => "senkt den Wert ab",
    }
}
