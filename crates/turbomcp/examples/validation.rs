//! # TurboMCP Validation - Comprehensive Demo
//!
//! This example demonstrates all validation approaches supported by TurboMCP.
//! Choose your approach via command-line flags.
//!
//! ## Usage
//!
//! ```bash
//! # Run all demonstrations
//! cargo run --example validation
//!
//! # Run specific approach
//! cargo run --example validation -- --approach newtype
//! cargo run --example validation -- --approach garde
//! cargo run --example validation -- --approach validator
//! cargo run --example validation -- --approach nutype
//!
//! # Show decision tree and comparison
//! cargo run --example validation -- --compare
//! ```

use std::env;

// Import validation patterns
use turbomcp::validation::patterns::{Email, NonEmpty, Url};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse command line arguments
    let args: Vec<String> = env::args().collect();

    let mode = if args.len() > 1 {
        match args[1].as_str() {
            "--approach" => {
                if args.len() > 2 {
                    args[2].as_str()
                } else {
                    print_usage();
                    return Ok(());
                }
            }
            "--compare" => "compare",
            "--help" | "-h" => {
                print_usage();
                return Ok(());
            }
            other => {
                println!("Unknown option: {}", other);
                print_usage();
                return Ok(());
            }
        }
    } else {
        "all"
    };

    match mode {
        "newtype" => demo_newtype(),
        "garde" => demo_garde(),
        "validator" => demo_validator(),
        "nutype" => demo_nutype(),
        "compare" => show_comparison(),
        "all" => run_all_demos(),
        _ => {
            println!("Unknown approach: {}", mode);
            print_usage();
        }
    }

    Ok(())
}

fn print_usage() {
    println!("TurboMCP Validation Examples\n");
    println!("USAGE:");
    println!("    cargo run --example validation [OPTIONS]\n");
    println!("OPTIONS:");
    println!("    --approach <TYPE>    Run specific validation approach");
    println!("                         Types: newtype, garde, validator, nutype");
    println!("    --compare            Show comparison and decision tree");
    println!("    --help, -h           Show this help message\n");
    println!("EXAMPLES:");
    println!("    cargo run --example validation");
    println!("    cargo run --example validation -- --approach newtype");
    println!("    cargo run --example validation -- --compare");
}

fn run_all_demos() {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║  TurboMCP Validation: Complete Demo Suite                   ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    println!("Running all validation approach demonstrations...\n");

    demo_newtype();
    println!("\n{}\n", "─".repeat(64));

    demo_garde();
    println!("\n{}\n", "─".repeat(64));

    demo_validator();
    println!("\n{}\n", "─".repeat(64));

    demo_nutype();
    println!("\n{}\n", "─".repeat(64));

    show_comparison();
}

// ============================================================================
// Approach 1: Manual Newtypes (Zero Dependencies)
// ============================================================================

fn demo_newtype() {
    println!("┌────────────────────────────────────────────────────────────┐");
    println!("│ Approach 1: Manual Newtypes (Zero Dependencies)           │");
    println!("└────────────────────────────────────────────────────────────┘\n");

    println!("✅ Using built-in patterns from turbomcp::validation::patterns");
    println!("   • Email, NonEmpty, Url, Bounded<T>");
    println!("   • Zero external dependencies");
    println!("   • Type-safe validation during deserialization\n");

    // Valid cases
    println!("Valid Examples:");
    match Email::new("user@example.com") {
        Ok(email) => println!("  ✓ Email: {}", email.as_str()),
        Err(e) => println!("  ✗ Email error: {}", e),
    }

    match NonEmpty::new("johndoe") {
        Ok(name) => println!("  ✓ Username: {}", name.as_str()),
        Err(e) => println!("  ✗ Username error: {}", e),
    }

    match Url::new("https://example.com") {
        Ok(url) => println!("  ✓ URL: {}", url.as_str()),
        Err(e) => println!("  ✗ URL error: {}", e),
    }

    // Invalid cases
    println!("\nInvalid Examples:");
    match Email::new("not-an-email") {
        Ok(email) => println!("  ✓ Email: {}", email.as_str()),
        Err(e) => println!("  ✗ Email error: {}", e),
    }

    match NonEmpty::new("   ") {
        Ok(name) => println!("  ✓ Username: {}", name.as_str()),
        Err(e) => println!("  ✗ Username error: {}", e),
    }

    println!("\n📝 Code Example:");
    println!("```rust");
    println!("use turbomcp::validation::patterns::Email;");
    println!();
    println!("#[tool(\"Register user\")]");
    println!("async fn register(&self, email: Email) -> McpResult<String> {{");
    println!("    // Email is guaranteed valid by type system!");
    println!("    Ok(format!(\"Registered: {{}}\", email.as_str()))");
    println!("}}");
    println!("```\n");

    println!("✅ Best for: Simple rules, zero deps, maximum control");
}

