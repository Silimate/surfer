//! Code related to the mouse gesture handling.
use egui::{Context, Painter, PointerButton, Response, RichText, Sense, Window};
use emath::{Align2, Pos2, Rect, RectTransform, Vec2};
use epaint::{FontId, Stroke};

use crate::config::SurferTheme;
use crate::time::time_string;
use crate::view::DrawingContext;
use crate::{wave_data::WaveData, Message, SystemState};

/// The supported mouse gesture operations.
#[derive(Clone, PartialEq, Copy)]
enum GestureKind {
    ZoomToFit,
    ZoomIn,
    ZoomOut,
    GoToEnd,
    GoToStart,
}

impl SystemState {
    /// Draw the mouse gesture widget, i.e., the line(s) and text showing which gesture is being drawn.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_mouse_gesture_widget(
        &self,
        egui_ctx: &egui::Context,
        waves: &WaveData,
        pointer_pos_canvas: Option<Pos2>,
        response: &Response,
        msgs: &mut Vec<Message>,
        ctx: &mut DrawingContext,
        viewport_idx: usize,
    ) {
        if let Some(start_location) = self.gesture_start_location {
            let modifiers = egui_ctx.input(|i| i.modifiers);
            if response.dragged_by(PointerButton::Middle)
                || modifiers.command && response.dragged_by(PointerButton::Primary)
            {
                self.start_dragging(
                    pointer_pos_canvas,
                    start_location,
                    ctx,
                    response,
                    waves,
                    viewport_idx,
                );
            }

            if response.drag_stopped_by(PointerButton::Middle)
                || modifiers.command && response.drag_stopped_by(PointerButton::Primary)
            {
                let frame_width = response.rect.width();
                self.stop_dragging(
                    pointer_pos_canvas,
                    start_location,
                    msgs,
                    viewport_idx,
                    waves,
                    frame_width,
                );
            }
        }
    }

    fn stop_dragging(
        &self,
        pointer_pos_canvas: Option<Pos2>,
        start_location: Pos2,
        msgs: &mut Vec<Message>,
        viewport_idx: usize,
        waves: &WaveData,
        frame_width: f32,
    ) {
        let num_timestamps = waves.num_timestamps().unwrap_or(1.into());
        let end_location = pointer_pos_canvas.unwrap();
        let distance = end_location - start_location;
        if distance.length_sq() >= self.user.config.gesture.deadzone {
            match gesture_type(distance) {
                Some(GestureKind::ZoomToFit) => {
                    msgs.push(Message::ZoomToFit { viewport_idx });
                }
                Some(GestureKind::ZoomIn) => {
                    let (minx, maxx) = if end_location.x < start_location.x {
                        (end_location.x, start_location.x)
                    } else {
                        (start_location.x, end_location.x)
                    };
                    msgs.push(Message::ZoomToRange {
                        // FIXME: No need to go via bigint here, this could all be relative
                        start: waves.viewports[viewport_idx].as_time_bigint(
                            minx,
                            frame_width,
                            &num_timestamps,
                        ),
                        end: waves.viewports[viewport_idx].as_time_bigint(
                            maxx,
                            frame_width,
                            &num_timestamps,
                        ),
                        viewport_idx,
                    });
                }
                Some(GestureKind::GoToStart) => {
                    msgs.push(Message::GoToStart { viewport_idx });
                }
                Some(GestureKind::GoToEnd) => {
                    msgs.push(Message::GoToEnd { viewport_idx });
                }
                Some(GestureKind::ZoomOut) => {
                    msgs.push(Message::CanvasZoom {
                        mouse_ptr: None,
                        delta: 2.0,
                        viewport_idx,
                    });
                }
                _ => {}
            }
        }
        msgs.push(Message::SetDragStart(None));
    }

    fn start_dragging(
        &self,
        pointer_pos_canvas: Option<Pos2>,
        start_location: Pos2,
        ctx: &mut DrawingContext<'_>,
        response: &Response,
        waves: &WaveData,
        viewport_idx: usize,
    ) {
        let current_location = pointer_pos_canvas.unwrap();
        let distance = current_location - start_location;
        if distance.length_sq() >= self.user.config.gesture.deadzone {
            match gesture_type(distance) {
                Some(GestureKind::ZoomToFit) => self.draw_gesture_line(
                    start_location,
                    current_location,
                    "Zoom to fit",
                    true,
                    ctx,
                ),
                Some(GestureKind::ZoomIn) => self.draw_zoom_in_gesture(
                    start_location,
                    current_location,
                    response,
                    ctx,
                    waves,
                    viewport_idx,
                ),

                Some(GestureKind::GoToStart) => self.draw_gesture_line(
                    start_location,
                    current_location,
                    "Go to start",
                    true,
                    ctx,
                ),
                Some(GestureKind::GoToEnd) => {
                    self.draw_gesture_line(start_location, current_location, "Go to end", true, ctx)
                }
                Some(GestureKind::ZoomOut) => {
                    self.draw_gesture_line(start_location, current_location, "Zoom out", true, ctx)
                }
                _ => self.draw_gesture_line(start_location, current_location, "Cancel", false, ctx),
            }
        } else {
            self.draw_gesture_help(response, ctx.painter, Some(start_location), true);
        }
    }

    /// Draw the line used by most mouse gestures.
    fn draw_gesture_line(
        &self,
        start: Pos2,
        end: Pos2,
        text: &str,
        active: bool,
        ctx: &mut DrawingContext,
    ) {
        let stroke = Stroke {
            color: if active {
                self.user.config.gesture.style.color
            } else {
                self.user.config.gesture.style.color.gamma_multiply(0.3)
            },
            width: self.user.config.gesture.style.width,
        };
        ctx.painter.line_segment(
            [
                (ctx.to_screen)(end.x, end.y),
                (ctx.to_screen)(start.x, start.y),
            ],
            stroke,
        );
        draw_gesture_text(
            ctx,
            (ctx.to_screen)(end.x, end.y),
            text.to_string(),
            &self.user.config.theme,
        );
    }

    /// Draw the lines used for the zoom-in gesture.
    fn draw_zoom_in_gesture(
        &self,
        start_location: Pos2,
        current_location: Pos2,
        response: &Response,
        ctx: &mut DrawingContext<'_>,
        waves: &WaveData,
        viewport_idx: usize,
    ) {
        let stroke = Stroke {
            color: self.user.config.gesture.style.color,
            width: self.user.config.gesture.style.width,
        };
        let height = response.rect.height();
        let width = response.rect.width();
        ctx.painter.line_segment(
            [
                (ctx.to_screen)(start_location.x, 0.0),
                (ctx.to_screen)(start_location.x, height),
            ],
            stroke,
        );
        ctx.painter.line_segment(
            [
                (ctx.to_screen)(current_location.x, 0.0),
                (ctx.to_screen)(current_location.x, height),
            ],
            stroke,
        );
        ctx.painter.line_segment(
            [
                (ctx.to_screen)(start_location.x, start_location.y),
                (ctx.to_screen)(current_location.x, start_location.y),
            ],
            stroke,
        );
        let (minx, maxx) = if current_location.x < start_location.x {
            (current_location.x, start_location.x)
        } else {
            (start_location.x, current_location.x)
        };
        let num_timestamps = waves.num_timestamps().unwrap_or(1.into());
        let start_time = waves.viewports[viewport_idx].as_time_bigint(minx, width, &num_timestamps);
        let end_time = waves.viewports[viewport_idx].as_time_bigint(maxx, width, &num_timestamps);
        let diff_time = &end_time - &start_time;
        let timescale = &waves.inner.metadata().timescale;
        draw_gesture_text(
            ctx,
            (ctx.to_screen)(current_location.x, current_location.y),
            format!(
                "Zoom in: {} ({} to {})",
                time_string(
                    &diff_time,
                    timescale,
                    &self.user.wanted_timeunit,
                    &self.get_time_format()
                ),
                time_string(
                    &start_time,
                    timescale,
                    &self.user.wanted_timeunit,
                    &self.get_time_format()
                ),
                time_string(
                    &end_time,
                    timescale,
                    &self.user.wanted_timeunit,
                    &self.get_time_format()
                ),
            ),
            &self.user.config.theme,
        );
    }

    /// Draw the mouse gesture help window.
    pub fn mouse_gesture_help(&self, ctx: &Context, msgs: &mut Vec<Message>) {
        let mut open = true;
        Window::new("Mouse gestures")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new(
                        "Press middle mouse button (or ctrl+primary mouse button) and drag",
                    ));
                    ui.add_space(20.);
                    let (response, painter) =
                        ui.allocate_painter(Vec2 { x: 300.0, y: 300.0 }, Sense::click());
                    self.draw_gesture_help(&response, &painter, None, false);
                    ui.add_space(10.);
                    ui.separator();
                    if ui.button("Close").clicked() {
                        msgs.push(Message::SetGestureHelpVisible(false));
                    }
                });
            });
        if !open {
            msgs.push(Message::SetGestureHelpVisible(false));
        }
    }

    /// Draw the "compass" showing the boundaries for different gestures.
    fn draw_gesture_help(
        &self,
        response: &Response,
        painter: &Painter,
        midpoint: Option<Pos2>,
        draw_bg: bool,
    ) {
        // Compute sizes and coordinates
        let tan225 = 0.41421357;
        let (midx, midy, deltax, deltay) = if let Some(midpoint) = midpoint {
            let halfsize = self.user.config.gesture.size * 0.5;
            (midpoint.x, midpoint.y, halfsize, halfsize)
        } else {
            let halfwidth = response.rect.width() * 0.5;
            let halfheight = response.rect.height() * 0.5;
            (halfwidth, halfheight, halfwidth, halfheight)
        };

        let container_rect = Rect::from_min_size(Pos2::ZERO, response.rect.size());
        let to_screen = &|x, y| {
            RectTransform::from_to(container_rect, response.rect)
                .transform_pos(Pos2::new(x, y) + Vec2::new(0.5, 0.5))
        };
        let stroke = Stroke {
            color: self.user.config.gesture.style.color,
            width: self.user.config.gesture.style.width,
        };
        let tan225deltax = tan225 * deltax;
        let tan225deltay = tan225 * deltay;
        let left = midx - deltax;
        let right = midx + deltax;
        let top = midy - deltay;
        let bottom = midy + deltay;
        let bg_radius = 1.35 * deltax;
        // Draw background
        if draw_bg {
            painter.circle_filled(
                to_screen(midx, midy),
                bg_radius,
                self.user
                    .config
                    .theme
                    .canvas_colors
                    .background
                    .gamma_multiply(0.75),
            );
        }
        // Draw lines
        painter.line_segment(
            [
                to_screen(left, midy + tan225deltax),
                to_screen(right, midy - tan225deltax),
            ],
            stroke,
        );
        painter.line_segment(
            [
                to_screen(left, midy - tan225deltax),
                to_screen(right, midy + tan225deltax),
            ],
            stroke,
        );
        painter.line_segment(
            [
                to_screen(midx + tan225deltay, top),
                to_screen(midx - tan225deltay, bottom),
            ],
            stroke,
        );
        painter.line_segment(
            [
                to_screen(midx - tan225deltay, top),
                to_screen(midx + tan225deltay, bottom),
            ],
            stroke,
        );

        let halfwaytexty_upper = top + (deltay - tan225deltax) * 0.5;
        let halfwaytexty_lower = bottom - (deltay - tan225deltax) * 0.5;
        // Draw commands
        painter.text(
            to_screen(left, midy),
            Align2::LEFT_CENTER,
            "Zoom in",
            FontId::default(),
            self.user.config.theme.foreground,
        );
        painter.text(
            to_screen(right, midy),
            Align2::RIGHT_CENTER,
            "Zoom in",
            FontId::default(),
            self.user.config.theme.foreground,
        );
        painter.text(
            to_screen(left, halfwaytexty_upper),
            Align2::LEFT_CENTER,
            "Zoom to fit",
            FontId::default(),
            self.user.config.theme.foreground,
        );
        painter.text(
            to_screen(right, halfwaytexty_upper),
            Align2::RIGHT_CENTER,
            "Zoom out",
            FontId::default(),
            self.user.config.theme.foreground,
        );
        painter.text(
            to_screen(midx, top),
            Align2::CENTER_TOP,
            "Cancel",
            FontId::default(),
            self.user.config.theme.foreground,
        );
        painter.text(
            to_screen(left, halfwaytexty_lower),
            Align2::LEFT_CENTER,
            "Go to start",
            FontId::default(),
            self.user.config.theme.foreground,
        );
        painter.text(
            to_screen(right, halfwaytexty_lower),
            Align2::RIGHT_CENTER,
            "Go to end",
            FontId::default(),
            self.user.config.theme.foreground,
        );
        painter.text(
            to_screen(midx, bottom),
            Align2::CENTER_BOTTOM,
            "Cancel",
            FontId::default(),
            self.user.config.theme.foreground,
        );
    }
}

