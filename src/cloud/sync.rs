//! Keeps boards on this computer and boards in the cloud the same.
//!
//! Each board is a folder in the cloud holding its `board.json` and its pictures. Pictures are
//! named after their content, so a picture is uploaded or downloaded once and never changes;
//! only `board.json` (the layout) can differ. Each local board remembers, in `cloud.json`, which
//! cloud board it belongs to and what both sides looked like at the last sync. From that:
//!
//! - changed only here: upload it;
//! - changed only in the cloud: download it;
//! - changed on both sides: nothing is lost. The cloud version is saved as a separate board
//!   ("… (other computer)") and this computer's version is uploaded.
//!
//! Deleting a board on one computer doesn't delete it anywhere else.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::board::{Board, fnv1a};

/// The file in each board folder that links it to its cloud board.
const STATE: &str = "cloud.json";
/// What a sync of the open board leaves for the app to apply (see `Report::incoming`).
pub const INCOMING: &str = "board.json.incoming";
const INCOMING_STATE: &str = "cloud.json.incoming";

/// A board in the cloud.
#[derive(Clone, Debug)]
pub struct RemoteBoard {
    pub id: String,
    pub name: String,
}

/// Where boards are kept in the cloud. Google Drive in the app; a pretend one in the tests.
pub trait Remote {
    fn boards(&mut self) -> Result<Vec<RemoteBoard>, String>;
    /// Makes an empty board called `name` and returns its id.
    fn create_board(&mut self, name: &str) -> Result<String, String>;
    /// The version of the board's layout, or None when it has none yet. Any change to the
    /// layout changes the version.
    fn layout_version(&mut self, board: &str) -> Result<Option<String>, String>;
    /// The layout and its version.
    fn download_layout(&mut self, board: &str) -> Result<(Vec<u8>, String), String>;
    /// Replaces the layout and returns the new version.
    fn upload_layout(&mut self, board: &str, json: &[u8]) -> Result<String, String>;
    /// Names of the pictures the board has in the cloud.
    fn pictures(&mut self, board: &str) -> Result<HashSet<String>, String>;
    fn upload_picture(&mut self, board: &str, name: &str, bytes: &[u8]) -> Result<(), String>;
    fn download_picture(&mut self, board: &str, name: &str) -> Result<Vec<u8>, String>;
}

/// What a local board knew at its last sync.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    /// The cloud board it belongs to.
    pub remote: String,
    /// Hash of the layout both sides agreed on, when they have.
    pub synced: Option<u64>,
    /// The cloud layout's version at that point.
    pub version: Option<String>,
}

fn read_state(dir: &Path) -> Option<State> {
    serde_json::from_slice(&fs::read(dir.join(STATE)).ok()?).ok()
}

fn write_state(dir: &Path, name: &str, state: &State) -> Result<(), String> {
    let json = serde_json::to_vec_pretty(state).map_err(|e| e.to_string())?;
    fs::write(dir.join(name), json).map_err(|e| e.to_string())
}

/// What a sync did.
#[derive(Debug, Default)]
pub struct Report {
    pub uploaded: usize,
    pub downloaded: usize,
    /// Boards that came from the cloud for the first time (or as a kept copy after both
    /// sides changed), to list in the Boards window.
    pub new_boards: Vec<PathBuf>,
    /// The open board changed in the cloud: the new layout is waiting in `INCOMING` for the
    /// app to apply, unless the board was edited meanwhile.
    pub incoming: bool,
    /// Boards where both sides had changed.
    pub conflicts: usize,
}

/// The folder for a new board called `name` under `root`, not taken yet.
fn free_folder(root: &Path, name: &str) -> PathBuf {
    let name = crate::boards::folder_name(name).unwrap_or_else(|| "Board".into());
    let mut dir = root.join(&name);
    let mut n = 2;
    while dir.exists() {
        dir = root.join(format!("{name} {n}"));
        n += 1;
    }
    dir
}

/// A cloud name not used by any of `taken`.
fn free_name(taken: &HashSet<String>, name: &str) -> String {
    let mut candidate = name.to_string();
    let mut n = 2;
    while taken.contains(&candidate) {
        candidate = format!("{name} {n}");
        n += 1;
    }
    candidate
}

fn board_name(dir: &Path) -> String {
    crate::boards::display_name(dir)
}

fn picture_names(board: &Board) -> HashSet<String> {
    board
        .items
        .iter()
        .map(|i| i.file.clone())
        .filter(|f| !f.is_empty())
        .collect()
}

