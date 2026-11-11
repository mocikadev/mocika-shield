//! Windows 本次进程及后代的真实回归；不执行 Java、用户 APK 或任何全局清场命令。
use super::*;
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
};
use windows_sys::Win32::{
    Foundation::WAIT_OBJECT_0,
    System::{
        JobObjects::{JobObjectBasicProcessIdList, JOBOBJECT_BASIC_PROCESS_ID_LIST},
        Threading::{
            OpenProcess, QueryFullProcessImageNameW, WaitForSingleObject,
            PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
        },
    },
};

const ENTRY: &str = "automation::windows_job::tests::内部进程夹具";
struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn command(role: &str, root: &Path) -> Command {
    let mut cmd = shield_core::utils::no_window_command(std::env::current_exe().unwrap());
    cmd.args([
        "--ignored",
        "--exact",
        ENTRY,
        "--nocapture",
        "--test-threads=1",
    ])
    .env("MOCIKA_JOB_TEST_ROLE", role)
    .env("MOCIKA_JOB_TEST_ROOT", root)
    .env_remove("MOCIKA_SHIELD_KS_PASS")
    .env_remove("MOCIKA_SHIELD_KEY_PASS")
    .env_remove("MOCIKA_SHIELD_TOOL_KS_PASS")
    .env_remove("MOCIKA_SHIELD_TOOL_KEY_PASS")
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null());
    cmd
}

