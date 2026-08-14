use std::collections::HashMap;

use sqlx::{FromRow, Sqlite, SqlitePool, Transaction};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use ulid::Ulid;

use crate::error::AppError;

pub const STATUS_ACTIVE: &str = "active";
pub const STATUS_REDEEMED: &str = "redeemed";
pub const STATUS_CANCELLED: &str = "cancelled";

pub const PRODUCT_AUDIT_CREATED: &str = "created";
pub const PRODUCT_AUDIT_UPDATED: &str = "updated";
pub const PRODUCT_AUDIT_ENABLED: &str = "enabled";
pub const PRODUCT_AUDIT_DISABLED: &str = "disabled";
pub const PRODUCT_AUDIT_DELETED: &str = "deleted";

/// 数据库商品的完整当前值；订单会在购买时保存其中必要字段的独立快照。
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct ProductRow {
    pub id: String,
    pub name: String,
    pub description: String,
    pub icon_storage_name: String,
    pub icon_media_type: String,
    pub price: i64,
    pub valid_days: Option<i64>,
    pub max_active_per_user: i64,
    pub total_limit: Option<i64>,
    pub sold_count: i64,
    pub enabled: bool,
    pub sort_order: i64,
    pub created_by_user_id: String,
    pub updated_by_user_id: String,
    pub created_at: String,
    pub updated_at: String,
}

/// 新建或编辑商品时由上层明确提供的可变字段，销量只能由购买事务修改。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewProduct {
    pub id: String,
    pub name: String,
    pub description: String,
    pub icon_storage_name: String,
    pub icon_media_type: String,
    pub price: i64,
    pub valid_days: Option<i64>,
    pub max_active_per_user: i64,
    pub total_limit: Option<i64>,
    pub sort_order: i64,
}

/// 商品管理审计记录；快照由服务层序列化为文本，表中不保存图片二进制。
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct ProductAuditRow {
    pub id: String,
    pub product_id: String,
    pub action: String,
    pub actor_user_id: String,
    pub before_snapshot: String,
    pub after_snapshot: String,
    pub created_at: String,
}

