//! A workspace tab for exploring how pages connect: links between pages and
//! subpages (H13 graph redesign). A global map with an inspector, and a
//! local view centred on one page. Never writes graph geometry to documents.
use super::{SilexApp, icons, theme};
use eframe::egui::{self, Color32, FontFamily, FontId, Pos2, Rect, Sense, Vec2, pos2, vec2};
use silex_desktop::model::PageKind;
use silex_desktop::property::OptionColor;
use std::collections::HashMap;

pub(super) const TAB_ID: &str = "silex:graph";

/// Width of the inspector on the right of the canvas.
const INSPECTOR: f32 = 280.0;
/// Ideal distance between linked nodes in the force layout (world px).
const SPRING: f32 = 110.0;
/// Space between pages on the shelf of pages without connections.
const SHELF_STEP: Vec2 = vec2(170.0, 64.0);
/// Widest a node label gets on the canvas.
const LABEL_WIDTH: f32 = 140.0;
/// Rows listed per section of the inspector before «y N más».
const INSPECTOR_ROWS: usize = 12;

#[derive(Clone, Copy, PartialEq, Eq)]
enum EdgeKind {
    /// A `[[link]]` from one page to another.
    Link,
    /// A page and one of its subpages (or a database and a row).
    Sub,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Global,
    Local,
}

pub(super) struct Graph {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    /// Where the pages without connections sit, below the map (world).
    shelf: Option<Rect>,
    zoom: f32,
    pan: Vec2,
    query: String,
    selected: Option<String>,
    fit: bool,
    mode: Mode,
    /// The page at the centre of the local graph.
    focus: Option<String>,
    /// Steps from the focus shown in the local graph: 1 or 2.
    depth: u8,
    /// The global camera, kept while the local graph is shown.
    global_camera: Option<(f32, Vec2)>,
    inspector: bool,
    show_subpages: bool,
    show_orphans: bool,
    show_databases: bool,
    color_by_root: bool,
}

struct Node {
    id: String,
    title: String,
    kind: PageKind,
    parent: Option<usize>,
    /// Inside a database (its row) or a database itself.
    database: bool,
    /// Color group: the root page of its tree, when that tree has subpages.
    root: Option<usize>,
    position: Pos2,
    degree: usize,
}

struct Edge {
    a: usize,
    b: usize,
    kind: EdgeKind,
}

/// What one frame shows: nodes at world positions, and the lines between
/// them (indices into `items`).
struct Scene {
    items: Vec<Item>,
    edges: Vec<(usize, usize, EdgeKind)>,
    shelf: Option<(Rect, usize)>,
}

struct Item {
    node: usize,
    pos: Pos2,
    /// Steps from the focus in the local graph; 0 in the global one.
    level: u8,
}

/// Force layout of one group of connected pages (linked pages pull, all
/// pages push), starting from a spiral.
fn relax(group: &[usize], edges: &[Edge], pos: &mut [Vec2]) {
    for (rank, &i) in group.iter().enumerate() {
        let angle = rank as f32 * 2.399_963;
        pos[i] = vec2(angle.cos(), angle.sin()) * 40.0 * (rank as f32 + 1.0).sqrt();
    }
    let inside: std::collections::HashSet<usize> = group.iter().copied().collect();
    let links: Vec<&Edge> = edges.iter().filter(|e| inside.contains(&e.a)).collect();
    // Fewer steps on big groups, so a refresh stays quick.
    let m = group.len() as f32;
    let steps = (20_000_000.0 / (m * m)).clamp(40.0, 300.0) as usize;
    let mut temperature = 80.0_f32;
    let mut moves: HashMap<usize, Vec2> = HashMap::new();
    for _ in 0..steps {
        moves.clear();
        for (x, &i) in group.iter().enumerate() {
            for &j in &group[x + 1..] {
                let d = pos[i] - pos[j];
                let dist = d.length().max(1.0);
                let push = d / dist * (SPRING * SPRING / dist);
                *moves.entry(i).or_default() += push;
                *moves.entry(j).or_default() -= push;
            }
        }
        for e in &links {
            let d = pos[e.a] - pos[e.b];
            let dist = d.length().max(1.0);
            let pull = d / dist * (dist * dist / SPRING);
            *moves.entry(e.a).or_default() -= pull;
            *moves.entry(e.b).or_default() += pull;
        }
        for &i in group {
            let step = moves.get(&i).copied().unwrap_or_default() - pos[i] * 0.02;
            let len = step.length();
            if len > 0.0 {
                pos[i] += step / len * len.min(temperature);
            }
        }
        temperature = (temperature * 0.97).max(2.0);
    }
}

