use std::path::Path;
use tempfile::TempDir;
use tokio::fs;
use turbomcp_security::*;

#[tokio::main]
async fn main() {
    let temp_dir = TempDir::new().unwrap();
    let validator = FileSecurityValidator::new();

    println!("Temp dir: {:?}", temp_dir.path());
    
    // Create test file
    let test_file = temp_dir.path().join("mmap_test.dat");
    let test_data = vec![b'M'; 1024]; // 1KB file
    fs::write(&test_file, test_data).await.unwrap();

    println!("Test file: {:?}", test_file);

    // Valid mmap access should succeed
    let result = validator
        .validate_mmap_access(&test_file, 0, Some(512))
        .await;
        
    match result {
        Ok((safe_path, offset, length)) => {
            println!("Success: path={:?}, offset={}, length={}", safe_path, offset, length);
        }
        Err(e) => {
            println!("Error: {:?}", e);
        }
    }
}
