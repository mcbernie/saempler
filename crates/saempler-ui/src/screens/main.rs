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
    header_rule, inset, knob, legend, metal_panel, panel_header, readout, screw, stereo_meter,
    textures, ViewRange, HEADER_HEIGHT,
};

pub(crate) const THEME: Theme = Theme::ivory();

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
pub const MIN_EDITOR_SIZE: (f32, f32) = (1_180.0, 1_050.0);

/// Height of the masthead strip.
const MASTHEAD_HEIGHT: f32 = 42.0;
/// Height of the source panel, waveform and toolbar together.
const SOURCE_HEIGHT: f32 = 178.0;
/// Height of the footer row holding the modifiers and the output strip.
const FOOTER_HEIGHT: f32 = 138.0;
/// Room between the top edge of a plate and its header row.
const PANEL_TOP: f32 = 10.0;
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
    let mut visuals = egui::Visuals::light();
    visuals.panel_fill = theme.window_bg;
    visuals.extreme_bg_color = theme.control_pressed_bg;
    // Windows and open lists are plates like the panels: ivory, a fine
    // engraved edge and a short soft shadow. The editor windows draw the
    // textured plate themselves (see `plate_window`); this is the fallback
    // and what the lists open on.
    visuals.window_fill = theme.chassis_mid;
    visuals.window_stroke = egui::Stroke::new(theme.stroke_thin, theme.chassis_shadow);
    visuals.window_corner_radius = theme.radius_md;
    visuals.window_highlight_topmost = false;
    let shadow = egui::epaint::Shadow {
        offset: [2, 5],
        blur: 16,
        spread: 0,
        color: egui::Color32::from_black_alpha(70),
    };
    visuals.window_shadow = shadow;
    visuals.popup_shadow = shadow;
    visuals.menu_corner_radius = theme.radius_md;
    visuals.override_text_color = Some(theme.title);
    visuals.hyperlink_color = theme.control_selected_bg;
    // The close cross and the line under a window's title.
    visuals.widgets.noninteractive.bg_stroke =
        egui::Stroke::new(theme.stroke_thin, theme.chassis_shadow.gamma_multiply(0.6));
    for widget in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
    ] {
        widget.fg_stroke.color = theme.title;
    }
    visuals.resize_corner_size = 14.0;
    ctx.set_visuals(visuals);
}

