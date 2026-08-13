use std::{
    collections::HashSet,
    fs,
    path::{Component, Path},
};

use anyhow::{Context, Result};
use image::{ImageFormat, ImageReader};
use unicode_segmentation::UnicodeSegmentation;

/// 第一版商品统一兑换为兑换码，避免配置文件暴露尚未支持的履约方式。
pub const FULFILLMENT_REDEMPTION_TOKEN: &str = "redemption_token";

const MIN_PRODUCT_ID_LENGTH: usize = 1;
const MAX_PRODUCT_ID_LENGTH: usize = 64;
const MIN_PRODUCT_NAME_LENGTH: usize = 1;
const MAX_PRODUCT_NAME_LENGTH: usize = 80;
const MIN_PRODUCT_DESCRIPTION_LENGTH: usize = 1;
const MAX_PRODUCT_DESCRIPTION_LENGTH: usize = 500;

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize)]
pub struct ShopProduct {
    pub id: String,
    pub name: String,
    pub description: String,
    pub icon_file: String,
    pub price: i64,
    pub valid_days: Option<i64>,
    pub max_active_per_user: i64,
    pub enabled: bool,
    pub sort_order: i64,
}

#[derive(serde::Deserialize)]
struct ProductFile {
    #[serde(default)]
    products: Vec<ShopProduct>,
}

/// 读取并验证商品目录，确保静态图标和商品元数据可安全地直接用于展示。
pub fn load_products(
    file: &Path,
    icon_dir: &Path,
    icon_max_bytes: usize,
    icon_max_dimension: u32,
) -> Result<Vec<ShopProduct>> {
    let content = fs::read_to_string(file)
        .with_context(|| format!("读取商品目录 {} 失败", file.display()))?;
    let mut products: Vec<ShopProduct> = toml::from_str::<ProductFile>(&content)
        .with_context(|| format!("商品目录 {} 不是有效的 TOML", file.display()))?
        .products;
    let mut ids = HashSet::new();

    for product in &products {
        validate_product(product, icon_dir, icon_max_bytes, icon_max_dimension)?;
        anyhow::ensure!(
            ids.insert(product.id.as_str()),
            "商品 {} 的 ID 与其他商品重复",
            product.id
        );
    }

    products.sort_by(|left, right| {
        (left.sort_order, left.id.as_str()).cmp(&(right.sort_order, right.id.as_str()))
    });
    Ok(products)
}

/// 按稳定商品 ID 查询商品，不会隐式过滤已下架商品。
pub fn find_product<'a>(products: &'a [ShopProduct], id: &str) -> Option<&'a ShopProduct> {
    products.iter().find(|product| product.id == id)
}

/// 确保图标名不包含路径信息，防止商品配置读取图标目录外的文件。
pub fn validate_icon_file_name(file_name: &str) -> Result<()> {
    let path = Path::new(file_name);
    anyhow::ensure!(
        matches!(path.components().next(), Some(Component::Normal(_)))
            && path.components().count() == 1,
        "图标文件名必须是单个普通路径组件"
    );
    anyhow::ensure!(
        icon_media_type(file_name).is_some(),
        "图标文件扩展名必须是 PNG、JPEG、GIF 或 WebP"
    );
    Ok(())
}

