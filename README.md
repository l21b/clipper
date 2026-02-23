# Clipper

一个基于 Rust + Tauri + Svelte 的轻量剪贴板历史工具，专注文本工作流：快速呼出、快速搜索、快速粘贴。

## 功能特性

- 全局快捷键呼出（默认 `Ctrl+Shift+V`，支持设置修改）
- 自动监听文本剪贴板并写入本地历史
- 历史/收藏双视图，支持搜索
- 文本去重
- 记录置顶、收藏、删除、清空
- 收藏导入/导出（JSON）
- 界面跟随鼠标位置
- 支持浅色/深色主题

## 当前版本说明

- 当前版本以文本链路为主，图片记录与图片回贴暂未开放

## 技术栈

- Rust
- Tauri 2.x
- Svelte 5
- SQLite（rusqlite）
- arboard
- enigo
- clipboard-master





