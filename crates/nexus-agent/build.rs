fn main() {
    // `include_dir!` embeds these folders; rebuild when a TOML or prompt changes.
    println!("cargo:rerun-if-changed=../../assistants");
    println!("cargo:rerun-if-changed=../../prompts/internal");
    println!("cargo:rerun-if-changed=../../schemas");
}
