//! Attachments in the editor (H7.1, ADR 0007): dropping files on a page,
//! the file block (a card with «Abrir» and «Guardar una copia»), and the
//! results of the `silex-attach` worker.
//!
//! The worker only stores files. The blocks are added here, to the open
//! document in memory, like any other edit: the autosave writes them and
//! Cmd+Z takes them away (the file stays, unused). If the page was left
//! meanwhile, the blocks are added to it in the database.
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use eframe::egui;
use silex_desktop::attach_job::{self, AttachWorker, Folders, Outcome, Request};
use silex_desktop::attachment::{Attachment, FileKind, format_size, has_preview};
use silex_desktop::model::{Block, BlockKind, PageKind};

use super::*;
use super::{icons, theme};

/// Height of a file card (and of the caption under an image).
const CARD_HEIGHT: f32 = 44.0;
/// Room around an image preview in its card.
const IMAGE_PAD: f32 = 8.0;
/// Tallest an image preview is shown (points).
const MAX_IMAGE_HEIGHT: f32 = 480.0;

/// An image's preview (H7.2): decoded by the attach worker, kept as a
/// texture while its page is open.
pub(super) enum PreviewState {
    Loading,
    Ready(egui::TextureHandle),
    Failed(String),
}

/// Most memory the open page's previews may hold (RGBA bytes). Past it,
/// the ones out of sight are dropped, and decoded again when they come
/// back.
pub(super) const PREVIEW_BUDGET: usize = 96 * 1024 * 1024;

impl PreviewState {
    fn bytes(&self) -> usize {
        match self {
            PreviewState::Ready(texture) => {
                let [w, h] = texture.size();
                w * h * 4
            }
            _ => 0,
        }
    }
}

/// The size an image of `w` × `h` pixels is shown at: one point per pixel,
/// scaled down to fit `max_width` and [`MAX_IMAGE_HEIGHT`].
fn fit_image(w: u32, h: u32, max_width: f32) -> egui::Vec2 {
    let (w, h) = (w.max(1) as f32, h.max(1) as f32);
    let scale = (max_width.max(1.0) / w).min(MAX_IMAGE_HEIGHT / h).min(1.0);
    egui::vec2(w * scale, h * scale)
}

/// The attachment work: a thread for a database file; done in place for
/// an in-memory database (tests).
#[derive(Default)]
pub(super) struct Files {
    /// The open page's attachments, by id (name, type, size).
    pub(super) attachments: HashMap<String, Attachment>,
    worker: Option<AttachWorker>,
    /// Where copies are written (`Folders::standard` unless a test sets it).
    pub(super) folders: Option<Folders>,
    /// Files being stored right now.
    pub(super) adding: usize,
    /// Opens a file or folder; tests replace it.
    pub(super) opener: Option<fn(&Path) -> Result<()>>,
    /// Outcomes of work done in place, handled like the worker's.
    done: Vec<Outcome>,
    /// The copies of an earlier session were deleted (once).
    cleared: bool,
    /// Previews of the open page's images, by attachment id.
    pub(super) previews: HashMap<String, PreviewState>,
    /// Images drawn in sight this frame that have no preview yet.
    pub(super) wanted: Vec<String>,
    /// Images drawn in sight this frame (kept when memory is short).
    pub(super) seen: std::collections::HashSet<String>,
    /// A smaller [`PREVIEW_BUDGET`], for tests.
    pub(super) budget: Option<usize>,
    /// Open the file dialog after this frame (H7.3). The inner id is the
    /// block «/archivo» was typed in: the files go after it, or in its
    /// place if it was left empty.
    pub(super) pick: Option<Option<String>>,
    /// That block, while its files are on their way.
    replace: Option<String>,
    /// Chooses files; tests replace it (the default opens the system's
    /// dialog, never in tests).
    pub(super) picker: Option<fn() -> Vec<PathBuf>>,
    /// «Abrir», «Guardar una copia»… chosen this frame (action, block id),
    /// sent in `tick_files`, which has the egui context.
    pub(super) pending: Vec<(BlockAction, String)>,
}

