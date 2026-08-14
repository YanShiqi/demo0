# Task 5 报告

## 完成内容

- `shop::purchase` 改为接收 `product_id`，在购买事务内重新读取当前数据库商品，并保留订单商品快照与购买幂等键。
- 新增 `store::increment_product_sales_if_available`：`total_limit IS NULL` 时不限售，否则通过条件更新原子增加 `sold_count`，避免并发超卖。
- 销量、余额、订单、兑换凭证和凭证审计均在同一事务中提交；货币不足、凭证写入失败或其他错误会整体回滚。
- `/shop` 每次从数据库读取启用商品并分页，结合用户有效凭证数量展示余额不足、个人上限和售罄状态。
- 删除 TOML 商品运行时加载、旧配置字段和 `content/shop.toml`；保留图标路由所需的安全媒体类型校验。

## 验证

- `cargo test`
- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `git diff --check`
