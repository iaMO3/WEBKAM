use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=pocketcam.rc");
    println!("cargo:rerun-if-changed=../assets/pocketcam-app.ico");
    println!("cargo:rerun-if-env-changed=WINDRES");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let (mut compiler, resource) = if env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        let resource = output.join("pocketcam.res");
        let mut compiler = Command::new("rc.exe");
        compiler
            .args(["/nologo", "/fo"])
            .arg(&resource)
            .arg("pocketcam.rc");
        (compiler, resource)
    } else {
        let resource = output.join("pocketcam-icon.o");
        let mut compiler = Command::new(env::var_os("WINDRES").unwrap_or_else(|| "windres".into()));
        compiler
            .args([
                "--input",
                "pocketcam.rc",
                "--output-format=coff",
                "--output",
            ])
            .arg(&resource);
        (compiler, resource)
    };
    let status = compiler
        .status()
        .expect("Start the Windows resource compiler (windres or rc.exe)");
    assert!(
        status.success(),
        "Could not embed the PocketCam executable icon"
    );
    println!("cargo:rustc-link-arg-bin=pocketcam={}", resource.display());
}
