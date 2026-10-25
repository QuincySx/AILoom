// 虚构项目，仅供静态阅读；不执行。
pub const DISPATCH_MODE: &str = "queue";
pub const RATE_LIMIT_RETRIES: u8 = 3;
pub const LEGACY_PAGE_SIZE: usize = 37;

pub enum CreateResult {
    Accepted,
    UnknownPendingReview,
}

pub fn on_create_timeout() -> CreateResult {
    CreateResult::UnknownPendingReview
}

// 回补与自动对账尚无实现；完整取消流程不在样例范围。
