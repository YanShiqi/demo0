mod views;

use std::{net::SocketAddr, str::FromStr};

use askama::Template;
use axum::{
    Form,
    body::Body,
    extract::{ConnectInfo, Multipart, Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Redirect, Response},
};
use serde::Deserialize;
use ulid::Ulid;

use crate::{
    app::AppState,
    auth,
    avatar::{self, DEFAULT_AVATAR},
    error::AppError,
    model::{PageContext, Role, SessionContext, SessionRow, User},
};
use views::{
    AdminUserView, AdminUsersTemplate, HomeTemplate, LoginTemplate, ProfileTemplate,
    RegisterTemplate,
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
pub struct NicknameForm {
    csrf_token: String,
    nickname: String,
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

pub async fn home(State(state): State<AppState>, headers: HeaderMap) -> Result<Response, AppError> {
    let (session, _, ctx) = page_context(&state, &headers).await?;
    render(HomeTemplate { ctx }, StatusCode::OK, session.new_cookie)
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
    redirect("/profile", Some(cookie))
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

pub async fn profile_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ProfileQuery>,
) -> Result<Response, AppError> {
    let session = auth::require_session(&state.pool, &headers).await?;
    let user = require_user(&state, &session).await?;
    let success = match query.updated.as_deref() {
        Some("nickname") => "昵称已更新",
        Some("avatar") => "头像已更新",
        _ => "",
    };
    render_profile(
        &session,
        &user,
        None,
        (!success.is_empty()).then_some(success),
    )
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
            return render_profile(&session, &user, Some(&message), None);
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
        return render_profile(&session, &user, Some("昵称已被使用"), None);
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
            return render_profile(&session, &user, Some("昵称已被使用"), None);
        }
        return Err(error.into());
    }
    redirect("/profile?updated=nickname", None)
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
            }
        })
        .collect();
    let ctx = PageContext::authenticated(session.csrf_token, &actor);
    render(
        AdminUsersTemplate {
            ctx,
            users,
            has_message: false,
            message: String::new(),
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

fn render_profile(
    session: &SessionRow,
    user: &User,
    error: Option<&str>,
    success: Option<&str>,
) -> Result<Response, AppError> {
    render(
        ProfileTemplate {
            ctx: PageContext::authenticated(session.csrf_token.clone(), user),
            user_id: user.id.clone(),
            username: user.username.clone(),
            nickname: user.nickname.clone(),
            role_label: user.parsed_role().label(),
            has_error: error.is_some(),
            error: error.unwrap_or_default().to_owned(),
            has_success: success.is_some(),
            success: success.unwrap_or_default().to_owned(),
        },
        if error.is_some() {
            StatusCode::BAD_REQUEST
        } else {
            StatusCode::OK
        },
        None,
    )
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
