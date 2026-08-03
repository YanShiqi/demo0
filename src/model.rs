use std::{fmt, str::FromStr};

use serde::Serialize;
use sqlx::FromRow;

#[derive(Clone, Debug, FromRow)]
pub struct User {
    pub id: String,
    pub username: String,
    pub username_key: String,
    pub nickname: String,
    pub nickname_key: String,
    pub password_hash: String,
    pub role: String,
    pub bio: String,
    pub must_change_password: bool,
    pub avatar_storage_name: Option<String>,
    pub avatar_media_type: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl User {
    pub fn parsed_role(&self) -> Role {
        Role::from_str(&self.role).unwrap_or(Role::User)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum Role {
    User,
    Admin,
    SuperAdmin,
}

impl Role {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Admin => "admin",
            Self::SuperAdmin => "super_admin",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::User => "普通用户",
            Self::Admin => "管理员",
            Self::SuperAdmin => "超级管理员",
        }
    }
}

impl FromStr for Role {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "user" => Ok(Self::User),
            "admin" => Ok(Self::Admin),
            "super_admin" => Ok(Self::SuperAdmin),
            _ => Err(()),
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, FromRow)]
pub struct SessionRow {
    pub token_hash: String,
    pub user_id: Option<String>,
    pub csrf_token: String,
    pub expires_at: i64,
    pub created_at: String,
}

#[derive(Clone, Debug)]
pub struct SessionContext {
    pub row: SessionRow,
    pub new_cookie: Option<String>,
}

#[derive(Clone, Debug)]
pub struct PageContext {
    pub csrf_token: String,
    pub authenticated: bool,
    pub nickname: String,
    pub is_admin: bool,
    pub is_super_admin: bool,
}

impl PageContext {
    pub fn anonymous(csrf_token: String) -> Self {
        Self {
            csrf_token,
            authenticated: false,
            nickname: String::new(),
            is_admin: false,
            is_super_admin: false,
        }
    }

    pub fn authenticated(csrf_token: String, user: &User) -> Self {
        Self {
            csrf_token,
            authenticated: true,
            nickname: user.nickname.clone(),
            is_admin: matches!(user.parsed_role(), Role::Admin | Role::SuperAdmin),
            is_super_admin: user.parsed_role() == Role::SuperAdmin,
        }
    }
}
