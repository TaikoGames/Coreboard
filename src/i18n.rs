//! The app's languages. Every text shown to the user goes through [`tr`] (or [`trf`] when it has
//! values filled in), written in English; when Spanish is chosen, the Spanish line is shown
//! instead. Spanish is picked automatically when the computer is set to Spanish, and can be
//! changed in the Boards window.

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    English,
    Spanish,
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// The language setting as stored: "auto" follows the computer, "en" or "es" picks one.
pub fn apply(setting: &str) {
    let lang = match setting {
        "en" => Lang::English,
        "es" => Lang::Spanish,
        _ => system(),
    };
    CURRENT.store(lang as u8, Ordering::Relaxed);
}

/// The computer's language, if it's one Core Board speaks.
pub fn system() -> Lang {
    let locale = sys_locale::get_locale().unwrap_or_default().to_lowercase();
    if locale.starts_with("es") {
        Lang::Spanish
    } else {
        Lang::English
    }
}

pub fn current() -> Lang {
    match CURRENT.load(Ordering::Relaxed) {
        1 => Lang::Spanish,
        _ => Lang::English,
    }
}

/// `english` in the current language.
pub fn tr(english: &'static str) -> &'static str {
    match current() {
        Lang::English => english,
        Lang::Spanish => spanish().get(english).copied().unwrap_or(english),
    }
}

/// [`tr`], then each `{}` replaced by the next of `values`.
pub fn trf(english: &'static str, values: &[&dyn Display]) -> String {
    fill(tr(english), values)
}

fn fill(template: &str, values: &[&dyn Display]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut values = values.iter();
    let mut rest = template;
    while let Some(at) = rest.find("{}") {
        out.push_str(&rest[..at]);
        if let Some(v) = values.next() {
            out.push_str(&v.to_string());
        }
        rest = &rest[at + 2..];
    }
    out.push_str(rest);
    out
}

fn spanish() -> &'static HashMap<&'static str, &'static str> {
    static TABLE: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    TABLE.get_or_init(|| SPANISH.iter().copied().collect())
}

