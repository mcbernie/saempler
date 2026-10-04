use std::sync::{Arc, Mutex};

use nih_plug::prelude::{FloatParam, ParamSetter};
use nih_plug_egui::egui::{
    self, pos2, vec2, Align2, Color32, FontId, Frame, Margin, Rect, Sense, Shape, Stroke, Ui,
};
use nih_plug_egui::{resizable_window::ResizableWindow, EguiState};
use saempler_audio::{CellSpec, CommandProducer, EngineCommand, Meters, SampleBuffer, SliceBounds};
use saempler_core::PeakCache;
use saempler_model::ProjectFile;

use crate::screens::cell::cell_section;
use crate::screens::modifiers::modifier_section;
use crate::screens::performance::performance_section;
use crate::screens::source::source_section;
use crate::theme::Theme;
use crate::widgets::{
    inset, knob, metal_panel, panel_header, readout, stereo_meter, ViewRange, HEADER_HEIGHT,
};

pub(crate) const THEME: Theme = Theme::dark();

const KNOB_DIAMETER: f32 = 48.0;
const METER_WIDTH: f32 = 130.0;
/// Width of the readouts that say what is being triggered.
const TRIGGER_WIDTH: f32 = 130.0;
/// Share of the width the performance pads take.
///
/// The pads and the editor are side by side rather than on separate pages:
/// what is being edited and what is being played have to be visible at once.
const PERFORM_SHARE: f32 = 0.42;

/// Smallest the editor window may be dragged to.
///
/// The size the layout actually needs, not a guess: every band below has a
/// fixed height and the two columns fill what is left, so a window any smaller
/// could only clip a panel. Dragging the window bigger hands the extra height
/// to the pads and the editor, which both grow with their column.
pub const MIN_EDITOR_SIZE: (f32, f32) = (1_180.0, 1_000.0);

/// Height of the masthead strip.
const MASTHEAD_HEIGHT: f32 = 42.0;
/// Height of the source panel, waveform and toolbar together.
const SOURCE_HEIGHT: f32 = 172.0;
/// Height of the footer row holding the modifiers and the output strip.
const FOOTER_HEIGHT: f32 = 118.0;
/// Smallest the two middle columns may become.
const MIN_BODY_HEIGHT: f32 = 510.0;

/// What the editor keeps between frames.
///
/// Owned by the plugin, so it survives the window being closed and reopened.
/// The peaks drive the waveform overview; the buffer is only read when the
/// view is zoomed in past the resolution of the peak cache.
#[derive(Default)]
pub struct EditorState {
    pub peaks: PeakCache,
    /// The decoded audio, shared with the engine.
    pub buffer: Option<Arc<SampleBuffer>>,
    /// Section of the sample the waveform is showing.
    pub view: ViewRange,
    /// Message from the last import attempt, shown until the next one.
    pub status: Option<String>,
    /// Whether an import is running right now.
    pub loading: bool,
}

impl EditorState {
    /// Show the whole sample again.
    pub fn reset_view(&mut self) {
        self.view = ViewRange::full(self.peaks.frames());
    }
}

/// Everything the editor needs to draw a frame.
///
/// The fields are borrowed rather than owned so that the plugin keeps
/// ownership of its state and the interface stays a pure view over it.
pub struct ViewState<'a> {
    /// Persisted project state. Locked on the UI thread only.
    pub project: &'a Mutex<ProjectFile>,
    /// Peaks, audio, import status and the current page.
    pub sample: &'a Mutex<EditorState>,
    /// Producing end of the engine command queue. Locked on the UI thread
    /// only; the audio thread owns the consumer and never blocks on this.
    pub commands: &'a Mutex<CommandProducer>,
    /// Values published by the audio thread.
    pub meters: &'a Meters,
    /// Master output gain, exposed to the host as an automatable parameter.
    pub gain: &'a FloatParam,
    /// Window size, which the resize corner writes back to.
    pub editor_state: &'a EguiState,
}

impl ViewState<'_> {
    /// Queue a command for the engine.
    ///
    /// A full queue means the engine has not run since the last few hundred
    /// edits, which in practice only happens while audio is stopped. Dropping
    /// the command is preferable to blocking the interface.
    pub(crate) fn send(&self, command: EngineCommand) {
        if let Ok(mut producer) = self.commands.lock() {
            let _ = producer.push(command);
        }
    }
}

/// A one-shot audition of a region, with the cell's own settings left out.
pub(crate) fn preview_spec(start_frame: u64, end_frame: u64) -> CellSpec {
    CellSpec {
        bounds: SliceBounds {
            start_frame,
            end_frame,
        },
        ..CellSpec::default()
    }
}

