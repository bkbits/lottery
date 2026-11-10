fn main() {
    println!("cargo:rerun-if-changed=ui/app.slint");
    println!("cargo:rerun-if-changed=ui/views.slint");
    println!("cargo:rerun-if-changed=assets/js/cwl.js");
    println!("cargo:rerun-if-changed=assets/js/w500.js");
    slint_kit_build::compile("ui/app.slint").expect("compile application UI");
}
