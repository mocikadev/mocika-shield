//! Linux 进程组状态核对：有界读取 procfs，区分仍运行的后代与不可回收的僵尸。
use super::{os_error, ProcessError};
use std::{fs, io::Read};

const MAX_PROCESSES: usize = 131_072;
const MAX_STAT: u64 = 8192;

fn status(bytes: &[u8]) -> Result<(u8, i32), ProcessError> {
    // comm 可含空格、括号及非 UTF-8 字节，不能按整行字符串拆分或取第一个右括号。
    let end = bytes
        .windows(2)
        .rposition(|pair| pair == b") ")
        .ok_or(ProcessError::Unconfirmed)?;
    let mut fields = bytes[end + 2..].split(|b| b.is_ascii_whitespace());
    let state = fields
        .next()
        .filter(|v| v.len() == 1)
        .ok_or(ProcessError::Unconfirmed)?[0];
    fields.next().ok_or(ProcessError::Unconfirmed)?; // ppid
    let group = fields.next().ok_or(ProcessError::Unconfirmed)?;
    let group = std::str::from_utf8(group)
        .ok()
        .and_then(|v| v.parse().ok())
        .ok_or(ProcessError::Unconfirmed)?;
    Ok((state, group))
}

fn read_status(pid: i32) -> Result<Option<(u8, i32)>, ProcessError> {
    let file = match fs::File::open(format!("/proc/{pid}/stat")) {
        Ok(file) => file,
        Err(error) if vanished(&error) => return Ok(None),
        Err(error) => return Err(os_error(error)),
    };
    let mut bytes = Vec::new();
    match file.take(MAX_STAT + 1).read_to_end(&mut bytes) {
        Ok(_) => {}
        Err(error) if vanished(&error) => return Ok(None),
        Err(error) => return Err(os_error(error)),
    }
    if bytes.len() as u64 > MAX_STAT {
        return Err(ProcessError::Unconfirmed);
    }
    if bytes.is_empty() {
        match fs::metadata(format!("/proc/{pid}")) {
            Err(error) if vanished(&error) => return Ok(None),
            Err(error) => return Err(os_error(error)),
            _ => return Err(ProcessError::Unconfirmed),
        }
    }
    status(&bytes).map(Some)
}

fn vanished(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::NotFound || error.raw_os_error() == Some(libc::ESRCH)
}

pub(super) fn process_running(pid: i32, group: i32) -> Result<bool, ProcessError> {
    Ok(read_status(pid)?
        .is_some_and(|(state, actual)| actual == group && !matches!(state, b'Z' | b'X' | b'x')))
}

pub(super) fn group_quiescent(
    group: i32,
    deadline: std::time::Instant,
) -> Result<bool, ProcessError> {
    // 组长仍由父端 WNOWAIT 保留，不能因回收后 PID 复用误查其他任务。
    for (count, entry) in fs::read_dir("/proc").map_err(os_error)?.enumerate() {
        if count >= MAX_PROCESSES || std::time::Instant::now() >= deadline {
            return Err(ProcessError::Unconfirmed);
        }
        let entry = entry.map_err(os_error)?;
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|v| v.parse::<i32>().ok())
        else {
            continue;
        };
        if pid > 0 && pid != group && process_running(pid, group)? {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 进程名中的括号空格及非法编码不改变组号解析() {
        assert_eq!(status(b"7 (a ) b \xff) S 1 42 0\n"), Ok((b'S', 42)));
        assert_eq!(status(b"7 (zombie) Z 1 42 0\n"), Ok((b'Z', 42)));
        for invalid in [b"".as_slice(), b"7 (x) S 1", b"7 (x) S 1 bad"] {
            assert_eq!(status(invalid), Err(ProcessError::Unconfirmed));
        }
    }
}
