#[cfg(unix)]
pub(crate) fn terminate(pid: i32) {
    const SIGTERM: i32 = 15;
    // SAFETY: kill(2) 只接受 pid 与信号常量，无内存安全风险。
    unsafe {
        kill(pid, SIGTERM);
    }
}

#[cfg(not(unix))]
pub(crate) fn terminate(_pid: i32) {}

#[cfg(unix)]
unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}
