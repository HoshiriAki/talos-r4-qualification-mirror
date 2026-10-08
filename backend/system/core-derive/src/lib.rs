use proc_macro::TokenStream;

/// 标记必须常数时间执行的函数
/// - release 模式：内联原始实现，零开销
/// - debug/test 模式：注入计时校验桩，检测执行时间方差
#[proc_macro_attribute]
pub fn constant_time(_attr: TokenStream, item: TokenStream) -> TokenStream {
    // 生成包装函数 — stub for now
    item
}
