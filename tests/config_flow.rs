use std::{env, ffi::OsString, sync::Mutex};

use demo0::config::Config;

// 进程环境由全部测试共享；锁与恢复逻辑避免未来增加的环境变量测试相互污染。
static ENVIRONMENT_LOCK: Mutex<()> = Mutex::new(());

struct EnvironmentVariableRestore {
    name: &'static str,
    previous_value: Option<OsString>,
}

impl EnvironmentVariableRestore {
    fn set(name: &'static str, value: &str) -> Self {
        let previous_value = env::var_os(name);
        // Rust 2024 将修改进程环境标记为 unsafe，因此仅在持有测试锁时修改。
        unsafe { env::set_var(name, value) };
        Self {
            name,
            previous_value,
        }
    }
}

impl Drop for EnvironmentVariableRestore {
    fn drop(&mut self) {
        // 即使断言失败也恢复变量，避免影响同一测试进程后续读取的配置。
        unsafe {
            if let Some(value) = &self.previous_value {
                env::set_var(self.name, value);
            } else {
                env::remove_var(self.name);
            }
        }
    }
}

#[test]
fn config_reads_shop_resize_dimensions_from_environment() {
    let _environment_lock = ENVIRONMENT_LOCK.lock().unwrap();
    let _resize_dimensions =
        EnvironmentVariableRestore::set("SHOP_ICON_RESIZE_DIMENSIONS", "480, 320, 160");

    let config = Config::from_env().unwrap();

    assert_eq!(config.shop.icon_resize_dimensions, vec![480, 320, 160]);
}