/// 待写入的商品审计值；由服务层创建 ULID 和 RFC 3339 时间后整体提交。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewProductAudit {
    pub id: String,
    pub product_id: String,
    pub action: String,
    pub actor_user_id: String,
    pub before_snapshot: String,
    pub after_snapshot: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct OrderRow {
    pub id: String,
    pub user_id: String,
    pub product_id: String,
    pub product_name: String,
    pub product_description: String,
    pub icon_file: String,
    pub fulfillment_type: String,
    pub price_paid: i64,
    pub valid_days: Option<i64>,
    pub purchase_key: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct VoucherRow {
    pub id: String,
    pub order_id: String,
    pub token_hash: String,
    pub token_mask: String,
    pub status: String,
    pub expires_at: Option<String>,
    pub redeemed_at: Option<String>,
    pub redeemed_by_user_id: Option<String>,
    pub redemption_note: String,
    pub cancelled_at: Option<String>,
    pub cancelled_by_user_id: Option<String>,
    pub cancellation_reason: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VoucherWithOrder {
    pub voucher: VoucherRow,
    pub order: OrderRow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EffectiveVoucherStatus {
    Active,
    Redeemed,
    Cancelled,
    Expired,
}

impl VoucherRow {
    /// 根据调用方提供的当前时间计算展示和流转使用的有效状态。
    pub fn effective_status(&self, now: OffsetDateTime) -> EffectiveVoucherStatus {
        match self.status.as_str() {
            STATUS_ACTIVE if self.is_expired_at(now) => EffectiveVoucherStatus::Expired,
            STATUS_ACTIVE => EffectiveVoucherStatus::Active,
            STATUS_REDEEMED => EffectiveVoucherStatus::Redeemed,
            // 约束会拒绝未知状态；异常旧数据也必须按不可用处理，不能放宽为有效。
            STATUS_CANCELLED => EffectiveVoucherStatus::Cancelled,
            _ => EffectiveVoucherStatus::Cancelled,
        }
    }

    fn is_expired_at(&self, now: OffsetDateTime) -> bool {
        self.expires_at
            .as_deref()
            .and_then(|value| OffsetDateTime::parse(value, &Rfc3339).ok())
            .is_some_and(|expires_at| expires_at <= now)
    }
}

impl VoucherWithOrder {
    /// 将订单快照与凭证状态一起判断，避免展示层重复实现有效期边界。
    pub fn effective_status(&self, now: OffsetDateTime) -> EffectiveVoucherStatus {
        self.voucher.effective_status(now)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VoucherPage {
    pub items: Vec<VoucherWithOrder>,
    pub current_page: i64,
    pub total_pages: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewOrder {
    pub id: String,
    pub user_id: String,
    pub product_id: String,
    pub product_name: String,
    pub product_description: String,
    pub icon_file: String,
    pub fulfillment_type: String,
    pub price_paid: i64,
    pub valid_days: Option<i64>,
    pub purchase_key: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewVoucher {
    pub id: String,
    pub order_id: String,
    pub token_hash: String,
    pub token_mask: String,
    pub expires_at: Option<String>,
    pub created_at: String,
}

/// 按公开商城所需的状态和排序读取已上架商品。
pub async fn list_enabled_products(pool: &SqlitePool) -> Result<Vec<ProductRow>, AppError> {
    Ok(sqlx::query_as::<_, ProductRow>(
        "SELECT id, name, description, icon_storage_name, icon_media_type, price, valid_days, max_active_per_user, total_limit, sold_count, enabled, sort_order, created_by_user_id, updated_by_user_id, created_at, updated_at FROM shop_products WHERE enabled = ? ORDER BY sort_order ASC, id ASC",
    )
    .bind(true)
    .fetch_all(pool)
    .await?)
}

/// 查询公开商品总数；分页边界由数据库真实结果决定，后台变更会立即生效。
pub async fn count_enabled_products(pool: &SqlitePool) -> Result<i64, AppError> {
    Ok(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM shop_products WHERE enabled = ?")
            .bind(true)
            .fetch_one(pool)
            .await?,
    )
}

/// 按排序和数据库分页读取已上架商品，避免公开页把整个目录加载到内存。
pub async fn list_enabled_products_page(
    pool: &SqlitePool,
    limit: i64,
    offset: i64,
) -> Result<Vec<ProductRow>, AppError> {
    Ok(sqlx::query_as::<_, ProductRow>(
        "SELECT id, name, description, icon_storage_name, icon_media_type, price, valid_days, max_active_per_user, total_limit, sold_count, enabled, sort_order, created_by_user_id, updated_by_user_id, created_at, updated_at FROM shop_products WHERE enabled = ? ORDER BY sort_order ASC, id ASC LIMIT ? OFFSET ?",
    )
    .bind(true)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?)
}

/// 管理页读取所有商品，已下架商品也保留在列表中供继续管理。
pub async fn list_admin_products(pool: &SqlitePool) -> Result<Vec<ProductRow>, AppError> {
    Ok(sqlx::query_as::<_, ProductRow>(
        "SELECT id, name, description, icon_storage_name, icon_media_type, price, valid_days, max_active_per_user, total_limit, sold_count, enabled, sort_order, created_by_user_id, updated_by_user_id, created_at, updated_at FROM shop_products ORDER BY sort_order ASC, id ASC",
    )
    .fetch_all(pool)
    .await?)
}

/// 根据稳定商品 ID 查找当前商品，不隐式过滤下架状态。
pub async fn find_product(
    pool: &SqlitePool,
    product_id: &str,
) -> Result<Option<ProductRow>, AppError> {
    Ok(sqlx::query_as::<_, ProductRow>(
        "SELECT id, name, description, icon_storage_name, icon_media_type, price, valid_days, max_active_per_user, total_limit, sold_count, enabled, sort_order, created_by_user_id, updated_by_user_id, created_at, updated_at FROM shop_products WHERE id = ?",
    )
    .bind(product_id)
    .fetch_optional(pool)
    .await?)
}

/// 在商品管理事务中读取当前商品，确保后续删除使用同一快照。
pub async fn find_product_in_transaction(
    transaction: &mut Transaction<'_, Sqlite>,
    product_id: &str,
) -> Result<Option<ProductRow>, AppError> {
    Ok(sqlx::query_as::<_, ProductRow>(
        "SELECT id, name, description, icon_storage_name, icon_media_type, price, valid_days, max_active_per_user, total_limit, sold_count, enabled, sort_order, created_by_user_id, updated_by_user_id, created_at, updated_at FROM shop_products WHERE id = ?",
    )
    .bind(product_id)
    .fetch_optional(&mut **transaction)
    .await?)
}

/// 在购买事务中按商品状态和全站余量原子增加销量；NULL 限售表示不限售。
pub async fn increment_product_sales_if_available(
    transaction: &mut Transaction<'_, Sqlite>,
    product_id: &str,
) -> Result<bool, AppError> {
    let result = sqlx::query(
        "UPDATE shop_products SET sold_count = sold_count + 1 WHERE id = ? AND enabled = ? AND (total_limit IS NULL OR sold_count < total_limit)",
    )
    .bind(product_id)
    .bind(true)
    .execute(&mut **transaction)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// 新建商品；时间由 Rust 生成后绑定，避免业务 SQL 依赖数据库时钟。
pub async fn insert_product(
    pool: &SqlitePool,
    product: &NewProduct,
    actor_user_id: &str,
    now: &str,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO shop_products (id, name, description, icon_storage_name, icon_media_type, price, valid_days, max_active_per_user, total_limit, sold_count, enabled, sort_order, created_by_user_id, updated_by_user_id, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&product.id)
    .bind(&product.name)
    .bind(&product.description)
    .bind(&product.icon_storage_name)
    .bind(&product.icon_media_type)
    .bind(product.price)
    .bind(product.valid_days)
    .bind(product.max_active_per_user)
    .bind(product.total_limit)
    .bind(0_i64)
    .bind(true)
    .bind(product.sort_order)
    .bind(actor_user_id)
    .bind(actor_user_id)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

/// 在商品管理事务中新增商品；审计记录和商品必须一起提交。
pub async fn insert_product_in_transaction(
    transaction: &mut Transaction<'_, Sqlite>,
    product: &NewProduct,
    actor_user_id: &str,
    now: &str,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO shop_products (id, name, description, icon_storage_name, icon_media_type, price, valid_days, max_active_per_user, total_limit, sold_count, enabled, sort_order, created_by_user_id, updated_by_user_id, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&product.id)
    .bind(&product.name)
    .bind(&product.description)
    .bind(&product.icon_storage_name)
    .bind(&product.icon_media_type)
    .bind(product.price)
    .bind(product.valid_days)
    .bind(product.max_active_per_user)
    .bind(product.total_limit)
    .bind(0_i64)
    .bind(true)
    .bind(product.sort_order)
    .bind(actor_user_id)
    .bind(actor_user_id)
    .bind(now)
    .bind(now)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

/// 编辑商品的可变字段；稳定 ID、销量和创建信息不允许通过此接口改写。
pub async fn update_product(
    pool: &SqlitePool,
    product_id: &str,
    product: &NewProduct,
    actor_user_id: &str,
    now: &str,
) -> Result<bool, AppError> {
    let result = sqlx::query(
        "UPDATE shop_products SET name = ?, description = ?, icon_storage_name = ?, icon_media_type = ?, price = ?, valid_days = ?, max_active_per_user = ?, total_limit = ?, sort_order = ?, updated_by_user_id = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&product.name)
    .bind(&product.description)
    .bind(&product.icon_storage_name)
    .bind(&product.icon_media_type)
    .bind(product.price)
    .bind(product.valid_days)
    .bind(product.max_active_per_user)
    .bind(product.total_limit)
    .bind(product.sort_order)
    .bind(actor_user_id)
    .bind(now)
    .bind(product_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// 在商品管理事务中编辑商品，不改写稳定 ID 和销量。
pub async fn update_product_in_transaction(
    transaction: &mut Transaction<'_, Sqlite>,
    product_id: &str,
    product: &NewProduct,
    actor_user_id: &str,
    now: &str,
) -> Result<bool, AppError> {
    let result = sqlx::query(
        "UPDATE shop_products SET name = ?, description = ?, icon_storage_name = ?, icon_media_type = ?, price = ?, valid_days = ?, max_active_per_user = ?, total_limit = ?, sort_order = ?, updated_by_user_id = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&product.name)
    .bind(&product.description)
    .bind(&product.icon_storage_name)
    .bind(&product.icon_media_type)
    .bind(product.price)
    .bind(product.valid_days)
    .bind(product.max_active_per_user)
    .bind(product.total_limit)
    .bind(product.sort_order)
    .bind(actor_user_id)
    .bind(now)
    .bind(product_id)
    .execute(&mut **transaction)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// 立即切换商品上架状态，商城下一次读取即可看到新状态。
pub async fn set_product_enabled(
    pool: &SqlitePool,
    product_id: &str,
    enabled: bool,
    actor_user_id: &str,
    now: &str,
) -> Result<bool, AppError> {
    let result = sqlx::query(
        "UPDATE shop_products SET enabled = ?, updated_by_user_id = ?, updated_at = ? WHERE id = ?",
    )
    .bind(enabled)
    .bind(actor_user_id)
    .bind(now)
    .bind(product_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// 在商品管理事务中切换上架状态并记录操作者。
pub async fn set_product_enabled_in_transaction(
    transaction: &mut Transaction<'_, Sqlite>,
    product_id: &str,
    enabled: bool,
    actor_user_id: &str,
    now: &str,
) -> Result<bool, AppError> {
    let result = sqlx::query(
        "UPDATE shop_products SET enabled = ?, updated_by_user_id = ?, updated_at = ? WHERE id = ?",
    )
    .bind(enabled)
    .bind(actor_user_id)
    .bind(now)
    .bind(product_id)
    .execute(&mut **transaction)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// 仅在没有历史订单时删除商品，订单快照仍可通过商品 ID 追溯原来的审计记录。
pub async fn delete_product(pool: &SqlitePool, product_id: &str) -> Result<bool, AppError> {
    let result = sqlx::query(
        "DELETE FROM shop_products WHERE id = ? AND NOT EXISTS (SELECT 1 FROM shop_orders WHERE product_id = ?)",
    )
    .bind(product_id)
    .bind(product_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// 在商品管理事务中删除无历史订单的商品，调用方应先写入删除审计。
pub async fn delete_product_in_transaction(
    transaction: &mut Transaction<'_, Sqlite>,
    product_id: &str,
) -> Result<bool, AppError> {
    let result = sqlx::query(
        "DELETE FROM shop_products WHERE id = ? AND NOT EXISTS (SELECT 1 FROM shop_orders WHERE product_id = ?)",
    )
    .bind(product_id)
    .bind(product_id)
    .execute(&mut **transaction)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// 判断图标是否仍被当前商品或历史订单快照引用。
pub async fn product_icon_is_referenced(
    pool: &SqlitePool,
    icon_storage_name: &str,
) -> Result<bool, AppError> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT CASE WHEN EXISTS (SELECT 1 FROM shop_products WHERE icon_storage_name = ?) OR EXISTS (SELECT 1 FROM shop_orders WHERE icon_file = ?) THEN 1 ELSE 0 END",
    )
    .bind(icon_storage_name)
    .bind(icon_storage_name)
    .fetch_one(pool)
    .await?
        != 0)
}

/// 返回当前商品对图标的可信媒体类型；历史订单只有文件快照，媒体类型由 canonical 扩展名推断。
pub async fn product_icon_media_type(
    pool: &SqlitePool,
    icon_storage_name: &str,
) -> Result<Option<String>, AppError> {
    Ok(sqlx::query_scalar::<_, String>(
        "SELECT icon_media_type FROM shop_products WHERE icon_storage_name = ? LIMIT 1",
    )
    .bind(icon_storage_name)
    .fetch_optional(pool)
    .await?)
}

/// 写入商品管理审计；商品 ID 不建外键，以保证永久删除后的历史仍可查询。
pub async fn insert_product_audit(
    pool: &SqlitePool,
    audit: &NewProductAudit,
) -> Result<(), AppError> {
    if !is_product_audit_action(&audit.action) {
        return Err(AppError::BadRequest("商品审计操作类型无效".to_owned()));
    }
    sqlx::query(
        "INSERT INTO shop_product_audit_logs (id, product_id, action, actor_user_id, before_snapshot, after_snapshot, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&audit.id)
    .bind(&audit.product_id)
    .bind(&audit.action)
    .bind(&audit.actor_user_id)
    .bind(&audit.before_snapshot)
    .bind(&audit.after_snapshot)
    .bind(&audit.created_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// 将商品审计写入现有事务，避免商品和审计出现半提交状态。
pub async fn insert_product_audit_in_transaction(
    transaction: &mut Transaction<'_, Sqlite>,
    audit: &NewProductAudit,
) -> Result<(), AppError> {
    if !is_product_audit_action(&audit.action) {
        return Err(AppError::BadRequest("商品审计操作类型无效".to_owned()));
    }
    sqlx::query(
        "INSERT INTO shop_product_audit_logs (id, product_id, action, actor_user_id, before_snapshot, after_snapshot, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&audit.id)
    .bind(&audit.product_id)
    .bind(&audit.action)
    .bind(&audit.actor_user_id)
    .bind(&audit.before_snapshot)
    .bind(&audit.after_snapshot)
    .bind(&audit.created_at)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

/// 已审计过的 ID 永远不可复用，即使对应商品随后被永久删除。
pub async fn product_id_has_audit_history(
    pool: &SqlitePool,
    product_id: &str,
) -> Result<bool, AppError> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM shop_product_audit_logs WHERE product_id = ?",
    )
    .bind(product_id)
    .fetch_one(pool)
    .await?
        > 0)
}

fn is_product_audit_action(action: &str) -> bool {
    matches!(
        action,
        PRODUCT_AUDIT_CREATED
            | PRODUCT_AUDIT_UPDATED
            | PRODUCT_AUDIT_ENABLED
            | PRODUCT_AUDIT_DISABLED
            | PRODUCT_AUDIT_DELETED
    )
}

#[derive(FromRow)]
struct VoucherWithOrderRow {
    voucher_id: String,
    voucher_order_id: String,
    token_hash: String,
    token_mask: String,
    status: String,
    expires_at: Option<String>,
    redeemed_at: Option<String>,
    redeemed_by_user_id: Option<String>,
    redemption_note: String,
    cancelled_at: Option<String>,
    cancelled_by_user_id: Option<String>,
    cancellation_reason: String,
    voucher_created_at: String,
    order_id: String,
    user_id: String,
    product_id: String,
    product_name: String,
    product_description: String,
    icon_file: String,
    fulfillment_type: String,
    price_paid: i64,
    valid_days: Option<i64>,
    purchase_key: String,
    order_created_at: String,
}

impl From<VoucherWithOrderRow> for VoucherWithOrder {
    fn from(row: VoucherWithOrderRow) -> Self {
        Self {
            voucher: VoucherRow {
                id: row.voucher_id,
                order_id: row.voucher_order_id,
                token_hash: row.token_hash,
                token_mask: row.token_mask,
                status: row.status,
                expires_at: row.expires_at,
                redeemed_at: row.redeemed_at,
                redeemed_by_user_id: row.redeemed_by_user_id,
                redemption_note: row.redemption_note,
                cancelled_at: row.cancelled_at,
                cancelled_by_user_id: row.cancelled_by_user_id,
                cancellation_reason: row.cancellation_reason,
                created_at: row.voucher_created_at,
            },
            order: OrderRow {
                id: row.order_id,
                user_id: row.user_id,
                product_id: row.product_id,
                product_name: row.product_name,
                product_description: row.product_description,
                icon_file: row.icon_file,
                fulfillment_type: row.fulfillment_type,
                price_paid: row.price_paid,
                valid_days: row.valid_days,
                purchase_key: row.purchase_key,
                created_at: row.order_created_at,
            },
        }
    }
}

#[derive(FromRow)]
struct ActiveProductCount {
    product_id: String,
    active_count: i64,
}

pub async fn find_order_by_purchase_key(
    pool: &SqlitePool,
    purchase_key: &str,
) -> Result<Option<OrderRow>, AppError> {
    Ok(sqlx::query_as::<_, OrderRow>(
        "SELECT id, user_id, product_id, product_name, product_description, icon_file, fulfillment_type, price_paid, valid_days, purchase_key, created_at FROM shop_orders WHERE purchase_key = ?",
    )
    .bind(purchase_key)
    .fetch_optional(pool)
    .await?)
}

/// 在购买事务中按幂等键查找订单，避免读写之间出现重复创建窗口。
pub async fn find_order_by_purchase_key_in_transaction(
    transaction: &mut Transaction<'_, Sqlite>,
    purchase_key: &str,
) -> Result<Option<OrderRow>, AppError> {
    Ok(sqlx::query_as::<_, OrderRow>(
        "SELECT id, user_id, product_id, product_name, product_description, icon_file, fulfillment_type, price_paid, valid_days, purchase_key, created_at FROM shop_orders WHERE purchase_key = ?",
    )
    .bind(purchase_key)
    .fetch_optional(&mut **transaction)
    .await?)
}

pub async fn count_active_for_user_product(
    pool: &SqlitePool,
    user_id: &str,
    product_id: &str,
    now: &str,
) -> Result<i64, AppError> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM shop_orders JOIN redemption_vouchers ON redemption_vouchers.order_id = shop_orders.id WHERE shop_orders.user_id = ? AND shop_orders.product_id = ? AND redemption_vouchers.status = ? AND (redemption_vouchers.expires_at IS NULL OR redemption_vouchers.expires_at > ?)",
    )
    .bind(user_id)
    .bind(product_id)
    .bind(STATUS_ACTIVE)
    .bind(now)
    .fetch_one(pool)
    .await?)
}

/// 统计购买事务内仍有效的同商品凭证，过期边界与展示状态保持一致。
pub async fn count_active_for_user_product_in_transaction(
    transaction: &mut Transaction<'_, Sqlite>,
    user_id: &str,
    product_id: &str,
    now: &str,
) -> Result<i64, AppError> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM shop_orders JOIN redemption_vouchers ON redemption_vouchers.order_id = shop_orders.id WHERE shop_orders.user_id = ? AND shop_orders.product_id = ? AND redemption_vouchers.status = ? AND (redemption_vouchers.expires_at IS NULL OR redemption_vouchers.expires_at > ?)",
    )
    .bind(user_id)
    .bind(product_id)
    .bind(STATUS_ACTIVE)
    .bind(now)
    .fetch_one(&mut **transaction)
    .await?)
}

/// 通过无值更新取得 SQLite 写锁，使同一用户的购买检查和扣款串行执行。
pub async fn lock_user_for_purchase(
    transaction: &mut Transaction<'_, Sqlite>,
    user_id: &str,
) -> Result<(), AppError> {
    sqlx::query("UPDATE users SET updated_at = updated_at WHERE id = ?")
        .bind(user_id)
        .execute(&mut **transaction)
        .await?
        .rows_affected()
        .eq(&1)
        .then_some(())
        .ok_or(AppError::NotFound)
}

pub async fn active_counts_for_user(
    pool: &SqlitePool,
    user_id: &str,
    now: &str,
) -> Result<HashMap<String, i64>, AppError> {
    let counts = sqlx::query_as::<_, ActiveProductCount>(
        "SELECT shop_orders.product_id AS product_id, COUNT(*) AS active_count FROM shop_orders JOIN redemption_vouchers ON redemption_vouchers.order_id = shop_orders.id WHERE shop_orders.user_id = ? AND redemption_vouchers.status = ? AND (redemption_vouchers.expires_at IS NULL OR redemption_vouchers.expires_at > ?) GROUP BY shop_orders.product_id",
    )
    .bind(user_id)
    .bind(STATUS_ACTIVE)
    .bind(now)
    .fetch_all(pool)
    .await?;
    Ok(counts
        .into_iter()
        .map(|count| (count.product_id, count.active_count))
        .collect())
}

pub async fn insert_order(
    transaction: &mut Transaction<'_, Sqlite>,
    order: &NewOrder,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO shop_orders (id, user_id, product_id, product_name, product_description, icon_file, fulfillment_type, price_paid, valid_days, purchase_key, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&order.id)
    .bind(&order.user_id)
    .bind(&order.product_id)
    .bind(&order.product_name)
    .bind(&order.product_description)
    .bind(&order.icon_file)
    .bind(&order.fulfillment_type)
    .bind(order.price_paid)
    .bind(order.valid_days)
    .bind(&order.purchase_key)
    .bind(&order.created_at)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

pub async fn insert_voucher(
    transaction: &mut Transaction<'_, Sqlite>,
    voucher: &NewVoucher,
) -> Result<(), AppError> {
    // 凭证明文只存在于购买响应，此处仅写入散列和可展示的掩码。
    sqlx::query(
        "INSERT INTO redemption_vouchers (id, order_id, token_hash, token_mask, status, expires_at, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&voucher.id)
    .bind(&voucher.order_id)
    .bind(&voucher.token_hash)
    .bind(&voucher.token_mask)
    .bind(STATUS_ACTIVE)
    .bind(&voucher.expires_at)
    .bind(&voucher.created_at)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

pub async fn insert_audit(
    transaction: &mut Transaction<'_, Sqlite>,
    voucher_id: &str,
    event_type: &str,
    actor_user_id: Option<&str>,
    note: &str,
    created_at: &str,
) -> Result<(), AppError> {
    // 审计记录独立生成稳定 ID，令订单、凭证和审计可在同一事务原子提交。
    sqlx::query(
        "INSERT INTO voucher_audit_logs (id, voucher_id, event_type, actor_user_id, note, created_at) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(Ulid::new().to_string())
    .bind(voucher_id)
    .bind(event_type)
    .bind(actor_user_id)
    .bind(note)
    .bind(created_at)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

pub async fn list_user_vouchers(
    pool: &SqlitePool,
    user_id: &str,
    page: i64,
    page_size: i64,
) -> Result<VoucherPage, AppError> {
    let total_items = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM shop_orders JOIN redemption_vouchers ON redemption_vouchers.order_id = shop_orders.id WHERE shop_orders.user_id = ?",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;
    let bounds = page_bounds(total_items, page, page_size)?;
    let rows = sqlx::query_as::<_, VoucherWithOrderRow>(
        "SELECT redemption_vouchers.id AS voucher_id, redemption_vouchers.order_id AS voucher_order_id, redemption_vouchers.token_hash, redemption_vouchers.token_mask, redemption_vouchers.status, redemption_vouchers.expires_at, redemption_vouchers.redeemed_at, redemption_vouchers.redeemed_by_user_id, redemption_vouchers.redemption_note, redemption_vouchers.cancelled_at, redemption_vouchers.cancelled_by_user_id, redemption_vouchers.cancellation_reason, redemption_vouchers.created_at AS voucher_created_at, shop_orders.id AS order_id, shop_orders.user_id, shop_orders.product_id, shop_orders.product_name, shop_orders.product_description, shop_orders.icon_file, shop_orders.fulfillment_type, shop_orders.price_paid, shop_orders.valid_days, shop_orders.purchase_key, shop_orders.created_at AS order_created_at FROM redemption_vouchers JOIN shop_orders ON shop_orders.id = redemption_vouchers.order_id WHERE shop_orders.user_id = ? ORDER BY redemption_vouchers.created_at DESC, redemption_vouchers.id DESC LIMIT ? OFFSET ?",
    )
    .bind(user_id)
    .bind(page_size)
    .bind(bounds.offset)
    .fetch_all(pool)
    .await?;
    Ok(VoucherPage {
        items: rows.into_iter().map(Into::into).collect(),
        current_page: bounds.current_page,
        total_pages: bounds.total_pages,
    })
}

pub async fn find_voucher_by_hash(
    pool: &SqlitePool,
    token_hash: &str,
) -> Result<Option<VoucherWithOrder>, AppError> {
    find_voucher_with_order(pool, "redemption_vouchers.token_hash = ?", token_hash).await
}

pub async fn find_voucher_with_order_by_id(
    pool: &SqlitePool,
    voucher_id: &str,
) -> Result<Option<VoucherWithOrder>, AppError> {
    find_voucher_with_order(pool, "redemption_vouchers.id = ?", voucher_id).await
}

/// 在状态流转事务中读取凭证和订单快照，避免读写落在不同事务。
pub async fn find_voucher_with_order_by_id_in_transaction(
    transaction: &mut Transaction<'_, Sqlite>,
    voucher_id: &str,
) -> Result<Option<VoucherWithOrder>, AppError> {
    Ok(sqlx::query_as::<_, VoucherWithOrderRow>(
        "SELECT redemption_vouchers.id AS voucher_id, redemption_vouchers.order_id AS voucher_order_id, redemption_vouchers.token_hash, redemption_vouchers.token_mask, redemption_vouchers.status, redemption_vouchers.expires_at, redemption_vouchers.redeemed_at, redemption_vouchers.redeemed_by_user_id, redemption_vouchers.redemption_note, redemption_vouchers.cancelled_at, redemption_vouchers.cancelled_by_user_id, redemption_vouchers.cancellation_reason, redemption_vouchers.created_at AS voucher_created_at, shop_orders.id AS order_id, shop_orders.user_id, shop_orders.product_id, shop_orders.product_name, shop_orders.product_description, shop_orders.icon_file, shop_orders.fulfillment_type, shop_orders.price_paid, shop_orders.valid_days, shop_orders.purchase_key, shop_orders.created_at AS order_created_at FROM redemption_vouchers JOIN shop_orders ON shop_orders.id = redemption_vouchers.order_id WHERE redemption_vouchers.id = ?",
    )
    .bind(voucher_id)
    .fetch_optional(&mut **transaction)
    .await?
    .map(Into::into))
}

/// 仅在仍有效的 active 状态下完成兑换，受影响行数是并发竞争的最终裁决。
pub async fn redeem_active_voucher(
    transaction: &mut Transaction<'_, Sqlite>,
    voucher_id: &str,
    actor_user_id: &str,
    note: &str,
    now: &str,
) -> Result<bool, AppError> {
    let result = sqlx::query(
        "UPDATE redemption_vouchers SET status = ?, redeemed_at = ?, redeemed_by_user_id = ?, redemption_note = ? WHERE id = ? AND status = ? AND (expires_at IS NULL OR expires_at > ?)",
    )
    .bind(STATUS_REDEEMED)
    .bind(now)
    .bind(actor_user_id)
    .bind(note)
    .bind(voucher_id)
    .bind(STATUS_ACTIVE)
    .bind(now)
    .execute(&mut **transaction)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// 取消与兑换使用同一有效期条件，过期凭证不会被改写为已取消。
pub async fn cancel_active_voucher(
    transaction: &mut Transaction<'_, Sqlite>,
    voucher_id: &str,
    actor_user_id: &str,
    reason: &str,
    now: &str,
) -> Result<bool, AppError> {
    let result = sqlx::query(
        "UPDATE redemption_vouchers SET status = ?, cancelled_at = ?, cancelled_by_user_id = ?, cancellation_reason = ? WHERE id = ? AND status = ? AND (expires_at IS NULL OR expires_at > ?)",
    )
    .bind(STATUS_CANCELLED)
    .bind(now)
    .bind(actor_user_id)
    .bind(reason)
    .bind(voucher_id)
    .bind(STATUS_ACTIVE)
    .bind(now)
    .execute(&mut **transaction)
    .await?;
    Ok(result.rows_affected() == 1)
}

async fn find_voucher_with_order(
    pool: &SqlitePool,
    condition: &str,
    value: &str,
) -> Result<Option<VoucherWithOrder>, AppError> {
    // 条件仅由两个内部常量提供，外部参数始终通过绑定传入。
    let query = format!(
        "SELECT redemption_vouchers.id AS voucher_id, redemption_vouchers.order_id AS voucher_order_id, redemption_vouchers.token_hash, redemption_vouchers.token_mask, redemption_vouchers.status, redemption_vouchers.expires_at, redemption_vouchers.redeemed_at, redemption_vouchers.redeemed_by_user_id, redemption_vouchers.redemption_note, redemption_vouchers.cancelled_at, redemption_vouchers.cancelled_by_user_id, redemption_vouchers.cancellation_reason, redemption_vouchers.created_at AS voucher_created_at, shop_orders.id AS order_id, shop_orders.user_id, shop_orders.product_id, shop_orders.product_name, shop_orders.product_description, shop_orders.icon_file, shop_orders.fulfillment_type, shop_orders.price_paid, shop_orders.valid_days, shop_orders.purchase_key, shop_orders.created_at AS order_created_at FROM redemption_vouchers JOIN shop_orders ON shop_orders.id = redemption_vouchers.order_id WHERE {condition}"
    );
    Ok(sqlx::query_as::<_, VoucherWithOrderRow>(&query)
        .bind(value)
        .fetch_optional(pool)
        .await?
        .map(Into::into))
}

#[derive(Clone, Copy, Debug)]
struct PageBounds {
    current_page: i64,
    total_pages: i64,
    offset: i64,
}

fn page_bounds(total_items: i64, page: i64, page_size: i64) -> Result<PageBounds, AppError> {
    if page < 1 {
        return Err(AppError::BadRequest("页码必须大于 0".to_owned()));
    }
    if page_size < 1 {
        return Err(AppError::Internal("分页大小必须大于 0".to_owned()));
    }
    // 空列表保留第一页，页面调用者不需要为“无结果”另行分支。
    let total_pages = if total_items == 0 {
        1
    } else {
        (total_items - 1) / page_size + 1
    };
    let current_page = page.min(total_pages);
    let offset = (current_page - 1)
        .checked_mul(page_size)
        .ok_or_else(|| AppError::BadRequest("页码过大".to_owned()))?;
    Ok(PageBounds {
        current_page,
        total_pages,
        offset,
    })
}
