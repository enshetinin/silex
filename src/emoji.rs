//! Emoji for page icons (H12.8), without egui: which text is one emoji,
//! and the emoji the picker offers, with Spanish words to find them.
//!
//! The app draws them with the system's emoji font (eframe's
//! `system_fonts`), in color where the system has one.
use crate::editor::fold;

/// Whether `text` is exactly one emoji (with its skin tone, variation
/// selector, ZWJ sequence or flag): what a page icon may be.
pub fn is_icon(text: &str) -> bool {
    matches!(leading_emoji(text), Some((emoji, rest)) if rest.is_empty() && emoji == text)
}

/// The emoji `text` starts with, and the rest after it. `None` if it does
/// not start with one.
pub fn leading_emoji(text: &str) -> Option<(&str, &str)> {
    let mut chars = text.char_indices().peekable();
    let (_, first) = chars.next()?;
    let mut end = first.len_utf8();
    if is_regional_indicator(first) {
        // A flag is two regional indicators.
        let (at, second) = chars.next()?;
        if !is_regional_indicator(second) {
            return None;
        }
        end = at + second.len_utf8();
        return Some((&text[..end], &text[end..]));
    }
    // A keycap: a digit, # or * with U+FE0F and U+20E3.
    let keycap = first.is_ascii_digit() || first == '#' || first == '*';
    if !(is_pictographic(first) || keycap) {
        return None;
    }
    let mut joined = false;
    let mut capped = false;
    while let Some(&(at, c)) = chars.peek() {
        let continues = match c {
            '\u{FE0F}' | '\u{FE0E}' => true,
            '\u{20E3}' => {
                capped = true;
                true
            }
            '\u{1F3FB}'..='\u{1F3FF}' | '\u{E0020}'..='\u{E007F}' => true,
            '\u{200D}' => {
                joined = true;
                true
            }
            c if joined && is_pictographic(c) => {
                joined = false;
                true
            }
            _ => false,
        };
        if !continues {
            break;
        }
        end = at + c.len_utf8();
        chars.next();
    }
    if keycap && !capped {
        return None;
    }
    Some((&text[..end], &text[end..]))
}

/// Characters that start an emoji (approximately Unicode's
/// Extended_Pictographic, without the ASCII ones).
fn is_pictographic(c: char) -> bool {
    matches!(c,
        '\u{00A9}' | '\u{00AE}' | '\u{203C}' | '\u{2049}' | '\u{2122}' | '\u{2139}'
        | '\u{2194}'..='\u{21AA}'
        | '\u{231A}'..='\u{23FF}'
        | '\u{24C2}'
        | '\u{25AA}'..='\u{25FE}'
        | '\u{2600}'..='\u{27BF}'
        | '\u{2934}' | '\u{2935}'
        | '\u{2B05}'..='\u{2B55}'
        | '\u{3030}' | '\u{303D}' | '\u{3297}' | '\u{3299}'
        | '\u{1F000}'..='\u{1F1E5}'
        | '\u{1F200}'..='\u{1FAFF}'
    )
}

fn is_regional_indicator(c: char) -> bool {
    ('\u{1F1E6}'..='\u{1F1FF}').contains(&c)
}

