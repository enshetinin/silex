//! Read-only relationship explorer. Positions and camera live only for this session.
use super::{SilexApp, theme};
use eframe::egui::{self, Color32, Pos2, Vec2};
use std::collections::HashMap;

#[derive(Clone)]
struct Graph {
    nodes: Vec<(String, String, Pos2)>,
    edges: Vec<(usize, usize)>,
    zoom: f32,
    pan: Vec2,
    query: String,
}
fn key() -> egui::Id {
    egui::Id::unique("relationship_graph")
}
impl SilexApp {
    pub(super) fn open_graph(&mut self, ctx: &egui::Context) -> anyhow::Result<()> {
        self.save_now()?;
        let edges = self.db.graph_edges()?;
        let nodes: Vec<_> = self
            .pages
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let angle = i as f32 * 2.399_963;
                let radius = 90.0 * (i as f32).sqrt();
                (
                    p.id.clone(),
                    super::display_title(&p.title).to_owned(),
                    Pos2::new(angle.cos() * radius, angle.sin() * radius),
                )
            })
            .collect();
        let indices: HashMap<_, _> = nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.0.as_str(), i))
            .collect();
        let edges = edges
            .iter()
            .filter_map(|(a, b)| Some((*indices.get(a.as_str())?, *indices.get(b.as_str())?)))
            .collect();
        ctx.data_mut(|d| {
            d.insert_temp(
                key(),
                Graph {
                    nodes,
                    edges,
                    zoom: 1.0,
                    pan: Vec2::ZERO,
                    query: String::new(),
                },
            )
        });
        Ok(())
    }

    pub(super) fn graph_window(&mut self, ctx: &egui::Context) {
        let Some(mut graph) = ctx.data(|d| d.get_temp::<Graph>(key())) else {
            return;
        };
        let mut visible = true;
        let mut open = None;
        egui::Window::new("Relaciones entre páginas")
            .open(&mut visible)
            .default_size(egui::vec2(850.0, 580.0))
            .show(ctx, |ui| {
                let mut p = *theme::current(ctx);
                if ui.visuals().dark_mode {
                    p.bg = Color32::from_rgb(31, 31, 40);
                    p.accent = Color32::from_rgb(126, 156, 216);
                    p.accent_soft = Color32::from_rgb(45, 49, 70);
                    p.border = Color32::from_rgb(64, 68, 88);
                    p.text = Color32::from_rgb(220, 215, 186);
                }
                ui.horizontal(|ui| {
                    ui.label(format!(
                        "{} páginas · {} enlaces",
                        graph.nodes.len(),
                        graph.edges.len()
                    ));
                    ui.add(
                        egui::TextEdit::singleline(&mut graph.query)
                            .hint_text("Buscar página…")
                            .desired_width(180.0),
                    );
                    if ui.button("Centrar").clicked() {
                        graph.pan = Vec2::ZERO;
                        graph.zoom =
                            (200.0 / (90.0 * (graph.nodes.len().max(1) as f32).sqrt())).min(1.5);
                    }
                });
                ui.label(
                    "Arrastra el fondo para moverte · Rueda para zoom · Doble clic para abrir",
                );
                if graph.nodes.is_empty() {
                    ui.label("Crea páginas y enlázalas con [[ para explorar sus relaciones.");
                    return;
                }
                let (response, painter) = ui.allocate_painter(
                    ui.available_size().max(egui::vec2(200.0, 200.0)),
                    egui::Sense::drag(),
                );
                let rect = response.rect;
                let painter = painter.with_clip_rect(rect.intersect(ui.clip_rect()));
                painter.rect_filled(rect, 12.0, p.bg);
                if response.dragged() {
                    graph.pan += response.drag_delta();
                }
                if response.hovered() {
                    let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                    let old = graph.zoom;
                    graph.zoom = (old * (scroll * 0.002).exp()).clamp(0.15, 4.0);
                    if let Some(pointer) = ui.input(|i| i.pointer.hover_pos()) {
                        graph.pan = pointer
                            - rect.center()
                            - (pointer - rect.center() - graph.pan) * (graph.zoom / old);
                    }
                }
                let positions: Vec<_> = graph
                    .nodes
                    .iter()
                    .map(|n| rect.center() + graph.pan + n.2.to_vec2() * graph.zoom)
                    .collect();
                let hovered = ui
                    .input(|i| i.pointer.hover_pos())
                    .filter(|pos| rect.contains(*pos))
                    .and_then(|pos| positions.iter().position(|p| p.distance(pos) < 15.0));
                let query = graph.query.to_lowercase();
                for &(a, b) in &graph.edges {
                    let active = hovered == Some(a) || hovered == Some(b);
                    painter.line_segment(
                        [positions[a], positions[b]],
                        egui::Stroke::new(
                            if active { 2.0 } else { 1.0 },
                            if active { p.accent } else { p.border },
                        ),
                    );
                }
                for (i, (_, title, _)) in graph.nodes.iter().enumerate() {
                    let pos = positions[i];
                    if !rect.shrink(12.0).contains(pos) {
                        continue;
                    }
                    let active = hovered == Some(i);
                    let matching = query.is_empty() || title.to_lowercase().contains(&query);
                    let color = if matching { p.accent } else { p.text_faint };
                    if active || (!query.is_empty() && matching) {
                        painter.circle_filled(pos, 19.0, p.accent_soft);
                    }
                    painter.circle_filled(pos, if active { 8.0 } else { 5.0 }, color);
                    painter.circle_stroke(
                        pos,
                        10.0,
                        egui::Stroke::new(
                            1.0,
                            Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 55),
                        ),
                    );
                    if active || (matching && (graph.zoom > 0.7 || !query.is_empty())) {
                        let label: String = title.chars().take(30).collect();
                        painter.text(
                            pos + egui::vec2(0.0, 15.0),
                            egui::Align2::CENTER_TOP,
                            label,
                            egui::FontId::proportional(12.0),
                            p.text,
                        );
                    }
                    let hit = ui.interact(
                        egui::Rect::from_center_size(pos, egui::vec2(24.0, 24.0)),
                        key().with(i),
                        egui::Sense::click(),
                    );
                    hit.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::Role::Button,
                            true,
                            format!("Abrir {title}"),
                        )
                    });
                    if hit.double_clicked()
                        || (hit.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                    {
                        open = Some(graph.nodes[i].0.clone());
                    }
                    hit.on_hover_text(title);
                }
            });
        if open.is_some() {
            visible = false;
        }
        ctx.data_mut(|d| {
            if visible {
                d.insert_temp(key(), graph);
            } else {
                d.remove::<Graph>(key());
            }
        });
        if let Some(id) = open {
            let result = self.open_page(id);
            self.report(result);
        }
    }
}
