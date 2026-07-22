use askama::Template;

use crate::model::PageContext;

#[derive(Template)]
#[template(path = "home.html")]
pub struct HomeTemplate {
    pub ctx: PageContext,
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
