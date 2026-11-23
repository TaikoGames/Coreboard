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
    Catalan,
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// The language setting as stored: "auto" follows the computer, "en" or "es" picks one.
pub fn apply(setting: &str) {
    let lang = match setting {
        "en" => Lang::English,
        "es" => Lang::Spanish,
        "ca" => Lang::Catalan,
        _ => system(),
    };
    CURRENT.store(lang as u8, Ordering::Relaxed);
}

/// The computer's language, if it's one Core Board speaks.
pub fn system() -> Lang {
    let locale = sys_locale::get_locale().unwrap_or_default().to_lowercase();
    if locale.starts_with("ca") {
        Lang::Catalan
    } else if locale.starts_with("es") {
        Lang::Spanish
    } else {
        Lang::English
    }
}

pub fn current() -> Lang {
    match CURRENT.load(Ordering::Relaxed) {
        1 => Lang::Spanish,
        2 => Lang::Catalan,
        _ => Lang::English,
    }
}

/// `english` in the current language.
pub fn tr(english: &'static str) -> &'static str {
    match current() {
        Lang::English => english,
        Lang::Spanish => spanish().get(english).copied().unwrap_or(english),
        Lang::Catalan => catalan().get(english).copied().unwrap_or(english),
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

fn catalan() -> &'static HashMap<&'static str, &'static str> {
    static TABLE: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    TABLE.get_or_init(|| CATALAN.iter().copied().collect())
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
    ("Zoom out", "Alejar"),
    ("Zoom in", "Acercar"),
    ("Back to 100%", "Volver al 100%"),
    ("Fit everything (F)", "Ver todo (F)"),
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
        "Color picker on: hover an image, click to copy its color",
        "Cuentagotas activado: pasa sobre una imagen y haz clic para copiar el color",
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
    ("Opening the PDF…", "Abriendo el PDF…"),
    ("it's password-protected", "tiene contraseña"),
    ("it couldn't be read", "no se pudo leer"),
    ("it has no pages", "no tiene páginas"),
    (
        "Couldn't show the PDF's pages: {}",
        "No se pudieron mostrar las páginas del PDF: {}",
    ),
    (
        "Added the first {} of {} pages from {}",
        "Añadidas las primeras {} de {} páginas de {}",
    ),
    ("Added {} pages from {}", "Añadidas {} páginas de {}"),
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
    (
        "copy as a picture, to paste in any app",
        "copiar como imagen, para pegar en cualquier app",
    ),
    ("Select something to copy", "Selecciona algo para copiar"),
    ("Couldn't copy: {}", "No se pudo copiar: {}"),
    ("no clipboard", "no hay portapapeles"),
    (
        "Copied: paste it in any app",
        "Copiado: pégalo en cualquier app",
    ),
    (
        "Copied {} items as one picture",
        "{} elementos copiados como una imagen",
    ),
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
    ("group / ungroup", "agrupar / desagrupar"),
    ("Ungrouped", "Desagrupado"),
    ("Grouped {} items", "{} elementos agrupados"),
    (
        "Select 2 or more items to group",
        "Selecciona 2 o más elementos para agrupar",
    ),
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
    (
        "color picker (HSV + value); click copies it",
        "cuentagotas (HSV + valor); clic para copiar",
    ),
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
    // Right-click menu
    ("Focus view", "Vista de enfoque"),
    ("Compare side by side", "Comparar lado a lado"),
    ("Save board as a file", "Guardar el tablero como archivo"),
    ("Open a board file", "Abrir un archivo de tablero"),
    (
        "Save this board as a file…",
        "Guardar este tablero como archivo…",
    ),
    ("Open a board file…", "Abrir un archivo de tablero…"),
    (
        "One file with the whole board and its pictures, to send to someone or keep as a backup",
        "Un solo archivo con todo el tablero y sus imágenes, para enviarlo a alguien o guardarlo como copia",
    ),
    (
        "Saved \u{201c}{}\u{201d} with {} pictures",
        "Guardado «{}» con {} imágenes",
    ),
    (
        "Couldn't save the board file: {}",
        "No se pudo guardar el archivo de tablero: {}",
    ),
    (
        "Couldn't open that board file: {}",
        "No se pudo abrir ese archivo de tablero: {}",
    ),
    (
        "save the board as one file",
        "guardar el tablero en un solo archivo",
    ),
    (
        "this isn't a Core Board file",
        "no es un archivo de Core Board",
    ),
    (
        "it was made by a newer Core Board, or is damaged ({})",
        "se hizo con un Core Board más nuevo o está dañado ({})",
    ),
    ("it's too big", "es demasiado grande"),
    ("2 images + Enter", "2 imágenes + Enter"),
    ("compare them side by side", "compararlas lado a lado"),
    ("comparing 2 images", "comparando 2 imágenes"),
    ("Copied {}", "Copiado {}"),
    (
        "← → change the second image   scroll zoom both   drag pan both   Esc back",
        "← → cambiar la segunda imagen   rueda zoom a ambas   arrastrar mueve ambas   Esc volver",
    ),
    ("Copy", "Copiar"),
    ("Duplicate", "Duplicar"),
    ("Rotate", "Girar"),
    ("Mirror", "Espejo"),
    ("Study view", "Vista de estudio"),
    ("Extract palette", "Sacar la paleta"),
    ("Find similar", "Buscar parecidas"),
    ("Caption", "Pie de foto"),
    ("Open", "Abrir"),
    ("Edit text", "Editar el texto"),
    ("Unlock", "Desbloquear"),
    ("Lock", "Bloquear"),
    ("Unpin from the screen", "Desfijar de la pantalla"),
    ("Pin to the screen", "Fijar a la pantalla"),
    ("Ungroup", "Desagrupar"),
    ("Group", "Agrupar"),
    ("Delete", "Borrar"),
    ("Add a note", "Añadir una nota"),
    ("Add an image slot", "Añadir un hueco de imagen"),
    ("Fit everything", "Ver todo"),
    ("Zoom 100%", "Zoom 100%"),
    ("Surprise me", "Sorpréndeme"),
    ("Boards…", "Tableros…"),
    ("Export PNG…", "Exportar PNG…"),
    ("Arrows / Shift+arrows", "Flechas / Shift+flechas"),
    ("nudge 1 / 10 points", "mover 1 / 10 puntos"),
];

/// English -> Catalan, the same texts as [`SPANISH`].
const CATALAN: &[(&str, &str)] = &[
    ("Similar\nvalues", "Valors\nsimilars"),
    ("Similar\npalette", "Paleta\nsimilar"),
    ("Search\nweb", "Cercar\na la web"),
    ("Extract\npalette", "Treure\npaleta"),
    ("Study\nview", "Vista\nd'estudi"),
    ("Guide", "Guia"),
    ("Crop", "Retallar"),
    ("Note", "Nota"),
    ("Focus\nview", "Vista\nd'enfocament"),
    ("Link", "Enllaç"),
    ("Image\nslot", "Espai\nd'imatge"),
    ("Paste", "Enganxar"),
    ("Fit\nview", "Veure-ho\ntot"),
    ("Surprise\nme", "Sorprèn-me"),
    ("Help", "Ajuda"),
    (
        "{} locked item(s) kept · unlock with L",
        "{} element(s) bloquejat(s) es queden · desbloqueja amb L",
    ),
    ("Select images first", "Selecciona imatges primer"),
    ("Golden spiral turned", "Espiral àuria girada"),
    ("Value", "Valor"),
    ("Duplicated", "Duplicat"),
    ("Duplicated {} items", "{} elements duplicats"),
    (
        "Values: whole board in grayscale",
        "Valors: tot el tauler en grisos",
    ),
    (
        "Values: whole board in 3 tones",
        "Valors: tot el tauler en 3 tons",
    ),
    ("Back to color", "Tornem al color"),
    (
        "Select images to extract colors from",
        "Selecciona imatges per treure'n els colors",
    ),
    ("Extracting colors…", "Traient colors…"),
    (
        "Color extraction failed: {}",
        "No s'han pogut treure els colors: {}",
    ),
    ("Crop done", "Retall fet"),
    (
        "Select one image to crop",
        "Selecciona una imatge per retallar",
    ),
    (
        "Drag over the image to crop · Enter/Esc when done · Shift+X resets",
        "Arrossega sobre la imatge per retallar · Enter/Esc en acabar · Shift+X ho desfà",
    ),
    ("Crop reset", "Retall desfet"),
    ("Untitled", "Sense títol"),
    (
        "Name this slot, e.g. Front view",
        "Posa nom a aquest espai, p. ex. Vista frontal",
    ),
    (
        "Add a caption, e.g. Sargent, 1885, rim light",
        "Afegeix un peu de foto, p. ex. Sargent, 1885, llum de contorn",
    ),
    (
        "That file isn't an image I can read",
        "Aquest fitxer no és una imatge que pugui llegir",
    ),
    (
        "That isn't a picture I can read",
        "Això no és una imatge que pugui llegir",
    ),
    (
        "Clipboard has no image",
        "No hi ha cap imatge al porta-retalls",
    ),
    ("Downloading picture…", "Baixant la imatge…"),
    (
        "Couldn't get that picture: {}",
        "No s'ha pogut aconseguir la imatge: {}",
    ),
    ("all {} images", "les {} imatges"),
    ("{} selected image(s)", "{} imatge(s) seleccionada(es)"),
    ("Export PNG", "Exportar PNG"),
    ("Exports {} as one PNG.", "Exporta {} en un sol PNG."),
    (
        "Watermark text (leave empty for none):",
        "Text de la marca d'aigua (buit per a cap):",
    ),
    ("e.g. © Your Name", "p. ex. © El teu nom"),
    ("Position", "Posició"),
    ("Opacity", "Opacitat"),
    ("Exporting…", "Exportant…"),
    ("Export", "Exportar"),
    ("Cancel", "Cancel·lar"),
    ("Saved to {}", "Es desa a {}"),
    ("Change…", "Canviar…"),
    ("Folder for exports", "Carpeta per exportar"),
    ("Reset", "Restablir"),
    ("Exported {}", "Exportat: {}"),
    ("✓ Exported {}", "✓ Exportat: {}"),
    ("Open folder", "Obrir la carpeta"),
    ("Export failed: {}", "No s'ha pogut exportar: {}"),
    ("Image to find", "Imatge per trobar"),
    (
        "Select an image to find more like it",
        "Selecciona una imatge per trobar-ne d'altres de semblants",
    ),
    ("Find more", "Trobar-ne més"),
    ("Similar values", "Valors similars"),
    ("Similar palette", "Paleta similar"),
    ("Search the web", "Cercar a la web"),
    ("value structure", "estructura de valors"),
    ("colors", "colors"),
    (
        "Images on your other boards ranked by {}. Click one to copy it here.",
        "Imatges dels teus altres taulers ordenades per {}. Fes clic en una per copiar-la aquí.",
    ),
    ("Scanning your boards…", "Revisant els teus taulers…"),
    (
        "No images on your other boards yet. Make more boards with Ctrl+O and drop references in; they'll show up here.",
        "Encara no hi ha imatges als teus altres taulers. Crea'n més amb Ctrl+O i deixa-hi anar referències; apareixeran aquí.",
    ),
    (
        "Couldn't read this image: {}",
        "No s'ha pogut llegir aquesta imatge: {}",
    ),
    (
        "From {}\nClick to copy onto this board",
        "De {}\nFes clic per copiar-la en aquest tauler",
    ),
    (
        "Opens an image search in your browser.",
        "Obre una cerca d'imatges al navegador.",
    ),
    (
        "Subject, e.g. oak tree at dusk",
        "Tema, p. ex. roure al capvespre",
    ),
    ("Any color", "Qualsevol color"),
    ("Black & white", "Blanc i negre"),
    ("{} (from this image)", "{} (d'aquesta imatge)"),
    ("Color", "Color"),
    ("Type a subject to search.", "Escriu un tema per cercar."),
    ("Can't read {}", "No es pot llegir {}"),
    ("Boards", "Taulers"),
    ("New board", "Tauler nou"),
    (
        "Name, e.g. Character refs",
        "Nom, p. ex. Refs de personatge",
    ),
    ("Create", "Crear"),
    (
        "Open or create in a folder…",
        "Obrir o crear en una carpeta…",
    ),
    ("Board folder", "Carpeta del tauler"),
    ("Recent", "Recents"),
    ("No other boards yet", "Encara no hi ha altres taulers"),
    ("Animations", "Animacions"),
    (
        "Glides, pops, swing and flicks. Turn off for a still board.",
        "Lliscaments, rebots, balancejos i llançaments. Desactiva-les per tenir un tauler quiet.",
    ),
    ("Mini-map", "Minimapa"),
    (
        "An overview of the whole board in the corner, while part of it is off screen. Click or drag in it to go there.",
        "Una vista de tot el tauler a la cantonada quan una part queda fora de la pantalla. Fes clic o arrossega-hi per anar-hi.",
    ),
    ("Close", "Tancar"),
    ("Zoom out", "Allunyar"),
    ("Zoom in", "Apropar"),
    ("Back to 100%", "Tornar al 100%"),
    ("Fit everything (F)", "Veure-ho tot (F)"),
    ("Close the help (H)", "Tancar l'ajuda (H)"),
    ("Shortcuts and help (H)", "Dreceres i ajuda (H)"),
    ("Language", "Idioma"),
    ("Automatic ({})", "Automàtic ({})"),
    ("Can't open {}: {}", "No es pot obrir {}: {}"),
    ("Opened {}", "Obert: {}"),
    ("Save failed: {}", "No s'ha pogut desar: {}"),
    ("Saved", "Desat"),
    ("Drawings shown", "Dibuixos visibles"),
    ("Drawings hidden", "Dibuixos amagats"),
    (
        "Color picker on: hover an image, click to copy its color",
        "Comptagotes activat: passa per sobre d'una imatge i fes clic per copiar-ne el color",
    ),
    ("Color picker off", "Comptagotes desactivat"),
    ("Always on top: on", "Sempre a sobre: activat"),
    ("Always on top: off", "Sempre a sobre: desactivat"),
    ("Opacity {}%", "Opacitat {}%"),
    ("Drop to fill “{}”", "Deixa anar per omplir «{}»"),
    (
        "Drop to fill this slot",
        "Deixa anar per omplir aquest espai",
    ),
    (
        "Drop images or files here, paste with Ctrl+V, or press N for a note",
        "Deixa anar imatges o fitxers aquí, enganxa amb Ctrl+V o prem N per a una nota",
    ),
    ("{} items", "{} elements"),
    ("opacity {}%", "opacitat {}%"),
    ("on top", "sempre a sobre"),
    (
        "DRAW: drag anywhere, Esc when done",
        "DIBUIX: arrossega on vulguis, Esc en acabar",
    ),
    (
        "VALUES: whole board in grayscale (Shift+B)",
        "VALORS: tot el tauler en grisos (Shift+B)",
    ),
    (
        "VALUES: whole board in 3 tones (Shift+B)",
        "VALORS: tot el tauler en 3 tons (Shift+B)",
    ),
    (
        "CROP: drag over the image, Enter when done",
        "RETALL: arrossega sobre la imatge, Enter en acabar",
    ),
    ("{} selected", "{} seleccionat(s)"),
    ("Empty note", "Nota buida"),
    ("shortcuts", "dreceres"),
    (
        "Can't find {}: it was moved or deleted",
        "No es troba {}: s'ha mogut o esborrat",
    ),
    ("Couldn't open it: {}", "No s'ha pogut obrir: {}"),
    ("Opening…", "Obrint…"),
    ("Opening the PDF…", "Obrint el PDF…"),
    ("it's password-protected", "té contrasenya"),
    ("it couldn't be read", "no s'ha pogut llegir"),
    ("it has no pages", "no té pàgines"),
    (
        "Couldn't show the PDF's pages: {}",
        "No s'han pogut mostrar les pàgines del PDF: {}",
    ),
    (
        "Added the first {} of {} pages from {}",
        "Afegides les primeres {} de {} pàgines de {}",
    ),
    ("Added {} pages from {}", "Afegides {} pàgines de {}"),
    ("Opening the video…", "Obrint el vídeo…"),
    ("Add a link", "Afegir un enllaç"),
    (
        "Paste or type a web address",
        "Enganxa o escriu una adreça web",
    ),
    ("Add", "Afegir"),
    (
        "Double-click the card to open the page.",
        "Fes doble clic a la targeta per obrir la pàgina.",
    ),
    (
        "That doesn't look like a web address",
        "Això no sembla una adreça web",
    ),
    ("Folder", "Carpeta"),
    ("Shortcut", "Drecera"),
    ("App", "Aplicació"),
    (
        "Draw: drag anywhere · Esc or D when done",
        "Dibuixar: arrossega on vulguis · Esc o D en acabar",
    ),
    ("Drawing done", "Dibuix acabat"),
    ("Drawings cleared", "Dibuixos esborrats"),
    ("Clear", "Esborrar"),
    (
        "Remove the drawings on the selected images",
        "Treu els dibuixos de les imatges seleccionades",
    ),
    ("Hide", "Amagar"),
    ("Show", "Mostrar"),
    (
        "Show or hide all drawings (Shift+D)",
        "Mostra o amaga tots els dibuixos (Shift+D)",
    ),
    ("Done", "Fet"),
    ("Esc or D", "Esc o D"),
    ("Pen", "Llapis"),
    ("Arrow", "Fletxa"),
    ("Circle", "Cercle"),
    ("Box", "Caixa"),
    ("No images to focus on", "No hi ha imatges per veure"),
    (
        "Add some images first, then press S for a surprise",
        "Afegeix algunes imatges primer i després prem S per a una sorpresa",
    ),
    ("Surprise! From \u{201c}{}\u{201d}", "Sorpresa! De «{}»"),
    ("image {} of {}", "imatge {} de {}"),
    (
        "← → step   S surprise   scroll zoom   drag pan   V C B M R study   Esc back",
        "← → passar   S sorpresa   roda zoom   arrossegar moure   V C B M R estudi   Esc tornar",
    ),
    ("Unlocked", "Desbloquejat"),
    ("Locked", "Bloquejat"),
    ("Unpinned", "Desfixat"),
    ("Pinned to the screen", "Fixat a la pantalla"),
    (
        "Select something to tag",
        "Selecciona alguna cosa per etiquetar",
    ),
    ("Tags · {} selected", "Etiquetes · {} seleccionat(s)"),
    ("No tags yet", "Encara no hi ha etiquetes"),
    ("Remove from the selection", "Treure de la selecció"),
    (
        "Add a tag and press Enter",
        "Afegeix una etiqueta i prem Enter",
    ),
    ("On this board", "En aquest tauler"),
    (
        "Search tags, notes and frame titles",
        "Cerca etiquetes, notes i títols de marcs",
    ),
    ("{} found", "{} trobat(s)"),
    ("Select", "Seleccionar"),
    (
        "Select the matches and zoom to them (Enter)",
        "Selecciona els resultats i apropa-t'hi (Enter)",
    ),
    ("Close search (Esc)", "Tancar la cerca (Esc)"),
    (
        "Select 3 or more items to space evenly",
        "Selecciona 3 o més elements per espaiar-los igual",
    ),
    ("Align", "Alinear"),
    ("Space", "Espaiar"),
    ("Across", "En amplada"),
    ("Down", "En alçada"),
    ("Left", "Esquerra"),
    ("Center", "Centre"),
    ("Right", "Dreta"),
    ("Top", "Dalt"),
    ("Middle", "Mig"),
    ("Bottom", "Baix"),
    ("Filled “{}”", "Omplert «{}»"),
    ("Slot filled", "Espai omplert"),
    ("Drop an image here", "Deixa anar una imatge aquí"),
    ("Checking…", "Comprovant…"),
    ("Version {} is available", "La versió {} està disponible"),
    ("You're up to date ({})", "Tens l'última versió ({})"),
    (
        "Core Board {} is the latest version",
        "Core Board {} és l'última versió",
    ),
    ("Couldn't check: {}", "No s'ha pogut comprovar: {}"),
    (
        "Couldn't check for updates: {}",
        "No s'han pogut buscar actualitzacions: {}",
    ),
    (
        "Update failed: {}. Opening the download page instead.",
        "L'actualització ha fallat: {}. S'obre la pàgina de baixada.",
    ),
    (
        "Core Board {} is available",
        "Core Board {} està disponible",
    ),
    ("(you have {})", "(tens la {})"),
    ("Downloading the update…", "Baixant l'actualització…"),
    ("Update now", "Actualitzar ara"),
    ("Download", "Baixar"),
    (
        "Downloads and installs it, then reopens Core Board. Your boards are saved first.",
        "La baixa i la instal·la, i torna a obrir Core Board. Els teus taulers es desen abans.",
    ),
    ("Opens the download page", "Obre la pàgina de baixada"),
    ("What's new", "Novetats"),
    ("Later", "Més tard"),
    ("Check for updates", "Buscar actualitzacions"),
    ("Download page", "Pàgina de baixada"),
    (
        "mikibutler.ca/coreboard: every version, for Windows, macOS and Linux",
        "mikibutler.ca/coreboard: totes les versions, per a Windows, macOS i Linux",
    ),
    ("Show the update", "Veure l'actualització"),
    (
        "Check for updates when Core Board starts",
        "Buscar actualitzacions en obrir Core Board",
    ),
    (
        "One small request to mikibutler.ca. Nothing about you or your boards is sent.",
        "Una petita consulta a mikibutler.ca. No s'envia res sobre tu ni els teus taulers.",
    ),
    (
        "This board can't be opened in this version",
        "Aquest tauler no es pot obrir en aquesta versió",
    ),
    (
        "It was probably saved by a newer Core Board. Your board is safe: this version won't change or save over it. Update Core Board to open it.",
        "Segurament es va desar amb un Core Board més nou. El teu tauler està segur: aquesta versió no el canviarà ni el sobreescriurà. Actualitza Core Board per obrir-lo.",
    ),
    ("Details: {}", "Detalls: {}"),
    ("Other boards…", "Altres taulers…"),
    ("Can't watch {}: {}", "No es pot vigilar {}: {}"),
    (
        "Watching {} · new images will appear here",
        "Vigilant {} · les imatges noves apareixeran aquí",
    ),
    ("{} images", "{} imatges"),
    ("Added {} from {}", "Afegit {} de {}"),
    ("Watching: {}", "Vigilant: {}"),
    ("Watch a folder", "Vigilar una carpeta"),
    (
        "New images saved there (screenshots, renders) appear on this board.",
        "Les imatges noves que s'hi desin (captures, renders) apareixen en aquest tauler.",
    ),
    ("Choose folder…", "Triar carpeta…"),
    ("Folder to watch", "Carpeta per vigilar"),
    ("or paste a folder path", "o enganxa el camí d'una carpeta"),
    ("Watch", "Vigilar"),
    ("Watching {}", "Vigilant {}"),
    ("Can't read it: {}", "No es pot llegir: {}"),
    ("Add the {} already there", "Afegir les {} que ja hi ha"),
    ("Stop watching", "Deixar de vigilar"),
    ("Normal view", "Vista normal"),
    ("Values: 3 tones", "Valors: 3 tons"),
    ("Values: 5 tones", "Valors: 5 tons"),
    ("Saturation map", "Mapa de saturació"),
    ("Noise / detail map", "Mapa de soroll / detall"),
    ("Squint (big shapes)", "Ulls aclucats (formes grans)"),
    ("No guide", "Sense guia"),
    ("Rule of thirds", "Regla dels terços"),
    ("Phi grid", "Quadrícula phi"),
    ("Golden spiral", "Espiral àuria"),
    ("Diagonals + reciprocals", "Diagonals + recíproques"),
    ("Center cross", "Creu central"),
    ("Top left", "A dalt a l'esquerra"),
    ("Top right", "A dalt a la dreta"),
    ("Bottom left", "A baix a l'esquerra"),
    ("Bottom right", "A baix a la dreta"),
    ("Tiled", "En mosaic"),
    ("Add & move around", "Afegir i moure's"),
    ("Drag & drop / Ctrl+V", "Arrossegar i deixar anar / Ctrl+V"),
    (
        "add images, or any file as a card",
        "afegir imatges, o qualsevol fitxer com a targeta",
    ),
    ("Ctrl+V a web link", "Ctrl+V amb un enllaç"),
    (
        "download its picture (or add a link)",
        "baixar-ne la imatge (o afegir l'enllaç)",
    ),
    ("Ctrl+click empty spot", "Ctrl+clic en un lloc buit"),
    ("add a link card", "afegir una targeta d'enllaç"),
    ("Double-click a card", "Doble clic en una targeta"),
    ("open the file or link", "obrir el fitxer o l'enllaç"),
    ("Scroll / pinch", "Roda / pessic"),
    ("zoom", "zoom"),
    (
        "Right/middle/Space drag",
        "Arrossegar amb dret/central/Espai",
    ),
    ("pan", "moure la vista"),
    ("fit to view / zoom 100%", "veure-ho tot / zoom 100%"),
    ("Drag empty space", "Arrossegar en buit"),
    ("box select", "seleccionar amb un requadre"),
    ("Shift+click", "Shift+clic"),
    (
        "add to / remove from selection",
        "afegir a / treure de la selecció",
    ),
    ("Corner handle", "Cantonada"),
    ("resize", "canviar la mida"),
    ("Del", "Supr"),
    ("remove", "esborrar"),
    ("undo / redo", "desfer / refer"),
    ("duplicate", "duplicar"),
    (
        "copy as a picture, to paste in any app",
        "copiar com a imatge, per enganxar a qualsevol app",
    ),
    (
        "Select something to copy",
        "Selecciona alguna cosa per copiar",
    ),
    ("Couldn't copy: {}", "No s'ha pogut copiar: {}"),
    ("no clipboard", "no hi ha porta-retalls"),
    (
        "Copied: paste it in any app",
        "Copiat: enganxa-ho a qualsevol app",
    ),
    (
        "Copied {} items as one picture",
        "{} elements copiats com una imatge",
    ),
    ("Arrange", "Organitzar"),
    (
        "grid arrange / same height",
        "ordenar en quadrícula / mateixa alçada",
    ),
    ("Alt+arrows, Alt+C / M", "Alt+fletxes, Alt+C / M"),
    ("align", "alinear"),
    ("space evenly", "espaiar igual"),
    ("Alt while dragging", "Alt en arrossegar"),
    ("move without snapping", "moure sense imants"),
    ("lock / pin to screen", "bloquejar / fixar a la pantalla"),
    ("frame around selection", "marc al voltant de la selecció"),
    ("group / ungroup", "agrupar / desagrupar"),
    ("Ungrouped", "Desagrupat"),
    ("Grouped {} items", "{} elements agrupats"),
    (
        "Select 2 or more items to group",
        "Selecciona 2 o més elements per agrupar",
    ),
    ("Images", "Imatges"),
    ("rotate 90° cw / ccw", "girar 90° horari / antihorari"),
    ("mirror", "mirall"),
    (
        "crop (drag) / reset crop",
        "retallar (arrossegar) / desfer el retall",
    ),
    ("Double-click image", "Doble clic en una imatge"),
    ("caption", "peu de foto"),
    (
        "focus view, ← → to step through",
        "vista d'enfocament, ← → per passar",
    ),
    (
        "surprise me: a random image",
        "sorprèn-me: una imatge a l'atzar",
    ),
    ("Study", "Estudi"),
    (
        "value, saturation, noise, squint",
        "valor, saturació, soroll, aclucar",
    ),
    ("black & white", "blanc i negre"),
    ("whole board in values", "tot el tauler en valors"),
    (
        "composition guide / turn spiral",
        "guia de composició / girar l'espiral",
    ),
    (
        "color picker (HSV + value); click copies it",
        "comptagotes (HSV + valor); clic per copiar",
    ),
    ("6 / 10 color palette", "paleta de 6 / 10 colors"),
    ("Hold Q", "Mantenir Q"),
    ("Hold Z", "Mantenir Z"),
    (
        "magnifier (scroll to change its zoom)",
        "lupa (la roda en canvia l'augment)",
    ),
    (
        "radial menu: find similar, web…",
        "menú radial: semblants, web…",
    ),
    ("Notes & drawing", "Notes i dibuix"),
    ("new note / note color", "nota nova / color de nota"),
    ("empty image slot", "espai per a imatge"),
    ("Double-click", "Doble clic"),
    (
        "edit a note, frame or slot",
        "editar una nota, marc o espai",
    ),
    (
        "draw (anywhere) / hide drawings",
        "dibuixar (on sigui) / amagar dibuixos",
    ),
    ("Boards & window", "Taulers i finestra"),
    (
        "boards, updates, settings",
        "taulers, actualitzacions, ajustos",
    ),
    ("tags / search", "etiquetes / cercar"),
    ("export PNG (+ watermark)", "exportar PNG (+ marca d'aigua)"),
    ("window opacity", "opacitat de la finestra"),
    ("always on top", "sempre a sobre"),
    ("show / hide this help", "mostrar / amagar aquesta ajuda"),
    // Right-click menu
    ("Focus view", "Vista d'enfocament"),
    ("Compare side by side", "Comparar una al costat de l'altra"),
    ("Save board as a file", "Desar el tauler com a fitxer"),
    ("Open a board file", "Obrir un fitxer de tauler"),
    (
        "Save this board as a file…",
        "Desar aquest tauler com a fitxer…",
    ),
    ("Open a board file…", "Obrir un fitxer de tauler…"),
    (
        "One file with the whole board and its pictures, to send to someone or keep as a backup",
        "Un sol fitxer amb tot el tauler i les seves imatges, per enviar-lo a algú o guardar-lo com a còpia",
    ),
    (
        "Saved \u{201c}{}\u{201d} with {} pictures",
        "Desat «{}» amb {} imatges",
    ),
    (
        "Couldn't save the board file: {}",
        "No s'ha pogut desar el fitxer de tauler: {}",
    ),
    (
        "Couldn't open that board file: {}",
        "No s'ha pogut obrir aquest fitxer de tauler: {}",
    ),
    (
        "save the board as one file",
        "desar el tauler en un sol fitxer",
    ),
    (
        "this isn't a Core Board file",
        "no és un fitxer de Core Board",
    ),
    (
        "it was made by a newer Core Board, or is damaged ({})",
        "s'ha fet amb un Core Board més nou o està malmès ({})",
    ),
    ("it's too big", "és massa gran"),
    ("2 images + Enter", "2 imatges + Enter"),
    (
        "compare them side by side",
        "comparar-les una al costat de l'altra",
    ),
    ("comparing 2 images", "comparant 2 imatges"),
    ("Copied {}", "Copiat {}"),
    (
        "← → change the second image   scroll zoom both   drag pan both   Esc back",
        "← → canviar la segona imatge   roda zoom a totes dues   arrossegar mou totes dues   Esc tornar",
    ),
    ("Copy", "Copiar"),
    ("Duplicate", "Duplicar"),
    ("Rotate", "Girar"),
    ("Mirror", "Mirall"),
    ("Study view", "Vista d'estudi"),
    ("Extract palette", "Treure la paleta"),
    ("Find similar", "Buscar-ne de semblants"),
    ("Caption", "Peu de foto"),
    ("Open", "Obrir"),
    ("Edit text", "Editar el text"),
    ("Unlock", "Desbloquejar"),
    ("Lock", "Bloquejar"),
    ("Unpin from the screen", "Desfixar de la pantalla"),
    ("Pin to the screen", "Fixar a la pantalla"),
    ("Ungroup", "Desagrupar"),
    ("Group", "Agrupar"),
    ("Delete", "Esborrar"),
    ("Add a note", "Afegir una nota"),
    ("Add an image slot", "Afegir un espai d'imatge"),
    ("Fit everything", "Veure-ho tot"),
    ("Zoom 100%", "Zoom 100%"),
    ("Surprise me", "Sorprèn-me"),
    ("Boards…", "Taulers…"),
    ("Export PNG…", "Exportar PNG…"),
    ("Arrows / Shift+arrows", "Fletxes / Shift+fletxes"),
    ("nudge 1 / 10 points", "moure 1 / 10 punts"),
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
            include_str!("pdf.rs"),
            include_str!("app/copy.rs"),
            include_str!("app/pdfs.rs"),
            include_str!("app/groups.rs"),
            include_str!("app/context.rs"),
            include_str!("app/loupe.rs"),
            include_str!("app/boardfiles.rs"),
            include_str!("boardfile.rs"),
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
                    missing.push(key.clone());
                }
                if words && !catalan().contains_key(key.as_str()) {
                    missing.push(format!("(Catalan) {key}"));
                }
            }
        }
        assert!(missing.is_empty(), "no Spanish for: {missing:#?}");
        for (en, translated) in SPANISH.iter().chain(CATALAN) {
            assert_eq!(
                en.matches("{}").count(),
                translated.matches("{}").count(),
                "{en}"
            );
        }
        assert_eq!(
            spanish().len(),
            SPANISH.len(),
            "an English line is listed twice"
        );
        assert_eq!(
            catalan().len(),
            CATALAN.len(),
            "an English line is listed twice"
        );
        // Both translations cover exactly the same texts.
        let mut es: Vec<&str> = SPANISH.iter().map(|(en, _)| *en).collect();
        let mut ca: Vec<&str> = CATALAN.iter().map(|(en, _)| *en).collect();
        es.sort_unstable();
        ca.sort_unstable();
        assert_eq!(es, ca);
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
