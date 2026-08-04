mod views;

use std::{net::SocketAddr, str::FromStr};

use askama::Template;
use axum::{
    Form,
    body::Body,
    extract::{ConnectInfo, Multipart, Path, Query, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::Next,
    response::{Html, IntoResponse, Redirect, Response},
};
use serde::Deserialize;
use ulid::Ulid;

use crate::{
    app::AppState,
    auth,
    avatar::{self, DEFAULT_AVATAR},
    error::AppError,
    memes::{self, MemeRow, MemeWithTags, NewMeme},
    model::{PageContext, Role, SessionContext, SessionRow, User},
    public_messages::{self, PublicMessageRow},
    time_display,
};
use views::{
    AdminMemesTemplate, AdminUserView, AdminUsersTemplate, HomeTemplate, LoginTemplate, MemeView,
    MemesTemplate, MessageView, MessagesTemplate, NewMemeTemplate, PasswordChangeRequiredTemplate,
    PopularTagView, ProfileTemplate, PublicProfileTemplate, RegisterTemplate,
};

#[derive(Deserialize)]
pub struct RegisterForm {
    csrf_token: String,
    username: String,
    nickname: String,
    password: String,
    password_confirmation: String,
}

#[derive(Deserialize)]
pub struct LoginForm {
    csrf_token: String,
    username: String,
    password: String,
}

#[derive(Deserialize)]
pub struct CsrfForm {
    csrf_token: String,
}

#[derive(Deserialize)]
pub struct PasswordChangeForm {
    csrf_token: String,
    password: String,
    password_confirmation: String,
}

#[derive(Deserialize)]
pub struct DeleteMessageForm {
    csrf_token: String,
    return_to: Option<String>,
}

#[derive(Deserialize)]
pub struct DeleteMemeForm {
    csrf_token: String,
    return_to: Option<String>,
}

#[derive(Deserialize)]
pub struct MessageForm {
    csrf_token: String,
    body: String,
}

#[derive(Deserialize, Default)]
pub struct MemeQuery {
    tag: Option<String>,
    page: Option<i64>,
}

#[derive(Deserialize, Default)]
pub struct AdminMemeQuery {
    status: Option<String>,
    q: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct AdminUsersQuery {
    password_reset: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct HomeQuery {
    tab: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HomeTab {
    Messages,
    Memes,
}

impl HomeTab {
    fn from_query(tab: Option<&str>) -> Self {
        match tab.map(str::trim) {
            Some(HOME_TAB_MEMES) => Self::Memes,
            _ => Self::Messages,
        }
    }
}

const HOME_TAB_MEMES: &str = "memes";

#[derive(Deserialize)]
pub struct NicknameForm {
    csrf_token: String,
    nickname: String,
}

#[derive(Deserialize)]
pub struct BioForm {
    csrf_token: String,
    bio: String,
}

#[derive(Deserialize)]
pub struct RoleForm {
    csrf_token: String,
    role: String,
}

#[derive(Deserialize, Default)]
pub struct ProfileQuery {
    updated: Option<String>,
}

pub async fn home(
    State(state): State<AppState>,
    Query(query): Query<HomeQuery>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let (session, _, ctx) = page_context(&state, &headers).await?;
    let home_tab = HomeTab::from_query(query.tab.as_deref());
    let messages: Vec<MessageView> = public_messages::list_recent_limited(
        &state.pool,
        &state.config.messages,
        state.config.messages.home_preview_limit,
    )
    .await?
    .into_iter()
    .map(|message| home_message_view(message, state.config.display.utc_offset_hours))
    .collect();
    let has_messages = !messages.is_empty();
    let memes: Vec<MemeView> = memes::list_approved(&state.pool, None, 1, &state.config.memes)
        .await?
        .items
        .into_iter()
        .take(state.config.memes.home_preview_limit as usize)
        .map(|meme| meme_view(meme, state.config.display.utc_offset_hours))
        .collect();
    let has_memes = !memes.is_empty();
    render(
        HomeTemplate {
            ctx,
            messages,
            has_messages,
            message_preview_limit: state.config.messages.home_preview_limit,
            memes,
            has_memes,
            meme_preview_limit: state.config.memes.home_preview_limit,
            home_messages_tab_active: home_tab == HomeTab::Messages,
            home_memes_tab_active: home_tab == HomeTab::Memes,
        },
        StatusCode::OK,
        session.new_cookie,
    )
}

pub async fn register_page(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let (session, user, ctx) = page_context(&state, &headers).await?;
    if user.is_some() {
        return redirect("/profile", session.new_cookie);
    }
    render(
        RegisterTemplate {
            ctx,
            has_error: false,
            error: String::new(),
            username: String::new(),
            nickname: String::new(),
        },
        StatusCode::OK,
        session.new_cookie,
    )
}

pub async fn register(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<RegisterForm>,
) -> Result<Response, AppError> {
    let session = auth::require_session(&state.pool, &headers).await?;
    auth::verify_csrf(&session, &form.csrf_token)?;

    if let Err(AppError::BadRequest(message)) =
        auth::validate_password_confirmation(&form.password, &form.password_confirmation)
    {
        return render(
            RegisterTemplate {
                ctx: PageContext::anonymous(session.csrf_token),
                has_error: true,
                error: message,
                username: form.username,
                nickname: form.nickname,
            },
            StatusCode::BAD_REQUEST,
            None,
        );
    }

    match auth::create_user(
        &state.pool,
        &form.username,
        &form.nickname,
        &form.password,
        Role::User,
    )
    .await
    {
        Ok(user) => {
            let cookie = auth::sign_in(&state.pool, &state.config, &session, &user.id).await?;
            redirect("/profile", Some(cookie))
        }
        Err(AppError::BadRequest(message)) => render(
            RegisterTemplate {
                ctx: PageContext::anonymous(session.csrf_token),
                has_error: true,
                error: message,
                username: form.username,
                nickname: form.nickname,
            },
            StatusCode::BAD_REQUEST,
            None,
        ),
        Err(error) => Err(error),
    }
}

pub async fn login_page(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let (session, user, ctx) = page_context(&state, &headers).await?;
    if user.is_some() {
        return redirect("/profile", session.new_cookie);
    }
    render(
        LoginTemplate {
            ctx,
            has_error: false,
            error: String::new(),
            username: String::new(),
        },
        StatusCode::OK,
        session.new_cookie,
    )
}

pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectInfo(address): ConnectInfo<SocketAddr>,
    Form(form): Form<LoginForm>,
) -> Result<Response, AppError> {
    let session = auth::require_session(&state.pool, &headers).await?;
    auth::verify_csrf(&session, &form.csrf_token)?;
    let client = address.ip().to_string();
    let login_key = format!("{}:{}", client, form.username.trim().to_lowercase());
    if !state.login_limiter.is_allowed(&login_key).await {
        return render(
            LoginTemplate {
                ctx: PageContext::anonymous(session.csrf_token),
                has_error: true,
                error: "登录尝试过于频繁，请十分钟后再试".to_owned(),
                username: form.username,
            },
            StatusCode::TOO_MANY_REQUESTS,
            None,
        );
    }
    let user = auth::authenticate(&state.pool, &form.username, &form.password).await?;
    let Some(user) = user else {
        state.login_limiter.record_failure(&login_key).await;
        return render(
            LoginTemplate {
                ctx: PageContext::anonymous(session.csrf_token),
                has_error: true,
                error: "用户名或密码错误".to_owned(),
                username: form.username,
            },
            StatusCode::UNAUTHORIZED,
            None,
        );
    };
    state.login_limiter.clear(&login_key).await;
    let cookie = auth::sign_in(&state.pool, &state.config, &session, &user.id).await?;
    if user.must_change_password {
        return redirect("/password/change-required", Some(cookie));
    }
    redirect("/profile", Some(cookie))
}

pub async fn change_password_required_page(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let session = auth::require_session(&state.pool, &headers).await?;
    let user = require_user(&state, &session).await?;
    if !user.must_change_password {
        return redirect("/profile", None);
    }
    render_password_change_required(&session, &user, None, StatusCode::OK)
}

pub async fn change_required_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<PasswordChangeForm>,
) -> Result<Response, AppError> {
    let session = auth::require_session(&state.pool, &headers).await?;
    auth::verify_csrf(&session, &form.csrf_token)?;
    let user = require_user(&state, &session).await?;
    if !user.must_change_password {
        return redirect("/profile", None);
    }
    match auth::change_required_password(
        &state.pool,
        &user,
        &form.password,
        &form.password_confirmation,
    )
    .await
    {
        Ok(()) => redirect("/profile", None),
        Err(AppError::BadRequest(message)) => render_password_change_required(
            &session,
            &user,
            Some(&message),
            StatusCode::BAD_REQUEST,
        ),
        Err(error) => Err(error),
    }
}

pub async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<CsrfForm>,
) -> Result<Response, AppError> {
    let session = auth::require_session(&state.pool, &headers).await?;
    auth::verify_csrf(&session, &form.csrf_token)?;
    auth::sign_out(&state.pool, &session).await?;
    redirect("/", Some(auth::expired_session_cookie(&state.config)))
}

pub async fn messages_page(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let (session, user, ctx) = page_context(&state, &headers).await?;
    render_messages(
        &state,
        session.new_cookie,
        ctx,
        user.as_ref(),
        None,
        String::new(),
    )
    .await
}

pub async fn create_message(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<MessageForm>,
) -> Result<Response, AppError> {
    let session = auth::require_session(&state.pool, &headers).await?;
    auth::verify_csrf(&session, &form.csrf_token)?;
    let user = require_user(&state, &session).await?;
    match public_messages::create(&state.pool, &user.id, &form.body, &state.config.messages).await {
        Ok(()) => redirect("/messages", None),
        Err(AppError::BadRequest(message)) => {
            let ctx = PageContext::authenticated(session.csrf_token.clone(), &user);
            render_messages(&state, None, ctx, Some(&user), Some(&message), form.body).await
        }
        Err(error) => Err(error),
    }
}

pub async fn delete_message(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(message_id): Path<String>,
    Form(form): Form<DeleteMessageForm>,
) -> Result<Response, AppError> {
    let session = auth::require_session(&state.pool, &headers).await?;
    auth::verify_csrf(&session, &form.csrf_token)?;
    let user = require_user(&state, &session).await?;
    public_messages::mark_deleted(&state.pool, &message_id, &user.id, user.parsed_role()).await?;
    redirect(delete_return_to(form.return_to.as_deref()), None)
}

pub async fn memes_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<MemeQuery>,
) -> Result<Response, AppError> {
    let (session, _, ctx) = page_context(&state, &headers).await?;
    let page = memes::list_approved(
        &state.pool,
        query.tag.as_deref(),
        query.page.unwrap_or(1),
        &state.config.memes,
    )
    .await?;
    let memes: Vec<MemeView> = page
        .items
        .into_iter()
        .map(|meme| meme_view(meme, state.config.display.utc_offset_hours))
        .collect();
    let has_memes = !memes.is_empty();
    let tag = query.tag.unwrap_or_default();
    let popular_tags: Vec<PopularTagView> =
        memes::list_popular_tags(&state.pool, &state.config.memes)
            .await?
            .into_iter()
            .map(|popular_tag| popular_tag_view(popular_tag, &tag))
            .collect();
    let has_popular_tags = !popular_tags.is_empty();
    render(
        MemesTemplate {
            ctx,
            memes,
            has_memes,
            popular_tags,
            has_popular_tags,
            has_tag: !tag.trim().is_empty(),
            tag,
            current_page: page.current_page,
            has_previous_page: page.previous_page.is_some(),
            previous_page: page.previous_page.unwrap_or_default(),
            has_next_page: page.next_page.is_some(),
            next_page: page.next_page.unwrap_or_default(),
            page_size: state.config.memes.page_size,
        },
        StatusCode::OK,
        session.new_cookie,
    )
}

