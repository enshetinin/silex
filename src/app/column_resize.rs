use eframe::egui;

#[derive(Clone, Copy)]
pub(super) struct Resize {
    pub width: f32,
    pub finished: bool,
}

/// A separator usable by dragging or with the arrow keys (Shift: 10 px).
pub(super) fn handle(
    ui: &mut egui::Ui,
    cell: egui::Rect,
    column: &str,
    width: f32,
) -> Option<Resize> {
    let rect = egui::Rect::from_center_size(
        egui::pos2(cell.right() - 4.0, cell.center().y),
        egui::vec2(10.0, cell.height()),
    );
    let response = ui
        .interact(
            rect,
            ui.make_persistent_id(("column-resize", column)),
            egui::Sense::click_and_drag(),
        )
        .on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
        .on_hover_text("Arrastra para ajustar el ancho. Flechas: ajustar; Mayús: 10 px.");
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::Role::Button,
            true,
            format!("Ajustar ancho de {column}"),
        )
    });
    if response.hovered() || response.dragged() || response.has_focus() {
        ui.painter().vline(
            rect.center().x,
            rect.y_range(),
            egui::Stroke::new(2.0, ui.visuals().selection.bg_fill),
        );
    }
    let anchor = response.id.with("drag-origin");
    if response.drag_started()
        && let Some(origin) = ui.input(|i| i.pointer.press_origin())
    {
        ui.data_mut(|d| d.insert_temp(anchor, (origin.x, width)));
    }
    let finished = response.drag_stopped();
    if response.dragged() || finished {
        let start = ui.data(|d| d.get_temp::<(f32, f32)>(anchor));
        if finished {
            ui.data_mut(|d| d.remove::<(f32, f32)>(anchor));
        }
        if let Some((origin, initial)) = start {
            let target = response.interact_pointer_pos().map_or(width, |pos| {
                (initial + pos.x - origin).clamp(80.0, 1200.0).round()
            });
            if target != width || finished {
                ui.ctx().request_repaint();
                return Some(Resize {
                    width: target,
                    finished,
                });
            }
        }
    }
    if response.has_focus() {
        let step = ui.input(|i| if i.modifiers.shift { 10.0 } else { 1.0 });
        for (key, delta) in [(egui::Key::ArrowLeft, -step), (egui::Key::ArrowRight, step)] {
            if ui.input_mut(|i| {
                i.consume_key(
                    if i.modifiers.shift {
                        egui::Modifiers::SHIFT
                    } else {
                        egui::Modifiers::NONE
                    },
                    key,
                )
            }) {
                return Some(Resize {
                    width: (width + delta).clamp(80.0, 1200.0),
                    finished: true,
                });
            }
        }
    }
    None
}
