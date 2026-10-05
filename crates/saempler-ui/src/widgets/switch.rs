use nih_plug_egui::egui::{pos2, vec2, Align2, Color32, CursorIcon, FontId, Rect, Sense, Ui};

use crate::theme::Theme;
use crate::widgets::panel::{lamp, legend};
use crate::widgets::texture::{self, size_of};

/// Width the switch texture is drawn at; its height follows the texture.
const SWITCH_WIDTH: f32 = 34.0;
/// Width of the hex nut as a share of the texture's width.
const NUT_WIDTH: f32 = 0.94;
/// Where the hex nut starts and ends, as shares of the texture's height.
/// Everything above or below it is lever.
const NUT_TOP: f32 = 0.23;
const NUT_BOTTOM: f32 = 0.915;
/// Room for the ON and OFF legends beside the switch.
const LEGEND_WIDTH: f32 = 22.0;
/// Room for the lamp on the other side.
const LAMP_ROOM: f32 = 14.0;

/// A nickel toggle switch, lever up for on.
///
/// The caption sits above it, ON and OFF are printed beside the two lever
/// positions, and a lamp says the same thing again: a lever's angle is easy to
/// misread at a glance, a lit lamp is not.
///
/// Only the hex nut takes room in the layout. The lever stands off the plate
/// the way a real one does, so thrown up it reaches over the caption above
/// it, and thrown down a little way past the row.
///
/// Returns true when it was clicked this frame; the caller owns the value.
pub fn toggle_switch(ui: &mut Ui, theme: &Theme, caption: &str, on: bool) -> bool {
    let textures = texture::textures(ui.ctx());
    let lever = if on {
        &textures.switch_up
    } else {
        &textures.switch_down
    };
    let natural = size_of(lever);
    let drawn = vec2(SWITCH_WIDTH, natural.y * SWITCH_WIDTH / natural.x);
    let nut = vec2(SWITCH_WIDTH * NUT_WIDTH, drawn.y * (NUT_BOTTOM - NUT_TOP));
    let caption_height = theme.font_sm + theme.spacing_sm;
    let size = vec2(LAMP_ROOM + nut.x + LEGEND_WIDTH, caption_height + nut.y);

    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let response = response.on_hover_cursor(CursorIcon::PointingHand);
    let painter = ui.painter();

    legend(
        painter,
        theme,
        pos2(rect.min.x + LAMP_ROOM + nut.x * 0.5, rect.min.y),
        Align2::CENTER_TOP,
        caption,
    );

    let nut_rect = Rect::from_min_size(
        pos2(rect.min.x + LAMP_ROOM, rect.min.y + caption_height),
        nut,
    );
    let switch = Rect::from_min_size(
        pos2(
            nut_rect.center().x - drawn.x * 0.5,
            nut_rect.min.y - drawn.y * NUT_TOP,
        ),
        drawn,
    );

    // The legends belong to the positions, not to the state: both are
    // always printed, and the one the lever points at is the dark one.
    let legend_x = nut_rect.max.x + 3.0;
    let dim = theme.chassis_shadow.gamma_multiply(0.75);
    for (text, y, active) in [
        ("ON", nut_rect.min.y + nut.y * 0.22, on),
        ("OFF", nut_rect.max.y - nut.y * 0.22, !on),
    ] {
        painter.text(
            pos2(legend_x, y),
            Align2::LEFT_CENTER,
            text,
            FontId::proportional(theme.font_sm - 2.0),
            if active { theme.label } else { dim },
        );
    }

    lamp(
        painter,
        theme,
        pos2(rect.min.x + LAMP_ROOM * 0.45, nut_rect.center().y),
        on.then_some(theme.active),
    );

    // Last, so the lever lies over whatever it reaches across, with its own
    // shadow thrown down and to the right.
    painter.add(texture::image(
        lever,
        switch.translate(vec2(1.0, 2.0)),
        Color32::from_black_alpha(70),
    ));
    painter.add(texture::image(lever, switch, Color32::WHITE));

    response.clicked()
}
