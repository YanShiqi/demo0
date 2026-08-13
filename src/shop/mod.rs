pub mod catalog;
pub mod store;
pub mod token;

use sqlx::SqlitePool;
use time::{OffsetDateTime, UtcOffset, format_description::well_known::Rfc3339};
use ulid::Ulid;

use crate::{
    config::CurrencyConfig,
    currency::{self, CurrencyReason},
    error::AppError,
    model::User,
};

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
