use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=wrapper.h");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let opus_dir = out_dir.join("opus");

    // Opusのソースコードをダウンロード（まだ存在しない場合）
    if !opus_dir.exists() {
        println!("cargo:warning=Downloading Opus...");
        let status = Command::new("git")
            .args(&[
                "clone",
                "--depth",
                "1",
                "--branch",
                "v1.5.2",
                "https://github.com/xiph/opus.git",
            ])
            .current_dir(&out_dir)
            .status();

        if status.is_err() || !status.unwrap().success() {
            panic!("Failed to clone Opus repository. Make sure git is installed.");
        }
    }

    // CMakeでビルド
    let mut cmake_config = cmake::Config::new(&opus_dir);
    cmake_config
        .define("CMAKE_BUILD_TYPE", "Release")
        .define("BUILD_TESTING", "OFF")
        .define("OPUS_BUILD_PROGRAMS", "OFF")
        .define("OPUS_BUILD_SHARED_LIBRARY", "OFF")
        .define("CMAKE_MSVC_RUNTIME_LIBRARY", "MultiThreadedDLL")
        .profile("Release")
        .very_verbose(true);

    let build_output = cmake_config.build();

    println!(
        "cargo:warning=CMake build output directory: {}",
        build_output.display()
    );

    // 静的ライブラリのパスを設定
    #[cfg(target_os = "windows")]
    {
        // 複数の可能なパスを探索（libyuv-sys と同じパターン）
        let base_paths: Vec<Vec<&str>> =
            vec![vec![], vec!["build"], vec!["lib"], vec!["lib", "opus"]];

        let build_types: Vec<Option<&str>> = vec![
            Some("Release"),
            Some("Debug"),
            Some("MinSizeRel"),
            Some("RelWithDebInfo"),
            None,
        ];

        let lib_names = vec!["opus.lib", "libopus.lib"];

        let mut possible_paths = Vec::new();
        for base_path_parts in &base_paths {
            for build_type in &build_types {
                for lib_name in &lib_names {
                    let mut path = build_output.clone();
                    for part in base_path_parts {
                        path = path.join(part);
                    }
                    if let Some(bt) = build_type {
                        path = path.join(bt);
                    }
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
                    "cargo:warning=Found Opus library at: {}",
                    lib_path.display()
                );
                break;
            }
        }

        if let Some(lib_dir) = found_lib {
            println!("cargo:rustc-link-search=native={}", lib_dir.display());
            println!("cargo:rustc-link-lib=static=opus");
        } else {
            let fallback_paths = vec![
                vec!["build", "Release"],
                vec!["build", "Debug"],
                vec!["Release"],
                vec!["Debug"],
                vec!["lib", "Release"],
                vec!["lib", "Debug"],
                vec![],
            ];

            for path_parts in &fallback_paths {
                let mut path = build_output.clone();
                for part in path_parts {
                    path = path.join(part);
                }
                println!("cargo:rustc-link-search=native={}", path.display());
            }
            println!("cargo:rustc-link-lib=static=opus");

            eprintln!("cargo:warning=Opus library not found in expected locations:");
            for path in &possible_paths {
                eprintln!("cargo:warning=  Checked: {}", path.display());
            }
            eprintln!(
                "cargo:warning=Build output directory: {}",
                build_output.display()
            );

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
        let possible_paths = vec![
            build_output.join("libopus.a"),
            build_output.join("lib").join("libopus.a"),
            build_output.join("opus").join("libopus.a"),
        ];

        let mut found_lib = None;
        for lib_path in &possible_paths {
            if lib_path.exists() {
                found_lib = Some(lib_path.parent().unwrap().to_path_buf());
                println!(
                    "cargo:warning=Found Opus library at: {}",
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
        println!("cargo:rustc-link-lib=static=opus");
    }

    // bindgenでバインディングを生成
    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .clang_arg(format!("-I{}", opus_dir.join("include").display()))
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .allowlist_function("opus_encoder_.*")
        .allowlist_function("opus_decoder_.*")
        .allowlist_function("opus_.*")
        .allowlist_type("OpusEncoder")
        .allowlist_type("OpusDecoder")
        .allowlist_var("OPUS_.*")
        .generate()
        .expect("Unable to generate bindings");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
