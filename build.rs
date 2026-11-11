fn main() {
    println!("cargo:rerun-if-changed=ui/app.slint");
    println!("cargo:rerun-if-changed=ui/views.slint");
    println!("cargo:rerun-if-changed=assets/js/cwl.js");
    println!("cargo:rerun-if-changed=assets/js/w500.js");
    println!("cargo:rerun-if-changed=assets/fonts/AlibabaPuHuiTi-3-55-Regular.ttf");
    println!("cargo:rerun-if-changed=assets/icon/app-icon-256.png");
    // 交叉编译到 Android 时也由 build 脚本执行，但只为 Windows 目标嵌入 exe 图标。
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=assets/icon/app.rc");
        println!("cargo:rerun-if-changed=assets/icon/app-icon.ico");
        let _ = embed_resource::compile("assets/icon/app.rc", embed_resource::NONE);
    }
    slint_kit_build::compile("ui/app.slint").expect("compile application UI");
}