/// Places connected pages with a force layout (linked pages pull, all pages
/// push) and puts the pages without connections on a shelf below. Returns
/// the shelf. Deterministic: the same pages give the same map.
fn arrange(nodes: &mut [Node], edges: &[Edge]) -> Option<Rect> {
    let n = nodes.len();
    let mut degree = vec![0; n];
    for e in edges {
        degree[e.a] += 1;
        degree[e.b] += 1;
    }
    for (node, d) in nodes.iter_mut().zip(&degree) {
        node.degree = *d;
    }
    // By title (then id), so the map does not depend on the order pages
    // come in.
    let mut by_title: Vec<usize> = (0..n).collect();
    by_title.sort_by_cached_key(|&i| {
        (
            silex_desktop::editor::fold(&nodes[i].title),
            nodes[i].id.clone(),
        )
    });
    let mut rank = vec![0; n];
    for (r, &i) in by_title.iter().enumerate() {
        rank[i] = r;
    }
    let connected: Vec<usize> = by_title
        .iter()
        .copied()
        .filter(|&i| degree[i] > 0)
        .collect();
    let mut adjacency = vec![Vec::new(); n];
    for e in edges {
        adjacency[e.a].push(e.b);
        adjacency[e.b].push(e.a);
    }
    // Each group of connected pages is laid out on its own, then the groups
    // are packed in rows, biggest first: no group drifts far from the rest.
    let mut seen = vec![false; n];
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for &start in &connected {
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
        group.sort_by_key(|&i| rank[i]);
        groups.push(group);
    }
    groups.sort_by_key(|g| std::cmp::Reverse(g.len()));
    let mut pos = vec![Vec2::ZERO; n];
    let mut boxes = Vec::new();
    for group in &groups {
        relax(group, edges, &mut pos);
        let rect = Rect::from_points(&group.iter().map(|&i| pos[i].to_pos2()).collect::<Vec<_>>())
            .expand2(vec2(80.0, 50.0));
        boxes.push(rect);
    }
    let area: f32 = boxes.iter().map(|b| b.area()).sum();
    let row_width = boxes
        .iter()
        .map(|b| b.width())
        .fold(area.sqrt() * 1.8, f32::max);
    let (mut x, mut y, mut row_height) = (0.0_f32, 0.0_f32, 0.0_f32);
    let mut row: Vec<usize> = Vec::new();
    let place = |row: &mut Vec<usize>, y: f32, height: f32, pos: &mut [Vec2], x0: &mut f32| {
        let mut x = 0.0;
        for &g in row.iter() {
            let b: Rect = boxes[g];
            let shift = vec2(x - b.left(), y + (height - b.height()) / 2.0 - b.top());
            for &i in &groups[g] {
                pos[i] += shift;
            }
            x += b.width();
        }
        row.clear();
        *x0 = 0.0;
    };
    for (g, b) in boxes.iter().enumerate() {
        if x > 0.0 && x + b.width() > row_width {
            place(&mut row, y, row_height, &mut pos, &mut x);
            y += row_height;
            row_height = 0.0;
        }
        row.push(g);
        x += b.width();
        row_height = row_height.max(b.height());
    }
    place(&mut row, y, row_height, &mut pos, &mut x);
    let bounds = if connected.is_empty() {
        Rect::from_center_size(Pos2::ZERO, Vec2::ZERO)
    } else {
        Rect::from_points(
            &connected
                .iter()
                .map(|&i| pos[i].to_pos2())
                .collect::<Vec<_>>(),
        )
    };
    for &i in &connected {
        nodes[i].position = pos[i].to_pos2() - bounds.center().to_vec2();
    }
    let mut orphans: Vec<usize> = (0..n).filter(|&i| degree[i] == 0).collect();
    if orphans.is_empty() {
        return None;
    }
    orphans.sort_by_key(|&i| silex_desktop::editor::fold(&nodes[i].title));
    let columns = if connected.is_empty() {
        ((orphans.len() as f32).sqrt() * 1.6).ceil()
    } else {
        (bounds.width() / SHELF_STEP.x).floor().max(4.0)
    }
    .max(1.0) as usize;
    let columns = columns.min(orphans.len());
    let width = (columns - 1) as f32 * SHELF_STEP.x;
    let top = if connected.is_empty() {
        0.0
    } else {
        bounds.height() / 2.0 + 90.0
    };
    for (k, &i) in orphans.iter().enumerate() {
        let (row, column) = (k / columns, k % columns);
        nodes[i].position = pos2(
            -width / 2.0 + column as f32 * SHELF_STEP.x,
            top + 56.0 + row as f32 * SHELF_STEP.y,
        );
    }
    let rows = orphans.len().div_ceil(columns);
    Some(Rect::from_min_max(
        pos2((-width / 2.0 - 60.0).min(-bounds.width() / 2.0), top),
        pos2(
            (width / 2.0 + 60.0).max(bounds.width() / 2.0),
            top + 56.0 + (rows - 1) as f32 * SHELF_STEP.y + 30.0,
        ),
    ))
}

/// A label cut to one line with «…».
fn label_job(text: &str, font: FontId, color: Color32, width: f32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping {
        max_width: width,
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    job
}

/// Colors of the page trees when «Color: por página raíz» is on (no blue:
/// it marks the selection).
fn root_colors(ctx: &egui::Context) -> [Color32; 6] {
    [
        OptionColor::Purple,
        OptionColor::Orange,
        OptionColor::Green,
        OptionColor::Pink,
        OptionColor::Yellow,
        OptionColor::Red,
    ]
    .map(|c| theme::option_colors(ctx, c).1)
}

/// A segmented control (like the tab bar). Returns the option clicked.
fn segmented(
    ui: &mut egui::Ui,
    p: &theme::Palette,
    id: &str,
    options: &[&str],
    active: usize,
) -> Option<usize> {
    const PAD: f32 = 3.0;
    let font = FontId::proportional(13.0);
    let widths: Vec<f32> = options
        .iter()
        .map(|o| {
            ui.fonts_mut(|f| {
                f.layout_no_wrap((*o).to_owned(), font.clone(), p.text)
                    .size()
                    .x
            }) + 20.0
        })
        .collect();
    let total = widths.iter().sum::<f32>() + 2.0 * PAD + 2.0 * (options.len() as f32 - 1.0);
    let (rect, _) = ui.allocate_exact_size(vec2(total, 30.0), Sense::hover());
    let dark = ui.visuals().dark_mode;
    ui.painter().rect(
        rect,
        8.0,
        if dark { p.surface } else { p.subtle },
        egui::Stroke::new(1.0, p.border),
        egui::StrokeKind::Inside,
    );
    let mut x = rect.left() + PAD;
    let mut clicked = None;
    for (index, (option, width)) in options.iter().zip(widths).enumerate() {
        let chip = Rect::from_min_size(pos2(x, rect.top() + PAD), vec2(width, 24.0));
        x += width + 2.0;
        let response = ui.interact(chip, egui::Id::unique(id).with(index), Sense::click());
        let on = index == active;
        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, true, on, *option));
        if on {
            ui.painter().rect(
                chip,
                6.0,
                if dark { p.pressed } else { p.bg },
                egui::Stroke::new(1.0, p.border_strong),
                egui::StrokeKind::Inside,
            );
        } else if response.hovered() {
            ui.painter().rect_filled(chip, 6.0, p.subtle);
        }
        if response.has_focus() {
            ui.painter().rect_stroke(
                chip,
                6.0,
                ui.visuals().selection.stroke,
                egui::StrokeKind::Inside,
            );
        }
        ui.painter().text(
            chip.center(),
            egui::Align2::CENTER_CENTER,
            *option,
            font.clone(),
            if on { p.text } else { p.text_muted },
        );
        if response.clicked() {
            clicked = Some(index);
        }
    }
    clicked
}

/// The marker of a page in the inspector and the legend: a dot, or a small
/// square for a database.
fn marker(painter: &egui::Painter, centre: Pos2, kind: PageKind, color: Color32) {
    if kind == PageKind::Database {
        painter.rect_filled(Rect::from_center_size(centre, Vec2::splat(8.0)), 2.0, color);
    } else {
        painter.circle_filled(centre, 3.5, color);
    }
}

