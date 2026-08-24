//! 把分配器囤着的空闲页还给系统。
//!
//! 一次全量摄取要解析成千上万条 jsonl，产生的 String 和 `serde_json::Value` 都是小对象。
//! 释放之后分配器不会主动把这些页还给内核，它们仍然计入 phys_footprint,
//! 活动监视器上就是几百 MB 常驻不掉——实测 16 MB 存活对象背后挂着 326 MB 空闲页。
//!
//! macOS 自带的 libmalloc 没有可用的归还入口：`malloc_zone_pressure_relief` 在
//! macOS 26 上对所有 zone 都返回 0，footprint 分毫不动（实测 409 MB 施压后仍是 409 MB）。
//! 所以换 mimalloc 做全局分配器，用它的 `mi_collect` 强制回收——同样的负载下
//! 867 MB 能压回 3.5 MB。
//!
//! 摄取是低频操作，跑完主动收一次即可。代价是下次分配要重新向内核要页，
//! 相比扫盘和 SQLite 事务可以忽略。

/// mimalloc 只在自己的堆上生效，因此 [`release_idle`] 也只对它管理的内存有意义。
#[global_allocator]
static ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

extern "C" {
    /// mimalloc 的强制回收。`force = true` 表示连线程本地缓存一起收，
    /// 而不是只处理已经完全空闲的 segment。
    fn mi_collect(force: bool);
}

/// 归还空闲页。
pub fn release_idle() {
    // SAFETY: mi_collect 只整理分配器自己的空闲 segment，不触碰存活分配。
    // mimalloc 是本 crate 的全局分配器，符号一定存在。
    unsafe {
        mi_collect(true);
    }
}
