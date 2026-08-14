use std::io::Cursor;

use gif::{ColorOutput, DecodeOptions, Repeat as GifRepeat};
use image::{
    AnimationDecoder, Delay, DynamicImage, Frame, GenericImageView, ImageDecoder, ImageEncoder,
    ImageFormat, ImageReader,
    codecs::{
        gif::{GifDecoder, GifEncoder, Repeat},
        webp::WebPEncoder,
    },
    imageops::FilterType,
};

use crate::{config::ShopConfig, error::AppError};

#[derive(Debug)]
pub struct ProcessedIcon {
    pub bytes: Vec<u8>,
    pub extension: &'static str,
    pub media_type: &'static str,
    pub width: u32,
    pub height: u32,
    pub frame_count: usize,
}

pub struct IconProcessor;

impl IconProcessor {
    /// 校验并压缩商品图标；结果只包含可信格式元数据，不接收或返回上传文件名。
    pub fn process(bytes: Vec<u8>, config: &ShopConfig) -> Result<ProcessedIcon, AppError> {
        if bytes.is_empty() || bytes.len() > config.icon_upload_max_bytes {
            return Err(AppError::BadRequest(format!(
                "商品图标上传大小不能超过 {} 字节",
                config.icon_upload_max_bytes
            )));
        }

        let format = image::guess_format(&bytes)
            .map_err(|_| AppError::BadRequest("无法识别商品图标格式".to_owned()))?;
        match format {
            ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP => {
                process_static(&bytes, format, config)
            }
            ImageFormat::Gif => process_gif(&bytes, config),
            _ => Err(AppError::BadRequest(
                "商品图标仅支持 PNG、JPEG、WebP 或 GIF".to_owned(),
            )),
        }
    }
}

fn process_static(
    bytes: &[u8],
    format: ImageFormat,
    config: &ShopConfig,
) -> Result<ProcessedIcon, AppError> {
    let (width, height) = ImageReader::with_format(Cursor::new(bytes), format)
        .into_dimensions()
        .map_err(|_| AppError::BadRequest("商品图标已损坏或格式不正确".to_owned()))?;
    validate_input_dimensions(width, height, config)?;
    let image = image::load_from_memory_with_format(bytes, format)
        .map_err(|_| AppError::BadRequest("商品图标已损坏或格式不正确".to_owned()))?;

    for candidate in &config.icon_resize_dimensions {
        let resized = resize_static(&image, *candidate);
        let (output_width, output_height) = resized.dimensions();
        let output = encode_webp(&resized)?;
        if output.len() <= config.icon_max_stored_bytes {
            return Ok(ProcessedIcon {
                bytes: output,
                extension: "webp",
                media_type: "image/webp",
                width: output_width,
                height: output_height,
                frame_count: 1,
            });
        }
    }

    Err(output_too_large(config))
}

fn process_gif(bytes: &[u8], config: &ShopConfig) -> Result<ProcessedIcon, AppError> {
    let repeat = read_gif_repeat(bytes)?;
    let decoder = GifDecoder::new(Cursor::new(bytes))
        .map_err(|_| AppError::BadRequest("GIF 已损坏或格式不正确".to_owned()))?;
    let (width, height) = decoder.dimensions();
    validate_input_dimensions(width, height, config)?;

    let mut frames = Vec::new();
    for decoded in decoder.into_frames() {
        if frames.len() >= config.icon_max_gif_frames {
            return Err(AppError::BadRequest(format!(
                "GIF 不能超过 {} 帧",
                config.icon_max_gif_frames
            )));
        }
        let frame = decoded.map_err(|_| AppError::BadRequest("GIF 包含无法解码的帧".to_owned()))?;
        frames.push(frame);
        validate_decoded_pixels(width, height, frames.len(), config)?;
    }
    if frames.is_empty() {
        return Err(AppError::BadRequest("GIF 不包含有效画面".to_owned()));
    }

    for candidate in &config.icon_resize_dimensions {
        let (output_width, output_height) = target_dimensions(width, height, *candidate);
        let output = encode_gif(&frames, output_width, output_height, repeat)?;
        if output.len() <= config.icon_max_stored_bytes {
            return Ok(ProcessedIcon {
                bytes: output,
                extension: "gif",
                media_type: "image/gif",
                width: output_width,
                height: output_height,
                frame_count: frames.len(),
            });
        }
    }

    // 动图无法满足体积限制时明确拒绝，避免悄悄冻结为首帧造成误解。
    Err(output_too_large(config))
}

