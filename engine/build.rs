use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn target_tool(name: &str, target: &str, default: &str) -> String {
    let underscored = target.replace('-', "_");
    env::var(format!("{name}_{underscored}"))
        .or_else(|_| env::var(format!("{name}_{target}")))
        .or_else(|_| env::var(name))
        .unwrap_or_else(|_| default.to_owned())
}

fn run(command: &mut Command) {
    let status = command.status().expect("failed to start native tool");
    assert!(status.success(), "native tool failed: {command:?}");
}

fn main() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let vendor = manifest.join("vendor/braids");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let target = env::var("TARGET").unwrap();
    let cross_aarch64 = target == "aarch64-unknown-linux-gnu";
    let cxx_default = if cross_aarch64 {
        "aarch64-linux-gnu-g++"
    } else {
        "c++"
    };
    let ar_default = if cross_aarch64 {
        "aarch64-linux-gnu-ar"
    } else {
        "ar"
    };
    let cxx = target_tool("CXX", &target, cxx_default);
    let ar = target_tool("AR", &target, ar_default);
    let opt_level = env::var("OPT_LEVEL").unwrap_or_else(|_| "0".to_owned());
    let optimization = match opt_level.as_str() {
        "0" => "-O0",
        "1" => "-O1",
        "2" | "3" | "s" | "z" => "-O2",
        _ => "-O2",
    };

    let sources = [
        "braids/analog_oscillator.cc",
        "braids/digital_oscillator.cc",
        "braids/macro_oscillator.cc",
        "braids/resources.cc",
        "stmlib/utils/random.cc",
        "zt_braids_wrapper.cc",
    ];
    for path in [
        "braids/analog_oscillator.h",
        "braids/digital_oscillator.h",
        "braids/excitation.h",
        "braids/macro_oscillator.h",
        "braids/parameter_interpolation.h",
        "braids/resources.h",
        "braids/settings.h",
        "braids/svf.h",
        "stmlib/stmlib.h",
        "stmlib/utils/dsp.h",
        "stmlib/utils/random.h",
        "README.md",
    ] {
        println!("cargo:rerun-if-changed={}", vendor.join(path).display());
    }
    let mut objects = Vec::new();
    for (index, source) in sources.iter().enumerate() {
        let source_path = vendor.join(source);
        println!("cargo:rerun-if-changed={}", source_path.display());
        let object_path = out.join(format!("braids_{index}.o"));
        run(Command::new(&cxx)
            .arg("-std=c++11")
            .arg("-fPIC")
            .arg("-DTEST")
            .arg(optimization)
            .arg("-I")
            .arg(&vendor)
            .arg("-c")
            .arg(&source_path)
            .arg("-o")
            .arg(&object_path));
        objects.push(object_path);
    }

    let archive = out.join("libbraids_macro.a");
    let _ = fs::remove_file(&archive);
    let mut archive_command = Command::new(ar);
    archive_command.arg("crs").arg(&archive);
    for object in &objects {
        archive_command.arg(object);
    }
    run(&mut archive_command);

    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=braids_macro");
    println!("cargo:rustc-link-lib=dylib=stdc++");
    println!(
        "cargo:rerun-if-changed={}",
        vendor.join("stmlib/LICENSE").display()
    );
}
