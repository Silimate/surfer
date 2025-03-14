use egui::{Context, Grid, RichText, WidgetText, Window};
use emath::{Align2, Pos2, Rect, Vec2};
use epaint::{CornerRadius, FontId, Stroke};
use itertools::Itertools;
use num::BigInt;

use crate::SystemState;
use crate::{
    config::SurferTheme,
    displayed_item::{DisplayedItem, DisplayedMarker},
    message::Message,
    time::time_string,
    view::{DrawingContext, ItemDrawingInfo},
    viewport::Viewport,
    wave_data::WaveData,
};

pub const DEFAULT_MARKER_NAME: &str = "Marker";

impl WaveData {
    pub fn draw_cursor(
        &self,
        theme: &SurferTheme,
        ctx: &mut DrawingContext,
        size: Vec2,
        viewport: &Viewport,
    ) {
        if let Some(marker) = &self.cursor {
            let num_timestamps = self.num_timestamps().unwrap_or(1.into());
            let x = viewport.pixel_from_time(marker, size.x, &num_timestamps);

            let stroke = Stroke {
                color: theme.cursor.color,
                width: theme.cursor.width,
            };
            ctx.painter.line_segment(
                [
                    (ctx.to_screen)(x + 0.5, -0.5),
                    (ctx.to_screen)(x + 0.5, size.y),
                ],
                stroke,
            );
        }
    }

    pub fn draw_markers(
        &self,
        theme: &SurferTheme,
        ctx: &mut DrawingContext,
        size: Vec2,
        viewport: &Viewport,
    ) {
        let num_timestamps = self.num_timestamps().unwrap_or(1.into());
        for (idx, marker) in &self.markers {
            let color = self
                .items_tree
                .iter()
                .map(|node| self.displayed_items.get(&node.item_ref))
                .find_map(|item| match item {
                    Some(DisplayedItem::Marker(tmp_marker)) => {
                        if *idx == tmp_marker.idx {
                            Some(tmp_marker)
                        } else {
                            None
                        }
                    }
                    _ => None,
                })
                .and_then(|displayed_maker| displayed_maker.color.clone())
                .and_then(|color| theme.get_color(&color))
                .unwrap_or(&theme.cursor.color);
            let stroke = Stroke {
                color: *color,
                width: theme.cursor.width,
            };
            let x = viewport.pixel_from_time(marker, size.x, &num_timestamps);
            ctx.painter.line_segment(
                [
                    (ctx.to_screen)(x + 0.5, -0.5),
                    (ctx.to_screen)(x + 0.5, size.y),
                ],
                stroke,
            );
        }
    }

    pub fn can_add_marker(&self) -> bool {
        self.markers.len() < 255
    }

    pub fn add_marker(&mut self, location: &BigInt, name: Option<String>, move_focus: bool) {
        if !self.can_add_marker() {
            return;
        }

        let idx = (0..=254)
            .find(|idx| !self.markers.contains_key(idx))
            .unwrap();

        self.insert_item(
            DisplayedItem::Marker(DisplayedMarker {
                color: None,
                background_color: None,
                name,
                idx,
            }),
            None,
            move_focus,
        );
        self.markers.insert(idx, location.clone());
    }

    pub fn remove_marker(&mut self, idx: u8) {
        if let Some(&marker_item_ref) =
            self.displayed_items
                .iter()
                .find_map(|(id, item)| match item {
                    DisplayedItem::Marker(marker) if marker.idx == idx => Some(id),
                    _ => None,
                })
        {
            self.remove_displayed_item(marker_item_ref);
        }
    }

    /// Set the marker with the specified id to the location. If the marker doesn't exist already,
    /// it will be created
    pub fn set_marker_position(&mut self, idx: u8, location: &BigInt) {
        if !self.markers.contains_key(&idx) {
            self.insert_item(
                DisplayedItem::Marker(DisplayedMarker {
                    color: None,
                    background_color: None,
                    name: None,
                    idx,
                }),
                None,
                true,
            );
        }
        self.markers.insert(idx, location.clone());
    }

    pub fn move_marker_to_cursor(&mut self, idx: u8) {
        let Some(location) = self.cursor.clone() else {
            return;
        };
        self.set_marker_position(idx, &location);
    }

    pub fn draw_marker_number_boxes(
        &self,
        ctx: &mut DrawingContext,
        size: Vec2,
        theme: &SurferTheme,
        viewport: &Viewport,
    ) {
        let text_size = ctx.cfg.text_size;

        for displayed_item in self
            .items_tree
            .iter_visible()
            .map(|node| self.displayed_items.get(&node.item_ref))
            .filter_map(|item| match item {
                Some(DisplayedItem::Marker(marker)) => Some(marker),
                _ => None,
            })
        {
            let background_color = displayed_item
                .color
                .as_ref()
                .and_then(|color| theme.get_color(color))
                .unwrap_or(&theme.cursor.color);

            let x = self.numbered_marker_location(displayed_item.idx, viewport, size.x);

            let idx_string = displayed_item.idx.to_string();
            // Determine size of text
            let rect = ctx.painter.text(
                (ctx.to_screen)(x, size.y * 0.5),
                Align2::CENTER_CENTER,
                idx_string.clone(),
                FontId::proportional(text_size),
                theme.foreground,
            );

            // Background rectangle
            let min = (ctx.to_screen)(rect.min.x, 0.);
            let max = (ctx.to_screen)(rect.max.x, size.y);
            let min = Pos2::new(rect.min.x - 2., min.y);
            let max = Pos2::new(rect.max.x + 2., max.y);

            ctx.painter.rect_filled(
                Rect { min, max },
                CornerRadius::default(),
                *background_color,
            );

            // Draw actual text on top of rectangle
            ctx.painter.text(
                (ctx.to_screen)(x, size.y * 0.5),
                Align2::CENTER_CENTER,
                idx_string,
                FontId::proportional(text_size),
                theme.foreground,
            );
        }
    }
}

