use std::net::TcpListener;

use studio_core::{Error, Result};

/// 进程内端口池。避免多个引擎实例争抢同一端口。
#[derive(Debug, Clone)]
pub struct PortPool {
    next: u16,
    end: u16,
}

impl PortPool {
    pub fn new(start: u16, end: u16) -> Self {
        Self { next: start, end }
    }

    /// 返回一个当前可监听的端口，并暂时占用（立即释放，由调用方交给引擎）。
    pub fn allocate(&mut self) -> Result<u16> {
        while self.next <= self.end {
            let port = self.next;
            self.next += 1;
            if TcpListener::bind(("127.0.0.1", port)).is_ok() {
                return Ok(port);
            }
        }
        Err(Error::Process(format!(
            "no free port in range {}-{}",
            self.next, self.end
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocates_ports_and_skips_busy() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let busy = listener.local_addr().unwrap().port();

        let mut pool = PortPool::new(busy, busy + 4);
        let port = pool.allocate().expect("allocate");
        assert_ne!(port, busy);
    }
}
