use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::mpsc,
    time::Duration,
};

use silex_desktop::db::Database;
use silex_desktop::export::{WorkspaceExportJob, export_workspace};
use silex_desktop::markdown::page_to_markdown;
use silex_desktop::model::BlockKind;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> anyhow::Result<Self> {
        let dir = std::env::temp_dir().join(format!("silex-ws-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir)?;
        Ok(Self(dir))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Every file and folder under `dir`, relative, with "/" separators.
fn tree(dir: &Path) -> anyhow::Result<Vec<String>> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) -> anyhow::Result<()> {
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            let rel = path
                .strip_prefix(base)?
                .to_string_lossy()
                .replace('\\', "/");
            if path.is_dir() {
                out.push(format!("{rel}/"));
                walk(base, &path, out)?;
            } else {
                out.push(rel);
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out)?;
    out.sort();
    Ok(out)
}

fn page(
    db: &mut Database,
    title: &str,
    parent: Option<&str>,
    text: &str,
) -> anyhow::Result<String> {
    let id = db.create_page(title, parent)?;
    db.add_block(&id, BlockKind::Paragraph)?;
    let mut blocks = db.list_blocks(&id)?;
    blocks[0].content = text.into();
    db.save_document(&id, title, &blocks)?;
    Ok(id)
}

#[test]
fn whole_tree_is_exported_with_folders_for_subpages() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    let mut db = Database::open_in_memory()?;
    let notas = page(&mut db, "Notas", None, "raíz")?;
    let viaje = page(&mut db, "Viaje", Some(&notas), "hija")?;
    page(&mut db, "Islandia", Some(&viaje), "nieta")?;
    page(&mut db, "Trabajo", None, "otra raíz")?;
    let borrada = page(&mut db, "Borrada", None, "en la papelera")?;
    db.trash_page(&borrada)?;

    let result = export_workspace(&db, &tmp.0)?;
    assert_eq!(result.pages, 4, "the trash is not exported");
    assert!(result.dir.starts_with(&tmp.0));
    let name = result
        .dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    assert!(name.starts_with("Silex "), "{name}");
    assert_eq!(
        tree(&result.dir)?,
        [
            "Notas.md",
            "Notas/",
            "Notas/Viaje.md",
            "Notas/Viaje/",
            "Notas/Viaje/Islandia.md",
            "Trabajo.md",
        ]
    );
    let blocks = db.list_blocks(&viaje)?;
    assert_eq!(
        fs::read_to_string(result.dir.join("Notas/Viaje.md"))?,
        page_to_markdown("Viaje", &blocks),
        "same content as exporting the page alone"
    );
    // Only the final folder remains in the root (no temporary leftovers).
    let top: Vec<_> = fs::read_dir(&tmp.0)?.collect::<Result<_, _>>()?;
    assert_eq!(top.len(), 1);
    assert_eq!(top[0].path(), result.dir);
    Ok(())
}

#[test]
fn hostile_and_repeated_titles_stay_inside_and_never_collide() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    let mut db = Database::open_in_memory()?;
    let parent = page(&mut db, "Nota", None, "con hijos")?;
    page(&mut db, "hija", Some(&parent), "x")?;
    page(
        &mut db,
        "nota",
        None,
        "mismo nombre sin distinguir mayúsculas",
    )?;
    page(&mut db, "Nota.md", None, "parece el archivo de otra")?;
    page(&mut db, "../../fuera", None, "intento de salir")?;
    page(&mut db, "CON", None, "nombre reservado")?;
    page(&mut db, "a/b\\c:d", None, "separadores")?;
    page(&mut db, "", None, "sin título")?;

    let result = export_workspace(&db, &tmp.0)?;
    assert_eq!(result.pages, 8);
    let entries = tree(&result.dir)?;
    assert_eq!(
        entries,
        [
            "Nota.md",
            // Not "Nota.md.md": its subpage folder would be "Nota.md",
            // clashing with the file of the page "Nota".
            "Nota.md (2).md",
            "Nota/",
            "Nota/hija.md",
            "Sin título.md",
            "_CON.md",
            // Separators and leading dots become "_": cannot leave the folder.
            "___.._fuera.md",
            "a_b_c_d.md",
            "nota (2).md",
        ],
        "{entries:#?}"
    );
    // Nothing escaped the export folder.
    for entry in fs::read_dir(&tmp.0)? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        assert!(name.starts_with("Silex "), "unexpected {name}");
    }
    Ok(())
}