/// A clickable row of the inspector: marker and title. `link` draws the
/// link icon instead of the marker.
fn row(
    ui: &mut egui::Ui,
    p: &theme::Palette,
    title: &str,
    kind: PageKind,
    link: bool,
) -> egui::Response {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, 28.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, title));
    let hovered = response.hovered() || response.has_focus();
    if hovered {
        ui.painter().rect_filled(rect, 6.0, p.subtle);
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect,
            6.0,
            ui.visuals().selection.stroke,
            egui::StrokeKind::Inside,
        );
    }
    let icon_centre = pos2(rect.left() + 16.0, rect.center().y);
    if link {
        ui.painter().text(
            icon_centre,
            egui::Align2::CENTER_CENTER,
            icons::LINK,
            FontId::new(14.0, FontFamily::Name(icons::FAMILY.into())),
            p.text_muted,
        );
    } else {
        marker(ui.painter(), icon_centre, kind, p.text_muted);
    }
    let color = if hovered { p.text } else { p.sidebar_text };
    let galley = ui.painter().layout_job(label_job(
        title,
        FontId::proportional(13.0),
        color,
        rect.width() - 40.0,
    ));
    ui.painter().galley(
        pos2(rect.left() + 30.0, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}

/// A small grey title over a list in the inspector, with a count.
fn section(ui: &mut egui::Ui, p: &theme::Palette, title: &str, count: Option<usize>) {
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(egui::RichText::new(title).size(12.0).color(p.text_muted));
        if let Some(count) = count {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(count.to_string())
                        .size(12.0)
                        .color(p.text_muted),
                );
            });
        }
    });
}

/// The inspector's note for a page with no links.
const NO_LINKS: &str = "Ninguno todavía. Escribe [[ en la página para enlazar otra.";

/// Grey help text in the inspector, wrapped, in line with the rows.
fn hint(ui: &mut egui::Ui, p: &theme::Palette, text: &str) {
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.add(
                egui::Label::new(egui::RichText::new(text).size(12.0).color(p.text_muted)).wrap(),
            );
        });
}

/// What the graph asks the app to do after drawing.
#[derive(Default)]
struct Actions {
    open: Option<String>,
    refresh: bool,
}

