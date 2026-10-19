# Core Board

A small, fast reference board for images, written in Rust with [egui](https://github.com/emilk/egui).
Drop images on an infinite canvas, arrange and resize them, and they're saved automatically.

## Run

```sh
cargo run --release              # opens your default board
cargo run --release -- ~/boards/moodboard   # open (or create) a board in any folder
```

The default board lives in your platform's data dir
(`~/.local/share/coreboard/default` on Linux, `~/Library/Application Support/coreboard/default` on macOS,
`%APPDATA%\coreboard\data\default` on Windows).

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
| Export PNG | **Ctrl+E** (selection, or everything; optional watermark) |
| Study views | **V** / **Shift+V** cycle value (3 or 5 tones), saturation, noise/detail and squint |
| Composition guide | **C** cycles thirds, phi grid, golden spiral, diagonals, center; **Shift+C** turns the spiral |
| Color picker | **I**, then hover: hex, HSV and perceptual value (L*) |
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
with the board, and exports go to the board's `exports/` folder as `coreboard-<date>-<time>.png`
(UTC). Very large boards are scaled down to fit 8192 px.

## Overlay mode

Lower the opacity with **[** and press **T** to keep the board floating above other apps, e.g. to
trace or match colors against reference while you work. Both settings are saved with the board.
On Linux, see-through windows need a compositing desktop (GNOME, KDE and most others have one).

## Linux build notes

Needs the usual windowing libraries at runtime (`libxkbcommon-x11`, a Vulkan or GL driver), which any
desktop install already has.