/// 返回浏览器响应图标时应使用的固定媒体类型。
pub fn icon_media_type(file_name: &str) -> Option<&'static str> {
    match Path::new(file_name).extension()?.to_str()? {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

fn validate_product(
    product: &ShopProduct,
    icon_dir: &Path,
    icon_max_bytes: usize,
    icon_max_dimension: u32,
) -> Result<()> {
    let id_length = product.id.len();
    anyhow::ensure!(
        (MIN_PRODUCT_ID_LENGTH..=MAX_PRODUCT_ID_LENGTH).contains(&id_length)
            && product
                .id
                .bytes()
                .all(|character| character.is_ascii_lowercase()
                    || character.is_ascii_digit()
                    || matches!(character, b'_' | b'-')),
        "商品 {} 的 ID 必须为 {}～{} 个小写 ASCII 字母、数字、下划线或连字符",
        product.id,
        MIN_PRODUCT_ID_LENGTH,
        MAX_PRODUCT_ID_LENGTH
    );
    validate_visible_text(
        &product.id,
        "名称",
        &product.name,
        MIN_PRODUCT_NAME_LENGTH,
        MAX_PRODUCT_NAME_LENGTH,
    )?;
    validate_visible_text(
        &product.id,
        "说明",
        &product.description,
        MIN_PRODUCT_DESCRIPTION_LENGTH,
        MAX_PRODUCT_DESCRIPTION_LENGTH,
    )?;
    anyhow::ensure!(product.price > 0, "商品 {} 的价格必须大于 0", product.id);
    anyhow::ensure!(
        product.max_active_per_user > 0,
        "商品 {} 的最大有效兑换码数量必须大于 0",
        product.id
    );
    if let Some(valid_days) = product.valid_days {
        anyhow::ensure!(valid_days > 0, "商品 {} 的有效天数必须大于 0", product.id);
    }
    validate_icon_file_name(&product.icon_file)
        .with_context(|| format!("商品 {} 的图标文件名无效", product.id))?;
    validate_icon_file(product, icon_dir, icon_max_bytes, icon_max_dimension)
}

fn validate_visible_text(
    id: &str,
    field: &str,
    value: &str,
    minimum: usize,
    maximum: usize,
) -> Result<()> {
    let length = value.graphemes(true).count();
    anyhow::ensure!(
        value == value.trim()
            && !value.is_empty()
            && !value.chars().any(char::is_control)
            && (minimum..=maximum).contains(&length),
        "商品 {id} 的{field}必须为 {minimum}～{maximum} 个可见字符，且不能包含首尾空白或控制字符"
    );
    Ok(())
}

fn validate_icon_file(
    product: &ShopProduct,
    icon_dir: &Path,
    icon_max_bytes: usize,
    icon_max_dimension: u32,
) -> Result<()> {
    let path = icon_dir.join(&product.icon_file);
    let bytes = fs::read(&path).with_context(|| {
        format!(
            "商品 {} 的图标 {} 不存在或无法读取",
            product.id, product.icon_file
        )
    })?;
    anyhow::ensure!(
        bytes.len() <= icon_max_bytes,
        "商品 {} 的图标 {} 超过 {} 字节限制",
        product.id,
        product.icon_file,
        icon_max_bytes
    );

    let decoded_format = image::guess_format(&bytes).with_context(|| {
        format!(
            "商品 {} 的图标 {} 不是可识别的图片",
            product.id, product.icon_file
        )
    })?;
    let expected_format = expected_image_format(&product.icon_file).expect("validated extension");
    anyhow::ensure!(
        decoded_format == expected_format,
        "商品 {} 的图标 {} 扩展名与图片格式不匹配",
        product.id,
        product.icon_file
    );
    let image = ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .context("图标格式识别失败")?
        .decode()
        .with_context(|| format!("商品 {} 的图标 {} 无法解码", product.id, product.icon_file))?;
    anyhow::ensure!(
        image.width() <= icon_max_dimension && image.height() <= icon_max_dimension,
        "商品 {} 的图标 {} 尺寸超过 {} 像素限制",
        product.id,
        product.icon_file,
        icon_max_dimension
    );
    Ok(())
}

fn expected_image_format(file_name: &str) -> Option<ImageFormat> {
    match Path::new(file_name).extension()?.to_str()? {
        "png" => Some(ImageFormat::Png),
        "jpg" | "jpeg" => Some(ImageFormat::Jpeg),
        "gif" => Some(ImageFormat::Gif),
        "webp" => Some(ImageFormat::WebP),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_and_sorts_multiple_token_products() {
        let temporary = tempfile::tempdir().unwrap();
        let icon_dir = temporary.path().join("icons");
        std::fs::create_dir_all(&icon_dir).unwrap();
        image::DynamicImage::new_rgba8(2, 2)
            .save(icon_dir.join("token.png"))
            .unwrap();
        let file = temporary.path().join("shop.toml");
        std::fs::write(
            &file,
            r#"
[[products]]
id = "second"
name = "第二件"
description = "第二件说明"
icon_file = "token.png"
price = 20
valid_days = 30
max_active_per_user = 2
enabled = true
sort_order = 20

[[products]]
id = "first"
name = "第一件"
description = "第一件说明"
icon_file = "token.png"
price = 10
max_active_per_user = 1
enabled = true
sort_order = 10
"#,
        )
        .unwrap();

        let products = load_products(&file, &icon_dir, 256 * 1024, 1024).unwrap();
        assert_eq!(
            products
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            ["first", "second"]
        );
        assert_eq!(products[0].valid_days, None);
    }

    #[test]
    fn rejects_invalid_products_and_icon_files() {
        let temporary = tempfile::tempdir().unwrap();
        let icon_dir = temporary.path().join("icons");
        std::fs::create_dir_all(&icon_dir).unwrap();
        image::DynamicImage::new_rgba8(2, 2)
            .save(icon_dir.join("token.png"))
            .unwrap();

        for (id, icon_file, price, max_active_per_user, valid_days, expected) in [
            ("unsafe", "../token.png", 10, 1, None, "unsafe"),
            ("missing", "missing.png", 10, 1, None, "missing"),
            ("oversized", "token.png", 10, 1, None, "oversized"),
            ("extension", "token.txt", 10, 1, None, "extension"),
            ("zero-price", "token.png", 0, 1, None, "zero-price"),
            ("zero-limit", "token.png", 10, 0, None, "zero-limit"),
            ("zero-days", "token.png", 10, 1, Some(0), "zero-days"),
        ] {
            let file = temporary.path().join(format!("{id}.toml"));
            std::fs::write(
                &file,
                format!(
                    "[[products]]\nid = {id:?}\nname = \"名称\"\ndescription = \"说明\"\nicon_file = {icon_file:?}\nprice = {price}\n{}max_active_per_user = {max_active_per_user}\nenabled = true\nsort_order = 1\n",
                    valid_days.map(|days| format!("valid_days = {days}\n")).unwrap_or_default(),
                ),
            )
            .unwrap();
            let max_bytes = if id == "oversized" { 1 } else { 256 * 1024 };
            let error = load_products(&file, &icon_dir, max_bytes, 1024).unwrap_err();
            assert!(error.to_string().contains(expected));
        }
    }

    #[test]
    fn rejects_duplicate_product_ids() {
        let temporary = tempfile::tempdir().unwrap();
        let icon_dir = temporary.path().join("icons");
        std::fs::create_dir_all(&icon_dir).unwrap();
        image::DynamicImage::new_rgba8(2, 2)
            .save(icon_dir.join("token.png"))
            .unwrap();
        let file = temporary.path().join("shop.toml");
        std::fs::write(
            &file,
            "[[products]]\nid = \"same\"\nname = \"名称\"\ndescription = \"说明\"\nicon_file = \"token.png\"\nprice = 1\nmax_active_per_user = 1\nenabled = true\nsort_order = 1\n\n[[products]]\nid = \"same\"\nname = \"名称\"\ndescription = \"说明\"\nicon_file = \"token.png\"\nprice = 1\nmax_active_per_user = 1\nenabled = true\nsort_order = 2\n",
        )
        .unwrap();

        let error = load_products(&file, &icon_dir, 256 * 1024, 1024).unwrap_err();
        assert!(error.to_string().contains("same"));
    }
}