fn validate_input_dimensions(width: u32, height: u32, config: &ShopConfig) -> Result<(), AppError> {
    if width == 0
        || height == 0
        || width > config.icon_input_max_dimension
        || height > config.icon_input_max_dimension
    {
        return Err(AppError::BadRequest(format!(
            "商品图标尺寸不能超过 {}×{}",
            config.icon_input_max_dimension, config.icon_input_max_dimension
        )));
    }
    Ok(())
}

fn validate_decoded_pixels(
    width: u32,
    height: u32,
    frame_count: usize,
    config: &ShopConfig,
) -> Result<(), AppError> {
    let decoded_pixels = u64::from(width)
        .saturating_mul(u64::from(height))
        .saturating_mul(frame_count as u64);
    if decoded_pixels > config.icon_max_decoded_pixels {
        return Err(AppError::BadRequest(
            "GIF 解码后的总像素过大，请减少尺寸或帧数".to_owned(),
        ));
    }
    Ok(())
}

fn resize_static(image: &DynamicImage, candidate: u32) -> DynamicImage {
    let (width, height) = image.dimensions();
    let (target_width, target_height) = target_dimensions(width, height, candidate);
    if (target_width, target_height) == (width, height) {
        image.clone()
    } else {
        image.resize_exact(target_width, target_height, FilterType::Lanczos3)
    }
}

fn target_dimensions(width: u32, height: u32, maximum: u32) -> (u32, u32) {
    if width <= maximum && height <= maximum {
        return (width, height);
    }
    if width >= height {
        let target_height =
            (u64::from(height) * u64::from(maximum) / u64::from(width)).max(1) as u32;
        (maximum, target_height)
    } else {
        let target_width =
            (u64::from(width) * u64::from(maximum) / u64::from(height)).max(1) as u32;
        (target_width, maximum)
    }
}

fn encode_webp(image: &DynamicImage) -> Result<Vec<u8>, AppError> {
    let rgba = image.to_rgba8();
    let mut output = Vec::new();
    WebPEncoder::new_lossless(&mut output)
        .write_image(
            rgba.as_raw(),
            rgba.width(),
            rgba.height(),
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|error| AppError::Internal(format!("商品图标 WebP 编码失败：{error}")))?;
    Ok(output)
}

fn read_gif_repeat(bytes: &[u8]) -> Result<Repeat, AppError> {
    let mut options = DecodeOptions::new();
    options.set_color_output(ColorOutput::RGBA);
    let decoder = options
        .read_info(Cursor::new(bytes))
        .map_err(|_| AppError::BadRequest("GIF 已损坏或格式不正确".to_owned()))?;
    Ok(match decoder.repeat() {
        GifRepeat::Finite(count) => Repeat::Finite(count),
        GifRepeat::Infinite => Repeat::Infinite,
    })
}

fn encode_gif(
    frames: &[Frame],
    width: u32,
    height: u32,
    repeat: Repeat,
) -> Result<Vec<u8>, AppError> {
    let mut output = Vec::new();
    {
        let mut encoder = GifEncoder::new(&mut output);
        encoder
            .set_repeat(repeat)
            .map_err(|error| AppError::Internal(format!("GIF 循环设置失败：{error}")))?;
        for frame in frames {
            let resized = if frame.buffer().dimensions() == (width, height) {
                frame.buffer().clone()
            } else {
                image::imageops::resize(frame.buffer(), width, height, FilterType::Lanczos3)
            };
            encoder
                .encode_frame(Frame::from_parts(
                    resized,
                    0,
                    0,
                    preserve_delay(frame.delay()),
                ))
                .map_err(|error| AppError::Internal(format!("GIF 重新编码失败：{error}")))?;
        }
    }
    Ok(output)
}

fn preserve_delay(delay: Delay) -> Delay {
    let (numerator, denominator) = delay.numer_denom_ms();
    Delay::from_numer_denom_ms(numerator, denominator)
}

fn output_too_large(config: &ShopConfig) -> AppError {
    AppError::BadRequest(format!(
        "商品图标处理后体积仍超过 {} 字节，请降低图片复杂度或尺寸",
        config.icon_max_stored_bytes
    ))
}

#[cfg(test)]
mod tests {
    use std::{io::Cursor, path::PathBuf};

