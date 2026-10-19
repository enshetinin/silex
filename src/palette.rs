//! Command palette (H5.8): the actions the search window offers and how a
//! query picks them. Pure: the app decides what each one does.
use crate::editor::fold;
use crate::settings::{Settings, ThemeMode, zoom_step};

/// An action that can be run from the search window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Command {
    NewPage,
    NewDatabase,
    NewSubpage,
    NewRow,
    RenamePage,
    AddFavorite,
    RemoveFavorite,
    ExportPage,
    /// The file dialog, to attach files to the open page (H7.3).
    AttachFiles,
    SaveNow,
    TrashPage,
    OpenTrash,
    BackupNow,
    RestoreBackup,
    ExportWorkspace,
    ThemeSystem,
    ThemeLight,
    ThemeDark,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    ShowShortcuts,
}

/// The state that decides which commands make sense right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaletteContext {
    /// A page is open in the editor.
    pub page_open: bool,
    /// The open page is a database (its children are rows).
    pub database_open: bool,
    /// The open page is a favorite.
    pub favorite: bool,
    /// The database is a file (backups and workspace export need one).
    pub file_database: bool,
    pub backup_running: bool,
    pub export_running: bool,
    pub settings: Settings,
}

/// Most commands listed for a non-empty query (pages follow them).
pub const MAX_MATCHES: usize = 5;

impl Command {
    /// Menu order: page actions, workspace, appearance, help.
    pub const ALL: [Command; 22] = [
        Command::NewPage,
        Command::NewDatabase,
        Command::NewSubpage,
        Command::NewRow,
        Command::RenamePage,
        Command::AddFavorite,
        Command::RemoveFavorite,
        Command::ExportPage,
        Command::AttachFiles,
        Command::SaveNow,
        Command::TrashPage,
        Command::OpenTrash,
        Command::BackupNow,
        Command::RestoreBackup,
        Command::ExportWorkspace,
        Command::ThemeSystem,
        Command::ThemeLight,
        Command::ThemeDark,
        Command::ZoomIn,
        Command::ZoomOut,
        Command::ZoomReset,
        Command::ShowShortcuts,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Command::NewPage => "Nueva página",
            Command::NewDatabase => "Nueva base de datos",
            Command::NewSubpage => "Nueva subpágina",
            Command::NewRow => "Nueva fila",
            Command::RenamePage => "Renombrar la página",
            Command::AddFavorite => "Añadir a favoritos",
            Command::RemoveFavorite => "Quitar de favoritos",
            Command::ExportPage => "Exportar la página a Markdown",
            Command::AttachFiles => "Adjuntar archivos…",
            Command::SaveNow => "Guardar ahora",
            Command::TrashPage => "Enviar la página a la papelera",
            Command::OpenTrash => "Abrir la papelera",
            Command::BackupNow => "Crear una copia de seguridad",
            Command::RestoreBackup => "Restaurar una copia de seguridad…",
            Command::ExportWorkspace => "Exportar todo a Markdown",
            Command::ThemeSystem => "Tema: Sistema",
            Command::ThemeLight => "Tema: Claro",
            Command::ThemeDark => "Tema: Oscuro",
            Command::ZoomIn => "Aumentar el tamaño de la interfaz",
            Command::ZoomOut => "Reducir el tamaño de la interfaz",
            Command::ZoomReset => "Tamaño de la interfaz al 100 %",
            Command::ShowShortcuts => "Ver los atajos de teclado",
        }
    }

    /// Other words the command answers to (without accents).
    fn keywords(self) -> &'static [&'static str] {
        match self {
            Command::NewPage => &["crear", "documento", "new", "page"],
            Command::NewDatabase => &["crear", "tabla", "coleccion", "database", "table"],
            Command::NewSubpage => &["crear", "hija", "anidada", "child"],
            Command::NewRow => &["crear", "entrada", "registro", "row"],
            Command::RenamePage => &["titulo", "nombre", "cambiar", "rename"],
            Command::AddFavorite => &["estrella", "marcar", "favorite", "star"],
            Command::RemoveFavorite => &["estrella", "desmarcar", "favorite", "star"],
            Command::ExportPage => &["md", "descargar", "guardar", "export"],
            Command::AttachFiles => &[
                "subir",
                "agregar",
                "imagen",
                "foto",
                "pdf",
                "documento",
                "file",
            ],
            Command::SaveNow => &["save"],
            Command::TrashPage => &["borrar", "eliminar", "quitar", "delete"],
            Command::OpenTrash => &["restaurar", "borradas", "eliminadas", "recuperar", "trash"],
            Command::BackupNow => &["backup", "respaldo", "copiar"],
            Command::RestoreBackup => &["backup", "respaldo", "recuperar", "volver"],
            Command::ExportWorkspace => &["espacio", "trabajo", "md", "carpeta", "export"],
            Command::ThemeSystem => &["apariencia", "modo", "automatico", "theme"],
            Command::ThemeLight => &["apariencia", "modo", "dia", "blanco", "light", "theme"],
            Command::ThemeDark => &["apariencia", "modo", "noche", "negro", "dark", "theme"],
            Command::ZoomIn => &["zoom", "agrandar", "mas", "grande", "letra", "texto"],
            Command::ZoomOut => &["zoom", "achicar", "menos", "pequeno", "letra", "texto"],
            Command::ZoomReset => &["zoom", "normal", "original", "restablecer", "letra"],
            Command::ShowShortcuts => &["teclas", "teclado", "ayuda", "shortcuts", "help"],
        }
    }

    /// Whether the command makes sense in `cx` (unavailable ones are hidden).
    pub fn available(self, cx: &PaletteContext) -> bool {
        let theme = cx.settings.theme;
        let zoom = cx.settings.zoom_percent;
        match self {
            Command::NewPage
            | Command::NewDatabase
            | Command::SaveNow
            | Command::OpenTrash
            | Command::ShowShortcuts => true,
            // Inside a database a child is a row: «Nueva fila» instead.
            Command::NewSubpage => cx.page_open && !cx.database_open,
            Command::NewRow => cx.database_open,
            // A database shows its table, not blocks.
            Command::AttachFiles => cx.page_open && !cx.database_open,
            Command::RenamePage | Command::ExportPage | Command::TrashPage => cx.page_open,
            Command::AddFavorite => cx.page_open && !cx.favorite,
            Command::RemoveFavorite => cx.page_open && cx.favorite,
            Command::BackupNow | Command::RestoreBackup => cx.file_database && !cx.backup_running,
            Command::ExportWorkspace => cx.file_database && !cx.export_running,
            Command::ThemeSystem => theme != ThemeMode::System,
            Command::ThemeLight => theme != ThemeMode::Light,
            Command::ThemeDark => theme != ThemeMode::Dark,
            Command::ZoomIn => zoom_step(zoom, true).is_some(),
            Command::ZoomOut => zoom_step(zoom, false).is_some(),
            Command::ZoomReset => zoom != 100,
        }
    }

    /// Moves a page to the trash: listed after other matches unless the
    /// query starts its label, so a vague query plus Enter never runs it.
    fn cautious(self) -> bool {
        self == Command::TrashPage
    }

    /// How well `words` (already folded) match: `None` if some word
    /// matches nothing. Lower is better: 0 the label starts with the
    /// query, 1 every word starts a word of the label, 2 keywords needed;
    /// one more for a cautious command.
    fn score(self, query: &str, words: &[&str]) -> Option<u8> {
        let label = fold(self.label());
        if label.starts_with(query) {
            return Some(0);
        }
        let penalty = u8::from(self.cautious());
        let label_words: Vec<&str> = label
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .collect();
        let in_label = |word: &str| label_words.iter().any(|l| l.starts_with(word));
        if words.iter().all(|w| in_label(w)) {
            return Some(1 + penalty);
        }
        let in_keywords = |word: &str| self.keywords().iter().any(|k| k.starts_with(word));
        words
            .iter()
            .all(|w| in_label(w) || in_keywords(w))
            .then_some(2 + penalty)
    }
}

