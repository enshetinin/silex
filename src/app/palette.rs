//! Command palette (H5.8): what each command does, its icon and shortcut.
//! Which commands are listed is decided in `silex_desktop::palette`.
use anyhow::Result;
use eframe::egui::{self};
use silex_desktop::{
    palette::{Command, PaletteContext},
    settings::{Settings, ThemeMode, zoom_step},
};

use super::icons;
use super::*;

/// Icon shown before a command.
pub(super) fn command_icon(command: Command) -> &'static str {
    match command {
        Command::NewPage | Command::NewSubpage | Command::NewRow => icons::PLUS,
        Command::NewDatabase => icons::TABLE,
        Command::RenamePage => icons::TEXT,
        Command::DuplicatePage => icons::COPY,
        Command::FindInPage | Command::ReplaceInPage => icons::SEARCH,
        Command::AddFavorite | Command::RemoveFavorite => icons::STAR,
        Command::ExportPage
        | Command::ExportViewCsv
        | Command::ExportDatabaseCsv
        | Command::ExportWorkspace => icons::EXPORT,
        Command::AttachFiles => icons::PAPERCLIP,
        Command::LinkPage => icons::LINK,
        Command::PageHistory => icons::HISTORY,
        Command::SaveAsTemplate | Command::NewFromTemplate => icons::COPY,
        Command::SaveNow => icons::SAVE,
        Command::TrashPage | Command::OpenTrash => icons::TRASH,
        Command::BackupNow | Command::RestoreBackup => icons::BACKUP,
        Command::ThemeSystem => icons::SETTINGS,
        Command::ThemeLight => icons::SUN,
        Command::ThemeDark => icons::MOON,
        Command::ZoomIn => icons::ARROW_UP,
        Command::ZoomOut => icons::ARROW_DOWN,
        Command::ZoomReset => icons::TEXT,
        Command::ShowShortcuts => icons::KEYBOARD,
        Command::CheckUpdates => icons::DOWNLOAD,
        Command::Format(format) => super::inline::icon(format),
        Command::Link => icons::LINK,
        Command::ToggleOutline => icons::TABLE_OF_CONTENTS,
        Command::MoveBlocks => icons::ARROW_RIGHT,
        Command::ToSubpage => icons::PAGE,
        Command::CopyMarkdown => icons::COPY,
    }
}

/// The keyboard shortcut that does the same, if any (`cmd` is "Cmd" or "Ctrl").
pub(super) fn command_shortcut(command: Command, cmd: &str) -> Option<String> {
    Some(match command {
        Command::NewPage => format!("{cmd}+N"),
        // In a database Cmd+Shift+N adds a row (`create_subpage`).
        Command::NewSubpage | Command::NewRow => format!("{cmd}+Mayús+N"),
        Command::RenamePage => "F2".to_owned(),
        Command::FindInPage => format!("{cmd}+F"),
        Command::ReplaceInPage if cmd == "Cmd" => "Cmd+Opción+F".to_owned(),
        Command::ReplaceInPage => "Ctrl+H".to_owned(),
        Command::SaveNow => format!("{cmd}+S"),
        Command::ZoomIn => format!("{cmd}++"),
        Command::ZoomOut => format!("{cmd}+−"),
        Command::ZoomReset => format!("{cmd}+0"),
        Command::Format(format) => super::inline::shortcut_label(format, cmd),
        Command::Link => format!("{cmd}+K"),
        _ => return None,
    })
}

impl SilexApp {
    /// What decides which commands are offered right now.
    pub(super) fn palette_context(&self) -> PaletteContext {
        PaletteContext {
            page_open: self.selected.is_some(),
            database_open: self.database.is_some(),
            favorite: self
                .selected
                .as_ref()
                .is_some_and(|id| self.favorites.iter().any(|p| &p.id == id)),
            row_open: self.row_page.is_some(),
            template_open: self.template_open(),
            file_database: self.db.path().is_some(),
            backup_running: self.backup_job.is_some(),
            export_running: self.export_job.is_some(),
            csv_export_running: self.csv_export.is_some(),
            format_block: self.format_target.as_ref().is_some_and(|(id, _, _)| {
                self.blocks
                    .iter()
                    .any(|b| &b.id == id && b.kind.takes_marks())
            }),
            blocks_target: self.selection.is_some() || self.format_target.is_some(),
            settings: self.settings,
        }
    }