// ============================================================================
// Approach 2: garde (Modern Runtime Validation)
// ============================================================================

fn demo_garde() {
    println!("┌────────────────────────────────────────────────────────────┐");
    println!("│ Approach 2: garde (Modern Runtime Validation)             │");
    println!("└────────────────────────────────────────────────────────────┘\n");

    println!("Modern runtime validation with excellent error messages");
    println!("Add to Cargo.toml: garde = {{ version = \"0.20\", features = [\"derive\"] }}\n");

    println!("📝 Code Example:");
    println!("```rust");
    println!("use garde::Validate;");
    println!();
    println!("#[derive(Deserialize, Validate)]");
    println!("struct RegisterParams {{");
    println!("    #[garde(email)]");
    println!("    email: String,");
    println!();
    println!("    #[garde(length(min = 8))]");
    println!("    #[garde(custom(check_password_strength))]");
    println!("    password: String,");
    println!("}}");
    println!();
    println!("#[tool(\"register\")]");
    println!("async fn register(&self, params: Value) -> McpResult<String> {{");
    println!("    let params: RegisterParams = serde_json::from_value(params)?;");
    println!("    params.validate(&())?;  // Context-aware validation");
    println!("    Ok(format!(\"Registered: {{}}\", params.email))");
    println!("}}");
    println!("```\n");

    println!("✅ Best for: Complex validation, nested structs, context-aware rules");
}

// ============================================================================
// Approach 3: validator (Mature Ecosystem)
// ============================================================================

fn demo_validator() {
    println!("┌────────────────────────────────────────────────────────────┐");
    println!("│ Approach 3: validator (Mature Ecosystem)                  │");
    println!("└────────────────────────────────────────────────────────────┘\n");

    println!("Most mature validation library (6+ years, 1.4M+ downloads/month)");
    println!("Add to Cargo.toml: validator = {{ version = \"0.18\", features = [\"derive\"] }}\n");

    println!("📝 Code Example:");
    println!("```rust");
    println!("use validator::Validate;");
    println!();
    println!("#[derive(Deserialize, Validate)]");
    println!("struct RegisterParams {{");
    println!("    #[validate(email)]");
    println!("    email: String,");
    println!();
    println!("    #[validate(credit_card)]");
    println!("    card: String,");
    println!();
    println!("    #[validate(phone)]");
    println!("    phone: String,");
    println!("}}");
    println!();
    println!("#[tool(\"register\")]");
    println!("async fn register(&self, params: Value) -> McpResult<String> {{");
    println!("    let params: RegisterParams = serde_json::from_value(params)?;");
    println!("    params.validate()?;");
    println!("    Ok(format!(\"Registered: {{}}\", params.email))");
    println!("}}");
    println!("```\n");

    println!("✅ Best for: Ecosystem compatibility, rich built-ins (credit card, phone)");
}

// ============================================================================
// Approach 4: nutype (Type-Level Guarantees)
// ============================================================================

fn demo_nutype() {
    println!("┌────────────────────────────────────────────────────────────┐");
    println!("│ Approach 4: nutype (Type-Level Guarantees)                │");
    println!("└────────────────────────────────────────────────────────────┘\n");

    println!("Strongest type safety with compile-time guarantees");
    println!("Add to Cargo.toml: nutype = {{ version = \"0.5\", features = [\"serde\"] }}\n");

    println!("📝 Code Example:");
    println!("```rust");
    println!("use nutype::nutype;");
    println!();
    println!("#[nutype(");
    println!("    sanitize(trim, lowercase),");
    println!("    validate(email),");
    println!("    derive(Debug, Clone, Serialize, Deserialize)");
    println!(")]");
    println!("pub struct Email(String);");
    println!();
    println!("#[tool(\"register\")]");
    println!("async fn register(&self, email: Email) -> McpResult<String> {{");
    println!("    // Email is GUARANTEED valid - impossible to construct invalid!");
    println!("    Ok(format!(\"Registered: {{}}\", email.into_inner()))");
    println!("}}");
    println!("```\n");

    println!("✅ Best for: Domain-Driven Design, impossible-to-misuse APIs");
}

// ============================================================================
// Comparison and Decision Tree
// ============================================================================

