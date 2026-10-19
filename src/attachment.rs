//! Attachments (ADR 0007), without SQL or egui: the size limit, the type of
//! a file from its content, image dimensions read from the header (nothing
//! is decoded), names that are safe to show, and which files are never
//! opened from Silex (programs and scripts).
use std::{fs, io::Read, path::Path};

use anyhow::{Context, Result, bail};

/// Largest attachment, in bytes (20 MB). Also a CHECK in migration 0006.
pub const MAX_SIZE: u64 = 20 * 1024 * 1024;

/// Longest name kept, in characters.
const MAX_NAME_CHARS: usize = 255;

/// An attachment as listed (without its data).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    pub id: String,
    pub name: String,
    pub mime: String,
    pub size: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

impl Attachment {
    pub fn kind(&self) -> FileKind {
        FileKind::of(&self.mime)
    }

    /// Whether Silex refuses to open it (a program or a script).
    pub fn is_executable(&self) -> bool {
        is_executable_name(&self.name) || self.mime == EXECUTABLE
    }
}

/// A file read from disk (or the clipboard), checked and ready to store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAttachment {
    pub name: String,
    pub mime: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub data: Vec<u8>,
}

impl NewAttachment {
    /// Reads the file at `path`. A folder, a file over [`MAX_SIZE`] (checked
    /// before reading it, and again while reading) or an unreadable file is
    /// an error that says why.
    pub fn read(path: &Path) -> Result<Self> {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let shown = display_name(&name);
        let meta = fs::metadata(path).with_context(|| format!("No se puede leer «{shown}»"))?;
        if !meta.is_file() {
            bail!("«{shown}» no es un archivo (las carpetas no se adjuntan)");
        }
        if meta.len() > MAX_SIZE {
            bail!(
                "«{shown}» pesa {}; el límite es {}",
                format_size(meta.len()),
                format_size(MAX_SIZE)
            );
        }
        let mut data = Vec::with_capacity(meta.len() as usize);
        fs::File::open(path)
            .and_then(|f| f.take(MAX_SIZE + 1).read_to_end(&mut data))
            .with_context(|| format!("No se puede leer «{shown}»"))?;
        Self::from_bytes(&name, data)
    }

    /// Checks `data` and works out its type and, for an image, its size.
    pub fn from_bytes(name: &str, data: Vec<u8>) -> Result<Self> {
        let name = display_name(name);
        if data.len() as u64 > MAX_SIZE {
            bail!("«{name}» pesa más de {}, el límite", format_size(MAX_SIZE));
        }
        let mime = sniff(&data);
        let (width, height) = match image_size(mime, &data) {
            Some((w, h)) => (Some(w), Some(h)),
            None => (None, None),
        };
        Ok(Self {
            name,
            mime: mime.to_owned(),
            width,
            height,
            data,
        })
    }
}

/// The type stored for programs (by their content).
const EXECUTABLE: &str = "application/x-executable";

/// The type of a file from the first bytes of its content, never from its
/// name: a «.png» that is not a PNG is just a file.
pub fn sniff(data: &[u8]) -> &'static str {
    let starts = |magic: &[u8]| data.starts_with(magic);
    if starts(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if starts(b"\xFF\xD8\xFF") {
        "image/jpeg"
    } else if starts(b"GIF87a") || starts(b"GIF89a") {
        "image/gif"
    } else if starts(b"RIFF") && data.get(8..12) == Some(b"WEBP") {
        "image/webp"
    } else if starts(b"%PDF-") {
        "application/pdf"
    } else if starts(b"PK\x03\x04") || starts(b"PK\x05\x06") {
        "application/zip"
    } else if starts(b"ID3") || starts(b"\xFF\xFB") || starts(b"fLaC") || starts(b"OggS") {
        "audio/mpeg"
    } else if data.get(4..8) == Some(b"ftyp") {
        "video/mp4"
    } else if starts(b"MZ")
        || starts(b"\x7FELF")
        || starts(b"#!")
        || [
            b"\xFE\xED\xFA\xCE",
            b"\xFE\xED\xFA\xCF",
            b"\xCE\xFA\xED\xFE",
            b"\xCF\xFA\xED\xFE",
            b"\xCA\xFE\xBA\xBE",
        ]
        .iter()
        .any(|m| starts(*m))
    {
        EXECUTABLE
    } else {
        "application/octet-stream"
    }
}