pub async fn new_meme_page(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let session = auth::require_session(&state.pool, &headers).await?;
    let user = require_user(&state, &session).await?;
    render_new_meme(&state, &session, &user, None, String::new(), String::new()).await
}

pub async fn create_meme(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Response, AppError> {
    let session = auth::require_session(&state.pool, &headers).await?;
    let user = require_user(&state, &session).await?;
    let mut csrf_token = None;
    let mut title = String::new();
    let mut raw_tags = String::new();
    let mut image_bytes = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| AppError::BadRequest("无法读取 Meme 上传表单".to_owned()))?
    {
        let field_name = field.name().unwrap_or_default().to_owned();
        match field_name.as_str() {
            "csrf_token" => {
                csrf_token = Some(
                    field
                        .text()
                        .await
                        .map_err(|_| AppError::BadRequest("CSRF 字段无效".to_owned()))?,
                );
            }
            "title" => {
                title = field
                    .text()
                    .await
                    .map_err(|_| AppError::BadRequest("标题字段无效".to_owned()))?;
            }
            "tags" => {
                raw_tags = field
                    .text()
                    .await
                    .map_err(|_| AppError::BadRequest("标签字段无效".to_owned()))?;
            }
            "meme" => {
                image_bytes = Some(
                    field
                        .bytes()
                        .await
                        .map_err(|_| AppError::BadRequest("Meme 文件读取失败".to_owned()))?
                        .to_vec(),
                );
            }
            _ => {}
        }
    }
    auth::verify_csrf(&session, csrf_token.as_deref().unwrap_or_default())?;

    let title = match memes::validate_title(&title, &state.config.memes) {
        Ok(value) => value,
        Err(AppError::BadRequest(message)) => {
            return render_new_meme(&state, &session, &user, Some(&message), title, raw_tags).await;
        }
        Err(error) => return Err(error),
    };
    let tags = match memes::normalize_tags(&raw_tags, &state.config.memes) {
        Ok(value) => value,
        Err(AppError::BadRequest(message)) => {
            return render_new_meme(&state, &session, &user, Some(&message), title, raw_tags).await;
        }
        Err(error) => return Err(error),
    };
    let bytes = image_bytes.ok_or_else(|| AppError::BadRequest("请选择 Meme 图片".to_owned()))?;
    let config = state.config.memes.clone();
    let processed = tokio::task::spawn_blocking(move || memes::process_image(bytes, &config))
        .await
        .map_err(|error| AppError::Internal(format!("Meme 处理任务异常：{error}")))??;

    tokio::fs::create_dir_all(&state.config.memes.dir).await?;
    let storage_name = format!(
        "{}.{}",
        Ulid::new().to_string().to_lowercase(),
        processed.extension
    );
    let temporary_name = format!(".{storage_name}.tmp");
    let temporary_path = state.config.memes.dir.join(&temporary_name);
    let final_path = state.config.memes.dir.join(&storage_name);
    tokio::fs::write(&temporary_path, &processed.bytes).await?;
    tokio::fs::rename(&temporary_path, &final_path).await?;

    let create_result = memes::create(
        &state.pool,
        &user,
        NewMeme {
            storage_name: storage_name.clone(),
            media_type: processed.media_type.to_owned(),
            title,
            tags,
        },
    )
    .await;
    if let Err(error) = create_result {
        // 数据库写入失败时清除刚落盘的文件，避免长期留下无人引用的图片。
        let _ = tokio::fs::remove_file(&final_path).await;
        return Err(error);
    }
    tracing::info!(
        user_id = %user.id,
        width = processed.width,
        height = processed.height,
        frames = processed.frame_count,
        "Meme 已提交待审核"
    );
    redirect("/memes", None)
}

