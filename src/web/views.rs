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
    pub home_messages_tab_active: bool,
    pub home_memes_tab_active: bool,
    pub home_novels_tab_active: bool,
    pub novel_chapter_previews: Vec<NovelChapterPreviewView>,
    pub has_novel_chapter_previews: bool,
    pub novel_preview_limit: i64,
    pub updates: Vec<UpdateView>,
    pub has_updates: bool,
    pub update_preview_limit: i64,
    pub check_in_enabled: bool,
    pub check_in_completed: bool,
    pub check_in_reward_amount: i64,
    pub check_in_currency_name: String,
    pub check_in_currency_symbol: String,
    pub check_in_message: String,
    pub has_check_in_message: bool,
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
#[template(path = "password_change_required.html")]
pub struct PasswordChangeRequiredTemplate {
    pub ctx: PageContext,
    pub has_error: bool,
    pub error: String,
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
    pub show_identity: bool,
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
    pub anonymous: bool,
}

#[derive(Clone, Debug)]
pub struct CurrencyLogView {
    pub amount_delta: i64,
    pub balance_after: i64,
    pub reason_label: String,
    pub note: String,
    pub created_at: String,
}

#[derive(Template)]
#[template(path = "currency.html")]
pub struct CurrencyTemplate {
    pub ctx: PageContext,
    pub currency_name: String,
    pub currency_symbol: String,
    pub balance: i64,
    pub logs: Vec<CurrencyLogView>,
    pub has_logs: bool,
    pub current_page: i64,
    pub total_pages: i64,
    pub previous_page: i64,
    pub has_previous_page: bool,
    pub next_page: i64,
    pub has_next_page: bool,
}

#[derive(Clone, Debug)]
pub struct CurrencyUserView {
    pub id: String,
    pub username: String,
    pub nickname: String,
    pub role_label: &'static str,
    pub balance: i64,
    pub href: String,
}

#[derive(Clone, Debug)]
pub struct RecentCurrencyLogView {
    pub username: String,
    pub nickname: String,
    pub user_href: String,
    pub amount_delta: i64,
    pub balance_after: i64,
    pub reason_label: String,
    pub note: String,
    pub created_at: String,
}

#[derive(Template)]
#[template(path = "admin_currency.html")]
pub struct AdminCurrencyTemplate {
    pub ctx: PageContext,
    pub currency_name: String,
    pub currency_symbol: String,
    pub query: String,
    pub has_query: bool,
    pub users: Vec<CurrencyUserView>,
    pub has_users: bool,
    pub selected_user: Option<CurrencyUserView>,
    pub recent_logs: Vec<RecentCurrencyLogView>,
    pub has_recent_logs: bool,
    pub recent_log_limit: i64,
    pub logs: Vec<CurrencyLogView>,
    pub has_logs: bool,
    pub current_page: i64,
    pub total_pages: i64,
    pub previous_page: i64,
    pub has_previous_page: bool,
    pub next_page: i64,
    pub has_next_page: bool,
    pub can_adjust: bool,
    pub max_adjust_amount: i64,
    pub max_note_length: usize,
}

#[derive(Clone, Debug)]
pub struct MemeView {
    pub id: String,
    pub author_user_id: String,
    pub username: String,
    pub nickname: String,
    pub title: String,
    pub detail_href: String,
    pub is_pending: bool,
    pub status_label: &'static str,
    pub created_at: String,
    pub tags: Vec<String>,
    pub has_tags: bool,
}

#[derive(Clone, Debug)]
pub struct MemeAdjacentView {
    pub title: String,
    pub href: String,
}

#[derive(Clone, Debug)]
pub struct UpdateView {
    pub date: String,
    pub version: String,
    pub title: String,
    pub summary: String,
    pub changes: Vec<String>,
}

#[derive(Template)]
#[template(path = "updates.html")]
pub struct UpdatesTemplate {
    pub ctx: PageContext,
    pub updates: Vec<UpdateView>,
    pub has_updates: bool,
}

#[derive(Clone, Debug)]
pub struct PopularTagView {
    pub name: String,
    pub usage_count: i64,
    pub href: String,
    pub is_active: bool,
}

#[derive(Template)]
#[template(path = "memes.html")]
pub struct MemesTemplate {
    pub ctx: PageContext,
    pub memes: Vec<MemeView>,
    pub has_memes: bool,
    pub popular_tags: Vec<PopularTagView>,
    pub has_popular_tags: bool,
    pub tag: String,
    pub has_tag: bool,
    pub current_page: i64,
    pub total_pages: i64,
    pub previous_page: i64,
    pub has_previous_page: bool,
    pub next_page: i64,
    pub has_next_page: bool,
    pub page_size: i64,
}

