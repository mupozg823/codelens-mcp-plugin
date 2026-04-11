use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::ChildStdout;
use std::time::Duration;

pub(super) fn language_id_for_path(path: &Path) -> Result<&'static str> {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    crate::lang_registry::language_id(&extension)
        .ok_or_else(|| anyhow::anyhow!("unsupported LSP language for extension: {extension}"))
}

pub(super) fn send_message(writer: &mut impl Write, payload: &Value) -> Result<()> {
    let body = serde_json::to_vec(payload)?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    writer.flush()?;
    Ok(())
}

pub(super) fn read_message(reader: &mut BufReader<impl Read>) -> Result<Value> {
    let mut content_length = None;
    loop {
        let mut header = String::new();
        let bytes = reader.read_line(&mut header)?;
        if bytes == 0 {
            bail!("unexpected EOF while reading LSP headers");
        }
        let trimmed = header.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':')
            && name.eq_ignore_ascii_case("Content-Length")
        {
            content_length = Some(value.trim().parse::<usize>()?);
        }
    }

    let length = content_length.context("missing Content-Length header")?;
    let mut body = vec![0_u8; length];
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body).context("failed to decode LSP body")
}

/// Wait for data on a pipe fd using poll(2). Returns true if readable, false on timeout.
#[cfg(unix)]
pub(super) fn poll_readable(stdout: &ChildStdout, timeout: Duration) -> bool {
    use std::os::unix::io::AsRawFd;
    let fd = stdout.as_raw_fd();
    let timeout_ms = timeout.as_millis().min(i32::MAX as u128) as i32;
    let mut pfd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: single pollfd, valid fd, bounded timeout
    let ret = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
    ret > 0
}

#[cfg(not(unix))]
pub(super) fn poll_readable(_stdout: &ChildStdout, _timeout: Duration) -> bool {
    true // fallback: always attempt read on non-Unix
}