pub async fn meme_image(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(meme_id): Path<String>,
) -> Result<Response, AppError> {
    let (_, user, _) = page_context(&state, &headers).await?;
    let (storage_name, media_type) =
        memes::image_info(&state.pool, &meme_id, user.as_ref()).await?;
    if !safe_storage_name(&storage_name) {
        return Err(AppError::NotFound);
    }
    let bytes = tokio::fs::read(state.config.memes.dir.join(storage_name)).await?;
    // 删除后的图片不应继续被浏览器缓存，避免个人页仍短暂显示旧内容。
    binary_response(bytes, &media_type, "no-store")
}

pub async fn admin_memes(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AdminMemeQuery>,
) -> Result<Response, AppError> {
    let session = auth::require_session(&state.pool, &headers).await?;
    let actor = require_admin(&state, &session).await?;
    let status_filter = memes::AdminMemeStatus::from_query(query.status.as_deref())?;
    let search_query = query
        .q
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
        .to_owned();
    let memes: Vec<MemeView> = memes::list_for_admin(
        &state.pool,
        status_filter,
        query.q.as_deref(),
        state.config.memes.page_size,
    )
    .await?
    .into_iter()
    .map(|meme| meme_view(meme, state.config.display.utc_offset_hours))
    .collect();
    let has_memes = !memes.is_empty();
    render(
        AdminMemesTemplate {
            ctx: PageContext::authenticated(session.csrf_token, &actor),
            memes,
            has_memes,
            pending_filter_active: status_filter.is_pending(),
            approved_filter_active: status_filter.is_approved(),
            empty_message: status_filter.empty_message(),
            query: search_query,
            has_query: query
                .q
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty()),
            return_to: admin_memes_return_to(status_filter, query.q.as_deref()),
        },
        StatusCode::OK,
        None,
    )
}