/// The emoji the picker offers, by group, each with the words it answers
/// to (Spanish, without accents, as typed in the search).
pub const GROUPS: &[(&str, &[(&str, &str)])] = &[
    (
        "Uso frecuente",
        &[
            ("📄", "pagina documento hoja"),
            ("📝", "nota escribir apunte lapiz"),
            ("📒", "libreta cuaderno notas"),
            ("📚", "libros estudio biblioteca lectura"),
            ("📌", "chincheta fijar importante"),
            ("⭐", "estrella favorito"),
            ("✅", "hecho tarea completado check"),
            ("💡", "idea bombilla luz"),
            ("🚀", "cohete proyecto lanzamiento"),
            ("🎯", "objetivo meta diana"),
            ("📅", "calendario fecha dia agenda"),
            ("🗓️", "calendario planificar semana"),
            ("⏰", "alarma reloj hora tiempo"),
            ("🔥", "fuego urgente prioridad"),
            // Not ❤️: Inter has a one-colour ❤ of its own, which would win.
            ("💖", "corazon amor"),
            ("🏠", "casa hogar inicio"),
        ],
    ),
    (
        "Trabajo",
        &[
            ("💼", "trabajo maletin oficina negocio"),
            ("📊", "grafico barras datos estadisticas"),
            ("📈", "grafico subir crecimiento ventas"),
            ("📉", "grafico bajar"),
            ("🗂️", "archivo carpetas fichero"),
            ("📁", "carpeta archivo"),
            ("📂", "carpeta abierta"),
            ("📋", "portapapeles lista tareas"),
            ("📎", "clip adjunto"),
            ("🖊️", "boligrafo firmar"),
            ("✏️", "lapiz editar escribir"),
            ("📧", "correo email mensaje"),
            ("📞", "telefono llamada"),
            ("💬", "mensaje conversacion chat"),
            ("🤝", "acuerdo reunion trato"),
            ("🧾", "factura recibo cuentas"),
            ("💰", "dinero bolsa finanzas"),
            ("💶", "euro dinero billete"),
            ("🏦", "banco finanzas"),
            ("⚖️", "balanza ley justicia"),
            ("🔑", "llave clave acceso contrasena"),
            ("🔒", "candado privado seguridad"),
            ("🛠️", "herramientas arreglar"),
            ("⚙️", "ajustes engranaje configuracion"),
            ("🧩", "pieza puzle plugin"),
        ],
    ),
    (
        "Tecnología",
        &[
            ("💻", "portatil ordenador codigo"),
            ("🖥️", "ordenador pantalla escritorio"),
            ("⌨️", "teclado"),
            ("📱", "movil telefono app"),
            ("🌐", "web internet mundo red"),
            ("🐛", "bug error insecto fallo"),
            ("🧪", "prueba test experimento laboratorio"),
            ("🤖", "robot ia automatizar"),
            ("🔌", "enchufe conectar"),
            ("💾", "disquete guardar"),
            ("🗄️", "archivador base datos"),
            ("🔍", "lupa buscar investigar"),
            ("🔧", "llave inglesa arreglar"),
            ("📦", "paquete caja envio"),
            ("🧠", "cerebro pensar mente"),
        ],
    ),
    (
        "Estudio y arte",
        &[
            ("🎓", "graduacion universidad estudios"),
            ("🏫", "escuela colegio"),
            ("📖", "libro abierto leer"),
            ("✍️", "escribir mano"),
            ("🧮", "abaco matematicas calculo"),
            ("🔬", "microscopio ciencia"),
            ("🌍", "mundo tierra geografia europa"),
            ("🎨", "arte pintura diseno paleta"),
            ("🎵", "musica nota cancion"),
            ("🎸", "guitarra musica"),
            ("🎬", "cine pelicula video"),
            ("📷", "camara foto fotografia"),
            ("🎮", "juego videojuego"),
            ("♟️", "ajedrez estrategia"),
            ("🎭", "teatro"),
        ],
    ),
    (
        "Casa y vida",
        &[
            ("🍞", "pan panaderia"),
            ("🍳", "cocina receta huevo"),
            ("🥗", "ensalada comida sana"),
            ("🍎", "manzana fruta"),
            ("☕", "cafe desayuno"),
            ("🍷", "vino copa"),
            ("🛒", "compra carrito supermercado"),
            ("🧹", "limpiar escoba"),
            ("🛏️", "cama dormir"),
            ("👕", "ropa camiseta"),
            ("💊", "medicina pastilla salud"),
            ("🏃", "correr deporte ejercicio"),
            ("🧘", "meditacion yoga calma"),
            ("💪", "fuerza gimnasio"),
            ("🚲", "bici bicicleta"),
            ("🐶", "perro mascota"),
            ("🐱", "gato mascota"),
            ("👶", "bebe nino familia"),
            ("👨‍👩‍👧", "familia"),
            ("🎁", "regalo cumpleanos"),
            ("🎉", "fiesta celebrar"),
        ],
    ),
    (
        "Viajes y lugares",
        &[
            ("✈️", "avion viaje vuelo"),
            ("🧳", "maleta viaje equipaje"),
            ("🗺️", "mapa viaje ruta"),
            ("🏖️", "playa vacaciones"),
            ("⛰️", "montana monte"),
            ("🏕️", "camping acampada"),
            ("🚗", "coche viaje"),
            ("🚆", "tren viaje"),
            ("🏙️", "ciudad"),
            ("🏢", "edificio oficina empresa"),
            ("📍", "lugar ubicacion sitio"),
            ("🇪🇸", "espana bandera"),
        ],
    ),
    (
        "Naturaleza",
        &[
            ("🌾", "espiga trigo campo cereal"),
            ("🌱", "brote planta crecer semilla"),
            ("🌳", "arbol"),
            ("🌸", "flor primavera"),
            ("🍂", "hojas otono"),
            ("☀️", "sol verano"),
            ("🌙", "luna noche"),
            ("⛅", "nubes tiempo clima"),
            ("❄️", "nieve invierno frio"),
            ("🌊", "ola mar agua"),
            ("⚡", "rayo energia rapido"),
            ("🌈", "arcoiris"),
        ],
    ),
    (
        "Símbolos",
        &[
            ("❓", "pregunta duda"),
            ("❗", "exclamacion importante"),
            ("⚠️", "aviso cuidado alerta"),
            ("🚫", "prohibido no"),
            ("❌", "cruz error no"),
            ("✨", "brillo nuevo magia"),
            ("🔔", "campana aviso notificacion"),
            ("🏷️", "etiqueta"),
            ("🔗", "enlace link cadena"),
            ("♻️", "reciclar"),
            ("🟢", "verde circulo estado"),
            ("🟡", "amarillo circulo estado"),
            ("🔴", "rojo circulo estado"),
            ("🔵", "azul circulo"),
            ("1️⃣", "uno numero"),
            ("2️⃣", "dos numero"),
            ("3️⃣", "tres numero"),
        ],
    ),
];

