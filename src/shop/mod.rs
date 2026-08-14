pub mod catalog;
pub mod icon;
pub mod store;
pub mod token;

use std::path::Path;

use sqlx::SqlitePool;
use time::{OffsetDateTime, UtcOffset, format_description::well_known::Rfc3339};
use ulid::Ulid;
use unicode_segmentation::UnicodeSegmentation;

use crate::{
    config::CurrencyConfig,
    currency::{self, CurrencyReason},
    error::AppError,
    model::{Role, User},
};

const MIN_PRODUCT_ID_LENGTH: usize = 1;
const MAX_PRODUCT_ID_LENGTH: usize = 64;
const MIN_PRODUCT_NAME_LENGTH: usize = 1;
const MAX_PRODUCT_NAME_LENGTH: usize = 80;
const MIN_PRODUCT_DESCRIPTION_LENGTH: usize = 1;
const MAX_PRODUCT_DESCRIPTION_LENGTH: usize = 500;

/// 校验新商品的稳定标识和值，并拒绝任何曾经出现在审计中的 ID。
pub async fn validate_product_for_create(
    pool: &SqlitePool,
    product: &store::NewProduct,
) -> Result<(), AppError> {
    validate_product_values(product, 0)?;
    if store::find_product(pool, &product.id).await?.is_some() {
        return Err(AppError::BadRequest("商品 ID 已存在".to_owned()));
    }
    if store::product_id_has_audit_history(pool, &product.id).await? {
        return Err(AppError::BadRequest(
            "商品 ID 已使用过，不可复用".to_owned(),
        ));
    }
    Ok(())
}

/// 校验编辑请求，并使用当前销量约束新的全站限售数量。
pub async fn validate_product_for_update(
    pool: &SqlitePool,
    product_id: &str,
    product: &store::NewProduct,
) -> Result<store::ProductRow, AppError> {
    let current = store::find_product(pool, product_id)
        .await?
        .ok_or(AppError::NotFound)?;
    if product.id != product_id {
        return Err(AppError::BadRequest("商品 ID 创建后不可修改".to_owned()));
    }
    validate_product_values(product, current.sold_count)?;
    Ok(current)
}

/// 供创建和编辑共享的字段校验；销量由购买事务维护，不能由表单传入。
pub fn validate_product_values(
    product: &store::NewProduct,
    sold_count: i64,
) -> Result<(), AppError> {
    let id_length = product.id.len();
    if !(MIN_PRODUCT_ID_LENGTH..=MAX_PRODUCT_ID_LENGTH).contains(&id_length)
        || !product.id.bytes().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, b'_' | b'-')
        })
    {
        return Err(AppError::BadRequest(format!(
            "商品 ID 必须为 {MIN_PRODUCT_ID_LENGTH}～{MAX_PRODUCT_ID_LENGTH} 个小写字母、数字、下划线或连字符"
        )));
    }
    validate_product_text(
        "商品名称",
        &product.name,
        MIN_PRODUCT_NAME_LENGTH,
        MAX_PRODUCT_NAME_LENGTH,
    )?;
    validate_product_text(
        "商品说明",
        &product.description,
        MIN_PRODUCT_DESCRIPTION_LENGTH,
        MAX_PRODUCT_DESCRIPTION_LENGTH,
    )?;
    if product.price <= 0 {
        return Err(AppError::BadRequest("商品价格必须大于 0".to_owned()));
    }
    if product.max_active_per_user <= 0 {
        return Err(AppError::BadRequest(
            "每位用户的有效凭证上限必须大于 0".to_owned(),
        ));
    }
    if product.valid_days.is_some_and(|days| days <= 0) {
        return Err(AppError::BadRequest("商品有效天数必须大于 0".to_owned()));
    }
    if product.total_limit.is_some_and(|limit| limit <= 0) {
        return Err(AppError::BadRequest("商品限售数量必须大于 0".to_owned()));
    }
    if product.total_limit.is_some_and(|limit| limit < sold_count) {
        return Err(AppError::BadRequest(format!(
            "商品限售数量不能低于已售数量 {sold_count}"
        )));
    }
    if product.icon_storage_name.trim().is_empty()
        || product.icon_storage_name != product.icon_storage_name.trim()
        || product.icon_storage_name.contains(['/', '\\'])
        || matches!(product.icon_storage_name.as_str(), "." | "..")
    {
        return Err(AppError::BadRequest("商品图标存储名无效".to_owned()));
    }
    // 存储名由服务端生成；仍需绑定扩展名和可信媒体类型，避免响应头与文件类型脱节。
    let expected_media_type = match Path::new(&product.icon_storage_name)
        .extension()
        .and_then(|extension| extension.to_str())
    {
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        _ => return Err(AppError::BadRequest("商品图标存储名无效".to_owned())),
    };
    if product.icon_media_type != expected_media_type {
        return Err(AppError::BadRequest("商品图标媒体类型无效".to_owned()));
    }
    Ok(())
}