pub async fn approve_meme(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(meme_id): Path<String>,
    Form(form): Form<CsrfForm>,
) -> Result<Response, AppError> {
    let session = auth::require_session(&state.pool, &headers).await?;
    let actor = require_admin(&state, &session).await?;
    auth::verify_csrf(&session, &form.csrf_token)?;
    memes::approve(&state.pool, &meme_id, &actor).await?;
    redirect("/admin/memes", None)
}

pub async fn delete_meme(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(meme_id): Path<String>,
    Form(form): Form<DeleteMemeForm>,
) -> Result<Response, AppError> {
    let session = auth::require_session(&state.pool, &headers).await?;
    let actor = require_admin(&state, &session).await?;
    auth::verify_csrf(&session, &form.csrf_token)?;
    memes::mark_deleted(&state.pool, &meme_id, &actor).await?;
    redirect(admin_meme_return_to(form.return_to.as_deref()), None)
}

pub async fn delete_own_meme(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(meme_id): Path<String>,
    Form(form): Form<CsrfForm>,
) -> Result<Response, AppError> {
    let session = auth::require_session(&state.pool, &headers).await?;
    auth::verify_csrf(&session, &form.csrf_token)?;
    let user = require_user(&state, &session).await?;
    memes::mark_deleted_by_author(&state.pool, &meme_id, &user).await?;
    redirect("/profile?updated=meme", None)
}

pub async fn profile_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ProfileQuery>,
) -> Result<Response, AppError> {
    let session = auth::require_session(&state.pool, &headers).await?;
    let user = require_user(&state, &session).await?;
    let success = match query.updated.as_deref() {
        Some("nickname") => "昵称已更新",
        Some("bio") => "个人简介已更新",
        Some("avatar") => "头像已更新",
        Some("meme") => "Meme 已删除",
        _ => "",
    };
    render_profile(
        &state,
        &session,
        &user,
        None,
        (!success.is_empty()).then_some(success),
    )
    .await
}

