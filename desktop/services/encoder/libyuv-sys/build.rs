use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=wrapper.h");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let libyuv_dir = out_dir.join("libyuv");

    // libyuvのソースコードをダウンロード（まだ存在しない場合）
    if !libyuv_dir.exists() {
        println!("cargo:warning=Downloading libyuv...");
        let status = Command::new("git")
            .args(&[
                "clone",
                "--depth",
                "1",
                "https://chromium.googlesource.com/libyuv/libyuv",
            ])
            .current_dir(&out_dir)
            .status();

        if status.is_err() || !status.unwrap().success() {
            panic!("Failed to clone libyuv repository. Make sure git is installed.");
        }
    }

    // CMakeでビルド
    // libyuvのCMakeLists.txtでは、ターゲット名は通常"yuv"または"libyuv"
    let mut cmake_config = cmake::Config::new(&libyuv_dir);
    cmake_config
        .define("CMAKE_BUILD_TYPE", "Release")
        .build_target("yuv")
        .very_verbose(true); // デバッグ用に詳細な出力を有効化

    let build_output = cmake_config.build();

    println!(
        "cargo:warning=CMake build output directory: {}",
        build_output.display()
    );

    // 静的ライブラリのパスを設定
    // cmakeクレートはbuild()がビルドディレクトリのパスを返す
    // Windowsの場合、CMakeは通常 build/Release/yuv.lib または build/Debug/yuv.lib に生成する

    #[cfg(target_os = "windows")]
    {
        // 複数の可能なパスを探索
        // Windowsの場合、Visual Studioジェネレーターは通常 build/Release または build/Debug ディレクトリに生成する
        // MinGWジェネレーターの場合は lib ディレクトリに生成する可能性がある

        // ベースパスパターン: ルート、buildサブディレクトリ、libサブディレクトリ、lib/yuvサブディレクトリ
        let base_paths: Vec<Vec<&str>> = vec![
            vec![],             // ルート
            vec!["build"],      // build/
            vec!["lib"],        // lib/
            vec!["lib", "yuv"], // lib/yuv/
        ];

        // ビルドタイプパターン: Release, Debug, MinSizeRel, RelWithDebInfo、またはなし（ルートに直接）
        let build_types: Vec<Option<&str>> = vec![
            Some("Release"),
            Some("Debug"),
            Some("MinSizeRel"),
            Some("RelWithDebInfo"),
            None, // ビルドタイプディレクトリなし（ルートに直接）
        ];

        // ライブラリ名パターン: yuv.lib または libyuv.lib
        let lib_names = vec!["yuv.lib", "libyuv.lib"];

        // パターンを組み合わせて可能なパスを生成
        let mut possible_paths = Vec::new();
        for base_path_parts in &base_paths {
            for build_type in &build_types {
                for lib_name in &lib_names {
                    let mut path = build_output.clone();
                    // ベースパスを追加
                    for part in base_path_parts {
                        path = path.join(part);
                    }
                    // ビルドタイプを追加（ある場合）
                    if let Some(bt) = build_type {
                        path = path.join(bt);
                    }
                    // ライブラリ名を追加
                    path = path.join(lib_name);
                    possible_paths.push(path);
                }
            }
        }

        let mut found_lib = None;
        for lib_path in &possible_paths {
            if lib_path.exists() {
                found_lib = Some(lib_path.parent().unwrap().to_path_buf());
                println!(
                    "cargo:warning=Found libyuv library at: {}",
                    lib_path.display()
                );
                break;
            }
        }

        if let Some(lib_dir) = found_lib {
            println!("cargo:rustc-link-search=native={}", lib_dir.display());
            println!("cargo:rustc-link-lib=static=yuv");
        } else {
            // ライブラリが見つからない場合、一般的なパスを追加
            // 主要なパターンのみをフォールバックとして追加
            let fallback_paths = vec![
                vec!["build", "Release"],
                vec!["build", "Debug"],
                vec!["Release"],
                vec!["Debug"],
                vec!["lib", "Release"],
                vec!["lib", "Debug"],
                vec![], // ルート
            ];

            for path_parts in &fallback_paths {
                let mut path = build_output.clone();
                for part in path_parts {
                    path = path.join(part);
                }
                println!("cargo:rustc-link-search=native={}", path.display());
            }
            println!("cargo:rustc-link-lib=static=yuv");

            // デバッグ情報を出力
            eprintln!("cargo:warning=libyuv library not found in expected locations:");
            for path in &possible_paths {
                eprintln!("cargo:warning=  Checked: {}", path.display());
            }
            eprintln!(
                "cargo:warning=Build output directory: {}",
                build_output.display()
            );

            // ビルドディレクトリの内容をリストアップ
            if build_output.exists() {
                eprintln!("cargo:warning=Contents of build directory:");
                if let Ok(entries) = std::fs::read_dir(&build_output) {
                    for entry in entries.flatten() {
                        eprintln!("cargo:warning=  {}", entry.path().display());
                    }
                }
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        // Unix系システムの場合
        let possible_paths = vec![
            build_output.join("libyuv.a"),
            build_output.join("lib").join("libyuv.a"),
            build_output.join("libyuv").join("libyuv.a"),
        ];

        let mut found_lib = None;
        for lib_path in &possible_paths {
            if lib_path.exists() {
                found_lib = Some(lib_path.parent().unwrap().to_path_buf());
                println!(
                    "cargo:warning=Found libyuv library at: {}",
                    lib_path.display()
                );
                break;
            }
        }

        if let Some(lib_dir) = found_lib {
            println!("cargo:rustc-link-search=native={}", lib_dir.display());
        } else {
            println!("cargo:rustc-link-search=native={}", build_output.display());
            if build_output.join("lib").exists() {
                println!(
                    "cargo:rustc-link-search=native={}",
                    build_output.join("lib").display()
                );
            }
        }
        println!("cargo:rustc-link-lib=static=yuv");
    }

    // bindgenでバインディングを生成
    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .clang_arg(format!("-I{}", libyuv_dir.join("include").display()))
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("Unable to generate bindings");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
