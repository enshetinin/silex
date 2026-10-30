//! A workspace tab for exploring page links. Never writes graph geometry to documents.
use super::{SilexApp, theme};
use eframe::egui::{self, Color32, Pos2, Vec2};
use std::collections::HashMap;

pub(super) const TAB_ID: &str = "silex:graph";
pub(super) struct Graph {
    nodes: Vec<Node>,
    edges: Vec<(usize, usize)>,
    zoom: f32,
    pan: Vec2,
    query: String,
    selected: Option<String>,
    fit: bool,
}
struct Node {
    id: String,
    title: String,
    position: Pos2,
    group: usize,
    degree: usize,
}

/// Connected components occupy separate islands, with the best-connected node at the centre.
fn arrange(nodes: &mut [Node], edges: &[(usize, usize)]) {
    let mut adjacency = vec![Vec::new(); nodes.len()];
    for &(a, b) in edges {
        adjacency[a].push(b);
        adjacency[b].push(a);
    }
    let mut seen = vec![false; nodes.len()];
    let mut groups = Vec::new();
    for start in 0..nodes.len() {
        if seen[start] {
            continue;
        }
        seen[start] = true;
        let mut stack = vec![start];
        let mut group = Vec::new();
        while let Some(i) = stack.pop() {
            group.push(i);
            for &j in &adjacency[i] {
                if !seen[j] {
                    seen[j] = true;
                    stack.push(j);
                }
            }
        }
        group.sort_by_key(|&i| std::cmp::Reverse(adjacency[i].len()));
        groups.push(group);
    }
    groups.sort_by_key(|g| std::cmp::Reverse(g.len()));
    let columns = (groups.len() as f32).sqrt().ceil().max(1.0) as usize;
    let cell = groups
        .iter()
        .map(|g| 180.0 * (g.len() as f32).sqrt())
        .fold(320.0_f32, f32::max);
    for (group_id, group) in groups.iter().enumerate() {
        let centre = egui::vec2(
            (group_id % columns) as f32 * cell,
            (group_id / columns) as f32 * cell,
        );
        for (rank, &i) in group.iter().enumerate() {
            let radius = if rank == 0 {
                0.0
            } else {
                130.0 * (rank as f32).sqrt()
            };
            let angle = rank as f32 * 2.399_963;
            nodes[i].position = Pos2::ZERO + centre + egui::vec2(angle.cos(), angle.sin()) * radius;
            nodes[i].group = group_id;
            nodes[i].degree = adjacency[i].len();
        }
    }
}

impl SilexApp {
    pub(super) fn graph_active(&self) -> bool {
        self.tabs.active_id() == Some(TAB_ID)
    }

    pub(super) fn open_graph(&mut self, _ctx: &egui::Context) -> anyhow::Result<()> {
        self.save_now()?;
        for closed in self.tabs.open_new(TAB_ID) {
            self.parked.remove(&closed);
        }
        self.show_active_tab(true)
    }

