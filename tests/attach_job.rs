//! The attach worker's work (ADR 0007), without its thread.
use std::{cell::RefCell, fs, path::PathBuf};

use silex_desktop::attach_job::{AttachWorker, Folders, Outcome, Request, clear_opened, handle};
use silex_desktop::db::Database;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> anyhow::Result<Self> {
        let dir = std::env::temp_dir().join(format!("silex-job-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir)?;
        Ok(Self(dir))
    }

    fn folders(&self) -> Folders {
        Folders {
            opened: self.0.join("cache").join("abiertos"),
            saved: self.0.join("Exportaciones"),
        }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Stores the files `(name, content)` and returns their ids in order.
fn add(db: &mut Database, tmp: &TempDir, files: &[(&str, &[u8])]) -> anyhow::Result<Vec<String>> {
    let source = tmp.0.join("origen");
    fs::create_dir_all(&source)?;
    let mut paths = Vec::new();
    for (name, content) in files {
        let path = source.join(name);
        fs::write(&path, content)?;
        paths.push(path);
    }
    let request = Request::Add {
        page: "p".into(),
        after: Some("b".into()),
        paths,
    };
    let Outcome::Added { page, after, files } = handle(db, &tmp.folders(), request, &|_| {
        unreachable!("nothing opened")
    }) else {
        anyhow::bail!("not an Added outcome");
    };
    assert_eq!((page.as_str(), after.as_deref()), ("p", Some("b")));
    files
        .into_iter()
        .map(|(_, stored)| stored.map(|a| a.id))
        .collect()
}

#[test]
fn dropped_files_are_stored_one_by_one() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    let mut db = Database::open_in_memory()?;
    let source = tmp.0.join("origen");
    fs::create_dir_all(&source)?;
    fs::write(source.join("nota.txt"), "hola")?;
    let big = source.join("enorme.bin");
    fs::File::create(&big)?.set_len(silex_desktop::attachment::MAX_SIZE + 1)?;
    let request = Request::Add {
        page: "p".into(),
        after: None,
        paths: vec![source.join("nota.txt"), big, source.join("no-existe.pdf")],
    };
    let Outcome::Added { files, .. } = handle(&mut db, &tmp.folders(), request, &|_| Ok(())) else {
        anyhow::bail!("not an Added outcome");
    };
    let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["nota.txt", "enorme.bin", "no-existe.pdf"]);
    let stored = files[0].1.as_ref().expect("the small one is stored");
    assert_eq!(db.attachment_data(&stored.id)?, b"hola");
    let too_big = files[1].1.as_ref().expect_err("over the limit");
    assert!(format!("{too_big:#}").contains("20 MB"), "{too_big:#}");
    assert!(files[2].1.is_err());
    Ok(())
}

#[test]
fn opening_writes_a_copy_and_never_opens_programs() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    let mut db = Database::open_in_memory()?;
    let ids = add(
        &mut db,
        &tmp,
        &[
            ("informe.pdf", b"%PDF-1.7"),
            ("instalar.sh", b"#!/bin/sh\necho"),
        ],
    )?;
    let opened = RefCell::new(Vec::new());
    let opener = |path: &std::path::Path| {
        opened.borrow_mut().push(path.to_path_buf());
        Ok(())
    };

    let Outcome::Opened(path) = handle(
        &mut db,
        &tmp.folders(),
        Request::Open(ids[0].clone()),
        &opener,
    ) else {
        anyhow::bail!("not Opened");
    };
    let path = path?;
    assert_eq!(fs::read(&path)?, b"%PDF-1.7");
    assert!(path.starts_with(tmp.folders().opened.join(&ids[0])));
    assert_eq!(
        path.file_name().and_then(|n| n.to_str()),
        Some("informe.pdf")
    );
    assert_eq!(*opened.borrow(), std::slice::from_ref(&path));

    // A script: refused, nothing written or opened.
    let Outcome::Opened(refused) = handle(
        &mut db,
        &tmp.folders(),
        Request::Open(ids[1].clone()),
        &opener,
    ) else {
        anyhow::bail!("not Opened");
    };
    let err = refused.expect_err("scripts are not opened");
    assert!(format!("{err:#}").contains("no abre programas"), "{err:#}");
    assert_eq!(opened.borrow().len(), 1);
    assert!(!tmp.folders().opened.join(&ids[1]).exists());

    // It can be shown in its folder.
    let Outcome::Revealed(shown) = handle(
        &mut db,
        &tmp.folders(),
        Request::Reveal(ids[1].clone()),
        &opener,
    ) else {
        anyhow::bail!("not Revealed");
    };
    let shown = shown?;
    assert_eq!(
        opened.borrow().last(),
        shown.parent().map(|p| p.to_path_buf()).as_ref()
    );

    // Unknown ids are explained.
    let Outcome::Opened(gone) = handle(&mut db, &tmp.folders(), Request::Open("x".into()), &opener)
    else {
        anyhow::bail!("not Opened");
    };
    assert!(format!("{:#}", gone.expect_err("gone")).contains("ya no existe"));

    // The next session starts with no copies.
    clear_opened(&tmp.folders())?;
    assert!(!tmp.folders().opened.exists());
    clear_opened(&tmp.folders())?;
    Ok(())
}