pub async fn update_nickname(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<NicknameForm>,
) -> Result<Response, AppError> {
    let session = auth::require_session(&state.pool, &headers).await?;
    auth::verify_csrf(&session, &form.csrf_token)?;
    let user = require_user(&state, &session).await?;
    let (nickname, nickname_key) = match auth::validate_nickname(&form.nickname) {
        Ok(value) => value,
        Err(AppError::BadRequest(message)) => {
            return render_profile(&state, &session, &user, Some(&message), None).await;
        }
        Err(error) => return Err(error),
    };

    let duplicate = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM users WHERE nickname_key = ? AND id <> ?",
    )
    .bind(&nickname_key)
    .bind(&user.id)
    .fetch_one(&state.pool)
    .await?
        > 0;
    if duplicate {
        return render_profile(&state, &session, &user, Some("昵称已被使用"), None).await;
    }

    let result =
        sqlx::query("UPDATE users SET nickname = ?, nickname_key = ?, updated_at = ? WHERE id = ?")
            .bind(nickname)
            .bind(nickname_key)
            .bind(auth::now_string()?)
            .bind(&user.id)
            .execute(&state.pool)
            .await;
    if let Err(error) = result {
        if is_unique_violation(&error) {
            return render_profile(&state, &session, &user, Some("昵称已被使用"), None).await;
        }
        return Err(error.into());
    }
    redirect("/profile?updated=nickname", None)
}

pub async fn update_bio(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<BioForm>,
) -> Result<Response, AppError> {
    let session = auth::require_session(&state.pool, &headers).await?;
    auth::verify_csrf(&session, &form.csrf_token)?;
    let user = require_user(&state, &session).await?;
    let bio = match auth::validate_bio(&form.bio) {
        Ok(value) => value,
        Err(AppError::BadRequest(message)) => {
            return render_profile(&state, &session, &user, Some(&message), None).await;
        }
        Err(error) => return Err(error),
    };

    sqlx::query("UPDATE users SET bio = ?, updated_at = ? WHERE id = ?")
        .bind(bio)
        .bind(auth::now_string()?)
        .bind(&user.id)
        .execute(&state.pool)
        .await?;
    redirect("/profile?updated=bio", None)
}

pub async fn update_avatar(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Response, AppError> {
    let session = auth::require_session(&state.pool, &headers).await?;
    let user = require_user(&state, &session).await?;
    let mut csrf_token = None;
    let mut avatar_bytes = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| AppError::BadRequest("无法读取上传表单".to_owned()))?
    {
        let field_name = field.name().unwrap_or_default().to_owned();
        match field_name.as_str() {
            "csrf_token" => {
                csrf_token = Some(
                    field
                        .text()
                        .await
                        .map_err(|_| AppError::BadRequest("CSRF 字段无效".to_owned()))?,
                );
            }
            "avatar" => {
                avatar_bytes = Some(
                    field
                        .bytes()
                        .await
                        .map_err(|_| AppError::BadRequest("头像文件读取失败".to_owned()))?
                        .to_vec(),
                );
            }
            _ => {}
        }
    }
    auth::verify_csrf(&session, csrf_token.as_deref().unwrap_or_default())?;
    let bytes = avatar_bytes.ok_or_else(|| AppError::BadRequest("请选择头像文件".to_owned()))?;
    let processed = tokio::task::spawn_blocking(move || avatar::process(bytes))
        .await
        .map_err(|error| AppError::Internal(format!("头像处理任务异常：{error}")))??;

    let storage_name = format!(
        "{}.{}",
        Ulid::new().to_string().to_lowercase(),
        processed.extension
    );
    let temporary_name = format!(".{storage_name}.tmp");
    let temporary_path = state.config.avatar_dir.join(&temporary_name);
    let final_path = state.config.avatar_dir.join(&storage_name);
    tokio::fs::write(&temporary_path, &processed.bytes).await?;
    tokio::fs::rename(&temporary_path, &final_path).await?;

    let update_result = sqlx::query(
        "UPDATE users SET avatar_storage_name = ?, avatar_media_type = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&storage_name)
    .bind(processed.media_type)
    .bind(auth::now_string()?)
    .bind(&user.id)
    .execute(&state.pool)
    .await;
    if let Err(error) = update_result {
        let _ = tokio::fs::remove_file(&final_path).await;
        return Err(error.into());
    }
    if let Some(old_name) = user.avatar_storage_name
        && safe_storage_name(&old_name)
    {
        let _ = tokio::fs::remove_file(state.config.avatar_dir.join(old_name)).await;
    }
    tracing::info!(
        user_id = %user.id,
        width = processed.width,
        height = processed.height,
        frames = processed.frame_count,
        "用户头像已更新"
    );
    redirect("/profile?updated=avatar", None)
}

pub async fn public_profile(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(username): Path<String>,
) -> Result<Response, AppError> {
    let (session, _, ctx) = page_context(&state, &headers).await?;
    let username_key = username.trim().to_lowercase();
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE username_key = ?")
        .bind(username_key)
        .fetch_optional(&state.pool)
        .await?
        .ok_or(AppError::NotFound)?;

    render(
        PublicProfileTemplate {
            ctx,
            user_id: user.id.clone(),
            username: user.username.clone(),
            nickname: user.nickname.clone(),
            role_label: user.parsed_role().label(),
            has_bio: !user.bio.is_empty(),
            bio: user.bio,
            created_at: time_display::friendly_rfc3339(
                &user.created_at,
                state.config.display.utc_offset_hours,
            ),
        },
        StatusCode::OK,
        session.new_cookie,
    )
}

pub async fn user_avatar(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
) -> Result<Response, AppError> {
    let avatar = sqlx::query_as::<_, (Option<String>, Option<String>)>(
        "SELECT avatar_storage_name, avatar_media_type FROM users WHERE id = ?",
    )
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;

    let (bytes, media_type) = match avatar {
        (Some(storage_name), Some(media_type)) if safe_storage_name(&storage_name) => {
            match tokio::fs::read(state.config.avatar_dir.join(storage_name)).await {
                Ok(bytes) => (bytes, media_type),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    tracing::warn!(%error, "头像文件缺失，回退到默认头像");
                    (DEFAULT_AVATAR.to_vec(), "image/png".to_owned())
                }
                Err(error) => return Err(error.into()),
            }
        }
        _ => (DEFAULT_AVATAR.to_vec(), "image/png".to_owned()),
    };
    binary_response(bytes, &media_type, "public, max-age=300")
}

