use axum::{
    Form,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::Response,
};
use serde::Deserialize;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use ulid::Ulid;

use crate::{
    app::AppState,
    auth,
    error::AppError,
    shop::{self, catalog, store},
    time_display,
};

use super::{
    binary_response, page_context, page_context_for_user, redirect, render, require_user,
    views::{ShopProductView, ShopTemplate, VoucherRevealTemplate, VoucherView, VouchersTemplate},
};

#[derive(Deserialize)]
pub struct PurchaseForm {
    csrf_token: String,
    purchase_key: String,
}

#[derive(Deserialize, Default)]
pub struct VoucherQuery {
    page: Option<i64>,
    purchase: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct ShopQuery {
    page: Option<i64>,
}

/// 展示可购买商品；未登录访客可以浏览，但不能提交购买。
pub async fn shop_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ShopQuery>,
) -> Result<Response, AppError> {
    ensure_shop_enabled(&state)?;
    let (session, user, ctx) = page_context(&state, &headers).await?;
    let active_counts = match &user {
        Some(user) => store::active_counts_for_user(&state.pool, &user.id, &now_string()?).await?,
        None => Default::default(),
    };
    let products: Vec<ShopProductView> = state
        .config
        .shop
        .products
        .iter()
        .filter(|product| product.enabled)
        .map(|product| shop_product_view(product, user.as_ref(), &active_counts))
        .collect();
    let total_pages = page_count(products.len(), state.config.shop.page_size)?;
    let current_page = query.page.unwrap_or(1).clamp(1, total_pages);
    let start = ((current_page - 1) * state.config.shop.page_size) as usize;
    let products: Vec<ShopProductView> = products
        .into_iter()
        .skip(start)
        .take(state.config.shop.page_size as usize)
        .collect();
    let has_products = !products.is_empty();
    render(
        ShopTemplate {
            ctx,
            products,
            has_products,
            current_page,
            total_pages,
            previous_page: current_page.saturating_sub(1).max(1),
            has_previous_page: current_page > 1,
            next_page: (current_page + 1).min(total_pages),
            has_next_page: current_page < total_pages,
        },
        StatusCode::OK,
        session.new_cookie,
    )
}

/// 用服务端商品快照完成购买，成功时仅在这个响应中返回兑换码明文。
pub async fn purchase_product(
    State(state): State<AppState>,
    Path(product_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<PurchaseForm>,
) -> Result<Response, AppError> {
    ensure_shop_enabled(&state)?;
    let session = auth::require_session(&state.pool, &headers).await?;
    auth::verify_csrf(&session, &form.csrf_token)?;
    let user = require_user(&state, &session).await?;
    let product = catalog::find_product(&state.config.shop.products, &product_id)
        .filter(|product| product.enabled)
        .ok_or(AppError::NotFound)?;

    match shop::purchase(
        &state.pool,
        &user,
        product,
        &form.purchase_key,
        &state.config.currency,
        OffsetDateTime::now_utc(),
    )
    .await?
    {
        shop::PurchaseOutcome::AlreadyProcessed => redirect("/vouchers?purchase=already", None),
        shop::PurchaseOutcome::Created(voucher) => {
            let refreshed_user = auth::find_user_by_id(&state.pool, &user.id)
                .await?
                .ok_or(AppError::NotFound)?;
            let mut response = render(
                VoucherRevealTemplate {
                    ctx: page_context_for_user(&state, session.csrf_token, &refreshed_user).await?,
                    product_name: product.name.clone(),
                    plaintext_token: voucher.plaintext_token,
                    has_expiration: voucher.expires_at.is_some(),
                    expires_at: voucher
                        .expires_at
                        .as_deref()
                        .map(|value| {
                            time_display::friendly_rfc3339(
                                value,
                                state.config.display.utc_offset_hours,
                            )
                        })
                        .unwrap_or_default(),
                },
                StatusCode::OK,
                None,
            )?;
            response
                .headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            response
                .headers_mut()
                .insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
            response.headers_mut().insert(
                header::X_CONTENT_TYPE_OPTIONS,
                HeaderValue::from_static("nosniff"),
            );
            Ok(response)
        }
    }
}

/// 列出当前用户自己的订单快照和掩码兑换码，永不读取或展示明文。
pub async fn voucher_list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<VoucherQuery>,
) -> Result<Response, AppError> {
    ensure_shop_enabled(&state)?;
    let session = auth::require_session(&state.pool, &headers).await?;
    let user = require_user(&state, &session).await?;
    let page = store::list_user_vouchers(
        &state.pool,
        &user.id,
        query.page.unwrap_or(1),
        state.config.shop.voucher_page_size,
    )
    .await?;
    let now = OffsetDateTime::now_utc();
    let vouchers: Vec<VoucherView> = page
        .items
        .into_iter()
        .map(|voucher| {
            let status_label = voucher_status_label(voucher.effective_status(now));
            VoucherView {
                product_name: voucher.order.product_name,
                product_description: voucher.order.product_description,
                icon_url: icon_url(&voucher.order.icon_file),
                token_mask: voucher.voucher.token_mask,
                status_label,
                has_expiration: voucher.voucher.expires_at.is_some(),
                expires_at: voucher
                    .voucher
                    .expires_at
                    .as_deref()
                    .map(|value| {
                        time_display::friendly_rfc3339(value, state.config.display.utc_offset_hours)
                    })
                    .unwrap_or_default(),
                created_at: time_display::friendly_rfc3339(
                    &voucher.voucher.created_at,
                    state.config.display.utc_offset_hours,
                ),
            }
        })
        .collect();
    let has_vouchers = !vouchers.is_empty();
    render(
        VouchersTemplate {
            ctx: page_context_for_user(&state, session.csrf_token, &user).await?,
            vouchers,
            has_vouchers,
            has_already_message: query.purchase.as_deref() == Some("already"),
            current_page: page.current_page,
            total_pages: page.total_pages,
            previous_page: page.current_page.saturating_sub(1).max(1),
            has_previous_page: page.current_page > 1,
            next_page: (page.current_page + 1).min(page.total_pages),
            has_next_page: page.current_page < page.total_pages,
        },
        StatusCode::OK,
        None,
    )
}

