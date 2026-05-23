# DeepSeek TUI 开发经验指南

## 命令重命名

重命名一个斜杠命令（例如 `/agent` → `/role`）涉及以下文件：

| 文件 | 变更项 |
|------|--------|
| `crates/tui/src/commands/mod.rs` | `CommandInfo.name`、`aliases`、`usage`、`description_id`；dispatch match 分支 |
| `crates/tui/src/commands/<cmd>.rs` | 模块文档注释、help/list 输出文本中的命令引用 |
| `crates/tui/src/localization.rs` | `MessageId` 枚举变体名（如有 semantic 含义变化则重命名）；各语言描述文本 |
| `crates/tui/src/tui/app.rs` | 字段文档注释中的命令引用 |
| `crates/tui/src/prompts.rs` | Prompt 构建注释中的命令引用 |
| 搜索 `/old-command` | `grep_files` 全仓库搜索，处理所有文本引用 |

### 原则

1. **别名保持语义一致**：中文别名和中文化名应同步更新（`"智能体"` → `"角色"`，`"daili"` → `"juese"`）
2. **MessageId 跟随语义**：如果命令语义变了（不只是改名），enum 变体也重命名；否则可以只改文本
3. **usage 字符串同时更新**：`CommandInfo.usage` 在 `/help` 输出中直接显示
4. **测试中的命令字符串**：`execute("/agent ...")` 需要同步更新

---

## 内置主题删除

删除一个内置主题（如 Grayscale、Catppuccin Mocha）的影响范围：

### 核心结构 (theme.rs)

| 位置 | 变更 |
|------|------|
| `ThemeId` 枚举 | 移除对应变体 |
| `ThemeId::from_name()` | 移除 match 臂 |
| `ThemeId::name()` | 移除 match 臂 |
| `ThemeId::display_name()` | 移除 match 臂 |
| `ThemeId::tagline()` | 移除 match 臂 |
| `ThemeId::ui_theme()` | 移除 match 臂 |
| `SELECTABLE_THEMES` 常量 | 移除条目 |
| 主题常量 (`*_THEME`) | 整块删除 |

### PaletteMode (palette.rs)

如果删除的主题是唯一的 `Grayscale` 变体使用者：

| 位置 | 变更 |
|------|------|
| `PaletteMode` 枚举 | 移除 `Grayscale` 变体 |
| `theme_label_for_mode()` | 移除对应 arm |
| `normalize_theme_name()` | 移除对应别名 |
| `adapt_fg_for_palette_mode()` | 移除对应 arm |
| `adapt_bg_for_palette_mode()` | 移除对应 arm |
| 专用适配函数 | 删除（如 `adapt_fg_for_grayscale_palette`） |
| 颜色常量 | 删除 RGB + Color 两套（如 `GRAYSCALE_SURFACE_RGB` + `GRAYSCALE_SURFACE`） |

### 社区主题 remap 层

如果所有社区主题（Catppuccin、Tokyo Night 等）都被删除：

| 位置 | 变更 |
|------|------|
| `is_community_preset()` | 删除方法 |
| `theme_id()` | 删除方法（仅被 community preset 函数调用） |
| `adapt_fg_for_theme()` | 简化为 `color` 直通 |
| `adapt_bg_for_theme()` | 简化为 `color` 直通 |
| `theme_red/green/diff_*_bg()` | 删除（仅被 remap 层调用） |

### 测试更新

| 文件 | 典型变更 |
|------|----------|
| `tui/color_compat.rs` | 删除 Grayscale/community 主题的 cell 测试 |
| `commands/config.rs` | 将 `theme("grayscale")` 测试改为 `theme("dark")` |
| `palette.rs` tests | 删除 Grayscale 专项测试；更新 `normalize_theme_name` 测试 |
| `theme.rs` tests | 删除 `grayscale_theme_uses_neutral_tokens` 等测试 |
| `settings.rs` | 更新 `theme_normalizes_*`、`tui_prefs_validate_*` 测试的预期值 |

### 清理顺序建议

1. 先删 `ThemeId` 变体 → 编译器会标出所有 match 非穷尽错误
2. 逐个修复 match 臂，直到编译通过
3. 删主题常量 → 编译器标出 `SELECTABLE_THEMES` 和 `ui_theme()` 引用
4. 删 `PaletteMode` 变体 → 编译器标出所有 `Grayscale` 引用
5. 删专用适配函数和颜色常量
6. 最后更新测试，确保 `cargo test` 通过

---

## 测试修复策略

### 预存在错误的识别

在修改代码后运行 `cargo test --all-features --no-run` 时，可能遇到：
- 测试代码引用了已删除的 enum 变体 → **你的变更引起**
- 测试代码缺少 struct 字段 → **预存在**（生产代码签名已改，测试未跟进）
- 测试代码缺少函数参数 → **预存在**（同上）

### 批量修复预存在错误

对于测试代码中的签名不匹配，常见模式：

1. **struct 字段缺失**：在构造处补充 `field_name: default_value`
2. **函数参数缺失**：在调用处补充缺失参数，通常为 `&HashMap::new()` 或 `&[]`
3. **断言值过时**：更新为当前行为的预期值

Python 脚本可辅助批量修复，但需注意：
- 只修复测试代码（`#[cfg(test)]` 块内）
- 不要修改生产代码的调用

### 依赖链对渲染测试的影响

sidebar 的 `work_panel` 渲染测试对 checklist item 的 `depends_on` 字段敏感。
错误的依赖关系会改变缩进层级，导致截断测试失败。测试数据中的
`depends_on` 应设为空 `vec![]`，除非专门测试依赖链渲染。

---

## 开发工作流

```bash
# 1. 代码修改后，先快速编译验证
cargo check --workspace --all-features

# 2. 确保无警告（尤其是 dead_code、unused_imports）
cargo clippy --workspace --all-features 2>&1 | grep warning | wc -l

# 3. 运行完整测试套件
cargo test --workspace --all-features

# 4. 仅运行变更相关 crate 的测试（更快）
cargo test -p deepseek-tui --all-features

# 5. 提交（语义化 commit message）
git add -A
git commit -m 'feat(scope): imperative summary'
```

### 提交信息规范

遵循 Conventional Commits：
- `feat(scope):` — 新功能
- `fix(scope):` — Bug 修复
- `refactor(scope):` — 无行为变化的重构
- scope 示例：`commands`, `theme`, `localization`, `palette`

### 删除代码时的额外检查

- `grep_files` 搜索被删除的标识符，确认无遗漏引用
- 检查 `pub` 导出的符号 — 删除它们时需同时修 `pub use` 重导出
- 删除大量代码后运行 `cargo clippy` 清理 `use` 导入

---

## 文件关系速查

```
/commands 路由层
├── mod.rs          — CommandInfo 注册表 + execute() dispatch
├── agent.rs        — 各命令的实现
├── config.rs       — /config、/theme 等设置命令
└── ...

/                  — 全局引用
├── localization.rs — 所有面向用户的字符串（6 种语言）
├── prompts.rs      — LLM 系统提示构建
└── settings.rs     — 持久化配置验证

/tui
├── app.rs          — App 状态（active_agent_type 等）
├── theme.rs        — ThemeId、Theme struct、所有主题常量
├── color_compat.rs — 终端颜色适配层
├── sidebar.rs      — 侧边栏 Work 面板渲染
└── widgets/        — 各 UI 组件的具体绘制

/palette.rs         — 颜色常量、PaletteMode、adapt_* 函数
```