/// The app's folders. Tests never touch the user's: they get a temporary
/// folder of their own.
#[cfg(not(test))]
fn default_folders() -> Result<Folders> {
    Folders::standard()
}

#[cfg(test)]
fn default_folders() -> Result<Folders> {
    let root = std::env::temp_dir().join(format!("silex-app-files-{}", uuid::Uuid::new_v4()));
    Ok(Folders {
        opened: root.join("abiertos"),
        saved: root.join("Exportaciones"),
    })
}

/// The system's dialog to choose files (`rfd`, ADR 0007). Tests never open
/// one.
#[cfg(not(test))]
fn default_picker() -> Vec<PathBuf> {
    rfd::FileDialog::new()
        .set_title("Adjuntar archivos")
        .pick_files()
        .unwrap_or_default()
}

#[cfg(test)]
fn default_picker() -> Vec<PathBuf> {
    Vec::new()
}

/// Opens a file with its app, or a folder. Tests never open anything.
#[cfg(not(test))]
fn default_opener(path: &Path) -> Result<()> {
    silex_desktop::export::open_in_file_manager(path)
}

#[cfg(test)]
fn default_opener(_: &Path) -> Result<()> {
    Ok(())
}

/// Icon of a file by its type.
fn file_icon(kind: FileKind) -> &'static str {
    match kind {
        FileKind::Image => icons::FILE_IMAGE,
        FileKind::Pdf => icons::FILE_PDF,
        FileKind::Archive => icons::FILE_ZIP,
        FileKind::Audio => icons::FILE_AUDIO,
        FileKind::Video => icons::FILE_VIDEO,
        FileKind::Other => icons::FILE,
    }
}