    use gif::{ColorOutput, DecodeOptions, Repeat as GifRepeat};
    use image::{
        AnimationDecoder, Delay, DynamicImage, Frame, GenericImageView, ImageBuffer, ImageFormat,
        Rgba, RgbaImage,
        codecs::gif::{GifDecoder, GifEncoder, Repeat},
    };

    use super::*;

    #[test]
    fn static_image_is_resized_with_aspect_ratio_and_encoded_as_webp() {
        let config = test_config();
        let bytes = encode_static(800, 400, ImageFormat::Png, Rgba([10, 20, 30, 64]));

        let processed = IconProcessor::process(bytes, &config).unwrap();

        assert_eq!(processed.extension, "webp");
        assert_eq!(processed.media_type, "image/webp");
        assert_eq!((processed.width, processed.height), (512, 256));
        assert_eq!(processed.frame_count, 1);
        let decoded = image::load_from_memory_with_format(&processed.bytes, ImageFormat::WebP)
            .expect("输出应为可解码的 WebP");
        assert_eq!(decoded.dimensions(), (512, 256));
        assert_eq!(decoded.to_rgba8().get_pixel(0, 0).0[3], 64);
    }

    #[test]
    fn gif_preserves_frames_delays_and_infinite_repeat() {
        let config = test_config();
        let bytes = encode_gif(32, 16, &[70, 130], Repeat::Infinite);

        let processed = IconProcessor::process(bytes, &config).unwrap();

        assert_eq!(processed.extension, "gif");
        assert_eq!(processed.media_type, "image/gif");
        assert_eq!(processed.frame_count, 2);
        assert_eq!((processed.width, processed.height), (32, 16));
        let decoder = GifDecoder::new(Cursor::new(&processed.bytes)).unwrap();
        let delays = decoder
            .into_frames()
            .map(|frame| frame.unwrap().delay().numer_denom_ms().0)
            .collect::<Vec<_>>();
        assert_eq!(delays, vec![70, 130]);

        let mut options = DecodeOptions::new();
        options.set_color_output(ColorOutput::RGBA);
        let decoder = options.read_info(Cursor::new(&processed.bytes)).unwrap();
        assert_eq!(decoder.repeat(), GifRepeat::Infinite);
    }

    #[test]
    fn gif_without_repeat_extension_stays_non_looping() {
        let config = test_config();
        let bytes = encode_gif_without_repeat(8, 8, &[50, 80]);
        assert!(!contains_netscape_repeat_extension(&bytes));

        let processed = IconProcessor::process(bytes, &config).unwrap();

        assert!(!contains_netscape_repeat_extension(&processed.bytes));
    }

    #[test]
    fn rejects_input_larger_than_upload_limit_before_format_detection() {
        let mut config = test_config();
        config.icon_upload_max_bytes = 8;

        let error = IconProcessor::process(vec![0; 9], &config).unwrap_err();

        assert!(error.to_string().contains("上传大小"));
    }

    #[test]
    fn rejects_unsupported_actual_format_even_with_valid_image_bytes() {
        let config = test_config();
        let bytes = b"BM\0\0\0\0\0\0\0\0".to_vec();

        let error = IconProcessor::process(bytes, &config).unwrap_err();

        assert!(error.to_string().contains("PNG、JPEG、WebP 或 GIF"));
    }

    #[test]
    fn rejects_input_dimensions_before_full_processing() {
        let mut config = test_config();
        config.icon_input_max_dimension = 16;
        let bytes = encode_static(17, 8, ImageFormat::Png, Rgba([0, 0, 0, 255]));

        let error = IconProcessor::process(bytes, &config).unwrap_err();

        assert!(error.to_string().contains("16×16"));
    }

    #[test]
    fn rejects_gif_above_frame_limit() {
        let mut config = test_config();
        config.icon_max_gif_frames = 1;
        let bytes = encode_gif(2, 2, &[50, 50], Repeat::Finite(2));

        let error = IconProcessor::process(bytes, &config).unwrap_err();

        assert!(error.to_string().contains("1 帧"));
    }

    #[test]
    fn rejects_gif_above_decoded_pixel_budget() {
        let mut config = test_config();
        config.icon_max_decoded_pixels = 7;
        let bytes = encode_gif(2, 2, &[50, 50], Repeat::Finite(2));

        let error = IconProcessor::process(bytes, &config).unwrap_err();

        assert!(error.to_string().contains("总像素"));
    }

