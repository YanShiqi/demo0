use axum::{
    Form,
    extract::{Multipart, Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
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
    CsrfForm, binary_response, page_context, page_context_for_user, redirect, render, require_user,
    views::{
        AdminShopProductFormTemplate, AdminShopProductView, AdminShopProductsTemplate,
        AdminVoucherView, AdminVouchersTemplate, ShopProductView, ShopTemplate,
        VoucherRevealTemplate, VoucherView, VouchersTemplate,
    },
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

#[derive(Deserialize)]
pub struct VoucherLookupForm {
    csrf_token: String,
    token: String,
}

#[derive(Deserialize)]
pub struct VoucherRedeemForm {
    csrf_token: String,
    note: String,
}

#[derive(Deserialize)]
pub struct VoucherCancelForm {
    csrf_token: String,
    reason: String,
}

#[derive(Default)]
struct ProductMultipart {
    csrf_token: String,
    id: String,
    name: String,
    description: String,
    price: String,
    valid_days: String,
    max_active_per_user: String,
    total_limit: String,
    sort_order: String,
    icon_bytes: Option<Vec<u8>>,
}

impl ProductMultipart {
    fn into_product(
        self,
        icon_storage_name: String,
        icon_media_type: String,
    ) -> Result<store::NewProduct, AppError> {
        Ok(store::NewProduct {
            id: self.id.trim().to_owned(),
            name: self.name.trim().to_owned(),
            description: self.description.trim().to_owned(),
            icon_storage_name,
            icon_media_type,
            price: parse_product_integer("价格", &self.price)?,
            valid_days: parse_optional_product_integer("有效天数", &self.valid_days)?,
            max_active_per_user: parse_product_integer("每位用户上限", &self.max_active_per_user)?,
            total_limit: parse_optional_product_integer("全站限售数量", &self.total_limit)?,
            sort_order: parse_product_integer("排序", &self.sort_order)?,
        })
    }
}

/// 超级管理员兑换码管理页；页面不接受或保留明文兑换码。
pub async fn admin_vouchers(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let session = auth::require_session(&state.pool, &headers).await?;
    let actor = super::require_super_admin(&state, &session).await?;
    render_admin_vouchers(&state, &session.csrf_token, &actor, None, false).await
}

/// 使用一次性查询字段定位凭证，随后仅通过凭证 ID 操作。
pub async fn lookup_voucher(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<VoucherLookupForm>,
) -> Result<Response, AppError> {
    let session = auth::require_session(&state.pool, &headers).await?;
    let actor = super::require_super_admin(&state, &session).await?;
    auth::verify_csrf(&session, &form.csrf_token)?;
    if !state
        .voucher_lookup_limiter
        .check_and_record(&actor.id)
        .await
    {
        return Err(AppError::TooManyRequests(
            "Token 查询过于频繁，请稍后再试".to_owned(),
        ));
    }
    let voucher =
        shop::lookup_by_token(&state.pool, &form.token, OffsetDateTime::now_utc()).await?;
    let result = match voucher {
        Some(voucher) => {
            let buyer = auth::find_user_by_id(&state.pool, &voucher.order.user_id)
                .await?
                .ok_or(AppError::NotFound)?;
            Some(admin_voucher_view(
                voucher,
                buyer.nickname,
                buyer.username,
                OffsetDateTime::now_utc(),
                state.config.display.utc_offset_hours,
            ))
        }
        None => None,
    };
    render_admin_vouchers(&state, &session.csrf_token, &actor, result, true).await
}

pub async fn redeem_voucher(
    State(state): State<AppState>,
    Path(voucher_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<VoucherRedeemForm>,
) -> Result<Response, AppError> {
    let session = auth::require_session(&state.pool, &headers).await?;
    let actor = super::require_super_admin(&state, &session).await?;
    auth::verify_csrf(&session, &form.csrf_token)?;
    shop::redeem_voucher(
        &state.pool,
        &voucher_id,
        &actor,
        &form.note,
        state.config.shop.admin_note_max_length,
        OffsetDateTime::now_utc(),
    )
    .await?;
    Ok(Redirect::to("/admin/vouchers").into_response())
}

pub async fn cancel_voucher(
    State(state): State<AppState>,
    Path(voucher_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<VoucherCancelForm>,
) -> Result<Response, AppError> {
    let session = auth::require_session(&state.pool, &headers).await?;
    let actor = super::require_super_admin(&state, &session).await?;
    auth::verify_csrf(&session, &form.csrf_token)?;
    shop::cancel_voucher(
        &state.pool,
        &voucher_id,
        &actor,
        &form.reason,
        state.config.shop.admin_note_max_length,
        OffsetDateTime::now_utc(),
    )
    .await?;
    Ok(Redirect::to("/admin/vouchers").into_response())
}

/// 超级管理员商品列表；普通管理员也必须被拒绝，避免目录元数据泄露。
pub async fn admin_shop_products(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let session = auth::require_session(&state.pool, &headers).await?;
    let actor = super::require_super_admin(&state, &session).await?;
    render_admin_shop_products(&state, &session, &actor).await
}

/// 展示空白商品表单，商品 ID 仅在创建时可填写。
pub async fn admin_shop_product_new(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let session = auth::require_session(&state.pool, &headers).await?;
    let actor = super::require_super_admin(&state, &session).await?;
    render_admin_shop_product_form(&state, &session, &actor, None, None, StatusCode::OK).await
}

/// 展示现有商品的编辑表单；不返回图标二进制，只返回可缓存预览 URL。
pub async fn admin_shop_product_edit(
    State(state): State<AppState>,
    Path(product_id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let session = auth::require_session(&state.pool, &headers).await?;
    let actor = super::require_super_admin(&state, &session).await?;
    let product = store::find_product(&state.pool, &product_id)
        .await?
        .ok_or(AppError::NotFound)?;
    render_admin_shop_product_form(
        &state,
        &session,
        &actor,
        Some(product),
        None,
        StatusCode::OK,
    )
    .await
}

/// 创建商品。图片先在 blocking 线程处理并写入临时服务端文件，数据库失败时始终清理临时文件。
pub async fn create_admin_shop_product(
    State(state): State<AppState>,
    headers: HeaderMap,
    multipart: Multipart,
) -> Result<Response, AppError> {
    let session = auth::require_session(&state.pool, &headers).await?;
    let actor = super::require_super_admin(&state, &session).await?;
    let form = read_product_multipart(multipart).await?;
    auth::verify_csrf(&session, &form.csrf_token)?;
    let icon_bytes = form
        .icon_bytes
        .clone()
        .ok_or_else(|| AppError::BadRequest("请选择商品图标".to_owned()))?;
    let processed = process_icon(icon_bytes, &state).await?;
    let storage_name = generated_icon_name(processed.extension);
    let temporary_path = state
        .config
        .shop
        .icon_dir
        .join(format!(".{storage_name}.tmp"));
    let final_path = state.config.shop.icon_dir.join(&storage_name);
    let product = match form.into_product(storage_name.clone(), processed.media_type.to_owned()) {
        Ok(product) => product,
        Err(error) => return Err(error),
    };
    write_temporary_icon(
        &temporary_path,
        &processed.bytes,
        &state.config.shop.icon_dir,
    )
    .await?;
    if let Err(error) = tokio::fs::rename(&temporary_path, &final_path).await {
        remove_quietly(&temporary_path).await;
        return Err(error.into());
    }
    if let Err(error) =
        shop::create_product(&state.pool, &actor, &product, OffsetDateTime::now_utc()).await
    {
        remove_quietly(&temporary_path).await;
        remove_quietly(&final_path).await;
        return Err(error);
    }
    redirect("/admin/shop/products", None)
}

/// 编辑商品；未上传新图标时沿用旧图标，上传新图标则以新文件名破除浏览器缓存。
pub async fn update_admin_shop_product(
    State(state): State<AppState>,
    Path(product_id): Path<String>,
    headers: HeaderMap,
    multipart: Multipart,
) -> Result<Response, AppError> {
    let session = auth::require_session(&state.pool, &headers).await?;
    let actor = super::require_super_admin(&state, &session).await?;
    let form = read_product_multipart(multipart).await?;
    auth::verify_csrf(&session, &form.csrf_token)?;
    let current = store::find_product(&state.pool, &product_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let (storage_name, media_type, temporary_path, final_path) =
        if let Some(icon_bytes) = form.icon_bytes.clone() {
            let processed = process_icon(icon_bytes, &state).await?;
            let storage_name = generated_icon_name(processed.extension);
            let temporary_path = state
                .config
                .shop
                .icon_dir
                .join(format!(".{storage_name}.tmp"));
            let final_path = state.config.shop.icon_dir.join(&storage_name);
            write_temporary_icon(
                &temporary_path,
                &processed.bytes,
                &state.config.shop.icon_dir,
            )
            .await?;
            (
                storage_name,
                processed.media_type.to_owned(),
                Some(temporary_path),
                Some(final_path),
            )
        } else {
            (
                current.icon_storage_name.clone(),
                current.icon_media_type.clone(),
                None,
                None,
            )
        };
    let product = match form.into_product(storage_name, media_type) {
        Ok(product) => product,
        Err(error) => {
            if let Some(path) = temporary_path.as_ref() {
                remove_quietly(path).await;
            }
            return Err(error);
        }
    };
    if let Some(temporary_path) = temporary_path.as_ref() {
        let final_path = final_path.as_ref().expect("new icon has final path");
        if let Err(error) = tokio::fs::rename(temporary_path, final_path).await {
            remove_quietly(temporary_path).await;
            return Err(error.into());
        }
    }
    if let Err(error) = shop::update_product(
        &state.pool,
        &actor,
        &product_id,
        &product,
        OffsetDateTime::now_utc(),
    )
    .await
    {
        if let Some(path) = temporary_path.as_ref() {
            remove_quietly(path).await;
        }
        if let Some(path) = final_path.as_ref() {
            remove_quietly(path).await;
        }
        return Err(error);
    }
    // 旧图标可能仍被历史订单快照引用，管理操作不主动删除它。
    redirect("/admin/shop/products", None)
}

pub async fn enable_admin_shop_product(
    State(state): State<AppState>,
    Path(product_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<CsrfForm>,
) -> Result<Response, AppError> {
    mutate_product_enabled(&state, &product_id, &headers, form, true).await
}

pub async fn disable_admin_shop_product(
    State(state): State<AppState>,
    Path(product_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<CsrfForm>,
) -> Result<Response, AppError> {
    mutate_product_enabled(&state, &product_id, &headers, form, false).await
}

pub async fn delete_admin_shop_product(
    State(state): State<AppState>,
    Path(product_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<CsrfForm>,
) -> Result<Response, AppError> {
    let session = auth::require_session(&state.pool, &headers).await?;
    let actor = super::require_super_admin(&state, &session).await?;
    auth::verify_csrf(&session, &form.csrf_token)?;
    let current = store::find_product(&state.pool, &product_id)
        .await?
        .ok_or(AppError::NotFound)?;
    shop::delete_product(&state.pool, &actor, &product_id, OffsetDateTime::now_utc()).await?;
    remove_quietly(&state.config.shop.icon_dir.join(current.icon_storage_name)).await;
    redirect("/admin/shop/products", None)
}

async fn read_product_multipart(mut multipart: Multipart) -> Result<ProductMultipart, AppError> {
    let mut form = ProductMultipart::default();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| AppError::BadRequest("无法读取商品表单".to_owned()))?
    {
        let field_name = field.name().unwrap_or_default().to_owned();
        match field_name.as_str() {
            "csrf_token" => form.csrf_token = field_text(field, "CSRF 字段无效").await?,
            "id" => form.id = field_text(field, "商品 ID 字段无效").await?,
            "name" => form.name = field_text(field, "商品名称字段无效").await?,
            "description" => form.description = field_text(field, "商品说明字段无效").await?,
            "price" => form.price = field_text(field, "商品价格字段无效").await?,
            "valid_days" => form.valid_days = field_text(field, "有效天数字段无效").await?,
            "max_active_per_user" => {
                form.max_active_per_user = field_text(field, "用户上限字段无效").await?
            }
            "total_limit" => form.total_limit = field_text(field, "限售数字段无效").await?,
            "sort_order" => form.sort_order = field_text(field, "排序字段无效").await?,
            "icon" => {
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|_| AppError::BadRequest("商品图标读取失败".to_owned()))?;
                if !bytes.is_empty() {
                    form.icon_bytes = Some(bytes.to_vec());
                }
            }
            _ => {}
        }
    }
    Ok(form)
}

async fn field_text(
    field: axum::extract::multipart::Field<'_>,
    message: &str,
) -> Result<String, AppError> {
    field
        .text()
        .await
        .map_err(|_| AppError::BadRequest(message.to_owned()))
}

async fn process_icon(
    bytes: Vec<u8>,
    state: &AppState,
) -> Result<shop::icon::ProcessedIcon, AppError> {
    let config = state.config.shop.clone();
    tokio::task::spawn_blocking(move || shop::icon::IconProcessor::process(bytes, &config))
        .await
        .map_err(|error| AppError::Internal(format!("商品图标处理任务异常：{error}")))?
}

fn generated_icon_name(extension: &str) -> String {
    format!("{}.{}", Ulid::new().to_string().to_lowercase(), extension)
}

async fn write_temporary_icon(
    temporary_path: &std::path::Path,
    bytes: &[u8],
    icon_dir: &std::path::Path,
) -> Result<(), AppError> {
    tokio::fs::create_dir_all(icon_dir).await?;
    if let Err(error) = tokio::fs::write(temporary_path, bytes).await {
        remove_quietly(temporary_path).await;
        return Err(error.into());
    }
    Ok(())
}

async fn remove_quietly(path: &std::path::Path) {
    let _ = tokio::fs::remove_file(path).await;
}

fn parse_product_integer(field: &str, value: &str) -> Result<i64, AppError> {
    value
        .trim()
        .parse::<i64>()
        .map_err(|_| AppError::BadRequest(format!("{field}必须是整数")))
}

fn parse_optional_product_integer(field: &str, value: &str) -> Result<Option<i64>, AppError> {
    let value = value.trim();
    if value.is_empty() {
        Ok(None)
    } else {
        parse_product_integer(field, value).map(Some)
    }
}

async fn render_admin_shop_products(
    state: &AppState,
    session: &crate::model::SessionRow,
    actor: &crate::model::User,
) -> Result<Response, AppError> {
    let products = store::list_admin_products(&state.pool)
        .await?
        .into_iter()
        .map(admin_shop_product_view)
        .collect::<Vec<_>>();
    render(
        AdminShopProductsTemplate {
            ctx: page_context_for_user(state, session.csrf_token.clone(), actor).await?,
            has_products: !products.is_empty(),
            products,
            message: String::new(),
            has_message: false,
        },
        StatusCode::OK,
        None,
    )
}

async fn render_admin_shop_product_form(
    state: &AppState,
    session: &crate::model::SessionRow,
    actor: &crate::model::User,
    product: Option<store::ProductRow>,
    error: Option<&str>,
    status: StatusCode,
) -> Result<Response, AppError> {
    let view = product_form_view(product.as_ref(), error.unwrap_or_default());
    render(
        AdminShopProductFormTemplate {
            ctx: page_context_for_user(state, session.csrf_token.clone(), actor).await?,
            is_edit: product.is_some(),
            product_id: view.0,
            name: view.1,
            description: view.2,
            price: view.3,
            valid_days: view.4,
            max_active_per_user: view.5,
            total_limit: view.6,
            sort_order: view.7,
            icon_url: view.8,
            has_icon: view.9,
            error: view.10,
            has_error: error.is_some(),
        },
        status,
        None,
    )
}

fn admin_shop_product_view(product: store::ProductRow) -> AdminShopProductView {
    AdminShopProductView {
        id: product.id,
        name: product.name,
        description: product.description,
        icon_url: icon_url(&product.icon_storage_name),
        price: product.price,
        valid_days_label: product
            .valid_days
            .map(|days| format!("{days} 天"))
            .unwrap_or_else(|| "长期有效".to_owned()),
        max_active_per_user: product.max_active_per_user,
        total_limit_label: product
            .total_limit
            .map(|limit| format!("{limit}（已售 {}）", product.sold_count))
            .unwrap_or_else(|| format!("不限量（已售 {}）", product.sold_count)),
        sold_count: product.sold_count,
        sort_order: product.sort_order,
        enabled: product.enabled,
        can_delete: product.sold_count == 0,
    }
}

#[allow(clippy::type_complexity)]
fn product_form_view(
    product: Option<&store::ProductRow>,
    error: &str,
) -> (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    bool,
    String,
) {
    match product {
        Some(product) => (
            product.id.clone(),
            product.name.clone(),
            product.description.clone(),
            product.price.to_string(),
            product
                .valid_days
                .map(|value| value.to_string())
                .unwrap_or_default(),
            product.max_active_per_user.to_string(),
            product
                .total_limit
                .map(|value| value.to_string())
                .unwrap_or_default(),
            product.sort_order.to_string(),
            icon_url(&product.icon_storage_name),
            true,
            error.to_owned(),
        ),
        None => (
            String::new(),
            String::new(),
            String::new(),
            "1".to_owned(),
            String::new(),
            "1".to_owned(),
            String::new(),
            "1".to_owned(),
            String::new(),
            false,
            error.to_owned(),
        ),
    }
}

async fn mutate_product_enabled(
    state: &AppState,
    product_id: &str,
    headers: &HeaderMap,
    form: CsrfForm,
    enabled: bool,
) -> Result<Response, AppError> {
    let session = auth::require_session(&state.pool, headers).await?;
    let actor = super::require_super_admin(state, &session).await?;
    auth::verify_csrf(&session, &form.csrf_token)?;
    shop::set_product_enabled(
        &state.pool,
        &actor,
        product_id,
        enabled,
        OffsetDateTime::now_utc(),
    )
    .await?;
    redirect("/admin/shop/products", None)
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
    if catalog::validate_icon_file_name(&file_name).is_err()
        && !is_generated_icon_file_name(&file_name)
    {
        return Err(AppError::NotFound);
    }
    let media_type = catalog::icon_media_type(&file_name)
        .or_else(|| generated_icon_media_type(&file_name))
        .ok_or(AppError::NotFound)?;
    let bytes = match tokio::fs::read(state.config.shop.icon_dir.join(&file_name)).await {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(AppError::NotFound);
        }
        Err(error) => return Err(error.into()),
    };
    binary_response(bytes, media_type, "public, max-age=86400")
}

fn is_generated_icon_file_name(file_name: &str) -> bool {
    let path = std::path::Path::new(file_name);
    path.components().count() == 1
        && path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| {
                !stem.is_empty()
                    && stem.chars().all(|character| {
                        character.is_ascii_lowercase()
                            || character.is_ascii_digit()
                            || character == '-'
                    })
            })
        && generated_icon_media_type(file_name).is_some()
}

fn generated_icon_media_type(file_name: &str) -> Option<&'static str> {
    match std::path::Path::new(file_name).extension()?.to_str()? {
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        _ => None,
    }
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

fn admin_voucher_view(
    voucher: store::VoucherWithOrder,
    buyer_nickname: String,
    buyer_username: String,
    now: OffsetDateTime,
    utc_offset_hours: i8,
) -> AdminVoucherView {
    let status_label = voucher_status_label(voucher.effective_status(now));
    AdminVoucherView {
        id: voucher.voucher.id,
        product_name: voucher.order.product_name,
        product_description: voucher.order.product_description,
        token_mask: voucher.voucher.token_mask,
        status_label,
        has_expiration: voucher.voucher.expires_at.is_some(),
        expires_at: voucher
            .voucher
            .expires_at
            .as_deref()
            .map(|value| time_display::friendly_rfc3339(value, utc_offset_hours))
            .unwrap_or_default(),
        buyer_nickname,
        buyer_username,
    }
}

async fn render_admin_vouchers(
    state: &AppState,
    csrf_token: &str,
    actor: &crate::model::User,
    result: Option<AdminVoucherView>,
    lookup_performed: bool,
) -> Result<Response, AppError> {
    let has_result = result.is_some();
    let mut response = render(
        AdminVouchersTemplate {
            ctx: page_context_for_user(state, csrf_token.to_owned(), actor).await?,
            result,
            has_not_found: lookup_performed && !has_result,
            note_max_length: state.config.shop.admin_note_max_length,
        },
        StatusCode::OK,
        None,
    )?;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_form_parses_blank_optional_limits() {
        assert_eq!(
            parse_optional_product_integer("有效天数", "").unwrap(),
            None
        );
        assert_eq!(parse_product_integer("价格", "42").unwrap(), 42);
        assert!(parse_product_integer("价格", "not-a-number").is_err());
    }
}