/// A file block: a card with its icon, name and size, and its buttons.
/// The card takes the block's focus id, so the arrow keys reach it like a
/// text block; Intro opens it (see `run_editor_key`). Returns the card's
/// response and the button clicked.
pub(super) fn file_card(
    ui: &mut egui::Ui,
    id: egui::Id,
    block: &Block,
    attachment: Option<&Attachment>,
    preview: Option<&PreviewState>,
) -> (egui::Response, Option<BlockAction>) {
    let p = theme::current(ui.ctx());
    // A PNG or JPEG of known size shows itself above its caption; its room
    // is known before it is decoded, so nothing moves when it arrives.
    let width = ui.available_width();
    let image = attachment
        .filter(|a| has_preview(&a.mime))
        .and_then(|a| Some((a.width?, a.height?)));
    let image_size = image.map(|(w, h)| fit_image(w, h, width - 2.0 * IMAGE_PAD));
    let height = CARD_HEIGHT + image_size.map_or(0.0, |s| s.y + IMAGE_PAD);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let response = ui.interact(rect, id, egui::Sense::click());
    let size = attachment.map(|a| format_size(a.size));
    let executable = attachment.is_some_and(Attachment::is_executable);
    let label = match (image, &size) {
        (Some((w, h)), Some(size)) => format!("Imagen «{}», {w} × {h}, {size}", block.content),
        (_, Some(size)) => format!("Archivo «{}», {size}", block.content),
        _ => format!("Archivo «{}»", block.content),
    };
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));

    let radius = egui::CornerRadius::same(theme::radius::MD);
    let fill = if response.hovered() || response.has_focus() {
        p.subtle
    } else {
        p.bg
    };
    ui.painter().rect_filled(rect, radius, fill);
    ui.painter().rect_stroke(
        rect,
        radius,
        egui::Stroke::new(1.0, p.border),
        egui::StrokeKind::Inside,
    );
    focus_ring(ui, rect, &response);

    if let Some(size) = image_size {
        let area = egui::Rect::from_min_size(rect.min + egui::vec2(IMAGE_PAD, IMAGE_PAD), size);
        paint_preview(ui, area, preview);
    }
    // The caption: the bottom row of the card.
    let rect = egui::Rect::from_min_max(
        egui::pos2(rect.left(), rect.bottom() - CARD_HEIGHT),
        rect.max,
    );
    let kind = attachment.map_or(FileKind::Other, Attachment::kind);
    let icon = ui.painter().text(
        egui::pos2(rect.left() + 12.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        file_icon(kind),
        icons::font(theme::icon::LG),
        p.text_muted,
    );

    // Buttons on the right first; the name takes the room left.
    let mut action = None;
    let inner = egui::Rect::from_min_max(
        egui::pos2(icon.right() + theme::space::SM, rect.top()),
        egui::pos2(rect.right() - theme::space::SM, rect.bottom()),
    );
    let ui = &mut inside(ui, inner, egui::Layout::right_to_left(egui::Align::Center));
    let button = |ui: &mut egui::Ui, icon: &str, text: &str, name: String| {
        ui.add(
            egui::Button::new((
                icons::icon(icon).color(p.text_muted),
                egui::RichText::new(text).small().color(p.text_muted),
            ))
            .frame_when_inactive(false),
        )
        .accessible_name(name)
    };
    let save = button(
        ui,
        icons::DOWNLOAD,
        " Guardar una copia",
        format!("Guardar una copia de «{}»", block.content),
    )
    .on_hover_text("En la carpeta de exportaciones");
    if save.clicked() {
        action = Some(BlockAction::SaveFile);
    }
    let open = if executable {
        button(
            ui,
            icons::FOLDER_OPEN,
            " Mostrar en la carpeta",
            format!("Mostrar «{}» en la carpeta", block.content),
        )
        .on_hover_text("Silex no abre programas ni scripts")
    } else {
        button(
            ui,
            icons::OPEN_EXTERNAL,
            " Abrir",
            format!("Abrir «{}»", block.content),
        )
        .on_hover_text("Con la aplicación del sistema; los cambios no vuelven a Silex")
    };
    if open.clicked() {
        action = Some(if executable {
            BlockAction::RevealFile
        } else {
            BlockAction::OpenFile
        });
    }
    if response.double_clicked() && action.is_none() {
        action = Some(if executable {
            BlockAction::RevealFile
        } else {
            BlockAction::OpenFile
        });
    }

    // Name and size, cut to the room left.
    let text_rect = egui::Rect::from_min_max(
        inner.min,
        egui::pos2(ui.min_rect().left() - theme::space::SM, inner.max.y),
    );
    let mut job = egui::text::LayoutJob::default();
    job.append(
        &block.content,
        0.0,
        egui::TextFormat::simple(egui::TextStyle::Body.resolve(ui.style()), p.text),
    );
    if let Some(size) = size {
        job.append(
            &format!("  {size}"),
            0.0,
            egui::TextFormat::simple(egui::TextStyle::Small.resolve(ui.style()), p.text_muted),
        );
    }
    job.wrap = egui::text::TextWrapping::truncate_at_width(text_rect.width().max(0.0));
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let at = egui::Align2::LEFT_CENTER
        .align_size_within_rect(galley.size(), text_rect)
        .min;
    ui.painter().galley(at, galley, p.text);
    (response, action)
}

/// An image in `area`: its texture, or a placeholder while it loads or
/// when it cannot be shown.
fn paint_preview(ui: &egui::Ui, area: egui::Rect, preview: Option<&PreviewState>) {
    let p = theme::current(ui.ctx());
    let radius = egui::CornerRadius::same(theme::radius::SM);
    match preview {
        Some(PreviewState::Ready(texture)) => {
            egui::Image::from_texture(texture)
                .corner_radius(radius)
                .paint_at(ui, area);
        }
        state => {
            ui.painter().rect_filled(area, radius, p.subtle);
            let text = match state {
                Some(PreviewState::Failed(message)) => message.as_str(),
                _ => "Cargando la imagen…",
            };
            ui.painter().text(
                area.center(),
                egui::Align2::CENTER_CENTER,
                text,
                egui::TextStyle::Small.resolve(ui.style()),
                p.text_muted,
            );
        }
    }
}

