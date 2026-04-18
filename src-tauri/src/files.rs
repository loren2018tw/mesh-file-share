use std::path::PathBuf;

/// 取得檔案資訊（名稱、大小）
pub async fn get_file_metadata(path: &PathBuf) -> Result<(String, u64), std::io::Error> {
    let metadata = tokio::fs::metadata(path).await?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    Ok((name, metadata.len()))
}
