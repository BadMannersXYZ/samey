use std::process::{Command, Stdio};

use crate::SameyError;

pub(crate) fn generate_thumbnail(
    input_path: &str,
    output_path: &str,
    max_thumbnail_dimension: u32,
) -> Result<(), SameyError> {
    let status = Command::new("ffmpeg")
        .args([
            "-i",
            input_path,
            "-vf",
            "thumbnail",
            "-vf",
            &format!(
                "scale={}:{}:force_original_aspect_ratio=decrease",
                max_thumbnail_dimension, max_thumbnail_dimension
            ),
            "-frames:v",
            "1",
            "-q:v",
            "2", // Quality (2 is good)
            output_path,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .status()?;

    if status.success() {
        Ok(())
    } else {
        Err(SameyError::Other(
            "FFmpeg failed to generate thumbnail".into(),
        ))
    }
}

pub(crate) fn get_dimensions_for_video(input_path: &str) -> Result<(u32, u32), SameyError> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height",
            "-of",
            "default=nw=1:nk=1",
            input_path,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;

    if !output.status.success() {
        return Err(SameyError::Other(
            "FFprobe failed to get dimensions for video".into(),
        ));
    }

    let output_str = String::from_utf8_lossy(&output.stdout);

    let mut dimensions = output_str
        .lines()
        .filter_map(|line| line.trim().parse().ok());

    match (dimensions.next(), dimensions.next()) {
        (Some(width), Some(height)) => Ok((width, height)),
        _ => Err(SameyError::Other("Failed to parse FFprobe output".into())),
    }
}
