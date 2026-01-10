use dctx_core::ignore;
use dctx_core::llm_formatter;
use dctx_core::version::Version;
use dctx_core::walker;
use dctx_core::writer;
use std::env;
use std::time::Instant;

fn main() {
    let start_time = Instant::now();

    // 1. Setup paths
    let current_dir = env::current_dir().expect("Failed to get current directory");
    let project_name = current_dir
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();

    println!("🚀 dctx running on: {}", current_dir.display());

    // 2. Load Ignore Rules (L1 + L2 + L3)
    let rules = ignore::load_ignore_rules(&current_dir);
    println!("ℹ️  Loaded {} ignore rules.", rules.len());

    // 3. Walk Structure (Metadata only)
    println!("📂 Scanning directory structure...");
    let mut root = match walker::walk_directory_structure(&current_dir, &rules) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("❌ Error walking directory: {}", e);
            return;
        }
    };

    // 4. Populate Content (Parallel)
    println!("⚡ Reading file contents (Multi-threaded)...");
    walker::populate_file_contents_parallel(&mut root);

    // 5. Write Binary State (Locally for now, until Registry is ready)
    let bin_filename = format!("{}.dctx", project_name);
    let bin_path = current_dir.join(&bin_filename);

    match writer::write_dircontxt_file(&bin_path, &mut root) {
        Ok(_) => println!("✅ Binary state saved to: {}", bin_filename),
        Err(e) => eprintln!("❌ Failed to write binary: {}", e),
    }

    // 6. Write LLM Context (using the baked-in Template)
    let txt_filename = format!("{}.llmcontext.txt", project_name);
    let txt_path = current_dir.join(&txt_filename);

    // Mock versioning for now (V1)
    let version = Version::new_v1();

    let mut file = std::fs::File::create(&txt_path).expect("Failed to create txt file");
    match llm_formatter::generate_llm_context(&mut file, &root, &version, &project_name) {
        Ok(_) => println!("✅ LLM Context saved to: {}", txt_filename),
        Err(e) => eprintln!("❌ Failed to write text output: {}", e),
    }

    let duration = start_time.elapsed();
    println!("✨ Done in {:.2?}", duration);
}
