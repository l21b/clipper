# Clipper

一个基于 Rust + Tauri + Svelte 的轻量剪贴板历史工具，专注文本工作流：快速呼出、快速搜索、快速粘贴。

## 功能特性

- 全局快捷键呼出（默认 `Ctrl+Shift+V`，支持设置修改）
- 自动监听文本剪贴板并写入本地历史
- 历史/收藏双视图，支持搜索
- 文本去重（命中时刷新时间，不重复堆积）
- 记录置顶、收藏、删除、清空
- 收藏 JSON 导入/导出（导入为增量）
- 托盘运行（设置 / 关于 / 退出）
- 支持浅色/深色主题，滚动条风格随主题适配

## 当前版本说明（v1）

- 当前版本以文本链路为主，图片记录与图片回贴暂未开放
- 主窗口默认不在任务栏显示，使用托盘与快捷键交互

## 技术栈

- Rust
- Tauri 2.x
- Svelte 5
- SQLite（rusqlite）
- arboard
- enigo

## 本地开发

```bash
# 安装依赖
npm install

# 启动开发模式
npm run tauri dev
```

## 打包发布

```bash
npm run tauri build
```

## 作者

Jiaxin