/// 安全提供商品图标；即使商城关闭，订单快照仍可引用历史图标。
pub async fn shop_product_icon(
    State(state): State<AppState>,
    Path(file_name): Path<String>,
) -> Result<Response, AppError> {
    // 拒绝时不记录请求中的文件名，避免将攻击性路径写入日志。
    if catalog::validate_icon_file_name(&file_name).is_err() {
        return Err(AppError::NotFound);
    }
    let media_type = catalog::icon_media_type(&file_name).ok_or(AppError::NotFound)?;
    let bytes = match tokio::fs::read(state.config.shop.icon_dir.join(&file_name)).await {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(AppError::NotFound);
        }
        Err(error) => return Err(error.into()),
    };
    binary_response(bytes, media_type, "public, max-age=86400")
}

fn ensure_shop_enabled(state: &AppState) -> Result<(), AppError> {
    state
        .config
        .shop
        .enabled
        .then_some(())
        .ok_or(AppError::NotFound)
}

fn shop_product_view(
    product: &catalog::ShopProduct,
    user: Option<&crate::model::User>,
    active_counts: &std::collections::HashMap<String, i64>,
) -> ShopProductView {
    let active = active_counts.get(&product.id).copied().unwrap_or_default();
    let disabled_reason = match user {
        None => "请先登录后购买".to_owned(),
        Some(_) if active >= product.max_active_per_user => "有效兑换码持有数量已达上限".to_owned(),
        Some(user) if user.currency_balance < product.price => "余额不足".to_owned(),
        Some(_) => String::new(),
    };
    ShopProductView {
        id: product.id.clone(),
        name: product.name.clone(),
        description: product.description.clone(),
        icon_url: icon_url(&product.icon_file),
        price: product.price,
        valid_days_label: product
            .valid_days
            .map(|days| format!("购买后 {days} 天内有效"))
            .unwrap_or_else(|| "长期有效".to_owned()),
        max_active_per_user: product.max_active_per_user,
        purchase_key: Ulid::new().to_string(),
        can_purchase: disabled_reason.is_empty(),
        disabled_reason,
    }
}

fn icon_url(file_name: &str) -> String {
    format!("/static/shop/products/{file_name}")
}

fn voucher_status_label(status: store::EffectiveVoucherStatus) -> &'static str {
    match status {
        store::EffectiveVoucherStatus::Active => "有效",
        store::EffectiveVoucherStatus::Redeemed => "已兑换",
        store::EffectiveVoucherStatus::Cancelled => "已取消",
        store::EffectiveVoucherStatus::Expired => "已过期",
    }
}

fn now_string() -> Result<String, AppError> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|_| AppError::Internal("无法格式化当前时间".to_owned()))
}

fn page_count(total_items: usize, page_size: i64) -> Result<i64, AppError> {
    if page_size < 1 {
        return Err(AppError::Internal("商城分页大小必须大于 0".to_owned()));
    }
    Ok(((total_items as i64).max(1) + page_size - 1) / page_size)
}
