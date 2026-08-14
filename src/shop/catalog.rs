use std::path::{Component, Path};

/// 第一版商品统一兑换为兑换码，避免支持未实现的履约方式。
pub const FULFILLMENT_REDEMPTION_TOKEN: &str = "redemption_token";

/// 校验受控图标路由中的文件名只能是一个普通路径组件。
pub fn validate_icon_file_name(file_name: &str) -> Result<(), &'static str> {
    let path = Path::new(file_name);
    if !matches!(path.components().next(), Some(Component::Normal(_)))
        || path.components().count() != 1
    {
        return Err("图标文件名必须是单个普通路径组件");
    }
    if icon_media_type(file_name).is_none() {
        return Err("图标文件扩展名不受支持");
    }
    Ok(())
}

/// 返回图标响应使用的固定媒体类型。
pub fn icon_media_type(file_name: &str) -> Option<&'static str> {
    match Path::new(file_name).extension()?.to_str()? {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        _ => None,
    }
}