pub async fn admin_users(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AdminUsersQuery>,
) -> Result<Response, AppError> {
    let session = auth::require_session(&state.pool, &headers).await?;
    let actor = require_super_admin(&state, &session).await?;
    let users = sqlx::query_as::<_, User>("SELECT * FROM users ORDER BY created_at ASC")
        .fetch_all(&state.pool)
        .await?
        .into_iter()
        .map(|user| {
            let role = user.parsed_role();
            AdminUserView {
                id: user.id,
                username: user.username,
                nickname: user.nickname,
                role_label: role.label(),
                can_change: role != Role::SuperAdmin,
                is_admin: role == Role::Admin,
                must_change_password: user.must_change_password,
            }
        })
        .collect();
    let ctx = PageContext::authenticated(session.csrf_token, &actor);
    let reset_user = query
        .password_reset
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    render(
        AdminUsersTemplate {
            ctx,
            users,
            has_message: reset_user.is_some(),
            message: reset_user
                .map(|username| {
                    format!("已将 {username} 的密码重置为用户名，并要求其下次登录修改密码")
                })
                .unwrap_or_default(),
        },
        StatusCode::OK,
        None,
    )
}

pub async fn update_role(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(target_id): Path<String>,
    Form(form): Form<RoleForm>,
) -> Result<Response, AppError> {
    let session = auth::require_session(&state.pool, &headers).await?;
    let actor = require_super_admin(&state, &session).await?;
    auth::verify_csrf(&session, &form.csrf_token)?;
    let new_role =
        Role::from_str(&form.role).map_err(|_| AppError::BadRequest("身份无效".to_owned()))?;
    if !matches!(new_role, Role::User | Role::Admin) {
        return Err(AppError::BadRequest("只能授予或撤销管理员身份".to_owned()));
    }

    let mut transaction = state.pool.begin().await?;
    let current_role = sqlx::query_scalar::<_, String>("SELECT role FROM users WHERE id = ?")
        .bind(&target_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(AppError::NotFound)?;
    if Role::from_str(&current_role) == Ok(Role::SuperAdmin) {
        return Err(AppError::Forbidden);
    }
    sqlx::query("UPDATE users SET role = ?, updated_at = ? WHERE id = ?")
        .bind(new_role.as_str())
        .bind(auth::now_string()?)
        .bind(&target_id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query(
        "INSERT INTO role_audit_logs (id, actor_user_id, target_user_id, old_role, new_role, created_at) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(Ulid::new().to_string())
    .bind(&actor.id)
    .bind(target_id)
    .bind(current_role)
    .bind(new_role.as_str())
    .bind(auth::now_string()?)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    redirect("/admin/users", None)
}

pub async fn reset_user_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(target_id): Path<String>,
    Form(form): Form<CsrfForm>,
) -> Result<Response, AppError> {
    let session = auth::require_session(&state.pool, &headers).await?;
    let actor = require_super_admin(&state, &session).await?;
    auth::verify_csrf(&session, &form.csrf_token)?;
    let target = auth::reset_password_to_username(&state.pool, &actor, &target_id).await?;
    redirect(
        &format!(
            "/admin/users?password_reset={}",
            percent_encode_query_value(&target.username)
        ),
        None,
    )
}

pub async fn app_css() -> Response {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("../../static/app.css"),
    )
        .into_response()
}

pub async fn not_found() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, "页面不存在")
}