#[test]
fn exports_never_overwrite_each_other() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    let mut db = Database::open_in_memory()?;
    page(&mut db, "Única", None, "x")?;
    let first = export_workspace(&db, &tmp.0)?;
    let second = export_workspace(&db, &tmp.0)?;
    assert_ne!(first.dir, second.dir, "same second: a numbered folder");
    assert!(first.dir.join("Única.md").exists() && second.dir.join("Única.md").exists());
    Ok(())
}

#[test]
fn failure_leaves_nothing_behind() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    let mut db = Database::open_in_memory()?;
    page(&mut db, "Página", None, "x")?;
    let not_a_folder = tmp.0.join("archivo");
    fs::write(&not_a_folder, b"no soy una carpeta")?;
    assert!(export_workspace(&db, &not_a_folder).is_err());
    assert_eq!(tree(&tmp.0)?, ["archivo"]);
    Ok(())
}

#[test]
fn background_job_exports_from_its_own_connection() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    let db_path = tmp.0.join("silex.sqlite3");
    let mut db = Database::open(&db_path)?;
    let id = page(&mut db, "Desde el hilo", None, "contenido")?;
    let out = tmp.0.join("exportaciones");
    let (done_tx, done_rx) = mpsc::channel();
    let job = WorkspaceExportJob::spawn(db_path, out.clone(), move || {
        let _ = done_tx.send(());
    })?;
    done_rx.recv_timeout(Duration::from_secs(30))?;
    let result = job.poll().expect("finished")?;
    let contents: BTreeMap<String, String> = tree(&result.dir)?
        .into_iter()
        .filter(|e| !e.ends_with('/'))
        .map(|e| {
            let text = fs::read_to_string(result.dir.join(&e)).unwrap_or_default();
            (e, text)
        })
        .collect();
    assert_eq!(
        contents.get("Desde el hilo.md"),
        Some(&page_to_markdown("Desde el hilo", &db.list_blocks(&id)?))
    );
    Ok(())
}

/// H12.8b: a page's icon is written before its title inside the file, not
/// in its name; databases too.
#[test]
fn icons_go_before_the_title_inside_the_files() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    let mut db = Database::open_in_memory()?;
    let pan = page(&mut db, "Pan", None, "harina")?;
    db.set_page_icon(&pan, Some("🍞"))?;
    page(&mut db, "Sin icono", None, "nada")?;
    let table = db.create_database("Tareas", None)?;
    db.set_page_icon(&table, Some("✅"))?;
    let out = export_workspace(&db, &tmp.0)?;
    let files = tree(&out.dir)?;
    assert!(files.contains(&"Pan.md".to_owned()), "{files:?}");
    let pan_md = fs::read_to_string(out.dir.join("Pan.md"))?;
    assert!(pan_md.starts_with("# 🍞 Pan\n"), "{pan_md}");
    let plain = fs::read_to_string(out.dir.join("Sin icono.md"))?;
    assert!(plain.starts_with("# Sin icono\n"), "{plain}");
    let tasks = fs::read_to_string(out.dir.join("Tareas.md"))?;
    assert!(tasks.starts_with("# ✅ Tareas\n"), "{tasks}");
    Ok(())
}

/// H12.11: a database made inside a page goes to the trash with it, comes
/// back with it, and is exported inside its folder.
#[test]
fn a_database_inside_a_page_travels_with_it() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    let mut db = Database::open_in_memory()?;
    let notas = page(&mut db, "Notas", None, "con una base")?;
    let lecturas = db.create_database("Lecturas", Some(&notas))?;
    db.create_row(&lecturas, "Dune")?;
    db.trash_page(&notas)?;
    assert!(db.get_page(&lecturas)?.is_none(), "trashed with its page");
    db.restore_page(&notas)?;
    assert!(db.get_page(&lecturas)?.is_some(), "back with it");
    let result = export_workspace(&db, &tmp.0)?;
    let entries = tree(&result.dir)?;
    assert!(
        entries.iter().any(|e| e == "Notas/Lecturas.md"),
        "{entries:?}"
    );
    assert!(
        entries
            .iter()
            .all(|e| e == "Notas.md" || e.starts_with("Notas/")),
        "{entries:?}"
    );
    Ok(())
}
