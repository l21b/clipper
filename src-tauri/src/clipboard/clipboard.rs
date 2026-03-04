use arboard::{Clipboard, Error, ImageData};
use std::borrow::Cow;

// ==========================================
// 数据结构定义 (隔离底层依赖)
// ==========================================

/// 纯净的图像数据结构，对上层业务屏蔽 `arboard::ImageData` 和 `Cow` 生命周期
#[derive(Debug, Clone)]
pub struct ClipboardImage {
    pub width: usize,
    pub height: usize,
    pub bytes: Vec<u8>,
}

// ==========================================
// Context 抽象 (重用剪贴板连接)
// ==========================================

/// 剪贴板上下文，封装底层句柄以支持高效的连续操作
pub struct ClipboardContext(Clipboard);

impl ClipboardContext {
    /// 创建一个新的剪贴板上下文
    pub fn new() -> Result<Self, Error> {
        Ok(Self(Clipboard::new()?))
    }

    /// 读取纯文本
    pub fn read_text(&mut self) -> Result<String, Error> {
        self.0.get_text()
    }

    /// 读取图像数据
    pub fn read_image(&mut self) -> Result<ClipboardImage, Error> {
        let image = self.0.get_image()?;
        Ok(ClipboardImage {
            width: image.width,
            height: image.height,
            bytes: image.bytes.into_owned(),
        })
    }

    /// 写入纯文本
    pub fn write_text(&mut self, text: &str) -> Result<(), Error> {
        self.0.set_text(text)
    }

    /// 写入图像数据
    pub fn write_image(
        &mut self,
        width: usize,
        height: usize,
        bytes: Vec<u8>,
    ) -> Result<(), Error> {
        self.0.set_image(ImageData {
            width,
            height,
            bytes: Cow::Owned(bytes),
        })
    }

    /// 写入富文本 (HTML)
    #[allow(dead_code)]
    pub fn write_html(&mut self, html: &str, alternate_text: &str) -> Result<(), Error> {
        self.0.set_html(html, Some(alternate_text))
    }

    /// 清空剪贴板
    #[allow(dead_code)]
    pub fn clear(&mut self) -> Result<(), Error> {
        self.0.clear()
    }
}

// ==========================================
// 便捷原子操作 (向后兼容/简单调用)
// ==========================================

#[allow(dead_code)]
pub fn read_text() -> Result<String, Error> {
    ClipboardContext::new()?.read_text()
}

#[allow(dead_code)]
pub fn read_image() -> Result<ClipboardImage, Error> {
    ClipboardContext::new()?.read_image()
}

pub fn write_text(text: &str) -> Result<(), Error> {
    ClipboardContext::new()?.write_text(text)
}

pub fn write_image(width: usize, height: usize, bytes: Vec<u8>) -> Result<(), Error> {
    ClipboardContext::new()?.write_image(width, height, bytes)
}

#[allow(dead_code)]
pub fn write_html(html: &str, alternate_text: &str) -> Result<(), Error> {
    ClipboardContext::new()?.write_html(html, alternate_text)
}

#[allow(dead_code)]
pub fn clear() -> Result<(), Error> {
    ClipboardContext::new()?.clear()
}
