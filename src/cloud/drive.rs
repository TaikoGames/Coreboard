//! Boards in Google Drive: a "Core Board" folder in the person's Drive, holding one folder per
//! board, each with its `board.json` and its pictures.

use std::collections::{HashMap, HashSet};

use serde::Deserialize;

use super::auth::encode;
use super::sync::{Remote, RemoteBoard};

const API: &str = "https://www.googleapis.com/drive/v3";
const UPLOAD: &str = "https://www.googleapis.com/upload/drive/v3";
const FOLDER: &str = "application/vnd.google-apps.folder";
/// The folder in Drive that holds every board.
const ROOT_NAME: &str = "Core Board";
const LAYOUT: &str = "board.json";
const MAX_DOWNLOAD: u64 = 512 * 1024 * 1024;

#[derive(Deserialize)]
struct File {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    version: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileList {
    #[serde(default)]
    files: Vec<File>,
    #[serde(default)]
    next_page_token: Option<String>,
}

pub struct Drive {
    agent: ureq::Agent,
    token: String,
    root: Option<String>,
    /// Per board folder: file name -> file id, filled when listed.
    files: HashMap<String, HashMap<String, String>>,
}

/// A Drive search query value: quotes and backslashes escaped.
fn quoted(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// The body of a Drive "multipart" upload: the file's details, then its bytes.
fn multipart(metadata: &serde_json::Value, bytes: &[u8], boundary: &str) -> Vec<u8> {
    let mut body = Vec::with_capacity(bytes.len() + 512);
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{metadata}\r\n\
             --{boundary}\r\nContent-Type: application/octet-stream\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

fn http_error(err: ureq::Error) -> String {
    match err {
        ureq::Error::StatusCode(401) => {
            crate::i18n::tr("Google Drive asked to sign in again").to_string()
        }
        ureq::Error::StatusCode(403) => {
            crate::i18n::tr("Google Drive refused (full, or too many requests; try later)")
                .to_string()
        }
        ureq::Error::StatusCode(code) => format!("Google Drive answered {code}"),
        ureq::Error::HostNotFound | ureq::Error::ConnectionFailed => {
            crate::i18n::tr("couldn't reach Google Drive (offline?)").to_string()
        }
        other => other.to_string(),
    }
}

impl Drive {
    pub fn new(token: String) -> Self {
        Self {
            agent: crate::web::agent(),
            token,
            root: None,
            files: HashMap::new(),
        }
    }

    fn bearer(&self) -> String {
        format!("Bearer {}", self.token)
    }

    fn get(&self, url: &str) -> Result<Vec<u8>, String> {
        let mut response = self
            .agent
            .get(url)
            .header("Authorization", self.bearer())
            .call()
            .map_err(http_error)?;
        response
            .body_mut()
            .with_config()
            .limit(MAX_DOWNLOAD)
            .read_to_vec()
            .map_err(|e| e.to_string())
    }

    fn send(&self, method: &str, url: &str, kind: &str, body: &[u8]) -> Result<Vec<u8>, String> {
        let request = match method {
            "PATCH" => self.agent.patch(url),
            _ => self.agent.post(url),
        };
        let mut response = request
            .header("Authorization", self.bearer())
            .header("Content-Type", kind)
            .send(body)
            .map_err(http_error)?;
        response.body_mut().read_to_vec().map_err(|e| e.to_string())
    }

    /// Every file matching a Drive search, following pages.
    fn list(&self, query: &str) -> Result<Vec<File>, String> {
        let mut out = Vec::new();
        let mut page: Option<String> = None;
        loop {
            let mut url = format!(
                "{API}/files?q={}&fields={}&pageSize=1000&spaces=drive",
                encode(query),
                encode("nextPageToken,files(id,name,version)")
            );
            if let Some(token) = &page {
                url += &format!("&pageToken={}", encode(token));
            }
            let list: FileList =
                serde_json::from_slice(&self.get(&url)?).map_err(|e| e.to_string())?;
            out.extend(list.files);
            match list.next_page_token {
                Some(token) => page = Some(token),
                None => return Ok(out),
            }
        }
    }

    fn create_folder(&self, name: &str, parent: Option<&str>) -> Result<String, String> {
        let mut metadata = serde_json::json!({ "name": name, "mimeType": FOLDER });
        if let Some(parent) = parent {
            metadata["parents"] = serde_json::json!([parent]);
        }
        let answer = self.send(
            "POST",
            &format!("{API}/files?fields=id"),
            "application/json; charset=UTF-8",
            metadata.to_string().as_bytes(),
        )?;
        let file: File = serde_json::from_slice(&answer).map_err(|e| e.to_string())?;
        Ok(file.id)
    }

    /// The "Core Board" folder, made the first time.
    fn root(&mut self) -> Result<String, String> {
        if let Some(root) = &self.root {
            return Ok(root.clone());
        }
        let found = self.list(&format!(
            "name = {} and mimeType = {} and trashed = false and 'root' in parents",
            quoted(ROOT_NAME),
            quoted(FOLDER)
        ))?;
        let id = match found.into_iter().next() {
            Some(file) => file.id,
            None => self.create_folder(ROOT_NAME, None)?,
        };
        self.root = Some(id.clone());
        Ok(id)
    }

    /// The files in a board's folder, by name.
    fn files_in(&mut self, board: &str) -> Result<&HashMap<String, String>, String> {
        if !self.files.contains_key(board) {
            let files = self.list(&format!("{} in parents and trashed = false", quoted(board)))?;
            let map = files.into_iter().map(|f| (f.name, f.id)).collect();
            self.files.insert(board.to_string(), map);
        }
        Ok(&self.files[board])
    }

    fn upload_new(&mut self, board: &str, name: &str, bytes: &[u8]) -> Result<File, String> {
        let boundary = format!("coreboard{:016x}", crate::board::fnv1a(bytes));
        let metadata = serde_json::json!({ "name": name, "parents": [board] });
        let answer = self.send(
            "POST",
            &format!("{UPLOAD}/files?uploadType=multipart&fields=id,name,version"),
            &format!("multipart/related; boundary={boundary}"),
            &multipart(&metadata, bytes, &boundary),
        )?;
        let file: File = serde_json::from_slice(&answer).map_err(|e| e.to_string())?;
        if let Some(files) = self.files.get_mut(board) {
            files.insert(name.to_string(), file.id.clone());
        }
        Ok(file)
    }

    /// The signed-in person's email, to show who's connected.
    pub fn email(&self) -> Result<String, String> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct User {
            #[serde(default)]
            email_address: String,
        }
        #[derive(Deserialize)]
        struct About {
            user: User,
        }
        let about: About = serde_json::from_slice(&self.get(&format!(
            "{API}/about?fields={}",
            encode("user(emailAddress)")
        ))?)
        .map_err(|e| e.to_string())?;
        Ok(about.user.email_address)
    }
}

impl Remote for Drive {
    fn boards(&mut self) -> Result<Vec<RemoteBoard>, String> {
        let root = self.root()?;
        Ok(self
            .list(&format!(
                "{} in parents and mimeType = {} and trashed = false",
                quoted(&root),
                quoted(FOLDER)
            ))?
            .into_iter()
            .map(|f| RemoteBoard {
                id: f.id,
                name: f.name,
            })
            .collect())
    }

    fn create_board(&mut self, name: &str) -> Result<String, String> {
        let root = self.root()?;
        let id = self.create_folder(name, Some(&root))?;
        self.files.insert(id.clone(), HashMap::new());
        Ok(id)
    }

    fn layout_version(&mut self, board: &str) -> Result<Option<String>, String> {
        let files = self.list(&format!(
            "{} in parents and name = {} and trashed = false",
            quoted(board),
            quoted(LAYOUT)
        ))?;
        Ok(files
            .into_iter()
            .next()
            .map(|f| f.version.unwrap_or_default()))
    }

    fn download_layout(&mut self, board: &str) -> Result<(Vec<u8>, String), String> {
        let file = self
            .list(&format!(
                "{} in parents and name = {} and trashed = false",
                quoted(board),
                quoted(LAYOUT)
            ))?
            .into_iter()
            .next()
            .ok_or("the board has no layout in Drive")?;
        let bytes = self.get(&format!("{API}/files/{}?alt=media", file.id))?;
        Ok((bytes, file.version.unwrap_or_default()))
    }

    fn upload_layout(&mut self, board: &str, json: &[u8]) -> Result<String, String> {
        let existing = self.files_in(board)?.get(LAYOUT).cloned();
        let file: File = match existing {
            Some(id) => serde_json::from_slice(&self.send(
                "PATCH",
                &format!("{UPLOAD}/files/{id}?uploadType=media&fields=id,version"),
                "application/json",
                json,
            )?)
            .map_err(|e| e.to_string())?,
            None => self.upload_new(board, LAYOUT, json)?,
        };
        Ok(file.version.unwrap_or_default())
    }

    fn pictures(&mut self, board: &str) -> Result<HashSet<String>, String> {
        // Listed fresh: another computer may have added some since.
        self.files.remove(board);
        Ok(self
            .files_in(board)?
            .keys()
            .filter(|name| name.as_str() != LAYOUT)
            .cloned()
            .collect())
    }

    fn upload_picture(&mut self, board: &str, name: &str, bytes: &[u8]) -> Result<(), String> {
        self.upload_new(board, name, bytes).map(|_| ())
    }

    fn download_picture(&mut self, board: &str, name: &str) -> Result<Vec<u8>, String> {
        let id = self
            .files_in(board)?
            .get(name)
            .cloned()
            .ok_or("a picture is missing in Drive")?;
        self.get(&format!("{API}/files/{id}?alt=media"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_names_in_searches() {
        assert_eq!(quoted("Ana's \\ board"), "'Ana\\'s \\\\ board'");
    }

    #[test]
    fn builds_an_upload_with_details_and_bytes() {
        let body = multipart(&serde_json::json!({"name": "a.png"}), b"PNG", "XYZ");
        let text = String::from_utf8(body).unwrap();
        assert!(text.starts_with("--XYZ\r\nContent-Type: application/json"));
        assert!(text.contains("{\"name\":\"a.png\"}\r\n--XYZ\r\n"));
        assert!(text.ends_with("\r\n\r\nPNG\r\n--XYZ--\r\n"));
    }
}