fn show_comparison() {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║  Validation Approaches: Comparison & Decision Tree          ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    // Comparison Matrix
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│ COMPARISON MATRIX                                           │");
    println!("└─────────────────────────────────────────────────────────────┘\n");

    println!("╭────────────────┬──────────┬────────────┬────────────┬──────────╮");
    println!("│ Feature        │ Manual   │ garde      │ validator  │ nutype   │");
    println!("├────────────────┼──────────┼────────────┼────────────┼──────────┤");
    println!("│ Dependencies   │ 0        │ 1          │ 1          │ 1        │");
    println!("│ Type Safety    │ ★★★★★    │ ★★★☆☆      │ ★★★☆☆      │ ★★★★★    │");
    println!("│ Error Messages │ Custom   │ ★★★★★      │ ★★★☆☆      │ ★★★★☆    │");
    println!("│ Flexibility    │ ★★★★★    │ ★★★★★      │ ★★★★☆      │ ★★★☆☆    │");
    println!("│ Compile Time   │ Fast     │ Fast       │ Fast       │ Slower   │");
    println!("│ Ecosystem      │ N/A      │ Growing    │ Largest    │ Small    │");
    println!("│ Maturity       │ Pattern  │ Modern     │ 6+ years   │ 2+ years │");
    println!("╰────────────────┴──────────┴────────────┴────────────┴──────────╯\n");

    // Decision Tree
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│ DECISION TREE                                               │");
    println!("└─────────────────────────────────────────────────────────────┘\n");

    println!("Need validation?");
    println!("│");
    println!("├─ Want zero dependencies?");
    println!("│  └─► Manual Newtypes (turbomcp::validation::patterns)");
    println!("│      ✓ Maximum control");
    println!("│      ✓ Fast compilation");
    println!("│");
    println!("├─ Need STRONGEST type safety?");
    println!("│  └─► nutype");
    println!("│      ✓ Impossible to construct invalid values");
    println!("│      ✓ Perfect for Domain-Driven Design");
    println!("│");
    println!("├─ Complex validation (nested, context-aware)?");
    println!("│  ├─ Starting new project (2025)?");
    println!("│  │  └─► garde");
    println!("│  │      ✓ Modern API design");
    println!("│  │      ✓ Best error messages");
    println!("│  │");
    println!("│  └─ Existing project or ecosystem?");
    println!("│     └─► validator");
    println!("│         ✓ Most mature (6+ years)");
    println!("│         ✓ Largest ecosystem");
    println!("│");
    println!("└─ Maximum control?");
    println!("   └─► Manual Newtypes");
    println!("       ✓ Use ANY validation library internally\n");

    // When to use each
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│ WHEN TO USE EACH APPROACH                                   │");
    println!("└─────────────────────────────────────────────────────────────┘\n");

    println!("✅ Manual Newtypes:");
    println!("   • Simple validation rules (email, non-empty, range)");
    println!("   • Want zero external dependencies");
    println!("   • Need maximum flexibility and control\n");

    println!("✅ garde (Modern):");
    println!("   • Starting new project in 2025");
    println!("   • Need excellent error messages");
    println!("   • Complex nested validation\n");

    println!("✅ validator (Mature):");
    println!("   • Existing project using validator");
    println!("   • Need ecosystem compatibility");
    println!("   • Rich built-in validators needed\n");

    println!("✅ nutype (Type-Level):");
    println!("   • Domain-Driven Design projects");
    println!("   • Want impossible-to-misuse APIs");
    println!("   • Simple validation rules\n");

    // Mix and match
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│ 💡 PRO TIP: Mix and Match!                                  │");
    println!("└─────────────────────────────────────────────────────────────┘\n");

    println!("You can combine approaches:");
    println!("  • nutype for core domain types (Email, UserId, Price)");
    println!("  • garde/validator for complex request validation");
    println!("  • Manual newtypes for custom business rules\n");

    println!("Example:");
    println!("```rust");
    println!("#[derive(Validate)]");
    println!("struct RegisterParams {{");
    println!("    email: EmailAddress,        // nutype (type-safe)");
    println!("    #[garde(length(min = 8))]");
    println!("    password: String,           // garde (complex rules)");
    println!("    code: ReferralCode,         // manual (custom logic)");
    println!("}}");
    println!("```\n");

    println!("📚 For more details, see VALIDATION_GUIDE.md");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_email_validation() {
        assert!(Email::new("user@example.com").is_ok());
        assert!(Email::new("invalid").is_err());
    }

    #[test]
    fn test_non_empty_validation() {
        assert!(NonEmpty::new("hello").is_ok());
        assert!(NonEmpty::new("   ").is_err());
    }

    #[test]
    fn test_url_validation() {
        assert!(Url::new("https://example.com").is_ok());
        assert!(Url::new("not a url").is_err());
    }
}