/// Syncs every board in `locals` and brings down cloud boards this computer doesn't have yet
/// (into `root`). `open` is the board on screen: it's never written to directly.
pub fn sync_all(
    remote: &mut dyn Remote,
    locals: &[PathBuf],
    root: &Path,
    open: &Path,
) -> Result<Report, String> {
    let mut report = Report::default();
    let mut boards = remote.boards()?;
    let mut linked: HashSet<String> = HashSet::new();
    let same = |a: &Path, b: &Path| {
        a.canonicalize().unwrap_or_else(|_| a.to_path_buf())
            == b.canonicalize().unwrap_or_else(|_| b.to_path_buf())
    };

    // Each local board once, linked to its cloud board (made now if it has none).
    let mut seen: Vec<PathBuf> = Vec::new();
    let mut jobs: Vec<(PathBuf, State)> = Vec::new();
    for dir in locals {
        if !dir.join("board.json").is_file() || seen.iter().any(|d| same(d, dir)) {
            continue;
        }
        seen.push(dir.clone());
        let state = match read_state(dir) {
            Some(state) if boards.iter().any(|b| b.id == state.remote) => state,
            // Linked to a cloud board that's gone (deleted there): upload it again.
            _ => {
                let name = board_name(dir);
                let local: Board = serde_json::from_slice(
                    &fs::read(dir.join("board.json")).map_err(|e| e.to_string())?,
                )
                .unwrap_or_default();
                let unclaimed = boards
                    .iter()
                    .find(|b| b.name == name && !linked.contains(&b.id))
                    .cloned();
                match unclaimed {
                    // An empty board here and one of that name in the cloud (every computer
                    // starts with a "default" board): this one becomes that one.
                    Some(b) if local.items.is_empty() => State {
                        remote: b.id,
                        synced: None,
                        version: None,
                    },
                    // Two different boards with one name: this one goes up under a new name.
                    _ => {
                        let taken: HashSet<String> =
                            boards.iter().map(|b| b.name.clone()).collect();
                        let name = free_name(&taken, &name);
                        let id = remote.create_board(&name)?;
                        boards.push(RemoteBoard {
                            id: id.clone(),
                            name,
                        });
                        State {
                            remote: id,
                            synced: None,
                            version: None,
                        }
                    }
                }
            }
        };
        if linked.insert(state.remote.clone()) {
            jobs.push((dir.clone(), state));
        }
    }

    for (dir, state) in jobs {
        let is_open = same(&dir, open);
        sync_board(remote, &dir, state, root, is_open, &mut report)?;
    }

    // Cloud boards with no copy here yet.
    for board in boards.iter().filter(|b| !linked.contains(&b.id)) {
        if remote.layout_version(&board.id)?.is_none() {
            continue;
        }
        let dir = free_folder(root, &board.name);
        pull_into(remote, &board.id, &dir, None, STATE, &mut report)?;
        report.new_boards.push(dir);
    }
    Ok(report)
}

fn sync_board(
    remote: &mut dyn Remote,
    dir: &Path,
    state: State,
    root: &Path,
    is_open: bool,
    report: &mut Report,
) -> Result<(), String> {
    let local = fs::read(dir.join("board.json")).map_err(|e| e.to_string())?;
    let hash = fnv1a(&local);
    let version = remote.layout_version(&state.remote)?;
    let local_changed = state.synced != Some(hash);
    let remote_changed = version.is_some() && version != state.version;

    match (local_changed, remote_changed) {
        (false, false) => write_state(dir, STATE, &state),
        (true, false) => push(remote, dir, &local, state.remote, report),
        (false, true) => {
            if is_open {
                // Left for the app, which applies it unless the board was edited meanwhile.
                pull_into(
                    remote,
                    &state.remote,
                    dir,
                    Some(INCOMING),
                    INCOMING_STATE,
                    report,
                )?;
                report.incoming = true;
                Ok(())
            } else {
                pull_into(remote, &state.remote, dir, None, STATE, report)
            }
        }
        (true, true) => {
            // Changed on both sides. A board that has never synced and has nothing on it yet
            // simply takes the cloud's.
            let empty = serde_json::from_slice::<Board>(&local)
                .map(|b| b.items.is_empty())
                .unwrap_or(false);
            if state.synced.is_none() && empty {
                return if is_open {
                    pull_into(
                        remote,
                        &state.remote,
                        dir,
                        Some(INCOMING),
                        INCOMING_STATE,
                        report,
                    )?;
                    report.incoming = true;
                    Ok(())
                } else {
                    pull_into(remote, &state.remote, dir, None, STATE, report)
                };
            }
            // Keep the cloud's as its own board (it goes up as a new cloud board next time),
            // then this computer's goes up.
            let copy = free_folder(
                root,
                &format!(
                    "{} ({})",
                    board_name(dir),
                    crate::i18n::tr("other computer")
                ),
            );
            pull_into(
                remote,
                &state.remote,
                &copy,
                None,
                "cloud.json.unused",
                report,
            )?;
            let _ = fs::remove_file(copy.join("cloud.json.unused"));
            report.new_boards.push(copy);
            report.conflicts += 1;
            push(remote, dir, &local, state.remote, report)
        }
    }
}

