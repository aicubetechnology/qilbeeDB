//! Test password validation in bootstrap

use qilbee_server::security::{UserService, BootstrapService};
use std::sync::Arc;
use std::path::PathBuf;

fn test_password(password: &str, should_succeed: bool) {
    unsafe {
        std::env::set_var("QILBEEDB_ADMIN_USERNAME", "admin");
        std::env::set_var("QILBEEDB_ADMIN_EMAIL", "admin@test.com");
        std::env::set_var("QILBEEDB_ADMIN_PASSWORD", password);
    }

    let data_dir = PathBuf::from(format!("test_pwd_{}", rand::random::<u32>()));
    std::fs::create_dir_all(&data_dir).expect("Failed to create test directory");
    let user_service = Arc::new(UserService::new());
    let bootstrap = BootstrapService::new(data_dir.clone(), user_service);

    let result = bootstrap.run_from_env();

    match (result.is_ok(), should_succeed) {
        (true, true) => println!("  ✓ PASS: '{}' correctly accepted", password),
        (false, false) => println!("  ✓ PASS: '{}' correctly rejected", password),
        (true, false) => {
            println!("  ✗ FAIL: '{}' should have been rejected but was accepted!", password);
            std::process::exit(1);
        }
        (false, true) => {
            println!("  ✗ FAIL: '{}' should have been accepted but was rejected!", password);
            if let Err(e) = result {
                println!("     Error: {}", e);
            }
            std::process::exit(1);
        }
    }

    // Cleanup
    let _ = std::fs::remove_dir_all(data_dir);
}

fn main() {
    println!("=== Password Validation Tests ===\n");

    println!("Testing VALID passwords:");
    test_password("MySecureP@ssw0rd", true);
    test_password("Adm!n2024Password", true);
    test_password("C0mplex!tyRul3s", true);
    test_password("TestSecure123!", true);
    test_password("P@ssw0rdIsStr0ng", true);

    println!("\nTesting INVALID passwords (too short):");
    test_password("Short1!", false);
    test_password("Pass123!", false);

    println!("\nTesting INVALID passwords (missing uppercase):");
    test_password("alllowercase123!", false);
    test_password("noupperletters1!", false);

    println!("\nTesting INVALID passwords (missing lowercase):");
    test_password("ALLUPPERCASE123!", false);
    test_password("NOLOWERLETTERS1!", false);

    println!("\nTesting INVALID passwords (missing digit):");
    test_password("NoDigitsHere!", false);
    test_password("OnlyLetters@nd$pecial", false);

    println!("\nTesting INVALID passwords (missing special char):");
    test_password("NoSpecialChar123", false);
    test_password("MissingSpecial456", false);

    println!("\n=== All Password Validation Tests Passed! ===");
}
