use std::io::Cursor;

use image::{
    AnimationDecoder, GenericImageView, ImageDecoder, ImageFormat,
    codecs::gif::{GifDecoder, GifEncoder, Repeat},
};

use crate::error::AppError;

pub const MAX_UPLOAD_BYTES: usize = 1024 * 1024;
pub const MAX_DIMENSION: u32 = 3000;
pub const MAX_GIF_FRAMES: usize = 100;
pub const MAX_DECODED_PIXELS: u64 = 50_000_000;
pub const MAX_STORED_BYTES: usize = 5 * 1024 * 1024;
pub const DEFAULT_AVATAR: &[u8] = include_bytes!("../static/images/default-avatar.png");

#[derive(Debug)]
pub struct ProcessedAvatar {
    pub bytes: Vec<u8>,
    pub extension: &'static str,
    pub media_type: &'static str,
    pub width: u32,
    pub height: u32,
    pub frame_count: usize,
}

pub fn process(bytes: Vec<u8>) -> Result<ProcessedAvatar, AppError> {
    if bytes.is_empty() || bytes.len() > MAX_UPLOAD_BYTES {
        return Err(AppError::BadRequest("头像文件必须小于 1 MiB".to_owned()));
    }

    let format = image::guess_format(&bytes)
        .map_err(|_| AppError::BadRequest("无法识别头像文件".to_owned()))?;
    let processed = match format {
        ImageFormat::Png => process_static(bytes, ImageFormat::Png, "png", "image/png"),
        ImageFormat::Jpeg => process_static(bytes, ImageFormat::Jpeg, "jpg", "image/jpeg"),
        ImageFormat::Gif => process_gif(bytes),
        _ => Err(AppError::BadRequest(
            "头像仅支持 PNG、JPEG 和 GIF".to_owned(),
        )),
    }?;
    // 高压缩图片重新编码后可能显著变大，因此还要限制最终落盘体积。
    if processed.bytes.len() > MAX_STORED_BYTES {
        return Err(AppError::BadRequest(
            "头像处理后体积过大，请降低图片复杂度或尺寸".to_owned(),
        ));
    }
    Ok(processed)
}

fn process_static(
    bytes: Vec<u8>,
    format: ImageFormat,
    extension: &'static str,
    media_type: &'static str,
) -> Result<ProcessedAvatar, AppError> {
    let image = image::load_from_memory_with_format(&bytes, format)
        .map_err(|_| AppError::BadRequest("头像图片已损坏或格式不正确".to_owned()))?;
    let (width, height) = image.dimensions();
    validate_dimensions(width, height, 1)?;

    // 重新编码会移除图片中与显示无关的附加数据，避免原文件被原样公开。
    let mut output = Cursor::new(Vec::new());
    image
        .write_to(&mut output, format)
        .map_err(|error| AppError::Internal(format!("头像重新编码失败：{error}")))?;
    Ok(ProcessedAvatar {
        bytes: output.into_inner(),
        extension,
        media_type,
        width,
        height,
        frame_count: 1,
    })
}

fn process_gif(bytes: Vec<u8>) -> Result<ProcessedAvatar, AppError> {
    let decoder = GifDecoder::new(Cursor::new(bytes))
        .map_err(|_| AppError::BadRequest("GIF 已损坏或格式不正确".to_owned()))?;
    let (width, height) = decoder.dimensions();
    validate_dimensions(width, height, 1)?;

    let mut frames = Vec::new();
    for decoded in decoder.into_frames() {
        if frames.len() >= MAX_GIF_FRAMES {
            return Err(AppError::BadRequest(format!(
                "GIF 不能超过 {MAX_GIF_FRAMES} 帧"
            )));
        }
        let frame = decoded.map_err(|_| AppError::BadRequest("GIF 包含无法解码的帧".to_owned()))?;
        frames.push(frame);
        validate_dimensions(width, height, frames.len())?;
    }
    if frames.is_empty() {
        return Err(AppError::BadRequest("GIF 不包含有效画面".to_owned()));
    }

    // GIF 重新编码时保留每帧延迟，因此动画不会被转换成静态图片。
    let frame_count = frames.len();
    let mut output = Vec::new();
    {
        let mut encoder = GifEncoder::new(&mut output);
        encoder
            .set_repeat(Repeat::Infinite)
            .map_err(|error| AppError::Internal(format!("GIF 循环设置失败：{error}")))?;
        encoder
            .encode_frames(frames)
            .map_err(|error| AppError::Internal(format!("GIF 重新编码失败：{error}")))?;
    }

    Ok(ProcessedAvatar {
        bytes: output,
        extension: "gif",
        media_type: "image/gif",
        width,
        height,
        frame_count,
    })
}

fn validate_dimensions(width: u32, height: u32, frames: usize) -> Result<(), AppError> {
    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(AppError::BadRequest(format!(
            "头像尺寸不能超过 {MAX_DIMENSION}×{MAX_DIMENSION}"
        )));
    }
    let decoded_pixels = u64::from(width)
        .saturating_mul(u64::from(height))
        .saturating_mul(frames as u64);
    if decoded_pixels > MAX_DECODED_PIXELS {
        return Err(AppError::BadRequest(
            "GIF 解码后的总像素过大，请减少尺寸或帧数".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_file_type() {
        let error = process(b"not-an-image".to_vec()).unwrap_err();
        assert!(error.to_string().contains("无法识别"));
    }

    #[test]
    fn rejects_oversized_file_before_decoding() {
        let error = process(vec![0; MAX_UPLOAD_BYTES + 1]).unwrap_err();
        assert!(error.to_string().contains("1 MiB"));
    }

    #[test]
    fn preserves_multiple_gif_frames() {
        use image::{Delay, Frame, RgbaImage};

        let frames = [
            Frame::from_parts(
                RgbaImage::new(2, 2),
                0,
                0,
                Delay::from_numer_denom_ms(80, 1),
            ),
            Frame::from_parts(
                RgbaImage::new(2, 2),
                0,
                0,
                Delay::from_numer_denom_ms(120, 1),
            ),
        ];
        let mut encoded = Vec::new();
        GifEncoder::new(&mut encoded).encode_frames(frames).unwrap();

        let processed = process(encoded).unwrap();
        assert_eq!(processed.media_type, "image/gif");
        assert_eq!(processed.frame_count, 2);
    }
}