/// Uploads the pictures the cloud is missing, then the layout.
fn push(
    remote: &mut dyn Remote,
    dir: &Path,
    local: &[u8],
    id: String,
    report: &mut Report,
) -> Result<(), String> {
    let board: Board = serde_json::from_slice(local).map_err(|e| e.to_string())?;
    let there = remote.pictures(&id)?;
    for name in picture_names(&board) {
        if there.contains(&name) {
            continue;
        }
        // A picture missing here too can't go up; the layout still does.
        if let Ok(bytes) = fs::read(dir.join("images").join(&name)) {
            remote.upload_picture(&id, &name, &bytes)?;
        }
    }
    let version = remote.upload_layout(&id, local)?;
    report.uploaded += 1;
    write_state(
        dir,
        STATE,
        &State {
            remote: id,
            synced: Some(fnv1a(local)),
            version: Some(version),
        },
    )
}

/// Downloads cloud board `id` into `dir`: the pictures it's missing, then the layout, written
/// as `layout_name` (board.json unless given) with its state as `state_name`.
fn pull_into(
    remote: &mut dyn Remote,
    id: &str,
    dir: &Path,
    layout_name: Option<&str>,
    state_name: &str,
    report: &mut Report,
) -> Result<(), String> {
    let (json, version) = remote.download_layout(id)?;
    let board: Board = serde_json::from_slice(&json).map_err(|_| {
        crate::i18n::tr("a board in the cloud was made by a newer Core Board").to_string()
    })?;
    let images = dir.join("images");
    fs::create_dir_all(&images).map_err(|e| e.to_string())?;
    let there = remote.pictures(id)?;
    for name in picture_names(&board) {
        let path = images.join(&name);
        if path.exists() || !there.contains(&name) || !safe_picture_name(&name) {
            continue;
        }
        let bytes = remote.download_picture(id, &name)?;
        fs::write(path, bytes).map_err(|e| e.to_string())?;
    }
    fs::write(dir.join(layout_name.unwrap_or("board.json")), &json).map_err(|e| e.to_string())?;
    report.downloaded += 1;
    write_state(
        dir,
        state_name,
        &State {
            remote: id.to_string(),
            synced: Some(fnv1a(&json)),
            version: Some(version),
        },
    )
}

/// Picture names come from the cloud: only plain file names are written.
fn safe_picture_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 200
        && !name.starts_with('.')
        && !name.contains(['/', '\\', ':', '\0'])
}

/// The open board's new layout from the cloud, if a sync left one.
pub fn take_incoming(dir: &Path) -> Option<Board> {
    let bytes = fs::read(dir.join(INCOMING)).ok()?;
    let board = serde_json::from_slice(&bytes).ok();
    if board.is_some() {
        let _ = fs::rename(dir.join(INCOMING_STATE), dir.join(STATE));
    }
    discard_incoming(dir);
    board
}