/// The emoji whose words have every word of `query` (without accents or
/// case), in the picker's order. With nothing typed, all of them.
pub fn search(query: &str) -> Vec<&'static str> {
    let words: Vec<String> = fold(query).split_whitespace().map(str::to_owned).collect();
    GROUPS
        .iter()
        .flat_map(|(_, emoji)| emoji.iter())
        .filter(|(_, keys)| {
            words
                .iter()
                .all(|w| keys.split(' ').any(|k| k.starts_with(w)))
        })
        .map(|(emoji, _)| *emoji)
        .collect()
}

/// The first word of an emoji's description, for its accessible name.
pub fn name(emoji: &str) -> Option<&'static str> {
    GROUPS
        .iter()
        .flat_map(|(_, list)| list.iter())
        .find(|(e, _)| *e == emoji)
        .and_then(|(_, keys)| keys.split(' ').next())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_emoji_is_an_icon_and_anything_else_is_not() {
        for icon in [
            "🌾",
            "❤️",
            "👍🏽",
            "👨‍👩‍👧",
            "🏳️‍🌈",
            "🇪🇸",
            "1️⃣",
            "#️⃣",
            "☕",
            "✈️",
            "🧑‍💻",
        ] {
            assert!(is_icon(icon), "{icon:?}");
        }
        for not in [
            "", "a", "1", "🌾🌾", "🌾 ", " 🌾", "🌾a", "🇪", "#", "abc", "→",
        ] {
            assert!(!is_icon(not), "{not:?}");
        }
    }

    #[test]
    fn leading_emoji_splits_the_rest_off() {
        assert_eq!(leading_emoji("🌾 Recetas"), Some(("🌾", " Recetas")));
        assert_eq!(leading_emoji("👨‍👩‍👧 Familia"), Some(("👨‍👩‍👧", " Familia")));
        assert_eq!(leading_emoji("🇪🇸España"), Some(("🇪🇸", "España")));
        assert_eq!(leading_emoji("Recetas 🌾"), None);
        assert_eq!(leading_emoji("12 de mayo"), None, "a digit is not a keycap");
    }

    #[test]
    fn every_emoji_offered_is_an_icon_once_and_has_words() {
        let mut seen = std::collections::HashSet::new();
        for (group, list) in GROUPS {
            assert!(!list.is_empty(), "{group}");
            for (emoji, keys) in *list {
                assert!(is_icon(emoji), "{group}: {emoji:?}");
                assert!(seen.insert(*emoji), "{emoji} twice");
                assert!(!keys.is_empty() && fold(keys) == *keys, "{emoji}: {keys}");
            }
        }
        assert!(seen.len() > 100);
    }

    #[test]
    fn search_finds_by_spanish_words_without_accents() {
        assert_eq!(search("trigo"), ["🌾"]);
        assert!(search("CAFÉ").contains(&"☕"));
        assert!(search("cal").contains(&"📅"), "word starts");
        assert_eq!(search("viaje avion"), ["✈️"], "every word");
        assert!(search("zzz").is_empty());
        assert_eq!(search("").len(), search("  ").len());
        assert_eq!(name("🌾"), Some("espiga"));
    }
}
