// SPDX-License-Identifier: Apache-2.0
//! Extension classification for images, SVG, video, audio and fonts.
//! Implements the pack rule that these types are omitted (SVG as its own case).
//! Depends on [`super::FileContentHandling`]; pure over a lowercased extension.
//! Used by [`super::classify_file`] before content sniffing.
//! Unknown extensions return `None` so the caller can try packages, then bytes.

use super::FileContentHandling;

/// Classification for a lowercased media extension, or `None` when it is not one of these.
pub(super) fn classify(ext: &str) -> Option<FileContentHandling> {
    let handling = match ext {
        "png" => "PNG image",
        "jpg" | "jpeg" => "JPEG image",
        "gif" => "GIF image",
        "bmp" => "BMP image",
        "ico" => "ICO icon",
        "webp" => "WebP image",
        "avif" => "AVIF image",
        "tiff" | "tif" => "TIFF image",
        "mp4" => "MP4 video",
        "mov" => "QuickTime video",
        "avi" => "AVI video",
        "mkv" => "Matroska video",
        "webm" => "WebM video",
        "flv" => "Flash video",
        "wmv" => "Windows Media video",
        "m4v" => "M4V video",
        "mp3" => "MP3 audio",
        "wav" => "WAV audio",
        "ogg" => "OGG audio",
        "flac" => "FLAC audio",
        "aac" => "AAC audio",
        "m4a" => "M4A audio",
        "wma" => "WMA audio",
        "ttf" => "TrueType font",
        "otf" => "OpenType font",
        "woff" | "woff2" => "Web font",
        "eot" => "EOT font",
        "svg" => return Some(FileContentHandling::OmitSvgContent),
        _ => return None,
    };
    Some(FileContentHandling::OmitBinary {
        file_type: handling.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_svg_and_unknown_extensions() {
        assert_eq!(
            classify("png"),
            Some(FileContentHandling::OmitBinary {
                file_type: "PNG image".to_string()
            })
        );
        assert_eq!(classify("svg"), Some(FileContentHandling::OmitSvgContent));
        assert_eq!(classify("rs"), None);
    }
}