    #[test]
    fn retries_smaller_configured_candidate_when_output_is_too_large() {
        let mut config = test_config();
        config.icon_resize_dimensions = vec![64, 8];
        config.icon_max_stored_bytes = 400;
        let bytes = encode_noise_png(128, 128);

        let processed = IconProcessor::process(bytes, &config).unwrap();

        assert_eq!((processed.width, processed.height), (8, 8));
        assert!(processed.bytes.len() <= 400);
    }

    #[test]
    fn rejects_when_smallest_candidate_still_exceeds_output_limit() {
        let mut config = test_config();
        config.icon_resize_dimensions = vec![16, 8];
        config.icon_max_stored_bytes = 1;
        let bytes = encode_static(32, 32, ImageFormat::Png, Rgba([5, 10, 15, 255]));

        let error = IconProcessor::process(bytes, &config).unwrap_err();

        assert!(error.to_string().contains("处理后体积"));
    }

    fn test_config() -> ShopConfig {
        ShopConfig {
            enabled: true,
            products_file: PathBuf::from("content/shop.toml"),
            icon_dir: PathBuf::from("data/shop/product-icons"),
            page_size: 12,
            voucher_page_size: 20,
            admin_note_max_length: 200,
            token_lookup_max_attempts: 20,
            token_lookup_window_seconds: 60,
            icon_upload_max_bytes: 5 * 1024 * 1024,
            icon_input_max_dimension: 4096,
            icon_max_gif_frames: 120,
            icon_max_decoded_pixels: 80_000_000,
            icon_max_stored_bytes: 1024 * 1024,
            icon_resize_dimensions: vec![512, 384, 256],
            icon_max_bytes: 256 * 1024,
            icon_max_dimension: 1024,
            products: Vec::new(),
        }
    }

    fn encode_static(width: u32, height: u32, format: ImageFormat, pixel: Rgba<u8>) -> Vec<u8> {
        let image = DynamicImage::ImageRgba8(ImageBuffer::from_pixel(width, height, pixel));
        let mut output = Cursor::new(Vec::new());
        image.write_to(&mut output, format).unwrap();
        output.into_inner()
    }

    fn encode_noise_png(width: u32, height: u32) -> Vec<u8> {
        let image = RgbaImage::from_fn(width, height, |x, y| {
            Rgba([
                x.wrapping_mul(37).wrapping_add(y.wrapping_mul(17)) as u8,
                x.wrapping_mul(11).wrapping_add(y.wrapping_mul(43)) as u8,
                x.wrapping_mul(61).wrapping_add(y.wrapping_mul(7)) as u8,
                255,
            ])
        });
        let mut output = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(image)
            .write_to(&mut output, ImageFormat::Png)
            .unwrap();
        output.into_inner()
    }

    fn encode_gif(width: u32, height: u32, delays_ms: &[u32], repeat: Repeat) -> Vec<u8> {
        let mut output = Vec::new();
        {
            let mut encoder = GifEncoder::new(&mut output);
            encoder.set_repeat(repeat).unwrap();
            for (index, delay) in delays_ms.iter().enumerate() {
                let buffer =
                    RgbaImage::from_pixel(width, height, Rgba([index as u8 * 80, 64, 128, 255]));
                encoder
                    .encode_frame(Frame::from_parts(
                        buffer,
                        0,
                        0,
                        Delay::from_numer_denom_ms(*delay, 1),
                    ))
                    .unwrap();
            }
        }
        output
    }

    fn encode_gif_without_repeat(width: u32, height: u32, delays_ms: &[u32]) -> Vec<u8> {
        let mut output = Vec::new();
        {
            let mut encoder = GifEncoder::new(&mut output);
            for (index, delay) in delays_ms.iter().enumerate() {
                let buffer =
                    RgbaImage::from_pixel(width, height, Rgba([index as u8 * 80, 64, 128, 255]));
                encoder
                    .encode_frame(Frame::from_parts(
                        buffer,
                        0,
                        0,
                        Delay::from_numer_denom_ms(*delay, 1),
                    ))
                    .unwrap();
            }
        }
        output
    }

    fn contains_netscape_repeat_extension(bytes: &[u8]) -> bool {
        bytes
            .windows(b"NETSCAPE2.0".len())
            .any(|window| window == b"NETSCAPE2.0")
    }
}
