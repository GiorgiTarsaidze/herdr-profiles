use std::fmt;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};

const TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Api { code: String, message: String },
    Protocol(String),
}

impl Error {
    pub fn code(&self) -> Option<&str> {
        match self {
            Self::Api { code, .. } => Some(code),
            _ => None,
        }
    }

    pub fn is_transient(&self) -> bool {
        matches!(self, Self::Io(_)) || self.code() == Some("ui_busy")
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "herdr socket: {err}"),
            Self::Api { code, message } => write!(f, "{code}: {message}"),
            Self::Protocol(detail) => write!(f, "unexpected reply from herdr: {detail}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

#[derive(Deserialize)]
struct Reply {
    result: Option<Value>,
    error: Option<ApiError>,
}

#[derive(Deserialize)]
struct ApiError {
    code: String,
    message: String,
}

/// One newline-delimited JSON request over Herdr's socket API.
pub fn call(socket: &Path, method: &str, params: Value) -> Result<Value, Error> {
    let mut stream = UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.set_write_timeout(Some(TIMEOUT))?;
    let request = json!({ "id": "profiles", "method": method, "params": params });
    stream.write_all(request.to_string().as_bytes())?;
    stream.write_all(b"\n")?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    let reply: Reply =
        serde_json::from_str(&line).map_err(|err| Error::Protocol(err.to_string()))?;
    if let Some(ApiError { code, message }) = reply.error {
        return Err(Error::Api { code, message });
    }
    Ok(reply.result.unwrap_or(Value::Null))
}

#[cfg(test)]
pub mod testing {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;
    use std::path::Path;
    use std::thread;

    use serde_json::Value;

    /// Serves exactly one request on `socket`, replying with `handler`'s value.
    pub fn serve_once(socket: &Path, handler: impl FnOnce(Value) -> Value + Send + 'static) {
        let listener = UnixListener::bind(socket).unwrap();
        thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(&stream);
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            let mut writer = &stream;
            writeln!(writer, "{}", handler(request)).unwrap();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn returns_result_and_maps_errors() {
        let dir = tempfile::tempdir().unwrap();
        let ok = dir.path().join("ok.sock");
        testing::serve_once(
            &ok,
            |req| json!({ "id": req["id"], "result": { "echo": req["method"] } }),
        );
        assert_eq!(call(&ok, "ping", json!({})).unwrap()["echo"], "ping");

        let busy = dir.path().join("busy.sock");
        testing::serve_once(
            &busy,
            |_| json!({ "id": "", "error": { "code": "ui_busy", "message": "busy" } }),
        );
        let err = call(&busy, "plugin.pane.open", json!({})).unwrap_err();
        assert_eq!(err.code(), Some("ui_busy"));
        assert!(err.is_transient());

        let missing = call(&dir.path().join("none.sock"), "ping", json!({})).unwrap_err();
        assert!(matches!(missing, Error::Io(_)));
        assert!(missing.is_transient());
    }
}