    pub(super) fn rebuild_graph(&mut self) -> anyhow::Result<()> {
        let links = self.db.graph_edges()?;
        let mut nodes: Vec<_> = self
            .pages
            .iter()
            .map(|page| Node {
                id: page.id.clone(),
                title: super::display_title(&page.title).to_owned(),
                position: Pos2::ZERO,
                group: 0,
                degree: 0,
            })
            .collect();
        let indices: HashMap<_, _> = nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id.as_str(), i))
            .collect();
        let edges: Vec<_> = links
            .iter()
            .filter_map(|(a, b)| Some((*indices.get(a.as_str())?, *indices.get(b.as_str())?)))
            .collect();
        arrange(&mut nodes, &edges);
        let old = self.graph.take();
        self.graph = Some(Graph {
            nodes,
            edges,
            zoom: old.as_ref().map_or(1.0, |g| g.zoom),
            pan: old.as_ref().map_or(Vec2::ZERO, |g| g.pan),
            query: old.as_ref().map_or_else(String::new, |g| g.query.clone()),
            selected: old.as_ref().and_then(|g| g.selected.clone()),
            fit: old.is_none(),
        });
        Ok(())
    }

    pub(super) fn graph_view(&mut self, ui: &mut egui::Ui) {
        let Some(mut graph) = self.graph.take() else {
            return;
        };
        let mut open = None;
        let mut refresh = false;
        let p = *theme::current(ui.ctx());
        let dark = ui.visuals().dark_mode;
        let bg = if dark {
            Color32::from_rgb(31, 31, 40)
        } else {
            p.subtle
        };
        let colors = if dark {
            [
                Color32::from_rgb(126, 156, 216),
                Color32::from_rgb(152, 187, 108),
                Color32::from_rgb(187, 154, 247),
                Color32::from_rgb(224, 175, 104),
            ]
        } else {
            [
                Color32::from_rgb(55, 93, 156),
                Color32::from_rgb(63, 115, 78),
                Color32::from_rgb(113, 77, 150),
                Color32::from_rgb(143, 93, 35),
            ]
        };
        ui.add_space(18.0);
        ui.heading("Tu mapa de ideas");
        ui.label(
            egui::RichText::new(format!(
                "{} páginas · {} enlaces · Explora cómo se conecta tu espacio",
                graph.nodes.len(),
                graph.edges.len()
            ))
            .color(p.text_muted),
        );
        ui.add_space(14.0);
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut graph.query)
                    .hint_text("Buscar en el mapa…")
                    .desired_width(210.0),
            );
            if ui.button("−").on_hover_text("Alejar").clicked() {
                graph.zoom = (graph.zoom / 1.2).max(0.02);
            }
            ui.label(format!("{} %", (graph.zoom * 100.0).round()));
            if ui.button("+").on_hover_text("Acercar").clicked() {
                graph.zoom = (graph.zoom * 1.2).min(4.0);
            }
            if ui.button("Encajar").clicked() {
                graph.fit = true;
            }
            if ui.button("Actualizar").clicked() {
                refresh = true;
            }
        });
        ui.add_space(8.0);
        let selected = graph
            .nodes
            .iter()
            .position(|n| Some(&n.id) == graph.selected.as_ref());
        ui.horizontal(|ui| {
            if let Some(index) = selected {
                ui.label(egui::RichText::new(&graph.nodes[index].title).strong());
                ui.label(format!("{} conexiones", graph.nodes[index].degree));
                if ui.button("Abrir página").clicked() {
                    open = Some(graph.nodes[index].id.clone());
                }
                if ui.button("Quitar selección").clicked() {
                    graph.selected = None;
                }
            } else {
                ui.label(
                    egui::RichText::new("Selecciona una página para destacar sus conexiones")
                        .color(p.text_muted),
                );
            }
        });
        ui.add_space(10.0);
        let size = egui::vec2(
            ui.available_width(),
            (ui.available_height() - 28.0).max(100.0),
        );
        let (response, painter) = ui.allocate_painter(size, egui::Sense::click_and_drag());
        let rect = response.rect;
        let painter = painter.with_clip_rect(rect.intersect(ui.clip_rect()));
        painter.rect_filled(rect, 16.0, bg);
        painter.rect_stroke(
            rect,
            16.0,
            egui::Stroke::new(1.0, p.border),
            egui::StrokeKind::Inside,
        );
        if graph.fit && !graph.nodes.is_empty() {
            let bounds = egui::Rect::from_points(
                &graph.nodes.iter().map(|n| n.position).collect::<Vec<_>>(),
            )
            .expand(100.0);
            graph.zoom = ((rect.width() - 50.0) / bounds.width())
                .min((rect.height() - 50.0) / bounds.height())
                .clamp(0.02, 1.25);
            graph.pan = -bounds.center().to_vec2() * graph.zoom;
            graph.fit = false;
        }
        // A quiet dotted canvas gives movement and scale a visual reference.
        let step = 32.0;
        for x in 0..(rect.width() / step) as usize + 1 {
            for y in 0..(rect.height() / step) as usize + 1 {
                painter.circle_filled(
                    rect.min
                        + egui::vec2(
                            x as f32 * step + graph.pan.x.rem_euclid(step),
                            y as f32 * step + graph.pan.y.rem_euclid(step),
                        ),
                    0.7,
                    p.border.gamma_multiply(0.5),
                );
            }
        }
        if response.dragged() {
            graph.pan += response.drag_delta();
        }
        if response.hovered() {
            let old = graph.zoom;
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            graph.zoom = (old * (scroll * 0.002).exp()).clamp(0.02, 4.0);
            if let Some(pointer) = ui.input(|i| i.pointer.hover_pos()) {
                graph.pan = pointer
                    - rect.center()
                    - (pointer - rect.center() - graph.pan) * (graph.zoom / old);
            }
        }
        let positions: Vec<_> = graph
            .nodes
            .iter()
            .map(|n| rect.center() + graph.pan + n.position.to_vec2() * graph.zoom)
            .collect();
        let hovered = ui
            .input(|i| i.pointer.hover_pos())
            .filter(|pos| rect.contains(*pos))
            .and_then(|pos| positions.iter().position(|p| p.distance(pos) < 16.0));
        let focus = hovered.or(selected);
        let mut related = vec![false; graph.nodes.len()];
        if let Some(i) = focus {
            related[i] = true;
        }
        let query = silex_desktop::editor::fold(&graph.query);
        for &(a, b) in &graph.edges {
            let active = focus == Some(a) || focus == Some(b);
            if active {
                related[a] = true;
                related[b] = true;
            }
            let color = if active {
                colors[graph.nodes[a].group % colors.len()]
            } else {
                p.text_faint
                    .gamma_multiply(if focus.is_some() { 0.15 } else { 0.4 })
            };
            let start = positions[a];
            let end = positions[b];
            let delta = end - start;
            let normal = egui::vec2(-delta.y, delta.x).normalized() * 22.0;
            painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
                [
                    start,
                    start + delta * 0.33 + normal,
                    start + delta * 0.66 + normal,
                    end,
                ],
                false,
                Color32::TRANSPARENT,
                egui::Stroke::new(if active { 2.0 } else { 1.0 }, color),
            ));
        }
        let mut labels = Vec::<egui::Rect>::new();
        for (i, node) in graph.nodes.iter().enumerate() {
            let pos = positions[i];
            if !rect.shrink(18.0).contains(pos) {
                continue;
            }
            let matching =
                query.is_empty() || silex_desktop::editor::fold(&node.title).contains(&query);
            let emphasized = matching && (focus.is_none() || related[i]);
            let base = colors[node.group % colors.len()];
            let color = if emphasized {
                base
            } else {
                base.gamma_multiply(0.25)
            };
            let radius = 6.0 + (node.degree as f32).sqrt().min(6.0);
            if emphasized {
                painter.circle_filled(pos, radius + 8.0, color.gamma_multiply(0.08));
                painter.circle_filled(pos, radius + 4.0, color.gamma_multiply(0.12));
            }
            painter.circle_filled(pos, radius, color);
            if focus == Some(i) {
                painter.circle_stroke(pos, radius + 5.0, egui::Stroke::new(1.5, base));
            }
            let text: String = node.title.chars().take(32).collect();
            let galley = painter.layout_no_wrap(
                text,
                egui::FontId::proportional(12.0),
                if emphasized {
                    p.text
                } else {
                    p.text_faint.gamma_multiply(0.4)
                },
            );
            let label = egui::Rect::from_min_size(
                pos + egui::vec2(-galley.size().x / 2.0, radius + 9.0),
                galley.size(),
            )
            .expand2(egui::vec2(5.0, 3.0));
            if matching && (focus == Some(i) || !labels.iter().any(|r| r.intersects(label))) {
                painter.rect_filled(label, 4.0, bg);
                painter.galley(label.min + egui::vec2(5.0, 3.0), galley, p.text);
                labels.push(label);
            }
            let hit = ui.interact(
                egui::Rect::from_center_size(pos, egui::vec2(30.0, 30.0)),
                egui::Id::unique("graph_node").with(&node.id),
                egui::Sense::click(),
            );
            hit.widget_info(|| {
                egui::WidgetInfo::labeled(egui::Role::Button, true, format!("Abrir {}", node.title))
            });
            if hit.clicked() {
                graph.selected = Some(node.id.clone());
            }
            if hit.double_clicked()
                || (hit.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
            {
                open = Some(node.id.clone());
            }
            hit.on_hover_text(format!("{}\n{} conexiones", node.title, node.degree));
        }
        if graph.nodes.is_empty() {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Crea páginas y enlázalas con [[",
                egui::FontId::proportional(16.0),
                p.text_muted,
            );
        }
        ui.add_space(5.0);
        ui.label(egui::RichText::new("Arrastra para explorar · Rueda para zoom · Doble clic para abrir · Colores por grupo conectado").size(11.0).color(p.text_muted));
        self.graph = Some(graph);
        if refresh {
            let result = self.rebuild_graph();
            self.report(result);
        }
        if let Some(id) = open {
            let result = self.open_in_new_tab(&id);
            self.report(result);
        }
    }
}
