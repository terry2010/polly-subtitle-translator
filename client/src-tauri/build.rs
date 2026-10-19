fn main() {
    tauri_build::build();

    // speech crate（macOS）把 Swift 桥接静态链接进可执行产物，运行时需要 libswift_*.dylib。
    // 它自己的 build.rs 写的 `-rpath /usr/lib/swift` 只对"当前包"的 bin/test 生效，
    // 依赖包写的一律被 Cargo 忽略——由本 crate 补上，否则启动即
    // `dyld: Library not loaded: @rpath/libswift_Concurrency.dylib`（SIGABRT）。
    // /usr/lib/swift 是 macOS 13+ 自带 Swift 运行时的位置。
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_OS");
}