/// Apply the product theme to egui's own surfaces.
pub fn apply_style(ctx: &egui::Context, theme: &Theme) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = theme.window_bg;
    visuals.extreme_bg_color = theme.control_pressed_bg;
    // Popups are the one surface egui draws the frame for; give it the panel
    // colour and the accent border the rest of the interface uses.
    visuals.window_fill = theme.panel_bg;
    visuals.window_stroke = egui::Stroke::new(theme.stroke_thin, theme.accent);
    visuals.popup_shadow = egui::epaint::Shadow {
        offset: [0, 4],
        blur: 12,
        spread: 0,
        color: egui::Color32::from_black_alpha(160),
    };
    visuals.override_text_color = Some(theme.text);
    visuals.resize_corner_size = 14.0;
    ctx.set_visuals(visuals);
}

/// Draw the whole editor. Returns true when the user asked to import a file.
pub fn draw(ctx: &egui::Context, setter: &ParamSetter, state: &ViewState<'_>) -> bool {
    apply_style(ctx, &THEME);
    let mut import_requested = false;

    // The playheads move with every processed block, so while something is
    // sounding the editor cannot wait for the next input event to redraw.
    if state.meters.any_playhead() {
        ctx.request_repaint();
    }

    ResizableWindow::new("saempler-window")
        .min_size(vec2(MIN_EDITOR_SIZE.0, MIN_EDITOR_SIZE.1))
        .show(ctx, state.editor_state, |ui| {
            Frame::new()
                .fill(THEME.window_bg)
                .inner_margin(THEME.spacing_md)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(THEME.spacing_md, THEME.spacing_md);

                    // Every band gets its rectangle up front rather than
                    // growing out of what came before. Laid out by flow, a
                    // panel that is a few points too tall pushes the ones
                    // after it off the window, and an empty project lays out
                    // differently from a full one. Given rectangles, the
                    // picture holds still whatever is loaded.
                    let full = ui.available_rect_before_wrap();
                    let gap = THEME.spacing_md;

                    let masthead = band(full, full.min.y, MASTHEAD_HEIGHT);
                    let source = band(full, masthead.max.y + gap, SOURCE_HEIGHT);
                    let footer =
                        Rect::from_min_max(pos2(full.min.x, full.max.y - FOOTER_HEIGHT), full.max);
                    let body = Rect::from_min_max(
                        pos2(full.min.x, source.max.y + gap),
                        pos2(
                            full.max.x,
                            (footer.min.y - gap).max(source.max.y + MIN_BODY_HEIGHT),
                        ),
                    );

                    region(ui, masthead, |ui| header(ui, state));
                    region(ui, source, |ui| {
                        import_requested = source_section(ui, state)
                    });

                    let (pads, editor) = split(body, PERFORM_SHARE, gap);
                    region(ui, pads, |ui| performance_section(ui, state));
                    region(ui, editor, |ui| cell_section(ui, state));

                    // The footer: modifier cards on the left, the output
                    // strip on the right, as the reference lays it out.
                    let (keys, output) = split(footer, 0.63, gap);
                    region(ui, keys, |ui| modifier_section(ui, state));
                    region(ui, output, |ui| footer_section(ui, setter, state));

                    crate::screens::effects::sends_window(ui, state);
                    crate::screens::about::window(ui);
                });
        });

    import_requested
}

/// The masthead: name plate on the left, tempo display on the right.
///
/// A metal strip like the panels below it, so the window reads as one chassis
/// rather than panels floating over a void.
fn header(ui: &mut Ui, state: &ViewState<'_>) {
    let background = ui.painter().add(Shape::Noop);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 42.0), Sense::hover());
    ui.painter().set(background, metal_panel(&THEME, rect));
    let painter = ui.painter();

    // The name plate: a dark block with the logo mark and the product name.
    let plate = egui::Rect::from_min_size(
        pos2(rect.min.x + 10.0, rect.min.y + 7.0),
        vec2(190.0, rect.height() - 14.0),
    );
    inset(painter, &THEME, plate, THEME.waveform_bg);
    logo_mark(painter, pos2(plate.min.x + 16.0, plate.center().y));
    let about_plate = plate;
    painter.text(
        pos2(plate.min.x + 32.0, plate.center().y - 4.0),
        Align2::LEFT_CENTER,
        "SÄMPLER",
        FontId::proportional(THEME.font_lg),
        THEME.accent,
    );
    painter.text(
        pos2(plate.min.x + 32.0, plate.max.y - 6.0),
        Align2::LEFT_CENTER,
        "SLICE & REMIX INSTRUMENT",
        FontId::proportional(7.0),
        THEME.text_dim,
    );

    // The tempo the engine is following, as a display cut into the metal.
    let bpm = egui::Rect::from_min_size(
        pos2(rect.max.x - 120.0, rect.min.y + 8.0),
        vec2(110.0, rect.height() - 16.0),
    );
    inset(painter, &THEME, bpm, THEME.waveform_bg);
    painter.text(
        pos2(bpm.min.x + 8.0, bpm.center().y),
        Align2::LEFT_CENTER,
        "BPM",
        FontId::proportional(THEME.font_sm),
        THEME.text_dim,
    );
    painter.text(
        pos2(bpm.max.x - 8.0, bpm.center().y),
        Align2::RIGHT_CENTER,
        format!("{:.2}", state.meters.tempo()),
        FontId::monospace(THEME.font_md),
        THEME.accent,
    );

    let subtitle = match state.project.lock() {
        Ok(project) => match project.project.sample.as_ref() {
            Some(sample) => format!(
                "{}  ·  {:.2} s  ·  {} Hz  ·  {} ch",
                sample.display_name(),
                sample.duration_seconds(),
                sample.sample_rate,
                sample.channels
            ),
            None => "kein Sample geladen".to_owned(),
        },
        Err(_) => String::new(),
    };
    painter.text(
        pos2(bpm.min.x - THEME.spacing_lg, rect.center().y),
        Align2::RIGHT_CENTER,
        subtitle,
        FontId::proportional(THEME.font_sm),
        THEME.title,
    );

    if crate::screens::about::name_plate_clicked(ui, about_plate) {
        crate::screens::about::request(ui);
    }
}