    /// Runs a command chosen in the search window (already closed). Each
    /// one goes through the same code as its button, menu or shortcut.
    pub(super) fn run_command(&mut self, ctx: &egui::Context, command: Command) -> Result<()> {
        let page = self.selected.clone();
        let with_settings = |app: &mut Self, change: &dyn Fn(&mut Settings)| {
            let mut settings = app.settings;
            change(&mut settings);
            app.set_settings(ctx, settings);
        };
        match (command, page) {
            (Command::NewPage, _) => self.create_root_page("Sin título")?,
            (Command::NewDatabase, _) => self.create_database(None)?,
            (Command::NewRow, Some(id)) => self.add_row(&id)?,
            (Command::NewSubpage, Some(id)) => self.create_subpage(&id)?,
            // The open page is renamed in its title, as from its "…" menu.
            (Command::RenamePage, Some(_)) => self.focus_title = true,
            (Command::DuplicatePage, Some(id)) => self.start_duplicate(&id)?,
            (Command::FindInPage, Some(_)) => self.open_find(ctx, false),
            (Command::ReplaceInPage, Some(_)) => self.open_find(ctx, true),
            (Command::Format(format), Some(_)) => {
                if let Some((id, _, _)) = self.format_target.clone() {
                    self.format_block(ctx, &id, format, true);
                }
            }
            (Command::ToggleOutline, Some(_)) => self.toggle_outline(ctx),
            (Command::CopyMarkdown, Some(_)) => self.copy_page_as_markdown(ctx),
            (Command::MoveBlocks | Command::ToSubpage, Some(_)) => {
                let target = self.selection_range().or_else(|| {
                    let (id, _, _) = self.format_target.as_ref()?;
                    let index = self.blocks.iter().position(|b| &b.id == id)?;
                    Some((index, index))
                });
                if let Some((first, last)) = target {
                    if command == Command::MoveBlocks {
                        self.open_move_picker(first, last);
                    } else {
                        self.convert_to_subpage(first, last)?;
                    }
                }
            }
            (Command::Link, Some(_)) => {
                if let Some((id, _, _)) = self.format_target.clone() {
                    self.open_link_edit(ctx, &id, true);
                }
            }
            (Command::AddFavorite, Some(id)) => self.set_favorite(&id, true)?,
            (Command::RemoveFavorite, Some(id)) => self.set_favorite(&id, false)?,
            (Command::ExportPage, Some(_)) => self.export_current_page(ctx)?,
            (Command::ExportViewCsv, Some(_)) => self.export_database_csv(ctx, false)?,
            (Command::ExportDatabaseCsv, Some(_)) => self.export_database_csv(ctx, true)?,
            (Command::AttachFiles, Some(_)) => self.files.pick = Some(None),
            (Command::LinkPage, Some(_)) => self.open_link_picker(None, None),
            (Command::PageHistory, Some(_)) => self.open_page_history()?,
            (Command::SaveAsTemplate, Some(id)) => self.save_as_template(&id)?,
            // From the palette, a page at the root (the «…» menu of a page
            // puts it inside that page).
            (Command::NewFromTemplate, _) => self.open_template_picker(None),
            // A save error is already shown by the status bar.
            (Command::SaveNow, _) => {
                let _ = self.save_now();
            }
            (Command::TrashPage, Some(id)) => self.trash_page(&id)?,
            (Command::OpenTrash, _) => self.open_trash_dialog(),
            (Command::BackupNow, _) => self.start_backup(ctx),
            (Command::RestoreBackup, _) => self.open_restore_dialog(),
            (Command::ExportWorkspace, _) => {
                let root = silex_desktop::export::export_root()?;
                self.start_workspace_export(ctx, root)?;
            }
            (Command::ThemeSystem, _) => with_settings(self, &|s| s.theme = ThemeMode::System),
            (Command::ThemeLight, _) => with_settings(self, &|s| s.theme = ThemeMode::Light),
            (Command::ThemeDark, _) => with_settings(self, &|s| s.theme = ThemeMode::Dark),
            (Command::ZoomIn | Command::ZoomOut, _) => {
                if let Some(zoom) =
                    zoom_step(self.settings.zoom_percent, command == Command::ZoomIn)
                {
                    with_settings(self, &|s| s.zoom_percent = zoom);
                }
            }
            (Command::ZoomReset, _) => with_settings(self, &|s| s.zoom_percent = 100),
            (Command::ShowShortcuts, _) => self.shortcuts_dialog = true,
            (Command::CheckUpdates, _) => open_releases_page(ctx),
            // A page command with no page open (not offered then).
            (_, None) => {}
        }
        Ok(())
    }
}