/// Determine which mouse gesture ([`GestureKind`]) is currently drawn.
fn gesture_type(delta: Vec2) -> Option<GestureKind> {
    let tan225 = 0.41421357;
    let tan225x = tan225 * delta.x;
    let tan225y = tan225 * delta.y;
    if delta.x < 0.0 {
        if delta.y.abs() < -tan225x {
            // West
            Some(GestureKind::ZoomIn)
        } else if delta.y < 0.0 && delta.x < tan225y {
            // North west
            Some(GestureKind::ZoomToFit)
        } else if delta.y > 0.0 && delta.x < -tan225y {
            // South west
            Some(GestureKind::GoToStart)
        // } else if delta.y < 0.0 {
        //    // North
        //    None
        } else {
            // South
            None
        }
    } else if tan225x > delta.y.abs() {
        // East
        Some(GestureKind::ZoomIn)
    } else if delta.y < 0.0 && delta.x > -tan225y {
        // North east
        Some(GestureKind::ZoomOut)
    } else if delta.y > 0.0 && delta.x > tan225y {
        // South east
        Some(GestureKind::GoToEnd)
    // } else if delta.y > 0.0 {
    //    // North
    //    None
    } else {
        // South
        None
    }
}

fn draw_gesture_text(
    ctx: &mut DrawingContext,
    pos: Pos2,
    text: impl ToString,
    theme: &SurferTheme,
) {
    // Translate away from the mouse cursor so the text isn't hidden by it
    let pos = pos + Vec2::new(10.0, -10.0);

    let galley = ctx
        .painter
        .layout_no_wrap(text.to_string(), FontId::default(), theme.foreground);

    ctx.painter.rect(
        galley.rect.translate(pos.to_vec2()).expand(3.0),
        2.0,
        theme.primary_ui_color.background,
        Stroke::default(),
        egui::StrokeKind::Inside,
    );

    ctx.painter
        .galley(pos, galley, theme.primary_ui_color.foreground);
}