/// The little waveform glyph on the name plate.
pub(crate) fn logo_mark(painter: &egui::Painter, centre: egui::Pos2) {
    for (offset, height) in [(-6.0, 5.0), (-2.0, 9.0), (2.0, 7.0), (6.0, 4.0)] {
        painter.line_segment(
            [
                pos2(centre.x + offset, centre.y - height),
                pos2(centre.x + offset, centre.y + height),
            ],
            Stroke::new(2.0_f32, THEME.accent),
        );
    }
}

/// Output level, voice count and the modifier lamps.
///
/// Kept out of the tabs on purpose: while performing you need to see what the
/// modifiers are doing whichever page is open.
fn footer_section(ui: &mut Ui, setter: &ParamSetter, state: &ViewState<'_>) {
    let clipping = {
        let (left, right) = state.meters.peaks();
        (left.max(right) >= 1.0).then_some(THEME.danger)
    };
    section(ui, "OUTPUT", clipping.or(Some(THEME.active)), |ui| {
        ui.horizontal(|ui| {
            knob(ui, &THEME, state.gain, setter, KNOB_DIAMETER);
            ui.add_space(THEME.spacing_lg);

            ui.vertical(|ui| {
                stereo_meter(ui, &THEME, state.meters.peaks(), METER_WIDTH);
                readout(
                    ui,
                    &THEME,
                    "Voices",
                    &state.meters.active_voices().to_string(),
                    METER_WIDTH,
                );
            });

            ui.add_space(THEME.spacing_md);
            ui.vertical(|ui| {
                readout(ui, &THEME, "Spielt", &sounding_notes(state), TRIGGER_WIDTH);
                readout(ui, &THEME, "Slice", &sounding_slices(state), TRIGGER_WIDTH);
            });
        });
    });
}

/// Names of the notes currently sounding.
///
/// Worked out from the playheads the engine publishes rather than from a
/// separate message: a voice is inside the slice it plays, and the slice says
/// which key triggered it.
fn sounding_notes(state: &ViewState<'_>) -> String {
    let Ok(project) = state.project.lock() else {
        return String::new();
    };

    let mut names: Vec<String> = Vec::new();
    for frame in state.meters.playheads() {
        let Some(slice) = project.project.slice_at(frame) else {
            continue;
        };
        for cell in project.project.cells() {
            if cell.slice == slice.id {
                let name = saempler_model::note_name(cell.midi_note);
                if !names.contains(&name) {
                    names.push(name);
                }
                break;
            }
        }
    }

    if names.is_empty() {
        "—".to_owned()
    } else {
        names.join(" ")
    }
}

/// Numbers of the slices currently being read.
fn sounding_slices(state: &ViewState<'_>) -> String {
    let Ok(project) = state.project.lock() else {
        return String::new();
    };

    let mut numbers: Vec<String> = Vec::new();
    for frame in state.meters.playheads() {
        if let Some(index) = project
            .project
            .slices()
            .iter()
            .position(|slice| slice.contains(frame))
        {
            let label = format!("S{}", index + 1);
            if !numbers.contains(&label) {
                numbers.push(label);
            }
        }
    }

    if numbers.is_empty() {
        "—".to_owned()
    } else {
        numbers.join(" ")
    }
}

/// A full-width horizontal band of `full`, starting at `top`.
pub(crate) fn band(full: Rect, top: f32, height: f32) -> Rect {
    Rect::from_min_max(pos2(full.min.x, top), pos2(full.max.x, top + height))
}

