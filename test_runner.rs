#!/usr/bin/env rust-script

//! This is a test verification script
//! ```cargo
//! [dependencies]
//! ```

use std::process::Command;

fn main() {
    println!("=== WS3 Test Verification ===\n");
    
    // Build tests
    println!("Building test binary...");
    let build = Command::new("cargo")
        .args(&["test", "--no-run", "--message-format=short"])
        .current_dir("d:\\by\\polap-db\\services\\voltnuerongridd")
        .output();
    
    match build {
        Ok(output) => {
            println!("Build stdout: {}", String::from_utf8_lossy(&output.stdout));
            if !output.status.success() {
                println!("Build stderr: {}", String::from_utf8_lossy(&output.stderr));
                println!("Build exit code: {:?}", output.status.code());
            }
            println!("Build status: {}", if output.status.success() { "SUCCESS" } else { "FAILED" });
        }
        Err(e) => println!("Build error: {}", e),
    }
    
    println!("\n=== Listing test functions ===");
    let list = Command::new("cargo")
        .args(&["test", "--", "--list"])
        .current_dir("d:\\by\\polap-db\\services\\voltnuerongridd")
        .output();
    
    match list {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let ws3_tests: Vec<&str> = stdout
                .lines()
                .filter(|line| line.contains("ws3_"))
                .collect();
            
            println!("Found {} WS3 tests:", ws3_tests.len());
            for test in ws3_tests {
                println!("  {}", test);
            }
        }
        Err(e) => println!("List error: {}", e),
    }
}
