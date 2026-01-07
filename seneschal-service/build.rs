fn main() {
    // No build-time setup needed for dynamic linking.
    // PDFium is loaded at runtime from:
    // 1. Current directory
    // 2. vendor/pdfium/lib/ (run `just download-pdfium`)
    // 3. System library paths
    println!("cargo:rerun-if-changed=build.rs");
}
