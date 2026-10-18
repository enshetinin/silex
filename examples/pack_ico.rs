//! Packs PNG files into one Windows `.ico` (each image stored as PNG, which
//! Windows Vista and later read). Used by `scripts/make-icons.sh`.
//! Usage: cargo run --example pack_ico -- out.ico 16.png 32.png … 256.png
use anyhow::{Context, Result, bail, ensure};

/// Width and height from a PNG's IHDR chunk.
fn png_size(bytes: &[u8]) -> Result<(u32, u32)> {
    const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
    ensure!(
        bytes.len() > 24 && bytes.starts_with(SIGNATURE) && &bytes[12..16] == b"IHDR",
        "no es un PNG"
    );
    let read =
        |at: usize| u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    Ok((read(16), read(20)))
}

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let out = args
        .next()
        .context("Uso: cargo run --example pack_ico -- salida.ico entrada.png…")?;
    let images: Vec<Vec<u8>> = args
        .map(|path| std::fs::read(&path).with_context(|| format!("{}", path.to_string_lossy())))
        .collect::<Result<_>>()?;
    ensure!(!images.is_empty(), "Faltan los PNG de entrada");
    ensure!(images.len() <= usize::from(u16::MAX), "Demasiadas imágenes");

    // ICONDIR, then one 16-byte ICONDIRENTRY per image, then the images.
    let mut ico = Vec::new();
    ico.extend_from_slice(&0u16.to_le_bytes()); // reserved
    ico.extend_from_slice(&1u16.to_le_bytes()); // type: icon
    ico.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len();
    for image in &images {
        let (width, height) = png_size(image)?;
        if width > 256 || height > 256 {
            bail!("Un .ico admite como mucho 256×256 (hay {width}×{height})");
        }
        // 0 means 256 in the one-byte size fields.
        ico.push(if width == 256 { 0 } else { width as u8 });
        ico.push(if height == 256 { 0 } else { height as u8 });
        ico.push(0); // no palette
        ico.push(0); // reserved
        ico.extend_from_slice(&1u16.to_le_bytes()); // color planes
        ico.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        ico.extend_from_slice(&(image.len() as u32).to_le_bytes());
        ico.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += image.len();
    }
    for image in &images {
        ico.extend_from_slice(image);
    }
    std::fs::write(&out, ico).with_context(|| format!("{}", out.to_string_lossy()))?;
    Ok(())
}