/// Cut a rectangle into two columns with a gap between them.
fn split(rect: Rect, share: f32, gap: f32) -> (Rect, Rect) {
    let boundary = (rect.min.x + rect.width() * share).floor();
    (
        Rect::from_min_max(rect.min, pos2(boundary, rect.max.y)),
        Rect::from_min_max(pos2(boundary + gap, rect.min.y), rect.max),
    )
}

/// Draw into a given rectangle, laying contents out downwards.
///
/// The rectangle is both the room the contents get and the room they may use:
/// nothing inside can push a neighbour, which is what keeps the window from
/// rearranging itself as the project fills up.
pub(crate) fn region(ui: &mut Ui, rect: Rect, contents: impl FnOnce(&mut Ui)) {
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
        |ui| {
            ui.set_width(rect.width());
            contents(ui);
        },
    );
}

/// A group of controls on a panel of brushed metal.
///
/// `lit` is the colour of the panel's lamp, or `None` for a dark one. The
/// background is reserved before the contents and filled in afterwards,
/// because a panel is only as tall as what has been laid out on it.
pub(crate) fn section(
    ui: &mut Ui,
    title: &str,
    lit: Option<Color32>,
    contents: impl FnOnce(&mut Ui),
) {
    section_with(ui, title, lit, |_| {}, contents);
}

/// A section whose header row also carries controls, to the right of the
/// legend. That is how the source panel fits its whole toolbar without a row
/// of its own.
pub(crate) fn section_with(
    ui: &mut Ui,
    title: &str,
    lit: Option<Color32>,
    header: impl FnOnce(&mut Ui),
    contents: impl FnOnce(&mut Ui),
) {
    let background = ui.painter().add(Shape::Noop);

    let panel = Frame::new()
        .inner_margin(Margin {
            left: THEME.spacing_lg as i8,
            right: THEME.spacing_lg as i8,
            top: THEME.spacing_sm as i8,
            bottom: THEME.spacing_md as i8,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // The plate covers the whole region it was given, whatever its
            // contents measure. Otherwise an empty project draws short panels
            // and a full one draws tall ones, and the window never settles.
            ui.set_min_height(ui.available_height());

            let (rect, _) = ui.allocate_exact_size(
                vec2(ui.available_width(), HEADER_HEIGHT + 8.0),
                Sense::hover(),
            );
            panel_header(ui, &THEME, rect, title, lit);

            // Whatever the caller wants beside the legend, from where the
            // title ends to the right edge.
            let controls = egui::Rect::from_min_max(
                pos2(
                    rect.min.x + 34.0 + title.chars().count() as f32 * THEME.font_md * 0.72,
                    rect.min.y,
                ),
                rect.max,
            );
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(controls)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
                header,
            );

            contents(ui);
        });

    ui.painter()
        .set(background, metal_panel(&THEME, panel.response.rect));
}

/// A dimmed line of explanatory text.
///
/// Sized to the text rather than to the available width, so a hint placed in a
/// row leaves room for what follows it.
pub(crate) fn hint(ui: &mut Ui, text: &str) {
    let width = ui.fonts(|fonts| {
        fonts
            .layout_no_wrap(
                text.to_owned(),
                FontId::proportional(THEME.font_sm),
                THEME.text_dim,
            )
            .size()
            .x
    });
    let (rect, _) = ui.allocate_exact_size(
        vec2(width.min(ui.available_width()), THEME.font_sm * 1.7),
        Sense::hover(),
    );
    ui.painter().text(
        rect.left_center(),
        Align2::LEFT_CENTER,
        text,
        FontId::proportional(THEME.font_sm),
        THEME.title.gamma_multiply(0.75),
    );
}

/// A dimmed line of explanatory text on a dark surface.
///
/// The panels are light metal and [`hint`] is dark to suit them; inside the
/// editor windows the surface is dark again and the same text would vanish.
pub(crate) fn hint_light(ui: &mut Ui, text: &str) {
    let width = ui.fonts(|fonts| {
        fonts
            .layout_no_wrap(
                text.to_owned(),
                FontId::proportional(THEME.font_sm),
                THEME.text_dim,
            )
            .size()
            .x
    });
    let (rect, _) = ui.allocate_exact_size(
        vec2(width.min(ui.available_width()), THEME.font_sm * 1.7),
        Sense::hover(),
    );
    ui.painter().text(
        rect.left_center(),
        Align2::LEFT_CENTER,
        text,
        FontId::proportional(THEME.font_sm),
        THEME.text_dim,
    );
}

/// Text standing in for a page that has nothing to show yet.
pub(crate) fn placeholder(ui: &mut Ui, text: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 72.0), Sense::hover());
    inset(ui.painter(), &THEME, rect, THEME.waveform_bg);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(THEME.font_md),
        THEME.text_dim,
    );
}
