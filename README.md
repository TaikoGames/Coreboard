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
board.json   # layout: positions, sizes, stacking order, view
images/      # a copy of every image, named by content hash (duplicates are stored once)
```

Images are copied in, so the board keeps working if the originals move. The board autosaves shortly
after each change, and unreferenced images are cleaned up when you close the window.
Supported formats: PNG, JPEG, GIF (first frame), WebP, BMP.

## Linux build notes

Needs the usual windowing libraries at runtime (`libxkbcommon-x11`, a Vulkan or GL driver), which any
desktop install already has.
