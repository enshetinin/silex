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
use silex_desktop::attachment::{Attachment, FileKind, format_size};
use silex_desktop::model::{Block, BlockKind, PageKind};

use super::*;
use super::{icons, theme};

/// Height of a file card.
const CARD_HEIGHT: f32 = 44.0;

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
) -> (egui::Response, Option<BlockAction>) {
    let p = theme::current(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), CARD_HEIGHT),
        egui::Sense::hover(),
    );
    let response = ui.interact(rect, id, egui::Sense::click());
    let size = attachment.map(|a| format_size(a.size));
    let executable = attachment.is_some_and(Attachment::is_executable);
    let label = match &size {
        Some(size) => format!("Archivo «{}», {size}", block.content),
        None => format!("Archivo «{}»", block.content),
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

impl SilexApp {
    /// Reads the open page's attachments (name, type, size).
    pub(super) fn load_attachments(&mut self, page: &str) -> Result<()> {
        self.files.attachments = self
            .db
            .page_attachments(page)?
            .into_iter()
            .map(|a| (a.id.clone(), a))
            .collect();
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
        let page = self.selected.clone().filter(|id| {
            !self
                .pages
                .iter()
                .any(|p| p.id == *id && p.kind == PageKind::Database)
        });
        let Some(page) = page else {
            self.notice =
                Some("Abre una página (no una base de datos) para adjuntarle archivos.".into());
            return;
        };
        let after = self
            .caret
            .as_ref()
            .map(|c| c.block_id.clone())
            .or_else(|| self.blocks.last().map(|b| b.id.clone()));
        self.files.adding += paths.len();
        let result = self.file_request(ctx, Request::Add { page, after, paths });
        if result.is_err() {
            self.files.adding = 0;
        }
        self.report(result);
    }

    /// Handles what the attachment work finished: new file blocks, opened
    /// or saved copies, and what failed.
    pub(super) fn tick_files(&mut self, ctx: &egui::Context) {
        for (action, block) in std::mem::take(&mut self.files.pending) {
            self.file_action(ctx, action, &block);
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
                let at = after
                    .and_then(|id| self.blocks.iter().position(|b| b.id == id))
                    .map_or(self.blocks.len(), |i| i + 1);
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