/// Commands to list for `query`. An empty query lists every available
/// command in menu order. Otherwise, at most [`MAX_MATCHES`] commands where
/// each word of the query starts a word of the label or a keyword (ignoring
/// case and accents), best first and then in menu order.
pub fn matching_commands(query: &str, cx: &PaletteContext) -> Vec<Command> {
    let available = Command::ALL.into_iter().filter(|c| c.available(cx));
    let query = fold(query.trim());
    let words: Vec<&str> = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    if words.is_empty() {
        // Nothing typed, or only symbols: everything when empty.
        return if query.is_empty() {
            available.collect()
        } else {
            Vec::new()
        };
    }
    let mut scored: Vec<(u8, Command)> = available
        .filter_map(|c| c.score(&query, &words).map(|s| (s, c)))
        .collect();
    // Stable: equal scores keep menu order.
    scored.sort_by_key(|(score, _)| *score);
    scored
        .into_iter()
        .take(MAX_MATCHES)
        .map(|(_, c)| c)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cx() -> PaletteContext {
        PaletteContext {
            page_open: true,
            database_open: false,
            favorite: false,
            file_database: true,
            backup_running: false,
            export_running: false,
            settings: Settings::default(),
        }
    }

    #[test]
    fn empty_query_lists_what_is_available_in_menu_order() {
        let all = matching_commands("", &cx());
        assert_eq!(all.first(), Some(&Command::NewPage));
        assert_eq!(all.last(), Some(&Command::ShowShortcuts));
        // Default settings: "Sistema" and 100 % are current.
        for hidden in [
            Command::RemoveFavorite,
            Command::ThemeSystem,
            Command::ZoomReset,
        ] {
            assert!(!all.contains(&hidden), "{hidden:?}");
        }
        assert!(all.contains(&Command::AddFavorite));
        let order: Vec<usize> = all
            .iter()
            .map(|c| {
                Command::ALL
                    .iter()
                    .position(|a| a == c)
                    .unwrap_or(usize::MAX)
            })
            .collect();
        assert!(order.windows(2).all(|w| w[0] < w[1]), "{order:?}");
        // Only symbols: nothing.
        assert!(matching_commands("¿?", &cx()).is_empty());
    }

    #[test]
    fn availability_follows_the_state() {
        let no_page = PaletteContext {
            page_open: false,
            file_database: false,
            ..cx()
        };
        let listed = matching_commands("", &no_page);
        for hidden in [
            Command::NewSubpage,
            Command::RenamePage,
            Command::AddFavorite,
            Command::ExportPage,
            Command::TrashPage,
            Command::BackupNow,
            Command::RestoreBackup,
            Command::ExportWorkspace,
        ] {
            assert!(!listed.contains(&hidden), "{hidden:?}");
        }
        assert!(listed.contains(&Command::NewPage));
        assert!(listed.contains(&Command::NewDatabase));
        assert!(!listed.contains(&Command::NewRow), "no database open");

        let database = PaletteContext {
            database_open: true,
            ..cx()
        };
        assert!(Command::NewRow.available(&database));
        assert!(
            !Command::NewSubpage.available(&database),
            "a child is a row"
        );
        assert_eq!(
            matching_commands("nueva fila", &database),
            [Command::NewRow]
        );

        let favorite = PaletteContext {
            favorite: true,
            ..cx()
        };
        assert!(Command::RemoveFavorite.available(&favorite));
        assert!(!Command::AddFavorite.available(&favorite));

        let busy = PaletteContext {
            backup_running: true,
            export_running: true,
            ..cx()
        };
        assert!(!Command::BackupNow.available(&busy));
        assert!(!Command::ExportWorkspace.available(&busy));

        let big = PaletteContext {
            settings: Settings {
                theme: ThemeMode::Dark,
                zoom_percent: 150,
            },
            ..cx()
        };
        assert!(!Command::ZoomIn.available(&big));
        assert!(Command::ZoomOut.available(&big));
        assert!(Command::ZoomReset.available(&big));
        assert!(!Command::ThemeDark.available(&big));
        assert!(Command::ThemeSystem.available(&big));
    }

    #[test]
    fn words_match_labels_and_keywords_without_accents() {
        assert_eq!(
            matching_commands("nueva", &cx()),
            [Command::NewPage, Command::NewDatabase, Command::NewSubpage]
        );
        assert_eq!(matching_commands("tabla", &cx()), [Command::NewDatabase]);
        assert_eq!(matching_commands("SUBPAGINA", &cx()), [Command::NewSubpage]);
        assert_eq!(matching_commands("oscuro", &cx()), [Command::ThemeDark]);
        assert_eq!(matching_commands("dark", &cx()), [Command::ThemeDark]);
        assert_eq!(matching_commands("tema cla", &cx()), [Command::ThemeLight]);
        assert_eq!(matching_commands("pequeño", &cx()), [Command::ZoomOut]);
        assert_eq!(matching_commands("anadir", &cx()), [Command::AddFavorite]);
        assert_eq!(matching_commands("adjuntar", &cx()), [Command::AttachFiles]);
        assert_eq!(matching_commands("pdf", &cx()), [Command::AttachFiles]);
        assert!(matching_commands("receta", &cx()).is_empty());
    }

    #[test]
    fn label_matches_rank_before_keyword_matches() {
        // "papelera" is in both trash labels: opening it comes first, so
        // a vague query plus Enter never trashes the page.
        assert_eq!(
            matching_commands("papelera", &cx()),
            [Command::OpenTrash, Command::TrashPage]
        );
        // Asked for by name, trashing comes first.
        assert_eq!(matching_commands("enviar", &cx()), [Command::TrashPage]);
        assert_eq!(matching_commands("borrar", &cx()), [Command::TrashPage]);
        // "exportar" starts two labels; "md" is only a keyword.
        assert_eq!(
            matching_commands("export", &cx()),
            [Command::ExportPage, Command::ExportWorkspace]
        );
        // "restaurar": a label match (restore a backup) before a keyword
        // match (the trash, where pages are restored).
        assert_eq!(
            matching_commands("restaurar", &cx()),
            [Command::RestoreBackup, Command::OpenTrash]
        );
    }

    #[test]
    fn non_empty_queries_list_a_few_commands() {
        // "a" starts words of almost every label.
        assert_eq!(matching_commands("a", &cx()).len(), MAX_MATCHES);
    }
}