/// Width and height of a PNG, JPEG or GIF from its header. `None` for
/// anything else, or a header that does not make sense.
pub fn image_size(mime: &str, data: &[u8]) -> Option<(u32, u32)> {
    let be16 = |at: usize| Some(u16::from_be_bytes(data.get(at..at + 2)?.try_into().ok()?) as u32);
    let size = match mime {
        // IHDR is the first chunk: width and height at 16 and 20.
        "image/png" if data.get(12..16) == Some(b"IHDR") => {
            let be32 = |at: usize| Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?));
            (be32(16)?, be32(20)?)
        }
        "image/gif" => {
            let le16 =
                |at: usize| Some(u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?) as u32);
            (le16(6)?, le16(8)?)
        }
        "image/jpeg" => {
            // Walk the segments up to the first frame header (SOFn).
            let mut at = 2;
            loop {
                if *data.get(at)? != 0xFF {
                    return None;
                }
                let marker = *data.get(at + 1)?;
                let is_frame =
                    matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
                if is_frame {
                    break (be16(at + 7)?, be16(at + 5)?);
                }
                at += 2 + usize::try_from(be16(at + 2)?).ok()?;
            }
        }
        _ => return None,
    };
    (size.0 > 0 && size.1 > 0).then_some(size)
}

/// A file's name as stored and shown: no control characters, no
/// direction overrides (which can make «fdp.exe» read as «exe.pdf»), no
/// folders, trimmed and at most 255 characters. Never empty.
pub fn display_name(raw: &str) -> String {
    // Only the last part of a path (`a/b\c.txt` → `c.txt`).
    let last = raw.rsplit(['/', '\\']).next().unwrap_or_default();
    let clean: String = last
        .chars()
        .filter(|c| {
            !c.is_control()
                && !matches!(c, '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
        })
        .take(MAX_NAME_CHARS)
        .collect();
    match clean.trim() {
        "" => "archivo".to_owned(),
        name => name.to_owned(),
    }
}

/// Extensions of programs and scripts, never opened from Silex.
const EXECUTABLE_EXTENSIONS: &[&str] = &[
    "app",
    "applescript",
    "bat",
    "bin",
    "cmd",
    "com",
    "command",
    "cpl",
    "deb",
    "dmg",
    "exe",
    "gadget",
    "hta",
    "inf",
    "ins",
    "jar",
    "js",
    "jse",
    "lnk",
    "msc",
    "msi",
    "msp",
    "pif",
    "pkg",
    "ps1",
    "ps2",
    "psc1",
    "reg",
    "rpm",
    "run",
    "scf",
    "scr",
    "sh",
    "url",
    "vb",
    "vbe",
    "vbs",
    "ws",
    "wsc",
    "wsf",
    "wsh",
    "workflow",
    "appimage",
    "desktop",
    "py",
    "pl",
    "rb",
    "terminal",
];

fn is_executable_name(name: &str) -> bool {
    // Windows drops trailing dots and spaces: «a.bat.» runs as «a.bat».
    let name = display_name(name);
    let name = name.trim_end_matches([' ', '.']);
    let Some((_, extension)) = name.rsplit_once('.') else {
        return false;
    };
    let extension = extension.to_ascii_lowercase();
    EXECUTABLE_EXTENSIONS.contains(&extension.as_str())
}

/// «512 B», «2,4 KB», «13 MB»: one decimal under ten, with a comma.
pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        return format!("{bytes} B");
    }
    let text = if value < 10.0 {
        format!("{value:.1}").replace('.', ",")
    } else {
        format!("{value:.0}")
    };
    format!("{text} {}", UNITS[unit])
}

