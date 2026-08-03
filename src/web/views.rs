use askama::Template;

use crate::model::PageContext;

#[derive(Template)]
#[template(path = "home.html")]
pub struct HomeTemplate {
    pub ctx: PageContext,
    pub messages: Vec<MessageView>,
    pub has_messages: bool,
    pub message_preview_limit: i64,
    pub memes: Vec<MemeView>,
    pub has_memes: bool,
    pub meme_preview_limit: i64,
}

#[derive(Template)]
#[template(path = "register.html")]
pub struct RegisterTemplate {
    pub ctx: PageContext,
    pub has_error: bool,
    pub error: String,
    pub username: String,
    pub nickname: String,
}

#[derive(Template)]
#[template(path = "login.html")]
pub struct LoginTemplate {
    pub ctx: PageContext,
    pub has_error: bool,
    pub error: String,
    pub username: String,
}

#[derive(Clone, Debug)]
pub struct MessageView {
    pub id: String,
    pub author_user_id: String,
    pub username: String,
    pub nickname: String,
    pub role_label: &'static str,
    pub body: String,
    pub created_at: String,
    pub can_delete: bool,
}

#[derive(Template)]
#[template(path = "messages.html")]
pub struct MessagesTemplate {
    pub ctx: PageContext,
    pub messages: Vec<MessageView>,
    pub has_messages: bool,
    pub authenticated: bool,
    pub has_error: bool,
    pub error: String,
    pub body: String,
    pub message_limit: i64,
    pub retention_days: i64,
    pub max_length: usize,
}

#[derive(Clone, Debug)]
pub struct MemeView {
    pub id: String,
    pub author_user_id: String,
    pub username: String,
    pub nickname: String,
    pub title: String,
    pub is_pending: bool,
    pub status_label: &'static str,
    pub created_at: String,
    pub tags: Vec<String>,
    pub has_tags: bool,
}

#[derive(Template)]
#[template(path = "memes.html")]
pub struct MemesTemplate {
    pub ctx: PageContext,
    pub memes: Vec<MemeView>,
    pub has_memes: bool,
    pub tag: String,
    pub has_tag: bool,
    pub current_page: i64,
    pub previous_page: i64,
    pub has_previous_page: bool,
    pub next_page: i64,
    pub has_next_page: bool,
    pub page_size: i64,
}

#[derive(Template)]
#[template(path = "meme_new.html")]
pub struct NewMemeTemplate {
    pub ctx: PageContext,
    pub has_error: bool,
    pub error: String,
    pub title: String,
    pub tags: String,
    pub max_upload_kib: usize,
    pub max_tags: usize,
    pub max_title_length: usize,
}

#[derive(Template)]
#[template(path = "admin_memes.html")]
pub struct AdminMemesTemplate {
    pub ctx: PageContext,
    pub memes: Vec<MemeView>,
    pub has_memes: bool,
    pub pending_filter_active: bool,
    pub approved_filter_active: bool,
    pub empty_message: &'static str,
    pub query: String,
    pub has_query: bool,
    pub return_to: String,
}

#[derive(Template)]
#[template(path = "profile.html")]
pub struct ProfileTemplate {
    pub ctx: PageContext,
    pub user_id: String,
    pub username: String,
    pub nickname: String,
    pub role_label: &'static str,
    pub bio: String,
    pub has_error: bool,
    pub error: String,
    pub has_success: bool,
    pub success: String,
    pub messages: Vec<MessageView>,
    pub has_messages: bool,
    pub retention_days: i64,
    pub memes: Vec<MemeView>,
    pub has_memes: bool,
}

#[derive(Template)]
#[template(path = "public_profile.html")]
pub struct PublicProfileTemplate {
    pub ctx: PageContext,
    pub user_id: String,
    pub username: String,
    pub nickname: String,
    pub role_label: &'static str,
    pub bio: String,
    pub has_bio: bool,
    pub created_at: String,
}

#[derive(Clone, Debug)]
pub struct AdminUserView {
    pub id: String,
    pub username: String,
    pub nickname: String,
    pub role_label: &'static str,
    pub can_change: bool,
    pub is_admin: bool,
}

#[derive(Template)]
#[template(path = "admin_users.html")]
pub struct AdminUsersTemplate {
    pub ctx: PageContext,
    pub users: Vec<AdminUserView>,
    pub has_message: bool,
    pub message: String,
}