/// Drops what a sync left for the open board (it was edited meanwhile: the next sync keeps
/// both versions).
pub fn discard_incoming(dir: &Path) {
    let _ = fs::remove_file(dir.join(INCOMING));
    let _ = fs::remove_file(dir.join(INCOMING_STATE));
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::board::Item;

    type Pictures = std::collections::HashMap<String, Vec<u8>>;
    /// A board in the pretend cloud: its details, its layout and version, its pictures.
    type FakeBoard = (RemoteBoard, Option<(Vec<u8>, u32)>, Pictures);

    /// An in-memory cloud.
    #[derive(Default)]
    pub struct FakeRemote {
        pub boards: Vec<FakeBoard>,
        pub uploads: usize,
    }

    impl FakeRemote {
        fn find(&mut self, id: &str) -> Result<&mut FakeBoard, String> {
            self.boards
                .iter_mut()
                .find(|b| b.0.id == id)
                .ok_or_else(|| format!("no board {id}"))
        }
    }

    impl Remote for FakeRemote {
        fn boards(&mut self) -> Result<Vec<RemoteBoard>, String> {
            Ok(self.boards.iter().map(|b| b.0.clone()).collect())
        }
        fn create_board(&mut self, name: &str) -> Result<String, String> {
            let id = format!("r{}", self.boards.len() + 1);
            self.boards.push((
                RemoteBoard {
                    id: id.clone(),
                    name: name.into(),
                },
                None,
                Pictures::new(),
            ));
            Ok(id)
        }
        fn layout_version(&mut self, board: &str) -> Result<Option<String>, String> {
            Ok(self.find(board)?.1.as_ref().map(|(_, v)| v.to_string()))
        }
        fn download_layout(&mut self, board: &str) -> Result<(Vec<u8>, String), String> {
            let (json, v) = self.find(board)?.1.clone().ok_or("no layout")?;
            Ok((json, v.to_string()))
        }
        fn upload_layout(&mut self, board: &str, json: &[u8]) -> Result<String, String> {
            let entry = self.find(board)?;
            let v = entry.1.as_ref().map(|(_, v)| v + 1).unwrap_or(1);
            entry.1 = Some((json.to_vec(), v));
            Ok(v.to_string())
        }
        fn pictures(&mut self, board: &str) -> Result<HashSet<String>, String> {
            Ok(self.find(board)?.2.keys().cloned().collect())
        }
        fn upload_picture(&mut self, board: &str, name: &str, bytes: &[u8]) -> Result<(), String> {
            self.uploads += 1;
            self.find(board)?.2.insert(name.into(), bytes.to_vec());
            Ok(())
        }
        fn download_picture(&mut self, board: &str, name: &str) -> Result<Vec<u8>, String> {
            self.find(board)?
                .2
                .get(name)
                .cloned()
                .ok_or("no picture".into())
        }
    }

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("coreboard-sync-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Writes a board with one picture per caption.
    fn make_board(dir: &Path, captions: &[&str]) {
        fs::create_dir_all(dir.join("images")).unwrap();
        let mut board = Board::default();
        for (n, caption) in captions.iter().enumerate() {
            let file = format!("{caption}.png");
            fs::write(dir.join("images").join(&file), caption.as_bytes()).unwrap();
            board.items.push(Item {
                id: n as u64 + 1,
                file,
                caption: caption.to_string(),
                ..Item::default()
            });
        }
        fs::write(dir.join("board.json"), serde_json::to_vec(&board).unwrap()).unwrap();
    }

    fn captions(dir: &Path) -> Vec<String> {
        let board: Board =
            serde_json::from_slice(&fs::read(dir.join("board.json")).unwrap()).unwrap();
        board.items.into_iter().map(|i| i.caption).collect()
    }

    /// Two computers sharing one cloud.
    struct Setup {
        base: PathBuf,
        cloud: FakeRemote,
    }

    impl Setup {
        fn new(name: &str) -> Self {
            Self {
                base: temp(name),
                cloud: FakeRemote::default(),
            }
        }
        fn root(&self, computer: &str) -> PathBuf {
            self.base.join(computer)
        }
        /// Syncs every board one computer has; nothing is open.
        fn sync(&mut self, computer: &str) -> Report {
            let root = self.root(computer);
            fs::create_dir_all(&root).unwrap();
            let mut locals: Vec<PathBuf> = fs::read_dir(&root)
                .unwrap()
                .flatten()
                .map(|e| e.path())
                .collect();
            locals.sort();
            sync_all(&mut self.cloud, &locals, &root, Path::new("/nowhere")).unwrap()
        }
    }

    #[test]
    fn boards_travel_between_computers() {
        let mut s = Setup::new("travel");
        make_board(&s.root("laptop").join("Faces"), &["eye", "nose"]);
        let r = s.sync("laptop");
        assert_eq!(r.uploaded, 1);
        assert_eq!(s.cloud.uploads, 2);

        // The desktop gets it, pictures and all.
        let r = s.sync("desktop");
        assert_eq!(r.new_boards, vec![s.root("desktop").join("Faces")]);
        assert_eq!(captions(&s.root("desktop").join("Faces")), ["eye", "nose"]);
        assert_eq!(
            fs::read(s.root("desktop").join("Faces/images/nose.png")).unwrap(),
            b"nose"
        );

        // Nothing changed: nothing moves.
        let uploads = s.cloud.uploads;
        let r = s.sync("desktop");
        assert_eq!((r.uploaded, r.downloaded), (0, 0));

        // A change on the desktop reaches the laptop; only the new picture is uploaded.
        make_board(&s.root("desktop").join("Faces"), &["eye", "nose", "mouth"]);
        s.sync("desktop");
        assert_eq!(s.cloud.uploads, uploads + 1);
        let r = s.sync("laptop");
        assert_eq!(r.downloaded, 1);
        assert_eq!(
            captions(&s.root("laptop").join("Faces")),
            ["eye", "nose", "mouth"]
        );
        let _ = fs::remove_dir_all(&s.base);
    }

    #[test]
    fn changes_on_both_computers_keep_both() {
        let mut s = Setup::new("conflict");
        make_board(&s.root("laptop").join("Faces"), &["eye"]);
        s.sync("laptop");
        s.sync("desktop");
        make_board(&s.root("laptop").join("Faces"), &["eye", "laptop"]);
        make_board(&s.root("desktop").join("Faces"), &["eye", "desktop"]);
        s.sync("laptop");
        let r = s.sync("desktop");
        assert_eq!(r.conflicts, 1);
        // The desktop keeps its own, plus the laptop's as a separate board.
        assert_eq!(
            captions(&s.root("desktop").join("Faces")),
            ["eye", "desktop"]
        );
        let copy = s.root("desktop").join("Faces (other computer)");
        assert_eq!(captions(&copy), ["eye", "laptop"]);
        // And next time round everything, including the copy, is everywhere.
        s.sync("desktop");
        s.sync("laptop");
        assert_eq!(
            captions(&s.root("laptop").join("Faces")),
            ["eye", "desktop"]
        );
        assert_eq!(
            captions(&s.root("laptop").join("Faces (other computer)")),
            ["eye", "laptop"]
        );
        let _ = fs::remove_dir_all(&s.base);
    }

    #[test]
    fn a_new_computer_joins_without_losing_anything() {
        let mut s = Setup::new("join");
        make_board(&s.root("laptop").join("default"), &["sky"]);
        s.sync("laptop");
        // The desktop's own default board is empty: it simply becomes the cloud's.
        make_board(&s.root("desktop").join("default"), &[]);
        // And it has a different board that happens to share a name with nothing.
        make_board(&s.root("desktop").join("Hands"), &["thumb"]);
        let r = s.sync("desktop");
        assert_eq!(r.conflicts, 0);
        assert_eq!(captions(&s.root("desktop").join("default")), ["sky"]);
        s.sync("laptop");
        assert_eq!(captions(&s.root("laptop").join("Hands")), ["thumb"]);

        // A third computer with a full board of the same name keeps it, under a new cloud name.
        make_board(&s.root("studio").join("default"), &["tree"]);
        s.sync("studio");
        assert_eq!(captions(&s.root("studio").join("default")), ["tree"]);
        assert_eq!(captions(&s.root("studio").join("Hands")), ["thumb"]);
        s.sync("laptop");
        assert_eq!(captions(&s.root("laptop").join("default")), ["sky"]);
        assert_eq!(captions(&s.root("laptop").join("default 2")), ["tree"]);
        let _ = fs::remove_dir_all(&s.base);
    }

    #[test]
    fn the_open_board_is_never_written_behind_the_apps_back() {
        let mut s = Setup::new("open");
        make_board(&s.root("laptop").join("Faces"), &["eye"]);
        s.sync("laptop");
        s.sync("desktop");
        make_board(&s.root("laptop").join("Faces"), &["eye", "ear"]);
        s.sync("laptop");

        let root = s.root("desktop");
        let open = root.join("Faces");
        let r = sync_all(&mut s.cloud, std::slice::from_ref(&open), &root, &open).unwrap();
        assert!(r.incoming);
        // Still the old layout on disk, the new one waiting.
        assert_eq!(captions(&open), ["eye"]);
        let board = take_incoming(&open).unwrap();
        assert_eq!(board.items.len(), 2);
        assert!(open.join("images/ear.png").exists());
        // Applied: the app writes it and the next sync has nothing to do.
        let (json, _) = s
            .cloud
            .download_layout(&read_state(&open).unwrap().remote)
            .unwrap();
        fs::write(open.join("board.json"), json).unwrap();
        let r = sync_all(&mut s.cloud, std::slice::from_ref(&open), &root, &open).unwrap();
        assert_eq!((r.uploaded, r.downloaded, r.incoming), (0, 0, false));
        let _ = fs::remove_dir_all(&s.base);
    }
}