impl SilexApp {
    /// Reads the open page's attachments (name, type, size).
    pub(super) fn load_attachments(&mut self, page: &str) -> Result<()> {
        self.files.attachments = self
            .db
            .page_attachments(page)?
            .into_iter()
            .map(|a| (a.id.clone(), a))
            .collect();
        // Another page: its previews are decoded when they come into sight.
        self.files.previews.clear();
        self.files.wanted.clear();
        Ok(())
    }

    /// Where copies go: the standard folders, unless a test chose others.
    fn file_folders(&mut self) -> Result<Folders> {
        if let Some(folders) = &self.files.folders {
            return Ok(folders.clone());
        }
        let folders = default_folders()?;
        self.files.folders = Some(folders.clone());
        Ok(folders)
    }

    /// Sends attachment work to the worker (started on first use), or does
    /// it in place for an in-memory database.
    pub(super) fn file_request(&mut self, ctx: &egui::Context, request: Request) -> Result<()> {
        let folders = self.file_folders()?;
        if !std::mem::replace(&mut self.files.cleared, true) {
            // Copies opened in an earlier session; failing is not worth a stop.
            let _ = attach_job::clear_opened(&folders);
        }
        let Some(path) = self.db.path() else {
            let opener = self.files.opener.unwrap_or(default_opener);
            let outcome = attach_job::handle(&mut self.db, &folders, request, &opener);
            self.files.done.push(outcome);
            return Ok(());
        };
        if self.files.worker.is_none() {
            let repaint = ctx.clone();
            self.files.worker = Some(AttachWorker::spawn(path, folders, move || {
                repaint.request_repaint();
            })?);
        }
        if let Some(worker) = &self.files.worker {
            worker.request(request)?;
        }
        Ok(())
    }

    /// Dropped files that are not Markdown: attached to the open page,
    /// after the block being edited (or at the end).
    pub(super) fn attach_dropped(&mut self, ctx: &egui::Context, paths: Vec<PathBuf>) {
        let Some((page, after)) = self.attach_target() else {
            return;
        };
        self.files.adding += paths.len();
        let result = self.file_request(ctx, Request::Add { page, after, paths });
        if result.is_err() {
            self.files.adding = 0;
        }
        self.report(result);
    }

    /// The file dialog («/archivo», «Adjuntar archivos…»). It is modal and
    /// waits for the user, outside of the frame's drawing. Nothing chosen,
    /// nothing done.
    fn pick_files(&mut self, ctx: &egui::Context, replace: Option<String>) {
        let paths = self.files.picker.unwrap_or(default_picker)();
        if paths.is_empty() {
            return;
        }
        let Some((page, mut after)) = self.attach_target() else {
            return;
        };
        if let Some(block) = replace {
            after = Some(block.clone());
            self.files.replace = Some(block);
        }
        self.files.adding += paths.len();
        let result = self.file_request(ctx, Request::Add { page, after, paths });
        if result.is_err() {
            self.files.adding = 0;
        }
        self.report(result);
    }

    /// An image pasted with Cmd+V (egui sends it when the clipboard has an
    /// image and no text): stored as «Imagen pegada.png» after the block
    /// being edited (H7.2). Taken before any widget sees it.
    pub(super) fn take_pasted_image(&mut self, ctx: &egui::Context) {
        let mut pasted = None;
        take_event(ctx, |event| match event {
            egui::Event::PasteImage(image) => {
                pasted = Some(image.clone());
                true
            }
            _ => false,
        });
        let Some(image) = pasted else {
            return;
        };
        let Some((page, after)) = self.attach_target() else {
            return;
        };
        let [width, height] = image.size;
        let rgba: Vec<u8> = image
            .pixels
            .iter()
            .flat_map(|c| c.to_srgba_unmultiplied())
            .collect();
        self.files.adding += 1;
        let request = Request::AddImage {
            page,
            after,
            width: width as u32,
            height: height as u32,
            rgba,
        };
        let result = self.file_request(ctx, request);
        if result.is_err() {
            self.files.adding = 0;
        }
        self.report(result);
    }

