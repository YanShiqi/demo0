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
    shop::{self, store},
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
pub struct AdminVoucherQuery {
    notice: Option<String>,
    voucher_id: Option<String>,
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

const VOUCHER_NOTICE_REDEEMED: &str = "redeemed";
const VOUCHER_NOTICE_CANCELLED: &str = "cancelled";

#[derive(Clone, Default)]
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
    Query(query): Query<AdminVoucherQuery>,
) -> Result<Response, AppError> {
    let session = auth::require_session(&state.pool, &headers).await?;
    let actor = super::require_super_admin(&state, &session).await?;
    let result = match query.voucher_id.as_deref() {
        Some(voucher_id) => admin_voucher_by_id(&state, voucher_id).await?,
        None => None,
    };
    render_admin_vouchers(
        &state,
        &session.csrf_token,
        &actor,
        result,
        query.voucher_id.is_some(),
        query.notice.as_deref(),
    )
    .await
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
    render_admin_vouchers(&state, &session.csrf_token, &actor, result, true, None).await
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
    voucher_feedback_redirect(VOUCHER_NOTICE_REDEEMED, &voucher_id)
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
    voucher_feedback_redirect(VOUCHER_NOTICE_CANCELLED, &voucher_id)
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
    render_admin_shop_product_form(&state, &session, &actor, None, None, None, StatusCode::OK).await
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
    let _icon_lock = state.product_icon_mutation_lock.lock().await;
    let form = read_product_multipart(multipart).await?;
    auth::verify_csrf(&session, &form.csrf_token)?;
    let icon_bytes = match form.icon_bytes.clone() {
        Some(icon_bytes) => icon_bytes,
        None => {
            return render_admin_shop_product_bad_request(
                &state,
                &session,
                &actor,
                None,
                &form,
                "请选择商品图标",
            )
            .await;
        }
    };
    let processed = match process_icon(icon_bytes, &state).await {
        Ok(processed) => processed,
        Err(AppError::BadRequest(message)) => {
            return render_admin_shop_product_bad_request(
                &state, &session, &actor, None, &form, &message,
            )
            .await;
        }
        Err(error) => return Err(error),
    };
    let storage_name = generated_icon_name(processed.extension);
    let temporary_path = state
        .config
        .shop
        .icon_dir
        .join(format!(".{storage_name}.tmp"));
    let final_path = state.config.shop.icon_dir.join(&storage_name);
    let product = match form
        .clone()
        .into_product(storage_name.clone(), processed.media_type.to_owned())
    {
        Ok(product) => product,
        Err(AppError::BadRequest(message)) => {
            return render_admin_shop_product_bad_request(
                &state, &session, &actor, None, &form, &message,
            )
            .await;
        }
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
        return match error {
            AppError::BadRequest(message) => {
                render_admin_shop_product_bad_request(
                    &state, &session, &actor, None, &form, &message,
                )
                .await
            }
            error => Err(error),
        };
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
    let _icon_lock = state.product_icon_mutation_lock.lock().await;
    let form = read_product_multipart(multipart).await?;
    auth::verify_csrf(&session, &form.csrf_token)?;
    let current = store::find_product(&state.pool, &product_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let (storage_name, media_type, temporary_path, final_path) =
        if let Some(icon_bytes) = form.icon_bytes.clone() {
            let processed = match process_icon(icon_bytes, &state).await {
                Ok(processed) => processed,
                Err(AppError::BadRequest(message)) => {
                    return render_admin_shop_product_bad_request(
                        &state,
                        &session,
                        &actor,
                        Some(&current),
                        &form,
                        &message,
                    )
                    .await;
                }
                Err(error) => return Err(error),
            };
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
    let product = match form.clone().into_product(storage_name, media_type) {
        Ok(product) => product,
        Err(AppError::BadRequest(message)) => {
            if let Some(path) = temporary_path.as_ref() {
                remove_quietly(path).await;
            }
            return render_admin_shop_product_bad_request(
                &state,
                &session,
                &actor,
                Some(&current),
                &form,
                &message,
            )
            .await;
        }
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
    let old_icon_storage_name = current.icon_storage_name.clone();
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
        return match error {
            AppError::BadRequest(message) => {
                render_admin_shop_product_bad_request(
                    &state,
                    &session,
                    &actor,
                    Some(&current),
                    &form,
                    &message,
                )
                .await
            }
            error => Err(error),
        };
    }
    cleanup_icon_if_unreferenced(&state, &old_icon_storage_name).await?;
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
    let _icon_lock = state.product_icon_mutation_lock.lock().await;
    auth::verify_csrf(&session, &form.csrf_token)?;
    let deleted_icon =
        shop::delete_product(&state.pool, &actor, &product_id, OffsetDateTime::now_utc()).await?;
    cleanup_icon_if_unreferenced(&state, &deleted_icon).await?;
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

async fn cleanup_icon_if_unreferenced(
    state: &AppState,
    icon_storage_name: &str,
) -> Result<(), AppError> {
    if store::product_icon_is_referenced(&state.pool, icon_storage_name).await? {
        return Ok(());
    }
    match tokio::fs::remove_file(state.config.shop.icon_dir.join(icon_storage_name)).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            // 数据库已提交，清理失败可由后续维护任务重试；记录原因但不回滚已完成的管理操作。
            tracing::warn!(error = %error, "商品旧图标清理失败");
        }
    }
    Ok(())
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
    submitted: Option<&ProductMultipart>,
    error: Option<&str>,
    status: StatusCode,
) -> Result<Response, AppError> {
    let view = product_form_view(product.as_ref(), submitted, error.unwrap_or_default());
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

async fn render_admin_shop_product_bad_request(
    state: &AppState,
    session: &crate::model::SessionRow,
    actor: &crate::model::User,
    product: Option<&store::ProductRow>,
    submitted: &ProductMultipart,
    message: &str,
) -> Result<Response, AppError> {
    render_admin_shop_product_form(
        state,
        session,
        actor,
        product.cloned(),
        Some(submitted),
        Some(message),
        StatusCode::BAD_REQUEST,
    )
    .await
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
    submitted: Option<&ProductMultipart>,
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
    if let Some(form) = submitted {
        return (
            form.id.clone(),
            form.name.clone(),
            form.description.clone(),
            form.price.clone(),
            form.valid_days.clone(),
            form.max_active_per_user.clone(),
            form.total_limit.clone(),
            form.sort_order.clone(),
            product
                .map(|product| icon_url(&product.icon_storage_name))
                .unwrap_or_default(),
            product.is_some(),
            error.to_owned(),
        );
    }
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
    let total_items = store::count_enabled_products(&state.pool).await?;
    let total_pages = page_count(total_items, state.config.shop.page_size)?;
    let current_page = query.page.unwrap_or(1).clamp(1, total_pages);
    let offset = (current_page - 1) * state.config.shop.page_size;
    let database_products =
        store::list_enabled_products_page(&state.pool, state.config.shop.page_size, offset).await?;
    let products: Vec<ShopProductView> = database_products
        .iter()
        .map(|product| shop_product_view(product, user.as_ref(), &active_counts))
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
    match shop::purchase(
        &state.pool,
        &user,
        &product_id,
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
            let order = store::find_order_by_purchase_key(&state.pool, &form.purchase_key)
                .await?
                .ok_or(AppError::NotFound)?;
            let mut response = render(
                VoucherRevealTemplate {
                    ctx: page_context_for_user(&state, session.csrf_token, &refreshed_user).await?,
                    product_name: order.product_name,
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
    // 只接受服务端生成的 canonical ULID 文件名，避免任意路径和旧上传名成为文件探针。
    if !is_generated_icon_file_name(&file_name) {
        return Err(AppError::NotFound);
    }
    if !store::product_icon_is_referenced(&state.pool, &file_name).await? {
        return Err(AppError::NotFound);
    }
    let extension_media_type = generated_icon_media_type(&file_name).ok_or(AppError::NotFound)?;
    let media_type = match store::product_icon_media_type(&state.pool, &file_name).await? {
        Some(product_media_type) if product_media_type == extension_media_type => {
            product_media_type
        }
        Some(_) => return Err(AppError::NotFound),
        // 订单只保存购买时的文件名；canonical 扩展名是其可信媒体类型。
        None => extension_media_type.to_owned(),
    };
    let bytes = match tokio::fs::read(state.config.shop.icon_dir.join(&file_name)).await {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(AppError::NotFound);
        }
        Err(error) => return Err(error.into()),
    };
    binary_response(bytes, &media_type, "public, max-age=31536000, immutable")
}

fn is_generated_icon_file_name(file_name: &str) -> bool {
    let path = std::path::Path::new(file_name);
    path.components().count() == 1
        && path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| {
                stem.len() == 26
                    && stem.bytes().all(|character| {
                        character.is_ascii_lowercase() || character.is_ascii_digit()
                    })
                    && Ulid::from_string(stem).is_ok()
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
    product: &store::ProductRow,
    user: Option<&crate::model::User>,
    active_counts: &std::collections::HashMap<String, i64>,
) -> ShopProductView {
    let active = active_counts.get(&product.id).copied().unwrap_or_default();
    let disabled_reason = match user {
        None => "请先登录后购买".to_owned(),
        Some(_)
            if product
                .total_limit
                .is_some_and(|limit| product.sold_count >= limit) =>
        {
            "商品已售罄".to_owned()
        }
        Some(_) if active >= product.max_active_per_user => "有效兑换码持有数量已达上限".to_owned(),
        Some(user) if user.currency_balance < product.price => "余额不足".to_owned(),
        Some(_) => String::new(),
    };
    let stock_label = product
        .total_limit
        .map(|limit| {
            let remaining = (limit - product.sold_count).max(0);
            if remaining == 0 {
                "库存：已售罄".to_owned()
            } else {
                format!("库存：剩余 {remaining} 件")
            }
        })
        .unwrap_or_else(|| "库存：不限量".to_owned());
    ShopProductView {
        id: product.id.clone(),
        name: product.name.clone(),
        description: product.description.clone(),
        icon_url: icon_url(&product.icon_storage_name),
        price: product.price,
        valid_days_label: product
            .valid_days
            .map(|days| format!("购买后 {days} 天内有效"))
            .unwrap_or_else(|| "长期有效".to_owned()),
        max_active_per_user: product.max_active_per_user,
        stock_label,
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

async fn admin_voucher_by_id(
    state: &AppState,
    voucher_id: &str,
) -> Result<Option<AdminVoucherView>, AppError> {
    let voucher = store::find_voucher_with_order_by_id(&state.pool, voucher_id).await?;
    let Some(voucher) = voucher else {
        return Ok(None);
    };
    let buyer = auth::find_user_by_id(&state.pool, &voucher.order.user_id)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(Some(admin_voucher_view(
        voucher,
        buyer.nickname,
        buyer.username,
        OffsetDateTime::now_utc(),
        state.config.display.utc_offset_hours,
    )))
}

// 只允许 ULID 进入重定向地址，避免路径参数被原样拼进查询字符串。
fn voucher_feedback_redirect(notice: &str, voucher_id: &str) -> Result<Response, AppError> {
    let voucher_id = Ulid::from_string(voucher_id).map_err(|_| AppError::NotFound)?;
    let location = format!("/admin/vouchers?notice={notice}&voucher_id={voucher_id}");
    Ok(Redirect::to(&location).into_response())
}

async fn render_admin_vouchers(
    state: &AppState,
    csrf_token: &str,
    actor: &crate::model::User,
    result: Option<AdminVoucherView>,
    lookup_performed: bool,
    notice: Option<&str>,
) -> Result<Response, AppError> {
    let has_result = result.is_some();
    let action_message = match notice {
        Some(VOUCHER_NOTICE_REDEEMED) => "Token 核销成功，状态已更新为“已兑换”。",
        Some(VOUCHER_NOTICE_CANCELLED) => "Token 已取消，状态已更新为“已取消”。",
        _ => "",
    };
    let mut response = render(
        AdminVouchersTemplate {
            ctx: page_context_for_user(state, csrf_token.to_owned(), actor).await?,
            result,
            has_not_found: lookup_performed && !has_result,
            has_action_message: !action_message.is_empty(),
            action_message: action_message.to_owned(),
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

fn page_count(total_items: i64, page_size: i64) -> Result<i64, AppError> {
    if page_size < 1 {
        return Err(AppError::Internal("商城分页大小必须大于 0".to_owned()));
    }
    Ok((total_items.max(1) + page_size - 1) / page_size)
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

    #[test]
    fn generated_icon_names_require_canonical_lowercase_ulids() {
        let stem = Ulid::new().to_string().to_lowercase();
        assert!(is_generated_icon_file_name(&format!("{stem}.webp")));
        assert!(!is_generated_icon_file_name(&format!(
            "{}.webp",
            stem.to_uppercase()
        )));
        assert!(!is_generated_icon_file_name(&format!(
            "{}.webp",
            &stem[..25]
        )));
    }
}