/// English -> Spanish. Keep `{}` placeholders in the same order.
const SPANISH: &[(&str, &str)] = &[
    // Radial menu
    ("Similar\nvalues", "Valores\nsimilares"),
    ("Similar\npalette", "Paleta\nsimilar"),
    ("Search\nweb", "Buscar\nen la web"),
    ("Extract\npalette", "Sacar\npaleta"),
    ("Study\nview", "Vista de\nestudio"),
    ("Guide", "Guía"),
    ("Crop", "Recortar"),
    ("Note", "Nota"),
    ("Focus\nview", "Vista de\nenfoque"),
    ("Link", "Enlace"),
    ("Image\nslot", "Hueco de\nimagen"),
    ("Paste", "Pegar"),
    ("Fit\nview", "Ver\ntodo"),
    ("Surprise\nme", "Sorpréndeme"),
    ("Help", "Ayuda"),
    // Messages on the board
    (
        "{} locked item(s) kept · unlock with L",
        "{} elemento(s) bloqueado(s) se quedan · desbloquea con L",
    ),
    ("Select images first", "Selecciona imágenes primero"),
    ("Golden spiral turned", "Espiral áurea girada"),
    ("Value", "Valor"),
    ("Duplicated", "Duplicado"),
    ("Duplicated {} items", "{} elementos duplicados"),
    (
        "Values: whole board in grayscale",
        "Valores: todo el tablero en grises",
    ),
    (
        "Values: whole board in 3 tones",
        "Valores: todo el tablero en 3 tonos",
    ),
    ("Back to color", "De vuelta al color"),
    (
        "Select images to extract colors from",
        "Selecciona imágenes para sacar sus colores",
    ),
    ("Extracting colors…", "Sacando colores…"),
    (
        "Color extraction failed: {}",
        "No se pudieron sacar los colores: {}",
    ),
    ("Crop done", "Recorte hecho"),
    (
        "Select one image to crop",
        "Selecciona una imagen para recortar",
    ),
    (
        "Drag over the image to crop · Enter/Esc when done · Shift+X resets",
        "Arrastra sobre la imagen para recortar · Enter/Esc al terminar · Shift+X lo deshace",
    ),
    ("Crop reset", "Recorte deshecho"),
    ("Untitled", "Sin título"),
    (
        "Name this slot, e.g. Front view",
        "Nombra este hueco, p. ej. Vista frontal",
    ),
    (
        "Add a caption, e.g. Sargent, 1885, rim light",
        "Añade un pie de foto, p. ej. Sargent, 1885, luz de contorno",
    ),
    (
        "That file isn't an image I can read",
        "Ese archivo no es una imagen que pueda leer",
    ),
    (
        "That isn't a picture I can read",
        "Eso no es una imagen que pueda leer",
    ),
    (
        "Clipboard has no image",
        "No hay ninguna imagen en el portapapeles",
    ),
    ("Downloading picture…", "Descargando imagen…"),
    (
        "Couldn't get that picture: {}",
        "No se pudo conseguir esa imagen: {}",
    ),
    // Export
    ("all {} images", "las {} imágenes"),
    ("{} selected image(s)", "{} imagen(es) seleccionada(s)"),
    ("Export PNG", "Exportar PNG"),
    ("Exports {} as one PNG.", "Exporta {} en un solo PNG."),
    (
        "Watermark text (leave empty for none):",
        "Texto de la marca de agua (vacío para ninguna):",
    ),
    ("e.g. © Your Name", "p. ej. © Tu Nombre"),
    ("Position", "Posición"),
    ("Opacity", "Opacidad"),
    ("Exporting…", "Exportando…"),
    ("Export", "Exportar"),
    ("Cancel", "Cancelar"),
    ("Saved to {}", "Se guarda en {}"),
    ("Change…", "Cambiar…"),
    ("Folder for exports", "Carpeta para exportar"),
    ("Reset", "Restablecer"),
    ("Exported {}", "Exportado: {}"),
    ("✓ Exported {}", "✓ Exportado: {}"),
    ("Open folder", "Abrir carpeta"),
    ("Export failed: {}", "No se pudo exportar: {}"),
    ("Image to find", "Imagen por encontrar"),
    // Find more
    (
        "Select an image to find more like it",
        "Selecciona una imagen para encontrar otras parecidas",
    ),
    ("Find more", "Encontrar más"),
    ("Similar values", "Valores similares"),
    ("Similar palette", "Paleta similar"),
    ("Search the web", "Buscar en la web"),
    ("value structure", "estructura de valores"),
    ("colors", "colores"),
    (
        "Images on your other boards ranked by {}. Click one to copy it here.",
        "Imágenes de tus otros tableros ordenadas por {}. Haz clic en una para copiarla aquí.",
    ),
    ("Scanning your boards…", "Revisando tus tableros…"),
    (
        "No images on your other boards yet. Make more boards with Ctrl+O and drop references in; they'll show up here.",
        "Aún no hay imágenes en tus otros tableros. Crea más tableros con Ctrl+O y suelta referencias en ellos; aparecerán aquí.",
    ),
    (
        "Couldn't read this image: {}",
        "No se pudo leer esta imagen: {}",
    ),
    (
        "From {}\nClick to copy onto this board",
        "De {}\nHaz clic para copiarla a este tablero",
    ),
    (
        "Opens an image search in your browser.",
        "Abre una búsqueda de imágenes en tu navegador.",
    ),
    (
        "Subject, e.g. oak tree at dusk",
        "Tema, p. ej. roble al atardecer",
    ),
    ("Any color", "Cualquier color"),
    ("Black & white", "Blanco y negro"),
    ("{} (from this image)", "{} (de esta imagen)"),
    ("Color", "Color"),
    ("Type a subject to search.", "Escribe un tema para buscar."),
    ("Can't read {}", "No se puede leer {}"),
    // Boards window
    ("Boards", "Tableros"),
    ("New board", "Tablero nuevo"),
    (
        "Name, e.g. Character refs",
        "Nombre, p. ej. Refs de personaje",
    ),
    ("Create", "Crear"),
    (
        "Open or create in a folder…",
        "Abrir o crear en una carpeta…",
    ),
    ("Board folder", "Carpeta del tablero"),
    ("Recent", "Recientes"),
    ("No other boards yet", "Aún no hay otros tableros"),
    ("Animations", "Animaciones"),
    (
        "Glides, pops, swing and flicks. Turn off for a still board.",
        "Deslizamientos, rebotes, balanceos y lanzamientos. Desactívalas para un tablero quieto.",
    ),
    ("Mini-map", "Minimapa"),
    (
        "An overview of the whole board in the corner, while part of it is off screen. Click or drag in it to go there.",
        "Una vista de todo el tablero en la esquina cuando parte de él queda fuera de la pantalla. Haz clic o arrastra en ella para ir allí.",
    ),
    ("Close", "Cerrar"),
    ("Close the help (H)", "Cerrar la ayuda (H)"),
    ("Shortcuts and help (H)", "Atajos y ayuda (H)"),
    ("Language", "Idioma"),
    ("Automatic ({})", "Automático ({})"),
    ("Can't open {}: {}", "No se puede abrir {}: {}"),
    ("Opened {}", "Abierto: {}"),
    ("Save failed: {}", "No se pudo guardar: {}"),
    ("Saved", "Guardado"),
    // Keys
    ("Drawings shown", "Dibujos visibles"),
    ("Drawings hidden", "Dibujos ocultos"),
    (
        "Color picker on: hover an image",
        "Cuentagotas activado: pasa sobre una imagen",
    ),
    ("Color picker off", "Cuentagotas desactivado"),
    ("Always on top: on", "Siempre encima: activado"),
    ("Always on top: off", "Siempre encima: desactivado"),
    ("Opacity {}%", "Opacidad {}%"),
    // Board
    ("Drop to fill “{}”", "Suelta para rellenar «{}»"),
    ("Drop to fill this slot", "Suelta para rellenar este hueco"),
    (
        "Drop images or files here, paste with Ctrl+V, or press N for a note",
        "Suelta imágenes o archivos aquí, pega con Ctrl+V o pulsa N para una nota",
    ),
    ("{} items", "{} elementos"),
    ("opacity {}%", "opacidad {}%"),
    ("on top", "siempre encima"),
    (
        "DRAW: drag anywhere, Esc when done",
        "DIBUJO: arrastra donde quieras, Esc al terminar",
    ),
    (
        "VALUES: whole board in grayscale (Shift+B)",
        "VALORES: todo el tablero en grises (Shift+B)",
    ),
    (
        "VALUES: whole board in 3 tones (Shift+B)",
        "VALORES: todo el tablero en 3 tonos (Shift+B)",
    ),
    (
        "CROP: drag over the image, Enter when done",
        "RECORTE: arrastra sobre la imagen, Enter al terminar",
    ),
    ("{} selected", "{} seleccionado(s)"),
    ("Empty note", "Nota vacía"),
    ("shortcuts", "atajos"),
    // Cards
    (
        "Can't find {}: it was moved or deleted",
        "No se encuentra {}: se movió o se borró",
    ),
    ("Couldn't open it: {}", "No se pudo abrir: {}"),
    ("Opening…", "Abriendo…"),
    ("Opening the video…", "Abriendo el vídeo…"),
    ("Add a link", "Añadir un enlace"),
    (
        "Paste or type a web address",
        "Pega o escribe una dirección web",
    ),
    ("Add", "Añadir"),
    (
        "Double-click the card to open the page.",
        "Haz doble clic en la tarjeta para abrir la página.",
    ),
    (
        "That doesn't look like a web address",
        "Eso no parece una dirección web",
    ),
    ("Folder", "Carpeta"),
    ("Shortcut", "Acceso directo"),
    ("App", "Aplicación"),
    // Drawing
    (
        "Draw: drag anywhere · Esc or D when done",
        "Dibujar: arrastra donde quieras · Esc o D al terminar",
    ),
    ("Drawing done", "Dibujo terminado"),
    ("Drawings cleared", "Dibujos borrados"),
    ("Clear", "Borrar"),
    (
        "Remove the drawings on the selected images",
        "Quita los dibujos de las imágenes seleccionadas",
    ),
    ("Hide", "Ocultar"),
    ("Show", "Mostrar"),
    (
        "Show or hide all drawings (Shift+D)",
        "Muestra u oculta todos los dibujos (Shift+D)",
    ),
    ("Done", "Listo"),
    ("Esc or D", "Esc o D"),
    ("Pen", "Lápiz"),
    ("Arrow", "Flecha"),
    ("Circle", "Círculo"),
    ("Box", "Caja"),
    // Focus view
    ("No images to focus on", "No hay imágenes que ver"),
    (
        "Add some images first, then press S for a surprise",
        "Añade algunas imágenes primero y luego pulsa S para una sorpresa",
    ),
    ("Surprise! From \u{201c}{}\u{201d}", "¡Sorpresa! De «{}»"),
    ("image {} of {}", "imagen {} de {}"),
    (
        "← → step   S surprise   scroll zoom   drag pan   V C B M R study   Esc back",
        "← → pasar   S sorpresa   rueda zoom   arrastrar mover   V C B M R estudio   Esc volver",
    ),
    // Organizing
    ("Unlocked", "Desbloqueado"),
    ("Locked", "Bloqueado"),
    ("Unpinned", "Desfijado"),
    ("Pinned to the screen", "Fijado a la pantalla"),
    ("Select something to tag", "Selecciona algo para etiquetar"),
    ("Tags · {} selected", "Etiquetas · {} seleccionado(s)"),
    ("No tags yet", "Aún no hay etiquetas"),
    ("Remove from the selection", "Quitar de la selección"),
    (
        "Add a tag and press Enter",
        "Añade una etiqueta y pulsa Enter",
    ),
    ("On this board", "En este tablero"),
    (
        "Search tags, notes and frame titles",
        "Busca etiquetas, notas y títulos de marcos",
    ),
    ("{} found", "{} encontrado(s)"),
    ("Select", "Seleccionar"),
    (
        "Select the matches and zoom to them (Enter)",
        "Selecciona los resultados y acércate a ellos (Enter)",
    ),
    ("Close search (Esc)", "Cerrar búsqueda (Esc)"),
    (
        "Select 3 or more items to space evenly",
        "Selecciona 3 o más elementos para espaciarlos por igual",
    ),
    ("Align", "Alinear"),
    ("Space", "Espaciar"),
    ("Across", "A lo ancho"),
    ("Down", "A lo alto"),
    ("Left", "Izquierda"),
    ("Center", "Centro"),
    ("Right", "Derecha"),
    ("Top", "Arriba"),
    ("Middle", "Medio"),
    ("Bottom", "Abajo"),
    // Slots
    ("Filled “{}”", "Rellenado «{}»"),
    ("Slot filled", "Hueco rellenado"),
    ("Drop an image here", "Suelta una imagen aquí"),
    // Updates
    ("Checking…", "Comprobando…"),
    ("Version {} is available", "La versión {} está disponible"),
    ("You're up to date ({})", "Tienes la última versión ({})"),
    (
        "Core Board {} is the latest version",
        "Core Board {} es la última versión",
    ),
    ("Couldn't check: {}", "No se pudo comprobar: {}"),
    (
        "Couldn't check for updates: {}",
        "No se pudieron buscar actualizaciones: {}",
    ),
    (
        "Update failed: {}. Opening the download page instead.",
        "La actualización falló: {}. Abriendo la página de descarga.",
    ),
    (
        "Core Board {} is available",
        "Core Board {} está disponible",
    ),
    ("(you have {})", "(tienes la {})"),
    ("Downloading the update…", "Descargando la actualización…"),
    ("Update now", "Actualizar ahora"),
    ("Download", "Descargar"),
    (
        "Downloads and installs it, then reopens Core Board. Your boards are saved first.",
        "La descarga y la instala, y vuelve a abrir Core Board. Tus tableros se guardan antes.",
    ),
    ("Opens the download page", "Abre la página de descarga"),
    ("What's new", "Novedades"),
    ("Later", "Más tarde"),
    ("Check for updates", "Buscar actualizaciones"),
    ("Download page", "Página de descarga"),
    (
        "mikibutler.ca/coreboard: every version, for Windows, macOS and Linux",
        "mikibutler.ca/coreboard: todas las versiones, para Windows, macOS y Linux",
    ),
    ("Show the update", "Ver la actualización"),
    (
        "Check for updates when Core Board starts",
        "Buscar actualizaciones al abrir Core Board",
    ),
    (
        "One small request to mikibutler.ca. Nothing about you or your boards is sent.",
        "Una pequeña consulta a mikibutler.ca. No se envía nada sobre ti ni tus tableros.",
    ),
    (
        "This board can't be opened in this version",
        "Este tablero no se puede abrir en esta versión",
    ),
    (
        "It was probably saved by a newer Core Board. Your board is safe: this version won't change or save over it. Update Core Board to open it.",
        "Seguramente se guardó con un Core Board más nuevo. Tu tablero está a salvo: esta versión no lo cambiará ni lo sobrescribirá. Actualiza Core Board para abrirlo.",
    ),
    ("Details: {}", "Detalles: {}"),
    ("Other boards…", "Otros tableros…"),
    // Watch a folder
    ("Can't watch {}: {}", "No se puede vigilar {}: {}"),
    (
        "Watching {} · new images will appear here",
        "Vigilando {} · las imágenes nuevas aparecerán aquí",
    ),
    ("{} images", "{} imágenes"),
    ("Added {} from {}", "Añadido {} de {}"),
    ("Watching: {}", "Vigilando: {}"),
    ("Watch a folder", "Vigilar una carpeta"),
    (
        "New images saved there (screenshots, renders) appear on this board.",
        "Las imágenes nuevas que se guarden allí (capturas, renders) aparecen en este tablero.",
    ),
    ("Choose folder…", "Elegir carpeta…"),
    ("Folder to watch", "Carpeta a vigilar"),
    ("or paste a folder path", "o pega la ruta de una carpeta"),
    ("Watch", "Vigilar"),
    ("Watching {}", "Vigilando {}"),
    ("Can't read it: {}", "No se puede leer: {}"),
    ("Add the {} already there", "Añadir las {} que ya hay"),
    ("Stop watching", "Dejar de vigilar"),
    // Study views and guides
    ("Normal view", "Vista normal"),
    ("Values: 3 tones", "Valores: 3 tonos"),
    ("Values: 5 tones", "Valores: 5 tonos"),
    ("Saturation map", "Mapa de saturación"),
    ("Noise / detail map", "Mapa de ruido / detalle"),
    ("Squint (big shapes)", "Ojos entrecerrados (formas grandes)"),
    ("No guide", "Sin guía"),
    ("Rule of thirds", "Regla de los tercios"),
    ("Phi grid", "Cuadrícula phi"),
    ("Golden spiral", "Espiral áurea"),
    ("Diagonals + reciprocals", "Diagonales + recíprocas"),
    ("Center cross", "Cruz central"),
    // Watermark positions
    ("Top left", "Arriba a la izquierda"),
    ("Top right", "Arriba a la derecha"),
    ("Bottom left", "Abajo a la izquierda"),
    ("Bottom right", "Abajo a la derecha"),
    ("Tiled", "En mosaico"),
    // Shortcuts overlay (H)
    ("Add & move around", "Añadir y moverse"),
    ("Drag & drop / Ctrl+V", "Arrastrar y soltar / Ctrl+V"),
    (
        "add images, or any file as a card",
        "añadir imágenes, o cualquier archivo como tarjeta",
    ),
    ("Ctrl+V a web link", "Ctrl+V con un enlace"),
    (
        "download its picture (or add a link)",
        "descargar su imagen (o añadir el enlace)",
    ),
    ("Ctrl+click empty spot", "Ctrl+clic en un sitio vacío"),
    ("add a link card", "añadir una tarjeta de enlace"),
    ("Double-click a card", "Doble clic en una tarjeta"),
    ("open the file or link", "abrir el archivo o enlace"),
    ("Scroll / pinch", "Rueda / pellizco"),
    ("zoom", "zoom"),
    (
        "Right/middle/Space drag",
        "Arrastrar con der./central/Espacio",
    ),
    ("pan", "mover la vista"),
    ("fit to view / zoom 100%", "ver todo / zoom 100%"),
    ("Drag empty space", "Arrastrar en vacío"),
    ("box select", "seleccionar con un recuadro"),
    ("Shift+click", "Shift+clic"),
    (
        "add to / remove from selection",
        "añadir a / quitar de la selección",
    ),
    ("Corner handle", "Esquina"),
    ("resize", "cambiar tamaño"),
    ("Del", "Supr"),
    ("remove", "borrar"),
    ("undo / redo", "deshacer / rehacer"),
    ("duplicate", "duplicar"),
    ("Arrange", "Organizar"),
    (
        "grid arrange / same height",
        "ordenar en cuadrícula / misma altura",
    ),
    ("Alt+arrows, Alt+C / M", "Alt+flechas, Alt+C / M"),
    ("align", "alinear"),
    ("space evenly", "espaciar por igual"),
    ("Alt while dragging", "Alt al arrastrar"),
    ("move without snapping", "mover sin imanes"),
    ("lock / pin to screen", "bloquear / fijar a la pantalla"),
    ("frame around selection", "marco alrededor de la selección"),
    ("Images", "Imágenes"),
    ("rotate 90° cw / ccw", "girar 90° horario / antihorario"),
    ("mirror", "espejo"),
    (
        "crop (drag) / reset crop",
        "recortar (arrastrar) / deshacer recorte",
    ),
    ("Double-click image", "Doble clic en imagen"),
    ("caption", "pie de foto"),
    (
        "focus view, ← → to step through",
        "vista de enfoque, ← → para pasar",
    ),
    (
        "surprise me: a random image",
        "sorpréndeme: una imagen al azar",
    ),
    ("Study", "Estudio"),
    (
        "value, saturation, noise, squint",
        "valor, saturación, ruido, entrecerrar",
    ),
    ("black & white", "blanco y negro"),
    ("whole board in values", "todo el tablero en valores"),
    (
        "composition guide / turn spiral",
        "guía de composición / girar espiral",
    ),
    ("color picker (HSV + value)", "cuentagotas (HSV + valor)"),
    ("6 / 10 color palette", "paleta de 6 / 10 colores"),
    ("Hold Q", "Mantener Q"),
    ("Hold Z", "Mantener Z"),
    (
        "magnifier (scroll to change its zoom)",
        "lupa (la rueda cambia su aumento)",
    ),
    (
        "radial menu: find similar, web…",
        "menú radial: parecidas, web…",
    ),
    ("Notes & drawing", "Notas y dibujo"),
    ("new note / note color", "nota nueva / color de nota"),
    ("empty image slot", "hueco para imagen"),
    ("Double-click", "Doble clic"),
    (
        "edit a note, frame or slot",
        "editar una nota, marco o hueco",
    ),
    (
        "draw (anywhere) / hide drawings",
        "dibujar (donde sea) / ocultar dibujos",
    ),
    ("Boards & window", "Tableros y ventana"),
    (
        "boards, updates, settings",
        "tableros, actualizaciones, ajustes",
    ),
    ("tags / search", "etiquetas / buscar"),
    ("export PNG (+ watermark)", "exportar PNG (+ marca de agua)"),
    ("window opacity", "opacidad de la ventana"),
    ("always on top", "siempre encima"),
    ("show / hide this help", "mostrar / ocultar esta ayuda"),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Every text the app shows (wrapped in `tr`/`trf` anywhere in the source, plus the
    /// shortcut list and align buttons, which are translated where they're drawn) has a Spanish
    /// line, with the same `{}` placeholders.
    #[test]
    fn every_text_has_spanish() {
        let sources = [
            include_str!("app.rs"),
            include_str!("app/cards.rs"),
            include_str!("app/draw.rs"),
            include_str!("app/focus.rs"),
            include_str!("app/organize.rs"),
            include_str!("app/slots.rs"),
            include_str!("app/updates.rs"),
            include_str!("app/watch.rs"),
            include_str!("app/minimap.rs"),
            include_str!("app/motion.rs"),
            include_str!("board.rs"),
            include_str!("export.rs"),
        ];
        let mut missing = Vec::new();
        for source in sources {
            for key in keys(source) {
                // Pure key names ("Ctrl+D", "F / 1") read the same in Spanish.
                let words = key.split(|c: char| !c.is_alphabetic()).any(|w| {
                    w.chars().count() >= 3
                        && !["Ctrl", "Shift", "Alt", "Esc", "Enter", "PNG", "HSV"].contains(&w)
                });
                if words && !spanish().contains_key(key.as_str()) {
                    missing.push(key);
                }
            }
        }
        assert!(missing.is_empty(), "no Spanish for: {missing:#?}");
        for (en, es) in SPANISH {
            assert_eq!(en.matches("{}").count(), es.matches("{}").count(), "{en}");
        }
        assert_eq!(
            spanish().len(),
            SPANISH.len(),
            "an English line is listed twice"
        );
    }

    /// The literal texts passed to `tr`/`trf`, plus those in the shortcut table and align
    /// buttons, as Rust would read them (line continuations joined, \n and \u{..} decoded).
    fn keys(source: &str) -> Vec<String> {
        let source = source.split("#[cfg(test)]").next().unwrap_or(source);
        let mut keys = Vec::new();
        let mut rest = source;
        let mut in_help = false;
        while let Some(quote) = rest.find('"') {
            let before = &rest[..quote];
            if before.contains("const HELP") {
                in_help = true;
            }
            if before.contains("fn paint_help") {
                in_help = false;
            }
            let call = before.trim_end();
            let wrapped = call.ends_with("tr(") || call.ends_with("trf(");
            let align = call.ends_with("(Align::Left,")
                || call.contains("(Align::") && call.ends_with(',') && !call.ends_with("\",");
            let Some(end) = string_end(&rest[quote + 1..]) else {
                break;
            };
            let literal = &rest[quote + 1..quote + 1 + end];
            if wrapped
                || (in_help && !literal.is_empty())
                || (align && literal.chars().all(char::is_alphabetic))
            {
                keys.push(decode(literal));
            }
            rest = &rest[quote + 1 + end + 1..];
        }
        keys
    }

    fn string_end(s: &str) -> Option<usize> {
        let mut escaped = false;
        for (i, c) in s.char_indices() {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => return Some(i),
                _ => {}
            }
        }
        None
    }

    fn decode(literal: &str) -> String {
        let mut out = String::new();
        let mut chars = literal.chars().peekable();
        while let Some(c) = chars.next() {
            if c != '\\' {
                out.push(c);
                continue;
            }
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('\n') => {
                    while chars.peek().is_some_and(|c| c.is_whitespace()) {
                        chars.next();
                    }
                }
                Some('u') => {
                    let code: String = chars.by_ref().skip(1).take_while(|&c| c != '}').collect();
                    if let Some(c) = u32::from_str_radix(&code, 16).ok().and_then(char::from_u32) {
                        out.push(c);
                    }
                }
                Some(other) => out.push(other),
                None => {}
            }
        }
        out
    }

    #[test]
    fn fills_values_in_order() {
        assert_eq!(fill("{} of {}", &[&3, &"7"]), "3 of 7");
        assert_eq!(fill("no values", &[&1]), "no values");
    }
}