/// A broad family of file, for its icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Image,
    Pdf,
    Archive,
    Audio,
    Video,
    Other,
}

impl FileKind {
    pub fn of(mime: &str) -> Self {
        match mime {
            m if m.starts_with("image/") => FileKind::Image,
            "application/pdf" => FileKind::Pdf,
            "application/zip" => FileKind::Archive,
            m if m.starts_with("audio/") => FileKind::Audio,
            m if m.starts_with("video/") => FileKind::Video,
            _ => FileKind::Other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal PNG header: signature and IHDR with `w` × `h`.
    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut data = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        data.extend(w.to_be_bytes());
        data.extend(h.to_be_bytes());
        data.extend([8, 6, 0, 0, 0]);
        data
    }

    /// A JPEG with an APP0 segment before its frame header (SOF0).
    fn jpeg(w: u16, h: u16) -> Vec<u8> {
        let mut data = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00];
        data.extend([0xFF, 0xC0, 0x00, 0x11, 0x08]);
        data.extend(h.to_be_bytes());
        data.extend(w.to_be_bytes());
        data.extend([3, 1, 0x22, 0]);
        data
    }

    #[test]
    fn types_come_from_the_content_not_the_name() {
        assert_eq!(sniff(&png(1, 1)), "image/png");
        assert_eq!(sniff(&jpeg(1, 1)), "image/jpeg");
        assert_eq!(sniff(b"GIF89a\x02\0\x03\0"), "image/gif");
        assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), "image/webp");
        assert_eq!(sniff(b"%PDF-1.7\n"), "application/pdf");
        assert_eq!(sniff(b"PK\x03\x04rest"), "application/zip");
        assert_eq!(sniff(b"MZ\x90\0"), EXECUTABLE);
        assert_eq!(sniff(b"\x7FELF\x02"), EXECUTABLE);
        assert_eq!(sniff(b"#!/bin/sh\nrm -rf ~"), EXECUTABLE);
        assert_eq!(sniff(b"\xCF\xFA\xED\xFE"), EXECUTABLE);
        assert_eq!(sniff(b"hola"), "application/octet-stream");
        assert_eq!(sniff(b""), "application/octet-stream");
        // A «.png» that is text is just a file, without dimensions.
        let fake =
            NewAttachment::from_bytes("foto.png", b"no soy una imagen".to_vec()).expect("small");
        assert_eq!(fake.mime, "application/octet-stream");
        assert_eq!((fake.width, fake.height), (None, None));
    }

    #[test]
    fn image_sizes_are_read_from_the_header() {
        assert_eq!(image_size("image/png", &png(640, 480)), Some((640, 480)));
        assert_eq!(
            image_size("image/jpeg", &jpeg(1920, 1080)),
            Some((1920, 1080))
        );
        assert_eq!(image_size("image/gif", b"GIF89a\x02\0\x03\0"), Some((2, 3)));
        // Truncated, zero-sized or nonsense headers give no size, never a panic.
        assert_eq!(image_size("image/png", &png(640, 480)[..18]), None);
        assert_eq!(image_size("image/png", &png(0, 480)), None);
        let mut broken = jpeg(10, 10);
        broken[2] = 0x00;
        assert_eq!(image_size("image/jpeg", &broken), None);
        assert_eq!(
            image_size("image/jpeg", &[0xFF, 0xD8, 0xFF, 0xE0, 0xFF, 0xFF]),
            None
        );
        assert_eq!(image_size("image/jpeg", &jpeg(10, 10)[..12]), None);
        assert_eq!(image_size("application/pdf", b"%PDF-"), None);
        let new = NewAttachment::from_bytes("plano.png", png(800, 600)).expect("small");
        assert_eq!((new.width, new.height), (Some(800), Some(600)));
    }

    #[test]
    fn names_are_safe_to_show() {
        assert_eq!(display_name("informe.pdf"), "informe.pdf");
        assert_eq!(display_name("../../etc/passwd"), "passwd");
        assert_eq!(display_name("C:\\Users\\a\\nota.txt"), "nota.txt");
        assert_eq!(display_name("  hola\n\t.txt "), "hola.txt");
        // A direction override (U+202E) would show «factura\u{202E}fdp.exe»
        // as «facturaexe.pdf».
        assert_eq!(display_name("factura\u{202E}fdp.exe"), "facturafdp.exe");
        assert_eq!(display_name(""), "archivo");
        assert_eq!(display_name("\u{202E}"), "archivo");
        assert_eq!(display_name("dir/"), "archivo");
        assert_eq!(display_name(&"ñ".repeat(400)).chars().count(), 255);
        assert_eq!(display_name("señal 🌾.txt"), "señal 🌾.txt");
    }

    #[test]
    fn programs_and_scripts_are_recognised() {
        let file = |name: &str, mime: &str| Attachment {
            id: "a".into(),
            name: name.into(),
            mime: mime.into(),
            size: 1,
            width: None,
            height: None,
        };
        for name in [
            "setup.exe",
            "SETUP.EXE",
            "x.app",
            "run.sh",
            "a.command",
            "b.ps1",
            "c.jar",
            "d.bat.",
            "e.msi ",
        ] {
            assert!(
                file(name, "application/octet-stream").is_executable(),
                "{name}"
            );
        }
        assert!(file("factura\u{202E}fdp.exe", "application/octet-stream").is_executable());
        for name in [
            "informe.pdf",
            "foto.jpeg",
            "notas",
            "exe",
            "archivo.exe.pdf",
        ] {
            assert!(
                !file(name, "application/octet-stream").is_executable(),
                "{name}"
            );
        }
        // A program renamed «.pdf» is still a program.
        assert!(file("informe.pdf", EXECUTABLE).is_executable());
    }

    #[test]
    fn sizes_read_like_people_write_them() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1,0 KB");
        assert_eq!(format_size(2_516_582), "2,4 MB");
        assert_eq!(format_size(MAX_SIZE), "20 MB");
        assert_eq!(format_size(3 * 1024 * 1024 * 1024), "3,0 GB");
    }

    #[test]
    fn files_over_the_limit_or_folders_are_refused() -> anyhow::Result<()> {
        let dir = std::env::temp_dir().join(format!("silex-attach-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir)?;
        let result = (|| -> anyhow::Result<()> {
            let big = dir.join("grande.bin");
            fs::File::create(&big)?.set_len(MAX_SIZE + 1)?;
            let err = NewAttachment::read(&big).expect_err("too big");
            assert!(format!("{err:#}").contains("el límite es 20 MB"), "{err:#}");
            let err = NewAttachment::read(&dir).expect_err("a folder");
            assert!(format!("{err:#}").contains("no es un archivo"), "{err:#}");
            assert!(NewAttachment::read(&dir.join("no-existe")).is_err());

            let exact = dir.join("justo.bin");
            fs::File::create(&exact)?.set_len(MAX_SIZE)?;
            assert_eq!(NewAttachment::read(&exact)?.data.len() as u64, MAX_SIZE);
            let small = dir.join("plano.png");
            fs::write(&small, png(3, 2))?;
            let read = NewAttachment::read(&small)?;
            assert_eq!(
                (read.name.as_str(), read.mime.as_str()),
                ("plano.png", "image/png")
            );
            assert_eq!(read.width, Some(3));
            Ok(())
        })();
        fs::remove_dir_all(&dir)?;
        result
    }

    #[test]
    fn file_kinds_follow_the_type() {
        assert_eq!(FileKind::of("image/png"), FileKind::Image);
        assert_eq!(FileKind::of("application/pdf"), FileKind::Pdf);
        assert_eq!(FileKind::of("application/zip"), FileKind::Archive);
        assert_eq!(FileKind::of("audio/mpeg"), FileKind::Audio);
        assert_eq!(FileKind::of("video/mp4"), FileKind::Video);
        assert_eq!(FileKind::of(EXECUTABLE), FileKind::Other);
    }
}