fn validate_product_text(
    field: &str,
    value: &str,
    minimum: usize,
    maximum: usize,
) -> Result<(), AppError> {
    let length = value.graphemes(true).count();
    if value != value.trim()
        || value.is_empty()
        || value.chars().any(char::is_control)
        || !(minimum..=maximum).contains(&length)
    {
        return Err(AppError::BadRequest(format!(
            "{field}必须为 {minimum}～{maximum} 个可见字符，且不能包含首尾空白或控制字符"
        )));
    }
    Ok(())
}

/// 以规范化散列查找凭证，整个过程中不保留或记录兑换码明文。
pub async fn lookup_by_token(
    pool: &SqlitePool,
    raw_token: &str,
    _now: OffsetDateTime,
) -> Result<Option<store::VoucherWithOrder>, AppError> {
    let normalized = token::normalize(raw_token)?;
    store::find_voucher_by_hash(pool, &token::hash_normalized(&normalized)).await
}

/// 将有效凭证兑换一次，并在同一事务写入操作者审计记录。
pub async fn redeem_voucher(
    pool: &SqlitePool,
    voucher_id: &str,
    actor: &User,
    note: &str,
    note_max_length: usize,
    now: OffsetDateTime,
) -> Result<(), AppError> {
    transition_voucher(pool, voucher_id, actor, note, note_max_length, now, true).await
}

/// 取消仍有效的凭证，并在同一事务写入操作者审计记录。
pub async fn cancel_voucher(
    pool: &SqlitePool,
    voucher_id: &str,
    actor: &User,
    reason: &str,
    note_max_length: usize,
    now: OffsetDateTime,
) -> Result<(), AppError> {
    transition_voucher(pool, voucher_id, actor, reason, note_max_length, now, false).await
}

async fn transition_voucher(
    pool: &SqlitePool,
    voucher_id: &str,
    actor: &User,
    supplied_note: &str,
    note_max_length: usize,
    now: OffsetDateTime,
    redeem: bool,
) -> Result<(), AppError> {
    if actor.parsed_role() != Role::SuperAdmin {
        return Err(AppError::Forbidden);
    }
    let note = validate_admin_note(supplied_note, note_max_length)?;
    let now_string = format_utc(now)?;
    let mut transaction = pool.begin().await?;
    let voucher = store::find_voucher_with_order_by_id_in_transaction(&mut transaction, voucher_id)
        .await?
        .ok_or(AppError::NotFound)?;
    if voucher.effective_status(now) != store::EffectiveVoucherStatus::Active {
        return Err(AppError::BadRequest("兑换码当前不可流转".to_owned()));
    }
    let changed = if redeem {
        store::redeem_active_voucher(&mut transaction, voucher_id, &actor.id, note, &now_string)
            .await?
    } else {
        store::cancel_active_voucher(&mut transaction, voucher_id, &actor.id, note, &now_string)
            .await?
    };
    if !changed {
        return Err(AppError::BadRequest("兑换码当前不可流转".to_owned()));
    }
    store::insert_audit(
        &mut transaction,
        voucher_id,
        if redeem { "redeemed" } else { "cancelled" },
        Some(&actor.id),
        note,
        &now_string,
    )
    .await?;
    transaction.commit().await?;
    Ok(())
}