/// Show an editor window as a plate of the instrument.
///
/// The window gets the same textured front plate the panels are made of,
/// reserved before its contents and filled in once egui knows how big the
/// window came out. Its title is printed on the plate like a panel legend.
pub(crate) fn plate_window<R>(
    window: egui::Window<'_>,
    ctx: &egui::Context,
    contents: impl FnOnce(&mut Ui) -> R,
) -> Option<R> {
    let frame = Frame::window(&ctx.style())
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::NONE)
        .inner_margin(Margin::same(THEME.spacing_lg as i8));
    let shown = window.frame(frame).show(ctx, |ui| {
        let background = ui.painter().add(Shape::Noop);
        (background, contents(ui))
    })?;
    let (background, inner) = shown.inner?;
    ctx.layer_painter(shown.response.layer_id)
        .set(background, metal_panel(&textures(ctx), shown.response.rect));
    Some(inner)
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
    ui.painter()
        .set(background, metal_panel(&textures(ui.ctx()), rect));
    let painter = ui.painter();
    let textures = textures(ui.ctx());
    screw(painter, &textures, pos2(rect.min.x + 14.0, rect.center().y));
    screw(painter, &textures, pos2(rect.max.x - 14.0, rect.center().y));

    // The name, printed large and heavy on the plate like a module's maker
    // mark, and what it is beside it.
    let name_at = pos2(rect.min.x + 30.0, rect.center().y);
    let mut name_end = name_at.x;
    for (offset, color) in [
        (vec2(0.0, 1.0), THEME.chassis_top),
        (vec2(0.0, 0.0), THEME.title),
        (vec2(0.6, 0.0), THEME.title),
    ] {
        let drawn = painter.text(
            name_at + offset,
            Align2::LEFT_CENTER,
            "SÄMPLER",
            FontId::proportional(26.0),
            color,
        );
        name_end = name_end.max(drawn.max.x);
    }
    let about_plate = egui::Rect::from_min_max(
        pos2(name_at.x, rect.min.y + 4.0),
        pos2(name_end, rect.max.y - 4.0),
    );
    painter.text(
        pos2(name_end + THEME.spacing_lg, rect.center().y + 2.0),
        Align2::LEFT_CENTER,
        "SLICE & REMIX INSTRUMENT",
        FontId::proportional(THEME.font_sm),
        THEME.label,
    );

    // The tempo the engine is following, on a display set into the plate.
    let bpm = egui::Rect::from_min_size(
        pos2(rect.max.x - 146.0, rect.min.y + 7.0),
        vec2(118.0, rect.height() - 14.0),
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
        FontId::monospace(THEME.font_lg - 2.0),
        THEME.accent,
    );

    // What is loaded, on a display of its own left of the tempo.
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
    let font = FontId::proportional(THEME.font_sm + 1.0);
    let width = ui
        .fonts(|fonts| fonts.layout_no_wrap(subtitle.clone(), font.clone(), THEME.text))
        .size()
        .x;
    let info = egui::Rect::from_min_max(
        pos2(bpm.min.x - THEME.spacing_md - width - 24.0, bpm.min.y),
        pos2(bpm.min.x - THEME.spacing_md, bpm.max.y),
    );
    let painter = ui.painter();
    inset(painter, &THEME, info, THEME.waveform_bg);
    painter.text(
        info.center(),
        Align2::CENTER_CENTER,
        subtitle,
        font,
        THEME.text,
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
        ui.add_space(THEME.spacing_sm);
        ui.horizontal(|ui| {
            ui.add_space(THEME.spacing_sm);
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

/// A row of controls of one height, each centred on the row's middle line,
/// so knobs, switches and lists of different heights line up.
pub(crate) fn row<R>(ui: &mut Ui, height: f32, contents: impl FnOnce(&mut Ui) -> R) -> R {
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), height),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_height(height);
            contents(ui)
        },
    )
    .inner
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
            // Room above the header for the milled edge, so the keys in
            // the header row do not sit against it.
            top: PANEL_TOP as i8,
            bottom: THEME.spacing_md as i8,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // The plate covers the whole region it was given, whatever its
            // contents measure. Otherwise an empty project draws short panels
            // and a full one draws tall ones, and the window never settles.
            ui.set_min_height(ui.available_height());

            let (rect, _) = ui.allocate_exact_size(
                vec2(ui.available_width(), HEADER_HEIGHT + 4.0),
                Sense::hover(),
            );
            let title_end = panel_header(ui, &THEME, rect, title);

            // Whatever the caller wants beside the legend, from where the
            // title ends to the right edge; the engraved rule fills whatever
            // the controls leave over.
            let controls =
                egui::Rect::from_min_max(pos2(title_end + THEME.spacing_lg, rect.min.y), rect.max);
            let used = ui
                .scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(controls)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    header,
                )
                .response
                .rect;
            let rule_from = if used.width() > 0.0 {
                used.max.x
            } else {
                title_end
            };
            header_rule(
                ui.painter(),
                &THEME,
                rect,
                rule_from + THEME.spacing_lg,
                lit,
            );

            contents(ui);
        });

    ui.painter().set(
        background,
        metal_panel(&textures(ui.ctx()), panel.response.rect),
    );
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

/// A small heading printed on a plate: capitals, heavy.
pub(crate) fn heading(ui: &mut Ui, text: &str) {
    let width = ui.fonts(|fonts| {
        fonts
            .layout_no_wrap(
                text.to_uppercase(),
                FontId::proportional(THEME.font_sm),
                THEME.label,
            )
            .size()
            .x
    }) + 1.0;
    let (rect, _) = ui.allocate_exact_size(
        vec2(width.min(ui.available_width()), THEME.font_sm * 1.7),
        Sense::hover(),
    );
    legend(
        ui.painter(),
        &THEME,
        rect.left_center(),
        Align2::LEFT_CENTER,
        text,
    );
}

/// Text standing in for a page that has nothing to show yet.
///
/// Printed straight on the plate, centred over the room the contents would
/// take: what is missing, and below it how to get it.
pub(crate) fn placeholder(ui: &mut Ui, text: &str, detail: &str) {
    ui.add_space(THEME.spacing_sm);
    let size = vec2(ui.available_width(), ui.available_height().max(72.0));
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter();
    painter.text(
        rect.center() - vec2(0.0, THEME.font_md * 0.6),
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(THEME.font_md + 1.0),
        THEME.title.gamma_multiply(0.7),
    );
    painter.text(
        rect.center() + vec2(0.0, THEME.font_md * 0.7),
        Align2::CENTER_CENTER,
        detail,
        FontId::proportional(THEME.font_sm),
        THEME.title.gamma_multiply(0.5),
    );
}
