//! 本机任务／会话的系统随机标识；不使用时间、机器身份或可预测回退。
use std::io;

pub(crate) fn create() -> io::Result<String> {
    let mut bytes = [0u8; 16];
    #[cfg(unix)]
    {
        use io::Read;
        std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Security::Cryptography::{
            BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        };
        // 安全：固定长度的本地缓冲区存活到调用完成，系统随机源不接受外部状态。
        if unsafe {
            BCryptGenRandom(
                std::ptr::null_mut(),
                bytes.as_mut_ptr(),
                bytes.len() as u32,
                BCRYPT_USE_SYSTEM_PREFERRED_RNG,
            )
        } != 0
        {
            return Err(io::Error::other("获取系统随机标识失败"));
        }
    }
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[cfg(test)]
mod tests {
    #[test]
    fn 系统随机标识保持固定十六字节且不能重复() {
        let mut seen = std::collections::HashSet::new();
        for _ in 0..32 {
            let id = super::create().unwrap();
            assert_eq!(id.len(), 32);
            assert!(id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
            assert!(seen.insert(id));
        }
    }
}