impl Graph {
    fn index(&self, id: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n.id == id)
    }

    fn visible(&self, i: usize) -> bool {
        let node = &self.nodes[i];
        (self.show_databases || !node.database) && (self.show_orphans || node.degree > 0)
    }

    fn edge_shown(&self, e: &Edge) -> bool {
        (self.show_subpages || e.kind == EdgeKind::Link) && self.visible(e.a) && self.visible(e.b)
    }

    fn scene(&self) -> Scene {
        match self.mode {
            Mode::Global => self.global_scene(),
            Mode::Local => self.local_scene(),
        }
    }

    fn global_scene(&self) -> Scene {
        let mut slot = vec![None; self.nodes.len()];
        let mut items = Vec::new();
        for (i, node) in self.nodes.iter().enumerate() {
            if self.visible(i) {
                slot[i] = Some(items.len());
                items.push(Item {
                    node: i,
                    pos: node.position,
                    level: 0,
                });
            }
        }
        let edges = self
            .edges
            .iter()
            .filter(|e| self.edge_shown(e))
            .filter_map(|e| Some((slot[e.a]?, slot[e.b]?, e.kind)))
            .collect();
        let orphans = (0..self.nodes.len())
            .filter(|&i| self.nodes[i].degree == 0 && self.visible(i))
            .count();
        Scene {
            items,
            edges,
            shelf: self.shelf.filter(|_| orphans > 0).map(|r| (r, orphans)),
        }
    }

    /// The focus at the centre, its neighbours on an ellipse around it and,
    /// at depth 2, theirs on a wider one, near the page that leads to them.
    fn local_scene(&self) -> Scene {
        let mut scene = Scene {
            items: Vec::new(),
            edges: Vec::new(),
            shelf: None,
        };
        let Some(focus) = self.focus.as_deref().and_then(|id| self.index(id)) else {
            return scene;
        };
        let mut adjacency: Vec<Vec<(usize, EdgeKind)>> = vec![Vec::new(); self.nodes.len()];
        for e in self.edges.iter().filter(|e| {
            (self.show_subpages || e.kind == EdgeKind::Link)
                && (self.show_databases || (!self.nodes[e.a].database && !self.nodes[e.b].database))
        }) {
            adjacency[e.a].push((e.b, e.kind));
            adjacency[e.b].push((e.a, e.kind));
        }
        for list in &mut adjacency {
            list.sort_by_key(|(j, _)| silex_desktop::editor::fold(&self.nodes[*j].title));
            list.dedup_by_key(|(j, _)| *j);
        }
        let mut slot: HashMap<usize, usize> = HashMap::new();
        slot.insert(focus, 0);
        scene.items.push(Item {
            node: focus,
            pos: Pos2::ZERO,
            level: 0,
        });
        let first = &adjacency[focus];
        let inner_x = (first.len() as f32 * 26.0).max(220.0);
        let inner = vec2(inner_x, inner_x * 0.68);
        let outer = inner + vec2(170.0, 100.0);
        let mut angles = Vec::new();
        for (k, &(j, kind)) in first.iter().enumerate() {
            let angle = -std::f32::consts::FRAC_PI_2
                + k as f32 * std::f32::consts::TAU / first.len() as f32;
            slot.insert(j, scene.items.len());
            scene.edges.push((0, scene.items.len(), kind));
            scene.items.push(Item {
                node: j,
                pos: pos2(angle.cos() * inner.x, angle.sin() * inner.y),
                level: 1,
            });
            angles.push((j, angle));
        }
        if self.depth >= 2 {
            let share = std::f32::consts::TAU / first.len().max(1) as f32;
            for (parent, angle) in angles {
                let next: Vec<_> = adjacency[parent]
                    .iter()
                    .filter(|(j, _)| !slot.contains_key(j))
                    .copied()
                    .collect();
                let spread = (share * 0.8 / next.len().max(1) as f32).min(0.34);
                let from = slot[&parent];
                for (k, (j, kind)) in next.iter().enumerate() {
                    let a = angle + (k as f32 - (next.len() as f32 - 1.0) / 2.0) * spread;
                    slot.insert(*j, scene.items.len());
                    scene.edges.push((from, scene.items.len(), *kind));
                    scene.items.push(Item {
                        node: *j,
                        pos: pos2(a.cos() * outer.x, a.sin() * outer.y),
                        level: 2,
                    });
                }
            }
        }
        scene
    }

    fn set_mode(&mut self, mode: Mode) {
        if mode == self.mode {
            return;
        }
        if mode == Mode::Local {
            self.global_camera = Some((self.zoom, self.pan));
            if self.selected.is_some() {
                self.focus = self.selected.clone();
            }
            self.fit = true;
        } else if let Some((zoom, pan)) = self.global_camera.take() {
            self.zoom = zoom;
            self.pan = pan;
        } else {
            self.fit = true;
        }
        self.mode = mode;
    }

    /// Centres the local graph on a page.
    fn focus_on(&mut self, id: &str) {
        self.focus = Some(id.to_owned());
        self.selected = Some(id.to_owned());
        self.fit = true;
    }

    /// Title, counts and the controls of the view.
    fn header(&mut self, ui: &mut egui::Ui, p: &theme::Palette) {
        let (title, subtitle) = match self.mode {
            Mode::Global => {
                let links = self
                    .edges
                    .iter()
                    .filter(|e| e.kind == EdgeKind::Link)
                    .count();
                let subs = self.edges.len() - links;
                (
                    "Grafo".to_owned(),
                    format!(
                        "{} {} · {} {} · {} {}",
                        self.nodes.len(),
                        if self.nodes.len() == 1 {
                            "página"
                        } else {
                            "páginas"
                        },
                        links,
                        if links == 1 { "enlace" } else { "enlaces" },
                        subs,
                        if subs == 1 {
                            "subpágina"
                        } else {
                            "subpáginas"
                        },
                    ),
                )
            }
            Mode::Local => match self.focus.as_deref().and_then(|id| self.index(id)) {
                Some(i) => {
                    let n = self.local_scene().items.len() - 1;
                    (
                        self.nodes[i].title.clone(),
                        format!(
                            "Grafo local · {n} {}",
                            if n == 1 { "conexión" } else { "conexiones" }
                        ),
                    )
                }
                None => (
                    "Grafo local".to_owned(),
                    "Ninguna página elegida".to_owned(),
                ),
            },
        };
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(
                    egui::RichText::new(title)
                        .font(FontId::new(22.0, FontFamily::Name(theme::SEMIBOLD.into())))
                        .color(p.text),
                );
                ui.label(egui::RichText::new(subtitle).color(p.text_muted));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = theme::space::SM;
                let (color, tip) = if self.inspector {
                    (p.text, "Ocultar el panel")
                } else {
                    (p.text_muted, "Mostrar el panel")
                };
                if icons::button(ui, true, icons::PANEL_RIGHT, color, tip, None).clicked() {
                    self.inspector = !self.inspector;
                }
                match self.mode {
                    Mode::Global => {
                        let focused = ui.memory(|m| m.has_focus(egui::Id::unique("graph_search")));
                        egui::Frame::new()
                            .stroke(egui::Stroke::new(
                                1.0,
                                if focused { p.accent } else { p.border_strong },
                            ))
                            .corner_radius(7.0)
                            .inner_margin(egui::Margin::symmetric(10, 0))
                            .show(ui, |ui| {
                                ui.allocate_ui_with_layout(
                                    vec2(196.0, 30.0),
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        ui.spacing_mut().item_spacing.x = 8.0;
                                        ui.label(
                                            icons::icon(icons::SEARCH)
                                                .size(14.0)
                                                .color(p.text_muted),
                                        );
                                        ui.add(
                                            egui::TextEdit::singleline(&mut self.query)
                                                .id(egui::Id::unique("graph_search"))
                                                .hint_text("Buscar en el grafo")
                                                .frame(egui::Frame::NONE)
                                                .margin(egui::Margin::ZERO)
                                                .desired_width(172.0),
                                        );
                                    },
                                );
                            });
                    }
                    Mode::Local => {
                        let depth = usize::from(self.depth.clamp(1, 2) - 1);
                        if let Some(chosen) = segmented(ui, p, "graph_depth", &["1", "2"], depth) {
                            self.depth = chosen as u8 + 1;
                            self.fit = true;
                        }
                        ui.label(
                            egui::RichText::new("Profundidad")
                                .size(12.0)
                                .color(p.text_muted),
                        );
                    }
                }
                let mode = usize::from(self.mode == Mode::Local);
                if let Some(chosen) = segmented(ui, p, "graph_mode", &["Global", "Local"], mode) {
                    self.set_mode(if chosen == 1 {
                        Mode::Local
                    } else {
                        Mode::Global
                    });
                }
            });
        });
    }

    /// The map: nodes, lines, the shelf and the floating controls.
    fn canvas(&mut self, ui: &mut egui::Ui, rect: Rect, p: &theme::Palette, act: &mut Actions) {
        let dark = ui.visuals().dark_mode;
        let bg = if dark {
            Color32::from_rgb(0x14, 0x14, 0x16)
        } else {
            p.sidebar_bg
        };
        let response = ui.interact(
            rect,
            egui::Id::unique("graph_canvas"),
            Sense::click_and_drag(),
        );
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, bg);
        let scene = self.scene();
        if self.fit && !scene.items.is_empty() {
            let mut bounds =
                Rect::from_points(&scene.items.iter().map(|i| i.pos).collect::<Vec<_>>());
            if let Some((shelf, _)) = scene.shelf {
                bounds = bounds.union(shelf);
            }
            let bounds = bounds.expand2(vec2(90.0, 70.0));
            self.zoom = ((rect.width() - 40.0) / bounds.width())
                .min((rect.height() - 40.0) / bounds.height())
                .clamp(0.1, 1.0);
            self.pan = -bounds.center().to_vec2() * self.zoom;
            self.fit = false;
        }
        // A quiet dotted canvas gives movement and scale a visual reference.
        let step = 20.0;
        let dot = if dark { p.border } else { p.border_strong };
        for x in 0..(rect.width() / step) as usize + 2 {
            for y in 0..(rect.height() / step) as usize + 2 {
                painter.circle_filled(
                    rect.min
                        + vec2(
                            x as f32 * step + self.pan.x.rem_euclid(step) - step,
                            y as f32 * step + self.pan.y.rem_euclid(step) - step,
                        ),
                    0.8,
                    dot,
                );
            }
        }
        if response.dragged() {
            self.pan += response.drag_delta();
        }
        if response.hovered() {
            let old = self.zoom;
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                self.zoom = (old * (scroll * 0.002).exp()).clamp(0.1, 3.0);
                if let Some(pointer) = ui.input(|i| i.pointer.hover_pos()) {
                    self.pan = pointer
                        - rect.center()
                        - (pointer - rect.center() - self.pan) * (self.zoom / old);
                }
            }
        }
        let to_screen = |world: Pos2| rect.center() + self.pan + world.to_vec2() * self.zoom;
        let positions: Vec<Pos2> = scene.items.iter().map(|i| to_screen(i.pos)).collect();

        // The shelf of pages without connections.
        if let Some((shelf, count)) = scene.shelf {
            let top = to_screen(shelf.left_top());
            let right = to_screen(shelf.right_top());
            painter.extend(egui::Shape::dashed_line(
                &[top, right],
                egui::Stroke::new(1.0, p.border_strong),
                4.0,
                4.0,
            ));
            painter.text(
                top + vec2(0.0, 8.0),
                egui::Align2::LEFT_TOP,
                format!("Sin conexiones · {count}"),
                FontId::proportional(12.0),
                p.text_muted,
            );
        }

        let hovered_item = ui
            .input(|i| i.pointer.hover_pos())
            .filter(|pos| rect.contains(*pos))
            .and_then(|pos| positions.iter().position(|p| p.distance(pos) < 12.0));
        let selected_item = self.selected.as_deref().and_then(|id| {
            scene
                .items
                .iter()
                .position(|item| self.nodes[item.node].id == id)
        });
        let highlight = hovered_item.or(selected_item);
        let query = silex_desktop::editor::fold(&self.query);
        let searching = self.mode == Mode::Global && !query.is_empty();
        let mut lit = vec![false; scene.items.len()];
        if let Some(h) = highlight {
            lit[h] = true;
            for &(a, b, _) in &scene.edges {
                if a == h {
                    lit[b] = true;
                }
                if b == h {
                    lit[a] = true;
                }
            }
        }
        if searching {
            for (k, item) in scene.items.iter().enumerate() {
                if silex_desktop::editor::fold(&self.nodes[item.node].title).contains(&query) {
                    lit[k] = true;
                }
            }
        }
        let dimming = (highlight.is_some() && self.mode == Mode::Global) || searching;

        for &(a, b, kind) in &scene.edges {
            let on = highlight.is_some_and(|h| h == a || h == b);
            let base = if kind == EdgeKind::Link {
                p.text_muted
            } else {
                p.text_faint
            };
            let color = if on {
                p.accent.gamma_multiply(0.8)
            } else if dimming {
                base.gamma_multiply(0.25)
            } else {
                base.gamma_multiply(0.7)
            };
            let stroke = egui::Stroke::new(if on { 1.5 } else { 1.0 }, color);
            let points = [positions[a], positions[b]];
            if kind == EdgeKind::Link {
                painter.line_segment(points, stroke);
            } else {
                painter.extend(egui::Shape::dashed_line(&points, stroke, 4.0, 3.0));
            }
        }

        let colors = root_colors(ui.ctx());
        let mut labels = Vec::<Rect>::new();
        // Lit nodes last, so their labels win the space.
        let mut order: Vec<usize> = (0..scene.items.len()).collect();
        order.sort_by_key(|&k| (lit[k], Some(k) == highlight));
        let mut label_order = order.clone();
        label_order.reverse();
        let mut label_ok = vec![false; scene.items.len()];
        let show_labels = self.zoom >= 0.5;
        for &k in &label_order {
            let item = &scene.items[k];
            let node = &self.nodes[item.node];
            if !(show_labels
                || lit[k]
                || node.degree >= 3
                || item.level <= 1 && self.mode == Mode::Local)
            {
                continue;
            }
            let pos = positions[k];
            let size = ui.fonts_mut(|f| {
                f.layout_job(label_job(
                    &node.title,
                    FontId::proportional(12.0),
                    p.text,
                    LABEL_WIDTH,
                ))
                .size()
            });
            let radius = node_radius(node, item.level, self.mode);
            let r = Rect::from_min_size(pos + vec2(-size.x / 2.0, radius + 6.0), size)
                .expand2(vec2(6.0, 2.0));
            if Some(k) == highlight || !labels.iter().any(|l| l.intersects(r)) {
                labels.push(r);
                label_ok[k] = true;
            }
        }
        let mut clicked = None;
        for &k in &order {
            let item = &scene.items[k];
            let node = &self.nodes[item.node];
            let pos = positions[k];
            if !rect.expand(40.0).contains(pos) {
                continue;
            }
            let is_selected = Some(k) == selected_item;
            let faded = dimming && !lit[k];
            let radius = node_radius(node, item.level, self.mode);
            let mut fill = match (self.color_by_root, node.root) {
                (true, Some(root)) => colors[root % colors.len()],
                _ => {
                    if item.level >= 2 {
                        p.text_faint
                    } else {
                        p.text_muted
                    }
                }
            };
            if lit[k] && !self.color_by_root || (self.mode == Mode::Local && item.level == 1) {
                fill = p.text;
            }
            if is_selected || (self.mode == Mode::Local && item.level == 0) {
                fill = p.accent;
                painter.circle_filled(pos, radius + 5.0, p.accent.gamma_multiply(0.22));
            }
            let alpha = if faded { 0.3 } else { 1.0 };
            if node.degree == 0 && !is_selected {
                let stroke = egui::Stroke::new(1.5, fill.gamma_multiply(alpha));
                if node.kind == PageKind::Database {
                    painter.rect_stroke(
                        Rect::from_center_size(pos, Vec2::splat(radius * 2.0)),
                        2.0,
                        stroke,
                        egui::StrokeKind::Inside,
                    );
                } else {
                    painter.circle_stroke(pos, radius - 0.75, stroke);
                }
            } else if node.kind == PageKind::Database {
                painter.rect_filled(
                    Rect::from_center_size(pos, Vec2::splat(radius * 2.0)),
                    2.0,
                    fill.gamma_multiply(alpha),
                );
            } else {
                painter.circle_filled(pos, radius, fill.gamma_multiply(alpha));
            }
            if label_ok[k] {
                let strong = is_selected || (self.mode == Mode::Local && item.level == 0);
                let color = if strong {
                    p.text
                } else if lit[k] {
                    p.sidebar_text
                } else {
                    p.text_muted
                };
                let font = if strong {
                    FontId::new(12.0, FontFamily::Name(theme::SEMIBOLD.into()))
                } else {
                    FontId::proportional(12.0)
                };
                let galley = painter.layout_job(label_job(
                    &node.title,
                    font,
                    color.gamma_multiply(alpha),
                    LABEL_WIDTH,
                ));
                let r = Rect::from_min_size(
                    pos + vec2(-galley.size().x / 2.0, radius + 6.0),
                    galley.size(),
                )
                .expand2(vec2(6.0, 2.0));
                painter.rect_filled(r, 4.0, bg.gamma_multiply(0.88));
                painter.galley(r.min + vec2(6.0, 2.0), galley, color);
            }
            let hit = ui.interact(
                Rect::from_center_size(pos, Vec2::splat(24.0)),
                egui::Id::unique("graph_node").with(&node.id),
                Sense::click(),
            );
            hit.widget_info(|| {
                egui::WidgetInfo::labeled(egui::Role::Button, true, format!("Abrir {}", node.title))
            });
            if hit.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if hit.double_clicked()
                || (hit.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
            {
                act.open = Some(node.id.clone());
            } else if hit.clicked() {
                clicked = Some(node.id.clone());
            }
            hit.on_hover_text(format!(
                "{}\n{} {} · doble clic para abrir",
                node.title,
                node.degree,
                if node.degree == 1 {
                    "conexión"
                } else {
                    "conexiones"
                }
            ));
        }
        if let Some(id) = clicked {
            match self.mode {
                Mode::Global => {
                    self.selected = if self.selected.as_deref() == Some(id.as_str()) {
                        None
                    } else {
                        Some(id)
                    };
                }
                Mode::Local => self.focus_on(&id),
            }
        }
        if response.clicked() && hovered_item.is_none() && self.mode == Mode::Global {
            self.selected = None;
        }
        if response.hovered() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.selected = None;
        }

        // Empty states.
        let empty = if self.nodes.is_empty() {
            Some("Crea páginas y enlázalas con [[ para ver cómo se conectan")
        } else if self.mode == Mode::Local && self.focus.is_none() {
            Some("Selecciona una página en el grafo global, o abre el grafo local desde una página")
        } else if self.mode == Mode::Local && scene.items.len() == 1 {
            Some("Esta página aún no tiene conexiones. Escribe [[ en ella para enlazar otra")
        } else {
            None
        };
        if let Some(text) = empty {
            let at = if scene.items.len() == 1 {
                positions[0] + vec2(0.0, 48.0)
            } else {
                rect.center()
            };
            painter.text(
                at,
                egui::Align2::CENTER_CENTER,
                text,
                FontId::proportional(13.0),
                p.text_muted,
            );
        }

        self.legend(&painter, rect, p);
        self.zoom_controls(ui, rect, p, act);
    }

    /// Top-left: what each mark means.
    fn legend(&self, painter: &egui::Painter, rect: Rect, p: &theme::Palette) {
        let font = FontId::proportional(12.0);
        let entries = ["Página", "Base de datos", "Enlace", "Subpágina"];
        let widths: Vec<f32> = entries
            .iter()
            .map(|e| {
                painter
                    .layout_no_wrap((*e).to_owned(), font.clone(), p.text_muted)
                    .size()
                    .x
            })
            .collect();
        let swatch = [8.0, 9.0, 18.0, 18.0];
        let width = 12.0
            + widths
                .iter()
                .zip(swatch)
                .map(|(w, s)| s + 6.0 + w)
                .sum::<f32>()
            + 14.0 * 3.0
            + 12.0;
        let r = Rect::from_min_size(rect.min + vec2(16.0, 16.0), vec2(width, 32.0));
        self.floating(painter, r, p);
        let mut x = r.left() + 12.0;
        let y = r.center().y;
        for (k, entry) in entries.iter().enumerate() {
            let s = swatch[k];
            match k {
                0 => {
                    painter.circle_filled(pos2(x + 4.0, y), 4.0, p.text_muted);
                }
                1 => {
                    painter.rect_filled(
                        Rect::from_center_size(pos2(x + 4.5, y), Vec2::splat(9.0)),
                        2.0,
                        p.text_muted,
                    );
                }
                2 => {
                    painter.line_segment(
                        [pos2(x, y), pos2(x + s, y)],
                        egui::Stroke::new(1.0, p.text_muted),
                    );
                }
                _ => {
                    painter.extend(egui::Shape::dashed_line(
                        &[pos2(x, y), pos2(x + s, y)],
                        egui::Stroke::new(1.0, p.text_muted),
                        4.0,
                        3.0,
                    ));
                }
            }
            x += s + 6.0;
            painter.text(
                pos2(x, y),
                egui::Align2::LEFT_CENTER,
                *entry,
                font.clone(),
                p.text_muted,
            );
            x += widths[k] + 14.0;
        }
    }

    /// A raised, rounded box over the canvas.
    fn floating(&self, painter: &egui::Painter, r: Rect, p: &theme::Palette) {
        painter.add(
            egui::epaint::Shadow {
                offset: [0, 6],
                blur: 18,
                spread: 0,
                color: Color32::from_black_alpha(40),
            }
            .as_shape(r, 8.0),
        );
        painter.rect(
            r,
            8.0,
            p.surface,
            egui::Stroke::new(1.0, p.border_strong),
            egui::StrokeKind::Inside,
        );
    }

    /// Bottom-right: zoom out, the zoom, zoom in, fit and rearrange.
    fn zoom_controls(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        p: &theme::Palette,
        act: &mut Actions,
    ) {
        let size = vec2(
            3.0 + 28.0 + 44.0 + 28.0 + 7.0 + 28.0 + 28.0 + 3.0 + 4.0 * 2.0,
            34.0,
        );
        let r = Rect::from_min_size(rect.max - size - vec2(16.0, 16.0), size);
        self.floating(ui.painter(), r, p);
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(r.shrink2(vec2(3.0, 3.0)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        child.spacing_mut().item_spacing.x = 2.0;
        if icons::button(&mut child, true, icons::MINUS, p.text_muted, "Alejar", None).clicked() {
            self.zoom = (self.zoom / 1.2).max(0.1);
        }
        child.add_sized(
            [44.0, 28.0],
            egui::Label::new(
                egui::RichText::new(format!("{} %", (self.zoom * 100.0).round()))
                    .size(12.0)
                    .color(p.sidebar_text),
            ),
        );
        if icons::button(&mut child, true, icons::PLUS, p.text_muted, "Acercar", None).clicked() {
            self.zoom = (self.zoom * 1.2).min(3.0);
        }
        let (sep, _) = child.allocate_exact_size(vec2(7.0, 16.0), Sense::hover());
        child.painter().vline(
            sep.center().x,
            sep.y_range(),
            egui::Stroke::new(1.0, p.border_strong),
        );
        if icons::button(&mut child, true, icons::SCAN, p.text_muted, "Encajar", None).clicked() {
            self.fit = true;
        }
        if icons::button(
            &mut child,
            true,
            icons::ROTATE_CCW,
            p.text_muted,
            "Actualizar y reorganizar",
            None,
        )
        .clicked()
        {
            act.refresh = true;
        }
    }

    /// The panel on the right: the selected page and the view's filters.
    fn inspector(&mut self, ui: &mut egui::Ui, p: &theme::Palette, act: &mut Actions) {
        ui.spacing_mut().item_spacing.y = 2.0;
        let selected = self.selected.as_deref().and_then(|id| self.index(id));
        // Filters, at the bottom of the panel.
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.add_space(4.0);
            let color = usize::from(self.color_by_root);
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                if let Some(chosen) =
                    segmented(ui, p, "graph_color", &["Neutro", "Por página raíz"], color)
                {
                    self.color_by_root = chosen == 1;
                }
            });
            section(ui, p, "Color", None);
            ui.add_space(theme::space::SM);
            for (value, label) in [
                (&mut self.show_databases, "Bases de datos y sus filas"),
                (&mut self.show_orphans, "Páginas sin conexiones"),
                (&mut self.show_subpages, "Líneas de subpágina"),
            ] {
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    if ui.checkbox(value, label).changed() {
                        self.fit = true;
                    }
                });
            }
            section(ui, p, "Mostrar", None);
            ui.add_space(theme::space::SM);
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
            ui.painter()
                .hline(r.x_range(), r.center().y, egui::Stroke::new(1.0, p.border));
            ui.add_space(theme::space::SM);
            // The selected page, in the space left above the filters.
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("graph_inspector")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let Some(i) = selected else {
                            ui.add_space(4.0);
                            hint(ui, p, "Selecciona una página para ver sus conexiones.");
                            return;
                        };
                        let node = &self.nodes[i];
                        let subs: Vec<usize> = self
                            .edges
                            .iter()
                            .filter(|e| e.kind == EdgeKind::Sub && e.a == i)
                            .map(|e| e.b)
                            .collect();
                        let mut links: Vec<usize> = self
                            .edges
                            .iter()
                            .filter(|e| e.kind == EdgeKind::Link && (e.a == i || e.b == i))
                            .map(|e| if e.a == i { e.b } else { e.a })
                            .collect();
                        links.sort_unstable();
                        links.dedup();
                        let mut pick = None;
                        ui.horizontal(|ui| {
                            ui.add_space(8.0);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(&node.title)
                                            .font(FontId::new(
                                                15.0,
                                                FontFamily::Name(theme::SEMIBOLD.into()),
                                            ))
                                            .color(p.text),
                                    )
                                    .truncate(),
                                );
                                let kind = if node.kind == PageKind::Database {
                                    "Base de datos"
                                } else {
                                    "Página"
                                };
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{kind} · {} {}",
                                        node.degree,
                                        if node.degree == 1 {
                                            "conexión"
                                        } else {
                                            "conexiones"
                                        }
                                    ))
                                    .size(12.0)
                                    .color(p.text_muted),
                                );
                            });
                        });
                        ui.add_space(theme::space::MD);
                        ui.horizontal(|ui| {
                            ui.add_space(8.0);
                            ui.spacing_mut().item_spacing.x = 6.0;
                            let width = (ui.available_width() - 8.0 - 6.0) / 2.0;
                            let open = egui::Button::new(
                                egui::RichText::new("Abrir").color(p.bg).strong(),
                            )
                            .fill(p.text)
                            .min_size(vec2(width, 28.0));
                            if ui.add(open).clicked() {
                                act.open = Some(node.id.clone());
                            }
                            let local = if self.mode == Mode::Local {
                                "Centrar aquí"
                            } else {
                                "Grafo local"
                            };
                            if ui
                                .add(egui::Button::new(local).min_size(vec2(width, 28.0)))
                                .clicked()
                            {
                                pick = Some((node.id.clone(), true));
                            }
                        });
                        ui.add_space(theme::space::MD);
                        let divider = |ui: &mut egui::Ui| {
                            ui.add_space(4.0);
                            let (r, _) = ui.allocate_exact_size(
                                vec2(ui.available_width(), 1.0),
                                Sense::hover(),
                            );
                            ui.painter().hline(
                                r.x_range(),
                                r.center().y,
                                egui::Stroke::new(1.0, p.border),
                            );
                            ui.add_space(8.0);
                        };
                        divider(ui);
                        if let Some(parent) = node.parent {
                            section(ui, p, "Dentro de", None);
                            let parent = &self.nodes[parent];
                            if row(ui, p, &parent.title, parent.kind, false).clicked() {
                                pick = Some((parent.id.clone(), false));
                            }
                            ui.add_space(theme::space::SM);
                        }
                        if !subs.is_empty() {
                            let title = if node.kind == PageKind::Database {
                                "Filas"
                            } else {
                                "Subpáginas"
                            };
                            section(ui, p, title, Some(subs.len()));
                            for &s in subs.iter().take(INSPECTOR_ROWS) {
                                let sub = &self.nodes[s];
                                if row(ui, p, &sub.title, sub.kind, false).clicked() {
                                    pick = Some((sub.id.clone(), false));
                                }
                            }
                            if subs.len() > INSPECTOR_ROWS {
                                ui.horizontal(|ui| {
                                    ui.add_space(8.0);
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "y {} más",
                                            subs.len() - INSPECTOR_ROWS
                                        ))
                                        .size(12.0)
                                        .color(p.text_muted),
                                    );
                                });
                            }
                            ui.add_space(theme::space::SM);
                        }
                        section(ui, p, "Enlaces", Some(links.len()));
                        if links.is_empty() {
                            hint(ui, p, NO_LINKS);
                        }
                        for &l in links.iter().take(INSPECTOR_ROWS) {
                            let other = &self.nodes[l];
                            if row(ui, p, &other.title, other.kind, true).clicked() {
                                pick = Some((other.id.clone(), false));
                            }
                        }
                        if let Some((id, local)) = pick {
                            if local || self.mode == Mode::Local {
                                self.set_mode(Mode::Local);
                                self.focus_on(&id);
                            } else {
                                self.selected = Some(id);
                            }
                        }
                    });
            });
        });
    }
}

