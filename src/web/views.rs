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

#[derive(Template)]
#[template(path = "profile.html")]
pub struct ProfileTemplate {
    pub ctx: PageContext,
    pub user_id: String,
    pub username: String,
    pub nickname: String,
    pub role_label: &'static str,
    pub has_error: bool,
    pub error: String,
    pub has_success: bool,
    pub success: String,
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
