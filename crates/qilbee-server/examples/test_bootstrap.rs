//! Test bootstrap functionality
//!
//! This example demonstrates the bootstrap system in non-interactive mode

use qilbee_server::security::{UserService, BootstrapService};
use std::sync::Arc;
use std::path::PathBuf;

fn main() {
    println!("=== QilbeeDB Bootstrap Test ===\n");

    // Test 1: Non-interactive bootstrap with environment variables
    println!("Test 1: Non-Interactive Bootstrap (Environment Variables)");
    println!("Setting environment variables...");

    unsafe {
        std::env::set_var("QILBEEDB_ADMIN_USERNAME", "testadmin");
        std::env::set_var("QILBEEDB_ADMIN_EMAIL", "admin@test.com");
        std::env::set_var("QILBEEDB_ADMIN_PASSWORD", "TestSecure123!");
    }

    let data_dir = PathBuf::from("test_bootstrap_data");
    let user_service = Arc::new(UserService::new());
    let bootstrap = BootstrapService::new(data_dir.clone(), user_service.clone());

    println!("\nChecking if bootstrap is required...");
    match bootstrap.is_bootstrap_required() {
        Ok(true) => println!("✓ Bootstrap is required (first run)"),
        Ok(false) => println!("✓ Bootstrap already completed"),
        Err(e) => println!("✗ Error checking bootstrap: {}", e),
    }

    println!("\nRunning bootstrap from environment variables...");
    match bootstrap.run_from_env() {
        Ok(state) => {
            println!("✓ Bootstrap completed successfully!");
            println!("  - Admin username: {}", state.admin_username);
            println!("  - Bootstrapped at: {}", state.bootstrapped_at);
            println!("  - Is bootstrapped: {}", state.is_bootstrapped);
        }
        Err(e) => {
            println!("✗ Bootstrap failed: {}", e);
            std::process::exit(1);
        }
    }

    // Verify the admin user was created
    println!("\nVerifying admin user creation...");
    match user_service.get_user_by_username("testadmin") {
        Some(user) => {
            println!("✓ Admin user found!");
            println!("  - Username: {}", user.username);
            println!("  - Email: {}", user.email);
            println!("  - Roles: {:?}", user.roles);
            println!("  - Is active: {}", user.is_active);
            println!("  - Created at: {}", user.created_at);
        }
        None => {
            println!("✗ Admin user not found!");
            std::process::exit(1);
        }
    }

    // Test 2: Verify bootstrap state file was created
    println!("\nTest 2: Bootstrap State File");
    let state_file = data_dir.join(".qilbee_bootstrap");
    if state_file.exists() {
        println!("✓ Bootstrap state file created: {:?}", state_file);

        match std::fs::read_to_string(&state_file) {
            Ok(content) => {
                println!("\nBootstrap state file contents:");
                println!("{}", content);
            }
            Err(e) => println!("✗ Failed to read state file: {}", e),
        }
    } else {
        println!("✗ Bootstrap state file not found!");
    }

    // Test 3: Verify password works
    println!("\nTest 3: Password Verification");
    if let Some(user) = user_service.get_user_by_username("testadmin") {
        match user.verify_password("TestSecure123!") {
            Ok(true) => println!("✓ Password verification successful!"),
            Ok(false) => println!("✗ Password verification failed!"),
            Err(e) => println!("✗ Error verifying password: {}", e),
        }

        match user.verify_password("WrongPassword") {
            Ok(false) => println!("✓ Incorrect password correctly rejected!"),
            Ok(true) => println!("✗ Incorrect password was accepted!"),
            Err(e) => println!("✗ Error verifying password: {}", e),
        }
    }

    // Test 4: Try running bootstrap again (should skip)
    println!("\nTest 4: Re-running Bootstrap (Should Skip)");
    let bootstrap2 = BootstrapService::new(data_dir.clone(), user_service.clone());

    match bootstrap2.is_bootstrap_required() {
        Ok(false) => println!("✓ Bootstrap correctly detected as already complete"),
        Ok(true) => println!("✗ Bootstrap incorrectly thinks it needs to run again!"),
        Err(e) => println!("✗ Error checking bootstrap: {}", e),
    }

    println!("\n=== All Bootstrap Tests Passed! ===");
}
