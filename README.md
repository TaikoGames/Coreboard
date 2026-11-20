# Core Board

A small, fast reference board for images, written in Rust with [egui](https://github.com/emilk/egui).
Drop images on an infinite canvas, arrange and resize them, and they're saved automatically.

## Install

Download the latest version from the **[Releases page](../../releases/latest)**. Every version stays
listed on **[all releases](../../releases)** with its own downloads and a list of what changed, so you
can always go back to an older one. The app shows its version in the window title and the help (**H**).


| You have | Download | Then |
| --- | --- | --- |
| **Windows** | `CoreBoard-Setup-….exe` | Double-click it, click **Install**. Core Board opens when it's done and is in the Start menu and on your desktop. |
| Windows, no install | `…-windows-portable.zip` | Unzip, double-click `coreboard.exe`. |
| **macOS** | `CoreBoard-….dmg` | Open it and drag **Core Board** onto **Applications**. |
| **Linux** | `coreboard_…_amd64.deb` | Double-click (or `sudo apt install ./coreboard_…_amd64.deb`). |

The first time you open it:

- **Windows** may say *"Windows protected your PC"*: click **More info → Run anyway**. (The app isn't
  code-signed yet; that needs a paid certificate.)
- **macOS** may say it *"can't be opened"*: right-click the app → **Open**, or go to System Settings →
  Privacy & Security → **Open Anyway**. (It isn't notarized by Apple yet.)

Then drop some images on the window. Press **H** any time for the shortcuts.

## Using boards

Press **Ctrl+O** for the **Boards** window: create a board by name, open or create one in any folder
(native folder picker), or jump back to a recent board. Switching saves the current board first.

Boards created by name, and the first default board, live in your platform's data dir
(`~/.local/share/coreboard/` on Linux, `~/Library/Application Support/coreboard/` on macOS,
`%APPDATA%\coreboard\data\` on Windows). The recent list is kept in the matching config dir.

## Controls

| Action | Input |
| --- | --- |
| Add images | Drag & drop files, or **Ctrl+V** (image data, copied files, or a web link) |
| Files & links | Any other file becomes a card; **Ctrl+click** an empty spot to add a link; **double-click** a card to open it |
| Zoom | Scroll wheel / trackpad pinch (anchored at the cursor) |
| Pan | Right or middle drag, or hold **Space** and drag |
| Select | Click; **Shift/Ctrl+click** to toggle; drag on empty space to box-select |
| Move | Drag selected images |
| Resize | Drag the corner handle (keeps aspect ratio) |
| Rotate | **R** / **Shift+R** (90° clockwise / counter-clockwise) |
| Flip horizontal | **M** (mirror) |
| Grayscale | **B** (black & white, toggles) |
| Window opacity | **[** / **]** (10% steps, down to 10%; the desktop shows through) |
| Always on top | **T** (toggles) |
| Tags | **Ctrl+T** tags the selection; tags show under hovered or selected items |
| Search | **Ctrl+F** fades everything but matches (tags, note text, frame titles); **Enter** selects them |
| Lock / pin | **L** locks the selection in place; **Shift+L** pins it to the screen while you pan and zoom |
| Snap / align | Moves snap to nearby edges and centers (hold **Alt** to move freely); **Alt+arrows**, **Alt+C/M** align, **Alt+H/V** space evenly |
| Watch folder | **Ctrl+O** → Watch a folder: new images there appear on the board |
| Radial menu | **Hold Q**, point at an option, release (find similar, web search, palette, study view, guide, crop, note) |
| Boards | **Ctrl+O**: new, open folder, recent boards |
| Export PNG | **Ctrl+E** (selection, or everything; optional watermark) |
| Study views | **V** / **Shift+V** cycle value (3 or 5 tones), saturation, noise/detail and squint |
| Composition guide | **C** cycles thirds, phi grid, golden spiral, diagonals, center; **Shift+C** turns the spiral |
| Color picker | **I**, then hover: hex, HSV and perceptual value (L*) |
| Note | **N** adds a note at the cursor and starts typing; **Shift+N** changes its color |
| Image slot | **E** adds an empty, labeled slot; drop, paste or drag an image onto it to fill it |
| Frame | **Ctrl+G** wraps the selection in a titled frame (nothing selected: an empty one) |
| Edit text | **Double-click** a note, or a frame's title bar |
| Color palette | **P** extracts 6 main colors under each selected image (**Shift+P**: 10) |
| Duplicate | **Ctrl+D** copies the selection right beside it |
| Caption | **Double-click** an image to write a caption under it |
| Surprise me | **S** opens a random image from this board or any recent one in focus view |
| Focus view | **Enter** shows the selected image (or the one under the cursor) big on a dark backdrop; **← →** step through every image |
| Whole-board values | **Shift+B**: every image and palette in grayscale, then 3 tones, then back to color |
| Draw on images | **D**, then drag on an image: arrow, circle, box or pen, in 6 colors; **Shift+D** hides drawings |
| Crop | Select one image, **X**, drag over the part to keep; **Enter**/**Esc** when done; **Shift+X** resets |
| Grid arrange | **G** packs the selection (or everything) into rows; **Shift+G** also scales them to one height |
| Remove | **Delete** / **Backspace** |
| Help | **H**, or the **?** button in the top-right corner (✕ closes it) |
| Radial menu | Hold **Q**: image actions over an image, board actions (note, link, slot, paste, fit, surprise, boards, help) elsewhere |
| Undo / redo | **Ctrl+Z** / **Ctrl+Shift+Z** (or **Ctrl+Y**) |
| Fit to view | **F** (fits selection, or everything) |
| Zoom 100% | **1** |
| Select all / deselect | **Ctrl+A** / **Esc** |
| Help overlay | **H** |

## How boards are stored

A board is a plain folder:

```
board.json   # layout: positions, sizes, stacking order, view, settings
images/      # a copy of every image, named by content hash (duplicates are stored once)
exports/     # PNGs made with Ctrl+E
signatures.json  # cached value/palette signatures for Find more
```

Images are copied in, so the board keeps working if the originals move. The board autosaves shortly
after each change, and unreferenced images are cleaned up when you close the window.
Supported formats: PNG, JPEG, GIF (first frame), WebP, BMP.

## Organizing

- **Tags**: select items and press **Ctrl+T**. Type a tag and press **Enter** (repeat for more), or
  click one already used on the board; click a tag's ✕ to remove it. Tags are lowercase and show
  as chips under an item while it's hovered or selected. An image's tags also prefill the subject
  in **Find more → Search the web**.
- **Search**: **Ctrl+F** opens a search bar. Everything that doesn't match fades back. Matching uses
  tags (by prefix: `for` finds `forest`), note text and frame titles; every word must match.
  **Enter** selects the matches and zooms to them; **Esc** closes the bar.
- **Lock** (**L**): locked items can't be moved, resized, rotated, cropped, arranged or deleted
  (Delete skips them) until you press **L** again. A 🔒 badge marks them.
- **Pin** (**Shift+L**): pinned items stay put on screen while the board pans and zooms under
  them, like a reference you keep in the corner while you work. You can still drag and resize them.
  📌 marks them; **Shift+L** again drops them back onto the board where they are.
- **Snapping**: while you drag, the moved group's edges and center snap to the edges and centers of
  nearby items, with magenta guides. Hold **Alt** to move freely.
- **Align and space evenly**: with two or more items selected, a toolbar at the bottom aligns
  their left, center, right, top, middle or bottom edges, or spaces them evenly across or down
  (3+ items, keeping the outer ones in place). Shortcuts: **Alt+←/→/↑/↓**, **Alt+C** (center),
  **Alt+M** (middle), **Alt+H** / **Alt+V** (space evenly).

## Watch a folder

Point a board at a folder (your Unity or game screenshots, a Blender render output, a downloads
folder) and new images saved there appear on the board by themselves. Open **Ctrl+O** and under
**Watch a folder** choose the folder (or paste its path and press **Watch**).

- New images land in a **Watching: <folder>** frame to the right of the board, in rows, scaled to
  the same height; the frame grows as they arrive. Move or resize the frame like any other.
- A file is only imported once it has stopped growing, so half-saved renders and screenshots are
  left alone until they're done. Hidden files and non-images are ignored.
- Images already in the folder when you start are skipped; **Add the N already there** brings
  them in too.
- Each arrival is one undo step. Watching is saved with the board and continues next time you
  open it; **Stop watching** ends it. The folder is checked every 1.5 seconds while the app runs.

## Find more pictures

Hold **Q** over an image to open the **radial menu**, point at an option and let go (or click it;
**Esc** closes it). Its first three options open the **Find more** window for that image:

- **Similar values** ranks every image on your other boards by value structure: an 8×8 grid of
  perceptual lightness (L*), so images with the same big light and dark masses come first.
  Mirror images count as matches.
- **Similar palette** ranks them by colors: each palette color is matched to its nearest
  counterpart (CIE Lab distance), weighted by how much of the image it covers.
- **Search the web** opens Google Images, Unsplash, Pinterest or ArtStation in your browser for a
  subject you type (prefilled from the frame the image sits in). It's filtered by the image's main
  color, or black & white, or any color you pick, where the site supports filters.

Click a result to copy it onto the current board next to the original. Matching runs offline. The
first search computes a small signature for each image and caches it in each board's
`signatures.json`, so later searches are fast. Subjects aren't recognized automatically (that would
need a large AI model), which is why web searches use the words you type.

## Notes and frames

- **Notes**: press **N** to drop a note at the cursor and type. Click anywhere else when done (an
  empty note disappears). Double-click to edit later, **Shift+N** cycles yellow, pink, blue, green
  and dark. Drag the corner handle to change the width; the height follows the text.
- **Frames** organize a board into labeled sections. Select some items and press **Ctrl+G** to put a
  frame around them, then type its title. Frames sit behind everything. Grab one by its **title
  bar** to move it, and everything inside comes along; the rest of the frame is click-through, so
  you can still box-select inside it. Resize freely with the corner handle; double-click the title
  to rename. To select a frame by box, box it in completely.

Both are undoable and included in PNG exports.

## Image slots

Plan a board before you have the pictures. Press **E** to drop an empty **slot** at the cursor and type
what belongs there, e.g. *Front view*, *Color ref*, *Lighting* (**Enter** when done; double-click to
rename later). Slots show a picture icon and their label, so it's easy to see what's still missing.

Fill a slot by:

- **dropping** an image file onto it,
- **selecting** it and pasting (**Ctrl+V**), or
- **dragging** an image that's already on the board onto it (the slot lights up with
  *Drop to fill "…"*).

The image takes the slot's place and exact size, center-cropped to fit (adjust with **X**, it's the
usual non-destructive crop), and the slot's label is kept as a tag, so it's still easy to identify and
search. Undo puts the empty slot back. Resize slots freely with the corner handle; empty slots show as
labeled dashed boxes in exports.

## Color palettes

Select one or more images and press **P** (or **Shift+P** for 10 colors). Each image gets a strip of
its dominant colors just below it, most common first, labeled with hex and perceptual value (L*)
when big enough to read. Cropped images only use the visible part. Palettes are board items like
images: move, resize, delete and export them, and the color picker (**I**) reads their swatches.

## Feels alive

Core Board moves like a real board: things **glide** to their new places (grid arrange, align, undo,
fit view) instead of jumping, new images, notes and copies **pop** in, a dragged image **lifts** with
a soft shadow and **swings** with your motion, and a **flicked** image, or the whole board when you
fling it while panning, coasts to a stop. Grab anything to stop it.

Prefer a still board? Untick **Animations** in the Boards window (**Ctrl+O**); it's remembered.

## Opening a board from a newer version

If a board was saved by a newer Core Board, an older version can't read it. It then says so and
leaves the board alone: nothing is saved over it and none of its pictures are removed, and a backup
copy of its board file is kept. Update Core Board to open it.

## Language

Core Board speaks English and Spanish. It follows your computer's language; to pick one yourself,
open the Boards window (**Ctrl+O**) and choose under **Language** (Automatic, English, Español).

## Updates

Core Board checks mikibutler.ca for a newer version when it starts (one small request; nothing
about you or your boards is sent). When there is one, a banner shows what's new:

- **Windows** (installed with the installer): **Update now** downloads the new version, installs it
  and reopens Core Board. Your boards are saved first.
- **macOS, Linux and the portable zip**: **Download** opens the download page.

The Boards window (**Ctrl+O**) has **Check for updates**, a **Download page** button and the option
to turn the startup check off.

## Files and links

Anything that isn't a picture still has a place on the board. Drop (or paste) an Illustrator
file, a Photoshop or Blender file, a video, an app shortcut or a folder, and it shows up as a small
card with its name and type. **Double-click** the card to open it with its usual app, just like in
your file browser. The file stays where it is: the card is a shortcut to it.

**Ctrl+click** an empty spot to add a web link: type or paste the address, and the card shows the
page's title and its preview picture (the one chat apps show for links). Double-click it to open the page. Pasting a link to a page without a picture also
makes a link card.

## Copy a picture

Select an image and press **Ctrl+C**, then paste it into Photoshop, Krita, Blender, a chat or any
other app. It's copied at full resolution as the board shows it: crop, turns, mirroring and study
view included. With several items selected, they're copied as one picture, like an export.

## PDFs

Drop or paste a PDF (an art book, a model sheet, a magazine) and its pages land on the board as
images, laid out in a grid and labeled with the file name and page number. Up to 40 pages are
imported from one PDF. A PDF that can't be read (for example, password-protected) goes on the
board as a card that opens it.

## Groups

Select a few items and press **Ctrl+Shift+G** to group them: clicking any of them then picks the
whole group, so it moves, copies and deletes as one, with a dashed outline while it's selected.
Press **Ctrl+Shift+G** again on a group to ungroup it. (**Ctrl+G** still puts a frame around the
selection.)

## YouTube videos

Paste a YouTube link (or add one with **Ctrl+click**) and it becomes a video card with the video's
picture, title and a play button. **Double-click** it to watch: on Windows and macOS a small player
window opens and floats on top, so the video plays next to your board; on Linux it opens in the
browser.

## Paste a web link

Copy a picture's web address (right-click > *Copy image address* in the browser) and press **Ctrl+V**
on the board: Core Board downloads it and drops it where the pointer is. It also works with the
address of a page, such as an ArtStation, museum or blog page: the page's main picture (the one link
previews show) is downloaded. Pasting onto a selected empty slot fills the slot.

## Mini-map

While part of the board is off screen, a small overview sits in the bottom-right corner with the
visible part outlined. Click in it to glide there, or drag in it to move around. Untick
**Mini-map** in the Boards window (**Ctrl+O**) to hide it.

## Surprise me

Press **S** for a random image from this board or any of your recent boards, shown in focus view.
Great for rediscovering old references or picking a warm-up study. Press **S** again for another;
a pick from a different board opens that board.

## Magnifier

Hold **Z** to look closer without zooming the board: a round magnifying glass follows the
pointer and shows the pictures under it enlarged (3× to start). **Scroll** while holding Z to
change how much it enlarges, from 2× to 8×. Let go of Z and it's gone.

## Focus view

Press **Enter** to study one image without the rest of the board around it. The selected image (or
the one under the cursor) grows out of its spot to fill the window on a dark backdrop, with its
caption and an *image 3 of 7* counter underneath.

- **← →** (or **↑ ↓**, Page Up/Down) step through every image on the board in reading order: rows
  top to bottom, each row left to right.
- **Scroll** zooms into details, **drag** pans, **double-click** (or **F**) fits it again.
- The study keys work on the image you're looking at: **V** value and saturation views, **C** guides,
  **B** black and white, **Shift+B** whole-board values, **M** mirror, **R** rotate.
- **Esc**, **Enter**, **Space** or a click on the backdrop shrinks it back into the board, with that
  image selected. The board's view isn't touched.

## Captions, duplicates and whole-board values

- **Captions**: double-click an image and type a short caption, e.g. *Sargent, 1885, rim light*
  (**Enter** when done). It sits under the image, moves with it, shows up in search and in PNG
  exports. Double-click again to change it; clear the text to remove it.
- **Duplicate** (**Ctrl+D**): copies the selected items right beside them, to try a different crop,
  study view or set of drawings side by side.
- **Whole-board value check** (**Shift+B**): shows every image and palette on the board in grayscale,
  press again for 3 tones (notan), and again to go back to color. It's a view only: nothing on the
  board changes, and exports stay in color.

## Drawing

Press **D** (or **Done** / **Esc** to stop) and a toolbar appears at the top: **Pen**, **Arrow**,
**Circle** or **Box**, and six marker colors. Drag anywhere to draw, e.g. an arrow at the rim light
or a circle around a detail to copy. Pan with the right or middle button, or Space.

- **Start on a picture** and the drawing belongs to it: it moves and resizes with it, and turns,
  flips and crops along with it.
- **Start on an empty spot** and the drawing is its own object: click its line to select it, then
  move, resize or delete it like anything else.

Each stroke is one undo step (**Ctrl+Z**). **Clear** removes the drawings on the selected items (or
everywhere, with nothing selected), **Hide** / **Shift+D** hides or shows all drawings, and
drawings are included in PNG exports.

## Cropping

Select one image and press **X**. The whole image shows with the cut-away parts dimmed; drag a
box over the part you want to keep. You can drag again to refine, even outward to bring back parts
you cropped off. Press **Enter** or **Esc** (or click without dragging) when done. **Shift+X**
shows the whole image again.

Cropping is non-destructive: the original file is never changed, and crops survive rotating,
flipping, study views and export.

## Study tools

For painting and drawing reference. Select images, then:

- **V** steps through study views (**Shift+V** goes back). Each one is per image:
  - **Values: 3 / 5 tones**: lightness grouped into a few flat tones, like a notan study, to
    check the value structure.
  - **Saturation map**: white where color is most saturated, black where it's gray.
  - **Noise / detail map**: bright where fine detail is busy, dark where the image rests. Good for
    spotting noisy areas versus calm ones.
  - **Squint**: a heavy blur that shows only the big shapes.
- **B** desaturates (plain grayscale).
- **C** cycles composition templates drawn over the image: rule of thirds, phi grid, golden spiral,
  diagonals with reciprocals (dynamic symmetry) and center cross. **Shift+C** turns the spiral to
  each corner.
- **I** toggles the color picker. Hover any image to read its hex color, hue/saturation/value, and
  perceptual value (CIE L*, 0 = black, 100 = white). It always reads the original colors, even
  when the image is shown in a study view.

Values use CIE L* lightness, so the tone bands match how light or dark colors actually look
(a pure yellow and a pure blue have the same HSV value but very different L*). Study views are
saved with the board and included in PNG exports; guides are on-screen only.

## Export with watermark

**Ctrl+E** opens the export dialog. It renders the selected images (or the whole board) into one PNG
from the original files, so the export is full quality with rotation, flip and grayscale applied.

- **Watermark text**: e.g. `© Your Name`. Leave it empty to export without a watermark.
- **Position**: any corner, center, or tiled across the whole image.
- **Opacity**: 10–100%.

When it's done, a message shows the file name until you close it (**✕**); click it or **Open folder**
to go straight to the file.

The text gets a soft shadow so it reads on light and dark images. Your watermark settings are saved
with the board. Exports go to the board's `exports/` folder as `coreboard-<date>-<time>.png` (UTC);
use **Change…** in the dialog to pick another folder for this board (**Reset** goes back). Very large boards are scaled down to fit 8192 px.

## Overlay mode

Lower the opacity with **[** and press **T** to keep the board floating above other apps, e.g. to
trace or match colors against reference while you work. Both settings are saved with the board.
On Linux, see-through windows need a compositing desktop (GNOME, KDE and most others have one).

## Building from source

For development. You need [Rust](https://rustup.rs) (on Windows, also the Visual Studio Build Tools
with "Desktop development with C++").

```sh
cargo run --release              # reopens the board you used last
cargo run --release -- ~/boards/moodboard   # open (or create) a board in any folder
```

Installers are built by GitHub Actions (`.github/workflows/release.yml`). To ship a new version, bump
`version` in `Cargo.toml` and push to `main`: the workflow builds Windows, macOS and Linux and publishes
release `v<version>` with what changed. Running **Build installers** by hand from the Actions tab
just builds (files under the run's Artifacts); tick **publish** to release, and set **commit** to
build an older commit. The Windows installer script is `installer/coreboard.iss` (Inno Setup).

## Linux build notes

The folder picker uses the desktop's file chooser portal (`xdg-desktop-portal`), which GNOME, KDE
and most desktops run already.

Needs the usual windowing libraries at runtime (`libxkbcommon-x11`, a Vulkan or GL driver), which any
desktop install already has.
