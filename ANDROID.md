# Android 构建与签名

当前打包方式为 cargo-apk 0.10.0 + Slint 1.18.1 NativeActivity；仅构建 `arm64-v8a`，最低 Android API 24，目标 API 35。应用包名 `dev.lkq.lottery`，显示名称“双色球分析”。

## 当前机器环境

以下命令在项目根目录的 PowerShell 中执行：

```powershell
$env:ANDROID_HOME = 'C:\Users\dell\AppData\Local\Android\Sdk'
$env:ANDROID_NDK_ROOT = 'C:\Users\dell\AppData\Local\Android\Sdk\ndk\27.3.13750724'
$env:JAVA_HOME = 'C:\java\dragonwell21'
$env:Path = "$env:JAVA_HOME\bin;C:\Users\dell\.cargo\bin;$env:Path"
```

SDK 需包含 `platforms/android-35` 和 `build-tools/35.0.0`。Rust 需安装 `aarch64-linux-android`，并安装 `cargo-apk`。不要将 Windows 的 linker 配置用于 Android；cargo-apk 会配置 NDK 工具链。

只检查 Android Rust 代码，不生成 APK：

```powershell
cargo apk check --lib --target aarch64-linux-android
```

`--lib` 必须保留：Android 入口位于 `lottery_core` 动态库，不使用桌面二进制入口。

## 本地分发的 release APK

cargo-apk 不会为 release 自动生成证书。当前机器已存在 `C:\Users\dell\.android\debug.keystore`，包含 `androiddebugkey`，密码为 Android 默认调试密码。以下命令生成 release 优化、调试证书签名的 APK，可用于本地安装与文件传输：

```powershell
$env:CARGO_APK_RELEASE_KEYSTORE = 'C:\Users\dell\.android\debug.keystore'
$env:CARGO_APK_RELEASE_KEYSTORE_PASSWORD = 'android'
cargo apk build --release --lib --target aarch64-linux-android
if ($LASTEXITCODE -ne 0) { throw 'APK 构建失败' }
& 'C:\Users\dell\AppData\Local\Android\Sdk\build-tools\35.0.0\apksigner.bat' verify --verbose --print-certs 'target\release\apk\lottery.apk'
if ($LASTEXITCODE -ne 0) { throw 'APK 签名验证失败' }
& 'C:\Users\dell\AppData\Local\Android\Sdk\build-tools\35.0.0\aapt.exe' dump badging 'target\release\apk\lottery.apk'
```

最终文件为 `target/release/apk/lottery.apk`。`aapt dump badging` 应显示包名、中文 label、`android.permission.INTERNET` 和 `arm64-v8a`。

调试证书 SHA-256（2026-11-10 本机验证）：

```text
E0:16:BB:14:25:F8:9D:3B:E5:CB:35:B0:BB:41:FE:DB:0C:F5:36:59:A3:FC:F6:CB:EB:F2:54:E2:B6:64:7F:80
```

这不是正式发布证书。正式发布时，将上述两个 `CARGO_APK_RELEASE_*` 环境变量改为受控签名库的绝对路径与密码；不要将私钥或密码写进 Cargo.toml 或版本库。cargo-apk 0.10.0 不显式选择 alias，签名库应只保留一个签名条目，私钥密码与库密码一致。更新已有安装必须使用相同包名与证书，否则需先卸载；卸载会删除应用私有数据。

## 运行与验证

- 数据库使用 `AndroidApp::internal_data_path()` 返回的应用私有目录下的 `lottery/lottery.db`，保留 `LOTTERY_DB` 覆盖能力；无需外部存储权限。路径在 `android_main` 中、打开数据库前初始化，可跨线程读取。
- Android 复制复用 Slint Android 后端，通过严格锁定 `=1.18.1` 的 `i-slint-backend-selector` 访问系统剪贴板。升级 Slint 时必须同步升级该依赖。Slint 将底层 JNI 剪贴板异常写入日志，不向调用方返回错误；设备验收需实际粘贴检查。
- 清单仅新增联网权限，不申请存储或剪贴板权限。
- 首次加载应能联网获取历史数据；生成号码后测试单组、全部复制；关闭并重新启动应用，检查历史数据和预测配置仍在。
- Android 目标启用 Boa `jsvalue-enum`，兼容带指针标记的 ARM64 实机；否则首次执行数据脚本会触发 NaN boxing panic。
- 字体使用内嵌阿里巴巴普惠体 3.0 Regular，完整字库经 Slint 编译进入原生库。统一设置 `default-font-family`，避免依赖 Android 系统中文回退。字体来源、版权与校验值见 `assets/fonts/README.md`。
- Android 使用 `Theme.Material.Light.NoActionBar`，避免系统标题栏覆盖应用顶部。
- 已在型号 `24129RT7CC`、Android 16/API 36 真机上通过 ADB 安装并启动 release APK，联网载入 3526 期，重启后数据保留；验证预测 5 注、全部复制反馈及输入法剪贴板预览、历史 13 指标展开和滑动续载、深浅主题、算法弹窗、数字软键盘避让。应用进程日志未见 panic/JNI 错误。
- 修复字体后，用本地 OCR 对真机三页与弹窗截图验证中文恢复；已检查 APK 内原生库包含完整字体原始字节，签名 v2/v3 验证通过。未覆盖其它机型和全部算法组合。

已安装依赖的 API 核验来源：Slint 1.18.1 `android.rs`、`i-slint-backend-selector` 的 `with_platform`、Android 后端的 `set_clipboard_text`；android-activity 0.6.1 `AndroidApp::internal_data_path`；cargo-apk 0.10.0 README 与 `src/apk.rs` 的签名环境变量处理。
