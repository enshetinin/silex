//! The work on attachments that touches files (ADR 0007): storing dropped
//! files, and writing a copy of one to open it, save it or show it in its
//! folder. It runs in a `silex-attach` thread with its own connection, so
//! a 20 MB file never stalls the interface; [`handle`] is the same work
//! without the thread (tests, and in-memory databases).
//!
//! Programs and scripts are never opened, whatever the caller asks: they
//! can only be shown in their folder.
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender},
};

use anyhow::{Context, Result, anyhow, bail};

use crate::attachment::{Attachment, NewAttachment, Preview, decode_preview, encode_png};
use crate::db::Database;
use crate::export::write_atomic;
use crate::safe_path::{join_inside, unique_attachment_name};

/// Where copies of attachments are written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folders {
    /// Copies to open or show: `<cache>/abiertos/<id>/<name>`. Emptied at
    /// startup ([`clear_opened`]); changes made there do not come back.
    pub opened: PathBuf,
    /// «Guardar una copia»: the exports folder.
    pub saved: PathBuf,
}

impl Folders {
    /// The app's cache folder and the exports folder.
    pub fn standard() -> Result<Self> {
        let dirs = directories::ProjectDirs::from("dev", "silex", "Silex")
            .context("No se encuentra la carpeta de la aplicación")?;
        Ok(Self {
            opened: dirs.cache_dir().join("abiertos"),
            saved: crate::export::export_root()?,
        })
    }
}

/// Deletes the copies opened in an earlier session.
pub fn clear_opened(folders: &Folders) -> Result<()> {
    match fs::remove_dir_all(&folders.opened) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
            Err(err).with_context(|| format!("No se pudo vaciar {}", folders.opened.display()))
        }
        _ => Ok(()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Store these files for the page `page`, to go after the block
    /// `after` (or at the end).
    Add {
        page: String,
        after: Option<String>,
        paths: Vec<PathBuf>,
    },
    /// Write a copy and open it with the system's app.
    Open(String),
    /// Write a copy in the exports folder.
    Save(String),
    /// Write a copy and show its folder (programs and scripts).
    Reveal(String),
    /// Decode an image attachment for its preview (H7.2).
    Preview(String),
    /// Store a pasted image (RGBA pixels) as a PNG, like `Add`.
    AddImage {
        page: String,
        after: Option<String>,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
}

/// The name of a pasted image.
pub const PASTED_IMAGE_NAME: &str = "Imagen pegada.png";

#[derive(Debug)]
pub enum Outcome {
    /// Each file by the name it was dropped with, stored or not.
    Added {
        page: String,
        after: Option<String>,
        files: Vec<(String, Result<Attachment>)>,
    },
    /// The copy written (opened, saved or shown), or why not.
    Opened(Result<PathBuf>),
    Saved(Result<PathBuf>),
    Revealed(Result<PathBuf>),
    /// An attachment's preview, by its id.
    Preview(String, Result<Preview>),
}

/// Does one request with `db`. `open` opens a file with its app, or a
/// folder in the file manager (the real one is
/// [`crate::export::open_in_file_manager`]).
pub fn handle(
    db: &mut Database,
    folders: &Folders,
    request: Request,
    open: &dyn Fn(&Path) -> Result<()>,
) -> Outcome {
    match request {
        Request::Add { page, after, paths } => {
            let files = paths
                .iter()
                .map(|path| {
                    let name = path
                        .file_name()
                        .map(|n| crate::attachment::display_name(&n.to_string_lossy()))
                        .unwrap_or_default();
                    let stored =
                        NewAttachment::read(path).and_then(|file| db.add_attachment(&file));
                    (name, stored)
                })
                .collect();
            Outcome::Added { page, after, files }
        }
        Request::Open(id) => Outcome::Opened((|| {
            let attachment = find(db, &id)?;
            if attachment.is_executable() {
                bail!(
                    "Silex no abre programas ni scripts: «{}» se puede mostrar en su carpeta",
                    attachment.name
                );
            }
            let path = write_opened(db, folders, &attachment)?;
            open(&path)?;
            Ok(path)
        })()),
        Request::Reveal(id) => Outcome::Revealed((|| {
            let attachment = find(db, &id)?;
            let path = write_opened(db, folders, &attachment)?;
            let folder = path.parent().unwrap_or(&folders.opened);
            open(folder)?;
            Ok(path)
        })()),
        Request::Preview(id) => {
            let preview = find(db, &id)
                .and_then(|attachment| decode_preview(&attachment.mime, &db.attachment_data(&id)?));
            Outcome::Preview(id, preview)
        }
        Request::AddImage {
            page,
            after,
            width,
            height,
            rgba,
        } => {
            let stored = encode_png(width, height, rgba)
                .and_then(|png| NewAttachment::from_bytes(PASTED_IMAGE_NAME, png))
                .and_then(|file| db.add_attachment(&file));
            Outcome::Added {
                page,
                after,
                files: vec![(PASTED_IMAGE_NAME.to_owned(), stored)],
            }
        }
        Request::Save(id) => Outcome::Saved((|| {
            let attachment = find(db, &id)?;
            fs::create_dir_all(&folders.saved).with_context(|| {
                format!("No se puede crear la carpeta {}", folders.saved.display())
            })?;
            // Names already in the folder are taken: nothing is overwritten.
            let mut taken: HashSet<String> = fs::read_dir(&folders.saved)?
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.file_name().to_string_lossy().to_lowercase())
                .collect();
            let name = unique_attachment_name(&attachment.name, &mut taken);
            let path = join_inside(&folders.saved, &[&name])?;
            write_atomic(&path, &db.attachment_data(&attachment.id)?)?;
            Ok(path)
        })()),
    }
}