    /// The open page and the block new files go after (the one being
    /// edited, else the last). `None`, with a notice, when no page that can
    /// take files is open.
    fn attach_target(&mut self) -> Option<(String, Option<String>)> {
        let page = self.selected.clone().filter(|id| {
            !self
                .pages
                .iter()
                .any(|p| p.id == *id && p.kind == PageKind::Database)
        });
        let Some(page) = page else {
            self.notice =
                Some("Abre una página (no una base de datos) para adjuntarle archivos.".into());
            return None;
        };
        let after = self
            .caret
            .as_ref()
            .map(|c| c.block_id.clone())
            .or_else(|| self.blocks.last().map(|b| b.id.clone()));
        Some((page, after))
    }

    /// Handles what the attachment work finished: new file blocks, opened
    /// or saved copies, and what failed.
    pub(super) fn tick_files(&mut self, ctx: &egui::Context) {
        if let Some(replace) = self.files.pick.take() {
            self.pick_files(ctx, replace);
        }
        for (action, block) in std::mem::take(&mut self.files.pending) {
            self.file_action(ctx, action, &block);
        }
        // Over budget: drop the previews out of sight.
        let seen = std::mem::take(&mut self.files.seen);
        let held: usize = self.files.previews.values().map(PreviewState::bytes).sum();
        if held > self.files.budget.unwrap_or(PREVIEW_BUDGET) {
            self.files
                .previews
                .retain(|id, state| seen.contains(id) || !matches!(state, PreviewState::Ready(_)));
        }
        for id in std::mem::take(&mut self.files.wanted) {
            if self.files.previews.contains_key(&id) {
                continue;
            }
            self.files
                .previews
                .insert(id.clone(), PreviewState::Loading);
            let result = self.file_request(ctx, Request::Preview(id));
            self.report(result);
        }
        let mut outcomes = std::mem::take(&mut self.files.done);
        if let Some(worker) = &self.files.worker {
            outcomes.extend(worker.poll());
        }
        for outcome in outcomes {
            match outcome {
                Outcome::Added { page, after, files } => {
                    self.files.adding = self.files.adding.saturating_sub(files.len());
                    let result = self.add_file_blocks(&page, after.as_deref(), files);
                    self.report(result);
                }
                Outcome::Opened(Err(err)) | Outcome::Revealed(Err(err)) => {
                    self.notice = Some(format!("No se pudo abrir el archivo: {err:#}"));
                }
                Outcome::Opened(Ok(_)) | Outcome::Revealed(Ok(_)) => {}
                Outcome::Saved(Ok(path)) => {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    self.notice = Some(format!("Se guardó una copia: «{name}»."));
                    self.last_export = Some(path);
                }
                Outcome::Saved(Err(err)) => {
                    self.notice = Some(format!("No se pudo guardar la copia: {err:#}"));
                }
                // Only for the page still open (its previews were cleared).
                Outcome::Preview(id, preview) => {
                    if !self.files.previews.contains_key(&id) {
                        continue;
                    }
                    let state = match preview {
                        Ok(preview) => {
                            let image = egui::ColorImage::from_rgba_unmultiplied(
                                [preview.width as usize, preview.height as usize],
                                &preview.rgba,
                            );
                            PreviewState::Ready(ctx.load_texture(
                                format!("silex-preview-{id}"),
                                image,
                                egui::TextureOptions::LINEAR,
                            ))
                        }
                        Err(err) => PreviewState::Failed(format!("{err:#}")),
                    };
                    self.files.previews.insert(id, state);
                }
            }
        }
    }