pub async fn enforce_required_password_change(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, AppError> {
    let path = request.uri().path().to_owned();
    if password_change_required_allowed_path(&path) {
        return Ok(next.run(request).await);
    }
    if auth::current_user_from_headers(&state.pool, request.headers())
        .await?
        .is_some_and(|user| user.must_change_password)
    {
        return redirect("/password/change-required", None);
    }
    Ok(next.run(request).await)
}

fn password_change_required_allowed_path(path: &str) -> bool {
    path == "/password/change-required"
        || path == "/logout"
        || path == "/static/app.css"
        || (path.starts_with("/users/") && path.ends_with("/avatar"))
        || (path.starts_with("/memes/") && path.ends_with("/image"))
}

async fn page_context(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(SessionContext, Option<User>, PageContext), AppError> {
    let session = auth::load_or_create_session(&state.pool, &state.config, headers).await?;
    let user = auth::current_user(&state.pool, &session.row).await?;
    let ctx = match &user {
        Some(user) => PageContext::authenticated(session.row.csrf_token.clone(), user),
        None => PageContext::anonymous(session.row.csrf_token.clone()),
    };
    Ok((session, user, ctx))
}

async fn require_user(state: &AppState, session: &SessionRow) -> Result<User, AppError> {
    auth::current_user(&state.pool, session)
        .await?
        .ok_or(AppError::Unauthorized)
}

async fn require_super_admin(state: &AppState, session: &SessionRow) -> Result<User, AppError> {
    let user = require_user(state, session).await?;
    if user.parsed_role() != Role::SuperAdmin {
        return Err(AppError::Forbidden);
    }
    Ok(user)
}

async fn require_admin(state: &AppState, session: &SessionRow) -> Result<User, AppError> {
    let user = require_user(state, session).await?;
    if !matches!(user.parsed_role(), Role::Admin | Role::SuperAdmin) {
        return Err(AppError::Forbidden);
    }
    Ok(user)
}

fn render_password_change_required(
    session: &SessionRow,
    user: &User,
    error: Option<&str>,
    status: StatusCode,
) -> Result<Response, AppError> {
    render(
        PasswordChangeRequiredTemplate {
            ctx: PageContext::authenticated(session.csrf_token.clone(), user),
            has_error: error.is_some(),
            error: error.unwrap_or_default().to_owned(),
        },
        status,
        None,
    )
}

async fn render_profile(
    state: &AppState,
    session: &SessionRow,
    user: &User,
    error: Option<&str>,
    success: Option<&str>,
) -> Result<Response, AppError> {
    let messages: Vec<MessageView> =
        public_messages::list_by_author(&state.pool, &user.id, &state.config.messages)
            .await?
            .into_iter()
            .map(|message| message_view(message, Some(user), state.config.display.utc_offset_hours))
            .collect();
    let has_messages = !messages.is_empty();
    let memes: Vec<MemeView> = memes::list_by_author(&state.pool, &user.id)
        .await?
        .into_iter()
        .map(|meme| meme_view(meme, state.config.display.utc_offset_hours))
        .collect();
    let has_memes = !memes.is_empty();
    render(
        ProfileTemplate {
            ctx: PageContext::authenticated(session.csrf_token.clone(), user),
            user_id: user.id.clone(),
            username: user.username.clone(),
            nickname: user.nickname.clone(),
            role_label: user.parsed_role().label(),
            bio: user.bio.clone(),
            has_error: error.is_some(),
            error: error.unwrap_or_default().to_owned(),
            has_success: success.is_some(),
            success: success.unwrap_or_default().to_owned(),
            messages,
            has_messages,
            retention_days: state.config.messages.retention_days,
            memes,
            has_memes,
        },
        if error.is_some() {
            StatusCode::BAD_REQUEST
        } else {
            StatusCode::OK
        },
        None,
    )
}

async fn render_messages(
    state: &AppState,
    cookie: Option<String>,
    ctx: PageContext,
    current_user: Option<&User>,
    error: Option<&str>,
    body: String,
) -> Result<Response, AppError> {
    let messages: Vec<MessageView> =
        public_messages::list_recent(&state.pool, &state.config.messages)
            .await?
            .into_iter()
            .map(|message| {
                message_view(message, current_user, state.config.display.utc_offset_hours)
            })
            .collect();
    let has_messages = !messages.is_empty();
    render(
        MessagesTemplate {
            ctx,
            messages,
            has_messages,
            authenticated: current_user.is_some(),
            has_error: error.is_some(),
            error: error.unwrap_or_default().to_owned(),
            body,
            message_limit: state.config.messages.limit_per_user,
            retention_days: state.config.messages.retention_days,
            max_length: state.config.messages.max_length,
        },
        if error.is_some() {
            StatusCode::BAD_REQUEST
        } else {
            StatusCode::OK
        },
        cookie,
    )
}

async fn render_new_meme(
    state: &AppState,
    session: &SessionRow,
    user: &User,
    error: Option<&str>,
    title: String,
    tags: String,
) -> Result<Response, AppError> {
    render(
        NewMemeTemplate {
            ctx: PageContext::authenticated(session.csrf_token.clone(), user),
            has_error: error.is_some(),
            error: error.unwrap_or_default().to_owned(),
            title,
            tags,
            max_upload_kib: state.config.memes.max_upload_bytes / 1024,
            max_tags: state.config.memes.max_tags_per_meme,
            max_title_length: state.config.memes.max_title_length,
        },
        if error.is_some() {
            StatusCode::BAD_REQUEST
        } else {
            StatusCode::OK
        },
        None,
    )
}

fn message_view(
    message: PublicMessageRow,
    current_user: Option<&User>,
    utc_offset_hours: i8,
) -> MessageView {
    let role = Role::from_str(&message.role).unwrap_or(Role::User);
    let can_delete = current_user.is_some_and(|user| {
        user.id == message.author_user_id
            || matches!(user.parsed_role(), Role::Admin | Role::SuperAdmin)
    });
    MessageView {
        id: message.id,
        author_user_id: message.author_user_id,
        username: message.username,
        nickname: message.nickname,
        role_label: role.label(),
        body: message.body,
        created_at: time_display::friendly_rfc3339(&message.created_at, utc_offset_hours),
        can_delete,
    }
}

fn home_message_view(message: PublicMessageRow, utc_offset_hours: i8) -> MessageView {
    let role = Role::from_str(&message.role).unwrap_or(Role::User);
    MessageView {
        id: message.id,
        author_user_id: message.author_user_id,
        username: message.username,
        nickname: message.nickname,
        role_label: role.label(),
        body: message.body,
        created_at: time_display::friendly_rfc3339(&message.created_at, utc_offset_hours),
        can_delete: false,
    }
}

fn meme_view(meme: MemeWithTags, utc_offset_hours: i8) -> MemeView {
    let status_label = meme_status_label(&meme.row);
    MemeView {
        id: meme.row.id,
        author_user_id: meme.row.author_user_id,
        username: meme.row.username,
        nickname: meme.row.nickname,
        title: meme.row.title,
        is_pending: meme.row.status == memes::STATUS_PENDING,
        status_label,
        created_at: time_display::friendly_rfc3339(&meme.row.created_at, utc_offset_hours),
        has_tags: !meme.tags.is_empty(),
        tags: meme.tags,
    }
}

fn popular_tag_view(tag: memes::PopularTag, selected_tag: &str) -> PopularTagView {
    let selected_tag = selected_tag.trim();
    let is_active = !selected_tag.is_empty() && selected_tag == tag.name;
    let href = format!("/memes?tag={}", percent_encode_query_value(&tag.name));
    PopularTagView {
        name: tag.name,
        usage_count: tag.usage_count,
        href,
        is_active,
    }
}

fn meme_status_label(meme: &MemeRow) -> &'static str {
    match meme.status.as_str() {
        memes::STATUS_PENDING => "待审核",
        memes::STATUS_APPROVED => "已通过",
        memes::STATUS_DELETED => "已删除",
        _ => "未知",
    }
}

fn delete_return_to(value: Option<&str>) -> &'static str {
    match value {
        Some("/profile") => "/profile",
        _ => "/messages",
    }
}

