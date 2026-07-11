use std::collections::HashSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::{Duration, Instant};

pub struct GenericLspClient {
    child: Child,
    writer: std::process::ChildStdin,
    rx: Receiver<serde_json::Value>,
    next_id: usize,
    opened_uris: HashSet<String>,
}

fn reader_thread<R: Read>(mut reader: BufReader<R>, tx: Sender<serde_json::Value>) {
    loop {
        let mut line = String::new();
        let mut content_length = None;
        loop {
            line.clear();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                return;
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                break;
            }
            if trimmed.to_lowercase().starts_with("content-length:")
                && let Some(val_str) = trimmed.split(':').nth(1)
                    && let Ok(len) = val_str.trim().parse::<usize>() {
                        content_length = Some(len);
                    }
        }

        if let Some(len) = content_length {
            let mut buf = vec![0; len];
            if reader.read_exact(&mut buf).is_ok()
                && let Ok(val) = serde_json::from_slice::<serde_json::Value>(&buf)
                    && tx.send(val).is_err() {
                        return;
                    }
        }
    }
}

impl GenericLspClient {
    pub fn new(workspace_root: &Path, command: &str, args: &[&str]) -> Result<Self, String> {
        let mut child = Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Failed to spawn {}: {}", command, e))?;

        let stdin = child.stdin.take().ok_or("Failed to open stdin")?;
        let stdout = child.stdout.take().ok_or("Failed to open stdout")?;

        let (tx, rx) = channel();
        thread::spawn(move || {
            let reader = BufReader::new(stdout);
            reader_thread(reader, tx);
        });

        let mut client = GenericLspClient {
            child,
            writer: stdin,
            rx,
            next_id: 1,
            opened_uris: HashSet::new(),
        };

        let root_uri = format!(
            "file://{}",
            workspace_root
                .canonicalize()
                .unwrap_or_else(|_| workspace_root.to_path_buf())
                .display()
        );
        let init_params = serde_json::json!({
            "processId": std::process::id(),
            "rootUri": root_uri,
            "capabilities": {
                "textDocument": {
                    "definition": {
                        "dynamicRegistration": false,
                        "linkSupport": false
                    }
                }
            },
            "workspaceFolders": [
                {
                    "uri": root_uri,
                    "name": "workspace"
                }
            ]
        });

        let _init_resp = client.request("initialize", init_params, Duration::from_secs(5))?;
        client.notify("initialized", serde_json::json!({}))?;

        Ok(client)
    }

    pub fn ensure_document_open(
        &mut self,
        file_path: &Path,
        canon_path: &Path,
        language_id: &str,
    ) -> Result<(), String> {
        let uri = format!("file://{}", canon_path.display());
        if self.opened_uris.contains(&uri) {
            return Ok(());
        }
        let text = std::fs::read_to_string(file_path)
            .map_err(|e| format!("Failed to read {} for didOpen: {}", file_path.display(), e))?;
        let params = serde_json::json!({
            "textDocument": {
                "uri": uri,
                "languageId": language_id,
                "version": 1,
                "text": text
            }
        });
        self.notify("textDocument/didOpen", params)?;
        self.opened_uris.insert(uri);
        Ok(())
    }

    fn send_raw(&mut self, payload: serde_json::Value) -> Result<(), String> {
        let msg = serde_json::to_string(&payload).map_err(|e| e.to_string())?;
        let data = format!("Content-Length: {}\r\n\r\n{}", msg.len(), msg);
        self.writer
            .write_all(data.as_bytes())
            .map_err(|e| e.to_string())?;
        self.writer.flush().map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn notify(&mut self, method: &str, params: serde_json::Value) -> Result<(), String> {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        });
        self.send_raw(payload)
    }

    pub fn request(
        &mut self,
        method: &str,
        params: serde_json::Value,
        timeout: Duration,
    ) -> Result<serde_json::Value, String> {
        let id = self.next_id;
        self.next_id += 1;

        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });

        self.send_raw(payload)?;

        let start = Instant::now();
        while start.elapsed() < timeout {
            let remaining = timeout
                .checked_sub(start.elapsed())
                .unwrap_or(Duration::ZERO);
            match self.rx.recv_timeout(remaining) {
                Ok(msg) => {
                    if let Some(resp_id) = msg.get("id")
                        && resp_id.as_u64() == Some(id as u64) {
                            if let Some(error) = msg.get("error") {
                                return Err(format!("LSP error: {}", error));
                            }
                            return Ok(msg
                                .get("result")
                                .cloned()
                                .unwrap_or(serde_json::Value::Null));
                        }
                }
                Err(_) => {
                    return Err(format!("Timeout waiting for response to {}", method));
                }
            }
        }
        Err(format!("Timeout waiting for response to {}", method))
    }
}

impl Drop for GenericLspClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::sync::mpsc::channel;
    use std::thread;

    #[test]
    fn reader_thread_parses_content_length_messages() {
        let payload = r#"{"jsonrpc":"2.0","id":1,"result":{"ok":true}}"#;
        let raw = format!("Content-Length: {}\r\n\r\n{}", payload.len(), payload);
        let (tx, rx) = channel();
        thread::spawn(move || {
            let reader = BufReader::new(Cursor::new(raw.into_bytes()));
            reader_thread(reader, tx);
        });

        let msg = rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(msg.get("id").and_then(|v| v.as_u64()), Some(1));
        assert_eq!(
            msg.get("result")
                .and_then(|v| v.get("ok"))
                .and_then(|v| v.as_bool()),
            Some(true)
        );
    }
}