#[cfg(test)]
impl Graph {
    pub(super) fn select_for_test(&mut self, id: &str) {
        self.selected = Some(id.to_owned());
    }

    pub(super) fn depth_for_test(&mut self, depth: u8) {
        self.depth = depth;
        self.fit = true;
    }

    /// Titles of the local graph's pages, focus first.
    pub(super) fn local_titles(&self) -> Vec<String> {
        self.local_scene()
            .items
            .iter()
            .map(|i| self.nodes[i.node].title.clone())
            .collect()
    }

    /// Whether the graph shows the local view.
    pub(super) fn is_local(&self) -> bool {
        self.mode == Mode::Local
    }

    /// (links, subpage lines) in the graph.
    pub(super) fn edge_counts(&self) -> (usize, usize) {
        let links = self
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Link)
            .count();
        (links, self.edges.len() - links)
    }

    /// Pages placed on the shelf of pages without connections.
    pub(super) fn shelf_titles(&self) -> Vec<String> {
        let mut titles: Vec<_> = self
            .nodes
            .iter()
            .filter(|n| n.degree == 0)
            .map(|n| n.title.clone())
            .collect();
        titles.sort();
        titles
    }
}

/// Radius of a node: hubs are bigger; the local focus is the biggest.
fn node_radius(node: &Node, level: u8, mode: Mode) -> f32 {
    if mode == Mode::Local && level == 0 {
        return 9.0;
    }
    if node.kind == PageKind::Database {
        return 5.0;
    }
    if node.degree == 0 {
        return 4.5;
    }
    4.0 + (node.degree.min(8) as f32) * 0.6
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

    /// Opens the graph centred on a page (the page header's button).
    pub(super) fn open_local_graph(&mut self, page_id: &str) -> anyhow::Result<()> {
        let page_id = page_id.to_owned();
        self.open_graph(&egui::Context::default())?;
        if let Some(graph) = self.graph.as_mut() {
            graph.set_mode(Mode::Local);
            graph.focus_on(&page_id);
        }
        Ok(())
    }

    pub(super) fn rebuild_graph(&mut self) -> anyhow::Result<()> {
        let links = self.db.graph_edges()?;
        let mut nodes: Vec<_> = self
            .pages
            .iter()
            .map(|page| Node {
                id: page.id.clone(),
                title: super::display_title(&page.title).to_owned(),
                kind: page.kind,
                parent: None,
                database: page.kind == PageKind::Database,
                root: None,
                position: Pos2::ZERO,
                degree: 0,
            })
            .collect();
        let indices: HashMap<_, _> = nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id.clone(), i))
            .collect();
        let mut edges = Vec::new();
        for (i, page) in self.pages.iter().enumerate() {
            if let Some(parent) = page.parent_id.as_ref().and_then(|id| indices.get(id)) {
                nodes[i].parent = Some(*parent);
                edges.push(Edge {
                    a: *parent,
                    b: i,
                    kind: EdgeKind::Sub,
                });
            }
        }
        for i in 0..nodes.len() {
            if let Some(parent) = nodes[i].parent
                && nodes[parent].kind == PageKind::Database
            {
                nodes[i].database = true;
            }
        }
        // The root of each tree (guarded against a cycle in bad data).
        let mut roots = Vec::new();
        for i in 0..nodes.len() {
            let mut top = i;
            let mut hops = 0;
            while let Some(parent) = nodes[top].parent {
                top = parent;
                hops += 1;
                if hops > nodes.len() {
                    break;
                }
            }
            roots.push(top);
        }
        let mut groups: HashMap<usize, usize> = HashMap::new();
        for i in 0..nodes.len() {
            let root = roots[i];
            let has_children = edges.iter().any(|e| e.a == root);
            if has_children {
                let next = groups.len();
                nodes[i].root = Some(*groups.entry(root).or_insert(next));
            }
        }
        for (a, b) in &links {
            if let (Some(&a), Some(&b)) = (indices.get(a), indices.get(b))
                && !edges.iter().any(|e| {
                    e.kind == EdgeKind::Link && ((e.a, e.b) == (a, b) || (e.a, e.b) == (b, a))
                })
            {
                edges.push(Edge {
                    a,
                    b,
                    kind: EdgeKind::Link,
                });
            }
        }
        let shelf = arrange(&mut nodes, &edges);
        let old = self.graph.take();
        self.graph = Some(match old {
            Some(old) => Graph {
                nodes,
                edges,
                shelf,
                ..old
            },
            None => Graph {
                nodes,
                edges,
                shelf,
                zoom: 1.0,
                pan: Vec2::ZERO,
                query: String::new(),
                selected: None,
                fit: true,
                mode: Mode::Global,
                focus: None,
                depth: 1,
                global_camera: None,
                inspector: true,
                show_subpages: true,
                show_orphans: true,
                show_databases: true,
                color_by_root: false,
            },
        });
        Ok(())
    }

    pub(super) fn graph_view(&mut self, ui: &mut egui::Ui) {
        let Some(mut graph) = self.graph.take() else {
            return;
        };
        let mut act = Actions::default();
        let p = *theme::current(ui.ctx());
        ui.add_space(theme::space::LG);
        ui.horizontal(|ui| {
            ui.add_space(theme::space::XL);
            ui.vertical(|ui| {
                ui.set_width(ui.available_width() - theme::space::XL);
                graph.header(ui, &p);
            });
        });
        ui.add_space(theme::space::LG);
        // The canvas fills the panel to its edges, under a 1 px line.
        let avail = ui.available_rect_before_wrap();
        let panel = ui.clip_rect();
        let area = Rect::from_min_max(
            pos2(panel.left().max(avail.left() - 24.0), avail.top()),
            pos2(panel.right().min(avail.right() + 24.0), panel.bottom()),
        );
        ui.allocate_rect(avail, Sense::hover());
        let canvas = if graph.inspector {
            Rect::from_min_max(area.min, pos2(area.right() - INSPECTOR, area.bottom()))
        } else {
            area
        };
        graph.canvas(ui, canvas, &p, &mut act);
        ui.painter_at(area).hline(
            area.x_range(),
            area.top() + 0.5,
            egui::Stroke::new(1.0, p.border),
        );
        if graph.inspector {
            let side = Rect::from_min_max(pos2(canvas.right(), area.top() + 1.0), area.max);
            ui.painter_at(area).rect_filled(side, 0.0, p.bg);
            ui.painter_at(area).vline(
                side.left() + 0.5,
                side.y_range(),
                egui::Stroke::new(1.0, p.border),
            );
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(side.shrink2(vec2(12.0, 16.0)))
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            child.set_clip_rect(side);
            graph.inspector(&mut child, &p, &mut act);
        }
        self.graph = Some(graph);
        if act.refresh {
            let result = self.rebuild_graph();
            if let Some(graph) = self.graph.as_mut() {
                graph.fit = true;
            }
            self.report(result);
        }
        if let Some(id) = act.open {
            let result = self.open_in_new_tab(&id);
            self.report(result);
        }
    }
}