fn validate_admin_note(note: &str, max_length: usize) -> Result<&str, AppError> {
    use unicode_segmentation::UnicodeSegmentation;

    let trimmed = note.trim();
    if trimmed.is_empty() {
        return Err(AppError::BadRequest("操作备注不能为空".to_owned()));
    }
    if trimmed.graphemes(true).count() > max_length {
        return Err(AppError::BadRequest("操作备注过长".to_owned()));
    }
    Ok(trimmed)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PurchasedVoucher {
    pub order_id: String,
    pub voucher_id: String,
    pub plaintext_token: String,
    pub token_mask: String,
    pub expires_at: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PurchaseOutcome {
    Created(PurchasedVoucher),
    AlreadyProcessed,
}

/// 原子购买商品并创建兑换凭证；兑换码明文只随首次成功结果返回。
pub async fn purchase(
    pool: &SqlitePool,
    user: &User,
    product: &catalog::ShopProduct,
    purchase_key: &str,
    _currency_config: &CurrencyConfig,
    now: OffsetDateTime,
) -> Result<PurchaseOutcome, AppError> {
    let purchase_key = validate_purchase_key(purchase_key)?;
    let mut transaction = pool.begin().await?;

    // SQLite 没有行级 FOR UPDATE；此无值更新会串行化同一用户的限额检查和扣款。
    store::lock_user_for_purchase(&mut transaction, &user.id).await?;
    if let Some(existing) =
        store::find_order_by_purchase_key_in_transaction(&mut transaction, purchase_key).await?
    {
        ensure_same_request(&existing, &user.id, &product.id)?;
        transaction.commit().await?;
        return Ok(PurchaseOutcome::AlreadyProcessed);
    }

    let now_string = format_utc(now)?;
    let active = store::count_active_for_user_product_in_transaction(
        &mut transaction,
        &user.id,
        &product.id,
        &now_string,
    )
    .await?;
    ensure_active_limit(active, product.max_active_per_user)?;

    let order_id = Ulid::new().to_string();
    let voucher_id = Ulid::new().to_string();
    let issued = token::issue()?;
    let expires_at = calculate_expiration(now, product.valid_days)?;
    let order = store::NewOrder {
        id: order_id.clone(),
        user_id: user.id.clone(),
        product_id: product.id.clone(),
        product_name: product.name.clone(),
        product_description: product.description.clone(),
        icon_file: product.icon_file.clone(),
        fulfillment_type: catalog::FULFILLMENT_REDEMPTION_TOKEN.to_owned(),
        price_paid: product.price,
        valid_days: product.valid_days,
        purchase_key: purchase_key.to_owned(),
        created_at: now_string.clone(),
    };
    store::insert_order(&mut transaction, &order).await?;
    currency::spend_currency(
        &mut transaction,
        &user.id,
        product.price,
        CurrencyReason::ShopPurchase,
        Some(&order_id),
        &format!("shop-purchase:{purchase_key}"),
        &format!("购买商品：{}", product.name),
    )
    .await?;
    let voucher = store::NewVoucher {
        id: voucher_id.clone(),
        order_id: order_id.clone(),
        token_hash: issued.hash,
        token_mask: issued.mask.clone(),
        expires_at: expires_at.clone(),
        created_at: now_string.clone(),
    };
    store::insert_voucher(&mut transaction, &voucher).await?;
    store::insert_audit(
        &mut transaction,
        &voucher_id,
        "created",
        Some(&user.id),
        "",
        &now_string,
    )
    .await?;
    transaction.commit().await?;

    Ok(PurchaseOutcome::Created(PurchasedVoucher {
        order_id,
        voucher_id,
        plaintext_token: issued.plaintext,
        token_mask: issued.mask,
        expires_at,
    }))
}

fn validate_purchase_key(value: &str) -> Result<&str, AppError> {
    if value != value.trim() || value.parse::<Ulid>().is_err() {
        return Err(AppError::BadRequest("购买请求键无效".to_owned()));
    }
    Ok(value)
}

fn ensure_same_request(
    order: &store::OrderRow,
    user_id: &str,
    product_id: &str,
) -> Result<(), AppError> {
    if order.user_id == user_id && order.product_id == product_id {
        Ok(())
    } else {
        Err(AppError::BadRequest("购买请求键与原请求不匹配".to_owned()))
    }
}

fn ensure_active_limit(active: i64, maximum: i64) -> Result<(), AppError> {
    if active >= maximum {
        Err(AppError::BadRequest(
            "该商品的有效兑换码持有数量已达上限".to_owned(),
        ))
    } else {
        Ok(())
    }
}

fn calculate_expiration(
    now: OffsetDateTime,
    valid_days: Option<i64>,
) -> Result<Option<String>, AppError> {
    valid_days
        .map(|days| {
            now.checked_add(time::Duration::days(days))
                .ok_or_else(|| AppError::BadRequest("兑换码有效期超出范围".to_owned()))
                .and_then(format_utc)
        })
        .transpose()
}

fn format_utc(value: OffsetDateTime) -> Result<String, AppError> {
    value
        .to_offset(UtcOffset::UTC)
        .format(&Rfc3339)
        .map_err(|_| AppError::Internal("无法格式化兑换码时间".to_owned()))
}