impl SystemState {
    pub fn draw_marker_window(&self, waves: &WaveData, ctx: &Context, msgs: &mut Vec<Message>) {
        let mut open = true;

        let mut markers: Vec<(u8, &BigInt, WidgetText)> = vec![];
        if let Some(cursor) = &waves.cursor {
            markers.push((255, cursor, WidgetText::RichText(RichText::new("Primary"))));
        }

        let mut numbered_markers = waves
            .items_tree
            .iter()
            .map(|node| waves.displayed_items.get(&node.item_ref))
            .filter_map(|displayed_item| match displayed_item {
                Some(DisplayedItem::Marker(marker)) => {
                    let text_color = self.get_item_text_color(displayed_item.unwrap());
                    Some((
                        marker.idx,
                        waves.numbered_marker_time(marker.idx),
                        marker.marker_text(text_color),
                    ))
                }
                _ => None,
            })
            .sorted_by(|a, b| Ord::cmp(&a.0, &b.0))
            .collect_vec();

        markers.append(&mut numbered_markers);
        Window::new("Markers")
            .collapsible(true)
            .resizable(true)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    Grid::new("markers")
                        .striped(true)
                        .num_columns(markers.len() + 1)
                        .spacing([5., 5.])
                        .show(ui, |ui| {
                            ui.label("");
                            for (marker_idx, _, widget_text) in &markers {
                                if *marker_idx < 255 {
                                    ui.selectable_label(false, widget_text.clone())
                                        .clicked()
                                        .then(|| {
                                            msgs.push(Message::GoToMarkerPosition(*marker_idx, 0));
                                        });
                                } else {
                                    ui.selectable_label(false, widget_text.clone())
                                        .clicked()
                                        .then(|| {
                                            msgs.push(Message::GoToTime(waves.cursor.clone(), 0));
                                        });
                                }
                            }
                            ui.end_row();
                            for (marker_idx, row_marker_time, row_widget_text) in &markers {
                                if *marker_idx < 255 {
                                    ui.selectable_label(false, row_widget_text.clone())
                                        .clicked()
                                        .then(|| {
                                            msgs.push(Message::GoToMarkerPosition(*marker_idx, 0));
                                        });
                                } else {
                                    ui.selectable_label(false, row_widget_text.clone())
                                        .clicked()
                                        .then(|| {
                                            msgs.push(Message::GoToTime(waves.cursor.clone(), 0));
                                        });
                                }
                                for (_, col_marker_time, _) in &markers {
                                    ui.label(time_string(
                                        &(*row_marker_time - *col_marker_time),
                                        &waves.inner.metadata().timescale,
                                        &self.user.wanted_timeunit,
                                        &self.get_time_format(),
                                    ));
                                }
                                ui.end_row();
                            }
                        });
                    ui.add_space(15.);
                    if ui.button("Close").clicked() {
                        msgs.push(Message::SetCursorWindowVisible(false));
                    }
                });
            });
        if !open {
            msgs.push(Message::SetCursorWindowVisible(false));
        }
    }

    pub fn draw_marker_boxes(
        &self,
        waves: &WaveData,
        ctx: &mut DrawingContext,
        view_width: f32,
        gap: f32,
        viewport: &Viewport,
        y_zero: f32,
    ) {
        let text_size = ctx.cfg.text_size;

        for drawing_info in waves.drawing_infos.iter().filter_map(|item| match item {
            ItemDrawingInfo::Marker(marker) => Some(marker),
            _ => None,
        }) {
            let Some(item) = waves
                .items_tree
                .get_visible(drawing_info.item_list_idx)
                .and_then(|node| waves.displayed_items.get(&node.item_ref))
            else {
                return;
            };

            // We draw in absolute coords, but the variable offset in the y
            // direction is also in absolute coordinates, so we need to
            // compensate for that
            let y_offset = drawing_info.top - y_zero;
            let y_bottom = drawing_info.bottom - y_zero;

            let background_color = item
                .color()
                .and_then(|color| self.user.config.theme.get_color(color))
                .unwrap_or(&self.user.config.theme.cursor.color);

            let x = waves.numbered_marker_location(drawing_info.idx, viewport, view_width);

            // Time string
            let time = time_string(
                waves
                    .markers
                    .get(&drawing_info.idx)
                    .unwrap_or(&BigInt::from(0)),
                &waves.inner.metadata().timescale,
                &self.user.wanted_timeunit,
                &self.get_time_format(),
            );

            let text_color = *self.user.config.theme.get_best_text_color(background_color);

            // Create galley
            let galley =
                ctx.painter
                    .layout_no_wrap(time, FontId::proportional(text_size), text_color);
            let offset_width = galley.rect.width() * 0.5 + 2. * gap;

            // Background rectangle
            let min = (ctx.to_screen)(x - offset_width, y_offset - gap);
            let max = (ctx.to_screen)(x + offset_width, y_bottom + gap);

            ctx.painter.rect_filled(
                Rect { min, max },
                CornerRadius::default(),
                *background_color,
            );

            // Draw actual text on top of rectangle
            ctx.painter.galley(
                (ctx.to_screen)(
                    x - galley.rect.width() * 0.5,
                    (y_offset + y_bottom - galley.rect.height()) * 0.5,
                ),
                galley,
                text_color,
            );
        }
    }
}