fn wait_for(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready() {
        assert!(Instant::now() < deadline, "Windows 原生夹具未在期限内就绪");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn running(root: &Path) -> bool {
    root.join("heartbeat").metadata().is_ok_and(|m| m.len() > 0)
}

fn observe(root: &Path, name: &str) -> OwnedHandle {
    let mut pid = None;
    wait_for(|| {
        pid = fs::read_to_string(root.join(name))
            .ok()
            .and_then(|value| value.parse::<u32>().ok());
        pid.is_some()
    });
    open_observed(pid.unwrap())
}

fn open_observed(pid: u32) -> OwnedHandle {
    // 仅查询、等待本夹具记录或本次 Job 返回的进程；没有按 PID 终止的权限。
    let raw = unsafe {
        OpenProcess(
            PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
            0,
            pid,
        )
    };
    assert!(!raw.is_null(), "无法取得本次夹具的等待句柄");
    unsafe { OwnedHandle::from_raw_handle(raw) }
}

fn members(job: &Job) -> Vec<OwnedHandle> {
    const LIMIT: usize = 64;
    type Info = JOBOBJECT_BASIC_PROCESS_ID_LIST;
    // 容量和字段位置来自 SDK；对齐的初始化存储覆盖可变数组，不另写结构布局。
    assert!(std::mem::align_of::<usize>() >= std::mem::align_of::<Info>());
    let offset = std::mem::offset_of!(Info, ProcessIdList);
    let mut buffer = vec![0usize; size_of::<Info>().div_ceil(size_of::<usize>()) + LIMIT - 1];
    let bytes = (buffer.len() * size_of::<usize>()) as u32;
    let mut written = 0;
    let ok = unsafe {
        QueryInformationJobObject(
            job.handle.as_raw_handle(),
            JobObjectBasicProcessIdList,
            buffer.as_mut_ptr().cast(),
            bytes,
            &mut written,
        )
    };
    assert_ne!(
        ok,
        0,
        "本次 Job 成员查询失败：{}",
        io::Error::last_os_error()
    );
    assert!((size_of::<Info>()..=bytes as usize).contains(&(written as usize)));
    // 安全：查询成功且缓冲区覆盖 SDK 头部，全部整数字段均允许零值初始化。
    let info = unsafe { &*buffer.as_ptr().cast::<Info>() };
    let count = info.NumberOfProcessIdsInList as usize;
    assert!((2..=LIMIT).contains(&count));
    assert_eq!(info.NumberOfAssignedProcesses as usize, count);
    assert!(offset + count * size_of::<usize>() <= written as usize);
    let ids = unsafe { std::slice::from_raw_parts(buffer.as_ptr().byte_add(offset), count) };
    let mut names = Vec::new();
    let handles = ids
        .iter()
        .map(|&id| {
            let handle = open_observed(u32::try_from(id).unwrap());
            assert!(job.contains(&handle).unwrap());
            let mut image = [0u16; 1024];
            let mut length = image.len() as u32;
            let ok = unsafe {
                QueryFullProcessImageNameW(
                    handle.as_raw_handle(),
                    0,
                    image.as_mut_ptr(),
                    &mut length,
                )
            };
            assert_ne!(ok, 0, "本次 Job 成员名称查询失败");
            assert!((length as usize) < image.len());
            let image = String::from_utf16(&image[..length as usize]).unwrap();
            names.push(
                Path::new(&image)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            );
            handle
        })
        .collect();
    println!("本次 Job 完整成员：{names:?}；逐个等待停止，不按固定成员数判断成功");
    handles
}

fn stopped(process: &impl AsRawHandle) {
    // 句柄固定内核对象，不受 PID 复用影响；等待值不能用心跳短时不增长替代。
    assert_eq!(
        unsafe { WaitForSingleObject(process.as_raw_handle(), 2000) },
        WAIT_OBJECT_0
    );
}

fn started(root: &Path) -> (Job, OwnedChild, OwnedHandle) {
    let job = Job::new().unwrap();
    assert_eq!(job.active().unwrap(), 0);
    let mut child = OwnedChild(
        command("leader", root)
            .stdin(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    job.assign(&child.0).unwrap();
    assert!(job.contains(&child.0).unwrap());
    assert!(!running(root));
    child.0.stdin.take().unwrap().write_all(b"S").unwrap();
    let writer = observe(root, "writer-pid");
    wait_for(|| running(root));
    assert!(job.contains(&writer).unwrap());
    // 系统可能产生辅助成员，不能假设 Job 只有两个进程；收尾仍须完整为零。
    assert!(job.active().unwrap() >= 2);
    (job, child, writer)
}

#[test]
fn 归属确认后整组停止且不影响无关进程() {
    let mut root = tempfile::tempdir().unwrap();
    root.disable_cleanup(true);
    let outside = tempfile::tempdir().unwrap();
    let outsider = OwnedChild(command("writer", outside.path()).spawn().unwrap());
    wait_for(|| running(outside.path()));
    let (job, child, writer) = started(root.path());
    let members = members(&job);
    assert!(!job.contains(&outsider.0).unwrap());
    job.stop(Instant::now() + Duration::from_secs(2)).unwrap();
    assert_eq!(job.active().unwrap(), 0);
    stopped(&child.0);
    stopped(&writer);
    for member in &members {
        stopped(member);
    }
    let before = outside.path().join("heartbeat").metadata().unwrap().len();
    wait_for(|| outside.path().join("heartbeat").metadata().unwrap().len() > before);
    job.stop(Instant::now() + Duration::from_secs(2)).unwrap();
    root.disable_cleanup(false);
}

#[test]
fn 关闭唯一工作组句柄会停止本次进程及后代() {
    let mut root = tempfile::tempdir().unwrap();
    root.disable_cleanup(true);
    let (job, child, writer) = started(root.path());
    let members = members(&job);
    drop(job);
    stopped(&child.0);
    stopped(&writer);
    for member in &members {
        stopped(member);
    }
    root.disable_cleanup(false);
}

#[test]
fn 父进程异常退出也不会因后代继承句柄而延命() {
    let mut root = tempfile::tempdir().unwrap();
    root.disable_cleanup(true);
    let mut owner = OwnedChild(command("owner", root.path()).spawn().unwrap());
    wait_for(|| root.path().join("owner-ready").exists());
    let leader = observe(root.path(), "leader-pid");
    let writer = observe(root.path(), "writer-pid");
    owner.0.kill().unwrap();
    owner.0.wait().unwrap();
    stopped(&leader);
    stopped(&writer);
    root.disable_cleanup(false);
}

#[test]
#[ignore = "仅由 Windows Job 原生测试启动的内部夹具，不作为独立验收项"]
fn 内部进程夹具() {
    let Ok(role) = std::env::var("MOCIKA_JOB_TEST_ROLE") else {
        return;
    };
    let root = std::path::PathBuf::from(std::env::var_os("MOCIKA_JOB_TEST_ROOT").unwrap());
    match role.as_str() {
        "writer" => {
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(root.join("heartbeat"))
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(30);
            while Instant::now() < deadline {
                file.write_all(b".").unwrap();
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        "leader" => {
            let mut start = [0; 1];
            std::io::stdin().read_exact(&mut start).unwrap();
            assert_eq!(&start, b"S");
            let writer = OwnedChild(command("writer", &root).spawn().unwrap());
            fs::write(root.join("writer-pid"), writer.0.id().to_string()).unwrap();
            std::thread::sleep(Duration::from_secs(30));
        }
        "owner" => {
            let (_job, leader, _writer) = started(&root);
            fs::write(root.join("leader-pid"), leader.0.id().to_string()).unwrap();
            fs::write(root.join("owner-ready"), b"yes").unwrap();
            std::thread::sleep(Duration::from_secs(30));
        }
        _ => panic!("未知的 Windows 进程夹具角色"),
    }
}
