#![forbid(unsafe_code)]

fn main() {
    std::process::exit(blastit_lsp::run() as i32)
}