fn find(db: &Database, id: &str) -> Result<Attachment> {
    db.attachment(id)?
        .ok_or_else(|| anyhow!("El archivo adjunto ya no existe"))
}

/// `<opened>/<id>/<safe name>`, written again each time (it may have been
/// changed or deleted by the app that opened it).
fn write_opened(db: &Database, folders: &Folders, attachment: &Attachment) -> Result<PathBuf> {
    let name = unique_attachment_name(&attachment.name, &mut HashSet::new());
    // The id is a UUID made by Silex, but it is checked like any name.
    let dir = join_inside(&folders.opened, &[&attachment.id])?;
    fs::create_dir_all(&dir)
        .with_context(|| format!("No se puede crear la carpeta {}", dir.display()))?;
    let path = join_inside(&dir, &[&name])?;
    write_atomic(&path, &db.attachment_data(&attachment.id)?)?;
    Ok(path)
}

/// The `silex-attach` thread: does requests in order, with its own
/// connection to the database file.
pub struct AttachWorker {
    requests: Sender<Request>,
    outcomes: Receiver<Outcome>,
}

impl AttachWorker {
    /// `on_done` runs after each outcome is sent (e.g. to repaint the UI).
    pub fn spawn(
        db_path: PathBuf,
        folders: Folders,
        on_done: impl Fn() + Send + 'static,
    ) -> Result<Self> {
        let (requests, inbox) = mpsc::channel::<Request>();
        let (outbox, outcomes) = mpsc::channel();
        std::thread::Builder::new()
            .name("silex-attach".into())
            .spawn(move || {
                let mut db = Database::open(&db_path);
                while let Ok(request) = inbox.recv() {
                    let outcome = match &mut db {
                        Ok(db) => {
                            handle(db, &folders, request, &crate::export::open_in_file_manager)
                        }
                        Err(err) => failed(request, anyhow!("No se puede abrir la base: {err:#}")),
                    };
                    if outbox.send(outcome).is_err() {
                        break;
                    }
                    on_done();
                }
            })
            .context("No se puede iniciar el hilo de adjuntos")?;
        Ok(Self { requests, outcomes })
    }

    pub fn request(&self, request: Request) -> Result<()> {
        self.requests
            .send(request)
            .map_err(|_| anyhow!("El hilo de adjuntos se ha detenido"))
    }

    /// The outcomes that arrived since the last call.
    pub fn poll(&self) -> Vec<Outcome> {
        self.outcomes.try_iter().collect()
    }
}

/// The outcome of `request` when nothing could be done.
fn failed(request: Request, err: anyhow::Error) -> Outcome {
    let message = format!("{err:#}");
    match request {
        Request::Add { page, after, paths } => Outcome::Added {
            page,
            after,
            files: paths
                .iter()
                .map(|p| {
                    let name = p
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    (name, Err(anyhow!("{message}")))
                })
                .collect(),
        },
        Request::AddImage { page, after, .. } => Outcome::Added {
            page,
            after,
            files: vec![(PASTED_IMAGE_NAME.to_owned(), Err(anyhow!("{message}")))],
        },
        Request::Preview(id) => Outcome::Preview(id, Err(err)),
        Request::Open(_) => Outcome::Opened(Err(err)),
        Request::Save(_) => Outcome::Saved(Err(err)),
        Request::Reveal(_) => Outcome::Revealed(Err(err)),
    }
}