    /// One file block per stored file, after `after` in `page`, and a
    /// notice for the files that were not stored.
    fn add_file_blocks(
        &mut self,
        page: &str,
        after: Option<&str>,
        files: Vec<(String, Result<Attachment>)>,
    ) -> Result<()> {
        let mut failed = Vec::new();
        let mut new_blocks = Vec::new();
        for (name, stored) in files {
            match stored {
                Ok(attachment) => {
                    new_blocks.push(Block {
                        id: uuid::Uuid::new_v4().to_string(),
                        page_id: page.to_owned(),
                        kind: BlockKind::File,
                        content: attachment.name.clone(),
                        checked: false,
                        position: 0,
                        indent: 0,
                        attachment: Some(attachment.id.clone()),
                        link: None,
                    });
                    if self.selected.as_deref() == Some(page) {
                        self.files
                            .attachments
                            .insert(attachment.id.clone(), attachment);
                    }
                }
                Err(err) => failed.push(format!("{name}: {err:#}")),
            }
        }
        let added = new_blocks.len();
        if added > 0 {
            if self.selected.as_deref() == Some(page) {
                // In memory, like any edit: one undo step, saved by the autosave.
                let mut at = after
                    .and_then(|id| self.blocks.iter().position(|b| b.id == id))
                    .map_or(self.blocks.len(), |i| i + 1);
                // «/archivo» in a block left empty: the files take its place.
                let replace = self.files.replace.take();
                if let Some(index) = at.checked_sub(1)
                    && replace.is_some()
                    && replace.as_deref() == after
                    && self.blocks.get(index).is_some_and(|b| {
                        b.kind == BlockKind::Paragraph && b.content.trim().is_empty()
                    })
                {
                    self.blocks.remove(index);
                    at = index;
                }
                let indent = at
                    .checked_sub(1)
                    .and_then(|i| self.blocks.get(i))
                    .map_or(0, |b| b.indent);
                for block in &mut new_blocks {
                    block.indent = indent;
                }
                let last = new_blocks.last().map(|b| b.id.clone());
                self.blocks.splice(at..at, new_blocks);
                self.mark_dirty();
                if let Some(last) = last {
                    self.pending_focus = Some((last, 0));
                }
            } else {
                // The page was left meanwhile: add them at its end.
                let mut blocks = self.db.list_blocks(page)?;
                let title = self.db.get_page(page)?.map(|p| p.title).unwrap_or_default();
                blocks.extend(new_blocks);
                self.db.save_document(page, &title, &blocks)?;
            }
        }
        let message = match (added, failed.is_empty()) {
            (_, false) => format!("No se adjuntaron: {}", failed.join("; ")),
            (1, true) => "Se adjuntó 1 archivo.".to_owned(),
            (n, true) => format!("Se adjuntaron {n} archivos."),
        };
        self.post_notice(message);
        Ok(())
    }

    /// «Abrir», «Guardar una copia» or «Mostrar en la carpeta» on a file block.
    fn file_action(&mut self, ctx: &egui::Context, action: BlockAction, block: &str) {
        let Some(attachment) = self
            .blocks
            .iter()
            .find(|b| b.id == block)
            .and_then(|b| b.attachment.clone())
        else {
            return;
        };
        let request = match action {
            BlockAction::OpenFile => {
                // Programs are only shown, never opened (the worker refuses too).
                if self
                    .files
                    .attachments
                    .get(&attachment)
                    .is_some_and(Attachment::is_executable)
                {
                    Request::Reveal(attachment)
                } else {
                    Request::Open(attachment)
                }
            }
            BlockAction::SaveFile => Request::Save(attachment),
            BlockAction::RevealFile => Request::Reveal(attachment),
            _ => return,
        };
        let result = self.file_request(ctx, request);
        self.report(result);
    }
}
