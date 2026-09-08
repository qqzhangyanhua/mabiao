//! 工作纪要测试用的连接来源：整个 harness 只有一条内存连接。
//!
//! 读写都走 `try_lock`，于是 `ConnectionSource` 那条「不得同时持有读与写 guard」的契约一旦被
//! 违反就当场报错，而不是把测试挂住。顺带记下取写连接的次数，供「没东西可写就不取写连接」
//! 那条测试断言。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

use rusqlite::Connection;

use crate::work_notes::ConnectionSource;

const HELD: &str = "测试连接已被占用：读与写 guard 不得同时持有";

pub struct TestConnection {
    conn: Mutex<Connection>,
    write_locks: AtomicUsize,
}

impl TestConnection {
    pub fn new(conn: Connection) -> Self {
        Self {
            conn: Mutex::new(conn),
            write_locks: AtomicUsize::new(0),
        }
    }

    /// 测试自己播数据、查断言用。
    pub fn get(&self) -> MutexGuard<'_, Connection> {
        self.conn.try_lock().expect(HELD)
    }

    pub fn write_locks(&self) -> usize {
        self.write_locks.load(Ordering::SeqCst)
    }
}

impl ConnectionSource for TestConnection {
    fn read(&self) -> Result<MutexGuard<'_, Connection>, String> {
        self.conn.try_lock().map_err(|_| format!("{HELD}（读）"))
    }

    fn write(&self) -> Result<MutexGuard<'_, Connection>, String> {
        self.write_locks.fetch_add(1, Ordering::SeqCst);
        self.conn.try_lock().map_err(|_| format!("{HELD}（写）"))
    }
}
