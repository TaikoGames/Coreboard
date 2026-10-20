# Core Board

A small, fast reference board for images, written in Rust with [egui](https://github.com/emilk/egui).
Drop images on an infinite canvas, arrange and resize them, and they're saved automatically.

## Run

```sh
cargo run --release              # reopens the board you used last
cargo run --release -- ~/boards/moodboard   # open (or create) a board in any folder
```

Press **Ctrl+O** for the **Boards** window: create a board by name, open or create one in any folder
(native folder picker), or jump back to a recent board. Switching saves the current board first.

Boards created by name, and the first default board, live in your platform's data dir
(`~/.local/share/coreboard/` on Linux, `~/Library/Application Support/coreboard/` on macOS,
`%APPDATA%\coreboard\data\` on Windows). The recent list is kept in the matching config dir.

## Controls

| Action | Input |
| --- | --- |
| Add images | Drag & drop files, or **Ctrl+V** (image data or copied files) |
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
| Boards | **Ctrl+O**: new, open folder, recent boards |
| Export PNG | **Ctrl+E** (selection, or everything; optional watermark) |
| Study views | **V** / **Shift+V** cycle value (3 or 5 tones), saturation, noise/detail and squint |
| Composition guide | **C** cycles thirds, phi grid, golden spiral, diagonals, center; **Shift+C** turns the spiral |
| Color picker | **I**, then hover: hex, HSV and perceptual value (L*) |
| Note | **N** adds a note at the cursor and starts typing; **Shift+N** changes its color |
| Frame | **Ctrl+G** wraps the selection in a titled frame (nothing selected: an empty one) |
| Edit text | **Double-click** a note, or a frame's title bar |
| Color palette | **P** extracts 6 main colors under each selected image (**Shift+P**: 10) |
| Crop | Select one image, **X**, drag over the part to keep; **Enter**/**Esc** when done; **Shift+X** resets |
| Grid arrange | **G** packs the selection (or everything) into rows; **Shift+G** also scales them to one height |
| Remove | **Delete** / **Backspace** |
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
```

Images are copied in, so the board keeps working if the originals move. The board autosaves shortly
after each change, and unreferenced images are cleaned up when you close the window.
Supported formats: PNG, JPEG, GIF (first frame), WebP, BMP.

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

## Color palettes

Select one or more images and press **P** (or **Shift+P** for 10 colors). Each image gets a strip of
its dominant colors just below it, most common first, labeled with hex and perceptual value (L*)
when big enough to read. Cropped images only use the visible part. Palettes are board items like
images: move, resize, delete and export them, and the color picker (**I**) reads their swatches.

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

The text gets a soft shadow so it reads on light and dark images. Your watermark settings are saved
with the board. Exports go to the board's `exports/` folder as `coreboard-<date>-<time>.png` (UTC);
use **Change…** in the dialog to pick another folder for this board (**Reset** goes back). Very large boards are scaled down to fit 8192 px.

## Overlay mode

Lower the opacity with **[** and press **T** to keep the board floating above other apps, e.g. to
trace or match colors against reference while you work. Both settings are saved with the board.
On Linux, see-through windows need a compositing desktop (GNOME, KDE and most others have one).

## Linux build notes

The folder picker uses the desktop's file chooser portal (`xdg-desktop-portal`), which GNOME, KDE
and most desktops run already.

Needs the usual windowing libraries at runtime (`libxkbcommon-x11`, a Vulkan or GL driver), which any
desktop install already has.