#[test]
fn saving_a_copy_never_overwrites() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    let mut db = Database::open_in_memory()?;
    let ids = add(&mut db, &tmp, &[("informe.pdf", b"%PDF-1.7")])?;
    fs::create_dir_all(&tmp.folders().saved)?;
    fs::write(tmp.folders().saved.join("Informe.PDF"), "mío")?;
    let save = |db: &mut Database| -> anyhow::Result<PathBuf> {
        match handle(db, &tmp.folders(), Request::Save(ids[0].clone()), &|_| {
            Ok(())
        }) {
            Outcome::Saved(path) => path,
            _ => anyhow::bail!("not Saved"),
        }
    };
    let first = save(&mut db)?;
    let second = save(&mut db)?;
    assert_eq!(
        first.file_name().and_then(|n| n.to_str()),
        Some("informe (2).pdf")
    );
    assert_eq!(
        second.file_name().and_then(|n| n.to_str()),
        Some("informe (3).pdf")
    );
    assert_eq!(fs::read(&first)?, b"%PDF-1.7");
    assert_eq!(
        fs::read_to_string(tmp.folders().saved.join("Informe.PDF"))?,
        "mío"
    );
    Ok(())
}

#[test]
fn the_worker_does_the_same_in_its_own_thread() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    let db_path = tmp.0.join("silex.sqlite3");
    drop(Database::open(&db_path)?);
    fs::write(tmp.0.join("nota.txt"), "hola")?;
    let (done, wait) = std::sync::mpsc::channel();
    let worker = AttachWorker::spawn(db_path.clone(), tmp.folders(), move || {
        let _ = done.send(());
    })?;
    worker.request(Request::Add {
        page: "p".into(),
        after: None,
        paths: vec![tmp.0.join("nota.txt")],
    })?;
    wait.recv_timeout(std::time::Duration::from_secs(10))?;
    let outcomes = worker.poll();
    let [Outcome::Added { files, .. }] = outcomes.as_slice() else {
        anyhow::bail!("one Added outcome: {outcomes:?}");
    };
    let id = files[0].1.as_ref().expect("stored").id.clone();
    // Stored through the worker's own connection, seen by another.
    assert_eq!(Database::open(&db_path)?.attachment_data(&id)?, b"hola");
    Ok(())
}

#[test]
fn pasted_images_are_stored_as_png_and_previews_decode() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    let mut db = Database::open_in_memory()?;
    let (width, height) = (4, 3);
    let rgba: Vec<u8> = (0..width * height).flat_map(|_| [255, 0, 0, 255]).collect();
    let request = Request::AddImage {
        page: "p".into(),
        after: None,
        width,
        height,
        rgba,
    };
    let Outcome::Added { files, .. } = handle(&mut db, &tmp.folders(), request, &|_| Ok(())) else {
        anyhow::bail!("not Added");
    };
    let stored = files[0].1.as_ref().expect("stored");
    assert_eq!(stored.name, "Imagen pegada.png");
    assert_eq!(stored.mime, "image/png");
    assert_eq!((stored.width, stored.height), (Some(4), Some(3)));

    let Outcome::Preview(id, preview) = handle(
        &mut db,
        &tmp.folders(),
        Request::Preview(stored.id.clone()),
        &|_| Ok(()),
    ) else {
        anyhow::bail!("not Preview");
    };
    assert_eq!(id, stored.id);
    let preview = preview?;
    assert_eq!((preview.width, preview.height), (4, 3));
    assert_eq!(&preview.rgba[..4], &[255, 0, 0, 255]);

    // Not an image: explained.
    let ids = add(&mut db, &tmp, &[("nota.txt", b"hola")])?;
    let Outcome::Preview(_, refused) = handle(
        &mut db,
        &tmp.folders(),
        Request::Preview(ids[0].clone()),
        &|_| Ok(()),
    ) else {
        anyhow::bail!("not Preview");
    };
    assert!(refused.is_err());
    // A pasted image that does not match its size is not stored.
    let request = Request::AddImage {
        page: "p".into(),
        after: None,
        width: 100,
        height: 100,
        rgba: vec![0; 4],
    };
    let Outcome::Added { files, .. } = handle(&mut db, &tmp.folders(), request, &|_| Ok(())) else {
        anyhow::bail!("not Added");
    };
    assert!(files[0].1.is_err());
    Ok(())
}

/// Prints how long a large image takes to become a preview (run with
/// `--release --ignored --nocapture`). It runs on the attach worker.
#[test]
#[ignore = "timing smoke test; run manually"]
fn preview_timing_smoke() -> anyhow::Result<()> {
    use silex_desktop::attachment::{decode_preview, encode_png};
    for (w, h) in [(1200u32, 800u32), (4000, 3000), (8000, 6000)] {
        let rgba: Vec<u8> = (0..w * h)
            .flat_map(|i| [(i % 251) as u8, (i % 97) as u8, (i / w % 255) as u8, 255])
            .collect();
        let png = encode_png(w, h, rgba)?;
        let t = std::time::Instant::now();
        let preview = decode_preview("image/png", &png)?;
        println!(
            "{w} × {h} ({:.1} MB PNG) → {} × {}: {:?}",
            png.len() as f64 / 1_048_576.0,
            preview.width,
            preview.height,
            t.elapsed()
        );
    }
    Ok(())
}