fn admin_meme_return_to(value: Option<&str>) -> &str {
    match value {
        Some(value) if value == "/admin/memes" || value.starts_with("/admin/memes?") => value,
        _ => "/admin/memes",
    }
}

fn admin_memes_return_to(status_filter: memes::AdminMemeStatus, query: Option<&str>) -> String {
    let query = query.map(str::trim).filter(|value| !value.is_empty());
    let mut return_to = "/admin/memes".to_owned();
    let mut separator = '?';
    if status_filter.is_approved() {
        return_to.push(separator);
        return_to.push_str("status=approved");
        separator = '&';
    }
    if let Some(query) = query {
        return_to.push(separator);
        return_to.push_str("q=");
        return_to.push_str(&percent_encode_query_value(query));
    }
    return_to
}

fn percent_encode_query_value(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push('%');
            encoded.push(HEX[(byte >> 4) as usize] as char);
            encoded.push(HEX[(byte & 0x0F) as usize] as char);
        }
    }
    encoded
}

fn render<T: Template>(
    template: T,
    status: StatusCode,
    cookie: Option<String>,
) -> Result<Response, AppError> {
    let html = template.render()?;
    let mut response = Html(html).into_response();
    *response.status_mut() = status;
    attach_cookie(&mut response, cookie)?;
    Ok(response)
}

fn redirect(location: &str, cookie: Option<String>) -> Result<Response, AppError> {
    let mut response = Redirect::to(location).into_response();
    attach_cookie(&mut response, cookie)?;
    Ok(response)
}

fn attach_cookie(response: &mut Response, cookie: Option<String>) -> Result<(), AppError> {
    if let Some(cookie) = cookie {
        response
            .headers_mut()
            .append(header::SET_COOKIE, auth::set_cookie_header(&cookie)?);
    }
    Ok(())
}

fn binary_response(
    bytes: Vec<u8>,
    media_type: &str,
    cache_control: &str,
) -> Result<Response, AppError> {
    let mut response = Body::from(bytes).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(media_type)
            .map_err(|error| AppError::Internal(format!("头像媒体类型无效：{error}")))?,
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_str(cache_control)
            .map_err(|error| AppError::Internal(format!("缓存响应头无效：{error}")))?,
    );
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    Ok(response)
}

fn safe_storage_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 96
        && name.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '.'
        })
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
}