#[derive(Template)]
#[template(path = "meme_detail.html")]
pub struct MemeDetailTemplate {
    pub ctx: PageContext,
    pub meme: MemeView,
    pub has_previous_meme: bool,
    pub previous_meme: MemeAdjacentView,
    pub has_next_meme: bool,
    pub next_meme: MemeAdjacentView,
    pub download_href: String,
    pub return_href: String,
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
    pub approval_reward_enabled: bool,
    pub approval_reward_amount: i64,
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

#[derive(Clone, Debug)]
pub struct NovelChapterPreviewView {
    pub novel_title: String,
    pub chapter_title: String,
    pub chapter_number: i64,
    pub updated_at: String,
    pub href: String,
}

#[derive(Clone, Debug)]
pub struct NovelView {
    pub id: String,
    pub title: String,
    pub updated_at: String,
    pub chapters: Vec<NovelChapterView>,
    pub has_chapters: bool,
}

#[derive(Clone, Debug)]
pub struct NovelChapterView {
    pub id: String,
    pub title: String,
    pub updated_at: String,
    pub href: String,
}

#[derive(Clone, Debug)]
pub struct NovelChapterCommentView {
    pub id: String,
    pub body: String,
    pub created_at: String,
    pub can_delete: bool,
}

#[derive(Template)]
#[template(path = "novels.html")]
pub struct NovelsTemplate {
    pub ctx: PageContext,
    pub novels: Vec<NovelView>,
    pub has_novels: bool,
}

#[derive(Template)]
#[template(path = "novel_detail.html")]
pub struct NovelDetailTemplate {
    pub ctx: PageContext,
    pub novel: NovelView,
}

#[derive(Template)]
#[template(path = "novel_chapter.html")]
pub struct NovelChapterTemplate {
    pub ctx: PageContext,
    pub novel_id: String,
    pub chapter_id: String,
    pub novel_title: String,
    pub novel_href: String,
    pub title: String,
    pub chapter_number: i64,
    pub html: String,
    pub has_previous_chapter: bool,
    pub previous_chapter_href: String,
    pub previous_chapter_title: String,
    pub has_next_chapter: bool,
    pub next_chapter_href: String,
    pub next_chapter_title: String,
    pub comments: Vec<NovelChapterCommentView>,
    pub has_comments: bool,
    pub authenticated: bool,
    pub comment_max_length: usize,
    pub return_to: String,
}

#[derive(Template)]
#[template(path = "admin_novels.html")]
pub struct AdminNovelsTemplate {
    pub ctx: PageContext,
    pub novels: Vec<NovelView>,
    pub has_novels: bool,
    pub max_upload_kib: usize,
    pub max_title_length: usize,
    pub max_chapter_title_length: usize,
}

#[derive(Template)]
#[template(path = "profile.html")]
pub struct ProfileTemplate {
    pub ctx: PageContext,
    pub user_id: String,
    pub username: String,
    pub nickname: String,
    pub role_label: &'static str,
    pub currency_name: String,
    pub currency_symbol: String,
    pub currency_balance: i64,
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
    pub meme_current_page: i64,
    pub meme_total_pages: i64,
    pub meme_previous_page: i64,
    pub has_meme_previous_page: bool,
    pub meme_next_page: i64,
    pub has_meme_next_page: bool,
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
    pub must_change_password: bool,
}

#[derive(Template)]
#[template(path = "admin_users.html")]
pub struct AdminUsersTemplate {
    pub ctx: PageContext,
    pub users: Vec<AdminUserView>,
    pub has_message: bool,
    pub message: String,
}

#[derive(Clone, Debug)]
pub struct ShopProductView {
    pub id: String,
    pub name: String,
    pub description: String,
    pub icon_url: String,
    pub price: i64,
    pub valid_days_label: String,
    pub max_active_per_user: i64,
    pub purchase_key: String,
    pub can_purchase: bool,
    pub disabled_reason: String,
}

#[derive(Template)]
#[template(path = "shop.html")]
pub struct ShopTemplate {
    pub ctx: PageContext,
    pub products: Vec<ShopProductView>,
    pub has_products: bool,
    pub current_page: i64,
    pub total_pages: i64,
    pub previous_page: i64,
    pub has_previous_page: bool,
    pub next_page: i64,
    pub has_next_page: bool,
}

#[derive(Template)]
#[template(path = "voucher_reveal.html")]
pub struct VoucherRevealTemplate {
    pub ctx: PageContext,
    pub product_name: String,
    pub plaintext_token: String,
    pub expires_at: String,
    pub has_expiration: bool,
}

#[derive(Clone, Debug)]
pub struct VoucherView {
    pub product_name: String,
    pub product_description: String,
    pub icon_url: String,
    pub token_mask: String,
    pub status_label: &'static str,
    pub expires_at: String,
    pub has_expiration: bool,
    pub created_at: String,
}

#[derive(Template)]
#[template(path = "vouchers.html")]
pub struct VouchersTemplate {
    pub ctx: PageContext,
    pub vouchers: Vec<VoucherView>,
    pub has_vouchers: bool,
    pub has_already_message: bool,
    pub current_page: i64,
    pub total_pages: i64,
    pub previous_page: i64,
    pub has_previous_page: bool,
    pub next_page: i64,
    pub has_next_page: bool,
}

#[derive(Clone, Debug)]
pub struct AdminVoucherView {
    pub id: String,
    pub product_name: String,
    pub product_description: String,
    pub token_mask: String,
    pub status_label: &'static str,
    pub expires_at: String,
    pub has_expiration: bool,
    pub buyer_nickname: String,
    pub buyer_username: String,
}

#[derive(Template)]
#[template(path = "admin_vouchers.html")]
pub struct AdminVouchersTemplate {
    pub ctx: PageContext,
    pub result: Option<AdminVoucherView>,
    pub has_not_found: bool,
    pub note_max_length: usize,
}
